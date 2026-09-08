# ABIYSS v0.1 Security Model

## Security objective

ABIYSS is an agent runtime that may eventually control a real Linux system. The main security boundary is therefore **between model intent and privileged execution**.

The model is untrusted with respect to authorization. It can request a tool, but only local ABIYSS policy can authorize that tool.

## Threat model

The v0.1 design considers:

- malformed or oversized model arguments;
- unknown or role-confused tool calls;
- duplicate function-call replay;
- QuPs corruption and path substitution;
- crash/restart ambiguity;
- prompt injection carried by tools or memory;
- Python import and environment poisoning;
- symlink/path attacks;
- shell and interpreter injection;
- unbounded stdout and file-descriptor exhaustion;
- timeout/process-group leakage;
- memory write races and duplication;
- secret leakage into logs or persistent memory;
- excessive model agency.

## Controls

### Complete mediation

Every tool call passes through registry validation. Tool type, argument schema and policy are checked locally. A model response is never accepted as permission.

### Queue isolation

Model-generated function calls become Aqueries and enter QQ. There is no direct Gemini-to-tool path. Aquery precedence and Squery preemption are scheduler rules, not model decisions.

### Durable side-effect protocol

The sequence is:

```text
admit Query
   |
   +--> durable QUEUED
   |
   +--> durable RUNNING
   |
   +--> execute external side effect
   |
   +--> durable result/checkpoint
```

If the `RUNNING` record cannot be persisted, execution does not begin. If persistence after a side effect fails, the Query becomes `recovery_required` in memory and the durable state remains evidence that the operation may have happened. Restart never automatically replays ambiguous work.

### QuPs integrity

Canonical JSON + SHA-256 detect accidental or unauthorized modifications that do not also possess a way to rewrite the digest. SHA-256 alone is not authentication and must not be presented as such.

### Process execution

Arbitrary execution is off by default. When explicitly enabled:

- argv is passed directly, never through a shell;
- the executable must be absolute and allowlisted;
- generic shell/environment/interpreter launchers are rejected;
- the environment is rebuilt from a deterministic baseline;
- loader injection variables such as `LD_PRELOAD`, `LD_LIBRARY_PATH`, `PYTHONPATH`, `BASH_ENV` and similar variables are rejected;
- output, arguments, file descriptors and wall-clock/CPU budgets are bounded;
- the child starts a new process group so timeout cleanup can target the group;
- Linux `PR_SET_NO_NEW_PRIVS` is requested;
- root execution requires explicit configuration.

### SkillLE

Skill manifests are bounded. Paths are checked for symlink escape, all files must match the declared file set, file hashes are verified, entrypoints are restricted, and root-enabled skills require a private root-owned tree.

SkillLE is explicitly **not** a hostile-code sandbox. Python subprocess restrictions cannot create a security boundary against a deliberately malicious process with sufficient host capabilities.

### Memory

Sensitive memories are omitted from ordinary reads. Tool metadata can prohibit persistence of sensitive results. Source IDs and uniqueness constraints provide provenance and idempotence.

### Audit

Audit events use recursive secret-key redaction and are written synchronously. The audit path rejects symlink redirection and uses no-follow opening where supported.

## Residual risks

### `preexec_fn`

Python documents that `subprocess.Popen(preexec_fn=...)` is unsafe in threaded applications because the child can deadlock before `exec`. ABIYSS keeps this hook minimal, but it is a known residual risk. A production implementation should move resource setup to an OS-level execution helper where possible.

### Kernel/filesystem races

User-space `Path` checks cannot eliminate every time-of-check/time-of-use race. A future privileged filesystem tool should use kernel-assisted resolution such as `openat2()` with restrictive resolution flags.

### Root

UID 0 is not a sandbox. A root ABIYSS process should eventually delegate untrusted generated execution to a separate service identity with explicit capabilities/resources.

### Provider data retention

Stateful Gemini Interactions are stored by the provider by default. The main agent loop intentionally uses stateful chaining because `previous_interaction_id` is part of the agent contract, so deployment must account for provider retention. Auxiliary Sleep alignment calls are stateless.

## Deployment posture for v0.1

The safest default is:

```text
allow_exec = false
allow_root_exec = false
small tool registry
no arbitrary filesystem mutation
no arbitrary package manager
no shell
```

Broader control should be added only alongside an OS-level containment layer and a much larger adversarial test program.
