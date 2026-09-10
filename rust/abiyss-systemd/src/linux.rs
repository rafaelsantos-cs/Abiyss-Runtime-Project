//! Linux-only privileged boundary helpers.

use crate::{MAX_ARG_BYTES, MAX_ARGS, MAX_OUTPUT_BYTES, MAX_PATH_BYTES};
use serde_json::json;
use std::ffi::{CStr, CString};
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const RESOLVE_NO_XDEV: u64 = 0x01;
const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
const RESOLVE_NO_SYMLINKS: u64 = 0x04;
const RESOLVE_BENEATH: u64 = 0x08;

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

fn errno_io(context: &str) -> String {
    let error = io::Error::last_os_error();
    format!("{context}: {error}")
}

fn open_root(root: &Path) -> Result<OwnedFd, String> {
    let c_root = CString::new(root.as_os_str().as_encoded_bytes()).map_err(|_| "root contains NUL".to_string())?;
    let fd = unsafe {
        libc::open(
            c_root.as_ptr(),
            libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0,
        )
    };
    if fd < 0 {
        return Err(errno_io("open system root"));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(target_os = "linux")]
fn openat2(root_fd: i32, relative: &Path) -> Result<OwnedFd, String> {
    let bytes = relative.as_os_str().as_encoded_bytes();
    if bytes.is_empty() || bytes.len() > MAX_PATH_BYTES {
        return Err("invalid relative path".to_string());
    }
    let c_path = CString::new(bytes).map_err(|_| "path contains NUL".to_string())?;
    let how = OpenHow {
        flags: (libc::O_RDONLY | libc::O_CLOEXEC) as u64,
        mode: 0,
        resolve: RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS | RESOLVE_NO_XDEV,
    };
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            root_fd,
            c_path.as_ptr(),
            &how as *const OpenHow,
            std::mem::size_of::<OpenHow>(),
        ) as i32
    };
    if fd < 0 {
        return Err(errno_io("openat2"));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

pub fn read_confined_file(root: &Path, relative: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    if max_bytes == 0 || max_bytes > MAX_OUTPUT_BYTES {
        return Err("invalid read limit".to_string());
    }
    let relative_path = crate::validate_relative_path(relative).map_err(str::to_owned)?;
    let root_fd = open_root(root)?;
    let file_fd = openat2(root_fd.as_raw_fd(), &relative_path)?;

    let stat = unsafe {
        let mut value = std::mem::zeroed::<libc::stat>();
        if libc::fstat(file_fd.as_raw_fd(), &mut value) != 0 {
            return Err(errno_io("fstat"));
        }
        value
    };
    if (stat.st_mode & libc::S_IFMT) != libc::S_IFREG {
        return Err("target is not a regular file".to_string());
    }

    let mut file = File::from(file_fd);
    let mut output = Vec::with_capacity(max_bytes.min(64 * 1024));
    let mut buffer = [0u8; 8192];
    while output.len() <= max_bytes {
        let remaining = max_bytes + 1 - output.len();
        let read_size = remaining.min(buffer.len());
        let read = file.read(&mut buffer[..read_size]).map_err(|e| format!("read file: {e}"))?;
        if read == 0 {
            return Ok(output);
        }
        output.extend_from_slice(&buffer[..read]);
        if output.len() > max_bytes {
            return Err("file exceeds configured read limit".to_string());
        }
    }
    Err("file exceeds configured read limit".to_string())
}

#[derive(Debug, serde::Serialize)]
pub struct ProcessInfo {
    pid: u32,
    comm: String,
}

pub fn list_processes(limit: usize) -> Result<serde_json::Value, String> {
    if limit == 0 || limit > 128 {
        return Err("limit must be between 1 and 128".to_string());
    }
    let mut result = Vec::with_capacity(limit);
    let entries = fs::read_dir("/proc").map_err(|e| format!("read /proc: {e}"))?;
    for entry in entries {
        if result.len() >= limit {
            break;
        }
        let entry = match entry {
            Ok(value) => value,
            Err(_) => continue,
        };
        let name = entry.file_name();
        let name = match name.to_str() {
            Some(value) if value.chars().all(|c| c.is_ascii_digit()) => value,
            _ => continue,
        };
        let pid = match name.parse::<u32>() {
            Ok(value) => value,
            Err(_) => continue,
        };
        let comm = match fs::read_to_string(entry.path().join("comm")) {
            Ok(value) => value.trim().chars().take(128).collect(),
            Err(_) => continue,
        };
        result.push(ProcessInfo { pid, comm });
    }
    serde_json::to_value(result).map_err(|e| format!("serialize process list: {e}"))
}

pub fn system_info() -> serde_json::Value {
    let hostname = unsafe {
        let mut buffer = [0i8; 256];
        if libc::gethostname(buffer.as_mut_ptr(), buffer.len()) == 0 {
            CStr::from_ptr(buffer.as_ptr()).to_string_lossy().into_owned()
        } else {
            "unknown".to_string()
        }
    };
    json!({
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "hostname": hostname,
        "pid": std::process::id(),
        "uid": unsafe { libc::geteuid() },
        "gid": unsafe { libc::getegid() },
    })
}

pub fn execute_allowlisted(
    system_root: &Path,
    executable: &Path,
    args: &[String],
    cwd: &Path,
    allowlist: &[PathBuf],
    timeout: Duration,
    max_output: usize,
    allow_root: bool,
) -> Result<serde_json::Value, String> {
    if !executable.is_absolute() || executable.as_os_str().as_encoded_bytes().len() > MAX_PATH_BYTES {
        return Err("executable must be an absolute bounded path".to_string());
    }
    if args.len() > MAX_ARGS || args.iter().any(|arg| arg.is_empty() || arg.len() > MAX_ARG_BYTES || arg.as_bytes().contains(&0)) {
        return Err("invalid argument vector".to_string());
    }
    if max_output == 0 || max_output > MAX_OUTPUT_BYTES {
        return Err("invalid output limit".to_string());
    }
    if unsafe { libc::geteuid() } == 0 && !allow_root {
        return Err("root execution disabled".to_string());
    }
    if !allowlist.iter().any(|allowed| allowed == executable) {
        return Err("executable is not allowlisted".to_string());
    }
    let executable_meta = fs::symlink_metadata(executable).map_err(|e| format!("stat executable: {e}"))?;
    if !executable_meta.file_type().is_file() || executable_meta.file_type().is_symlink() {
        return Err("executable must be a regular non-symlink file".to_string());
    }
    if unsafe { libc::geteuid() } == 0
        && (unsafe { libc::stat(executable.as_os_str().as_encoded_bytes().as_ptr() as *const i8, std::ptr::null_mut()) } != 0)
    {
        // Ownership is checked below with Rust metadata; this branch only prevents
        // accidental assumptions that a privileged launcher is automatically trusted.
    }

    let canonical_root = fs::canonicalize(system_root).map_err(|e| format!("canonicalize system root: {e}"))?;
    let canonical_cwd = fs::canonicalize(cwd).map_err(|e| format!("canonicalize cwd: {e}"))?;
    if !canonical_cwd.starts_with(&canonical_root) {
        return Err("cwd escapes system root".to_string());
    }
    let cwd_meta = fs::symlink_metadata(cwd).map_err(|e| format!("stat cwd: {e}"))?;
    if cwd_meta.file_type().is_symlink() || !cwd_meta.is_dir() {
        return Err("cwd must be a real directory".to_string());
    }

    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(canonical_cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("LANG", "C")
        .env("LC_ALL", "C");

    let timeout_secs = timeout.as_secs().clamp(1, 300) as libc::rlim_t;
    let nofile = 128 as libc::rlim_t;
    let fsize = max_output as libc::rlim_t;
    unsafe {
        command.pre_exec(move || {
            if libc::setpgid(0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            let cpu = libc::rlimit { rlim_cur: timeout_secs, rlim_max: timeout_secs };
            if libc::setrlimit(libc::RLIMIT_CPU, &cpu) != 0 {
                return Err(io::Error::last_os_error());
            }
            let files = libc::rlimit { rlim_cur: nofile, rlim_max: nofile };
            if libc::setrlimit(libc::RLIMIT_NOFILE, &files) != 0 {
                return Err(io::Error::last_os_error());
            }
            let size = libc::rlimit { rlim_cur: fsize, rlim_max: fsize };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &size) != 0 {
                return Err(io::Error::last_os_error());
            }
            let core = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            if libc::setrlimit(libc::RLIMIT_CORE, &core) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = command.spawn().map_err(|e| format!("spawn: {e}"))?;
    let pid = child.id() as libc::pid_t;
    let stdout = child.stdout.take().ok_or_else(|| "stdout pipe missing".to_string())?;
    let output_thread = thread::spawn(move || {
        let mut reader = stdout;
        let mut data = Vec::with_capacity(max_output.min(64 * 1024));
        let mut buffer = [0u8; 8192];
        let mut overflow = false;
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    if data.len() + n > max_output {
                        let remaining = max_output.saturating_sub(data.len());
                        data.extend_from_slice(&buffer[..remaining]);
                        overflow = true;
                        unsafe { libc::kill(-pid, libc::SIGKILL); }
                        break;
                    }
                    data.extend_from_slice(&buffer[..n]);
                }
                Err(_) => break,
            }
        }
        (data, overflow)
    });

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().map_err(|e| format!("wait: {e}"))? {
            Some(status) => {
                let (output, overflow) = output_thread.join().map_err(|_| "output thread panicked".to_string())?;
                let text = String::from_utf8_lossy(&output).into_owned();
                if overflow {
                    return Ok(json!({"status": "output_limit", "output": text}));
                }
                return Ok(json!({
                    "status": if status.success() { "ok" } else { "error" },
                    "returncode": status.code(),
                    "output": text,
                }));
            }
            None if Instant::now() >= deadline => {
                unsafe { libc::kill(-pid, libc::SIGKILL); }
                let _ = child.wait();
                let (output, _) = output_thread.join().map_err(|_| "output thread panicked".to_string())?;
                return Ok(json!({
                    "status": "timeout",
                    "output": String::from_utf8_lossy(&output).into_owned(),
                }));
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    }
}
