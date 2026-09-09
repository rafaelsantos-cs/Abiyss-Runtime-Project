# WarPigs Algorithm Specification

## Scope

This document is the implementation-oriented summary used as the design oracle for the sterile WarPigs simulator. It deliberately excludes real-world propagation, persistence, evasion, exploitation, destructive actions, and autonomous process creation.

## Core state

Each WarPig is one independent entity.

```text
WarPig
├── identity
├── configuration[4]
└── lifecycle
```

Each of the four ordered configuration positions has one of three values:

```text
0 = MASKED
1 = ACTIVE
2 = STERILE_UTERUS
```

Therefore:

```text
|S| = 3
n = 4
|S^n| = 3^4 = 81
```

The four positions are internal components of the same entity. They do not encode the population size and a position cannot contain another WarPig.

## Configuration encoding

The canonical C++ engine maps the four ternary digits to a base-3 code in `[0, 80]`:

```text
code = (((p0 * 3 + p1) * 3 + p2) * 3 + p3)
```

The inverse is exposed as `wp_configuration_code_text` and always emits exactly four digits plus a NUL terminator.

Examples:

```text
0000 -> 0
0001 -> 1
0010 -> 3
0210 -> 21
2012 -> 56
2222 -> 80
```

## Population

Population size is an external simulation parameter.

```text
PopulationSupervisor(N)
        |
        +-- WarPig 1
        +-- WarPig 2
        +-- ...
        +-- WarPig N
```

No WarPig has a creation primitive. The native engine enforces a hard maximum of 1,000,000 simulation instances.

Generation uses an explicit seed so experiments can be replayed. Configuration selection is uniform over the three component states per position.

## Identity

Each native instance receives a telemetry identifier generated from the same seeded PRNG stream. Identity is not used as an authorization mechanism, persistence mechanism, anti-analysis mechanism, or routing key.

## Sterility

`STERILE_UTERUS` exists solely to preserve the three-state combinatorial model.

```text
2 != reproduction
2 == inert structural state named STERILE_UTERUS
```

There is no clone, spawn, fork, process creation, network, or filesystem operation reachable from the WarPig model or C++ simulation engine.

## Lifecycle

```text
CREATED
   |
   v
READY
   |
   v
RUNNING
  |  \
  |   \
  v    v
QUARANTINED  TERMINATED
  |
  v
TERMINATED
```

`TERMINATED` is absorbing.

The C++ engine performs batch preflight before mutating lifecycle state. If one entity cannot accept an action, the whole batch is rejected and `tick` remains unchanged.

## Engine contract

The C++ engine exposes a fixed-width C ABI so every other language consumes the same semantics:

```text
C++20
  |
  +-- C ABI
       |
       +-- Rust
       +-- Python
       +-- Julia
       +-- Go
       +-- C#
```

The ABI must remain language-neutral. Do not expose C++ object layout, references, exceptions, or compiler-specific enum representations.

## Error semantics

ABI calls return integer error codes. No exception may cross the C boundary.

```text
0   = success
1   = null pointer
2   = invalid argument
3   = configured limit exceeded
4   = index out of bounds
5   = invalid lifecycle transition
6   = caller buffer too small
255 = internal failure
```

Unknown error codes are treated as internal failure by bindings.

## Concurrency

The native engine uses a single mutex protecting the engine's mutable state. Read operations lock the same mutex, giving callers a consistent snapshot. `destroy` must not race with another API call using the same handle.

The engine's batch transition is serialized. It is intentionally not a lock-free design because deterministic state integrity is more important than speculative throughput at this layer.

## Hostile test model

The hostile harness should target invariants rather than add capabilities to the simulator:

- invalid actions;
- invalid indices;
- null pointers;
- short buffers;
- population-limit abuse;
- repeated state transitions;
- repeated construction/destruction;
- concurrent readers;
- deterministic replay;
- histogram conservation;
- source-capability checks.

AddressSanitizer and UndefinedBehaviorSanitizer are used in CI for the native implementation.

## Binding rules

Bindings must:

1. load an explicit native library path or follow a platform loader policy documented by the caller;
2. validate their own public arguments;
3. translate native errors into language-native errors;
4. never duplicate simulation rules;
5. release the native handle exactly once;
6. test ABI version before use;
7. treat the native handle as opaque.

## Non-goals and security boundary

This simulator is not a malware implementation or a host-agent implementation. It is a controlled model for studying population accounting, state transitions, FFI behavior, deterministic experiments, and security-test methodology.

Any future experiment that models propagation, resource exhaustion, persistence, or destructive behavior must remain synthetic and sandboxed. The current ABI intentionally has no primitives for those operations.
