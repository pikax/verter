# C1 fourteenth deviation — F14: FlowSliceStores relocation strips the session cache runtime

Found while scoping F11's third method (`function_body_skeleton`, the
`FlowSliceStores` relocation) for implementation, immediately after landing
F11's first method (`module_augmentation_index`, commit `ce8fb3b65`).
Dispositioned via a fresh Codex xhigh consult. Full consult prompt/output:
`/tmp/c1-deviation8-consult-prompt.md` / `/tmp/c1-deviation8-consult-output.md`
(not committed — ephemeral scratch; this file is the durable record).

## Finding

`FlowSliceStores`'s two live query nodes — `FunctionFlowGraphStore::
get_or_build` and `FlowSliceHashNode` (`cache_runtime/flow_slice_node.rs`)
— are built on the SHARED, GENERIC `crate::cache_runtime` framework:
`FlowSliceHashNode` implements `cache_runtime::node::ArtifactNode`, stores
`Arc<CacheEntry<FlowSliceHashOutcome>>`, and uses `cache_runtime::
singleflight::InflightTable<QueryFlightKey<...>>` for cooperative dedup.

By contrast `SemanticGraphStore` (`semantic_query_memo/*`, F9's already-
relocating precedent F11's "F9-shaped" language invoked) does NOT use any
of `ArtifactNode`/`CacheEntry`/`InflightTable` at all — confirmed by this
round's Part G audit: its ONLY `cache_runtime` touch anywhere is a bare
re-export (`NonAdmissionReason`, already dispositioned as a non-blocker).
`SemanticGraphStore` has its own bespoke memoization primitives instead.
F11's "relocates (F9-shaped)" text did not anticipate this structural
difference.

## Disposition: ADOPT-NOW, recorded as F14, F11 cross-references it

**F11's ownership verdict stands** (the project-global function-flow-
graph/slice-hash/lowered-body memo is semantic-engine-owned, relocates
into `verter_semantic`) **but "F9-shaped" is corrected**: the CURRENT
`ArtifactNode`/`CacheEntry`/`InflightTable` implementation does NOT
relocate. `crate::cache_runtime` remains a `verter_session` admission/
validation/lifecycle/request-concurrency FRAMEWORK — not itself the
ownership criterion for what uses it. Confirmed via a fresh implementor
sweep:

- **`ShapeCacheDb`** (`component_meta_caches.rs:1790,1875`) IS a direct
  client of the shared runtime (`SingleEntryArtifactNode` adapter,
  `DashMap<..., Arc<CacheEntry<...>>>` + `InflightTable<QueryFlightKey<...>>`)
  — reinforces F11's own "stays session-side" ruling for it.
- **`RouteDb`** (`resolver_core/route_db.rs:251`) does NOT use this
  framework — it has its own `ValidatedFactCache` + bespoke
  `SingleflightGroup`. This is the proof point: implementation substrate
  is not itself the ownership criterion (RouteDb and ShapeCacheDb both
  stay session-side despite one using the shared framework and one not).
- Full production `ArtifactNode` inventory: `FlowSliceHashNode`,
  `FlowSliceLoweredBodyNode`, `CompileOutputNodePureContent`, and the
  generic `SingleEntryArtifactNode` (used by declaration lookup,
  resolvability, owner collection, shape caches) — plus additional
  `QueryNode` users in compile-output/component-meta candidate caching,
  not yet individually classified (out of scope for this consult).

**Corrected rule going forward**: a session-owned cache keeps the
framework and exposes a narrow validated-value boundary (`ShapeCacheDb`,
`RouteDb` shape); an ENGINE-owned cache relocates its authoritative
memoization but STRIPS AND REPLACES the framework dependencies with a
bespoke, dependency-neutral mechanism (`SemanticGraphStore`'s shape,
now also `FlowSliceStores`'s). "Anything using `cache_runtime` stays
session-side" is explicitly REJECTED as a blanket rule — ownership is
decided per-store, not by implementation substrate.

### Corrected minimum `FlowSliceStores` relocation scope

**Relocate into `verter_semantic`:**
- Dependency-neutral flow keys/value types: `FunctionBodySkeleton`,
  `FunctionFlowGraph`, `FlowSliceHash` and lowered artifact types.
- `build_function_flow_graph` and the hash/lowering computation.
- `FunctionFlowGraphStore`, the hash and lowered-body memo state,
  `FlowSliceBudget` state, per-canonical-file eviction/lifecycle.
- A NEW flow-specific dependency-neutral memoization/dedup mechanism
  (NOT `ArtifactNode`/`CacheEntry`/`InflightTable`) preserving: one cold
  computation per cacheable key/miss window; cooperative waiting +
  panic-safe deduplication; the graph built from the immutable skeleton
  alone; hash-before-lower type-state ordering; stable + exact
  per-function content identity; empty fact rails (content-addressed key
  IS validity); `BudgetExceeded`/`ReturnOnly` results NEVER admitted; no
  busy-spin polling.

**Keep in `verter_session`:**
- `RetainedSnapshotSkeletonSource`, `DeclBodyMemo`, retained-parse/OXC
  access, `ensure_indexed_ready_serve`, `ResolverContext`/`StoreView`.
- The generic `ArtifactNode`/`CacheEntry`/`InflightTable`/`ComputeCtx`/
  `QueryFlightKey`/`ReadSetSignature`/session compat+generation types.
- Session-side `flow_slice_content` extraction, driver retry/
  materialization behavior.
- The session-private `BuildToolchainFingerprint` — if its invalidation
  axis is still required by the relocated store, replace it with a
  semantic-owned algorithm/schema identity, never export the session
  type unchanged.

**The seam**: `ResolverObservation::function_body_skeleton()` is backed by
a retained-snapshot/`DeclBodyMemo` OBSERVATION (immediate `AttemptOutcome<
Option<Arc<FunctionBodySkeleton>>>`) — NOT by peeking an authoritative
session-side `FlowSliceStores` facade (no such facade survives). The
relocated store consumes that `Arc<FunctionBodySkeleton>` directly and
never takes `ResolverContext`/`StoreView`. Attempt semantics: ready
skeleton -> `Complete(Some(skeleton))`; stable absence/key mismatch ->
`Complete(None)`; required retained/indexed state not yet available ->
`NeedInputs`, driver materializes + retries.

## Explicit instruction, followed

"ADOPT-NOW... record this as F14 and have F11 cross-reference it,
preserving the historical F11 evidence" — F11's own file is left as-is
(historical record); this file is the corrected authority for
`FlowSliceStores`'s scope going forward. NOT implemented this round — this
is genuinely larger, new-implementation work (designing a new bespoke
dependency-neutral memo/dedup mechanism from scratch, not a straight
relocation like `module_augmentation_index`'s) and deserves its own
dedicated implementation pass rather than a rushed landing at the tail of
an already-long round.
