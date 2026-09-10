use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, BufRead, Write};

const VERSION: u64 = 1;
const MAX_LINE: usize = 256 * 1024;

#[derive(Deserialize)]
struct Request {
    version: u64,
    op: String,
    request_id: String,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Serialize)]
struct Response<'a> {
    version: u64,
    ok: bool,
    request_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn error(request_id: &str, message: &str) -> Response<'_> {
    Response { version: VERSION, ok: false, request_id, result: None, error: Some(message[..message.len().min(4096)].to_string()) }
}

fn handle(req: Request) -> Response<'static> {
    let request_id = Box::leak(req.request_id.into_boxed_str());
    if req.version != VERSION { return error(request_id, "unsupported protocol version"); }
    if request_id.is_empty() || request_id.len() > 256 || request_id.as_bytes().contains(&0) { return error(request_id, "invalid request id"); }

    match req.op.as_str() {
        "health" => Response { version: VERSION, ok: true, request_id, result: Some(serde_json::json!({"component":"rust-security","version":"0.2.0"})), error: None },
        "sha256" => {
            let Some(text) = req.payload.get("text").and_then(|v| v.as_str()) else { return error(request_id, "sha256 requires payload.text"); };
            if text.len() > 128 * 1024 { return error(request_id, "text too large"); }
            let digest = Sha256::digest(text.as_bytes());
            Response { version: VERSION, ok: true, request_id, result: Some(serde_json::json!({"sha256": hex::encode(digest)})), error: None }
        }
        _ => error(request_id, "unknown operation"),
    }
}

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout());
    for line in stdin.lock().lines() {
        let line = match line { Ok(v) => v, Err(_) => break };
        if line.len() > MAX_LINE { let response = error("", "request too large"); writeln!(stdout, "{}", serde_json::to_string(&response).unwrap()).ok(); stdout.flush().ok(); continue; }
        let response = match serde_json::from_str::<Request>(&line) { Ok(req) => handle(req), Err(_) => error("", "invalid JSON") };
        if let Ok(encoded) = serde_json::to_string(&response) { writeln!(stdout, "{}", encoded).ok(); stdout.flush().ok(); }
    }
}
