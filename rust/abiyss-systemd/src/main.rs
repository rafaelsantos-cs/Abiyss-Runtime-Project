mod linux;
mod server;

use abiyss_systemd::{ServerConfig, PROTOCOL_VERSION};
use std::env;
use std::path::PathBuf;
use std::time::Duration;

fn env_path(name: &str, default: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(env::var_os(name).unwrap_or_else(|| default.into())))
}

fn current_uid() -> Result<u32, String> {
    #[cfg(target_family = "unix")]
    {
        Ok(unsafe { libc::geteuid() as u32 })
    }
    #[cfg(not(target_family = "unix"))]
    {
        Err("ABIYSS system plane requires Unix".to_string())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let uid = current_uid()?;
    let socket = env_path("ABIYSS_SYSTEM_SOCKET", &format!("/tmp/abiyss-{uid}.sock"))?;
    let root = env_path("ABIYSS_SYSTEM_ROOT", &format!("/tmp/abiyss-{uid}-root"))?;
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

    eprintln!(
        "abiyss-systemd protocol={} socket={} root={} exec={}",
        PROTOCOL_VERSION,
        socket.display(),
        root.display(),
        allow_exec
    );

    let config = ServerConfig {
        socket_path: socket,
        root,
        allowed_uid: uid,
        allow_exec,
        allow_root_exec,
        command_allowlist: allowlist,
        exec_timeout: Duration::from_secs(timeout_secs),
        max_output_bytes: max_output,
    };
    server::run(config).map_err(Into::into)
}
