# Verter

Verter = a Vue compiler + Language Server Protocol (LSP) implementation. Converts Vue Single File Components (SFCs) to valid TSX (TypeScript type-checks them) and compiles templates to optimized render functions. Unlike Volar, Verter generates real valid TSX, not virtual files.

Hybrid Rust + TypeScript monorepo: Rust crates handle template compilation (exposed via NAPI-RS native bindings and wasm-bindgen WASM) and the LSP server (`verter_lsp` binary, stdio); TypeScript packages handle SFC-to-TSX transformation and IDE integration.

## Architecture

Detailed module reference, key files, and implementation specifics live in domain skills: `/type-resolution`, `/type-cache-architecture`, `/component-meta`, `/compiler-codegen`, `/host-session`, `/architecture`.

### Shared Optimized Codebase (CRITICAL)

Verter is one shared optimized codebase, not separate semantic implementations per consumer.

- Improvements land in the lowest reusable owner crate that can correctly serve all consumers.
- `verter_session` + shared workspace/VFS integration are the authority for host-backed loading, invalidation, dependency tracking, cache reuse.
- `verter_semantic` + `verter_compiler` own reusable semantics, lowering, codegen.
- `verter_session::resolver_core` owns the host-backed resolver stack + type-resolution orchestration.
- `verter_audit` is the leaf observability substrate — owns the `RequestAuditRecord` envelope, `RequestKind`/`RequestKindPayload` discriminants, all per-kind payload structs, the `AuditObserver` trait + `AuditEvent` counter hook, the `StructuredAuditEvent` enum, `AuditConfig` + consumer filter, the trivial `NoOpObserver`. Depends only on `verter_span`, no back-edge to higher crates; lower crates emit through `current_observer()` (TLS) without knowing whether a `HostAuditRuntime` is installed. The concrete host runtime, records store, registration lifecycle, accumulator, footprint miner, peak-RSS sampler live in `verter_session`.
- `verter_protocol` owns transport-facing schema DTOs; `verter_ffi` remains the thin native/WASM adapter layer.
- Consumer packages (`@verter/component-meta`, LSP, MCP, unplugin, playground) consume the shared substrate, not their own semantic forks.

Architectural consequence:

- A perf/correctness fix found in one surface is implemented in the shared owner layer whenever the behavior is reusable.
- Consumer-local wrappers stay thin and do not bypass shared parsing, analysis, resolution, or cache ownership.

**Exactly one type-resolution engine.** The shared typed-IR dispatch — `SemanticQueryKey` → `ProjectSemanticDispatch::execute` → `SemanticGraphStore`, with the five query modes (`Identity`/`Navigate`/`Shallow`/`Expanded`/`Skeleton`) — is the SOLE query-time type resolver. OXC is the syntax/lowering front-end ONLY: `verter_type_expr_oxc::lower_ts_type` lowers TS source to `TypeExpr` once during shallow analysis (stored on `IndexedReady`). OXC must never resolve types at query time. Any second query-time resolution path — a parallel `resolve_type` engine, a per-surface walker, a re-parse-and-resolve, an OXC element/frontier resolver — is a rule violation: delete it, route through the shared resolver. Two engines diverge; divergence is the bug/hang class. When every consumer resolves through the one engine, per-path bugs disappear.

### Build Philosophy (CRITICAL)

Same end-state philosophy as `binary-exploring-lamport.md`. Core rules:

1. Read, parse, shallow-process, cache each canonical file once per content hash through one shared host path.
2. Store the full shallow symbol inventory up front, then process only requested items on demand.
3. Same-file closure stays local to the owning file.
4. Cross-file deepening happens in one place only, one import level at a time.
5. The builder/solver reads only from cached lookup state; it does not reopen file loading or routing.
6. The design is demand-driven and query-scoped.
7. The final implementation lands as one clean cutover, not a merged dual-path transition.
8. Component-meta, LSP, MCP, and other host-backed consumers share the same file-ready/read/parse/shallow-process lifecycle.

These are architecture rules, not optimization hints. On conflict, fix the owner layer or delete the legacy path rather than preserve a second read/parse/resolution flow.

### Shallow File Processing Core Invariant (CRITICAL)

The shallow file process is a core architectural invariant and must be preserved. When a canonical file is processed, the host stores its shallow symbol inventory once; that inventory is the authoritative index later stages query.

Shallow state must classify and retain at minimum: imports; exports and reexports; type declarations; interfaces; enums; classes; variables/constants; functions/method signatures; `typeof`-relevant value declarations; local symbol dependency edges; cross-file dependency edges.

Design rule: processing a file means collecting and indexing its symbols, not eagerly evaluating them; later stages look up the indexed items they need and process only those on demand; no stage rescans the raw file to rediscover symbols shallow processing already captured. Performance: very high performance comes from targeted demand after broad shallow indexing, not repeated partial reparsing.

Architectural target for the project-global cache cutover:

- canonical post-parse artifact = `IndexedReady`; it owns canonical imports/exports plus compact owned symbol indexes, spans, operator tags, interned names, and shallow bodies safe for host-owned `Send + Sync` caches.
- parse each live file version once through the scheduler, then lower only the shallow syntax later passes need into the long-lived shared representation.
- transient OXC parse arenas are per-file/per-version only; drop after lowering, never leak into host-owned shared caches.
- component-meta and later analysis layers both build from `IndexedReady`.
- if analysis or component-meta expands a symbol, populate and reuse the same shared resolver caches — no separate expansion paths.
- type navigation stays narrower than expansion: walking `A['c']['full']['bar']` navigates intermediate hops and expands only the terminal requested projection unless limited normalization is required to continue.
- generic substitutions are semantic meaning; navigation/expansion operate on instantiated types and cache keys include the relevant substitutions/type arguments.
- navigators stay non-owning: they choose the next hop and may do non-owning normalization, but reusable semantic work enters through the shared query API, not a private drill-down path.
- the shared semantic layer is keyed by semantic query identity and stores immutable semantic data or ids, not borrowed AST pointers or retained parser arenas.
- top-level live-host results publish through a completion fence: record touched dependency facts, revalidate before publish, retry at most 3 times on mid-flight changes, never warm shared caches with torn provisional results.
- distinct top-level waiters on in-flight semantic/artifact work block cooperatively on completion, not busy-spin; same-path recursion never self-awaits.
- reusable cache population is path-independent: the same semantic result computed from different entry points populates the same shared cache entry.
- broader successful results may backfill narrower entries they actually satisfied; narrower results must not pretend broader work is cached.
- final payload caches hand out immutable `Arc` values; any backend preserving concurrency, size bounds, validation semantics is fine.
- cancelled, superseded, interrupted, budget-exceeded, or partial semantic results must not be promoted as warm shared cache entries.

**Project-global cache (final state):** `VerterHost` owns a single `ProjectTypeStore` accessed via `.project_type_store()`. The store owns `FileArtifactStore`, `AnalysisReadyDb`, the rehomed `RouteDb`, `OwnerImportSurfaceDb`, `ComponentMetaResultDb<ComponentMetaAnalysis>`, `MaterializeStructureDb`, `RefCycleResultDb`, `SemanticGraphStore`, `ShapeCacheDb`, and the `IntrinsicRegistry`. Vue macro resolution artifacts (the former `ResolvedNamedTypesDb`) live inside `SemanticGraphStore` via `SemanticNodeData::VueMacroElements` + the `HostResolvedNamedTypeKey` identity map — the parser's `NamedTypeCache` adapter hits the graph directly on the refcount-only hot path via `SemanticGraphStore::get_resolved_named_type`. `IndexedReady` is the single canonical post-parse artifact (the former `ModuleFactsDb` is retired). `get_component_meta` consults the final-result cache first (revalidating the entry's `ReadSetSignature.facts` against the live `StoreView`) before falling back to the cold resolver. Direct owner imports resolve through `resolve_owner_direct_import` once per `(owner, whole_hash)`. Structural materialisation routes through `materialize_component_meta_structure` and publishes into `MaterializeStructureDb` via cooperative-admission `post_publish` (the legacy walker's per-shape materialiser DB was retired — see `tests/no_legacy_walker.rs::RETIRED_SYMBOLS`). Per-member projection routes through the graph-native member reducer + the per-member slot of `ShapeCacheDb` indexed by `ShapeSubject::SemanticNode` (built via `ShapeCacheKey::semantic_node_whole(scope, member SemanticNodeId, mode)`): each per-member shape query peeks the cache BEFORE any `raise_node_to_type_expr(member.value)` round-trip; cache writes record the observed `ReadSetSignature.facts` + `validated_at_generation`; warm reads validate both gates before return. The split `MaterializeMemoDb`/`MemberShapeCacheDb` shape stores are RETIRED — the static guard `crates/verter_session/tests/block_6i_static_guards.rs::shape_cache_db_replaces_split_caches` asserts neither may be re-introduced. The `reduce_field_type_expr_with_mode` TypeExpr path is the entry for `reduce_published_field_types`' parser-side callers that genuinely start from `TypeExpr` (props, emits, slot bindings, model bindings), not `SemanticNodeId`; every published macro surface routes through it with `ProjectionMode::Navigate` (shallow-by-default). Transitive cycle detection for parameterized generic helpers (`ref_root_reaches_transitive_cycle_node`) is host-cached via `RefCycleResultDb` with strict self-root warm-read validation (every `peek` validates the entry's `self_root_canonicals` — the BFS root file plus every visited declaration's file — before returning) and a `ComputeAdmission` cooperative-admission cold-path BFS (an overflowed/unrootable signature returns the computed bool through `ComputeAdmission::ReturnOnly` without admitting and without a second uncached BFS); the BFS dispatches `Instantiate { base, args: [], context: InstantiateContext { projection_reduction, resolve_env_hash } }` with `context.projection_reduction.mode = ProjectionMode::Skeleton` (Skeleton-mode instantiation, empty args) so unbound type parameters become `TypeParam` shells (preserving Conditional branches that would otherwise collapse to `never` for unbound generics). Semantic subqueries dedup through `SemanticGraphStore::execute_cooperative` via `ProjectSemanticDispatch::execute` — every `SemanticQueryKey` variant dispatches through this memo. Intrinsic dispatch routes through `IntrinsicRegistry::lookup` — the SDK audit test asserts every `= intrinsic` declaration in `lib*.d.ts` has a registry entry. Validated cache writes record a `ReadSetSignature.facts` fact signature (the path-precise fact-tracer observation set) — the sole cache-validity rail, revalidated against the live `StoreView` on every warm hit. Host-backed resolvers use `HostStoreView` directly as the result-DB fence authority; the OLD request-view-era TLS rail (`CURRENT_REQUEST_VIEW`) and the `_in_view` signature surface are fully retired. The CURRENT `RequestStoreView` (`resolver_core::request_store_view`) is LIVE: it is the request-scoped read-through wrapper that chains a `CanonicalCompletionOverlay` in front of the request-entry `HostStoreView` so mid-request additive loads (`ensure_loaded`/`ensure_indexed_ready` successes the entry snapshot did not track) validate without a false miss. `HostStoreView` itself is Arc-backed by an immutable `StoreViewSnapshot` (`VerterHost::store_view_manager()` hands out one shared snapshot per `StoreViewValidationToken` generation by cheap `Arc` clone; `with_session_overlay` re-roots overlay/tombstone canonicals via copy-on-write so the shared base is never mutated in place). The `StoreViewValidationToken` (`store_view_epoch` + `project_generation` + `FileArtifactStore.artifact_generation` + `RouteOwnedShallowDb.artifact_generation` + `load_generation` + the workspace `content_generation` + folded env hashes + project identity + frozen overlay identity) is the complete reuse/validity oracle — `store_view_epoch` is an INPUT to it, not the oracle by itself. The token advances on EVERY state change a base view snapshots by value: every `FileArtifactStore` keyed insert/replace/evict/GC/per-canonical-retention sweep and every augmentation-index mutation bumps `artifact_generation`; every `RouteOwnedShallowDb.publish` bumps the route-owned generation; and a successful FIRST-TIME additive `ensure_loaded` bumps the dedicated `load_generation` (it adds a scheduler node + `derived_raw_cache` state the build folds into `whole_hashes`/`derived_hashes` but does NOT publish into `FileArtifactStore`, so `artifact_generation` alone would not cover it; it deliberately does NOT bump `store_view_epoch`, so a cold compute's own dependency loads stay EXCLUDED from the publish fence's `externally_superseded_by` check and never self-fence promotion). The singleflight/stability coalescing-lane identity is NARROWER than that full reuse oracle: it is `StoreViewCompatToken`, whose `validity_fingerprint` is the `lane_fingerprint` — which delegates to `external_supersession_fingerprint`, the SAME oracle the promotion fence `is_stable` applies. That fingerprint folds ONLY the external-supersession dimensions: `store_view_epoch` + `project_generation` + the workspace `content_generation` (file-set mutations — watcher recovery / dependency appearance — advance it without any host-side epoch; the snapshot's edge-currency gates evaluated against it at build time, so a cached snapshot MUST miss once it moves; a cold compute's own loads never advance it, so it cannot self-fence) + the folded env hashes (`env_hash_fold`) + `project_identity` + frozen overlay identity. The additive generations (`artifact_generation` / `route_owned_generation` / `load_generation`) are DELIBERATELY EXCLUDED from the lane identity — a cold compute advances them through its OWN work (materialising content-addressed caches gated by `store_view_epoch`, loading its dependencies, admitting its own routes), so two identical concurrent requests snapshot at slightly different points in the load sweep; folding those generations would split them across separate lanes and spawn multiple cold winners instead of one leader + N-1 dedup-joining followers. Because the lane oracle IS the promotion oracle, a follower that joins a lane shares exactly the external dimensions the leader's promotion was gated on, so the leader's dedup-joined result is validation-equivalent for it; and a request whose snapshot externally-supersedes the leader's (an epoch / project / env / identity / overlay change, even at an equal `store_view_epoch`) gets a different lane key and forks its own lane, never receiving a result computed under a different external view. The complete `StoreViewValidationToken` (including the additive generations) REMAINS the store-view reuse/validity oracle — the `StoreViewManager` rebuilds its base snapshot on any additive-generation change; only the singleflight LANE identity was narrowed to the external dimensions (reuse-oracle = full token; lane-identity = external fingerprint). The base-view build is no-torn-return (a coherent capture or `Superseded`, never a torn publishable view) and SINGLEFLIGHTED — on a token miss exactly one caller sweeps while concurrent token-miss callers wait on a condvar and clone the winner's `Arc<StoreViewSnapshot>` (no N-way parallel sweeps). The component-meta cold publish fence rechecks the computed-under token against the live host before promotion (mismatch → return-only, no shared-cache warm), keying off `externally_superseded_by` so the compute's OWN artifact publications do not self-fence. The handle-backed dimensions stay out of the token for DIFFERENT reasons: `ResolvedImportFactsDb` is content-addressed (its key carries `content_hash`, so a new version is a new key and a fixed handle reads correctly — immutable-by-key); `RouteDb` is NOT content-addressed (`EffectiveExportSetKey` has no content hash and evict/clear/replace reuse the same key) — it stays out of the token because its route-surface validator compares the consumer's recorded `expected_hash` fingerprint against the live `RouteDb` slot, so an evict/replace yields a conservative fail-closed MISS, never a stale positive.

### Canonical Dependency Cache Rule (CRITICAL)

Host-backed type/import resolution treats the canonical file ID as the cache identity. Load and parse each dependency at most once per canonical ID per workspace content generation. Cache the parsed state, eval env, symbol/export tables, and prepared declarations together. Later lookups hit cached maps — never rewalk the AST. VFS is the authority for file-change invalidation. Concurrent cold requests to the same file collapse onto one materialization path. Changes land as one clean cutover, no dual-path shims.

See `/type-resolution` skill for the full rule set (invalidation semantics, route caches, prepared declarations, cross-owner reuse, negative caching, the concrete performance contract).

### Cache Architecture (CRITICAL)

The fact-based cache architecture splits cache keys across five orthogonal env-hash dimensions (`parse_env_hash`, `resolve_env_hash`, `type_env_hash`, `lib_env_hash`, `project_identity`). Each cache layer keys only on the dimensions it actually depends on (R21 scoping rule — a single bundled `project_config_hash` is forbidden). `lib_env_hash` enters a key only when the value depends on lib data: `ResolvedImportFacts` does NOT include it; `RouteDb`, typed-IR resolve, `MaterializeStructureDb`, `RefCycleResultDb`, `SemanticGraphStore`, `ComponentMetaResultDb` DO.

Two cache families: **content-addressed artifact caches** (`FileArtifactStore`, `ResolvedImportFacts`, typed-IR resolve, `MemberSemanticFactStore`, `MemberDisplayFactStore`, `ModuleAugmentationIndex`) carry `content_hash` or `parse_stable_hash` in the key; **query-identity caches** (`RouteDb`, `MaterializeStructureDb`, `RefCycleResultDb`, `SemanticGraphStore` query nodes, `ComponentMetaResultDb`) exclude version hashes from the key — concurrent variants coexist as candidates in one slot, with version rooting (`VersionedDeclIdentity` + `fact_dep_signature`, or `ReadSetSignature.facts` + `self_root_canonicals` for the semantic-graph memo) on the cached value. Cache keys never include `fact_dep_signature`.

The `SemanticGraphStore` family memo's query-identity keys (`SemanticQueryKey::Instantiate.base` and `SemanticQueryKey::ResolveMacroPayload.owner`, mirrored on `FamilyKey`) carry the env-bearing, content-free `ResolvedDeclSlotIdentity` (`defining_canonical`, `merged_symbol_name`, `symbol_space` + the `project_identity`/`type_env_hash`/`lib_env_hash` ENV dims — `Instantiate`/`ResolveMacroPayload` base/owner are always `symbol_space = Type`). The extra `resolve_env_hash` (`R`) dim rides on a dedicated per-key `InstantiateContext { projection_reduction, resolve_env_hash }` / `MacroPayloadContext { resolve_env_hash, mode }` (NOT the shared `ProjectionReductionContext`, which stays a pure projection-demand identity); `provenance` + `merge_role` stay at FAMILY-IDENTITY on `FamilyKey`. The slot stays content-free (R6) — env hashes are key dims; content/version hashes and the versioned `DeclIdentity` are forbidden in any derived-`Hash` query-identity key. The cold build re-sources the live file content version from `ResolverContext::ensure_indexed_ready(base.defining_canonical).whole_hash` at value-compute time. Non-file bases (`__builtin__`, the empty global sentinel, `<synthetic>`) do NOT fabricate a `FileWholeHash` self-root; builtin/structural bases root their version through `args` nodes only. Each `(family, slot)` in `FamilySlots` holds a candidate list capped at `FAMILY_SLOT_CANDIDATE_CAP = 4` per slot; same-discriminant (`validated_at_generation`, `facts`) re-publish replaces in place, else the candidate is appended; cap overflow FIFO-evicts the oldest. A warm hit requires TWO independent gates (§3.4 materialised-record satisfaction): `cached_satisfies(MemoEntry.satisfied_projection, requested_point_for_key(key))` — some RECORDED materialised `(path, point)` the candidate's compute actually produced dominates the request at the same path (NOT the candidate's nominal slot/mode, NOT enum rank) — AND per-candidate `ReadSetSignature.validate_with_self_roots` against the caller's live view. Backfill clones a broader entry verbatim into a projection-depth-narrower target slot gated by `cached_satisfies` (the retired `backfill_targets` enum-rank fan-out's direction, minus the lattice-unsound `Shallow → Navigate` clone); the gate is directional because `Navigate ⊒ Shallow` in the lattice does NOT license serving a `Shallow` shell surface from a carrier-stopping `Navigate` result. `validated_at_generation` is recency metadata only, never a semantic-validity oracle. Guards: `cache_satisfaction_is_materialized_point_not_nominal_demand`, `backfill_writes_only_recorded_materialized_points`.

`FileArtifactStore` is the authoritative per-file storage layer. Keyed by `(canonical, content_hash, parse_env_hash, parser_version)`, stores `IndexedReady`, `FileFacts`, `ParsedEdges`, `parse_stable_hash`, `augmentations`. The `augmentation_index` skeleton on the same store provides inverse-lookup for module augmentation under `AugmentationTargetKey { project_identity, resolve_env_hash, lib_env_hash, population, target }` — project + env isolation prevents cross-project poisoning, and `population: AugmentationPopulation {Base, Session(overlay-set fingerprint)}` keeps a session overlay's augmenters in a `Session` slot (overlay ∪ base) distinct from `Base` without poisoning it (the `Session` fingerprint is the slot's content-view identity, self-invalidating; a base augmenter change invalidates the `Session` entries that include it). `parse_stable_hash` is a structural hash over the post-shallow-analysis decl skeleton, invariant under cosmetic edits.

Cache runtime hard rules: cache correctness is read-side authoritative; keys include every deterministic input that changes the value; query-identity keys never include content/version hashes or `fact_dep_signature`; the five env hashes stay split; empty signatures and overflowed signatures are distinct; overflow, budget exhaustion, cancellation, generation supersession, incomplete self-rooting, and unresolved provenance route through `ReturnOnly`; `ReturnOnly` never publishes entries, reverse-index metadata, or persistent artifacts; reverse dependency graphs are not invalidation authority; same-canonical edits are caught by strict self-root validation; cross-file edits invalidate lazily through recorded facts; overlay/session results do not populate base-only or persistent caches; pure artifacts persist only with complete semantic/compiler/env/profile/plugin/source-map-policy keys; fact-validated semantic query results stay memory-only until their query family has audited self-root validation and typed non-cacheable admission; cold cacheable nodes require singleflight; in-flight joiners validate the winner's entry against their own view; cache admission is typed, not boolean/sentinel/side-channel based; cacheable entries are immutable after publish; cache hits do not allocate audit payloads without an active accumulator; public APIs expose distinct `stateless`, `content`, `session` semantics; benchmarks report cache mode, source-map policy, batch shape, thread count, hit count, fallback count.

See `/type-cache-architecture` skill for the full rule set (R1–R31, two-fact `MemberPresence`/`Member` model, multi-candidate substrate, signature-overflow contract, module augmentation completeness, heuristic-cache-semantics prevention, exact policy identity) and `docs/arch/fact-based-cache.md` for the per-field audit table + per-cache-layer key composition.

### Macro Type Traversal Rule (CRITICAL)

When resolving cross-file macro types (`defineProps<T>()`, `defineEmits<T>()`, component-meta deep expansion, etc.), only follow the import graph reachable from the requested type's declaration graph. There is one shared cross-file type resolver with five query modes: `Identity`, `Navigate`, `Shallow`, `Expanded`, `Skeleton` (see `/type-resolution` → Query Mode Contract).

**Macro resolution is one shared path, not a per-macro engine.** Every macro (`defineProps` / `defineEmits` / `defineOptions` / `defineSlots` / `withDefaults`) and every imported `.vue` component surface resolves through exactly TWO steps:

1. **Resolve ONE type via the shared resolver** — the generic-parameter type (`define*<T>()`) OR the object-argument type (`define*({ ... })`). For `withDefaults`, resolve the props payload type plus the defaults-object type and merge. For `.vue`-component imports, resolve the imported component's synthesized `$props` / `$emit` / `$slots` / expose surface recursively through the same dispatch (`.vue` import is the hardest case — apply EXTRA caution: it is exactly where rule violations cause the worst hangs). Resolution is ALWAYS the shared typed-IR five-mode dispatch — no macro-specific engine, no per-surface walker, no eager element resolver.
2. **Normalise per kind (a thin transform, NOT a resolver)** — props: defaults / optionality / readonly / declaration provenance / `declared_in_macro_type_arg`; emits: call-signature event extraction first, property keys only as fallback, payload function strips the leading event-name parameter; slots: function-like members only, first-parameter object becomes bindings, return type preserved; options/expose: pass-through object surface.

A macro/import that resolves its surface through anything other than the shared resolver, or flattens a full surface eagerly before the consumer demands it, is a rule violation — collapse it into `shared_resolve(type) + normalise`. Macros, emits, options, slots are at bottom a single type lookup plus thin normalisation; treat them so. `Skeleton` is the BFS / generic-helper traversal mode used by `Instantiate { base, args: [], context: InstantiateContext { projection_reduction, resolve_env_hash } }` with `context.projection_reduction.mode = ProjectionMode::Skeleton` — unbound type parameters become `TypeParam` shells so Conditional branches do not collapse to `never`. Path projection is path-precise: intermediate hops run in `Navigate`, the terminal hop runs in the caller's mode, non-contributing intersection arms are ignored (not rewritten to `never`), open conditionals distribute the remaining path into both branches, closed conditionals reduce immediately. Do not walk unrelated imports. Do not treat plain imports as implicit exports. Cache discovered symbol mappings and barrel hops.

**TS-first resolution priority:** TypeScript types always take priority over JavaScript files. Use `effective_target()`: `.d.ts` > `.d.cts` > `.d.mts` > `.ts` > `.tsx` > `.js` > `.jsx` > `.cjs` > `.mjs`.

**Owned resolution is bounded by `workspace_root`:** `node_modules` and package `#imports` ancestor walks stop at `IdeProjectConfig.workspace_root`.

See `/type-resolution` skill for the full traversal rules and resolver mode details.

### Declaration Merging (CRITICAL)

Same-name declaration merge is produced ONLY by `verter_semantic::type_eval` ordered declaration groups. `EvalEnv` appends contributors in source/binder order (`add_type`/`add_value` push onto an ordered `TypeDeclGroup`/`ValueDeclGroup` — no last-wins `FxHashMap<String, TypeDeclInfo>`/`…ValueDeclInfo>` map, no overwrite `insert` for mergeable kinds). Multiple same-name `interface` declarations lower to an explicit `TypeDeclBody::Merged` carrier (on `ShallowTypeSymbol.body` → `PreparedTypeDecl.merged_contributors`), body-lowering interns as a distinct `SemanticNodeData::MergedDecl { contributors }` node.

A merged declaration MUST reach the project-semantic reducer as that distinct carrier — a bare `TypeExpr::Intersection` / `SemanticNodeData::Intersection` is FORBIDDEN as the merged-decl representation, because the intersection reducer applies **heritage-shadow** member precedence (a later same-named member SHADOWS the earlier) and cannot accumulate method overload groups. The `MergedDecl` peer-merge reducer (`reduce_merged_decl_with_graph` + `merge_declaration_surfaces`, routed through raise / expand / keyof / relation / substitute) instead: (a) same-name methods/call-signatures ACCUMULATE into one ordered overload group across contributors in source order; (b) conflicting non-method properties take deterministic first-contributor precedence (never `never`); (c) distinct members union.

Functions accumulate into an ordered `Vec<FunctionSignature>` (`ValueDeclGroup::merged_signatures`); each `FunctionSignature` carries `has_implementation_body`. Overload visibility is a PROJECTION-time rule (`build_typeof`): a lone signature is visible (even if bodied); a multi-signature overload group surfaces every bodiless overload in source order and hides the trailing implementation. Same-file merged values are version-rooted by the owner's single `FileWholeHash` self-root under a content-free query-identity key (R6) — no dedicated contributor-sequence fact. `verter_session` may route/consume contributors but MUST NOT synthesise the merge as `raw_body = TypeExpr::intersection(...)`. Cross-file ambient augmentation (`declare module`/`declare global`) reuses this same `MergedDecl` peer-merge path — see Declaration Augmentation (CRITICAL).

See `/type-resolution` skill for the carrier chain, the peer-merge reducer, and the architecture guards.

### Declaration Augmentation (CRITICAL)

Ambient declaration augmentation (`declare module "X" { ... }` / `declare global { ... }`) is a RETAINED, addressable scoped inventory — never fingerprint-only facts, never file-scope pollution. `EvalEnv.augmentation_scopes` keys `(AugmentationScopeKind {Global, Module(specifier)}, name)` → ordered `TypeDeclGroup` (interfaces / type aliases); `EvalEnv.augmentation_value_scopes` keys the same scope → ordered `ValueDeclGroup` (`const`/`let`/`var`, `function`, `class`); both mirrored on `ShallowFileState`; inner decls NEVER enter file-scope `type_symbols`/`value_symbols`. A scoped declaration is addressed by `AugmentationScopeKind` alone (no `ScopeId.kind`/`semantic_query::ScopeKind`). Parse-domain `ModuleAugmentationFact`s are DERIVED from this typed inventory (`fact_emission::collect_augmentations`) — NO raw-source byte-scan.

Cross-file augmentation merge is the SAME `MergedDecl` peer-merge path as same-file merging — NOT a second merge engine. When a declaration `(canonical, name)` is instantiated, `stitch_module_augmentations` (in `project_semantic_dispatch::build`) finds every augmenter file via `FileArtifactStore::ensure_augmentation_index_populated`, fetches each augmenter's RETAINED inner body from the typed `augmentation_symbol(Module(spec), name)` inventory (typed-IR only — never a source/byte scan in the resolver), lowers it in the augmenter's own file context through `prepare_augmentation_type_decl` + `lower_decl_body_with_provenance`, and folds the base body ∪ augmenter contributions into ONE `SemanticNodeData::MergedDecl` carrier (base body flattened if already a `MergedDecl`). Augmenter order is the stable `(canonical, parse_stable_hash)` key — discovery-order-independent. Relative-augmenter discovery loads the base's `reverse_deps_for` (the candidate-augmenter set) before the index scan, since augmenters depend on their base.

Cross-file FACTS reuse `get_or_compute_effective_export_set`'s rail: the cold stitch observes one `FactKey::ModuleAugmentationIndexShape` (the augmenter-set fingerprint) plus one `FileWholeHash` per contributing file (base ∪ augmenters), and records `self_root_canonicals = {base} ∪ {augmenters}` — a content edit to ANY contributor misses the warm read; torn/partial routes through `ReturnOnly`, never warmed. Query keys stay content-free (R6).

The augmentation index is OVERLAY-AWARE: `AugmentationTargetKey.population: AugmentationPopulation {Base, Session(overlay-set fingerprint)}`. A `Base` scan reads `is_legacy()` artifacts only; a `Session` scan reads the session's overlay (non-legacy) artifacts — matched by the session overlay discriminator — UNIONED with base. This is the CONTENT-ADDRESSED compute cache (on `FileArtifactStore`), so its `Session` key carries the overlay-set content fingerprint (a content view identity, not an R6 violation) and self-invalidates when overlay content/membership changes. The QUERY-IDENTITY `EffectiveExportSetKey` (on `RouteDb`) is instead keyed by the CONTENT-FREE `EffectiveExportSetScope {Base, Session(session_scope_id)}` (the `StoreViewCompatToken::session`) — the overlay content fingerprint NEVER enters this key (R6); overlay content identity is validated on the VALUE via the `ModuleAugmentationIndexShape` fingerprint fact + per-contributor `FileWholeHash` anchors, revalidated on every warm hit. Overlay augmenters NEVER poison the base index and NEVER cross sessions. There is NO base-only `assert!(view.compat_token().session.is_none(), …)` on the augmentation-index / `EffectiveExportSet` surface — a session view is accepted under `Session` scope.

See `/type-resolution` skill for the stitch chain, the overlay-aware index, and the architecture guards (`session_overlay_augmenter_isolated_from_base_index`, `no_effective_export_set_base_only_session_assert`).

### Two Template Codegen Paths (CRITICAL)

The Rust compiler has two separate template codegen paths; modifying one does NOT affect the other: **VDOM/Vapor** (`template/code_gen/vdom/`) for runtime render functions, and **IDE** (`ide/template/`) for valid JSX/TSX used by LSP/TSGO type checking. The LSP uses the IDE path via `CompileTarget::IDE`.

See `/compiler-codegen` skill for full codegen pipeline, backends, and CompileTarget details.

### Fallthrough / Root Inheritance (CRITICAL)

The shared Rust pipeline owns all fallthrough and root inheritance semantics. `verter_semantic::analysis` extracts root reachability facts only. `verter_session` owns the single inheritance resolver, recursion, conditional branch composition, generic propagation, caching, and final metadata projection.

Key rules: `inheritAttrs: false` → no inherited surface. Single native root → intrinsic attrs minus declared props/events. Single component root → recursive propagation. Conditional branches → exact union. Cycles → unresolved branches. `class`/`style` are never consumed.

See `/component-meta` skill for the full semantic rules, public contract, authority chain, and key files.

### Component-Meta Shallow-By-Default Rule (CRITICAL)

Types and properties are ALWAYS published shallow at the projector surface UNLESS the consumer explicitly walks the path. This is the single architectural invariant the projector pipeline (`meta_resolve::projectors::reduce_published_field_types` + `reduce_field_type_expr_with_mode`) enforces.

Concrete contract:

- Plain alias references (`type Foo = ...`) — published prop type stays `TypeExpr::Ref { name: "Foo" }`. Consumers re-resolve `Foo` through the registry on demand. The projector does NOT eagerly inline the alias body.
- `Pick<Foo, "bar">` — materialises ONLY the `bar` member of Foo. Other Foo properties stay shallow (path-precise). Built-in utility types (`Pick`, `Omit`, `Required`, `Partial`) behave identically to a userland implementation referencing the same keys.
- **Open key domain ⇒ shallow carrier (L1) — route/mode-independent.** The L1 carrier-stop covers TWO families through shared predicates consulted at EVERY entrance — the `Navigate` projector reduce route, the dispatch lowering/build entrances, the empty-path Shallow surface synthesiser, and the component-meta registry materialiser, whose top-level composition walk fails OPEN-OR-UNKNOWN — traversal-budget exhaustion preserves the carrier instead of falling through into Expanded materialisation (no route or mode escapes it): (1) an OBJECT-FILTER utility (`Pick`/`Omit` — family identity from the one `BuiltinUtility`-registry helper `raise.rs::is_l1_object_filter_utility`) whose enumeration domain (the source — argument 0) is OPEN or undecidable STAYS a shallow carrier in every mode instead of materialising the source (owner: `raise.rs::utility_enumeration_domain_is_open_or_unknown`); (2) a MAPPED type `{ [K in S]: V }` whose produced surface still depends on an unbound OUTER generic preserves the deferred `Mapped` carrier instead of enumerating keys and materialising per-key values (owner: `raise.rs::mapped_type_is_open_or_unknown`). **Per-ARGUMENT key-domain rule (both families), POSITION-SENSITIVE:** an instantiation in a KEY-DOMAIN position (`Pick`/`Omit` source, mapped source/keyspace, indexed-access index) is judged by `prepared_instantiation_key_domain_is_closed` over the per-argument identity-preserving binding vector (`KeyDomainBinding`: `Open`, or closed carrying the actual bound `TypeExpr`/`SemanticNodeId` where scope-safely available — environment-free exprs, forwarded parameter bindings, closed NAMED actuals resolved in their OWN originating scope to interned `DeclRef` identities, and unfilled defaulted params re-bound to their verified-closed DEFAULT's identity; closed shapes with no resolvable identity degrade to `ClosedAbstract`) — an open argument confined to member VALUE positions of a fixed-key body keeps the key domain CLOSED (`Omit<Foo<T>, 'items'>` and `{ [K in keyof Foo<T>]: V }` over `interface Foo<T> { label?: string; items?: T }` still materialise `label` path-precisely); an instantiation under a VALUE-SENSITIVE operand position (`Conditional.check`/`Conditional.extends`, `IndexedAccess.object` — the `OperandPosition` axis on the TypeExpr classifier AND the node-level `OpenWalk`) is instead OPEN if ANY argument is open, because the enclosing operator consumes the operand's VALUES (`Pick`/`Omit` over `Wrap<T>['a']` with `a: BigOpen<T>`, or over `Foo<T> extends X ? A : B`, carrier-stops even though the wrappers' own key sets are fixed); under `ValueSensitive` BOTH routes also DESCEND value surfaces — object member values, function params/returns, array/tuple elements — so a compound argument or inline literal operand hiding the open generic in a value position (`Wrap<{ nested: T }>['a']`, `{ a: BigOpen<T> }['a']`) opens, and an all-closed-argument instantiation is a concrete operand only when its BASE resolves (prepared decl or registry builtin — an unresolvable base is undecidable ⇒ open); tuple/array ELEMENTS are value positions on both routes (a tuple's KEY domain — its indices — is closed at `KeyDomain` without descending non-rest elements; a `rest` element (`[string, ...T]`) makes the index domain depend on the rest type's arity and is judged at `KeyDomain` in every position — an open rest element opens the domain, while a rest element that itself closes at `KeyDomain` conservatively keeps it closed, with no tuple-arity algebra); conditional BRANCHES stay in the surrounding position; a mapped body splits by ROLE: its `source`/`keyspace`/`as`-remap are KEY-PRODUCTION, walked pinned at `KeyDomain` regardless of the surrounding position (a value-sensitive parent must not false-OPEN a fixed-key mapped source), and its VALUE expression never opens the key domain at `KeyDomain` (`Omit<{ [K in 'a'|'b']: T }, 'a'>` stays CLOSED — `T` publishes shallowly in value position) but IS consumed under a value-sensitive operand or value-body-descending walk (`Omit<{ [K in 'a']: T }['a'], 'x'>` is genuinely open and carrier-stops); an index-signature KEY type IS key-domain-reachable (a key over an open param opens; a concrete `[k: string]` is the bounded Record-class signature surface and stays closed). **Tri-state conditionals via the shared oracle:** conditional closedness routes through `ProjectSemanticDispatch::conditional_branch_selection` — the ONE branch-selection oracle that owns the FULL selection path `build_conditional` reduces with: the pre-relation infer-pattern cases FIRST (`pre_relation_infer_selection` — a bare-`infer` extends ALWAYS selects TRUE with `X := check`, for any check; a function-typed extends with infer positions binds positionally against the materialised check), then `shallow_relation_check`, then the full memoised `relate_nodes` (`Unknown` ⇒ Deferred; `any`/`error` checks, which semantically use both branches / dominate, ⇒ Deferred) — True-selected classifies ONLY the true branch (an open LOSING branch is dead: `Omit<true extends true ? { label: string } : T, 'x'>` is CLOSED and materialises `label` — and so does `Omit<T extends infer X ? { label: string } : T, 'x'>` via the bare-infer selection), False-selected only the false branch, Deferred classifies check/extends value-sensitively plus BOTH branches; a bare-infer TRUE selection classifies the branch with the infer name bound to the CHECK's identity/openness (`? X : …` over an open check stays OPEN), while a function-infer selection widens to the Deferred treatment in the classifiers (a conservative superset — its bindings are check-signature components the classifier has no identity for); the classifier resolves operands to nodes solely via environment-free interning (literals/primitives/`infer` placeholders), identity bindings, and own-scope named-ref resolution (`prepared.name_resolution` → interned `DeclRef`) — it never reimplements assignability and never materialises branches. **Builtin route-independence, per-utility OUTPUT-KEY semantics:** `raise.rs::builtin_utility_key_domain_is_closed` is the one registry-owned key-domain rule shared verbatim by the node-level `__builtin__` arm and the TypeExpr unresolved-`Ref` fallback; its semantics are PER-UTILITY, owned by `BuiltinUtility::key_domain_argument_positions` — only the arguments that actually produce output keys are judged: `Pick`/`Omit` judge source + key-selection (args 0 and 1), the mapped utilities (`Partial`/`Required`/`Readonly`) judge the source, `Record<K, V>` judges ONLY `K` (`Omit<Record<'a', T>, 'x'>` stays CLOSED — the open value argument never opens the key domain), and value-producing utilities (`ReturnType`, `InstanceType`, `Awaited`, `NonNullable`, the union/extraction and string utilities, `NoInfer`) make NO closed-key claim — conservatively not-provably-closed, carrier preserved, until a per-utility output classification exists (`Pick<Partial<{a,b}>,'a'>` still enumerates). **Mapped family composition:** the mapper binder `K` is BOUND in EVERY walk — the node-level walk AND the single TypeExpr-layer classifier (`raise.rs::key_domain_type_expr_is_closed`, shared by the prepared-decl and instantiated-body routes); keyspace, value, remap — `as `on${K}`` over a finite keyspace is a K-only transform, CLOSED on every route; SOURCE/KEYSPACE openness is the per-argument key-domain rule above; the `as`-REMAP is KEY-PRODUCTION, judged by the binder-bound KEY-DOMAIN policy on both routes (per-argument rule — `as keyof Foo<T>` over a fixed-key `Foo` stays CLOSED and enumerates; a direct outer-generic remap or a value-sensitive conditional operand inside the remap stays OPEN); VALUE-BODY openness is "any unbound outer generic reached opens" (finite value surfaces are descended; conditional values follow the tri-state rule — the selected branch alone, both branches when deferred). The mapped utilities (`Partial`/`Required`/`Readonly`) lower to `MappedType` and are guarded by the same mapped predicate plus the deferred-shell fail-closed behaviour; `Record` is an index-signature key domain, not finite enumeration, and correctly falls back to a deferred mapped carrier. OPEN (object-filter domain) = a bounded typed-IR (`SemanticNodeData`) walk reaches an unsubstituted `TypeParam`, a deferred conditional with an open operand or branch, an open `IndexedAccess`/`KeyOf`/`Mapped`, an instantiation whose produced KEY SET depends on an open argument per the rules above, an unresolved or open-bodied `DeclRef` alias chain, an `Opaque`, or exhausts the walk budget; openness verdicts are memoized per `(node, position)` (a hash-consed repeated open node — `Pick<Foo<T, T>, K>` — stays open on revisit; only in-flight cycle back-edges are closed-for-revisit). CLOSED = a finite object surface / concrete instantiation (including a nested closed object-filter — `Pick<Pick<{…},'a'|'b'>,'a'>` — and a generic wrapper whose open argument stays in value positions: `type Outer<T> = Foo<T>` and `interface Outer<T> extends Foo<T> {}` both keep `Omit<Outer<T>,'items'>` closed) / finite union/intersection of those / a concrete-operand operator body (an oracle-selected conditional with a closed selected branch, a K-only remapped mapped type) reached without crossing an open node (a bounded alias chain `Foo→Bar→{bar:string}` resolves CLOSED). `Pick<PropsBase<T>, …>` over the SFC's open `generic="T"` stays `Pick<…>`; `Pick<{bar,baz},'bar'>` and `Pick<SimpleBox<string>,'icon'>` (a concrete object-bodied generic instantiation) still materialise the requested keys path-precisely. A chained-conditional-bodied concrete source such as `Pick<PropsBase<UIMessage[]>,'icon'>` is NOT L1 carrier-stopped either, but currently yields `semanticMiss` downstream — a separate conditional-reduction gap tracked as a follow-up, NOT an L1 concern (it does NOT materialise the requested keys today). `infer` is a conditional-inference binding, NOT an unbound generic — it does not open the domain by itself; an infer name bound by an oracle-selected bare-infer conditional classifies as its bound check. Typed-IR only, no string matching. **Invalidation:** the closedness walk's cross-file reads (alias-chain hops, barrel re-export hops, prepared-decl bodies) are observed as `FileWholeHash` facts onto the active tracer, so every published carrier/materialised entry's `ReadSetSignature.facts` carries them — an edit that flips a dependency's closedness rejects the warm entry on the read-side validator and the verdict recomputes. The carrier-stop is the PRIMARY defense for the open-generic class; the per-request projection budget (`request_budget.rs`) is an ARMED-by-default runaway fuse (`projection_op_budget == 0` ⇒ effective cap 2000); the projection keys plus `Instantiate`/`Conditional` count toward it; an armed fuse that trips returns `BudgetExceeded` as a genuine partial (refused warm admission — the no-poison invariant). Two real-corpus residuals remain on the armed-budget backstop, each a SEPARATE mechanism with a tracked follow-up AND an `#[ignore]`d RED tracker in `defect_b_corpus_prevention_gate.rs`: `Table.vue`'s structural `extends`-heritage residual still trips the fuse and resolves degraded (follow-up: structural extends-heritage carrier-stop; tracker `table_resolves_complete_and_warm`), and `ChatMessages.vue`'s open-conditional mapped-value empty-path-Expanded distribution still exceeds the budget (follow-up: open-conditional mapped-value terminal carrier-stop; tracker `chat_messages_resolves_without_timeout`).
- `Omit<Foo, "bar">` — keeps `bar` shallow (excluded from the surface) and materialises the others.
- `Foo['a']['b']` — path-precise: only the `a` and `b` hops load; other Foo keys never enter the published surface.
- True recursive types (`type Self = Pick<Self>`) — NOT supported. The published surface stays the bare `Ref { name: "Self" }`.
- Imported alias names (workspace-owned OR package-backed) — stay shallow regardless of where they live.

The projector pipeline is the sole post-projection authority — no eager per-field materialisation runs at publication time.

See `/component-meta` skill for the full rule set and the locked-down negative tests in `crates/verter_session/src/meta_tests.rs`.

### Component-Meta Native Vs Compat (CRITICAL)

The native component-meta payload is the semantic authority. `@verter/component-meta/compat` is a projection layer for `vue-component-meta` interoperability, not a second semantic pipeline.

Core rules: Fix metadata in the native layer first. Rust owns resolution, declaration routing, graph construction. One async native request per query. JS may transform structure but must not recover meaning. JS must not become a second resolver or expander. Cache-owned type recovery only — no AST/source fallbacks.

See `/component-meta` skill for the full policy, resolver rules, and cache contracts.

### Typed-IR-Only Resolver Rule (CRITICAL)

The native component-meta / typeinfo type resolver — analyzer → projector → registry → policy → materialiser — drives semantic decisions exclusively from the typed IR (`verter_semantic::analysis::type_expr::TypeExpr` on Rust, `TypeDescriptor` from `@verter/type-ir` on TS). Source slicing, regex against type text, hand-rolled type-text splitters (`split_top_level_*`, `find_top_level_char`, etc.), `starts_with("Pick<")` shape sniffing, `path.contains("/node_modules/")` classification, and the synthesise-then-reparse pattern (`format!(...).parse_type_annotation(...)`) are all forbidden inside that pipeline.

Concrete contract:

- OXC lowering happens once during shallow analysis via `lower_ts_type(ts_type, source)`. The lowered `TypeExpr` is stored alongside `Analyzed*Field` (and on `ResolvedLocalType.type_expr`, `ProjectedMacroSurfaces.*_expr`) and survives all caches.
- `parse_type_annotation` is reserved for JSDoc tag-type payloads. Calling it from the resolver / projector / registry / policy / materialiser / compat pipeline is the bug.
- Raw / display strings (`Analyzed*Field.type_annotation`, `ExpandedField.raw_type`, `ResolvedLocalType.expanded`, `PropMeta.rawType`) are display-only passthroughs. Resolver/compat consumers MUST NOT parse them back.
- Workspace classification uses `ResolverContext::workspace_is_workspace_owned` and `workspace_is_package_backed`. Substring tests on canonical paths (`"/node_modules/"`, `"\\node_modules\\"`) are banned.
- Hand-rolled type-text parsers (e.g. `extract_pick_slot_bindings`, `extract_string_literal_name`, `splitTopLevelTypeOperator`) must not exist inside the resolver or compat layer. Walk the typed IR instead.
- The JS compat layer (`@verter/component-meta/compat`) reads `prop.type` (`TypeDescriptor`) for every semantic decision. `prop.rawType` is display passthrough only — it must not feed any `looksLike*`, `extract*`, `normalize*`, `split*`, `strip*`, `prefer*`, `shouldPrefer*`, or `repairOpaque*` branch.
- Type-role classification is structural, not nominal. A type is a "prop type" / "emit type" / "model type" / "slot type" because a Vue SFC macro (`defineProps`, `defineEmits`, `defineModel`, `defineSlots`, `withDefaults`) consumes it — NOT because its identifier name ends with `"Props"` / `"Emits"` / `"Events"` / `"Model"` / `"Slots"`. Macro participation is read from `AnalyzedMacro.kind` / `parsed_type_argument` / `type_references` on the analyzer snapshot. Identifier-name suffix checks (`name.ends_with("Props")` etc.) are forbidden inside the resolver.
- The single explicit exception is JSDoc: `{Type}` payloads inside JSDoc tags are inherently text and may be parsed via the dedicated JSDoc path.

If a new requirement appears to need text manipulation inside the resolver, fix the producer (lower the right OXC node, store the right typed field, extend `@verter/type-ir` with a missing variant) rather than reparsing or pattern-matching on text.

See `/component-meta` and `/type-resolution` skills for the typed schema contract, the producer-side lowering points, and the architecture-guard list.

### CodeTransform Is the Single Source of Truth (CRITICAL)

**All modifications to generated code MUST go through `CodeTransform` operations** (`overwrite`, `prepend_left`, `append_left`, `move_with_suffix`, etc.). Never apply string replacements, regex transforms, or manual splicing to the output of `build_string()` or to content produced by a `CodeTransform`.

Post-hoc string manipulation breaks sourcemap accuracy: `CodeTransform` generates source maps by tracking chunks (Original, Inserted, Moved, Overwritten). Modifying the string after the transform means byte offsets in the source map no longer match the content, causing position mismatches in the LSP (hover landing on the wrong token, go-to-definition jumping to wrong locations).

**Correct:** `ct.prepend_left(pos, ".ts")` to insert text at a known position — chunk list and source map stay consistent.
**Wrong:** `content.replace(".vue'", ".vue.ts'")` on the built string — the source map still reflects the pre-replace byte offsets.

### Typeinfo Wire Contract (CRITICAL)

The typeinfo graph wire surface (`crates/verter_protocol/proto/verter/v1/typeinfo.proto`, the Rust and TS bindings it generates, and the audit envelope on top) is a closed contract. Four invariants govern every change:

1. **Closed-enum discipline at the wire surface.** `GraphTypeNode.kind`, `StructuredTypeExpression.kind`, `TypeInfoGraphRequest.payload`, `TypeInfoRequestError.kind` are closed `oneof` taxonomies. Adding a variant requires bumping `SemanticTypeGraph.schema_version`. Removing one requires `reserved` directives at the enclosing message scope (proto3 forbids `reserved` inside an `oneof` block).
2. **Wire-compat: existing field numbers never reused.** A retired variant's tag goes into the message's `reserved` list with its name; off-tree clients that decoded the original schema keep round-tripping the slot as an unknown field. New variants take the next free tag, not a recycled one.
3. **Audit envelope additions are purely additive.** Every new typeinfo audit field (`structured_event`, `kind_payload`, `RequestKind::TypeInfoGraph`) lands as a new arm or a new field with a default-zero value, never a replacement. Consumers that ignore the new field keep working.
4. **Request validation runs before semantic execution.** `validate_type_info_graph_request` rejects malformed envelopes through a typed `TypeInfoRequestError`; semantic dispatch never sees an unvalidated request. The schema-version gate is closed-set (`SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS`); per-variant structured-expression validation is exhaustive over the `oneof` taxonomy.

Pinned mechanically:

- proto/TS oneof parity by `crates/verter_session/tests/typeinfo_graph_taxonomy.rs`.
- byte-equal freshness by `crates/verter_protocol/tests/typeinfo_proto_ts_freshness.rs::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output` (regenerates the TS bindings via the workspace `buf` and `oxfmt` binaries and byte-compares).
- audit-parity by `crates/verter_audit/tests/request_kind_payload_parity.rs`.
- request validation by `crates/verter_session/tests/typeinfo_request_validation.rs` (closed-set schema-version + exhaustive structured-expression coverage).

## Build

```bash
pnpm install                  # Install all dependencies
pnpm build                    # Build everything: native → lsp → wasm → ts packages
pnpm run build:native         # Build native .node bindings only
pnpm run build:lsp            # Build Rust LSP binary (debug)
pnpm run build:lsp:release    # Build Rust LSP binary (release, optimized)
pnpm run build:mcp            # Build MCP server binary (debug)
pnpm run build:mcp:release    # Build MCP server binary (release, optimized)
pnpm run build:wasm           # Build WASM + copy to playground
pnpm run build:ts             # Build all TypeScript packages
pnpm run build:playground     # Build the playground for deployment
```

`pnpm build` runs sequentially: native bindings first (needed by unplugin), then LSP binary (shares compiled Rust deps with native, avoids recompilation), then WASM (needed by playground), then all TS packages.

See `/build-and-profiling` skill for build dependency chains, rebuild sequences, and profiling setup.

## Development

```bash
pnpm watch                    # Watch-build TS packages for extension dev
pnpm dev-extension            # Build LSP binary, then watch language-shared + vscode extension + typescript-plugin
pnpm clean                    # Remove build artifacts
```

## Testing

### Running Tests

```bash
# TypeScript / JavaScript
pnpm test                                    # All JS/TS tests
pnpm vitest --run                            # All tests (non-watch)
pnpm vitest --run path/to/test.spec.ts       # Specific file

# Rust — CANONICAL agent gate (completeness): nextest + the shared-process verter_session surface
cargo nextest run --workspace                # Authoritative completeness gate — runs every workspace test target INCLUDING the ~25 verter_session integration binaries
cargo test -p verter_session --tests         # Shared-process surface for the verter_session integration suite
cargo test --workspace --doc                 # Rust doctests only; run when rustdoc examples changed or explicitly requested
cargo test --package verter_compiler test_name   # Specific Rust test
# NOTE: bare `cargo test --workspace --tests` SILENTLY SKIPS the verter_session integration suite (~4404 tests) because `session_metrics` feature unification drops those binaries from the workspace test set — it MUST NOT be the sole Rust gate; use the nextest + `-p verter_session` pair above.
cargo test --package verter_compiler 2>&1 | tail -60  # Full suite with truncated output
```

### End-of-change Checks

Run after **every** change. Verter's crates are highly interconnected — a change in one crate frequently breaks tests in dependent crates. Always run the full workspace suite:

```bash
cargo nextest run --workspace 2>&1 | tee /tmp/test-output.txt   # CANONICAL completeness gate — runs the verter_session integration suite
cargo test -p verter_session --tests 2>&1 | tee -a /tmp/test-output.txt   # shared-process surface
cargo clippy --workspace -- -D warnings
cargo fmt --all --check
pnpm install --frozen-lockfile   # Verify lockfile is in sync (CI uses this)
```

Bare `cargo test --workspace --tests` silently skips the verter_session integration suite (feature unification drops those ~25 binaries) and must NOT be the sole Rust gate — always run the `cargo nextest run --workspace` + `cargo test -p verter_session --tests` pair above.

- Corpus audit-test regenerator (run after audit-record schema or fixture changes; idempotent): `node scripts/gen-corpus-audit-tests.mjs`

For TypeScript changes, also run `pnpm test`. Do not skip workspace-wide testing even for "small" changes.

**Agent test policy:** Run the canonical pair — `cargo nextest run --workspace` (completeness) plus `cargo test -p verter_session --tests` (shared-process surface) — as the default Rust gate. Do not run bare `cargo test --workspace` (no `--tests`) by default: it pulls in doctests and example builds, adding substantial runtime without improving the normal agent verification loop. Do not rely on `cargo test --workspace --tests` alone either — it silently skips the verter_session integration suite. Run doctests (`cargo test --workspace --doc`) only when rustdoc examples changed or the user explicitly asks.

### Documentation Updates

After adding, changing, or removing features, update the **owning** documentation:

- **Domain skills** (`.claude/skills/`) — update the skill that owns the affected module or API
- **`CLAUDE.md`** — only if summaries or skill pointers change
- **`AGENTS.md`** — if skill routing or shared sources change
- **`docs/`** — API docs, guide pages, contributing guides
- **Inline doc comments** — Public API rustdoc (`///`) and JSDoc (`/** */`) on changed signatures

Skip for purely internal refactors that don't change public behavior, module paths, or APIs.

### Testing Requirements

**MANDATORY: TDD must be followed for EVERY code change. Non-negotiable.**

1. Write failing tests FIRST — verify they fail before implementing
2. Implement minimum code to pass
3. Run tests, verify green
4. Refactor while keeping tests green

Coverage: new features need tests, bug fixes need regression tests, refactors must keep existing tests passing.

**Always include negative assertions**: verify both what SHOULD and should NOT be present. Codegen tests must check removed syntax is absent. Type tests must include `@ts-expect-error` guards against `any`/`never`.

**Architecture guards for critical rules**: every new `CRITICAL` architecture rule must land with either a static architecture guard or a discriminating regression test in the same change. If a guard cannot be automated yet, the rule text must name the planned guard/test and the temporary gap must be tracked in the owning skill/doc. The R6 meta-guard at `crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs` (`every_critical_rule_in_docs_has_registered_guard`) walks `CLAUDE.md` plus every `.claude/skills/*/SKILL.md`, extracts each `(CRITICAL)` section heading, and asserts every rule appears in the `CRITICAL_RULE_GUARDS` registry with at least one named guard — a prose-only `(CRITICAL)` section fails this gate.

**Rust test file organization**: When inline `#[cfg(test)]` exceeds ~400 lines, extract to a sibling `*_tests.rs` file.

### Testing-Hermeticity (MANDATORY)

Unit tests must only depend on locally-vendored fixtures. They must compile and run without any third-party repository (e.g., `nuxt-ui`, `element-plus`) checked out alongside this repository. Tests that need external corpora must be feature-gated (e.g., `#[cfg(feature = "external-corpus")]`) and excluded from the default canonical run (`cargo nextest run --workspace` + `cargo test -p verter_session --tests`).

A test that references `.integration-tests/repos/<third-party>/...` from a non-gated test file is a violation. The architecture guard `external_corpus_paths_not_present_outside_gated_tests` enforces this.

### No phase archaeology in production code (MANDATORY)

Source comments must not reference plan phases (`phase 5d`, `phase 11`, `post-cutover`, `pre-Phase`), cutover stages (`d-cutover`, `cutover`), deletion history (`deleted in 5g`, `retired in`), or any project-management vocabulary. Once a plan is over, the code reads as final-state.

Durable architecture insights belong in `.claude/skills/*` or `docs/arch/`, not in source comments. Test files named after retired phases must be renamed to describe the invariant they characterize, not the phase that produced them.

The architecture guard `no_phase_archaeology_in_production_code` enforces this on `crates/*/src/**`.

See `/testing` skill for full TS/Rust test patterns, sourcemap testing, and server cleanup.

**Audit infrastructure**: Rust-first deterministic per-request observability for every audited `VerterHost` entry-point (component-meta, type-resolution, compile, analyze, workspace ops, LSP handlers, MCP tools, bundler batches). Substrate DTOs + `AuditObserver` trait live in `verter_audit`; the host runtime, records store, registration lifecycle, accumulator, footprint miner, peak-RSS sampler live in `verter_session`. TS bindings in `packages/types/audit.generated.ts`. Opt-in via `HostConfig::audit_enabled + footprint_capture`. See [`docs/audit-footprint/`](docs/audit-footprint/) for the API reference and debug flow, and the `/audit-infrastructure` skill for the architectural map.

### VS Code Extension Testing (MANDATORY)

Changes to the VS Code extension or the LSP server MUST be verified with automated tests, NOT manual testing. Unit tests (Vitest) for pure logic, E2E tests (Mocha) for LSP integration features.

See `/testing` and `/e2e-vscode-testing` skills for commands, fixture design, and helpers API.

## Agent Implementation Rules

### Codebase Navigation

Use semantic code-navigation tools before broad source reads when available. For Serena or equivalent MCP tools, prefer symbol overviews, symbol lookup, reference lookup, and rename/refactor operations for exploration and targeted edits. Read full source files only when symbolic context is insufficient or the file is small enough that a full read is clearly the most direct path.

### Planning

Prefer architecturally correct, long-term solutions over easy or quick implementations. Evaluate approaches by correctness and durability, not implementation speed.

The codebase expects the best architecture for the problem. Time constraints, implementation size, migration breadth, anticipated breaking changes, or "a lot of work" are not valid reasons to weaken the design, preserve a compromised path, or diverge from the approved plan. If the correct implementation is larger or breaking, plan for it explicitly or raise it before execution; do not silently ship an architectural deviation.

Do not provide time estimates unless the user explicitly asks. Do not use estimated effort, duration, or perceived time cost as a factor for doing, not doing, or partially doing planned work; approved plans already account for timing expectations.

Plans must include these sections:
1. **Context** — why this change is being made
2. **Changes** — specific files to modify with concrete modifications
3. **Legacy Deletions** — explicit list of files, functions, code paths, feature flags to remove
4. **Verification** — full workspace test commands and expected outcomes

Without explicit legacy deletion lists, agents skip deletions and leave dual paths alive.

### Execution

Execute plans fully in one pass without intermediate checkpoints unless explicitly requested. Do not stop mid-plan to ask for confirmation on steps already approved in the plan.

Once execution starts, complete the approved plan end-to-end in the same pass. Do not pause, defer scope, or leave planned work unfinished because of estimated time or effort unless the user explicitly changes the request. Do not rewrite the plan into a smaller or safer variant during execution because the correct path is breaking, broad, or labor-intensive. Approved plans are expected to land as written unless the user explicitly re-scopes them.

### Orchestrating Large Plans

For a large multi-block plan, refactor, migration, or staged cutover executed autonomously, drive it via the `/multi-agent-orchestration` skill rather than improvising the coordination. A pure orchestrator delegates each block to implementer/reviewer/fix sub-agents, gates every block on dual review (an independent reviewer plus a `codex` review), runs per-block fix cycles until the re-review is clean, consults `codex` on any architectural doubt or sub-agent escalation, and verifies sub-agent reports against git state (trust but verify). This keeps the orchestrator's context clean enough to coordinate a plan far larger than one context window.

### Self-Review

After completing a plan, review the full implementation before declaring done:
- Verify all plan steps were executed
- Check for missed edge cases or incomplete migrations
- Run the full workspace test suite (see End-of-change Checks above)

### Legacy Code Deletion

When replacing a feature or refactoring a system, delete the superseded code in the same change. Do not add shims, double branches, compatibility wrappers, or feature flags to preserve old behavior alongside new. If unsure whether specific files or code paths should be preserved, ask the user explicitly rather than silently keeping them.

### Fix Quality

When encountering issues during implementation:
- If the correct fix aligns with the architecture → implement it properly
- If the fix would be a workaround, patch, or shim → do NOT apply it. Instead: add a `TODO(follow-up)` comment explaining the proper fix, note it in the feedback file, continue with the plan
- Never apply a dirty fix that contradicts architectural rules just to make tests pass
- A clean TODO with a follow-up plan beats a quick patch that accumulates debt

### Stub Prevention (CRITICAL)

Do not use empty test bodies, trivially-passing stubs, or "deferred to follow-up commit" placeholders to satisfy a named contract — a gate check, a characterization test, a plan invariant, a review obligation, a declared completion criterion. A stub that happens to pass is a gate-bypass, not a pass.

**Concrete anti-patterns, all forbidden on landed/mainline commits:**

- **Empty `#[test]` bodies.** `#[test] fn verifies_cycle_guard_terminates_on_recursion() {}` passes trivially and proves nothing. An un-ignored empty-body test is worse than an `#[ignore]`'d one — it falsely advertises coverage. If the body cannot be written yet, keep `#[ignore]` until the implementation lands.
- **Unconditional "unknown" / "default" returns as "scaffolding".** `fn relate_nodes(...) -> RelationResult::Unknown` that always returns Unknown is a nop, not a scaffold; `fn resolve(...) -> Opaque(Miss)` that always returns Miss is the same defect. Write real logic, or use `todo!()` / `unimplemented!()` so callers panic loudly and the nop is obvious from any first call.
- **"Real body deferred to follow-up commit."** A commit claiming to satisfy a gate via a stub, planning a later commit to "flesh it out", is bypassing the gate. The gate reflects implementation state on the tree under review, not future intent.
- **Always-true assertions.** `assert!(true)`, `assert_eq!(1, 1)`, `assert!(result.is_ok() || true)` — any predicate that holds regardless of the code under test is a stub in disguise.
- **Characterization tests that do not discriminate.** A characterization test must be writable such that it FAILS against the pre-change codebase AND PASSES against the post-change codebase. If that property does not hold, it characterizes nothing.

**Rule of thumb:** for every assertion you commit, ask "would this test catch the bug the cutover was written to fix?". If no, it is a stub.

**WIP exemption.** Scratch branches that will be squashed (e.g., `staging/*` → squash-merge to mainline) may contain `todo!()` bodies, empty tests, placeholder returns — that is their purpose. The rule applies to the squashed/landed commit, to any PR branch, and to any gate evaluated on the final tree. A landed commit message cannot cite "stub satisfies gate mechanically" as a legitimate state; that statement is a self-identified gate-bypass.

**Self-review obligation.** Before concluding a step that un-ignores or adds tests, re-open each test file and verify bodies are non-empty and assertions are discriminating. Before concluding a step that implements a function, verify the body exercises its inputs (branches on them, calls through to real logic) rather than returning a constant.

### Agent Feedback Capture

During work sessions, agents MUST continuously log feedback to a per-conversation file at `.feedback/feedback-{YYYY-MM-DD}-{short-id}.md`. The `.feedback/` directory is gitignored.

**What to log** — append entries whenever encountering something noteworthy:

- `[issue]` — bugs, unexpected behavior, workarounds applied
- `[improvement]` — code quality, performance, architecture ideas
- `[debt]` — things that work but could be better
- `[docs]` — missing or outdated documentation discovered

**Format**: `- [{category}] \`{file_path}\` — Brief description`

When delegating to subagents, pass the feedback file path and instruct them to append observations. One feedback file per conversation session.

## Dependencies Policy

- Keep dependencies at their latest versions
- Rust deps: update in `Cargo.toml`, run `cargo update`
- JS deps: `pnpm up -r -i -L` to interactively update all
- `workspace:^` deps are rewritten by `pnpm publish` automatically

## Commit Convention

This project uses **conventional commits** for automatic changelog generation via [git-cliff](https://git-cliff.org/).

```
<type>(<scope>): <description>

Types:
  feat     - New feature
  fix      - Bug fix
  perf     - Performance improvement
  refactor - Code refactoring (no behavior change)
  docs     - Documentation only
  test     - Adding/updating tests
  chore    - Build, CI, tooling changes
  release  - Version bump and release

Scopes:
  core     - verter_compiler Rust crate
  napi     - verter_napi / @verter/native
  wasm     - verter_wasm / @verter/wasm
  play     - playground
  unplugin - @verter/unplugin
  lsp      - language-server
  types    - @verter/types
  ts       - @verter/core (TypeScript)
  meta     - @verter/component-meta
  ci       - CI/CD workflows
  *        - multiple areas

Examples:
  feat(core): add v-memo directive support
  fix(wasm): correct memory leak in compile()
  chore(ci): add nightly WASM build workflow
  release(all): v0.0.1-alpha.1
```

## CI/CD

See [docs/contributing/ci-cd.md](docs/contributing/ci-cd.md) for detailed CI/CD documentation including:

- Workflow specifications (CI, nightly, release)
- Pre-release versioning flow (alpha → beta → rc → stable)
- Publishing process (npm + crates.io)
- Nightly WASM builds and playground deployment
- Required GitHub secrets configuration

## Skills Reference

Detailed reference material is available as on-demand skills (loaded automatically when relevant):

| Skill                    | Use When                                                                                         |
| ------------------------ | ------------------------------------------------------------------------------------------------ |
| `/type-resolution`       | Type solver, cross-file types, ShallowFileState, frontier engine, cache rules, macro traversal   |
| `/type-cache-architecture` | Fact-based cache architecture, env hash split (R21), `FileArtifactStore`, R1–R31 rules, module augmentation, multi-candidate storage |
| `/component-meta`        | Component metadata extraction, native/compat boundary, fallthrough, root inheritance             |
| `/compiler-codegen`      | Template codegen (VDOM/IDE), CodeTransform, cached directives, strict slots, style preprocessing |
| `/host-session`          | TypeProvider (TSGO/tsserver), workspace management, async scheduler, LSP host integration        |
| `/architecture`          | High-level module map, TS packages, plugin system, CSS analysis, MCP server, analysis types     |
| `/audit-infrastructure`  | `verter_audit` substrate, `HostAuditRuntime`, `AuditRequestRegistration`, `*_with_audit` API, footprint miner, structured events |
| `/position-encoding`     | Span types, position encoding, coordinate conversions, path normalization                        |
| `/build-and-profiling`   | Build order, rebuild sequences, profiling, MCP server setup                                      |
| `/testing`               | Test patterns, TDD workflow, sourcemap testing, server cleanup                                   |
| `/e2e-vscode-testing`    | VS Code E2E test fixtures, helpers API, adding new tests                                         |
| `/wsl-e2e-testing`       | WSL E2E tests to reproduce Linux/CI failures, fixture matrix                                     |
| `/rust-performance`      | Rust optimization patterns, allocation hierarchy, CodeTransform API                              |
| `/multi-agent-orchestration` | Driving a large multi-block plan, refactor, migration, or staged cutover autonomously: pure orchestrator + implementer/reviewer/fix sub-agents, dual review (independent + codex), per-block fix cycles, trust-but-verify |
