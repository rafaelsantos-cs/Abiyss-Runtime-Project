use crate::linux::{execute_allowlisted, list_processes, read_confined_file, system_info};
use crate::{validate_relative_path, Request, Response, ServerConfig, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES};
use serde_json::{json, Value};
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
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

fn validate_keys(args: &serde_json::Map<String, Value>, allowed: &[&str]) -> Result<(), (String, String)> {
    if args.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err((
            "invalid_argument".to_string(),
            "request contains unknown argument fields".to_string(),
        ));
    }
    Ok(())
}

fn optional_u64(args: &serde_json::Map<String, Value>, key: &str, default: u64) -> Result<u64, (String, String)> {
    match args.get(key) {
        None => Ok(default),
        Some(value) => value.as_u64().ok_or_else(|| {
            ("invalid_argument".to_string(), format!("{key} must be a non-negative integer"))
        }),
    }
}

fn optional_string<'a>(args: &'a serde_json::Map<String, Value>, key: &str, default: &'a str) -> Result<&'a str, (String, String)> {
    match args.get(key) {
        None => Ok(default),
        Some(value) => value.as_str().ok_or_else(|| {
            ("invalid_argument".to_string(), format!("{key} must be a string"))
        }),
    }
}

fn dispatch(config: &ServerConfig, request: &Request) -> Result<Value, (String, String)> {
    let args = arg_object(&request.args).map_err(|e| ("invalid_argument".to_string(), e))?;
    match request.op.as_str() {
        "system.info" => {
            validate_keys(args, &[])?;
            Ok(system_info())
        }
        "process.list" => {
            validate_keys(args, &["limit"])?;
            let limit = optional_u64(args, "limit", 32)?;
            if !(1..=128).contains(&limit) {
                return Err(("invalid_argument".to_string(), "limit is outside the allowed range".to_string()));
            }
            list_processes(limit as usize).map_err(|e| ("internal".to_string(), e))
        }
        "file.read" => {
            validate_keys(args, &["path", "max_bytes"])?;
            let path = args.get("path").and_then(Value::as_str).ok_or_else(|| {
                ("invalid_argument".to_string(), "path is required and must be a string".to_string())
            })?;
            let max_bytes = optional_u64(args, "max_bytes", 64 * 1024)?;
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
            validate_keys(args, &["executable", "argv", "cwd"])?;
            if !config.allow_exec {
                return Err(("denied".to_string(), "process execution is disabled by policy".to_string()));
            }
            let executable = args.get("executable").and_then(Value::as_str).ok_or_else(|| {
                ("invalid_argument".to_string(), "executable is required and must be a string".to_string())
            })?;
            let argv = args.get("argv").and_then(Value::as_array).ok_or_else(|| {
                ("invalid_argument".to_string(), "argv is required and must be an array".to_string())
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
            let cwd = optional_string(args, "cwd", ".")?;
            let cwd_path = config.root.join(
                validate_relative_path(cwd).map_err(|e| ("invalid_argument".to_string(), e.to_string()))?,
            );
            execute_allowlisted(
                &config.root,
                Path::new(executable),
                &parsed,
                &cwd_path,
                &config.command_allowlist,
                config.exec_timeout,
                config.max_output_bytes.min(crate::MAX_OUTPUT_BYTES),
                config.allow_root_exec,
            )
            .map_err(|e| ("denied".to_string(), e))
        }
        _ => Err(("unknown_operation".to_string(), "operation is not available".to_string())),
    }
}

fn handle_connection(mut stream: UnixStream, config: ServerConfig) {
    if !peer_uid(&stream).map(|uid| uid == config.allowed_uid).unwrap_or(false) {
        let _ = write_response(&mut stream, &Response::error("unknown".to_string(), "denied", "peer credentials are not authorized"));
        return;
    }

    let raw = match read_request(&mut stream) {
        Ok(value) => value,
        Err(error) => {
            let _ = write_response(&mut stream, &Response::error("unknown".to_string(), "invalid_request", error));
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

    if config.socket_path.exists() || fs::symlink_metadata(&config.socket_path).is_ok() {
        let metadata = fs::symlink_metadata(&config.socket_path)
            .map_err(|e| format!("inspect stale socket path: {e}"))?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err("refusing to replace symlink at socket path".to_string());
        }
        if !file_type.is_socket() {
            return Err("refusing to replace non-socket path".to_string());
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
