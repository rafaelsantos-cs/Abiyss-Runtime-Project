# WarPigs research layer

WarPigs is a separate security-research layer for ABIYSS. It is **not imported
by the runtime package** and is not part of the v0.1 runtime deployment
surface.

## Sterile model

The first implementation slice models one WarPig as four ordered component
positions. Each position has exactly three states:

| Value | State | Meaning in the simulator |
|---:|---|---|
| `0` | `MASKED` | Inert/masked component |
| `1` | `ACTIVE` | Inert simulated active component |
| `2` | `STERILE_UTERUS` | Structural uterus component with reproduction permanently disabled |

Therefore the configuration space is:

```text
3 states ^ 4 ordered positions = 3^4 = 81 configurations
```

Examples include `0000`, `0210`, `1022`, `2012`, and `2222`.

The `STERILE_UTERUS` state exists to preserve the original three-state model,
but it has no creation primitive. `WarPig.reproductive` is permanently false.
There is no method that clones an instance, starts a process, writes to the
network, or modifies the host.

## Population algorithm

Population is controlled exclusively by `PopulationSupervisor`:

```text
PopulationConfig(N)
        |
        v
PopulationSupervisor
        |
        +-- create WarPig #1
        +-- create WarPig #2
        +-- ...
        +-- create WarPig #N
```

A configured maximum bounds `N`. The default maximum is 10,000. The supervisor
can use a deterministic seed, making configuration-distribution tests
reproducible.

The population can therefore be selected independently of the instance model:

```text
N = 10
N = 100
N = 1,000
N = 10,000
```

Each generated instance receives a UUID-backed identifier for telemetry. The
identifier is not an anti-analysis or localization mechanism.

## Explicit non-goals

The sterile implementation deliberately excludes:

- autonomous reproduction;
- process creation or execution;
- network propagation;
- persistence outside the simulator's owning Python process;
- filesystem destruction;
- privilege escalation;
- evasion or anti-analysis behavior;
- interaction with real ABIYSS tools.

## Adversarial harness

The broader WarPigs security harness repeatedly attempts to violate ABIYSS
invariants around:

- Query persistence and recovery;
- queue ordering and admission;
- path traversal and symlink races;
- schema/type confusion;
- process execution and environment injection;
- output and argument resource bounds;
- SkillLE manifest integrity;
- sensitive-result boundaries;
- configuration validation;
- concurrency and restart semantics.

A WarPigs failure is a security finding until disproven. A test failure caused
by the harness itself is fixed in the harness and not counted as a runtime
victory.

WarPigs never relies on an external network target and does not contain
destructive system actions. Process attacks run against temporary test state
and bounded local child processes.

## Exit criterion

Hardening is mergeable only after the complete normal suite and WarPigs suite
pass repeatedly in CI, with no unexplained failures, and after every discovered
security issue has either been fixed or explicitly accepted as a documented
residual risk.

Known residual architectural risk: Python's `subprocess.preexec_fn` is unsafe
in multi-threaded applications. ABIYSS currently uses it only for bounded child
setup and documents migration to a dedicated OS/helper execution boundary as a
future hardening step. A true hostile-code sandbox should not rely on this
runtime-level guardrail alone.
