use crate::linux::{execute_allowlisted, list_processes, read_confined_file, system_info};
use crate::{validate_relative_path, Request, Response, ServerConfig, MAX_RESPONSE_BYTES, MAX_REQUEST_BYTES};
use serde_json::{json, Value};
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::thread;

fn peer_uid(stream: &UnixStream) -> Result<u32, String> {
    #[repr(C)]
    struct Ucred {
        pid: libc::pid_t,
        uid: libc::uid_t,
        gid: libc::gid_t,
    }
    let mut cred = Ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<Ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut Ucred as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(format!("SO_PEERCRED failed: {}", std::io::Error::last_os_error()));
    }
    if len < std::mem::size_of::<Ucred>() as libc::socklen_t {
        return Err("SO_PEERCRED returned a truncated credential".to_string());
    }
    Ok(cred.uid as u32)
}

fn read_request(stream: &mut UnixStream) -> Result<Vec<u8>, String> {
    let mut data = Vec::with_capacity(4096);
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(1) => {
                if byte[0] == b'\n' {
                    break;
                }
                data.push(byte[0]);
                if data.len() > MAX_REQUEST_BYTES {
                    return Err("request exceeds maximum size".to_string());
                }
            }
            Ok(_) => unreachable!(),
            Err(error) => return Err(format!("read request: {error}")),
        }
    }
    if data.is_empty() {
        return Err("empty request".to_string());
    }
    Ok(data)
}

fn write_response(stream: &mut UnixStream, response: &Response) -> io::Result<()> {
    let bytes = response
        .encode_line()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "response exceeds maximum size"));
    }
    stream.write_all(&bytes)
}

fn arg_object<'a>(args: &'a Value) -> Result<&'a serde_json::Map<String, Value>, String> {
    args.as_object().ok_or_else(|| "args must be an object".to_string())
}

fn dispatch(config: &ServerConfig, request: &Request) -> Result<Value, (String, String)> {
    let args = arg_object(&request.args).map_err(|e| ("invalid_argument".to_string(), e))?;
    match request.op.as_str() {
        "system.info" => Ok(system_info()),
        "process.list" => {
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(32);
            list_processes(limit as usize).map_err(|e| ("internal".to_string(), e))
        }
        "file.read" => {
            let path = args.get("path").and_then(Value::as_str).ok_or_else(|| {
                ("invalid_argument".to_string(), "path is required".to_string())
            })?;
            let max_bytes = args.get("max_bytes").and_then(Value::as_u64).unwrap_or(64 * 1024);
            if max_bytes == 0 || max_bytes as usize > crate::MAX_OUTPUT_BYTES {
                return Err(("invalid_argument".to_string(), "max_bytes is outside the allowed range".to_string()));
            }
            let relative = validate_relative_path(path).map_err(|e| ("invalid_argument".to_string(), e.to_string()))?;
            let bytes = read_confined_file(&config.root, relative.to_string_lossy().as_ref(), max_bytes as usize)
                .map_err(|e| ("denied".to_string(), e))?;
            Ok(json!({
                "encoding": "utf-8",
                "data": String::from_utf8_lossy(&bytes),
                "bytes": bytes.len(),
            }))
        }
        "process.exec" => {
            if !config.allow_exec {
                return Err(("denied".to_string(), "process execution is disabled by policy".to_string()));
            }
            let executable = args.get("executable").and_then(Value::as_str).ok_or_else(|| {
                ("invalid_argument".to_string(), "executable is required".to_string())
            })?;
            let argv = args.get("argv").and_then(Value::as_array).ok_or_else(|| {
                ("invalid_argument".to_string(), "argv is required".to_string())
            })?;
            if argv.len() > crate::MAX_ARGS {
                return Err(("invalid_argument".to_string(), "too many arguments".to_string()));
            }
            let mut parsed = Vec::with_capacity(argv.len());
            for item in argv {
                let value = item.as_str().ok_or_else(|| {
                    ("invalid_argument".to_string(), "argv entries must be strings".to_string())
                })?;
                parsed.push(value.to_string());
            }
            let cwd = args.get("cwd").and_then(Value::as_str).unwrap_or(".");
            let cwd_path = config.root.join(
                validate_relative_path(cwd).map_err(|e| ("invalid_argument".to_string(), e.to_string()))?,
            );
            execute_allowlisted(
                Path::new(executable),
                &parsed,
                &cwd_path,
                &config.command_allowlist,
                config.exec_timeout,
                config.max_output_bytes.min(MAX_OUTPUT_BYTES),
                config.allow_root_exec,
            )
            .map_err(|e| ("denied".to_string(), e))
        }
        _ => Err(("unknown_operation".to_string(), "operation is not available".to_string())),
    }
}

fn handle_connection(mut stream: UnixStream, config: ServerConfig) {
    let id = match peer_uid(&stream) {
        Ok(uid) if uid == config.allowed_uid => None,
        Ok(_) => {
            let _ = write_response(&mut stream, &Response::error("unknown".to_string(), "denied", "peer credentials are not authorized"));
            return;
        }
        Err(_) => {
            return;
        }
    };

    let raw = match read_request(&mut stream) {
        Ok(value) => value,
        Err(error) => {
            let _ = write_response(&mut stream, &Response::error(id.unwrap_or_else(|| "unknown".to_string()), "invalid_request", error));
            return;
        }
    };
    let request = match Request::parse(&raw) {
        Ok(value) => value,
        Err(error) => {
            let _ = write_response(&mut stream, &Response::error("unknown".to_string(), "invalid_request", error.to_string()));
            return;
        }
    };
    let response = match dispatch(&config, &request) {
        Ok(value) => Response::success(request.id.clone(), value),
        Err((code, message)) => Response::error(request.id.clone(), code, message),
    };
    let _ = write_response(&mut stream, &response);
}

pub fn run(config: ServerConfig) -> Result<(), String> {
    config.validate().map_err(|e| e.to_string())?;
    if !config.root.exists() {
        fs::create_dir_all(&config.root).map_err(|e| format!("create system root: {e}"))?;
    }
    if config.root.is_symlink() || !config.root.is_dir() {
        return Err("system root must be a real directory".to_string());
    }

    if config.socket_path.exists() {
        if config.socket_path.is_symlink() || !config.socket_path.is_file() {
            return Err("refusing to replace non-regular stale socket path".to_string());
        }
        fs::remove_file(&config.socket_path).map_err(|e| format!("remove stale socket: {e}"))?;
    }
    if let Some(parent) = config.socket_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create socket parent: {e}"))?;
    }

    let listener = UnixListener::bind(&config.socket_path).map_err(|e| format!("bind socket: {e}"))?;
    fs::set_permissions(&config.socket_path, fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("set socket permissions: {e}"))?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let worker_config = config.clone();
                thread::Builder::new()
                    .name("abiyss-system-client".to_string())
                    .spawn(move || handle_connection(stream, worker_config))
                    .map_err(|e| format!("spawn client worker: {e}"))?;
            }
            Err(error) => {
                eprintln!("abiyss-systemd: accept error: {error}");
            }
        }
    }
    Ok(())
}
