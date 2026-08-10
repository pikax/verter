# ADR-012 — Stable Entity IDs, Session Handles, and Canonical Ordering Are Distinct

**Status:** Accepted  
**Decision owner:** public identity and deterministic serialization  
**Reopen only if:** a public contract explicitly changes which identifiers are stable across regimes/sessions.

## Context

Deterministic cross-regime IDs and cohort-local continuation handles serve different purposes. Treating a raw semantic node or session handle as a stable public ID creates lifetime and equality errors. Parallel insertion order can leak into output unless ordering authority is explicit.

## Decision

- `StableEntityId` is deterministic from a documented canonical/content-relative basis and may be compared across declared portable regimes;
- `SessionHandle` is opaque, owner/cohort-bound, generation-validated, and not compared across sessions;
- graph export, when requested, uses deterministic graph-local canonical IDs under its serialization profile;
- every observable collection has a total canonical order and deterministic tie-breaker;
- allocation address, concurrent interner insertion, hash iteration, worker completion, cache history, and owner-shard assignment cannot affect observable ordering;
- canonical serialization records its profile/domain and uses deterministic string/table/reference ordering.

## Consequences

- storage cohorts can reclaim without breaking public stable identity promises;
- direct/prepared/managed/native/WASM equality is well-defined;
- parallelism cannot leak nondeterminism.

## Rejected alternatives

- **Expose raw node IDs:** lifetime-bound and not stable.
- **Sort only at some adapters:** permits internal nondeterminism to affect hashes, maps, and caches.
