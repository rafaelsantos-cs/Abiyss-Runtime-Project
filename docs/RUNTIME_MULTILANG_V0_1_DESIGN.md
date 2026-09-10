# ABIYSS Runtime v0.1 — Multilingual Reconstruction Design

## Status

Branch: `runtime-multilang-v0.1`.

This branch reconstructs the ABIYSS runtime from the surviving v0.1 implementation while preserving its behavioral contracts and moving the Linux/system boundary into a dedicated Rust component.

The objective is **not** to make every module multilingual. The objective is to give every language a narrow, defensible responsibility and one canonical contract between them.

## 1. Runtime model

```text
                       Gemini Interactions API
                                  |
                                  v
                         +-------------------+
                         | Python Control    |
                         | Plane             |
                         |                   |
                         | Query / QQ        |
                         | QuPs              |
                         | Memory / Sleep    |
                         | Tool policy       |
                         +---------+---------+
                                   |
                            typed JSON / IPC
                                   |
                                   v
                         +-------------------+
                         | Rust System Plane |
                         |                   |
                         | Linux observation|
                         | confined reads    |
                         | bounded process   |
                         | execution         |
                         +---------+---------+
                                   |
                                   v
                              Void Linux
```

The Python process is the cognitive/control coordinator. The Rust process is the system boundary. The model never receives authority merely by generating a function call.

## 2. Language responsibilities

### Python

Canonical responsibilities:

- Query and Checkpoint domain model;
- QQ admission, ordering and cooperative preemption;
- QuPs orchestration and recovery policy;
- Gemini provider adapter;
- Memory and Sleep orchestration;
- tool registry and authorization metadata;
- CLI and developer-facing diagnostics.

Python remains the reference implementation for high-level behavioral contracts during this reconstruction.

### Rust

Canonical responsibilities:

- operating-system interaction;
- Unix-domain IPC server;
- peer credential verification;
- Linux `openat2` confined file access;
- bounded process launch;
- process-group lifecycle control;
- resource-limit configuration;
- `PR_SET_NO_NEW_PRIVS` handling;
- future Linux-specific isolation primitives.

The Rust service does not contain Gemini logic or Query scheduling semantics.

### SQLite / SQL

Canonical responsibilities:

- durable local Memory state;
- provenance, sensitivity and idempotence constraints;
- future runtime metadata that benefits from relational transactions.

WAL remains appropriate for local same-host concurrency, but there is still one writer at a time. We therefore serialize high-value writes in the Python MemoryStore rather than pretending WAL is a multi-writer architecture.

### Julia

Canonical responsibilities:

- offline/runtime analysis;
- memory and query statistics;
- simulation and benchmarking;
- regression analysis.

Julia is not a trust boundary and does not execute privileged actions in v0.1.

### Go

Canonical responsibilities:

- optional long-running telemetry/health tooling;
- external experiment drivers;
- future high-concurrency side services where Go is materially simpler than Python.

Go does not own Query semantics.

### JavaScript / Electron

Canonical responsibilities:

- future desktop renderer and control surface;
- visual inspection of runtime state;
- user-facing diagnostics.

The renderer never receives direct authority over the Linux system plane.

### C#

Canonical responsibility:

- occasional administrative/tooling client of the stable C ABI/IPC contracts.

C# is deliberately peripheral in v0.1.

### C++

The C++ WarPigs engine remains a separate research/simulation component. It is not inserted into the runtime simply to increase the language count.

## 3. Process boundary

The Rust system plane runs as a separate process from Python. The preferred Void Linux deployment is a runit-managed service.

```text
runit
  |
  +--> abiyss-systemd (Rust)
  |
  +--> abiyss-runtime (Python)
```

The two processes communicate over a Unix-domain socket with a bounded JSON-lines protocol.

### Trust rule

The Python control plane can request an operation. The Rust system plane independently validates the request and its policy before acting.

## 4. System-plane protocol

Request:

```json
{
  "id": "req-123",
  "op": "process.list",
  "args": {"limit": 32}
}
```

Success:

```json
{
  "id": "req-123",
  "ok": true,
  "result": {"processes": []}
}
```

Failure:

```json
{
  "id": "req-123",
  "ok": false,
  "error": {
    "code": "invalid_argument",
    "message": "limit is outside the allowed range"
  }
}
```

Protocol invariants:

- request and response IDs are bounded and matched;
- maximum request line is bounded;
- unknown operations fail closed;
- unknown fields are rejected where schema is strict;
- output is bounded;
- the daemon never evaluates shell strings;
- there is no generic `run(command_string)` primitive.

## 5. v0.1 operations

### `system.info`

Read-only OS identity and daemon metadata.

### `process.list`

Read-only local process observation with a hard result limit.

### `file.read`

Read-only access beneath the configured system-plane root. On Linux, v0.1 uses `openat2()` with resolution restrictions rather than following arbitrary symbolic links.

### `process.exec`

Optional. Requires an explicit absolute executable allowlist. The service rejects shell interpreters and loader-injection environment variables. It is disabled by default and root execution is denied by default.

This is a guardrail, not a hostile-code sandbox.

## 6. Query lifecycle

The existing Python lifecycle remains canonical:

```text
QUEUED -> RUNNING -> COMPLETED
              |
              +-> PAUSED -> QUEUED
              |
              +-> FAILED
              |
              +-> RECOVERY_REQUIRED
```

A `RUNNING` Query persisted before a tool side effect begins is the durable intent record. A persistence failure before launch prevents the launch. A persistence failure after launch yields `RECOVERY_REQUIRED` and automatic replay is forbidden.

## 7. QQ

QQ remains one logical execution lane.

Ordering:

1. Aquery before Squery;
2. higher priority before lower priority inside a class;
3. FIFO for equal class and priority.

Squery preemption is cooperative and only occurs at a tool boundary. The runtime never claims to atomically cancel arbitrary external side effects.

## 8. QuPs

QuPs remains an integrity-protected durable Query envelope.

Properties retained from the reference implementation:

- versioned envelope;
- canonical JSON;
- SHA-256 integrity digest;
- restrictive atomic writes;
- no-follow reads;
- bounded payload;
- strict field validation.

A digest is not an authentication mechanism.

## 9. Memory and Sleep

Memory remains SQLite-backed and provenance-first.

Sleep remains split into:

- 5-minute tick;
- 30-minute review.

Sleep recaps are immutable by source fingerprint. Historical evidence may participate in repetition detection without becoming new Sleep input.

Sensitive tool results are excluded from normal memory persistence.

## 10. Gemini

The Gemini provider is an adapter only. The current model target is `gemini-3.8-flash`, with `thinking_level` configurable as `low`, `medium`, or `high`.

Stateful interaction continuation uses `previous_interaction_id`. Function calls are converted into Aqueries and must pass the same validation and durability path as any other Query.

Function results preserve the original function name and call ID.

The current PyPI release observed during this reconstruction is `google-genai 2.22.0`.

## 11. Security posture

The critical security rule is:

> Model output is data that may request an action. It is never authorization by itself.

The system plane adds a second authorization boundary outside the Python process.

For Linux path access, `openat2()` with `RESOLVE_BENEATH` and `RESOLVE_NO_SYMLINKS` is preferred because the kernel performs constrained path resolution. The service refuses to silently fall back to an unconstrained path walk when the primitive is unavailable.

The service is designed for Void Linux. Void uses runit for service supervision, so production installation is documented as a runit service rather than a systemd unit.

## 12. Deployment target

The initial production-shaped target is:

```text
Void Linux
   |
   +-- runit
       |
       +-- abiyss-systemd (Rust, bounded system authority)
       |
       +-- abiyss runtime (Python, model/control plane)
       |
       +-- optional abiyss-ui (Electron)
       |
       +-- optional abiyss-watch (Go)
```

The names `systemd` and `abiyss-systemd` refer to the ABIYSS system daemon, not the Linux systemd init system. Void remains runit-based.

## 13. Research basis

- Google currently recommends the Interactions API for Gemini agent and application workflows, and its documentation specifies `previous_interaction_id` for stateful multi-turn interactions. Function results are sent as `function_result` steps carrying the original function call `name` and `call_id`. [Google Gemini API documentation](https://ai.google.dev/gemini-api/docs/interactions-overview), [function calling](https://ai.google.dev/gemini-api/docs/function-calling).
- Google documents structured output through `response_format` with JSON Schema for Interactions. [Structured output](https://ai.google.dev/gemini-api/docs/structured-output).
- Rust's reference documents `extern "C"` as the stable C calling convention for foreign interfaces; the Rust Embedded Book documents C-compatible FFI patterns. [Rust Reference](https://doc.rust-lang.org/reference/items/external-blocks.html), [Rust/C interoperability](https://doc.rust-lang.org/stable/embedded-book/interoperability/rust-with-c.html).
- Linux `openat2(2)` documents `RESOLVE_BENEATH` and `RESOLVE_NO_SYMLINKS` specifically for restricting path resolution of untrusted paths. [openat2(2)](https://man7.org/linux/man-pages/man2/openat2.2.html).
- SQLite WAL permits readers and writers to proceed concurrently but still has only one writer at a time; SQLite also recommends client/server databases for workloads requiring very high write concurrency. [SQLite WAL](https://www.sqlite.org/wal.html), [When to use SQLite](https://www.sqlite.org/whentouse.html).
- Void Linux uses runit for service supervision and documents service directories, user services and service lifecycle management. [Void Linux services](https://docs.voidlinux.org/config/services/index.html).

## 14. Non-goals

The v0.1 runtime does not implement:

- autonomous propagation;
- self-replication;
- arbitrary shell execution;
- arbitrary filesystem mutation;
- automatic replay of ambiguous side effects;
- kernel-level hostile-code sandboxing;
- WarPigs integration;
- a renderer with system authority.
