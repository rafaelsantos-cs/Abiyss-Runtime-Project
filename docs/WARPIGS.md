# WarPigs Adversarial Harness

WarPigs is a separate security-research layer for ABIYSS. It is **not imported by the runtime package** and is not part of the v0.1 runtime deployment surface.

## Objective

Repeatedly attempt to violate ABIYSS invariants around:

- Query persistence and recovery;
- queue ordering and admission;
- path traversal and symlink races;
- schema/type confusion;
- process execution and environment injection;
- output and argument resource bounds;
- SkillLE manifest integrity;
- sensitive-result boundaries;
- configuration validation;
- concurrency and restart semantics.

## Rule

A WarPigs failure is a security finding until disproven. A test failure caused by the harness itself is fixed in the harness and not counted as a runtime victory.

WarPigs never relies on an external network target and does not contain destructive system actions. Process attacks run against temporary test state and bounded local child processes.

## Current status

The hardening branch contains an adversarial regression suite in `tests/test_warpigs.py`. The suite is deliberately separate from the release snapshot and is run by CI on the hardening branch.

Known residual architectural risk: Python's `subprocess.preexec_fn` is unsafe in multi-threaded applications. ABIYSS currently uses it only for bounded child setup and documents migration to a dedicated OS/helper execution boundary as a future hardening step. A true hostile-code sandbox should not rely on this runtime-level guardrail alone.

## Exit criterion

Hardening is mergeable only after the complete normal suite and WarPigs suite pass repeatedly in CI, with no unexplained failures, and after every discovered security issue has either been fixed or explicitly accepted as a documented residual risk.
