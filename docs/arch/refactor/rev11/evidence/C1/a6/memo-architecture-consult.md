# C1 A6 correction — cross-request memo architecture consult

Read-only ruling against candidate `3d52d05c00a9b44894b4588ee50d2b6347233759`.

## Binding invariants

1. `ModuleResolverCore` is the sole semantic module-resolution authority; no second resolver or altered resolution semantics is permitted (`C1.md:127-154`; Stage-2 ruling `:456-493`).
2. Dependency-neutral algorithms and values move into `verter_semantic`, while live cache admission, retention, publication, compaction, and stores remain above it (`ARCH-ADDENDUM-C1-THREE-GAPS.md:77-79`; Stage-2 ruling `:512-560`).
3. Any feature admitting reusable cache entries is governed by the cache-runtime rules; keys must contain every deterministic input and published values must be immutable (`type-cache-architecture/SKILL.md:37-45,86-89`).
4. Cache eviction is memory-bound, and durable reusable caches require bounded retention (`type-cache-architecture/SKILL.md:615-680,1053-1060`).
5. C1 must preserve lifecycle/result semantics and close the locked A6 cell without reweighting it (`C1.md:99-168,170-191`; Stage-2 ruling `:467-493`).
6. A newly discovered ownership or public-compatibility change requires disposition before proceeding (`Stage-2 ruling:617-620`).

## Ruling

**FAIL.**

The memoized functions are deterministic over the keys shown, so their values do not require workspace-fact or basis invalidation. That narrow correctness point does not make cross-request retention admissible.

The proposed `Arc<ResolutionSharedMemo>` shared by cloned cores published into five `RwLock<FxHashMap<…>>` stores across requests. The stores had no capacity, eviction, generation reset, or owner-controlled retention; request-frame clearing did not clear them. This was a new unbounded live cache/retention authority inside `verter_semantic`, not request-local computation reuse.

The 24-case semantic evidence and A6 counter evidence cannot cure that ownership and retention violation; they do not prove bounded memory or legal cache ownership. The permitted in-scope option is request/`ResolveFrame`-local pure derivation reuse. Cross-request reuse requires a separately ratified, bounded cache design owned through the existing cache-runtime/retention authority (`U3.CACHE_FACT_MODEL` / `verter_session::bounded_query_retention`). The uncommitted cross-request memo was removed and was never committed.

===VERTER-RECEIPT-BEGIN===
LANE: c1-memo-architecture-consult
RESULT: FAIL
REVIEWED: 3d52d05c00a9b44894b4588ee50d2b6347233759
FINDINGS: 1
FINDING C1-MEMO-1 | P1 | crates/verter_semantic/src/resolver_core/resolve_frame.rs:95 | ResolutionSharedMemo creates an unbounded cross-request live cache and retention authority inside verter_semantic
===VERTER-RECEIPT-END===
