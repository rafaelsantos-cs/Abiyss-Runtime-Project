# ABIYSS Changelog

## 0.1.0 - Runtime foundation

This release is the rebuilt ABIYSS runtime foundation. It intentionally focuses on deterministic execution semantics, durability, guarded tools, memory and a clean Gemini boundary. The War Pigs adversarial harness is kept outside the runtime package.

### Core

- Added Aquery and Squery query classes.
- Added deterministic QQ scheduling with Aquery precedence, numeric priority and FIFO tie-breaking.
- Added cooperative Squery preemption at tool boundaries.
- Added Query state machine and durable checkpoints.
- Added explicit `recovery_required` state for ambiguous post-side-effect persistence.
- Added duplicate `(interaction_id, tool_call_id)` protection.

### QuPs

- Added versioned durable Query envelopes.
- Added canonical JSON and SHA-256 integrity checks.
- Added atomic persistence and restrictive permissions.
- Added no-follow reads, regular-file enforcement and bounded payload reads.

### Tools

- Added AST and SST policy planes.
- Added bounded JSON-schema-like argument validation.
- Added `system.info` and `process.list` default observation tools.
- Added optional `system.exec` with absolute allowlisting, no shell, controlled environment, output/FD/time limits and process-group cleanup.
- Kept arbitrary execution disabled by default.

### Memory and Sleep

- Added SQLite memory with WAL and full synchronization.
- Added daily/contextual keys, relations, provenance and idempotent writes.
- Added sensitive-memory filtering.
- Added five-minute Sleep tick and thirty-minute review semantics.
- Added immutable recap fingerprints.
- Added deterministic and optional Gemini-backed Sleep Key Alignment.

### Skills

- Added SkillLE manifest parsing and file-hash verification.
- Added symlink/path checks, exact file-set verification and bounded execution.
- Added explicit root policy for privileged skills.
- Added Skill Emergence candidate scoring using repetition and competence.

### Gemini

- Added Google GenAI Interactions API adapter.
- Added manual function-call execution through ABIYSS rather than direct model execution.
- Added stateful chaining through `previous_interaction_id`.
- Added explicit Gemini thinking-level configuration (`low`, `medium`, `high`).
- Made auxiliary structured-output alignment requests stateless (`store=false`).
- Pinned the optional SDK range to `google-genai>=2.22.0,<3` for v0.1.

### Operations and QA

- Added `python -m abiyss` entrypoint and CLI commands `doctor`, `status`, `prompt` and `recover`.
- Added GitHub Actions CI configuration for Python 3.11 through 3.14.
- Added critical runtime/security regression tests.
- Local reconstruction validation reached 68 passing tests after the final Gemini privacy hardening.

### Known limitations

- No live Gemini request was performed in the reconstruction environment.
- Python `preexec_fn` remains a known residual risk in threaded process execution.
- OS-level isolation for untrusted generated code is deferred to a future release.
- Broad privileged filesystem/package-manager control is intentionally outside v0.1.
