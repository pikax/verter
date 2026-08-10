# ADR-008 — Deterministic Artifacts and Narrow Persistence Eligibility

**Status:** Accepted

## Context

Concurrency, cache warmth, native/WASM execution, and persistence can produce schedule-dependent or stale artifacts unless determinism and hermetic identity are explicit.

## Decision

Equal authoritative observations and result-affecting contracts produce equal Verter-owned observable outputs independent of legal schedule, worker assignment, cache warmth, or supported portable execution profile.

Persistent eligibility is limited to complete deterministic hermetic serializable values with complete compatibility, positive/negative fact, integrity, and size basis. OXC arenas, snapshot/session handles, transient cohorts, partial outcomes, and ambient-state-dependent values are never persisted.

An artifact publishes atomically with every mapping required to interpret it. Optional runtime/source-map products are not constructed when unrequested.

## Consequences

Persistence is optional acceleration, never correctness authority. Schedule-dependent map, diagnostic, ID, or serialization order is a defect.
