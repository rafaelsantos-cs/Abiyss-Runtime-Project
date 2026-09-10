# ABIYSS v0.2 Native Boundary

The v0.2 runtime introduces optional native components behind a language-neutral JSON Lines protocol.

## Design

Python remains the orchestration layer. Native components never receive the model directly and never become an authorization source. They receive already-validated, bounded requests and return structured results.

```text
Python ABIYSS
    |
    | JSONL / stdin-stdout
    v
Native component
    |
    +-- Rust: security primitives / canonical validation
    +-- C++: process and Linux system primitives
    +-- Julia: deterministic analysis/scoring
    +-- Go: optional long-lived service/IPC
    +-- C#: optional .NET integration
```

## Protocol

Each request is one JSON object terminated by `\\n`:

```json
{"version":1,"op":"health","request_id":"...","payload":{}}
```

Each response is one JSON object:

```json
{"version":1,"ok":true,"request_id":"...","result":{}}
```

Errors use `ok:false` and a bounded `error` string. Implementations must reject malformed JSON, unknown protocol versions, missing request IDs and oversized input.

The protocol is intentionally small. It is not an RPC framework and is not an authorization mechanism.
