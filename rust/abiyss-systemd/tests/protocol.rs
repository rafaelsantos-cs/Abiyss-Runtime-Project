use abiyss_systemd::{validate_relative_path, Request, Response, PROTOCOL_VERSION};
use serde_json::json;

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
