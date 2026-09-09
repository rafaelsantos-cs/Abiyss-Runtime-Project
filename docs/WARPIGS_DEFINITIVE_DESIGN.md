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

Population creation belongs exclusively to `PopulationSupervisor`.

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

The supervisor enforces a configured maximum. The current native engine hard-caps the absolute simulation population at 1,000,000 instances and defaults to 10,000 in the higher-level Python model.

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

The canonical execution implementation is C++20 with a narrow C ABI:

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

The ABI intentionally exposes value-oriented operations only:

- create/destroy simulator;
- advance a validated lifecycle batch;
- read tick/population size;
- read one configuration;
- read one identity;
- read lifecycle counts;
- read the 81-bin configuration histogram.

No ABI function performs process spawning, networking, filesystem mutation, or dynamic code loading.

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

The C ABI is the common interchange boundary. This avoids implementing five independent copies of the population algorithm.

Rust must treat the foreign boundary as unsafe and wrap it with ownership and argument checks. Julia can call the same C ABI directly. Go uses cgo when a direct native bridge is useful. Electron remains a user-interface process, not part of the simulation trust boundary.

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

The C++ engine uses an explicit seeded pseudo-random engine because reproducibility is a first-class testing property. `std::mt19937_64` is a standard library random engine with well-defined seeded state, which makes it appropriate for deterministic experiments. citeturn373024search1turn373024search3

The polyglot design intentionally uses a C-compatible ABI instead of exposing language-specific object layouts. Rust documents that foreign interfaces are inherently unsafe and that `extern "C"` is the conventional interoperability boundary; Julia documents direct calls to C-exported functions through `@ccall`; Go provides cgo for C interoperability. citeturn373024search4turn125373search1turn125373search4

For a future desktop UI, Electron's main/renderer/utility process model supports separating crash-prone or CPU-intensive work from the UI. The simulator itself should remain outside the renderer. citeturn125373search0

## 11. Review conclusion

The earlier Python implementation remains useful as a reference model and regression oracle. The native C++ engine is now the authoritative simulation implementation. Any future binding or visualization must be tested against the C++ oracle rather than defining independent semantics.
