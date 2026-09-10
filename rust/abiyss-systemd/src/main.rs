use abiyss_systemd::{ServerConfig, PROTOCOL_VERSION, DEFAULT_IO_TIMEOUT, DEFAULT_MAX_PENDING, DEFAULT_MAX_WORKERS};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

fn env_path(name: &str) -> Result<Option<PathBuf>, String> {
    match env::var_os(name) {
        Some(value) => {
            let path = PathBuf::from(value);
            if path.as_os_str().is_empty() || !path.is_absolute() {
                return Err(format!("{name} must be an absolute non-empty path"));
            }
            Ok(Some(path))
        }
        None => Ok(None),
    }
}

fn env_bool(name: &str, default: bool) -> Result<bool, String> {
    match env::var(name) {
        Ok(raw) => match raw.as_str() {
            "0" | "false" | "False" => Ok(false),
            "1" | "true" | "True" => Ok(true),
            _ => Err(format!("{name} must be 0/1 or true/false")),
        },
        Err(_) => Ok(default),
    }
}

fn env_u64(name: &str, default: u64) -> Result<u64, String> {
    match env::var(name) {
        Ok(raw) => raw
            .parse::<u64>()
            .map_err(|_| format!("{name} must be an unsigned integer")),
        Err(_) => Ok(default),
    }
}

fn env_usize(name: &str, default: usize) -> Result<usize, String> {
    match env::var(name) {
        Ok(raw) => raw
            .parse::<usize>()
            .map_err(|_| format!("{name} must be an unsigned integer")),
        Err(_) => Ok(default),
    }
}

fn current_uid() -> u32 {
    unsafe { libc::geteuid() as u32 }
}

fn default_socket(uid: u32) -> Result<PathBuf, String> {
    if let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR") {
        let dir = PathBuf::from(runtime_dir);
        if dir.is_absolute() && dir.is_dir() {
            return Ok(dir.join("abiyss-system.sock"));
        }
    }
    let run_user = PathBuf::from(format!("/run/user/{uid}"));
    if run_user.is_dir() {
        return Ok(run_user.join("abiyss-system.sock"));
    }
    Err("secure runtime directory unavailable; set ABIYSS_SYSTEM_SOCKET explicitly".to_string())
}

fn allowed_uid(default: u32) -> Result<u32, String> {
    match env::var("ABIYSS_SYSTEM_ALLOWED_UID") {
        Ok(raw) => raw
            .parse::<u32>()
            .map_err(|_| "ABIYSS_SYSTEM_ALLOWED_UID must be an unsigned integer".to_string()),
        Err(_) => Ok(default),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let daemon_uid = current_uid();
    let socket = match env_path("ABIYSS_SYSTEM_SOCKET")? {
        Some(path) => path,
        None => default_socket(daemon_uid)?,
    };
    let root = match env_path("ABIYSS_SYSTEM_ROOT")? {
        Some(path) => path,
        None => {
            if daemon_uid == 0 {
                PathBuf::from("/")
            } else if let Some(home) = env::var_os("HOME") {
                PathBuf::from(home).join(".local/share/abiyss/system-root")
            } else {
                return Err("ABIYSS_SYSTEM_ROOT must be set when HOME is unavailable".into());
            }
        }
    };

    let allow_exec = env_bool("ABIYSS_SYSTEM_ALLOW_EXEC", false)?;
    let allow_root_exec = env_bool("ABIYSS_SYSTEM_ALLOW_ROOT_EXEC", false)?;
    let timeout_secs = env_u64("ABIYSS_SYSTEM_EXEC_TIMEOUT", 20)?;
    let max_output = env_usize("ABIYSS_SYSTEM_MAX_OUTPUT", 64 * 1024)?;
    let max_workers = env_usize("ABIYSS_SYSTEM_MAX_WORKERS", DEFAULT_MAX_WORKERS)?;
    let max_pending = env_usize("ABIYSS_SYSTEM_MAX_PENDING", DEFAULT_MAX_PENDING)?;
    let io_timeout_secs = env_u64("ABIYSS_SYSTEM_IO_TIMEOUT", DEFAULT_IO_TIMEOUT.as_secs())?;
    if io_timeout_secs == 0 || io_timeout_secs > 60 {
        return Err("ABIYSS_SYSTEM_IO_TIMEOUT must be between 1 and 60 seconds".into());
    }
    let allowlist = env::var("ABIYSS_SYSTEM_COMMAND_ALLOWLIST")
        .unwrap_or_default()
        .split(':')
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .collect();
    let authorized_uid = allowed_uid(daemon_uid)?;

    if !root.exists() {
        fs::create_dir_all(&root)?;
    }

    eprintln!(
        "abiyss-system protocol={} socket={} root={} daemon_uid={} allowed_uid={} exec={} workers={} pending={}",
        PROTOCOL_VERSION,
        socket.display(),
        root.display(),
        daemon_uid,
        authorized_uid,
        allow_exec,
        max_workers,
        max_pending
    );

    let config = ServerConfig {
        socket_path: socket,
        root,
        allowed_uid: authorized_uid,
        allow_exec,
        allow_root_exec,
        command_allowlist: allowlist,
        exec_timeout: Duration::from_secs(timeout_secs),
        max_output_bytes: max_output,
        max_workers,
        max_pending,
        io_timeout: Duration::from_secs(io_timeout_secs),
    };
    abiyss_systemd::server::run(config).map_err(Into::into)
}
