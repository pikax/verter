---
name: type-cache-architecture
description: "Verter fact-based cache architecture — env hash split, FileArtifactStore, content-addressed caches, query-identity caches, R1–R31 architectural rules, module augmentation, multi-candidate storage"
---

# Verter Fact-Based Cache Architecture

> AMENDMENT 2026-05-11-A — the integration branch for the fact-based cache
> overhaul is `refactor/semantic-db-overhaul` (the user-mandated rename of
> the older `fix/cutover-review-findings` branch). Same baseline; the swap
> is documentation-only.

Reference for the cache architecture in `verter_session`. Every architectural
rule referenced by tests, plans, and code comments resolves here. See
`docs/arch/fact-based-cache.md` for the per-cache-layer key composition table.

## Why this architecture exists

Verter's IR is shallow-Refs + content-hashed parse + typed-IR-only resolver.
The cache substrate matches: **lazy-validate, fact-granular, read-side
authoritative**. The old model was **eager-evict, file-coarse, write-side
authoritative**, producing ~100 ms cold latency that failed to warm across the
loop. Cache validation moved from a write-side side-effect into a read-side
fact check.

End-state rule:

> Source updates produce semantic fact diffs. Cache entries are
> validated against the exact facts they read. Reuse is the default;
> recomputation is the exception. Overlays are views over the base
> host, not host mutations. Fact validation is the cache-correctness
> oracle; a view/content token remains the concurrency oracle.
> Shared semantic materialisations are keyed by resolved declaration
> **slot** identity (content-free); versioned declaration identity
> lives inside cached values.

## Cache Runtime Hard Rules

Part of the `Cache Architecture (CRITICAL)` rule in `CLAUDE.md`. Binding for
the cache-runtime overhaul and any feature admitting cache entries.

1. Cache correctness is read-side authoritative. A warm hit is correct
   only after validation against the caller's current `StoreView`.
2. A cache key must include every deterministic input that changes the
   value. If not possible, the value is not cacheable.
3. Query-identity keys must not include content hashes, version hashes,
   or `fact_dep_signature`. Version identity belongs on the cached value.
4. The five env hash dimensions stay split. `parse_env_hash`,
   `resolve_env_hash`, `type_env_hash`, `lib_env_hash`, and
   `project_identity` must not be bundled into a single
   `project_config_hash`.
5. Empty and overflowed signatures are different states. Empty means
   dependency-free. Overflowed means valid result, non-cacheable result.
6. Tracer overflow, budget exhaustion, cancellation, generation
   supersession, incomplete self-rooting, and unresolved provenance all
   route through `ReturnOnly`.
7. `ReturnOnly` never publishes a cache entry, never registers
   reverse-index metadata, and never becomes a persistent artifact.
8. Reverse dependency graphs are not invalidation authority. They may
   support observability, prefetch, diagnostics, and targeted stale
   sweeps only.
9. Same-canonical edits must be caught by strict self-root validation,
   not by eager own-canonical drains.
10. Cross-file edits invalidate consumers lazily through recorded facts,
    not through reverse-dependent eviction cascades.
11. Base cache and overlay cache writes are separate. Overlay/session
    results must not populate base-only artifacts or persistent cache
    entries.
12. Pure artifacts may be persisted only when their keys contain all
    semantic, compiler, env, profile, plugin, and source-map-policy
    dimensions.
13. Fact-validated semantic query results are memory-only until every
    query family has audited strict self-root validation, complete env
    keys, and typed non-cacheable admission.
14. Singleflight is required for every cold cacheable node. Concurrent
    callers for the same key produce at most one cold computation per
    miss window.
15. Joiners on an in-flight computation must validate the winner's
    published entry against their own view before returning it.
16. Cache admission must be typed. Boolean flags, empty arrays, sentinel
    hashes, or side-channel `RefCell` state must not decide cacheability.
17. Cacheable entries must be immutable after publish. Mutation creates
    a new versioned value or a new artifact key.
18. A cache hit must not allocate audit payloads when no request
    accumulator is active.
19. Public APIs must expose cache semantics when behavior differs. A
    single ambiguous compile path must not hide `stateless`, `content`,
    and `session` behavior.
20. Benchmarks must report cache mode, source-map policy, batch shape,
    thread count, hit count, and fallback count. A benchmark without
    those dimensions is not an architecture signal.

The existing `Cache Architecture` guard cluster covers the current production
subset. The plan in `docs/arch/cache-runtime-overhaul-plan.md` names the
additional guards and discriminating tests that must land with each implementation
block before the corresponding rule becomes executable policy.

## Architectural rules (R1–R31)

### Mutation semantics

**R1.** `host.upsert(canonical, source)` is a cache-state no-op iff the
`(canonical, content_hash, parse_env_hash, resolve_env_hash, lib_env_hash)`
quintuple is unchanged. No cache mutation, no semantic invalidation, no
`bump_store_view_epoch`, no scheduler round-trip beyond the quintuple check.

**R2.** `upsert` means "the source changed." Cache eviction is an explicit
method with a stated scope; never a side effect of `upsert`.

**R3.** Eager *reverse-dependent* cache invalidation is forbidden. Cache
entries validate on read against the exact facts they recorded; cross-file
staleness is detected lazily through `fact_dep_signature` checks. An owner
upsert does NOT iterate `reverse_deps_for` to drain its dependents — no
reverse-dependent cascade. `smart_invalidate_dependents`, a
`reverse_deps_for`-driven eviction loop, `invalidate_canonical` from upsert,
and `*_db::invalidate_canonical` as a public API are all banned in production
(the `reverse_graph_not_wired_to_invalidation` +
`host_upsert_performs_no_reverse_dependent_eviction` guards enforce this).

Distinct from the banned cascade: the **own-canonical drain**.
`upsert_via_scheduler_with_priority` still drains the upserted canonical's
*own* query-identity caches at upsert time
(`resolver.runtime.evict_canonical(&canonical_id)`,
`project_type_store.evict_canonical(&canonical_id)`,
`resolved_type_cache().clear()`).

**Self-version rooting of query-identity caches.** Each query-identity cache
entry carries a self-root `FileWholeHash` for its keyed canonical(s) inside
`fact_dep_signature` (prepended by the central fact-signature helpers). The
warm-read validator decides how strictly that self-root is checked:

- The **component-meta query DBs** — `declaration_lookup_db`,
  `imported_registry_db`, `resolvability_db`, `owner_collection_db`,
  `prepared_target_db`, `prepared_surface_db`, `prepared_member_db`,
  `routed_expr_surface_db`, `materialize_memo_db` — validate the self-root
  **strictly** via `validate_fact_signature_with_self_roots` (passing the
  entry's keyed canonical(s)): a same-canonical content edit, or a keyed
  canonical the live store view no longer tracks, rejects the entry on both
  the warm-hit and post-compute-revalidation paths. `prepared_target_db` roots
  the active scope, the original declaring canonical, AND the FINAL routed
  declaring canonical (when the requested name re-exports through an
  intermediate module to a third file) — the entry carries an explicit
  `self_root_canonicals` set because the cache key encodes only the first two;
  the routed canonical's observed hash is read from the prepared-decl bundle
  (`PreparedDeclBundle::owner_whole_hash`) actually used for the value.
  `materialize_memo_db` additionally merges every canonical observed during
  materialization as a cross-file dependency fact. Cross-file *dependency*
  facts keep the lazy "untracked → accept" permissiveness — only self-roots
  are strict.
- The **`SemanticGraphStore` query nodes** validate their self-root
  **strictly**. Each query-node `MemoEntry` records its `self_root_canonicals`
  — the keyed canonical for `ResolveDecl` / `TypeOf` / `Instantiate` /
  `ResolveMacroPayload`, or the file-derived origin of every input node for the
  node kinds keyed by interned `SemanticNodeId`s (`ProjectPath` /
  `ProjectMember` / `IndexedAccess` / `KeyOf` / `MappedType` / `Conditional` /
  `NormalizeUnion` / `NormalizeIntersection`). The carrier is built by the
  provenance-pure producer `semantic_graph_read_set_signature` (prepends a
  self-root `FileWholeHash` per observed self-root, merges the traced fact set,
  returns `None` — non-cacheable — on a conflicting self-root hash or a traced
  `FileWholeHash` that disagrees with an observed self-root). The warm-read
  validator — `execute_cooperative`'s fast path, `get_validated`, the slow-path
  step-1 recheck, and the relation memo's `get_relation` — validates every
  self-root strictly via `validate_fact_signature_with_self_roots` /
  `ReadSetSignature::validate_with_self_roots`. `get_unvalidated` has no
  production warm-read caller (test/debug only).
- The **structural carriers** — `materialize_structure_db` and `ref_cycle_db`
  — validate their self-root **strictly**. Each entry carries an explicit
  `self_root_canonicals` set checked via `ReadSetSignature::validate_with_self_roots`
  on every warm read AND post-compute revalidation. `MaterializeStructureDb`
  self-roots on the materialise SUBJECT's declaration-origin file
  (`materialize_subject_origin_self_root`): a route-shaped subject
  (`Pick`/`Omit`/IndexedAccess carrier) self-roots on the EXTRACTED ROUTE
  ROOT's file (e.g. `Shared` in `Pick<Shared,'id'>`, observed via
  `authoritative_current_content_hash`) plus the traced read-set, while a
  non-route subject self-roots on the `base` node's declaration-origin file
  (`SemanticGraphStore::node_scope`'s `NodeScopeId::File`). The consumer
  materialise scope is NEVER a self-root — a value's identity does not
  depend on which consumer reached it (R7 cross-owner reuse); rooting a
  route-shaped value on the first producer's wrapper file would falsely
  reject every other owner's warm reuse. The content-free
  DB cache key `MaterializationCacheKey` carries no consumer-scope dimension at
  all; the SEPARATE per-thread recursion identity `MaterializeRuntimeKey`
  excludes `scope_canonical_id` from its `Hash`/`PartialEq`. A non-route subject
  whose base origin is `Global` (or a route-shaped subject whose extracted
  root has no authoritative content hash) seeds no strict self-root —
  validity then rides on the traced read-set alone; a root-less anonymous
  subject keys no DB slot (uncached). `RefCycleResultDb` roots the BFS root file plus every visited
  declaration's file (recorded value-side in `self_root_canonicals`, NOT in the
  content-free `RefCycleResultKey`). The provenance-pure
  producers `materialize_structure_read_set` / `ref_cycle_read_set` lead the
  carrier with one observed-hash `FileWholeHash` per self-root and merge the
  traced fact set on top; a fence `RouteGeneration` dependency, or a fence
  `WholeHash` that conflicts with an observed self-root, routes the value
  through `ComputeAdmission::ReturnOnly` (valid result, no shared admission).
  `RefCycleResultDb` has no generation-equal fast return — every `peek`
  validates strictly. Its cold path is the transitive-cycle BFS for
  parameterized generic helpers (`ref_root_reaches_transitive_cycle_node`),
  gated by `ComputeAdmission` cooperative admission: an overflowed/unrootable
  signature returns the computed bool through `ComputeAdmission::ReturnOnly`
  WITHOUT admitting and WITHOUT a second uncached BFS. The BFS dispatches
  `Instantiate { base, args: [], context: InstantiateContext {
  projection_reduction, resolve_env_hash } }` with
  `context.projection_reduction.mode = ProjectionMode::Skeleton`
  (Skeleton-mode instantiation, empty args) so unbound type parameters become
  `TypeParam` shells — preserving Conditional branches that would otherwise
  collapse to `never` for unbound generics. `current_route_surface_hash` is the
  single route-fact production helper: the current-content `IndexedReady`
  artifact is the SOLE route-surface authority (no secondary route-surface
  artifact), gated by `indexed_surface_is_current` — edge currency plus the
  `project_generation` stamp for surfaces with cross-file edges.

The own-canonical drain serving the edited file's own caches is retained only
as a redundant fast eviction now that every query-identity cache validates its
self-root strictly; the warm-read validator rejects a same-canonical edit on
its own.

**R4.** Source-content changes produce a semantic fact diff (publishable via
`compute_upsert_changes_from_parse` for LSP / observability callers).
Invalidation is not propagated; the fact diff is the public observability of
what changed.

**Import-route admission ownership.** `DerivedRawState.import_routes` and
`DerivedRawState.import_routes_known_miss_recorded_at_generation` have distinct
validity models and distinct admission producers:

- `VerterHost::set_import_dependencies` is the **single producer** of the full
  caller-supplied route snapshot AND the sole admission point for the
  known-miss generation sidecar. For each known-miss specifier (no resolved
  canonical, no candidates, no effective target), it records the current
  workspace `content_generation` so the reader can detect when a new canonical
  may now satisfy a previously unresolvable specifier.
- `VerterHost::cache_positive_import_route_result` is the **single
  positive-only point producer** for `DerivedRawState.import_routes`. Positive
  resolutions stay valid until the owner's source content changes; they need no
  generation tag and must NOT touch the sidecar.
- `VerterHost::configure_projects` and
  `VerterHost::upsert_via_scheduler_with_priority` may `.clear()` both fields
  in lockstep. Leaving the sidecar populated after either reset would extend a
  stale `content_generation` stamp into the next admission cycle.

The integration guard at `crates/verter_session/tests/import_route_writer_guard.rs`
enforces both halves statically: no direct `import_routes` mutation outside the
three named writers, and no known-miss sidecar admission outside
`set_import_dependencies`.

### Cache identity & validation

**R5.** Caches divide into two families:

- **Content-addressed artifact caches** — `FileArtifactStore`,
  `ResolvedImportFacts`, typed-IR resolve, `MemberSemanticFactStore`,
  `MemberDisplayFactStore`, `ModuleAugmentationIndex`. Keys include
  `content_hash` or a derived parse-stable hash.
- **Query-identity caches** — `RouteDb`, `MaterializeStructureDb`,
  `RefCycleResultDb`, `SemanticGraphStore` query nodes, `ComponentMetaResultDb`.
  Keys exclude version hashes; concurrent variants coexist as candidates inside
  one slot.

Version rooting for query-identity caches lives **inside the cached value**,
never in the key. The rail per layer:

- The structural carriers (`MaterializeStructureDb`, `RefCycleResultDb`), the
  `SemanticGraphStore` family memo (`Instantiate` / `ResolveMacroPayload` query
  nodes), and `ShapeCacheDb` root via the candidate's `ReadSetSignature.facts` +
  `self_root_canonicals` + `validated_at_generation`, with the live whole-hash
  re-sourced at value-compute time (NOT carried in the key).
- `RouteDb` (per-name + barrel) roots via the value-side `ValidatedFactCache`
  fact signature (`RouteResult` / `BarrelRouteSurface.fact_dep_signature`).
- `ComponentMetaResultDb` roots via the owner whole-hash candidate discriminant
  (`BoundedCandidateMap<…, Hash16, …>`) + `ReadSetSignature.facts` +
  `validated_at_generation`; the owner `FileWholeHash` stays in the value facts.

In every case the live whole-hash / version is re-sourced at value-compute time
via `ensure_indexed_ready_serve` and validated against the live `StoreView` on
each warm hit — never carried in the content-free key. See the per-key-context
detail below.

**R6.** Cache keys never include `fact_dep_signature` or content / version
hashes (for query-identity caches). Signatures and version info live on the
cached value.

The `SemanticGraphStore` family memo applies R6 to its
`SemanticQueryKey::Instantiate.base`, `SemanticQueryKey::ResolveMacroPayload.owner`,
and `SemanticQueryKey::TypeOf.value_root`
fields (mirrored on the `FamilyKey` memo identity). The first two carry the
env-bearing, content-free `ResolvedDeclSlotIdentity` (`defining_canonical`,
`merged_symbol_name`, `symbol_space` + the `project_identity` / `type_env_hash`
/ `lib_env_hash` ENV dims — `Instantiate` / `ResolveMacroPayload` base/owner
are always `symbol_space = Type`); `TypeOf.value_root` carries the env-bearing,
content-free `ValueRootSlotIdentity` (the scoped `ValueRootKey` root + the same
`J`/`T`/`L` env dims — `build_typeof` does env-sensitive name/export
resolution, so an env-free value root would warm-hit across envs). The extra
`resolve_env_hash` (`R`) dim rides
on a dedicated per-key `InstantiateContext { projection_reduction,
resolve_env_hash }` / `MacroPayloadContext { resolve_env_hash, mode }` /
`TypeOfContext { projection_reduction, resolve_env_hash }` (NOT the
shared `ProjectionReductionContext`, which stays a pure projection-demand
identity — §2.6 per-key-context rule); the production derivation points are
`type_slot_for` + `instantiate_context_for` / `macro_payload_context_for` /
`typeof_key_for`; `provenance` + `merge_role` stay at
FAMILY-IDENTITY on `FamilyKey` for EVERY context-bearing projection-reduction
family — `Instantiate`, `KeyOf`, `MappedType`, and `ProjectPath` all thread
both axes into their `FamilyKey`, so two `KeyOf` (or `MappedType`) queries
differing only in provenance / merge_role get DISTINCT `(FamilyKey, slot)`
identities and never alias onto one memo entry. The slot is **content-free** —
its `T,L,J` env dims are KEY dims, but content/version hashes (`whole_hash` /
`content_hash` / `parse_stable_hash` / `fact_dep_signature`) are FORBIDDEN; the
versioned `DeclIdentity` type (`{ canonical_id, whole_hash, decl_name }`)
survives as a value-side payload for `SemanticNodeData::{TypeParam, DeclRef,
InstantiationRef}` and `ShallowDiagnostic`, but is forbidden inside any
derived-`Hash` query-identity key (the retired content-free `DeclKey`
query-key struct must NOT be reintroduced). The cold-build path
(`build_instantiate`, `build_resolve_macro_payload`) re-sources the live file
content version from
`ResolverContext::ensure_indexed_ready_serve(base.defining_canonical)`'s serve carrier `indexed.whole_hash` at
value-compute time and rolls it into the published `MemoEntry`'s
`ReadSetSignature.facts` + `self_root_canonicals` rails. The slot is U2-DERIVED
at query-key construction via `ProjectSemanticDispatch::type_slot_for` (reads
the live host env); test fixtures use the env-agnostic
`ResolvedDeclSlotIdentity::type_slot_unscoped`. Non-file bases (`__builtin__`,
empty global, `<synthetic>`) do NOT fabricate a `FileWholeHash` self-root —
they root via `args` nodes only. A real-file base whose `ensure_indexed_ready_serve`
returns `None` is a stale key and returns `cache_suppress = true`. Each
`(family, slot)` in `FamilySlots` holds a candidate list capped at
`FAMILY_SLOT_CANDIDATE_CAP = 4` (per-slot multi-candidate); two
content-versions of the same content-free key coexist as distinct candidates
inside one slot under R20 overlay isolation. Admission identity is the pair
`(validated_at_generation, ReadSetSignature.facts)` — same exact discriminant
replaces in place; a different view appends at the back; cap overflow
FIFO-evicts the oldest. Warm lookup scans every candidate and returns the FIRST
that passes BOTH §3.4 gates (below); `validated_at_generation` is LRU-recency
metadata, NOT a semantic-validity oracle.

**§3.4 materialised-record satisfaction (the two-gate warm hit + recorded
backfill).** A warm hit is decided by the RECORDED materialised `(path, point)`
set the candidate's compute ACTUALLY produced —
`MemoEntry.satisfied_projection: MaterializedSet` — NOT by the candidate's
nominal slot/mode, and NOT by enum rank. Each `MemoEntry` carries the terminal
point (`Demand::from(terminal_mode)` at the query path) PLUS one
`Demand::navigate(prefix)` per actually-walked intermediate (`ProjectPath`
records these in `build_project_path`; each `PrefixBackfill` records its own
single `Navigate` point; a non-path build defaults to the single terminal point
for the canonical key). A warm hit requires **two independent gates, both must
pass**: (1) `cached_satisfies(satisfied_projection, requested_point_for_key(key))`
— some recorded point dominates the request at the SAME path (interned-id
equality, not prefix; regime-equal componentwise via `semantically_dominates`,
so `display_needs` never gates typed-value reuse); (2)
`read_set_signature.validate_with_self_roots` against the live view. Recording
the NOMINAL demand instead of the recorded materialised set silently collapses
soundness to wrong warm hits (a deep compute that only `Navigate`-walked an
intermediate serving a `Shallow`/`Expanded` request it never materialised) —
the discriminating guard
`cache_satisfaction_is_materialized_point_not_nominal_demand` is the sole
defense. **Backfill** clones a broader entry UNCHANGED (recorded points
verbatim, never a meet/nominal point) into a projection-depth-narrower target
slot (`slot_domain_siblings`: the legacy `Expanded→Shallow→Navigate→Identity`
DIRECTION), GATED by `cached_satisfies`. The direction stays narrower-only
because the lattice's `Navigate ⊒ Shallow` (higher normalization/operator
rungs) does NOT mean a `Navigate` next-hop result can serve a `Shallow` shell
surface — it carrier-stops without materialising the shell; an all-peers
backfill would hide cyclic-heritage expansions. The gate still REJECTS the
unsound legacy `Shallow → Navigate` clone (`Shallow ⊅ Navigate`:
`normalization_depth None < NavigateOnly`). Guards:
`cache_satisfaction_is_materialized_point_not_nominal_demand`,
`cache_satisfaction_requires_path_exact_not_prefix` (the path-axis
discriminator — a deep recorded point never satisfies a strict-prefix request),
`backfill_writes_only_recorded_materialized_points` (in
`semantic_query_memo::tests`); the enum-rank `backfill_targets` unconditional
fan-out is RETIRED.

**R7.** Shared semantic materialisations key by `ResolvedDeclSlotIdentity`
(content-free) as the cache slot identity. The cached value carries
`VersionedDeclIdentity` (with content / version info) for fence seeding, scope
identity, and version-aware semantic operations. Cross-owner reuse is the
architectural invariant — a `ChatMessageProps` reached from N owners
materialises once.

```rust
struct ResolvedDeclSlotIdentity {
    defining_canonical: Arc<str>,
    merged_symbol_name: InternedName,
    symbol_space: SymbolSpace,
    project_identity: ProjectId,
    type_env_hash: Hash16,
    lib_env_hash: Hash16,
}

struct VersionedDeclIdentity {
    slot: ResolvedDeclSlotIdentity,
    content_hash: Hash16,
    parse_env_hash: Hash16,
    merged_parts: SmallVec<[(DeclPartId, FactHash); 2]>,
}
```

`merged_symbol_name` is **stable across declaration reordering and TypeScript
declaration merging**. Same-scope `interface Foo` parts merge into one
`merged_symbol_name`. Per-part fingerprints live in
`VersionedDeclIdentity.merged_parts` for diagnostics and overload surfacing —
they are NOT the cache validation oracle.

**R8.** Only final per-owner payloads (`ComponentMetaResultDb`) are
owner-keyed. The slot key is content-free — `(owner_canonical,
options_fingerprint)` — per the query-identity-cache model: the owner's content
version (`owner_whole_hash`) is NOT in the slot key, it is carried by the
per-slot candidate and validated strictly on read. Concurrent owner content
versions coexist as bounded candidates inside one slot (R20).

**R9.** Reuse is the default; recomputation is the exception. A cache miss
under steady-state load is treated as evidence of a fact-graph bug or a real
semantic change — never as a routine event.

### Fact model

**R10.** Facts use stable `FactKey`s, not `Vec` indices. Removed facts validate
as misses (registry lookup returns `None`). Reordering a file does not change
`FactKey`s.

**R11.** Every binding-naming fact key carries `SymbolSpace ∈ {Type, Value,
Namespace}`. **`BothTypeValue` is forbidden.** A `class Foo` declaration
occupying both spaces emits TWO distinct facts: `Export("Foo", Type)` and
`Export("Foo", Value)`.

**R12.** Facts split strictly by domain. **Parse-domain facts (`FileFacts`,
parse_env keyed) never reference resolved canonicals.** **Resolve-domain facts
(`ResolvedImportFacts`, `RouteDb`, resolve_env keyed) carry resolutions.** The
producer of a parse-domain fact does not run the resolver; it only emits the
syntactic shape. `FactKey::domain()` routes validator lookups to the correct
store.

**R13.** Each `Fact` carries `semantic_hash` (alpha-normalised structural
fingerprint) and `display_hash` (cosmetic — JSDoc, param names, comments).
Signatures record per-observation `lane: Semantic | Display` so cosmetic edits
invalidate display-bearing materialisations only. Storage of semantic vs
display facts is **physically split** at the cache layer
(`MemberSemanticFactStore` keyed on `parse_stable_hash`, `MemberDisplayFactStore`
keyed on `content_hash`) so a cosmetic edit hits only the display store and
does not recompute semantic facts.

**R14 (path-precise).** `Export(name).semantic_hash` is computed over the
**export's body alone**, with every cross-decl reference (same-file local decl,
same-file member, imported binding) recorded as a **reference-shape edge** by
name + space, NOT by inlining the referent's body. Editing an unused local in
the same file does NOT invalidate consumers of an export that does not reach it.
Editing a member that `Pick<Foo, "a">` does not select does NOT invalidate that
consumer.

**R15.** `SyntacticExportSet` (parse-domain) records local exports and bare
re-export specifiers only — no resolution. `EffectiveExportSet` (resolve-domain,
owned by `RouteDb`) records post-wildcard-expansion, post-module-augmentation
visible names with resolved canonicals. The two cannot be merged.

**R16.** Semantic fingerprints are alpha-normalised structural hashes.
Source-text hashes, span-based hashes, position offsets, or any hash that
changes under cosmetic edits (whitespace, comments, generic param rename,
declaration reordering) are forbidden as semantic hashes. Cosmetic changes live
in `display_hash` only.

### Sessions & concurrency

**R17.** Sessions are views over the base host. A `SessionView` never mutates
the host. `host.upsert` is not called from any query path. Overlay artifacts
are stored under the overlay's content hash and coexist with base artifacts
under different keys. Byte-identical overlay collapses to base hash
automatically.

`SessionView::content_hash_for` is a **view-authoritative current-content
oracle**, consistent with `SessionView::source` by contract — it returns the
hash of the exact bytes `source()` yields. An overlay-covered canonical
resolves to the overlay source's hash; every other canonical resolves through
`VerterHost::authoritative_current_content_hash` (the scheduler authority, no
permissive fallback). It is NOT a content-agnostic `FileArtifactStore` scan: a
stale pre-edit `IndexedReady` lingering past a same-canonical edit (lazy
invalidation) does not surface — an evicted / deleted canonical reports `None`.
The overlay materialiser (`materialize_overlay_indexed_ready_with_view`)
derives BOTH the overlay source and its content hash from the `SessionView`
itself (one authority) — a caller cannot pass a separate source/hash pair, so a
stale hash can never be paired with a fresh source. `FileArtifactStore` exposes
NO content-agnostic *currency oracle* — a canonical-only `Option`-returning
accessor that scans `artifacts` is forbidden by the
`file_artifact_store_defines_no_unpinned_currency_oracle` architecture guard.
The only intentional content-agnostic escapes are `get_any` /
`get_artifacts_any`, each guarded at every call site.

**R18.** `SessionView` is passed explicitly through `ResolverContext`.
Thread-local "current view" globals remain forbidden.

**R19.** Fact validation is the **cache-correctness oracle**.
`StoreViewCompatToken` is the **concurrency oracle**: singleflight lane
separation, mid-query change detection, write admission against superseded
computations. The two are orthogonal and must not be conflated.

**R20.** Multi-candidate storage isolates concurrent overlay variants in the
same query-identity slot. Default cap = 4 candidates per slot. **Eviction is
insertion-order (FIFO) on write only.** No LRU bookkeeping on read; warm reads
are `&self` shared borrows with zero atomic write or lock contention.
Concurrent sessions never overwrite each other's results.

**Bounded query-identity retention substrate.** Every durable query-identity
cache accumulates concurrent content-version CANDIDATES in its content-free
slots — the version state lives on each candidate's value-side carrier (per R6),
NOT in the key, so successive content edits co-locate as candidates in ONE slot
rather than minting fresh top-level keys. The bounded caches:
`ComponentMetaResultDb` (owner whole-hash candidate discriminant),
`RefCycleResultDb` (content-free `RefCycleResultKey`; per-version state on the
value's `self_root_canonicals`), `MaterializeStructureDb` (content-free
`MaterializationCacheKey`; per-version state on the value's `self_root_canonicals`),
the `SemanticGraphStore` family memo + relation memo + named-type index
(`ResolvedDeclSlotIdentity` slots; per-version state on the value) + the
`SemanticGraphStore` derivation/origin store (`DerivationStore` — `edges` keyed by
`(SemanticNodeId, OriginEdgeKind)`, plus its `signature_pool` fence interner;
content-addressed graph-instance state, NOT a query-identity slot) — are all
bounded by the shared `verter_session::bounded_query_retention` substrate. Each
distinct content edit appends a fresh CANDIDATE (or, for the content-addressed
`DerivationStore`, a fresh edge bucket); the substrate is the routine
memory-reclamation path (the eager own-canonical drain that formerly reclaimed
these is retired). Two cooperating pieces, tuned per cache:

- `GlobalRetentionBudget<K>` — a FIFO insertion-ordered total-size cap. A cache
  records each admitted entry's key from its write-side `post_publish` hook;
  the budget returns the oldest keys to evict once the count exceeds the cap.
  `MaterializeStructureDb` and `RefCycleResultDb` cap at `MAX_ENTRIES` (2048);
  the `SemanticGraphStore` family / relation / named-type budgets use
  `DEFAULT_BUDGET_CAP` (4096); the `DerivationStore` bounds its edge-bucket
  count (`DERIVATION_EDGE_BUCKET_CAP` = 4096) and its signature-interning pool
  (`DERIVATION_SIGNATURE_POOL_CAP` = 4096) with two such budgets, evicted
  write-side from `record` / `intern_signature`. The cache's invalidation paths
  call `forget` so the ledger stays consistent with the map.
- `BoundedCandidateMap<K, D, V>` — a content-free-keyed slot map with a bounded
  per-slot candidate list (`DEFAULT_CANDIDATE_CAP` = 4) AND a global budget.
  `ComponentMetaResultDb` is built on it: the slot key drops `owner_whole_hash`,
  which becomes the per-candidate discriminant `D`. A fifth owner version in a
  slot FIFO-evicts the oldest candidate; the global budget (`GLOBAL_BUDGET` =
  512) caps the total across all slots.

Eviction is stale-first when cheaply detectable, then FIFO. Evicting a *valid*
entry is permitted — it only forces a recompute, never an incorrect result. A
reader clones the candidate `Arc` before validating, so a concurrent removal
never invalidates an in-flight reader; removal is keyed by an insertion
sequence number unique per admission. `BoundedCandidateMap::admit` holds the
`DashMap` shard write guard across its candidate push, so an empty-slot
reaper's `remove_if`-detach cannot interleave between "slot observed empty" and
an in-flight admit's push — a published candidate is never stranded in a
detached slot.

**Single write-side consistency domain (rule).** Every budgeted cache must have
exactly one write-side consistency domain: either the map + budget + reverse
index are mutated under one exclusive lock, or every gap is closed by BOTH
atomic same-key admission AND identity-scoped removal. New budgeted caches must
prefer structural (single-lock) serialization. Concretely:
`BoundedCandidateMap::admit` runs the slot mutation, the removed-seq
`forget_seq`, AND the new candidate's `record_admission` inside one
continuously-held slot `Mutex` critical section (the `retention_gate` is only a
reset fence — a shared read guard does not serialise two admits of the same
content-free slot); global-budget victim eviction runs after that slot lock is
released (lock order `retention_gate.read → DashMap shard/slot → slot Mutex →
budget Mutex`, victim slot lock last — no AB-BA). The `SemanticGraphStore`
relation memo / named-type index (`BudgetedRelationMemo` /
`BudgetedNamedTypeIndex`) each hold a per-wrapper `admission_lock` across the
`DashMap::entry` decision + `record_admission` + victim `remove_if` (and, for
the named-type index, across `retain_for_canonical`'s map removal + `forget_seq`
loop), making their single-write-domain structural rather than
"safe-by-construction". The `MaterializeStructureDb` / `RefCycleResultDb`
cooperative publish holds the `publish_fence` (the Db's `retention_gate`)
across `entries.insert` + `post_publish`, and `post_publish` does
`bump_live_counter` + `record_admission` together — so the map insert, the
`live_counter` increment, and the budget admission are one fenced write-side
step.

**Per-MapK publish linearization (by-flight-key shape).** In the
`cooperative_admit_with_post_publish_by_flight_key` adapter the published map
key (`MapK`) is independent of the in-flight coalescing identity (`FlightK`) —
e.g. two overlays on the same cache key flight on distinct `FlightK` lanes but
publish under one `MapK`. Both can become cold winners and both reach the
publish path, so the second publish DISPLACES the first and the displaced entry
must receive `removal_cleanup`. The `publish_fence` is a SHARED read guard: it
serializes publish-vs-`clear` (write guard) but does NOT serialize
publisher-vs-publisher. A bare `map.insert` plus a post-hoc cleanup of the
returned displaced value is therefore unsound — the displacing publisher can
run `removal_cleanup` for an entry whose own `post_publish` has not yet run,
underflowing a `live_counter` (the matching bump never happened) or orphaning a
reverse-index registration. The full publish triple (insert/replace →
displaced `removal_cleanup` → new `post_publish`) must be ONE operation atomic
per `MapK` against other publishers. The substrate
(`publish_entry_linearized_per_map_key`) rides the map's per-key shard lock —
`DashMap::entry` holds the shard write guard for the `Entry`'s lifetime, and a
`DashMap` shards by key — rather than a new global lock; contention stays
per-shard, identical to a plain insert. A displaced entry is then observable
only to a shard guard acquired AFTER the displaced publisher released its own,
i.e. after that entry's `post_publish` completed: the displacing publisher's
cleanup is the linearized successor of the displaced publisher's completed
publish. Consequence: a hook published under this linearized shape MUST NOT
re-enter the SAME `entries` map (it would self-deadlock on the held shard write
guard) — the by-flight-key consumers' hooks are lock-free `live_counter`
atomics or touch a SEPARATE `CanonicalReverseIndex` map, so they are safe.

**Path-split: the unified-key form does NOT linearize.** The unified-key entry
point (`cooperative_admit_with_post_publish`, `MapK == FlightK`) coalesces every
caller of one key onto ONE cold compute via the in-flight table, so there is
exactly ONE publisher per key and the map slot is always vacant at publish — no
overwrite ever occurs and there is no publisher-vs-publisher to serialize. It
therefore publishes through `publish_entry_insert_then_post_publish`:
`map.insert` takes, mutates, and RELEASES the shard guard, then `post_publish`
runs with NO shard guard held. This is REQUIRED, not merely permitted: the two
budgeted unified consumers (`MaterializeStructureDb`, `RefCycleResultDb`) run a
FIFO `register_post_publish` → `evict_budget_victim` →
`entries.remove_if(victim)` re-entry on the SAME map from inside `post_publish`.
Were the unified path to hold the shard write guard across `post_publish` (the
linearized shape), a victim hashing to the just-published key's shard would
deadlock the publishing thread on a non-reentrant same-shard write-lock
acquisition. The two publish shapes are selected by `cooperative_admit_impl`'s
`linearize_publish` flag (the public by-flight-key wrapper passes `true`, the
unified wrapper passes `false`); both ride the one winner/joiner state machine.
Discriminators: `by_flight_key_displaced_cleanup_runs_after_its_own_post_publish`
proves the by-flight-key ORDERING (A's `post_publish` happens-before B's
cleanup-of-A via an event log + a signed-counter low-water invariant), distinct
from the aggregate-count test
`by_flight_key_overwrite_cleans_up_displaced_entry` which only proves the counts
net out; and `unified_budgeted_post_publish_eviction_does_not_self_deadlock`
proves the unified path's deadlock-freedom (a cap-1 budgeted cache whose
`post_publish` re-enters the map to evict a same-shard victim during
publication completes under a 5s watchdog — it hangs against the linearized
shape).

`SemanticGraphStore::invalidate_all` (the project-generation bump) clears EVERY
`SemanticNodeId`-keyed semantic cache on the store — the family memo, the
in-flight admission table, the relation memo, the named-type index, the
`DerivationStore` (edges + signature pool), and the Γ.B reverse index — so no
stale judgement survives a tsconfig / SDK / workspace-folder change. Each
cleared `SemanticNodeId`-keyed cache has its retention ledger cleared in
lockstep. Adding a new `SemanticNodeId`-keyed semantic cache to the store
obliges extending `invalidate_all`'s clear set. `invalidate_all` does NOT touch
the node arena: `SemanticNodeId` is a raw `u64` arena index with no generation
tag, so the arena's dense `nodes` / `scopes` storage is **append-only** — every
`SemanticNodeId` handed out stays valid for the store's lifetime. Reclaiming
the arena id space would require a generational `SemanticNodeId` redesign (a
mid-flight arena shrink under concurrent index-holding readers is unsafe) and
is out of scope of the bounded-retention substrate. The substrate bounds the
`SemanticNodeId`-keyed *caches* listed above; the node arena itself is
unbounded by design pending that redesign.

Concrete substrate (the per-domain target form):

- Outer table: `DashMap<K, Arc<CacheEntry<V>>>`.
- `CacheEntry<V> { candidates: ArcSwap<SmallVec<[Arc<Candidate<V>>; 2]>> }`.
- Admission writes use RCU; write contention serialised by
  `StoreViewCompatToken`-keyed singleflight.

Bounded signature size: a `fact_dep_signature` is capped at 1024 entries
(`FACT_SIGNATURE_CAP`). Beyond that the producer consumes a hierarchical fact
(downstream materialisation `semantic_hash`) instead of flattening transitive
facts. The path-precise tracer carries the overflow as a structural bit on the
[`ReadSetSignature`](../../crates/verter_session/src/fact_signature_helpers.rs)
carrier: `ReadSetSignature { facts: Arc<[FactVersionRef]>, overflowed: bool }`,
with `is_cacheable()` returning `!overflowed` (emptiness is NOT a non-cacheable
condition — only overflow is). Empty and overflow are structurally
distinguishable at the carrier type; the warm-hit oracle cannot conflate them.
Overflow produces `FactSignatureOverflow` audit event + candidate is admitted
as `NonCacheable`.

## Typed SignatureAdmission gate (CRITICAL)

Producers convert their finalised fact tracer into a typed admission verdict via
[`SignatureAdmission::from_finalise(FactReadSetFinalise)`](../../crates/verter_session/src/cache_runtime/admission.rs).
Two arms:

- `SignatureAdmission::Cacheable(ReadSetSignature)` — the tracer finalised with
  a bounded path-precise signature. The producer publishes its cache entry
  under this signature; the warm-hit oracle validates it against the live store
  view on every read.
- `SignatureAdmission::NonCacheable(NonAdmissionReason)` — the tracer
  overflowed, the provenance is unresolved, the self-root closure is
  incomplete, or another structured refusal applies. The verdict carries the
  typed refusal reason (`SignatureOverflow`, `UnresolvedProvenance`,
  `SelfRootConflict`, `RouteGenerationDependency`, `ForcedTestRefusal`,
  `IntrinsicNonCacheable`, etc.) for audit.

Production callsites pattern-match on `SignatureAdmission` directly:
`Cacheable(sig)` admits the entry through the cache substrate, `NonCacheable(_)`
refuses it. The two consumer families differ in how they route the refusal:

- **Cooperative cache-runtime producers** (imported registry,
  materialize-structure, ref-cycle) construct `CacheAdmission::ReturnOnly {
  value, reason }` so the cooperative joiners receive the value without
  admitting a shared cache entry. The typed reason is bridged from the
  producer's `SignatureAdmission::NonCacheable(reason)` arm to the lowering site
  via a per-thread TLS slot (`cache_runtime::set_return_only_reason` /
  `cache_runtime::take_return_only_reason`) so the reason reaches
  `CacheAdmission::ReturnOnly { reason }` honestly instead of defaulting to
  `SignatureOverflow`.
- **Non-cooperative producers that own their carrier slot** (notably
  `CompileSlot.fact_dep_signature: ReadSetSignature` on the compile-tier
  cold-build path) route the `NonCacheable(_)` arm to a **skip-publish
  refusal**: the producer holds the freshly computed result, returns it to its
  single caller, and refuses the `compile_slots.insert` so no slot lands. A
  prior successful slot for the same `(canonical, profile)` is ADDITIONALLY
  removed (not just left in place) so the carrier invariant `present in
  compile_slots ⇒ admitted cache entry for the current version` survives a
  recompute that overflows. The companion scheduler artifact commit is gated on
  `Cacheable` admission so the overflowed result is not observable through
  `try_get_artifact` either.

`SignatureAdmission::into_cacheable()` is a test-fixture / owned projection
helper that returns the carrier as `Option<ReadSetSignature>` (or `None` on
refusal). It is consumed by test scaffolds that need the owned rail; production
callsites match on the variants directly.

Hard rules:

- Direct construction of `Arc::from(Vec::<FactVersionRef>::new())` outside
  `ReadSetSignature::empty()` / `ReadSetSignature::overflow()` (allocated
  through the substrate helper `fact_signature_helpers::empty_fact_signature`)
  is forbidden. The legacy `finalise_signature_or_empty` helper that collapsed
  `Overflow → empty signature → publish anyway` was deleted; no caller may
  resurrect that path.
- `ReadSetSignature` carries facts + overflow only. The cache entry's
  world-generation lives on `CacheEntry<V>` alongside the value
  (`validated_at_generation`). Conflating generation onto the signature blurs
  the responsibility boundary.
- Empty and overflow are different states. `ReadSetSignature::empty()` is
  cacheable (an empty fact rail validates vacuously on warm hits);
  `ReadSetSignature::overflow()` is not.

The new guards are registered in
[`CRITICAL_RULE_GUARDS`](../../crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs)
under the `Typed SignatureAdmission gate` entry.

## Error-Tolerance Non-Admission + §22 Absorption (CRITICAL)

The error-tolerant admission decision and the type-lattice absorption are a
closed, fact-rooted contract (`docs/arch/u2-query-value-domain-design.md`
§18.2–18.3, §22). Three invariants:

1. **Admission keys on the rooting FACT, not the taint enum class (§18.2).**
   [`admit_decision(taint, sig)`](../../crates/verter_session/src/semantic_query/admit.rs)
   maps a result's [`ResultTaint`] + its sound
   [`ReadSetSignature`](../../crates/verter_session/src/fact_signature_helpers.rs)
   to `Admission::{Warm, ReturnOnly}`. `Clean` ⇒ `Warm`.
   `Partial(MissingDependency)` ⇒ `Warm` **iff**
   `sig.records_missing_dependency_fact()` (a `DerivedFactKind::ImportRoute`
   rail) else `ReturnOnly`; `Partial(UnresolvedReference)` ⇒ `Warm` **iff**
   `sig.records_negative_resolution_fact()` (a `ResolvedImportClause` /
   `ResolvedReexportBinding` whose `resolved_canonical` is the
   `UNRESOLVED_SENTINEL`) else `ReturnOnly`. `Partial(IncompleteDeclaration)`,
   `Broken(SyntaxError)`, and `Broken(TornRead)` are always `ReturnOnly`. The
   bare taint discriminant NEVER licenses a warm publish — the signature is the
   authority for whether the invalidation rail IS present. `admit_decision` is
   the sole admission gate in `finalise_traced_build_output`. The gate keys on
   the rooting fact KIND on the signature; correlating that fact to the SPECIFIC
   degraded reference is a §18.4 follow-up. Taint PRODUCERS (parser
   error-recovery, resolver degradation, completion-fence torn-read) are
   produced by the §18.4 producers; `taint` is currently always `Clean`, so the
   live publish behavior is unchanged and the partial/broken arms are exercised
   by the `admit_decision` unit tests.

2. **Taint join is monotone over `Clean ⊑ Partial ⊑ Broken` (§18.3).**
   [`ResultTaint::join`](../../crates/verter_session/src/semantic_query.rs)
   propagates the MAX level; within a level it keeps the more-severe
   `BrokenInputClass` (severity order `MissingDependency < UnresolvedReference <
   IncompleteDeclaration < SyntaxError < TornRead`). Finite + monotone — taint
   only moves up, so propagation terminates.

3. **§22 absorption is the reducers' FIRST fast-reject, as separable helpers.**
   [`absorb_*`](../../crates/verter_session/src/project_semantic_dispatch/absorb.rs)
   (`absorb_union`/`absorb_intersection`/`absorb_key_of`/`absorb_indexed_access`/`absorb_mapped`/`absorb_conditional`)
   are isolated entry hooks each reducer calls with ONE
   `if let Some(out) = self.absorb_*(...) { return out; }` line BEFORE its
   structural body — never invasive per-arm edits. They do **cheap `node_data`
   peeks only** (bounded transparent `Alias` unwrap; no `execute` /
   `evaluate_deferred` / resolver work). Table: `X|any=any`, `X|never=X`,
   `X|unknown=unknown`; `X&never=never`, `X&any=any`, `X&unknown=X`; `any[K]=any`,
   `never[K]=never`, `unknown[K]`=UNCONDITIONAL error, `keyof any/never =
   string|number|symbol`, `keyof unknown = never`; mapped over `never`=`{}`,
   direct mapped over `unknown`=error; conditional `any extends T ? X : Y = X|Y`
   (union of BOTH branches via `NormalizeUnion`, mode-independent, except when
   `extends` is an `infer` pattern → fall through to the infer-binding path),
   DISTRIBUTIVE `never extends T = never`, NON-distributive `never extends T = `
   true branch (the fast-reject gates the collapse on `distributive`),
   `error extends T = error` (carrier-dominating, FIRST). `any`/`never`/`unknown`
   are `Clean` (legitimately cacheable). **`error` rides
   `SemanticNodeData::Opaque(QueryError)`** (no new `GraphTypeNode` wire arm): an
   `error` operand **carrier-dominates** every other absorber, so the error
   CARRIER (node identity + `QueryError` payload) is never hidden behind a
   `Clean` extreme — relation/display keep seeing the error type. This is
   carrier-dominating, NOT taint-propagating: the absorbed `QueryBuildOutput`'s
   `taint` defaults to `Clean` and absorption does NOT join any operand's §18
   taint onto it. That is sound today because no producer emits non-`Clean`
   taint, so every absorbed type error is deterministic (`unknown[K]`,
   `keyof error`, …) and legitimately cacheable. An error becomes
   `ReturnOnly`-prone only when it is INPUT-DEGRADED — a §18.4 property routed
   through `admit_decision` once taint producers land (see the `TODO(§18.4)` in
   `absorbed_output`). `error` relates **bidirectionally like `any`** in
   `relate_nodes` (so a broken sub-result never cascades spurious
   `NotAssignable`). `QueryError::DeclPlaceholder` is an expandable carrier, NOT
   the error type, and is excluded from both the absorption and the relation
   flip.

Guards (registered in `CRITICAL_RULE_GUARDS` under
`Error-Tolerance Non-Admission + §22 Absorption`):
`error_tolerance_broken_input_is_returnonly_fact_rooted_error_is_cacheable`,
`error_any_never_propagation_lattice`,
`error_type_is_returnonly_prone_any_is_cacheable`
(`crates/verter_session/src/error_propagation_lattice_tests.rs`).

### Environment & GC

**R21.** Environment hashes split into **five** orthogonal dimensions (the
most-cited rule):

| Hash | Captures |
|---|---|
| `parse_env_hash` | Parser / SFC / compiler feature flags, syntax mode, language target |
| `resolve_env_hash` | `base_url`, `paths`, workspace aliases, project references, module resolution mode, package `exports`/`imports`, default extension order |
| `type_env_hash` | TS semantic options that change type meaning (`strict`, `noImplicitAny`, etc.) |
| `lib_env_hash` | TS built-in lib selection (`lib.dom.d.ts`, `lib.es*.d.ts`), `types`, `typeRoots`, registered ambient libs, global / module-augmentation corpus identity |
| `project_identity` | Project root, tsconfig path, provider root, workspace root, membership / owner selection |

**Scoping rule for `lib_env_hash` (per R21).** `lib_env_hash` enters a cache
key only when the cached value depends on lib data:

- `ResolvedImportFacts` (base import resolution) does **NOT** include
  `lib_env_hash`. A lib update does not change where `./theme` resolves.
- `RouteDb` per-name and effective-set caches **DO** include `lib_env_hash`
  because module augmentations stitch into the effective surface.
- Typed-IR resolve, `MaterializeStructureDb`, `RefCycleResultDb`,
  `SemanticGraphStore`, `ComponentMetaResultDb` **DO** include `lib_env_hash`
  because semantic meaning depends on intrinsic types (`Array<T>`,
  `HTMLElement`, etc.).

A single bundled `project_config_hash` is forbidden.

The five hash functions live on `verter_workspace::resolver::IdeProjectConfig`
+ per-call inputs surfaced through `EnvHashInputs<'_>`:

```rust
cfg.parse_env_hash(&inputs);   // Hash16
cfg.resolve_env_hash(&inputs); // Hash16
cfg.type_env_hash(&inputs);    // Hash16
cfg.lib_env_hash(&inputs);     // Hash16
cfg.project_identity();        // Hash16
```

Each function mixes a per-dimension salt so the five hashes derived from
identical baseline state never collide.

### Module-Resolution Keying (CRITICAL)

Module/import resolution is keyed on the **split** env dimensions, and the lib
corpus is **NEVER folded into `resolve_env_hash`**. This is the
import-resolving refinement of R21 — `resolve_env` and `lib_env` are orthogonal
dimensions, and conflating them is a correctness bug (a `lib.dom.d.ts` bump must
not invalidate where `./theme` resolves; a `moduleResolution` change must not
invalidate intrinsic-type meaning).

Concrete contract:

- Every import-resolving cache key / `*Context` carries only the dims it
  depends on: `resolve_env_hash` always; `lib_env_hash` ONLY when the value
  consults the ambient/types corpus (module augmentations); `parse_env_hash`
  and `project_identity` per their own scoping. `ResolvedImportFactsKey` =
  `{parse_env_hash, resolve_env_hash}` (NO `lib_env_hash`);
  `EffectiveExportSetKey` = `{resolve_env_hash, lib_env_hash, project_identity}`
  (lib because augmentations stitch in).
- **Resolve-domain ENV inputs** (hash into `resolve_env_hash` ONLY): the
  `moduleResolution` mode (`ModuleResolutionMode`), the active
  `exports`/`imports` condition set (`ConditionSet`), `base_url`/`paths`,
  workspace aliases, project references, extension order.
- **Lib-domain ENV inputs** (hash into `lib_env_hash`, NEVER
  `resolve_env_hash`): TS lib selection (`lib_names`), `typeRoots`, the
  ambient-corpus fingerprint.
- The module-resolution design vocabulary (`ModuleResolutionMode`,
  `SpecifierKind`, `ConditionSet`) is **content-free SHAPE** in
  `verter_workspace::module_resolution`. `SpecifierKind` is a per-specifier
  classification used by the U0 resolution-matrix walker — it is NOT an
  env-hash input. The matrix walker and the broken-input taint producers live
  in U0 `verter_session::resolver_core` (see
  `docs/arch/native-typeinfo-parity-u2-reducers.md` →
  `U0.RESOLVER_CORE_FOUNDATIONS`); this rule owns only the keying contract.

Guards (`crates/verter_workspace/src/env_hash_tests.rs`):
`module_resolution_keys_on_resolve_env_not_type_or_lib` (a module-resolution
input moves `resolve_env_hash` and leaves `type_env_hash`/`lib_env_hash`
untouched) and `resolve_env_does_not_fold_lib_dims` (a lib-only input moves
`lib_env_hash` and leaves `resolve_env_hash` untouched).

**R22.** Eviction is memory-bound, not correctness-bound. The reverse import
graph is content-addressed and serves reachability GC + LSP affected-files
reporting + diagnostics; it is never wired to cache invalidation. Live-content
reachability + a global LRU floor are the sole GC mechanisms.

### Observability & cost

**R23.** Audit events emitted by this refactor on cache subsystem call paths use
typed `StructuredAuditEvent` variants (`FileArtifactCache`, `FactRegistryWrite`,
`FactValidationSummary`, `ExportRouteResolved`, `FactSignatureAdmissionRefused`,
`FactSignatureOverflow`, `ModuleAugmentationStitched`,
`ModuleAugmentationIndexShape`, `CacheDrainedAtUpsert`). `Custom` events for the
new emissions on these paths are forbidden.

The `CacheDrainedAtUpsert { layer, canonical_id }` variant fires at every
own-canonical drain site reached by the full `host.upsert(...)` path (the
upserted canonical's own caches — no reverse-dependent cascade). The
quintuple-unchanged fast path (R1) emits ZERO `CacheDrainedAtUpsert` events;
that absence is the direct read-side proof that the fast path is a cache-state
no-op. The variant is emitted via the in-tree helper
`verter_session::host_manage::push_cache_drained_at_upsert(layer, canonical_id)`
so all drain sites in `host_upsert.rs` flow through one construction point.
Layer-string vocabulary (each is a `&'static str`): `compile_cache_overrides`,
`compile_slots`, `derived_raw_cache`, `dependency_cache`, `resolver_runtime`,
`project_type_store`, `resolved_type_cache`, `semantic_invalidate`,
`workspace_parsed_edges`, `store_view_epoch`.

**R24.** Warm cache validation is counter-only — zero allocation, zero
structured payload emission per hit. Structured events emit only on cold /
stale / admit-refused paths or when an observer is explicitly sampling.
Warm-hit validation must stay under 50µs p99 and must not allocate (asserted
via test-allocator counter).

**R25.** The fact read tracer (`FactReadSet` in `ResolverContext`) is active on
cold-compute and write-admission paths only. Warm hits validate stored
signatures directly without instantiating a tracer.

### Cache substrate

**`WorldSnapshot`** is the deterministic identity carrier for ONE in-flight
request. It is the lane identity that `cooperative_get_or_insert` coalesces on;
it is NOT a cache key. Cache layers project the snapshot down to scoped
dimensions via the `*_dims()` accessors (`parse_dims`, `resolve_dims`,
`type_dims`, `compile_dims`). Embedding the full snapshot as a single key field
on any cache layer violates R21 (the five env-hash dimensions must remain split
— bundling them into a single `project_config_hash` is forbidden).

Fields:

- `compat_token: StoreViewCompatToken` — singleflight lane identity (epoch +
  optional session); read through `ctx.store_view().compat_token()`.
- `project_identity, parse_env_hash, resolve_env_hash, type_env_hash,
  lib_env_hash: Hash16` — the five env-hash dimensions.
- `source_map_policy_hash, public_api_mode_hash: Hash16` — typed policy
  identities (typed `SourceMapPolicy` / `CompileCacheMode` enums introduced by a
  later block lower into these `Hash16`s through their `stable_hash()`
  conversions).
- `compiler_version, plugin_versions: Hash16` — host-side identity dimensions.
- `overlay_identity: Option<OverlayIdentity>` — base view is `None`, session
  overlay is `Some(OverlayIdentity(session_id))`.
- `generation: u64` — world generation under which the snapshot was constructed
  (stamped onto `CacheEntry.validated_at_generation` at admission).

`WorldSnapshot` lives at
`crates/verter_session/src/cache_runtime/world_snapshot.rs`. The struct is
`pub(crate)`; it is exercised internally by `#[cfg(test)] mod tests` in the
owning module — there is no `for_tests` re-export and no parallel
`for_tests_from_raw` constructor on the production type. Construction-discriminator
tests drive `from_request` through the bare-host
`impl ResolverContext for VerterHost` rail directly.

Architecture guard: `tests/world_snapshot_is_not_a_cache_key.rs` parses every
production `.rs` file under `crates/verter_session/src/` with
`syn::parse_file`, walks every `ItemStruct` whose name ends `Key` or `Identity`,
and rejects any field whose `syn::Type` mentions `WorldSnapshot` at a
Rust-identifier path segment — REGARDLESS of the field's name. The AST walk
descends through wrapper constructors (`Arc<>`, `Option<>`, `Box<>`, `Rc<>`,
`RefCell<>`, references, raw pointers), tuple-struct positions, tuple-element
compounds (`(WorldSnapshot, u64)`), and multi-line wrapped field declarations —
none of these launder the rejection. A synthetic discriminator suite proves the
predicate fires across every shape and does NOT fire on prefix/suffix lookalikes
like `WorldSnapshotShim`.

### Substrate, stack-safety, granularity

**R26.** `ValidatedFactCache<K, V>` is the substrate. Multi-candidate (R20), the
extended `FactVersionRef` variants, and the new lane / space metadata land
**inside** this primitive — not as a parallel cache type. No second
fact-validation infrastructure may be introduced.

`StoreView` trait surface evolves with explicit per-domain validator methods
routed via `FactKey::domain()`:

```rust
trait StoreView {
    fn compat_token(&self) -> StoreViewCompatToken;
    fn validates(&self, fact: &FactVersionRef) -> bool;
    fn validates_parse_domain(&self, fact: &ParseFactRef) -> bool;
    fn validates_resolve_imports_domain(&self, fact: &ResolveImportsFactRef) -> bool;
    fn validates_route_surface_domain(&self, fact: &RouteSurfaceFactRef) -> bool;
}
```

The dispatch table is bounded by `FactDomain` (3 variants), not by `FactKey`.
Adding a new `FactKey` extends the per-domain `*FactRef` enum but does NOT
widen the trait.

**R27.** All semantic fingerprint computation is **stack-safe**: implemented as
an explicit worklist with a `VisitedSet`. Cycles emit a stable
`CycleRef(visit_index)` placeholder. Visit order is canonical: lexicographic by
`(name, symbol_space)` at each unresolved-neighbor expansion; depth-budget
tie-break by `(canonical, name, symbol_space)`. `CycleRef` placeholder identity
is invariant under source-text reordering. Depth budget = 64; over-budget paths
emit `Opaque(BudgetExceeded)` and the cache entry is admitted as `NonCacheable`.

**R28.** Path-precise fact granularity is mandatory. Every cache that observes a
member, local, import edge, or member-presence must observe the corresponding
`Member` / `LocalDecl` / `MemberPresence` / `ImportRef` fact directly. Folding a
whole-file or whole-export closure into a single `semantic_hash` is forbidden.

**`MemberPresence` vs `Member` two-fact model** — orthogonal purposes and
orthogonal cache lifecycles. Consumers that select a specific member observe
BOTH:

| Fact | Purpose | Lifecycle | `semantic_hash` content |
|---|---|---|---|
| `MemberPresence(exporter, name, space)` | Existence + header | Phase 1 eager (parse-time) | `(name, member_kind, exporter_qualifier_salt)` |
| `Member(exporter, name, space)` | Canonical reusable member body fingerprint | Phase 2 lazy | Body alpha-normalised TypeExpr with cross-decl references kept as references |
| `MemberShape(exporter, space)` | Whole-surface fingerprint | Phase 1 eager | `sorted_by_name([(name, kind)])` |

Why both `MemberPresence` and `Member` exist: their cache lifecycles differ.
`MemberPresence` is cheap and eager — its body fingerprint is NOT included, so
adding member `b` does not force re-walking member `a`'s body. `Member` is the
canonical reusable navigation result — computed once per `(parse_stable_hash,
exporter, name, space)` and reused for every consumer that walks into the
member body.

Literal-key projections like `Pick<Foo, "a">` observe **`MemberPresence(Foo, "a")` + `Member(Foo, "a")`**:

- Adding `Foo.b` → new `MemberPresence(Foo, "b")` emitted; existing
  `MemberPresence(Foo, "a")` unchanged; `Member(Foo, "a")` unchanged → consumer
  NOT invalidated. `MemberShape` changes but is not in this consumer's
  signature.
- Editing `Foo.a` body → `MemberPresence(Foo, "a")` unchanged (header
  invariant); `Member(Foo, "a")` changes → consumer IS invalidated.
- Removing `Foo.a` → `MemberPresence(Foo, "a")` becomes a registry miss →
  consumer IS invalidated.

**R29 (Module augmentation).** Module augmentation is a fact-graph completeness
requirement. Parse-domain emits a syntactic fact `ModuleAugmentation {
specifier, augmented_name, space }` per `declare module 'x' { ... }` block.
Resolution of the augmentation target happens at the resolver stage, producing a
typed `AugmentationTargetKey`:

```rust
enum AugmentationTargetKind {
    ExternalSpecifier(InternedSpecifier),     // declare module "vue" {}
    ResolvedRelativeCanonical(Arc<str>),      // declare module "./local" {}
    WildcardAmbient(InternedGlobPattern),     // declare module "*.css" {}
    GlobalAugmentation,                       // declare global {}
}

struct AugmentationTargetKey {
    project_identity: ProjectId,
    resolve_env_hash: Hash16,
    lib_env_hash: Hash16,
    population: AugmentationPopulation, // {Base, Session(overlay-set fingerprint)}
    target: AugmentationTargetKind,
}
```

A `ModuleAugmentationIndex { entries: DashMap<AugmentationTargetKey,
Arc<AugmenterSet>> }` lives on `FileArtifactStore`, providing the inverse lookup
"which augmenters target X under env E and population P?". Project / env
isolation prevents cross-project poisoning, and the `population` dimension keeps
a session overlay's augmenters in a `Session` slot distinct from the `Base` slot
(see the **Overlay-aware contract** below).

**Index population semantics.** The augmentation index is populated
**incrementally** as files enter `FileArtifactStore`. There is NO workspace-wide
eager scan; out-of-program files contribute nothing — matching TypeScript's own
augmentation visibility rule. A reachability-anchored query that needs
`EffectiveExportSet(specifier)` triggers `ResolvedImportFacts` resolution for
the relevant import graph, which pulls in any augmenter that is part of that
reachability.

**Augmenter-set identity fact.** A parse-domain-derived fact
`ModuleAugmentationIndexShape { target: AugmentationTargetKey,
augmenter_set_fingerprint }` is observed by `EffectiveExportSet(specifier)`
consumers. `augmenter_set_fingerprint = stable_hash(sorted([(augmenter_canonical,
augmenter_parse_stable_hash)]))`. Adding / removing an augmenter changes the
fingerprint → existing `EffectiveExportSet` candidates invalidate.

**Stitching pipeline.** `RouteDb::get_or_compute_effective_export_set` is the
cold path. It calls
`FileArtifactStore::ensure_augmentation_index_populated(target_key)` on first
miss — the scan walks every loaded `FileArtifacts.augmentations`, sorts matched
augmenters by `(canonical, parse_stable_hash)`, computes the fingerprint,
inserts into `augmentation_index`, and emits a typed
`ModuleAugmentationIndexShape` audit event recording the install (or refresh on
re-population). Each `AugmenterSet` entry is an `AugmenterEntry` carrying the
**exact** `FileArtifactKey` of the scanned augmenter artifact (plus its
`parse_stable_hash`) — the stitcher re-fetches each augmenter's `.augmentations`
via `FileArtifactStore::get_artifacts(&key)` keyed by that exact key, never a
content-agnostic canonical-only scan, so the stitch reads precisely the
augmenter version the fingerprint was computed over. The stitcher then folds
each augmenter's `(augmented_name, space)` contributions into an
`EffectiveExportSetEntry { entries, augmenter_count, augmenter_set_fingerprint,
fact_dep_signature }`. The `fact_dep_signature` records the
`RouteSurface(ModuleAugmentationIndexShape)` fact plus per-contributor
`FileWholeHash` anchors so:

- adding / removing an augmenter changes the augmenter-set fingerprint →
  consumer invalidates (G1);
- editing one augmenter's body changes that file's whole hash → consumer
  invalidates;
- editing an unrelated file (not in the augmenter set) leaves the signature
  valid → consumer warm-hits (R14 / R28 narrow scope).

A typed `ModuleAugmentationStitched` audit event records each cold-path compute.
Both audit-event variants live on `verter_audit::StructuredAuditEvent`; the
`Custom` escape hatch is forbidden on the augmentation stitching surface (R23
scope-fence guarded by `tests/audit_event_shape.rs`).

**Overlay-aware contract (R6 key split).** Augmentation stitching is
OVERLAY-AWARE across two layers with deliberately DIFFERENT key discipline:

- The **content-addressed** augmentation index (`AugmentationTargetKey` on
  `FileArtifactStore`) carries `population: AugmentationPopulation {Base,
  Session(overlay-set fingerprint)}`. A `Base` scan filters to base
  (`FileArtifactKey::is_legacy`) artifacts; a `Session` scan unions the
  session's own overlay augmenters (matched by the session overlay discriminator
  derived from `SessionView::fingerprint`) with base. The fingerprint
  legitimately keys this slot because the index is a content-addressed compute
  cache — the fingerprint IS part of its content view identity, and it
  self-invalidates when overlay content/membership changes (new fingerprint →
  fresh scan). A `Session` slot is overlay ∪ base, distinct from `Base` without
  poisoning it; a base augmenter change invalidates the `Session` entries that
  include it.
- The **query-identity** `EffectiveExportSetKey` (on `RouteDb`) is keyed by the
  CONTENT-FREE `session_scope: EffectiveExportSetScope {Base,
  Session(session_scope_id)}` — the `StoreViewCompatToken::session` (R6: the
  overlay-set content fingerprint NEVER enters a query-identity key). Base and
  session reads occupy DISTINCT slots, so a base warm entry can never satisfy a
  session lookup (the "base-as-session" hazard) and vice-versa. Overlay CONTENT
  identity is rooted on the VALUE's `fact_dep_signature` (the
  `ModuleAugmentationIndexShape` augmenter-set fingerprint fact +
  per-contributor `FileWholeHash` anchors), revalidated against the live view on
  every warm hit — so a within-session overlay edit invalidates through facts,
  not through a new key.

Both producers (the body stitch in `project_semantic_dispatch::build` and
`RouteDb::get_or_compute_effective_export_set`) derive the content-addressed
`(population, overlay_discriminator)` through the single shared
`session_view::augmentation_population_for_view`; the content-free
`EffectiveExportSetScope` and the route-surface validator's `EffectiveExportSet`-fact
slot lookup both derive from
`EffectiveExportSetScope::from_session(compat_token().session)`. Overlay
augmenters NEVER poison the base index and NEVER cross sessions. The contract is
locked by `tests/g_misc3/module_augmentation_stitching.rs` —
`session_overlay_augmenter_isolated_from_base_index`,
`effective_export_set_warm_base_entry_does_not_satisfy_session_lookup`,
`effective_export_set_warm_session_entry_does_not_satisfy_base_lookup`,
`effective_export_set_same_session_overlay_content_edit_invalidates_via_facts`,
`effective_export_set_content_free_key_warm_hits_across_unrelated_fingerprint_change` —
plus the guard `no_effective_export_set_base_only_session_assert`
(`tests/g_misc0/critical_rules_have_guards.rs`) which pins that NO base-only
`compat_token().session.is_none()` assert is re-introduced on this surface.

**R30 (No heuristic cache semantics).** Cache identity and cache admission must
not encode semantic policy through local heuristics. Every dimension that can
change the returned type, published members, alias/provenance shape, exactness,
or completeness must be represented as one of:

- a typed cache-slot key dimension;
- a per-mode/per-policy entry inside a shared cache family;
- cached-value validation metadata such as a fact signature, self-root set, or
  store-view constraint;
- explicit result state such as `Exact`, `SurfaceOnly`, `Unsupported`,
  `BudgetExceeded`, or a recursion sentinel.

Rendered type text, raw source slices, path substrings, identifier spelling
conventions, arbitrary numeric caps, and candidate "better shape" scoring are
not cache correctness mechanisms. A fast path or gate may decide that work
should stay cold, avoid admission, or enter the shared semantic query layer; it
must not return or admit a different semantic result than the typed query
contract would produce. If a cached result depends on projection mode,
substitution environment, conditional context, package/workspace policy, scope
version, source generation, solver options, or component-meta schema options,
that dependency must be visible in the cache family shape or the cached value's
validation/provenance. Otherwise the result is non-cacheable.

**R31 (Exact policy identity and complete-result admission).** Query policy
dimensions are typed identities, not compressed flags. Projection mode,
traversal mode, package-boundary policy, output schema/options, substitution
environment, conditional context, and solver profile must be represented
exactly in either the cache slot, the per-policy entry, or the cached value's
validation/provenance. A boolean such as `is_navigate`, a mode-erased string,
or an enum subset is not a valid key when two concrete policies can diverge now
or after a new mode is added.

Cache admission also depends on result completeness. Entries may be admitted to
warm shared caches only when the value is complete for the policy it claims to
satisfy and its read-set/fact signature is known. `Partial`, `Unavailable`,
`Unsupported`, `BudgetExceeded`, `UnstableState`, recursion-sentinel,
bridge-truncated, and cancelled results may be returned to a caller, but they
must be marked non-cacheable or cached only in a typed degraded-result cache
whose callers cannot mistake them for exact answers.

## Cache layer key composition

See `docs/arch/fact-based-cache.md` for the canonical per-cache-layer key
composition table. Summary:

| Layer | Family | Key dimensions |
|---|---|---|
| `FileArtifactStore` | Content-addressed | `canonical, content_hash, parse_env_hash, parser_version` — the authoritative per-file storage layer; stores `IndexedReady`, `FileFacts`, `ParsedEdges`, `parse_stable_hash`, `augmentations` |
| `ModuleAugmentationIndex` (on `FileArtifactStore`) | Content-addressed | `project_identity, resolve_env_hash, lib_env_hash, population, target` (`population: AugmentationPopulation {Base, Session(overlay-set fingerprint)}`) |
| `ResolvedImportFacts` | Content-addressed | `canonical, content_hash, parse_env_hash, resolve_env_hash, resolver_version` (**no `lib_env_hash`** — R21) |
| Typed-IR resolve | Content-addressed | `canonical, content_hash, parse_env_hash, type_env_hash, lib_env_hash, parser_version` |
| `MemberSemanticFactStore` | Content-addressed | `canonical, parse_stable_hash, parse_env_hash, exporter, member_name, symbol_space` |
| `MemberDisplayFactStore` | Content-addressed | `canonical, content_hash, parse_env_hash, exporter, member_name, symbol_space` |
| `RouteDb` per-name | Query-identity (multi-candidate) | `RouteNameKey { provider_canonical, exported_name, symbol_space, project_identity, resolve_env_hash, lib_env_hash, resolver_version }` (content-free; routes are resolve-domain so no `parse_env`/`type_env`, but `lib_env` enters — augmentations stitch the surface). Value-side `ValidatedFactCache` fact validation. |
| `RouteDb` effective barrel surface | Query-identity (multi-candidate) | `BarrelSurfaceKey { barrel_canonical, project_identity, resolve_env_hash, lib_env_hash, resolver_version }`. Value-side `ValidatedFactCache` over `BarrelRouteSurface.fact_dep_signature`. |
| `MaterializeStructureDb` | Query-identity (multi-candidate) | `MaterializationCacheKey { decl: ResolvedDeclSlotIdentity, projection_path: RouteDemand, scope_axis, projection_mode, normalized_type_args: Arc<[SemanticNodeId]>, resolve_env_hash }` — content-free canonical SUBJECT (the slot, NOT a graph-instance `SemanticNodeId`); `normalized_type_args` carries `SemanticNodeId`s exactly as `SemanticQueryKey::Instantiate.args`. The per-thread recursion identity is the SEPARATE `MaterializeRuntimeKey { base: SemanticNodeId, scope_axis, mode }` (NOT a cache key). Root-less anonymous subject ⇒ uncached. Value-side `ReadSetSignature.facts` + `self_root_canonicals` (base node's decl-origin file) + `validated_at_generation`. |
| `RefCycleResultDb` | Query-identity (multi-candidate) | `RefCycleResultKey { root: ResolvedDeclSlotIdentity, resolve_env_hash, version }` — content-free (NOT the versioned `DeclIdentity`). Value-side `ReadSetSignature.facts` + `self_root_canonicals` (BFS root + every visited decl's file) + `validated_at_generation`. |
| `SemanticGraphStore` query nodes | Query-identity (multi-candidate) | `SemanticQueryKey` slot identity (e.g. `Instantiate { base: ResolvedDeclSlotIdentity, args }`); the memo value version-roots on `ReadSetSignature.facts` + `self_root_canonicals`. |
| `ShapeCacheDb` per-member slot | Query-identity | `ShapeCacheKey::semantic_node_whole(scope, member SemanticNodeId, mode)` (`ShapeSubject::SemanticNode`); writes record `ReadSetSignature.facts` + `validated_at_generation` |
| `ComponentMetaResultDb` | Query-identity (multi-candidate) | `ComponentMetaResultKey { owner_canonical, options_fingerprint, project_identity, parse_env_hash, resolve_env_hash, type_env_hash, lib_env_hash }` — content-free (owner whole-hash is the VALUE-side candidate discriminant, never a key field). Value-side owner whole-hash candidate + `ReadSetSignature.facts` + `validated_at_generation`. |

The split `MaterializeMemoDb`/`MemberShapeCacheDb` shape stores are RETIRED in
favour of `ShapeCacheDb`; the static guard
`crates/verter_session/tests/block_6i_static_guards.rs::shape_cache_db_replaces_split_caches`
asserts neither may be re-introduced.

## Two-phase emission map (R28)

Parse-time emission (eager, shallow, O(file_size)) populates the parse-domain
`FactRegistry` on `FileArtifacts.facts`. The producer is
`verter_session::fact_emission::emit_parse_facts(&IndexedReady)`, which emits:

- `Export(name, space)` — per locally-declared exported binding.
- `LocalDecl(name, space)` — per NOT-exported local.
- `MemberShape(exporter, space)` — whole-surface fingerprint.
- `MemberPresence(exporter, name, space)` — header-only `(name, kind,
  exporter_salt)`; NO body fingerprint.
- `SyntacticExportSet` — whole-file surface fingerprint.
- `ImportRef(specifier, binding, space)` — syntactic import shape; NO resolved
  canonical (R12).
- `SyntacticReexportRef(specifier, source_name, target_name, space)`
- `ExportAlias(exported_as, space)` — per `export {X as Y}`.
- `ModuleAugmentation(specifier, augmented_name, space)` — per augmented binding
  inside each `declare module "X" {…}` / `declare global {…}` block.

Lazy emission (member body, on first member-access query) lives in TWO separate
stores keyed differently to physically separate semantic vs display:

| Store | Key | Keys-on |
|---|---|---|
| `MemberSemanticFactStore` | `(canonical, parse_stable_hash, parse_env_hash, exporter, member_name, symbol_space)` | `parse_stable_hash` — cosmetic edits do NOT re-key |
| `MemberDisplayFactStore` | `(canonical, content_hash, parse_env_hash, exporter, member_name, symbol_space)` | `content_hash` — cosmetic edits DO re-key |

Both stores admit through `entry().or_insert(...)`: insert-only-if-absent.
Producer races for the same key collapse to one canonical fact; downstream
consumers observe pointer-equal `Arc<Fact>` for the same key.

## R27 worklist algorithm

`verter_semantic::facts::hashing::compute_semantic_hash(body, space, lens) ->
HashOutcome` is the stack-safe + cycle-safe + path-precise fingerprinter.
Contract:

- **Stack-safe**: explicit `depth` counter, hard cap `MAX_HASH_DEPTH = 64`.
  Over-budget walks set `HashOutcome.budget_exceeded = true` and emit
  `BUDGET_EXCEEDED` placeholder bytes. Producers MUST admit the cache entry as
  `NonCacheable` (the admission guard lives downstream).
- **Cycle-safe**: per-node identity-key `VisitedSet` (`BTreeMap`). Re-entry
  through a node emits `CycleRef(visit_index)` placeholder rather than recursing.
  Identity key folds in `Arc` pointer addresses of owned sub-nodes — two
  physically identical `Arc`s ARE the same node and re-enter as `CycleRef`.
  Different `Arc`s carrying structurally identical content are distinct nodes.
- **Canonical visit order**: lexicographic by `(name, symbol_space)` at each
  unresolved-neighbor expansion; tie-break by `(canonical, name, symbol_space)`.
  `CycleRef` placeholder identity is therefore invariant under source-text
  reordering.
- **Path-precise**: cross-decl references resolve through `CrossDeclLens` and
  emit reference-shape edges (`LocalDecl(name, space)`, `ImportRef(spec, binding,
  space)`, `TypeOfRef(name)`, `Unresolved(name, space)`) WITHOUT inlining the
  referent's body (R14). Free type parameters alpha-normalise to binder-relative
  indices.

## MemberPresence vs Member detailed table (R28)

| Consumer touch | Observes |
|---|---|
| `Pick<Foo, "a">` | `MemberPresence(Foo, "a")` + `Member(Foo, "a")` |
| `Omit<Foo, "a">` | `MemberShape(Foo)` (whole surface) |
| `keyof Foo` | `MemberShape(Foo)` |
| `Foo["a"]` | `MemberPresence(Foo, "a")` + `Member(Foo, "a")` |
| `Foo["a"]["b"]` | both `Member` chains observed |
| `{ [K in keyof Foo]: T }` | `MemberShape(Foo)` |

The discrimination matrix:

- Adding `Foo.b` → new `MemberPresence(Foo, "b")` emitted; existing
  `MemberPresence(Foo, "a")` unchanged; `Member(Foo, "a")` unchanged.
  `MemberShape` changes but `Pick<Foo, "a">` consumer does NOT observe
  `MemberShape` → NOT invalidated.
- Editing `Foo.a` body → `MemberPresence(Foo, "a")` unchanged (header invariant);
  `Member(Foo, "a")` changes → consumer IS invalidated.
- Removing `Foo.a` → `MemberPresence(Foo, "a")` becomes a registry miss →
  consumer IS invalidated.

## Key concrete files

- `crates/verter_workspace/src/env_hash.rs` — five env-hash functions on
  `IdeProjectConfig` + `EnvHashInputs<'_>`.
- `crates/verter_semantic/src/facts/registry.rs` — `FactKey`, `Fact`,
  `FactDomain`, `FactRegistry`, `SymbolSpace`, `MemberKind`, `FactLane`,
  `ObservedFact`, `MacroKind`, `MacroTargetKey`, `InternedSpecifier`,
  `InternedName`, `InternedGlobPattern`, `AugmentationTargetKindTag`.
- `crates/verter_semantic/src/facts/hashing.rs` — `compute_semantic_hash`,
  `compute_member_presence_hash`, `compute_member_shape_hash`, `CrossDeclLens`,
  `CrossDeclRef`, `HashOutcome`, `MAX_HASH_DEPTH = 64`.
- `crates/verter_session/src/file_artifact_store.rs` — `FileArtifactStore`,
  `FileArtifactKey`, `FileArtifacts`, `FileFacts` (registry-backed); re-exports
  `Interned{Specifier,Name,GlobPattern}` + `SymbolSpace` from
  `verter_semantic::facts::registry`; `AugmentationTargetKey`,
  `AugmentationTargetKind`, `AugmenterSet`, `AugmenterEntry`, `ParsedEdges`,
  `ModuleAugmentationFact`, `ProjectIdentity`.
- `crates/verter_session/src/fact_emission.rs` —
  `emit_parse_facts(&IndexedReady) -> ParseFactsEmission`,
  `GLOBAL_AUGMENTATION_TAG`, single-pass
  `extract_module_augmentations_from_source` byte scanner.
- `crates/verter_session/src/member_semantic_fact_store.rs` —
  `MemberSemanticFactStore`, `MemberSemanticFactKey`, `make_member_fact`,
  `member_fact_key`.
- `crates/verter_session/src/member_display_fact_store.rs` —
  `MemberDisplayFactStore`, `MemberDisplayFactKey`, `make_member_display_fact`,
  `member_display_fact_key`.
- `crates/verter_session/src/parse_stable_hash.rs` —
  `compute_parse_stable_hash(&IndexedReady) -> Hash16`. `parse_stable_hash` is
  a structural hash over the post-shallow-analysis decl skeleton, invariant
  under cosmetic edits.
- `crates/verter_session/src/resolver_core/mod.rs` — `ValidatedFactCache`,
  `FactVersionRef` (legacy + `Parse`/`ResolveImports`/`RouteSurface` per-domain
  variants), `ParseFactRef`, `ResolveImportsFactRef`, `RouteSurfaceFactRef`,
  `StoreView` (per-domain validator methods), `StoreViewCompatToken`.
- `crates/verter_session/src/semantic_query.rs` — `DeclIdentity` (the live
  value-side versioned identity) and `ResolvedDeclSlotIdentity` (the env-bearing
  content-free query-identity slot used as `Instantiate.base` /
  `ResolveMacroPayload.owner`; the migration to it has LANDED — the former
  content-free `DeclKey` query-key struct and `to_decl_key()` were deleted in
  the cutover).

## Discriminating tests

- `crates/verter_workspace/src/env_hash_tests.rs` — env-hash split unit tests
  (R21).
- `crates/verter_session/src/file_artifact_store_tests.rs` — store unit tests
  (R5, R6, R28, R29).
- `crates/verter_session/tests/env_hash_isolation.rs` — R21 scoping rule tests.
- `crates/verter_session/tests/cache_key_invariants.rs` — R5 / R6 key-shape
  tests.
- `crates/verter_session/tests/parse_stable_hash_invariance.rs` —
  cosmetic-invariant + decl-shape-discriminating tests.
- `crates/verter_session/tests/file_artifact_store_smoke.rs` — consumer-side
  smoke tests.
- `crates/verter_semantic/src/facts/registry.rs` (`registry_tests` inline
  module) — `FactKey::domain()` routing per R12 / R26, `SymbolSpace` tag
  stability per R11.
- `crates/verter_semantic/src/facts/hashing.rs` (inline `tests` module) —
  alpha-normalisation under object member reorder (R16), stack-safety on
  200-deep nesting (R27), `MemberPresence`/`MemberShape` discrimination (R28).
- `crates/verter_session/src/fact_emission.rs` (inline `tests` module) —
  `declare module …` source-byte scanner per R29 archetype.
- `crates/verter_session/src/member_semantic_fact_store.rs` /
  `member_display_fact_store.rs` (inline `tests` modules) — store admission,
  parse_stable_hash vs content_hash keying contract (R13).
- `crates/verter_session/tests/fact_fingerprint_stability.rs` —
  R10/R11/R13/R16 binding (cosmetic-invariance, namespace coexistence,
  decl-reorder stability, syntactic export set).
- `crates/verter_session/tests/fact_semantic_display_split.rs` — R13 binding
  (semantic store survives cosmetic edit; display store re-keys).
- `crates/verter_session/tests/parse_resolve_domain_separation.rs` — R12 binding
  (`ImportRef` invariant under resolution change).
- `crates/verter_session/tests/member_presence_vs_member.rs` — R28 two-fact
  model (`pick_literal_key.ts` invariant).
- `crates/verter_session/tests/cycle_safety.rs` — R27 binding (stack-safe +
  cycle-safe + canonical visit order).
- `crates/verter_session/tests/shallow_walk_invariant.rs` — R28 arch-guard
  (parse-time emitter does not call cross-decl AST traversal).
- `crates/verter_session/tests/module_augmentation.rs` — R29 binding
  (per-archetype fact emission; `augmentation_index` stays empty at parse time).
- `crates/verter_session/tests/declaration_merge_facts.rs` — R10 binding
  (merged `interface Foo` parts emit one `Export`).
- `crates/verter_session/tests/fact_lane_correctness.rs` — R13 lane binding
  (generic param rename invariant).
- `crates/verter_session/tests/fact_emission_parse_time_budget.rs` — emitter
  scales linearly on 10k-decl input.
- `crates/verter_session/tests/storeview_per_domain_dispatch.rs` — R26 binding
  (dispatch table bounded by `FactDomain`, not `FactKey`).
- `crates/verter_session/src/session_view_current_content_tests.rs` — R17
  binding (`SessionView::content_hash_for` is a view-authoritative
  current-content oracle: base + overlay fallthrough return the
  scheduler-authoritative hash, never a stale lingering artifact's; the overlay
  materialiser does not serve a stale `IndexedReady`).
- `crates/verter_session/tests/architecture_guards.rs`
  (`content_pinned_artifact_read_guards` module) — the named-currency-oracle
  closure: `file_artifact_store_defines_no_unpinned_currency_oracle`
  (definition-shape guard) + `no_named_currency_oracle_calls_in_production`
  (call-site ban) + a discriminating self-test.

## Block-vocabulary ban (CRITICAL)

Source comments under `crates/*/src/**` must not contain plan vocabulary
specific to the cache-runtime overhaul. The banned patterns are `\bblock \d+\b`,
`cache-runtime overhaul`, and `runtime cutover`. Production source must read as
final-state once the work lands; plan-management vocabulary leaks once-relevant
project context into the durable code base.

The guard is `architecture_guards::guard7_predicate_rejects_block_vocabulary`,
which sits inside the broader `no_phase_archaeology_in_production_code` walker at
`crates/verter_session/tests/architecture_guards.rs`. The walker scans every
production `.rs` file under `crates/*/src/` (excluding `_tests.rs`, `tests.rs`,
`tests/`, `benches/`, `examples/`, and `target/`).

Durable architectural insights belong here in
`.claude/skills/type-cache-architecture/SKILL.md` or in `docs/arch/` — not in
source comments referencing plan blocks.

### U2 Value-Domain Key Identity (CRITICAL)

The U2 query-value-domain design (`docs/arch/u2-query-value-domain-design.md`)
locks how env dimensions attach to semantic-query keys. The cache-key
composition is two-tier, env stays on the key, and no env-less uniform envelope
is permitted.

- **Two-tier env model.** `ResolvedDeclSlotIdentity` bakes its three intrinsic
  decl-site dimensions (`type_env_hash`, `lib_env_hash`, `project_identity`) as
  DECLARATION IDENTITY. Each per-key `*Context` then adds ONLY the extra QUERY
  dimensions that key depends on — chiefly `resolve_env_hash` for
  import-resolving keys and `parse_env_hash` for body-reading keys. This keeps
  R21 per-layer honest (each layer carries exactly the dims it depends on) and
  stays R6-clean: NO content/version hash and NO `fact_dep_signature` on any
  query-identity key.
- **No env-LESS envelope.** The superseded env-less uniform-envelope design —
  `SemanticQueryEnvKey`, `TypeLibEnvKey`, and the "U2a/U2b" split — is
  FORBIDDEN. Env stays ON the key via per-key `*Context`; it is never lifted
  into a separate uniform env envelope. The env-less KEY types
  (`SemanticQueryEnvKey`, `TypeLibEnvKey`) must appear NOWHERE in production
  source. The env-less `GraphDeclSlotRef` query-identity wire slot HAS BEEN
  RETIRED (deleted from the proto and the `verter_protocol` typed surface) in
  favour of the env-BEARING `GraphResolvedDeclSlotIdentity`
  (`GraphQueryIdentity.resolved_roots`, tag 18; `TYPEINFO_GRAPH_SCHEMA_VERSION`
  bumped to 2, retired `roots` tag/name reserved at message scope). The
  `no_envless_semantic_query_env_key_envelope` guard now bans the
  `GraphDeclSlotRef` symbol across ALL `crates/*/src/**`, and
  `typeinfo_proto_retires_envless_decl_slot_ref` pins the proto surface.
- **Module-resolution keys on SPLIT env.** Each import-resolving surface keys
  only on the dims it consults: `ResolvedImportFacts` keys on `{parse_env_hash,
  resolve_env_hash}` (moduleResolution / paths / baseUrl / conditions /
  extension order) and NEVER `lib_env_hash`; the augmentation-aware
  `EffectiveExportSet` surface additionally keys on `lib_env_hash` (typeRoots /
  types corpus) + `project_identity` because module augmentations stitch in. Lib
  dimensions are NEVER folded into `resolve_env`. See
  `### Module-Resolution Keying (CRITICAL)`.
- **Slot-keyed `Instantiate` / `ResolveMacroPayload` / `TypeOf`.**
  `SemanticQueryKey::Instantiate` / `ResolveMacroPayload` key their `base` /
  `owner` on the env-bearing, content-free `ResolvedDeclSlotIdentity`;
  `SemanticQueryKey::TypeOf` keys its `value_root` on the env-bearing,
  content-free `ValueRootSlotIdentity` (the extra
  `resolve_env_hash` rides on the per-key `InstantiateContext` /
  `MacroPayloadContext` / `TypeOfContext`). The `provenance` + `merge_role`
  discriminators STAY at
  FAMILY-IDENTITY level on `FamilyKey` — they are NOT demoted into a `*Context`.
- **Discriminating guards.** The env-scoping and value-domain guards are landed:
  `every_semantic_query_key_maps_to_exactly_one_value_domain`,
  `module_resolution_keys_on_resolve_env_not_type_or_lib`, and
  `semantic_query_key_spec_table_equals_enum`
  (`crates/verter_session/tests/g_block/`); the per-key `*_do_not_warm_hit` set
  — including `instantiate_same_base_different_env_or_context_do_not_warm_hit`,
  `decl_self_type_or_lib_env_change_produces_distinct_instantiate_key`,
  `resolve_macro_payload_same_owner_different_env_or_context_do_not_warm_hit`,
  and `resolved_named_type_key_identity_is_env_scoped` — lives in
  `crates/verter_session/src/semantic_query_memo/tests.rs`. The design-gate
  guards `no_envless_semantic_query_env_key_envelope` and
  `u2_value_domain_design_doc_locks_invariants` live in
  `crates/verter_session/tests/g_block/u2_value_domain_design_guards.rs`.

## Related skills

- `/architecture` — high-level module map
- `/host-session` — `VerterHost` lifecycle + `ProjectTypeStore`
- `/type-resolution` — cross-file type resolver + macro traversal
- `/component-meta` — downstream consumer
- `/audit-infrastructure` — `StructuredAuditEvent` extensions
