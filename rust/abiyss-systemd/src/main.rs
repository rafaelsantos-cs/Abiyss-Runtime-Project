mod linux;
mod server;

use abiyss_system_plane::{ServerConfig, PROTOCOL_VERSION};
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
        Ok(raw) => raw.parse::<u32>().map_err(|_| "ABIYSS_SYSTEM_ALLOWED_UID must be an unsigned integer".to_string()),
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

    let allow_exec = env::var("ABIYSS_SYSTEM_ALLOW_EXEC").map(|v| v == "1").unwrap_or(false);
    let allow_root_exec = env::var("ABIYSS_SYSTEM_ALLOW_ROOT_EXEC").map(|v| v == "1").unwrap_or(false);
    let timeout_secs = env::var("ABIYSS_SYSTEM_EXEC_TIMEOUT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(20);
    let max_output = env::var("ABIYSS_SYSTEM_MAX_OUTPUT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(64 * 1024);
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
        "abiyss-system protocol={} socket={} root={} daemon_uid={} allowed_uid={} exec={}",
        PROTOCOL_VERSION,
        socket.display(),
        root.display(),
        daemon_uid,
        authorized_uid,
        allow_exec
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
    };
    server::run(config).map_err(Into::into)
}
