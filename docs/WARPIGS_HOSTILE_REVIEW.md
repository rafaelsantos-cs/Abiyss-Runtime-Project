# WarPigs Hostile Review

## Purpose

This review treats the sterile WarPigs implementation as hostile software even though the implementation is deliberately inert. The objective is to find ways in which malformed input, concurrency, language boundaries, build assumptions, or accidental feature creep could violate the simulator's safety invariants.

The review is intentionally separate from ABIYSS runtime integration.

## Threat assumptions

The tester may control:

- all public API arguments;
- action values and indices;
- random seeds;
- population limits within and beyond configured bounds;
- caller-provided output buffers;
- lifecycle sequences;
- binding lifecycle such as repeated close/drop;
- build configuration and platform-specific loader paths.

The tester may not receive real host capabilities from the simulator. If a scenario requires a real process, network connection, filesystem mutation, or replication, it is considered a simulator design failure rather than a feature to add.

## Findings and fixes

### F-01: Native ABI enum representation

The first native ABI exposed C++-style enum types through the C boundary. That is unnecessarily dependent on language/compiler representation.

**Fix:** ABI now uses fixed-width `uint32_t` discriminants, explicit constants, and `WP_ABI_VERSION`.

### F-02: Batch partial mutation

A lifecycle action applied to a population could theoretically leave some entities advanced if validation were performed during mutation.

**Fix:** the native engine preflights every entity and mutates only after the complete batch has been validated. A failed batch leaves lifecycle state and tick unchanged.

### F-03: FIFO/blocking test path

Security tests using special files can accidentally block before the program reaches its intended validation boundary.

**Fix:** resource/path attack fixtures use non-blocking operations or synthetic failure paths so the test itself cannot hang the harness.

### F-04: FFI ownership

A native handle crossing into a higher-level language must have one clear owner and exactly one destruction path.

**Fix:** Rust owns the opaque pointer through `NonNull` and `Drop`; Julia uses an explicit mutable closed flag and an idempotent finalizer; Python exposes an explicit context manager.

### F-05: Transitive C++ header assumptions

The native source depended on headers indirectly declaring a utility type.

**Fix:** identity length calculation is now self-contained, and the native build declares its thread dependency explicitly.

### F-06: Native memory pressure

The first C++ representation stored every telemetry identifier as a dynamically allocated string. Large populations would therefore create unnecessary allocator pressure.

**Fix:** identifiers use fixed inline storage inside each simulated instance. The native engine remains capped at 1,000,000 instances.

### F-07: Binding/build divergence

Go and Julia initially referenced different native build directories from their CI jobs.

**Fix:** every binding CI job builds the native shared library itself and consumes the resulting artifact from that exact job.

### F-08: Sterile capability leakage

A simulator that is supposed to be inert must not gradually accumulate process, socket, dynamic-loading, or filesystem primitives merely because they are convenient for testing.

**Fix:** source guard tests reject selected host-capability tokens in the native simulator and Python simulator packages. The C++ core does not expose host operations.

## Residual risks

### R-01: Native handle destruction races

`destroy` is not safe to race against another call using the same opaque handle. This is a normal ownership contract, but bindings and callers must preserve it.

### R-02: PRNG is not cryptographic

`std::mt19937_64` is used only for deterministic experiment generation. It must never be presented as an unpredictable identity or security token.

### R-03: C++ ABI stability

ABI version `1` is explicit, but binary compatibility across future versions is not automatic. Any breaking change must increment the ABI version and keep old bindings pinned until migration is tested.

### R-04: Hostile code is outside scope

The sterile engine is not a sandbox. It deliberately has no hostile-code capability. If a future experiment needs to model hostile execution, isolation must be supplied outside this engine by an OS-level test environment.

### R-05: Binding availability

C++ is the authoritative implementation. A language binding that compiles is not proof of semantic parity. Every binding needs a smoke test against the same ABI and, where practical, the same seed/configuration oracle.

## Exit criteria for this review

The review is not considered clean merely because unit tests are green. The current minimum is:

```text
native build              PASS
native unit tests         PASS
hostile state fuzz        PASS
stress test               PASS
ASan + UBSan              PASS
Rust FFI tests            PASS
Python C-ABI smoke        PASS
Go C-ABI smoke            PASS
Julia C-ABI smoke         PASS
C# binding build          PASS
source capability guard  PASS
```

A future round should add thread sanitizer coverage and a language-parity test that compares a fixed vector of C++ outputs across every binding.
