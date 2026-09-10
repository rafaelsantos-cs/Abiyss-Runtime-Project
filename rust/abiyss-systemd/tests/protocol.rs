use abiyss_systemd::{validate_relative_path, ConfigError, Request, Response, ServerConfig, PROTOCOL_VERSION};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn accepts_valid_request_and_rejects_non_object_args() {
    let raw = serde_json::to_vec(&json!({
        "version": PROTOCOL_VERSION,
        "id": "req-1",
        "op": "system.info",
        "args": {}
    }))
    .unwrap();
    let request = Request::parse(&raw).unwrap();
    assert_eq!(request.id, "req-1");

    let bad = serde_json::to_vec(&json!({
        "version": PROTOCOL_VERSION,
        "id": "req-2",
        "op": "system.info",
        "args": []
    }))
    .unwrap();
    assert!(Request::parse(&bad).is_err());
}

#[test]
fn rejects_unknown_protocol_version_and_oversized_request() {
    let bad_version = serde_json::to_vec(&json!({
        "version": 999,
        "id": "req-1",
        "op": "system.info",
        "args": {}
    }))
    .unwrap();
    assert!(Request::parse(&bad_version).is_err());

    let oversized = vec![b'x'; abiyss_systemd::MAX_REQUEST_BYTES + 1];
    assert!(Request::parse(&oversized).is_err());
}

#[test]
fn rejects_unknown_request_fields() {
    let raw = serde_json::to_vec(&json!({
        "version": PROTOCOL_VERSION,
        "id": "req-1",
        "op": "system.info",
        "args": {},
        "unexpected": true
    }))
    .unwrap();
    assert!(Request::parse(&raw).is_err());
}

#[test]
fn path_policy_rejects_absolute_parent_and_parent_escape() {
    for path in ["/etc/passwd", "../outside", "a/../../outside", ""] {
        assert!(validate_relative_path(path).is_err(), "accepted unsafe path {path}");
    }
    assert!(validate_relative_path("etc/hosts").is_ok());
}

#[test]
fn response_is_newline_delimited_and_versioned() {
    let response = Response::success("req-1".to_string(), json!({"ok": true}));
    let bytes = response.encode_line().unwrap();
    assert!(bytes.ends_with(b"\n"));
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["version"], PROTOCOL_VERSION);
    assert_eq!(value["id"], "req-1");
    assert_eq!(value["ok"], true);
}

fn base_config() -> ServerConfig {
    ServerConfig {
        socket_path: PathBuf::from("/tmp/abiyss-test.sock"),
        root: PathBuf::from("/tmp/abiyss-root"),
        allowed_uid: 1000,
        allow_exec: false,
        allow_root_exec: false,
        command_allowlist: Vec::new(),
        exec_timeout: Duration::from_secs(20),
        max_output_bytes: 64 * 1024,
        max_workers: 8,
        max_pending: 32,
        io_timeout: Duration::from_secs(5),
    }
}

#[test]
fn config_rejects_invalid_worker_count() {
    let mut config = base_config();
    config.max_workers = 0;
    assert!(matches!(config.validate(), Err(ConfigError::InvalidWorkerCount)));
}

#[test]
fn config_rejects_invalid_io_timeout() {
    let mut config = base_config();
    config.io_timeout = Duration::ZERO;
    assert!(matches!(config.validate(), Err(ConfigError::InvalidIoTimeout)));
}
