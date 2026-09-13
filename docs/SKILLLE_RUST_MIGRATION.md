# SkillLE → Rust migration checkpoint

## Scope

This checkpoint moves **skill verification** from the Python-only security decision into the Rust system plane. It does not claim to migrate all skill execution to Rust.

## Authority model

```text
Python SkillLoader
      │
      │ skill.verify(directory)
      ▼
Rust system plane
      │
      ├── manifest parsing
      ├── path policy
      ├── pinned system-root FD
      ├── openat2 resolution
      ├── tree traversal and bounds
      ├── symlink/special-file rejection
      ├── SHA-256 verification
      └── entrypoint policy
      │
      ▼
verified manifest
      │
      ▼
Python SkillLoader
```

The canonical verifier is `rust/abiyss-systemd/src/skill_secure.rs`, exposed as `crate::skill` from `lib.rs`. The previous verifier implementation has been removed so there is no second verification authority in the Rust crate.

When a `SystemPlaneClient` is configured, Python does not independently read `skill.json` to decide whether the artifact is trusted. It reconstructs the manifest returned by the Rust verifier and checks that the returned canonical directory is the directory requested.

## Path-security model

The verifier opens the configured system root and anchors subsequent untrusted paths at that file descriptor. Files and directories are opened with Linux `openat2(2)` using `RESOLVE_BENEATH`, `RESOLVE_NO_SYMLINKS` and `RESOLVE_NO_MAGICLINKS`, followed by `fstat()` checks. Regular-file contents are hashed from the opened descriptor rather than from a second path lookup.

This is stronger than a `resolve()`/`is_symlink()` check alone, because path traversal and symlink policy are enforced by the kernel at the object open boundary. The verifier also uses `O_NONBLOCK` before type inspection so a FIFO cannot block the verifier during `open()`.

## Rust verification limits

- manifest: 64 KiB maximum;
- files: 256 maximum;
- tree depth: 32 maximum;
- individual file: 64 MiB maximum;
- aggregate skill tree: 256 MiB maximum;
- entrypoint tokens: bounded;
- output/argument limits are part of the manifest policy;
- manifest fields are closed with `deny_unknown_fields`;
- skill members must be canonical relative paths;
- symlinks and special files are rejected;
- the manifest file itself is excluded from the hashed skill file set;
- exact file-set equality is required;
- SHA-256 mismatches are reported by path;
- SHA-256 strings are lowercase hexadecimal only;
- absolute entrypoints are restricted to trusted system prefixes and generic launchers are rejected.

## What `skill.verify` does not do

`skill.verify` is read-only. It does not execute the skill, modify the filesystem, or create an authorization token.

The current v0.1 skill execution path still has a verification-to-execution time-of-check/time-of-use window because Python eventually launches the entrypoint itself. Verification is therefore **not** an execution authorization. Closing this final window requires binding verification to the exact executable object used for execution, for example by moving execution into the system plane and using an FD-bound execution primitive such as `execveat(2)`/`fexecve(3)`, with explicit handling for scripts. That is a later hardening stage and is intentionally not hidden by this checkpoint.

## Tests

Coverage exists at three levels:

1. Rust verifier tests for valid artifacts, hash behavior, symlink rejection, FIFO manifest rejection, path normalization and bounded file types.
2. Python contract tests proving `SkillLoader` consumes the Rust response instead of reading the local manifest when the system plane is configured.
3. Real system-plane E2E tests that start the Rust daemon and verify temporary skills, including tampering rejection and worker/resource limits.

The acceptance criterion for this checkpoint is not a green mocked test alone. The Rust build, rustfmt, Clippy, Rust tests and real daemon E2E must all pass in CI.
