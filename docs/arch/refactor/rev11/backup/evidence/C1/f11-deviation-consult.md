# C1 tenth deviation — F11: remaining `ProjectTypeStore` sub-accessors split by authority

Found while continuing to grow `ResolverObservation` past its 8 landed
methods, checking the 3 remaining `project_type_store()` sub-accessors
`project_semantic_dispatch` calls (`.indexed()`, `.shape_cache_db()`,
`.flow_slice()` — `project_generation` is done, `.semantic_graph()` was
already dispositioned by F9). Dispositioned via a fresh Codex xhigh
consult. Full consult prompt/output:
`/tmp/c1-deviation5-consult-prompt.md` / `/tmp/c1-deviation5-consult-output.md`
(not committed — ephemeral scratch; this file is the durable record).

## Finding

Each of the 3 accessors has exactly one non-test production call site in
`project_semantic_dispatch`, and each does something structurally
different from the 8 methods already built: they call BACK INTO the
store's own admission/protocol methods, passing `self.ctx`
(`&dyn ResolverContext`) along:

```rust
// build.rs:3403
let artifact_store = self.ctx.project_type_store().indexed();
let augmenter_set = artifact_store.ensure_augmentation_index_populated(&key, resolve_rel, overlay_discriminator);

// semantic_source.rs:1178
let cache = self.ctx.project_type_store().shape_cache_db();
let value = cache.with_owner_scope(self.ctx, |scope| { /* peek, warm/cold dispatch, fact emission */ });

// flow_return.rs:2036
let flow_slice = self.ctx.project_type_store().flow_slice();
let lowered = crate::cache_runtime::lookup(flow_slice.hash_node(), slice_key.clone(), self.ctx);
```

`ShapeCacheDb::with_owner_scope`'s own signature takes `&dyn
ResolverContext` as a direct parameter — the same shape
`ProjectSemanticDispatch` itself has (F8's original finding), raising the
question of whether these three stores are ALSO "wrongly homed" query-time
machinery (F10-style) or genuinely session-owned cache substrate that
merely needs a narrower observation surface.

## Disposition: split by store, ADOPT-NOW for the ownership ruling; DEFER trait growth

**The three stores do NOT share one disposition** — unlike F9's single
`SemanticGraphStore` ruling, this is per-store:

| Store | Verdict | Defining home |
|---|---|---|
| `FileArtifactStore` | **ADOPT-NOW: stays; relocation REJECTED** | `verter_session` |
| `ShapeCacheDb` | **ADOPT-NOW: stays; wholesale relocation REJECTED** | `verter_session` |
| `FlowSliceStores` | **ADOPT-NOW: relocates (F9-shaped), after splitting its producer** | Type/cache core → `verter_semantic`; retained-snapshot producer stays in `verter_session` |

### `FileArtifactStore` — stays

`ensure_augmentation_index_populated` doesn't actually take
`ResolverContext` (a generic relative-resolution callback instead), but
is still unsuitable as an observation method: it scans host artifacts,
MUTATES the augmentation index, advances membership/generation state,
emits audit events, and publishes cache state
(`file_artifact_store.rs:3488`). The eventual kernel-facing shape:

```rust
fn module_augmentation_index(
    &self,
    query: &ModuleAugmentationQuery,
) -> AttemptOutcome<ModuleAugmentationIndexObservation>;
```

`ModuleAugmentationIndexObservation { fingerprint: Hash16, contributors:
Arc<[AugmentationContributorObservation]> }` — `Complete` with an empty
contributor array is the stable negative fact; not-yet-materialized is
`NeedInputs` via a dedicated `InputKey::ModuleAugmentationIndex` (not
`FileContent`). `FileArtifactKey`/`AugmenterSet`/exact-key self-healing do
NOT cross — the session side heals/materializes before constructing the
immutable observation. `ensure_augmentation_index_populated`/
`populate_augmenter_set` remain session-only write/admission operations.
Relative target resolution remains a semantic-owned algorithm, not a
resolver callback exposed through `ResolverObservation`. (This is the
"third shape" the consult prompt asked about: dependency-neutral read
observation ABOVE the store, population/write maintenance BELOW it.)

### `ShapeCacheDb` — stays, but its cold ALGORITHM relocates

The `&dyn ResolverContext` parameter is evidence of an F10-LIKE authority
mixture, but NOT evidence the whole DB is wrongly homed.
`with_owner_scope` (installs session fact tracing + cacheability probing),
`peek` (generation/fact validation, fact bubbling), and
`get_or_compute_in_scope` (session singleflight + admission) all remain
session lifecycle/cache policy (`component_meta_caches.rs:1790`,
`:1875`). The SEMANTIC ALGORITHM inside the closure at
`semantic_source.rs:1178` — same-generation seed checking, graph
reduction, partiality — is what relocates. Eventual kernel-facing shape:

```rust
fn cached_synthetic_binding_shape(
    &self,
    key: &SyntheticBindingShapeKey,
) -> AttemptOutcome<Option<CachedSyntheticBindingShape>>;
```

`Complete(None)` means an ORDINARY optional cache miss — it must NOT
become `NeedInputs`. The kernel computes cold directly and records a
`ShapeCacheAdmissionCandidate` in its ATTEMPT OUTPUT; the session driver
validates/adopts that candidate afterward. This avoids both bad outcomes:
exposing the universal `ShapeCacheDb` capability to the kernel, and
leaving the semantic cold algorithm inside a session cache closure. Also
respects that `ShapeCacheDb` is NOT exclusively engine-private — it has
another production consumer, `meta_resolve/projectors/output_sink.rs`.

### `FlowSliceStores` — relocates (closer to F9), after a producer split

Closer to F9's `SemanticGraphStore` than the other two: its graph/hash/
lowered stores are used EXCLUSIVELY by the `FlowReturn` engine path,
cache semantic `FunctionFlowGraph`/`FlowSliceIR` products, and have no
independent host-wide consumer — project-global LIFETIME does not imply
session defining OWNERSHIP (F9's own distinction). But the current type
cannot move blindly: it contains `RetainedSnapshotSkeletonSource`, whose
impl calls `ensure_indexed_ready_serve` through `ResolverContext`
(`flow_slice_node.rs:155`), combined with the graph/hash/lowered engine
stores in the SAME `FlowSliceStores` struct (`flow_slice_node.rs:526`).
Correct split:

- Relocate the LOGICAL engine store: keys, `FunctionFlowGraphStore`, hash
  node, lowered node, budget state, and their content-addressed
  memoization.
- Keep retained-snapshot/`DeclBodyMemo` access in `verter_session`.
- Supply `FunctionBodySkeleton` as an immutable input:
  `fn function_body_skeleton(&self, key: &FlowFunctionObservationKey) ->
  AttemptOutcome<Option<Arc<FunctionBodySkeleton>>>`.
- Refactor the relocated cache compute to accept that skeleton directly —
  it must NOT receive `ResolverContext`.
- `ProjectTypeStore` keeps constructing/invalidating the runtime instance,
  as `Arc<verter_semantic::...::FlowSliceStores>` or a member of a
  `SemanticKernelStores` bundle (F9's own anticipated shape).

This is a structural C1 relocation the dispatcher SCC needs — NOT a
change to flow semantics forbidden by C1's D-track (flow) boundary.

### Corrected scope text (F11)

> **F11 — remaining `ProjectTypeStore` sub-accessors split by authority. ADOPT-NOW.**
>
> `project_semantic_dispatch` must not gain `ResolverObservation` methods
> mirroring `.indexed()`, `.shape_cache_db()`, or `.flow_slice()` directly.
>
> - `FileArtifactStore` remains the session-owned authoritative per-file/
>   content and augmentation-index store. Population, publication,
>   exact-key self-healing, membership epochs, generation bumps, and audit
>   emission remain session-only. The relocated kernel consumes a
>   dependency-neutral, target-specific `ModuleAugmentationIndexObservation`
>   through `AttemptOutcome`; no `FileArtifactKey`, `AugmenterSet`, store
>   handle, resolver callback, or write capability crosses.
> - `ShapeCacheDb` remains the session-owned universal shape-cache and
>   admission authority. `with_owner_scope`, fact-signature validation/
>   bubbling, cacheability probing, singleflight, and publication remain
>   session-side. The synthetic-binding cold-reduction ALGORITHM relocates
>   with `ProjectSemanticDispatch`; the kernel may consume a narrow
>   validated synthetic-binding cache hit and return a cache-admission
>   candidate as attempt output. It never receives `ShapeCacheDb` or
>   invokes `with_owner_scope`.
> - The flow-slice graph/hash/lowered stores are engine-owned state and
>   relocate dependency-neutralized into `verter_semantic`, held at
>   project lifetime by `ProjectTypeStore` like `SemanticGraphStore`.
>   `RetainedSnapshotSkeletonSource` and every retained-parse/`DeclBodyMemo`
>   operation remain in `verter_session`; the relocated flow store
>   consumes an immutable `FunctionBodySkeleton` input and never takes
>   `ResolverContext`.
> - Do not add any of these surfaces to `ResolverObservation` until their
>   dependency-neutral DTOs, missing-input keys, and attempt-output
>   admission sidecars are closed in the disposition table.

## Explicit instruction

"Do not defer the ownership ruling, but DO defer trait growth." The low
call-site count (one each) is a reason to keep the boundaries narrow, not
a reason to let three store-shaped capabilities harden into the interface
prematurely. None of `module_augmentation_index`/
`cached_synthetic_binding_shape`/`function_body_skeleton` are implemented
this round — this consult records the OWNERSHIP disposition only.
