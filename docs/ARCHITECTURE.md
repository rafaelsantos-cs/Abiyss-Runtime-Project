# ABIYSS v0.1 Architecture

## 1. Purpose

ABIYSS is a Linux-oriented agent runtime. An external cognitive model, currently Gemini, proposes work; ABIYSS owns validation, authorization, durability, scheduling, execution and recovery.

The central rule is:

> Model output is data that may request an action. It is never authorization by itself.

## 2. End-to-end flow

```text
                         Gemini / Model
                               |
                     function_call / turn
                               v
                         Query creation
                               |
                               v
                          QuPs durable
                               |
                               v
                               QQ
                 single logical execution lane
                    /                      \
              Aquery                    Squery
                 |                          |
                AST                        SST
                 |                          |
              action                   silent/internal
               tools                       tools
                 \                          /
                  +--------- Linux --------+
                               |
                         result/checkpoint
                               |
                               v
                       function_result
                               |
                               v
                      previous_interaction_id
```

Squery completion also feeds the deferred memory path:

```text
Squery observations
        |
        v
      Sleep
     /     \
   tick    review
    |         |
    v         v
 immutable   repetition
  recaps      evidence
    |
    v
 Memory / Sleep Key Alignment
    |
    v
 Skill Emergence -> SkillLE
```

## 3. Query model

### Aquery

An Aquery represents action-oriented work. In v0.1 every Gemini function call that reaches the runtime is translated into an Aquery.

### Squery

An Squery represents silent/internal work such as observation, maintenance or deferred consolidation. Squeries may contain multiple tool steps.

## 4. QQ scheduling

QQ is one logical execution lane. Its ordering is deterministic:

1. Aquery class has precedence over Squery class.
2. Higher numeric priority wins within a class.
3. Equal class and priority preserve FIFO admission order.

When an Aquery arrives while a Squery tool is running, QQ sets a cooperative preemption event. The current tool is not interrupted. After the tool returns, the Squery checkpoint is durably advanced; the Squery is paused/requeued and the Aquery is eligible to execute next.

This is intentionally boundary-based. The runtime never claims to have atomically interrupted an arbitrary external side effect.

## 5. Query state machine

```text
                 +-----------+
                 |  QUEUED   |
                 +-----+-----+
                       |
                durable running
                       v
                 +-----------+
                 |  RUNNING  |
                 +--+---+----+
                    |   |
            more    |   | terminal
            steps   |   |
                    |   +------------------+
                    v                      v
                PAUSED/QUEUED          COMPLETED
                    |
                    +-----> QUEUED

RUNNING + ambiguous post-side-effect persistence
                    |
                    v
             RECOVERY_REQUIRED
                /           \
             cancel       requeue
               |              |
               v              v
           CANCELLED       QUEUED
```

A durable `running` state at process restart is considered ambiguous and is converted to `recovery_required`. Automatic replay is forbidden because the runtime cannot prove that an external effect did not already occur.

## 6. QuPs

QuPs is the durable Query representation. Its envelope contains:

- `qups_version`;
- `kind`;
- complete Query snapshot;
- SHA-256 digest of the canonical JSON body.

Writes are atomic and restrictive. Reads use no-follow semantics where supported and reject non-regular files and oversized payloads.

The digest provides integrity, not authentication.

## 7. AST and SST

The tool registry is the authorization boundary. Each tool declares a Query type and argument schema. AST only accepts Aqueries; SST only accepts Squeries.

The runtime validates the same contract during admission and immediately before execution, reducing the window in which an in-memory object could become inconsistent with the declared tool policy.

## 8. Memory

Memory is SQLite-backed with WAL, foreign keys, `synchronous=FULL` and a busy timeout. Records include provenance and a sensitivity bit.

Daily keys are deterministic by local calendar day. Contextual keys have no fixed count limit, but individual key/context values are bounded.

Duplicate memory insertion is idempotent by `(key, fingerprint, source_id)`.

## 9. Sleep

Sleep uses two conceptual schedules:

- tick every 300 seconds;
- review every 1800 seconds.

A tick consumes only memories that are not already represented by an immutable recap. Recap identity is derived from the normalized source set and recap content, preventing the Sleep process from recursively consolidating its own recap output.

Review operates on historical observations as evidence. It must not convert history into fresh Sleep work merely because that history exists.

## 10. Sleep Key Alignment

Alignment starts with a deterministic baseline and may ask Gemini for semantic grouping using structured JSON output. The response is locally sanitized so it can only reference keys that existed in the input, each key appears at most once, and unmentioned keys become singleton groups.

Because this is auxiliary processing, the Gemini adapter uses `store=false` for structured alignment calls.

## 11. SkillLE

SkillLE verifies an executable skill before launch. The manifest is bounded and defines the entrypoint, version, file hashes and execution limits.

The loader rejects unsafe path components, symlinks, unhashed extra files and generic interpreter/launcher entrypoints. Root execution requires explicit policy; privileged skill trees must be root-owned and not group/world-writable.

SkillLE is not a full sandbox. A future production boundary should place untrusted generated code in a separate OS identity/service with cgroups/systemd limits and stronger kernel-level filesystem isolation.

## 12. Gemini boundary

The Gemini Interactions API is a provider adapter, not the runtime itself. ABIYSS repeats `tools`, `system_instruction` and `generation_config` on each interaction because these parameters are interaction-scoped. `previous_interaction_id` carries server-side conversation history for the agent loop.

The current v0.1 default is `gemini-3.8-flash` with `thinking_level=medium`.

## 13. Non-goals for v0.1

- War Pigs attack harness;
- arbitrary shell execution;
- unrestricted filesystem mutation;
- automatic replay of ambiguous side effects;
- pretending Python process controls are a hostile-code sandbox;
- provider-side persistence of Sleep alignment requests.
