# ABIYSS v0.1 Build Report

## Build identity

- Version: `0.1.0`
- Project: ABIYSS Runtime Project
- Scope: Linux-oriented runtime foundation
- War Pigs: intentionally excluded from this runtime release

## Current validation

The source-tree test suite currently reports:

- **44 passed**
- Python `compileall`: passed
- wheel build with local build backend (`pip wheel --no-build-isolation --no-deps`): passed in the development environment
- Gemini live API: not exercised in the build environment because network/API credentials are not assumed

The test suite covers normal behavior, persistence recovery, concurrency/preemption boundaries, schema validation, provider parsing, memory idempotence, Sleep recap semantics, guarded process execution, SkillLE validation and security regressions.

## Architecture under test

```text
Gemini
  |
  | model proposal / function_call
  v
Aquery
  |
  v
QuPs  -- durable representation
  |
  v
QQ   -- single logical execution lane
  |\
  | \__ Squery / Sleep work
  |
  +----> AST ----> action tools ----> Linux
  |
  +----> SST ----> silent tools ----> observation/maintenance
```

Aquery has scheduling precedence over Squery. Squery preemption is cooperative and happens only between tool steps. An already-running tool is never described as interrupted.

## Recovery invariant

The runtime follows a conservative side-effect rule:

1. persist `queued` before admission;
2. persist `running` before starting a side effect;
3. execute the side effect;
4. persist its result/checkpoint;
5. if that final persistence fails, mark the Query `recovery_required` in memory and leave the last durable state at `running`;
6. after restart, durable `running` work is **never replayed automatically**.

The reason is simple: the runtime cannot prove that an external side effect did not happen.

## Security validation

Covered controls include:

- bounded Query payloads and steps;
- strict JSON-like tool schemas;
- exact Query state transitions;
- QuPs SHA-256 integrity and atomic replacement;
- `O_NOFOLLOW` reads where available;
- runtime-root symlink rejection;
- absolute executable allowlisting;
- no shell execution;
- controlled child environment;
- dangerous loader/interpreter environment rejection;
- bounded stdout with process-group termination;
- wall-clock/CPU/file-descriptor/file-size limits;
- `PR_SET_NO_NEW_PRIVS` for child execution where supported;
- root execution disabled unless explicitly enabled;
- SkillLE manifest and file-set verification;
- root-owned/private checks for explicitly privileged skills;
- audit-log secret redaction;
- sensitive memory exclusion.

## Gemini verification

The adapter follows the current Interactions API shape: model-generated `function_call` steps are inspected by the application, the application executes the function, and a matching `function_result` is sent using the call ID and `previous_interaction_id`. Google documents `google-genai` 2.3.0+ for Interactions API support and currently lists `gemini-3.8-flash` as a stable GA model with function calling and structured outputs.

See `docs/GEMINI.md` for links to the official references reviewed on 2026-09-08.

## Known limitations

1. Python subprocess guardrails are not a complete hostile-code sandbox. The Python standard library warns that `preexec_fn` can deadlock in multithreaded applications. ABIYSS therefore treats this as a known engineering limitation and future work should move resource setup to an OS-level helper/service boundary.
2. `openat2()`-style kernel path-resolution constraints are not yet used by the Python implementation. Future broad filesystem tools should prefer kernel-assisted resolution restrictions over user-space path checks alone.
3. SQLite WAL improves read/write overlap but still permits only one writer at a time. High-volume memory storage will eventually require stronger workload isolation or a different persistence tier.
4. Stateful Gemini Interactions are stored by the provider unless configured otherwise. A production deployment should make the privacy/retention choice explicit.
5. The default tool registry intentionally does not expose arbitrary filesystem mutation or package-manager control.
