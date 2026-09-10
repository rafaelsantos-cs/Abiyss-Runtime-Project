# ABIYSS Runtime v0.2 development architecture

This branch is an incremental architecture update. `main` remains the v0.1 reference implementation.

## Responsibility split

- **Python**: public runtime API, Gemini adapter, Query orchestration, QQ, tool authorization, lifecycle.
- **Rust**: memory-safe security primitives and canonical low-level validation that benefits from native implementation.
- **C++**: Linux/system primitives where direct OS integration and predictable low-level control are useful.
- **Julia**: isolated deterministic numerical analysis and scoring. It is not an authorization layer and has no direct OS authority.
- **Go**: optional long-lived worker/service processes and future IPC endpoints.
- **SQLite/SQL**: durable operational state and event indexing. Existing QuPs remains the durable query envelope and integrity format during migration.
- **C#**: optional integration surface only when a .NET-specific capability is actually required. It is not part of the mandatory runtime core.

## Native boundary

The first v0.2 boundary is JSON Lines over stdin/stdout. Each request is bounded and contains an explicit operation. Workers do not receive Gemini responses directly and cannot authorize a Query.

Native workers are configured explicitly and are optional. A system running only Python behaves as a valid v0.1-compatible runtime.

## Execution principle

The model remains outside the authorization boundary. Model function calls become Aqueries, are persisted by QuPs, enter QQ, pass the ToolRegistry and only then reach AST/SST execution. Native components are infrastructure called by trusted runtime code, not a bypass around QQ.

## Migration plan

1. Establish stable native protocol.
2. Add Rust security primitives.
3. Move selected Linux process primitives into C++/OS helpers.
4. Introduce operational SQLite tables without removing QuPs.
5. Add Julia analysis behind explicit data-only calls.
6. Add Go service mode only when persistent concurrency/IPC requires it.
7. Add adversarial tests and benchmark each boundary.
8. Only then consider promoting v0.2 into a release candidate.
