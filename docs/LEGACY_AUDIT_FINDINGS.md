# ABIYSS Legacy Runtime Audit Findings

## Purpose

This document records independent findings reported during review of the surviving Python v0.1 runtime. It is a reconstruction input, not a claim that every item has already been independently reproduced by this branch.

## Confirmed findings reported by external review

### 1. QuPs FIFO blocking risk

`QuPsStore._read_nofollow()` opened a path for blocking read without first requiring a regular file. A FIFO can therefore block the runtime before the read loop has a chance to reject it.

Required reconstruction invariant:

```text
path
  -> open/read cannot block on attacker-controlled special file
  -> regular-file validation
  -> bounded read
```

The Rust system-plane design must not inherit this pattern. Path access should use kernel-constrained resolution and then verify the resulting descriptor type before reading.

### 2. Unknown schema types must fail closed

The previous Python schema validator reportedly treated unknown type names as matching rather than rejecting them.

Required invariant:

```text
type in known schema vocabulary -> validate normally
type not in vocabulary      -> reject
```

There is no safe permissive default for authorization-adjacent schema validation.

### 3. Process output flood must terminate execution

The previous Python `ProcessTool` reader reportedly detected output overflow but continued draining until the normal timeout. That turns a bounded-output contract into a potentially long resource consumption window.

Required invariant:

```text
output > capture_limit
        -> signal cancellation immediately
        -> kill process group
        -> bounded cleanup
        -> return output-limit failure
```

`RLIMIT_FSIZE` is not sufficient to protect a captured stdout pipe. The reader/supervisor path is responsible for enforcing the pipe capture budget.

## Documentation/configuration findings

### 4. License metadata mismatch

The surviving `pyproject.toml` declares MIT while the repository `LICENSE` file is an Apache 2.0 template. This must be resolved before a release artifact is called distributable.

### 5. Sleep throttle edge case

`SleepManager.tick()` reportedly leaves `_last_tick` unchanged when the summarizer returns an empty/invalid result. The expected behavior needs to be explicit. A failed attempt should not accidentally create an uncontrolled retry loop.

### 6. Author metadata

The package metadata says `Rafael Alves`; repository ownership is `rafaelsantos-cs`. This may be intentional, but release metadata must be consistent and explicit.

### 7. `RLIMIT_FSIZE` limitation

The legacy runtime uses `RLIMIT_FSIZE` as one output-related guard. This does not constrain bytes written into a captured pipe. Future process supervision must treat pipe capture limits and filesystem write limits as different controls.

### 8. CLI dead code

The previous CLI contained an unreachable final return after a required argparse subparser set. Reconstruction should remove unreachable paths when the CLI is rewritten.

## Positive findings preserved

The independent review also confirmed several design choices worth retaining:

- the model is never itself authorization;
- Query durability precedes tool side effects;
- ambiguous post-side-effect persistence enters recovery rather than blind replay;
- atomic persistence includes file and directory durability;
- environment-loader poisoning is explicitly considered;
- SkillLE uses constant-time hash comparison;
- the project documents the fact that its Python executor is not a hostile-code sandbox;
- Gemini Interactions / continuation semantics are aligned with the current provider model according to the audit.

## Reconstruction policy

These findings are treated as design constraints for the multilingual runtime. The implementation should not merely patch the old Python code if the new architecture can make the unsafe state unrepresentable or move the relevant boundary into Rust.

Before the multilingual runtime is merged into `main`, the findings should be converted into explicit regression tests and release checks.
