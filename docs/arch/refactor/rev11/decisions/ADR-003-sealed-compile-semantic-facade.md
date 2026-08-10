# ADR-003 — Compile Semantics Use a Concrete Sealed Facade

**Status:** Accepted

## Context

The compiler needs optional project-aware semantic projections. An open trait would permit alternate module/type/runtime-classification semantics and create a second engine.

## Decision

The compiler accepts only a concrete Verter-owned `CompileTypeInfo<'_>` facade. Lifecycle variants are private/sealed first-party constructions. External integrations may supply captured observations through a data-only environment; they cannot implement semantic behavior.

All construction modes execute one profile-parameterized semantic kernel and one module resolver. The facade has no blanket `Send + Sync`; ownership/concurrency is a lifecycle policy.

## Consequences

Direct local, captured-project, in-memory, engine-snapshot, and validated-precomputed modes remain possible without opening semantics or coupling the compiler to `Engine`.

## Rejected alternatives

- public semantic trait object;
- compiler receives host/Engine/provider state;
- framework/compiler-local resolver fallback.
