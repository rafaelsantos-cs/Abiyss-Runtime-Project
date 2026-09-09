# WarPigs Definitive Design

## Status

Design and implementation branch: `warpigs-definitive`.

The WarPigs runtime in this document is a **sterile security-research simulator**. It is deliberately unable to self-replicate, execute arbitrary host programs, modify the filesystem, communicate with real network peers, or persist outside its owning simulation process.

## 1. Canonical model

One WarPig is one independent simulation entity with four ordered component positions.

Each position has exactly three states:

| Value | Name | Semantic role |
|---:|---|---|
| `0` | MASKED | Inert/masked component |
| `1` | ACTIVE | Inert simulated active component |
| `2` | STERILE_UTERUS | Structural uterus state with reproduction permanently disabled |

The configuration space is exactly `3^4 = 81` ordered configurations.

Examples:

```text
0000
0210
1022
2012
2222
```

A configuration belongs to **one** WarPig. It is not a population encoding and does not contain another WarPig.

## 2. Identity versus configuration

Identity and configuration are independent:

```text
identity:      wp-17-...
configuration: 2012
```

The identity is simulation telemetry. It is not an anti-analysis primitive and it carries no authority.

## 3. Sterility invariant

`STERILE_UTERUS` preserves the original three-state model without granting a reproduction primitive.

The following invariant must always hold:

```text
WarPig -> cannot create WarPig
WarPig -> cannot clone WarPig
WarPig -> cannot start a host process
WarPig -> cannot open network sockets
WarPig -> cannot write arbitrary host files
```

Population creation belongs exclusively to `PopulationSupervisor` or the native engine's bounded constructor. The WarPig object itself has no creation capability.

## 4. Population model

Population size is selected externally:

```text
PopulationSupervisor(N)
       |
       +-- WarPig #1
       +-- WarPig #2
       +-- ...
       +-- WarPig #N
```

The native engine enforces an absolute simulation maximum of 1,000,000 instances. The higher-level Python reference model defaults to 10,000. These are test/resource limits, not propagation rules.

A deterministic seed makes repeated experiments reproducible. The random generator affects component selection and telemetry identifiers, but does not alter lifecycle rules.

## 5. Lifecycle model

The canonical lifecycle is:

```text
CREATED -> READY -> RUNNING
                    |\
                    | \
                    v  v
              QUARANTINED  TERMINATED
                    |
                    v
               TERMINATED
```

The only legal transitions are explicit. Terminal state is absorbing.

The native simulation engine applies a batch action by pre-validating the entire population first. If any instance would reject the transition, the batch is rejected before any instance is changed.

## 6. Engine architecture

The canonical execution implementation is C++20 with a narrow fixed-width C ABI:

```text
                 C++20 WarPigs Engine
                         |
           +-------------+-------------+
           |             |             |
      population      lifecycle    telemetry
           |
        81-state
       configurations
```

The ABI uses fixed-width integer discriminants rather than exposing C++ or compiler-specific enum layout. ABI versioning is explicit.

The ABI intentionally exposes value-oriented operations only:

- create/destroy simulator;
- advance a validated lifecycle batch;
- read tick/population size;
- read one configuration;
- decode a configuration code;
- read one identity;
- read per-instance component counts;
- read lifecycle counts;
- read the 81-bin configuration histogram.

No ABI function performs process spawning, networking, filesystem mutation, dynamic code loading, or replication.

## 7. Polyglot boundaries

The preferred language roles are:

```text
C++   -> canonical simulation engine
Rust  -> supervisory wrapper and safety-oriented control surface
Python -> orchestration, test tooling, experiments
Julia -> statistical analysis and scientific workloads
Go    -> optional batch/experiment service tooling
JS    -> optional Electron visualization layer
C#    -> occasional tooling via C-compatible ABI
```

The C ABI is the common interchange boundary. This avoids implementing independent copies of the population algorithm.

Rust treats the foreign boundary as unsafe and wraps it with ownership and validation. Julia and Go consume the same exported C functions. C# uses P/Invoke against the same ABI. Electron is considered UI only and is not part of the simulation trust boundary.

## 8. Adversarial properties to test

The sterile WarPigs harness should attack the simulator rather than become a second simulator implementation. Priority properties are:

1. **Population conservation**: no action changes population size.
2. **No reproduction**: no event creates an entity.
3. **Lifecycle validity**: illegal transitions never partially apply.
4. **Determinism**: same seed and parameters produce the same configurations.
5. **Identity uniqueness**: generated identities are unique for one population.
6. **Bound enforcement**: impossible population sizes are rejected.
7. **FFI totality**: null pointers, short buffers, invalid indices, and invalid actions return errors rather than invoking undefined behavior.
8. **Termination closure**: terminated entities cannot reactivate.
9. **Histogram conservation**: all 81 bins sum exactly to population size.
10. **Language parity**: bindings report the same canonical values as the C++ engine.
11. **Source capability guard**: the sterile package contains no process/network/filesystem primitives.
12. **Batch atomicity**: failed batch validation leaves tick and lifecycle state unchanged.

## 9. What is intentionally absent

The real-world ideas that motivated the fictional threat model are not implemented as capabilities:

- self-replication;
- autonomous propagation;
- destructive actions;
- persistence/evasion;
- network discovery or exploitation;
- anti-analysis mechanisms;
- process-tree escape.

Those behaviors may be represented as simulated events in future research scenarios, but the native engine remains a state machine and data generator.

## 10. Research basis

The native engine uses `std::mt19937_64` with an explicit seed because reproducibility is a first-class test property. The C++ standard library documents this engine as a 64-bit Mersenne Twister pseudo-random engine with deterministic state under a fixed seed.

The polyglot boundary uses a C ABI because Rust's current documentation explicitly treats foreign interfaces as unsafe contracts and recommends wrappers around foreign calls. Julia provides direct C-library calls through `@ccall`, and Go provides cgo for C interoperability. These mechanisms make one canonical native implementation practical without duplicating simulation semantics.

References used during design:

- cppreference, `std::mersenne_twister_engine`: https://en.cppreference.com/w/cpp/numeric/random/mersenne_twister_engine
- Rust Reference, external blocks and ABIs: https://doc.rust-lang.org/reference/items/external-blocks.html
- Rust Edition Guide, unsafe extern blocks: https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-extern.html
- Julia manual, C Interface: https://docs.julialang.org/en/v1/base/c/
- Go cgo overview: https://go.dev/wiki/cgo
- Electron process model: https://www.electronjs.org/docs/latest/tutorial/process-model

## 11. Review conclusion

The earlier Python implementation remains useful as a reference model and regression oracle. The native C++ engine is the authoritative simulation implementation. Any future binding or visualization must be tested against the C++ oracle rather than defining independent semantics.
