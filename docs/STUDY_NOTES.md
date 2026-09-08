# ABIYSS Study Notes

These notes capture the engineering facts that shaped v0.1 so future work starts from evidence instead of memory fragments.

## 1. Agent boundary

An LLM can choose a tool but cannot safely be the authority that grants itself permission to execute it. ABIYSS therefore separates cognition from authorization and execution.

The important architectural invariant is:

`model output -> local Query -> durable state -> QQ -> authorized tool -> result`

## 2. Why QQ is linear

A single logical execution lane makes ordering and crash reasoning tractable. Aquery precedence is deterministic. Squeries remain interruptible without pretending arbitrary side effects are preemptible.

The price is throughput. Parallelism can be introduced later only by adding explicit lanes/locks and redefining ordering guarantees.

## 3. Why persistence happens before execution

Suppose a tool performs an irreversible external action. If the runtime marks it `running` only after execution, a crash between execution and persistence can make the action invisible and cause accidental replay. Persisting `running` before the side effect makes the ambiguity visible after restart.

If the result checkpoint cannot be persisted after the side effect, the system must prefer duplicate-risk disclosure over duplicate-risk concealment. That is why the Query becomes `recovery_required` and is not automatically replayed.

## 4. Why SHA-256 is not enough

A hash proves that the current bytes match the hash. It does not prove who was allowed to change the file. If an attacker can write both content and hash, integrity is defeated. Authentication needs a separate trust anchor such as permissions, signatures or an authenticated datastore.

## 5. Why AST/SST are two policy planes

The query class is part of authorization. Keeping Aquery and Squery tool registries separate makes accidental role confusion testable. A silent maintenance query should not suddenly gain an action tool merely because a model emitted a matching name.

## 6. Sleep is deferred consolidation, not another queue

Sleep should not compete with foreground action work. It operates on durable evidence later. The key distinction is:

- fresh source observations can become new recaps;
- historical observations can inform repetition;
- a recap itself must not recursively become new input.

## 7. Stateful Gemini implications

The Interactions API can maintain server-side state using `previous_interaction_id`. This simplifies multi-turn agent orchestration and preserves the model's hidden reasoning continuity. The cost is provider-side retention. Therefore statefulness is a deployment/data-policy decision, not merely a coding detail.

Auxiliary structured alignment does not need that history, so v0.1 uses `store=false` for it.

## 8. Thinking level

Gemini 3.8 Flash supports low/medium/high thinking levels; `minimal` is not supported. ABIYSS exposes the supported choices instead of relying on an undocumented provider default.

`medium` is the v0.1 default because the runtime is intended for multi-step agentic work and should not make reasoning effort an invisible constant.

## 9. Process containment

A Python process wrapper with timeouts, process groups, environment filtering and resource limits is useful defense in depth. It is not equivalent to a VM, container, seccomp policy or separate OS identity.

Python also warns that `preexec_fn` is unsafe in threaded applications because the child can deadlock. This is a strong reason to move low-level resource setup into a dedicated execution helper in a future release.

## 10. SQLite expectations

WAL improves reader/writer overlap but does not create unlimited writer concurrency. The current Memory layer uses bounded writes and a busy timeout. Scaling Memory beyond v0.1 should consider workload sharding, queueing or another persistence backend rather than assuming WAL removes contention.

## 11. Design lesson from recovery attacks

The most dangerous agent failures are not necessarily immediate crashes. A compromised tool can produce plausible but false observations that are later consolidated into memory and influence future decisions. Future War Pigs work should therefore test **epistemic contamination** in addition to process compromise.

## 12. Sources reviewed during reconstruction

Google AI for Developers: Interactions API, function calling, Gemini 3.8 Flash, thinking and thought-signature documentation.

Python standard library: `subprocess.Popen` documentation.

Linux kernel/userspace documentation: `openat2(2)` and path-resolution controls.

SQLite documentation: WAL and transaction/concurrency behavior.

PyPI: `google-genai` release metadata.
