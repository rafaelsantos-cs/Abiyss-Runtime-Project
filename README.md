# ABIYSS Runtime Project

ABIYSS is a Linux-oriented agent runtime built around one hard architectural boundary:

```text
Gemini / model
      |
      v
   Query
      |
      v
     QuPs
      |
      v
      QQ
   /       \
 Aquery   Squery
   |         |
  AST       SST
   \         /
     tools
       |
       v
  Linux system
```

The model proposes work. ABIYSS validates, persists, schedules and executes that work. A model response is never itself an authorization token and a model tool call never bypasses QQ.

## v0.1 scope

This release establishes the durable runtime foundation:

- Aquery/Squery and deterministic QQ scheduling;
- Query-level checkpoints and crash-aware recovery;
- QuPs atomic persistence with SHA-256 integrity checking;
- AST/SST tool-plane separation;
- bounded JSON-schema-like tool argument validation;
- guarded process execution with no shell, absolute allowlisting, bounded arguments/output, environment hardening, timeout and process-group cleanup;
- SQLite-backed Memory with daily/contextual keys, provenance and sensitive-memory filtering;
- Sleep tick/review and immutable recap semantics;
- optional Gemini Interactions API adapter with manual function-call execution;
- SkillLE artifact validation/execution and Skill Emergence candidate detection;
- CLI/diagnostics and reproducible local build metadata.

The **War Pigs** adversarial attack harness is deliberately not part of this v0.1 runtime package. It belongs in a separate test/research layer.

## Installation

```bash
python -m pip install .
python -m pip install '.[dev]'
python -m pip install '.[gemini]'
```

For an offline source checkout where package-index access is unavailable:

```bash
PYTHONPATH=src python -m pytest -q
python -m compileall -q src
```

## CLI

```bash
abiyss doctor
abiyss status
abiyss prompt 'inspect the machine'
abiyss recover <query-id> cancel
python -m abiyss doctor
```

Execution-capable tools are disabled by default. This is intentional.

## Recovery model

Before a tool side effect starts, ABIYSS must durably persist the Query as `running`. If that persistence fails, the side effect is not started.

After the side effect begins, a later persistence failure means the runtime cannot prove whether the side effect was fully recorded. The Query therefore becomes `recovery_required` and is never replayed automatically after a crash.

## Security boundary

The process executor and SkillLE are **guardrails, not a hostile-code sandbox**. Root execution is an explicit opt-in. When genuinely untrusted generated code is introduced, the execution boundary should move to the OS using dedicated identities, cgroups/systemd limits, seccomp and kernel-assisted filesystem resolution.

## Gemini boundary

The current default is `gemini-3.8-flash` with explicit `thinking_level="medium"`. Gemini function calls become Aqueries; ABIYSS executes them through QQ and returns `function_result` using the original call ID and `previous_interaction_id`. Auxiliary structured alignment requests are stateless.

Provider-side stateful Interactions have their own retention/data-storage semantics. See `docs/GEMINI.md` before production deployment.

## Documentation

- `docs/ARCHITECTURE.md`: architecture and state machine;
- `docs/CONTRACTS.md`: module contracts and invariants;
- `docs/SECURITY.md`: threat model, controls and residual risks;
- `docs/GEMINI.md`: current Gemini API integration;
- `docs/STUDY_NOTES.md`: engineering rationale and research notes;
- `docs/BUILD_REPORT.md`: build and validation record.

## Validation snapshot

The reconstruction workspace currently passes **68 tests** and a clean virtual-environment wheel installation with `pip check`. The live Gemini API was not exercised in the reconstruction environment, so network transport remains an explicit deployment validation step.
