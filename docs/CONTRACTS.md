# ABIYSS v0.1 Module Contracts

This document defines the minimum contracts that modules must preserve. Internal implementation may change while these invariants remain true.

## Query contract

A Query has:

- stable ID;
- `Aquery` or `Squery` type;
- bounded integer priority;
- JSON-safe payload;
- durable state;
- checkpoint;
- optional Gemini interaction/call identity;
- source and attempt metadata.

Query snapshots are versioned and rejected when required fields, enums, timestamps or checkpoint structure are invalid.

## QQ contract

`QueryQueue.submit(query)` admits a Query only after local validation and durable QuPs persistence.

`QueryQueue.run_once()` executes at most one tool boundary at a time.

QQ guarantees:

- Aquery-before-Squery precedence;
- numeric priority within class;
- FIFO tie breaking;
- no automatic replay of ambiguous `RUNNING` work;
- cooperative, boundary-only Squery preemption;
- no tool-call bypass around QQ;
- duplicate external `(interaction_id, tool_call_id)` requests are idempotent.

## QuPs contract

`QuPsStore.save(query)` either durably replaces a complete envelope or raises an error.

`QuPsStore.load(query_id)` never follows a QuPs symlink and rejects non-regular or oversized files.

Canonical hashing is deterministic. The digest is an integrity mechanism only.

## Tool contract

Every registered tool declares:

- a unique name;
- a Query type;
- a bounded JSON argument schema;
- privilege metadata;
- reversibility metadata;
- memory-persistence policy.

AST refuses tools declared for Squery. SST refuses tools declared for Aquery.

The registry validates before admission and immediately before dispatch.

## Process execution contract

A process tool:

- receives argv, not a shell command string;
- accepts only an absolute executable in an explicit allowlist;
- constructs a controlled environment;
- rejects loader/interpreter injection variables;
- uses a dedicated process group;
- limits file descriptors and output;
- kills the process group on timeout/output violation;
- denies root by default.

These are guardrails, not a sandbox.

## System-plane contract

The Rust system plane is a separate local process connected through a bounded Unix-socket protocol.

The system plane:

- validates protocol version, request ID, operation and argument shape;
- authenticates the peer with local Unix credentials;
- bounds worker count, pending queue size, request size, response size and I/O timeouts;
- refuses unsafe filesystem paths and special files;
- performs bounded process execution under an explicit allowlist;
- never interprets model output as authorization.

System-plane operations are capability-specific. A successful read-only operation does not grant process-execution authority.

## SkillLE verification contract

Skill manifests and skill trees are treated as untrusted filesystem input.

When `SkillLoader` has a Rust `SystemPlaneClient`, verification is delegated to `skill.verify` in the Rust system plane. Python consumes only the manifest returned by the verifier and does not independently re-read `skill.json` for the verification decision.

Rust verification guarantees:

- bounded manifest size;
- closed manifest schema;
- bounded file count and tree depth;
- bounded individual and aggregate file sizes;
- canonical relative member paths only;
- regular-file-only traversal with symlink rejection;
- exact manifest file-set matching;
- SHA-256 digest matching;
- safe relative entrypoints and restricted absolute entrypoints;
- explicit root-skill policy checks.

`skill.verify` is read-only. It does not execute the skill and does not itself authorize execution.

The verification result is not an authentication token. The current v0.1 execution path still requires a later execution-hardening pass to close the remaining verification-to-execution time-of-check/time-of-use window.

## Memory contract

Memory records preserve provenance through `source_id` and can be marked sensitive.

Daily/contextual keys are idempotent. Duplicate `(key, fingerprint, source_id)` inserts do not create additional records.

Sensitive memory is excluded from normal queries.

## Sleep contract

A Sleep tick may materialize a recap only from source observations not already represented by an immutable recap.

A recap is immutable by fingerprint. Historical observations may be used as repetition evidence without becoming new Sleep input.

## Gemini contract

The provider returns a normalized `ModelTurn` containing:

- optional interaction ID;
- zero or more normalized function calls;
- text output.

A function call must have a non-empty ID/name and object arguments. IDs must be unique within the returned interaction.

The function result must preserve the original function name and call ID and use the previous interaction ID for stateful continuation.

Auxiliary structured output uses `store=false` in v0.1 to avoid unnecessary provider-side persistence.
