//! ABIYSS system-plane daemon library.
//!
//! The system plane is deliberately narrower than the Python control plane:
//! it accepts bounded, typed requests, applies a second authorization check,
//! and owns Linux-specific operations. It never interprets shell strings or
//! model output directly.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 128 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 128 * 1024;
pub const MAX_ID_BYTES: usize = 128;
pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_ARG_BYTES: usize = 4096;
pub const MAX_ARGS: usize = 32;
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub version: u32,
    pub id: String,
    pub op: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub version: u32,
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("invalid JSON request")]
    Json(#[from] serde_json::Error),
    #[error("unsupported protocol version")]
    Version,
    #[error("invalid request id")]
    Id,
    #[error("invalid operation")]
    Operation,
    #[error("arguments must be a JSON object")]
    Arguments,
    #[error("request line exceeds configured limit")]
    Oversized,
}

impl Request {
    pub fn parse(line: &[u8]) -> Result<Self, ProtocolError> {
        if line.len() > MAX_REQUEST_BYTES {
            return Err(ProtocolError::Oversized);
        }
        let request: Self = serde_json::from_slice(line)?;
        if request.version != PROTOCOL_VERSION {
            return Err(ProtocolError::Version);
        }
        validate_id(&request.id).map_err(|_| ProtocolError::Id)?;
        if request.op.is_empty() || request.op.len() > 64 || request.op.as_bytes().contains(&0) {
            return Err(ProtocolError::Operation);
        }
        if !request.args.is_object() {
            return Err(ProtocolError::Arguments);
        }
        Ok(request)
    }
}

impl Response {
    pub fn success(id: String, result: serde_json::Value) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: String, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            id,
            ok: false,
            result: None,
            error: Some(ErrorBody {
                code: code.into(),
                message: message.into(),
            }),
        }
    }

    pub fn encode_line(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut bytes = serde_json::to_vec(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

pub fn validate_id(id: &str) -> Result<(), ()> {
    if id.is_empty() || id.len() > MAX_ID_BYTES || id.as_bytes().contains(&0) {
        return Err(());
    }
    Ok(())
}

pub fn validate_relative_path(value: &str) -> Result<PathBuf, &'static str> {
    if value.is_empty() || value.len() > MAX_PATH_BYTES || value.as_bytes().contains(&0) {
        return Err("invalid path");
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err("absolute paths are forbidden");
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("path escapes configured root")
            }
        }
    }
    Ok(path.to_path_buf())
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub socket_path: PathBuf,
    pub root: PathBuf,
    pub allowed_uid: u32,
    pub allow_exec: bool,
    pub allow_root_exec: bool,
    pub command_allowlist: Vec<PathBuf>,
    pub exec_timeout: Duration,
    pub max_output_bytes: usize,
}

impl ServerConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.socket_path.as_os_str().is_empty() || self.root.as_os_str().is_empty() {
            return Err(ConfigError::EmptyPath);
        }
        if self.exec_timeout.is_zero() || self.exec_timeout > Duration::from_secs(300) {
            return Err(ConfigError::InvalidTimeout);
        }
        if self.max_output_bytes < 1024 || self.max_output_bytes > 4 * 1024 * 1024 {
            return Err(ConfigError::InvalidOutputLimit);
        }
        for path in &self.command_allowlist {
            if !path.is_absolute() {
                return Err(ConfigError::InvalidExecutable);
            }
        }
        if self.allow_root_exec && !self.allow_exec {
            return Err(ConfigError::RootRequiresExec);
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("socket and root paths must be non-empty")]
    EmptyPath,
    #[error("invalid execution timeout")]
    InvalidTimeout,
    #[error("invalid output limit")]
    InvalidOutputLimit,
    #[error("allowlisted executables must be absolute paths")]
    InvalidExecutable,
    #[error("root execution requires allow_exec=true")]
    RootRequiresExec,
}

impl fmt::Display for ErrorBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
