# ABIYSS v0.1 Build Report

## Build identity

- Version: `0.1.0`
- Project: ABIYSS Runtime Project
- Scope: Linux-oriented agent runtime foundation
- War Pigs: intentionally excluded from the v0.1 runtime package

## Reconstruction status

This release is a deliberate reconstruction from the recovered ABIYSS context. Historic details that could not be recovered were not silently invented. The runtime architecture is therefore documented as a current v0.1 contract rather than as a claim that every old implementation detail has been recovered verbatim.

## Validation completed in the development workspace

- **68 tests passed** with `PYTHONPATH=src python -m pytest -q`.
- `python -m compileall -q src`: passed.
- Wheel build with local build tooling and no dependency download: passed.
- Clean virtual-environment installation of the generated wheel: passed.
- Clean-environment `python -m abiyss doctor`: passed.
- Clean-environment `python -m abiyss status`: passed.
- Clean-environment `pip check`: passed with no broken requirements.
- Generated wheel SHA-256: `0f5314c6bbfce89f830a995ccb2891b103c0002e2a6decf46f39fc0d3651c542`.
- Gemini live API: **not** exercised in this environment because external API access and credentials are not assumed.

The GitHub repository contains a critical regression/security suite and GitHub Actions configuration. Connector-authored pushes may not automatically trigger Actions in every GitHub configuration, so the local clean-environment result above is the authoritative reconstruction validation unless a live CI run is observed.

## Architecture under test

```text
                         Gemini / Model
                               |
                     function_call / turn
                               v
                            Query
                         /          \
                   Aquery          Squery
                      |                |
                      +-------> QuPs <+
                                |
                                v
                               QQ
                     single logical execution lane
                       |                 |
                      AST               SST
                       |                 |
                 action tools      silent tools
                       |                 |
                       +--------+--------+
                                |
                                v
                           Linux system

Squery observations -> Sleep -> immutable recap -> Memory -> repetition evidence -> Skill Emergence
```

Model output is never itself an authorization decision. Every executable model function call becomes an Aquery, is durably admitted into QuPs, and passes through QQ before the corresponding tool plane can run.

## QQ invariants

1. Aquery class precedes Squery class.
2. Higher numeric priority wins within the same query class.
3. Equal class and priority preserve FIFO admission order.
4. QQ has one logical execution lane.
5. Aquery arrival while an Squery is executing sets a cooperative preemption request.
6. The currently running tool is allowed to finish; no false claim of mid-tool interruption is made.
7. A multi-step Squery resumes from the durably recorded next step.
8. Running-state persistence happens before a side effect.
9. A post-side-effect persistence failure causes `recovery_required` and forbids automatic replay.
10. Duplicate `(interaction_id, tool_call_id)` identities are not executed twice.

## QuPs invariants

QuPs is a versioned durable envelope containing the full Query snapshot and a SHA-256 digest of its canonical JSON body. Writes are atomic. Reads reject malformed JSON, non-finite numbers, over-size payloads, unsafe query IDs and non-regular/symlinked objects.

The hash is an integrity check, not authentication. An attacker who can rewrite the entire envelope can also recompute the digest; authorization therefore remains a separate responsibility.

## Memory and Sleep

Memory uses SQLite with WAL, `synchronous=FULL`, foreign keys and a bounded API. Daily and contextual keys are idempotent. Memories carry source IDs and a sensitivity flag.

Sleep separates quick consolidation from periodic review:

- tick: 300 seconds;
- review: 1800 seconds.

Recaps are immutable by source-set/content fingerprint. Historical observations may contribute to repetition evidence without being reintroduced as fresh consolidation work.

## Tool and Skill security

The v0.1 default registry is intentionally narrow. `system.info` is an Aquery observation tool; `process.list` is an Squery observation tool; arbitrary process execution is disabled unless explicitly enabled.

When process execution is enabled, the implementation requires absolute executable allowlisting, rejects shell/interpreter launchers, does not invoke a shell, builds a controlled environment, limits arguments/output/file descriptors and time, starts a dedicated process group, and uses `PR_SET_NO_NEW_PRIVS` on Linux child setup where available.

SkillLE validates manifests, hashes, paths, file sets, entrypoints and resource bounds before execution. Root execution requires explicit policy and a privileged skill tree must be root-owned and non-group/world-writable.

These mechanisms are **guardrails, not a hostile-code sandbox**. Production execution of generated or untrusted code should move behind an OS-level helper with dedicated identity, cgroups/systemd resource controls, seccomp and kernel-assisted path resolution.

## Gemini integration

The adapter targets the current Interactions API shape. The model emits `function_call` steps; ABIYSS executes the requested local tool and submits `function_result` using the function-call ID and `previous_interaction_id`.

`gemini-3.8-flash` is the v0.1 default. Thinking is explicit and defaults to `medium`. Structured output is used for Sleep Key Alignment and is validated again locally. Auxiliary alignment requests are stateless with `store=false`.

The Interactions API is stateful by default when `store=true`. That means provider-side interaction retention is part of the data boundary for the normal agent loop.

## External references reviewed on 2026-09-08

- Google Gemini Interactions API overview and migration documentation.
- Google Gemini function-calling documentation.
- Google Gemini 3.8 Flash model and thinking documentation.
- PyPI metadata for `google-genai` 2.22.0.
- Python `subprocess` documentation concerning `preexec_fn` and threaded applications.
- Linux `openat2(2)` documentation for future path-resolution hardening.
- SQLite WAL documentation for reader/writer concurrency semantics.

## Known limitations

1. No live Gemini request was executed during this build.
2. `preexec_fn` remains a residual risk in a multithreaded Python process because Python documents possible deadlocks. The current code keeps the hook deliberately small, but this is not a final isolation boundary.
3. Path safety is partly user-space. A future privileged filesystem tool should use kernel-enforced resolution constraints such as `openat2`.
4. SQLite remains a single-writer database despite WAL.
5. Provider-side stateful Gemini interactions introduce retention/privacy considerations.
6. Arbitrary filesystem mutation, package-manager control and unrestricted shell access are intentionally outside v0.1.
