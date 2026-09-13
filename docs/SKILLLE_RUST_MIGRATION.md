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
      ├── tree traversal
      ├── size limits
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

When a `SystemPlaneClient` is configured, Python does not independently read `skill.json` to decide whether the artifact is trusted. It reconstructs the manifest returned by the Rust verifier and checks that the returned canonical directory is the directory requested.

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
- absolute entrypoints are restricted to trusted system prefixes and generic launchers are rejected.

## What `skill.verify` does not do

`skill.verify` is read-only. It does not execute the skill, modify the filesystem, or create an authorization token.

The current v0.1 skill execution path still has a verification-to-execution time-of-check/time-of-use window. Closing that window requires moving the execution boundary itself into the system plane, or otherwise binding verification to the executable object used for execution. That is deliberately a later hardening step rather than being hidden inside this migration.

## Tests

Coverage exists at three levels:

1. Rust unit tests for valid artifacts, hash tampering, extra files, symlinks, traversal and unknown manifest fields.
2. Python contract tests proving `SkillLoader` consumes the Rust response instead of reading the local manifest.
3. system-plane E2E tests that start the real Rust daemon and verify a real temporary skill, including tampering rejection.

The final acceptance criterion for this checkpoint is not a green mocked test alone. The Rust build, Clippy, Rust tests and real daemon E2E must all pass in CI.
