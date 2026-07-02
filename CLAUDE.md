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
- `verter_audit` is the leaf observability substrate (depends only on `verter_span`, no back-edge; lower crates emit through `current_observer()` (TLS) without knowing whether a `HostAuditRuntime` is installed); the concrete host runtime lives in `verter_session` — full ownership inventory in `/audit-infrastructure`.
- `verter_protocol` owns transport-facing schema DTOs; `verter_ffi` remains the thin native/WASM adapter layer.
- Consumer packages (`@verter/component-meta`, LSP, MCP, unplugin, playground) consume the shared substrate, not their own semantic forks.

Architectural consequence:

- A perf/correctness fix found in one surface is implemented in the shared owner layer whenever the behavior is reusable.
- Consumer-local wrappers stay thin and do not bypass shared parsing, analysis, resolution, or cache ownership.

**Exactly one type-resolution engine.** `SemanticQueryKey` → `ProjectSemanticDispatch::execute` → `SemanticGraphStore`, five query modes (`Identity`/`Navigate`/`Shallow`/`Expanded`/`Skeleton`) — the SOLE query-time type resolver. OXC is the syntax/lowering front-end ONLY (declaration bodies lower to `TypeExpr` lazily on first semantic demand through the scheduler-retained parse snapshot — the `DeclBodyMemo` owned by `IndexedReady`); OXC must never resolve types at query time. Any second query-time resolution path — a parallel `resolve_type` engine, a per-surface walker, a re-parse-and-resolve, an OXC element/frontier resolver — is a rule violation: delete it, route through the shared resolver. Two engines diverge; divergence is the bug/hang class.

**Audit infrastructure:** Rust-first deterministic per-request observability for every audited `VerterHost` entry-point (component-meta, type-resolution, compile, analyze, workspace ops, LSP handlers, MCP tools, bundler batches). TS bindings in `packages/types/audit.generated.ts`; opt-in via `HostConfig::audit_enabled + footprint_capture`. See `/audit-infrastructure` and [`docs/audit-footprint/`](docs/audit-footprint/).

Guards: `verter_audit_no_upward_deps`, `audit_substrate_isolation`, `audit_observer_single_accessor`; single-engine cluster (registered under Macro Type Traversal Rule): `no_new_resolve_type_engine_path_production_file`, `no_new_resolved_elements_production_file`, `no_new_prepared_surface_projection_production_file`, `no_new_from_eager_meta_production_site`, `no_new_duplicate_read_surface_members_definition`.

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

Guards: `no_thread_local_oxc_caches`, `no_direct_oxc_parser_calls_outside_scheduler_path`, `recursion_budget_invariant_across_module_boundary`.

### Shallow File Processing Core Invariant (CRITICAL)

The shallow file process is a core architectural invariant and must be preserved. When a canonical file is processed, the host stores its shallow symbol inventory once; that inventory is the authoritative index later stages query.

Shallow state must classify and retain at minimum: imports; exports and reexports; type declarations; interfaces; enums; classes; variables/constants; functions/method signatures; `typeof`-relevant value declarations; local symbol dependency edges; cross-file dependency edges.

Design rule: processing a file means collecting and indexing its symbols, not eagerly evaluating them; later stages look up the indexed items they need and process only those on demand; no stage rescans the raw file to rediscover symbols shallow processing already captured. Performance: very high performance comes from targeted demand after broad shallow indexing, not repeated partial reparsing.

Core invariants (full architectural-target detail: `/type-resolution` → IndexedReady Target Contract + Cache Population Target Contract):

- Canonical post-parse artifact = `IndexedReady`: a shallow declaration INDEX plus body locators, NOT a body store. Eagerly it carries canonical imports/exports, top-level symbol names/kinds, declaration spans, source-order contributor grouping, type-parameter names, syntactic member headers, and augmentation inventory — all safe for host-owned `Send + Sync` caches. Declaration BODIES lower only on first semantic demand through the shared lazy body service (the content-addressed `DeclBodyMemo` + scheduler-side `DeclLoweringService` retained-parse workers); publishing an artifact lowers ZERO declaration bodies. Component-meta and later analysis layers both build from it; symbol expansion populates and reuses the same shared resolver caches — no separate expansion paths.
- Parse each live file version once; the lazy lowering service RETAINS the parse snapshot on its worker shard (keyed `(canonical, whole_hash, parse_env_hash)`) so body demands reuse it instead of re-parsing per touch. Transient OXC parse arenas stay per-file/per-version and never leak into host-owned shared caches — jobs borrow the retained AST on the worker and return owned typed IR.
- Navigation stays narrower than expansion: walking `A['c']['full']['bar']` navigates intermediate hops and expands only the terminal requested projection unless limited normalization is required to continue.
- Generic substitutions are semantic meaning: navigation/expansion operate on instantiated types; cache keys include the relevant substitutions/type arguments.
- Navigators stay non-owning (choose the next hop, non-owning normalization only); reusable semantic work enters through the shared query API, not a private drill-down path. The shared semantic layer is keyed by semantic query identity and stores immutable semantic data or ids — never borrowed AST pointers or retained parser arenas.
- Completion fence: top-level live-host results record touched dependency facts, revalidate before publish, retry at most 3 times on mid-flight changes; never warm shared caches with torn provisional results; cancelled, superseded, interrupted, budget-exceeded, or partial results are never promoted warm.
- Waiters on in-flight work block cooperatively, never busy-spin; same-path recursion never self-awaits.
- Cache population is path-independent (same result from different entry points → same shared entry); broader successful results may backfill only the narrower entries they actually satisfied; narrower results must not pretend broader work is cached.
- Final payload caches hand out immutable `Arc` values; any backend preserving concurrency, size bounds, validation semantics is fine.

Guards: `audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries`, `macro_impacting_constructs_fail_lowering_not_silent_skip`, `indexed_ready_publish_lowers_zero_decl_bodies`, `resolve_unrelated_symbol_lowers_only_demanded_decl`, `lazy_decl_body_singleflight_lowers_once`, `no_indexed_ready_eval_env_or_type_decl_body_storage`, `emit_parse_facts_never_hashes_decl_bodies`.

**Project-global cache (final state):** `VerterHost` owns a single `ProjectTypeStore` accessed via `.project_type_store()` — the sole shared cache graph: `FileArtifactStore`, `AnalysisReadyDb`, the rehomed `RouteDb`, `OwnerImportSurfaceDb`, `ComponentMetaResultDb<ComponentMetaAnalysis>`, `MaterializeStructureDb`, `RefCycleResultDb`, `SemanticGraphStore` (which also owns the Vue macro resolution artifacts — the former `ResolvedNamedTypesDb`), `ShapeCacheDb`, and the `IntrinsicRegistry`. `IndexedReady` is the single canonical post-parse artifact (the former `ModuleFactsDb` is retired). Validated cache writes record a `ReadSetSignature.facts` fact signature (the path-precise fact-tracer observation set) — the sole cache-validity rail, revalidated against the live `StoreView` on every warm hit. The `StoreViewValidationToken` is the complete reuse/validity oracle; the singleflight LANE identity is the narrower `external_supersession_fingerprint` (reuse-oracle = full token; lane-identity = external fingerprint). See `/host-session` (store-view token dimensions, token-advance rules, lane identity, singleflight, `RequestStoreView`/`CanonicalCompletionOverlay`, handle-backed dims), `/component-meta` (`get_component_meta` final-result flow, `resolve_owner_direct_import`, `materialize_component_meta_structure`, the `ShapeCacheDb` per-member route, `reduce_field_type_expr_with_mode`), `/type-cache-architecture` (admission, `RefCycleResultDb`, retired split stores), and `/type-resolution` (`execute_cooperative` dedup, `SemanticNodeData::VueMacroElements` hot path, `IntrinsicRegistry::lookup`).

### Canonical Dependency Cache Rule (CRITICAL)

Host-backed type/import resolution treats the canonical file ID as the cache identity. Load and parse each dependency at most once per canonical ID per workspace content generation. Cache the parsed state, the shallow declaration index plus lazy declaration-body memo, symbol/export tables, and prepared declarations together. Later lookups hit cached maps — never rewalk the AST. VFS is the authority for file-change invalidation. Concurrent cold requests to the same file collapse onto one materialization path. Changes land as one clean cutover, no dual-path shims.

Guards: `host_upsert_performs_no_reverse_dependent_eviction`, `host_upsert_reverse_dep_eviction_scanner_discriminates`, `import_route_writer_guard`.

See `/type-resolution` skill for the full rule set (invalidation semantics, route caches, prepared declarations, cross-owner reuse, negative caching, the concrete performance contract).

### Cache Architecture (CRITICAL)

The fact-based cache architecture splits cache keys across five orthogonal env-hash dimensions (`parse_env_hash`, `resolve_env_hash`, `type_env_hash`, `lib_env_hash`, `project_identity`). Each cache layer keys only on the dimensions it actually depends on (R21 scoping rule — a single bundled `project_config_hash` is forbidden). `lib_env_hash` enters a key only when the value depends on lib data: `ResolvedImportFacts` does NOT include it; `RouteDb`, typed-IR resolve, `MaterializeStructureDb`, `RefCycleResultDb`, `SemanticGraphStore`, `ComponentMetaResultDb` DO.

Two cache families: **content-addressed artifact caches** (`FileArtifactStore`, `ResolvedImportFacts`, typed-IR resolve, `MemberSemanticFactStore`, `MemberDisplayFactStore`, `ModuleAugmentationIndex`) carry `content_hash` or `parse_stable_hash` in the key; **query-identity caches** (`RouteDb`, `MaterializeStructureDb`, `RefCycleResultDb`, `SemanticGraphStore` query nodes, `ComponentMetaResultDb`) exclude version hashes from the key — concurrent variants coexist as candidates in one slot, with version rooting on the cached value (the structural + semantic-graph caches — `MaterializeStructureDb`, `RefCycleResultDb`, `SemanticGraphStore` memo, `ShapeCacheDb` — root via `ReadSetSignature.facts` + `self_root_canonicals`; `RouteDb` via its value-side `ValidatedFactCache` fact signature; `ComponentMetaResultDb` via the owner whole-hash candidate discriminant + `ReadSetSignature.facts`). Cache keys never include `fact_dep_signature`. The `MaterializeStructureDb` subject is the content-free `MaterializationCacheKey` (a `ResolvedDeclSlotIdentity` slot + projection/policy/mode axes + `resolve_env_hash`), NOT a graph-instance `SemanticNodeId` — the per-thread recursion identity `MaterializeRuntimeKey` is a separate, non-cache key; a root-less anonymous subject keys no slot (uncached). `RefCycleResultDb` keys the content-free `RefCycleResultKey` (`ResolvedDeclSlotIdentity` slot + `resolve_env_hash` + version), NOT the versioned `DeclIdentity`.

Family-memo slots (`SemanticQueryKey::Instantiate.base` / `ResolveMacroPayload.owner`, mirrored on `FamilyKey`) are the env-bearing, content-free `ResolvedDeclSlotIdentity` (R6 — content/version hashes and the versioned `DeclIdentity` are forbidden in any derived-`Hash` query-identity key; the live whole-hash is re-sourced at value-compute time, never carried in the key). A warm hit requires TWO independent gates (§3.4): `cached_satisfies` over a RECORDED materialised `(path, point)` the candidate's compute actually produced — never the candidate's nominal slot/mode, never enum rank — AND per-candidate `ReadSetSignature.validate_with_self_roots` against the caller's live view. Backfill clones only recorded materialised points, directionally gated (the `Shallow → Navigate` clone is lattice-unsound). `validated_at_generation` is recency metadata only, never a validity oracle. See `/type-cache-architecture` for the full key/context composition (`InstantiateContext`/`MacroPayloadContext` per-key contexts, `FAMILY_SLOT_CANDIDATE_CAP = 4` candidate semantics, non-file-base rooting).

`FileArtifactStore` is the authoritative per-file storage layer, keyed by `(canonical, content_hash, parse_env_hash, parser_version, file_language_id)` — `file_language_id` is the file's `FileLanguage` row (the per-file classification dimension of artifact identity, so a framework-capability flip misses exactly the affected files' artifact slots without touching the global `parse_env_hash`). The overlay-aware `augmentation_index` (module-augmentation inverse lookup) lives on the same store. See `/type-cache-architecture` for the full key composition, `file_language_id` producer wiring, `AugmentationTargetKey`/`AugmentationPopulation` semantics, and the `parse_stable_hash` definition.

Cache runtime hard rules — three always in force: cache correctness is read-side authoritative; `ReturnOnly` (overflow, budget exhaustion, cancellation, generation supersession, incomplete self-rooting, unresolved provenance) never publishes entries, reverse-index metadata, or persistent artifacts; overlay/session results never populate base-only or persistent caches. Full 20-rule list: `/type-cache-architecture` → Cache Runtime Hard Rules.

Guards: `cache_satisfaction_is_materialized_point_not_nominal_demand`, `cache_satisfaction_requires_path_exact_not_prefix`, `backfill_writes_only_recorded_materialized_points`, `no_off_store_host_caches`, the `r6_*` cluster, plus the four migrated-query-identity-key guards in `tests/g_cache/r6_r21_query_identity_keys.rs` (`component_meta_result_key_*`, `route_name_key_*`/`barrel_surface_key_*`, `ref_cycle_result_key_*`, `materialization_cache_key_*`) — full list in `CRITICAL_RULE_GUARDS`.

See `/type-cache-architecture` skill for the full rule set (R1–R31, two-fact `MemberPresence`/`Member` model, multi-candidate substrate, signature-overflow contract, module augmentation completeness, heuristic-cache-semantics prevention, exact policy identity) and `docs/arch/fact-based-cache.md` for the per-field audit table + per-cache-layer key composition.

### Macro Type Traversal Rule (CRITICAL)

When resolving cross-file macro types (`defineProps<T>()`, `defineEmits<T>()`, component-meta deep expansion, etc.), only follow the import graph reachable from the requested type's declaration graph. There is one shared cross-file type resolver with five query modes: `Identity`, `Navigate`, `Shallow`, `Expanded`, `Skeleton` (see `/type-resolution` → Query Mode Contract).

**Macro resolution is one shared path, not a per-macro engine.** Every macro (`defineProps` / `defineEmits` / `defineOptions` / `defineSlots` / `withDefaults`) and every imported `.vue` component surface resolves through exactly TWO steps: (1) resolve ONE type via the shared typed-IR five-mode dispatch — the generic-parameter type (`define*<T>()`) OR the object-argument type (`define*({ ... })`); `withDefaults` resolves the props payload type plus the defaults-object type and merges; `.vue`-component imports resolve the synthesized `$props` / `$emit` / `$slots` / expose surface recursively through the same dispatch (the hardest case — apply EXTRA caution: it is exactly where rule violations cause the worst hangs); no macro-specific engine, no per-surface walker, no eager element resolver. (2) Normalise per kind — a thin transform, NOT a resolver (per-kind field rules: `/type-resolution` → Macro Type Traversal Rule). A macro/import that resolves through anything else, or flattens a full surface eagerly before the consumer demands it, is a rule violation — collapse it into `shared_resolve(type) + normalise`.

`Skeleton` is the BFS / generic-helper traversal mode: unbound type parameters stay `TypeParam` shells so Conditional branches do not collapse to `never`. Path projection is path-precise: intermediate hops run in `Navigate`, the terminal hop runs in the caller's mode; non-contributing intersection arms are ignored (not rewritten to `never`); open conditionals distribute the remaining path into both branches; closed conditionals reduce immediately. Do not walk unrelated imports. Do not treat plain imports as implicit exports. Cache discovered symbol mappings and barrel hops.

**TS-first resolution priority:** TypeScript types always take priority over JavaScript files. Use `effective_target()`: `.d.ts` > `.d.cts` > `.d.mts` > `.ts` > `.tsx` > `.js` > `.jsx` > `.cjs` > `.mjs`.

**Owned resolution is bounded by `workspace_root`:** `node_modules` and package `#imports` ancestor walks stop at `IdeProjectConfig.workspace_root`.

Guards: `root_conditional_still_distributes`, `no_macro_string_heuristics_in_resolver_core`, `no_text_based_macro_surface_projection_helpers`, `no_role_inference_from_name_suffix`, `no_pick_or_omit_string_prefix_check`, plus the `no_new_*` single-engine shrinking-ledger cluster — full list in `CRITICAL_RULE_GUARDS`.

See `/type-resolution` skill for the full traversal rules and resolver mode details.

### Declaration Merging (CRITICAL)

Same-name declaration merge is produced ONLY by `verter_semantic::type_eval` ordered declaration groups: `EvalEnv` appends contributors in source/binder order (`add_type`/`add_value` push onto an ordered `TypeDeclGroup`/`ValueDeclGroup` — no last-wins `FxHashMap<String, TypeDeclInfo>`/`…ValueDeclInfo>` map, no overwrite `insert` for mergeable kinds). Same-name `interface` declarations lower to the explicit `TypeDeclBody::Merged` carrier (on the memo-owned `LoweredTypeDecl.body` read through `ShallowFileState::type_decl(name)` → `PreparedTypeDecl.merged_contributors`), interned as a distinct `SemanticNodeData::MergedDecl { contributors }` node.

A merged declaration MUST reach the project-semantic reducer as that distinct carrier — a bare `TypeExpr::Intersection` / `SemanticNodeData::Intersection` is FORBIDDEN as the merged-decl representation, because the intersection reducer applies **heritage-shadow** member precedence and cannot accumulate method overload groups. The `MergedDecl` peer-merge reducer (`reduce_merged_decl_with_graph` + `merge_declaration_surfaces`): (a) same-name methods/call-signatures ACCUMULATE into one ordered overload group across contributors in source order; (b) conflicting non-method properties take deterministic first-contributor precedence (never `never`); (c) distinct members union.

Functions accumulate into an ordered `Vec<FunctionSignature>` (`ValueDeclGroup::merged_signatures`), each carrying `has_implementation_body`; overload visibility is a PROJECTION-time rule (`build_typeof`): a lone signature is visible (even if bodied), a multi-signature group surfaces every bodiless overload in source order and hides the trailing implementation. Same-file merged values version-root on the owner's single `FileWholeHash` self-root under a content-free query-identity key (R6). `verter_session` MUST NOT synthesise the merge as `raw_body = TypeExpr::intersection(...)`. Cross-file ambient augmentation (`declare module`/`declare global`) reuses this same `MergedDecl` peer-merge path — see Declaration Augmentation (CRITICAL).

Guards: `eval_env_type_symbols_are_grouped_not_last_wins_map`, `eval_env_add_decl_appends_not_overwrites`, `no_intersection_merge_synthesis_in_verter_session`, `merged_decl_lowers_to_distinct_carrier_not_intersection`, `declaration_merge_facts`.

See `/type-resolution` skill for the carrier chain, the peer-merge reducer, and the architecture guards.

### Declaration Augmentation (CRITICAL)

Ambient declaration augmentation (`declare module "X" { ... }` / `declare global { ... }`) is a RETAINED, addressable scoped inventory — never fingerprint-only facts, never file-scope pollution. `EvalEnv.augmentation_scopes` / `EvalEnv.augmentation_value_scopes` key `(AugmentationScopeKind {Global, Module(specifier)}, name)` → ordered `TypeDeclGroup`/`ValueDeclGroup`, mirrored on `ShallowFileState`; inner decls NEVER enter file-scope `type_symbols`/`value_symbols`. Parse-domain `ModuleAugmentationFact`s are DERIVED from this typed inventory (`fact_emission::collect_augmentations`) — NO raw-source byte-scan.

Cross-file augmentation merge is the SAME `MergedDecl` peer-merge path as same-file merging — NOT a second merge engine: `stitch_module_augmentations` finds every augmenter via `FileArtifactStore::ensure_augmentation_index_populated`, lowers each augmenter's RETAINED inner body in its own file context, and folds base ∪ augmenter contributions into ONE `SemanticNodeData::MergedDecl` carrier; augmenter order is the stable `(canonical, parse_stable_hash)` key — discovery-order-independent.

Facts rail: the cold stitch observes one `FactKey::ModuleAugmentationIndexShape` fingerprint plus one `FileWholeHash` per contributing file and records `self_root_canonicals = {base} ∪ {augmenters}` — a content edit to ANY contributor misses the warm read; torn/partial routes through `ReturnOnly`. Query keys stay content-free (R6). The index is OVERLAY-AWARE (`AugmentationPopulation {Base, Session(overlay-set fingerprint)}`): overlay augmenters NEVER poison the base index and NEVER cross sessions, and there is NO base-only session assert on the augmentation-index / `EffectiveExportSet` surface — a session view is accepted under `Session` scope.

Guards: `session_overlay_augmenter_isolated_from_base_index`, `effective_export_set_session_view_stitches_overlay_augmenter`, `no_effective_export_set_base_only_session_assert`.

See `/type-resolution` skill for the stitch chain and the overlay-aware index, and `/type-cache-architecture` for the content-addressed vs query-identity augmentation key split.

### Two Template Codegen Paths (CRITICAL)

The Rust compiler has two separate template codegen paths; modifying one does NOT affect the other: **VDOM/Vapor** (`template/code_gen/vdom/`) for runtime render functions, and **IDE** (`ide/template/`) for valid JSX/TSX used by LSP/TSGO type checking. The LSP uses the IDE path via `CompileTarget::IDE`.

Guards: `compile_audit_sourcemap`.

See `/compiler-codegen` skill for full codegen pipeline, backends, and CompileTarget details.

### Carrier IDE TS Surface Principle

North star for the IDE/LSP experience: for every carrier with an IDE projection (`.vue`, `.svelte`), the script block (`<script>`, `<script setup>`, Svelte module/instance scripts) AND the supported template/markup expressions are **ONE** generated TypeScript/JavaScript/JSX surface — interpolations (`{{ }}`, Svelte `{expr}`), directive/attribute expression values (`v-if`/`v-for`/`v-bind`/`:`/`v-on`/`@`/`v-model`/`v-slot` and dynamic args; Svelte `bind:`/`on:`/`class:`/`style:`/`use:`, `{#if}`/`{#each}`/`{#await}`/snippets, `{@render}`/`{@html}`/`{@const}`, rune calls) all lower into it. That surface is obtained through the IDE path (`CompileTarget::IDE`/`TSX`), synced to the active TypeProvider, with provider positions/ranges/edits mapped back through the document's `ProviderPositionMapper`.

**The bar:** for any supported mapped TS/JS expression position, every provider-backed IDE feature — diagnostics, hover, definition/type-definition, references, rename, completion/resolve, signature help, document highlights, semantic tokens, inlay hints, and generic code actions whose edits map exactly — should behave like the equivalent standalone `.ts`/`.js`/`.jsx` program, with results mapped back to the carrier source. A binding represented in BOTH script and template is discoverable and renamable from either side (rename spans script + template; find-all-references finds both). This holds for **both Vue and Svelte** over the shared LSP path.

Fail-closed boundary: unmapped synthetic helper code, framework tokens with no TS correlate, unsupported/experimental projection regions, and provider edits whose full ranges cannot be mapped must fail closed or return framework-native results — never mis-mapped. Source actions (organize-imports, fix-all, formatting) require explicit per-action support and tests; they are NOT implied by this principle. This is a **principle, not yet a `(CRITICAL)` guarded rule** — it is promoted to CRITICAL once real-provider cross-region Vue/Svelte regression tests guard it.

See `/compiler-codegen` → "Carrier IDE TS Surface Principle" for the full normative text (every covered expression form), and `/host-session` / `/position-encoding` for provider sync and position/range/edit mapping.

### Fallthrough / Root Inheritance (CRITICAL)

The shared Rust pipeline owns all fallthrough and root inheritance semantics. `verter_semantic::analysis` extracts root reachability facts only. `verter_session` owns the single inheritance resolver, recursion, conditional branch composition, generic propagation, caching, and final metadata projection.

Key rules: `inheritAttrs: false` → no inherited surface. Single native root → intrinsic attrs minus declared props/events. Single component root → recursive propagation. Conditional branches → exact union. Cycles → unresolved branches. `class`/`style` are never consumed.

Guards: `fallthrough_recomputes_from_runtime_subnodes_after_top_level_node_clear`, `fallthrough_runtime_reuse_survives_host_cache_clear`, `fallthrough_reuses_root_follow_after_branch_union_node_clear`.

See `/component-meta` skill for the full semantic rules, public contract, authority chain, and key files.

### Component-Meta Shallow-By-Default Rule (CRITICAL)

Types and properties are ALWAYS published shallow at the projector surface UNLESS the consumer explicitly walks the path. This is the single architectural invariant the projector pipeline (`meta_resolve::projectors::reduce_published_field_types` + `reduce_field_type_expr_with_mode`) enforces.

Concrete contract:

- Plain alias references (`type Foo = ...`) — published prop type stays `TypeExpr::Ref { name: "Foo" }`. Consumers re-resolve `Foo` through the registry on demand. The projector does NOT eagerly inline the alias body.
- `Pick<Foo, "bar">` — materialises ONLY the `bar` member of Foo. Other Foo properties stay shallow (path-precise). Built-in utility types (`Pick`, `Omit`, `Required`, `Partial`) behave identically to a userland implementation referencing the same keys.
- **Carrier-preserving decl-body lowering.** Under `Shallow` (as under `Navigate` / `Skeleton`), decl-body lowering interns `DeclRef` / `InstantiationRef` carriers for member-value type references — including ALL builtin utilities — and never executes `ResolveDecl` / `Instantiate` eagerly; eager lowering-time execution is `Expanded` / `Identity` only; materialisation enters exclusively through the demand points (PathWalker hops, the shallow-surface synthesiser's carrier unwrap, closed object-filter surface reads, the relation/conditional oracle). Eager Shallow member-value lowering was the `Table.vue` storm: 94.3% of all budget charges were `Instantiate(StructuralTransit:Shallow)` recursion across the transitive TanStack decl graph.
- **Open key domain ⇒ shallow carrier (L1) — route/mode-independent.** TWO families stay shallow carriers at EVERY entrance, in every mode, and open-OR-UNKNOWN (including traversal-budget exhaustion) preserves the carrier instead of falling through into Expanded materialisation: (1) an object-filter utility (`Pick`/`Omit`) whose enumeration domain is OPEN or undecidable (`Pick<PropsBase<T>, …>` over the SFC's open `generic="T"` stays `Pick<…>`); (2) a mapped type `{ [K in S]: V }` whose produced surface still depends on an unbound OUTER generic (a CLOSED-key/open-VALUE mapped enumerates its keys path-precisely with shallow values). Closed sources still materialise the requested keys path-precisely. Typed-IR only, no string matching. The carrier-stop is the PRIMARY defense for the open-generic class; the per-request projection budget (`request_budget.rs`) is an ARMED-by-default runaway fuse (`projection_op_budget == 0` ⇒ effective cap 2000) whose trip returns `BudgetExceeded` as a genuine partial — refused warm admission, the no-poison invariant. Publication demand is `Navigate`-only on the projector/registry macro surfaces: a full `get_component_meta` records ZERO `Published(Expanded)` projection contexts; `Table.vue` and `ChatMessages.vue` are COMPLETE corpus members with un-ignored green trackers (`table_resolves_complete_and_warm`, `chat_messages_resolves_complete_without_false_partial`, `chat_messages_resolves_without_timeout`). The FULL authoritative spec — entrances, owner predicates, the per-argument position-sensitive key-domain rule, the tri-state conditional oracle, per-utility output-key semantics, mapped family composition, OPEN/CLOSED definitions, memoization, invalidation, the `TypeOf` demand rails, and the four named current scoped exceptions — lives in `/type-resolution` → Open-Key-Domain Carrier-Stop (L1).
- `Omit<Foo, "bar">` — keeps `bar` shallow (excluded from the surface) and materialises the others.
- `Foo['a']['b']` — path-precise: only the `a` and `b` hops load; other Foo keys never enter the published surface.
- True recursive types (`type Self = Pick<Self>`) — NOT supported. The published surface stays the bare `Ref { name: "Self" }`.
- Imported alias names (workspace-owned OR package-backed) — stay shallow regardless of where they live.

The projector pipeline is the sole post-projection authority — no eager per-field materialisation runs at publication time.

Guards: `decl_body_lowering_keeps_member_value_refs_as_carriers`, `publication_routes_never_demand_expanded`, `chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier`, `closed_pick_sources_still_materialize_path_precisely`, `projection_budget_counts_instantiate_and_conditional`, `cycle_guard_roots_at_utility_source_type_argument` — full list in `CRITICAL_RULE_GUARDS`.

See `/component-meta` skill for the publication-surface rules and the locked-down negative tests in `crates/verter_session/src/meta_tests.rs`, and `/type-resolution` for the authoritative L1 spec.

### Component-Meta Native Vs Compat (CRITICAL)

The native component-meta payload is the semantic authority. `@verter/component-meta/compat` is a projection layer for `vue-component-meta` interoperability, not a second semantic pipeline.

Core rules: Fix metadata in the native layer first. Rust owns resolution, declaration routing, graph construction. One async native request per query. JS may transform structure but must not recover meaning. JS must not become a second resolver or expander. Cache-owned type recovery only — no AST/source fallbacks.

Guards: `no_napi_direct_verter_compiler_emitters`, `compat_one_napi_call_audit`.

See `/component-meta` skill for the full policy, resolver rules, and cache contracts.

### Typed-IR-Only Resolver Rule (CRITICAL)

The native component-meta / typeinfo type resolver — analyzer → projector → registry → policy → materialiser — drives semantic decisions exclusively from the typed IR (`verter_semantic::analysis::type_expr::TypeExpr` on Rust, `TypeDescriptor` from `@verter/type-ir` on TS). Forbidden inside that pipeline:

- Source slicing, regex against type text, hand-rolled type-text splitters (`split_top_level_*`, `find_top_level_char`, `extract_pick_slot_bindings`, `extract_string_literal_name`, `splitTopLevelTypeOperator`), `starts_with("Pick<")` shape sniffing, and the synthesise-then-reparse pattern (`format!(...).parse_type_annotation(...)`). Walk the typed IR instead.
- `parse_type_annotation` anywhere except JSDoc tag-type payloads — the single explicit text exception: `{Type}` payloads inside JSDoc tags are inherently text, parsed via the dedicated JSDoc path only.
- Parsing back raw / display strings (`Analyzed*Field.type_annotation`, `ExpandedField.raw_type`, `ResolvedLocalType.expanded`, `PropMeta.rawType`) — display-only passthroughs. The JS compat layer (`@verter/component-meta/compat`) reads `prop.type` (`TypeDescriptor`) for every semantic decision; `prop.rawType` must not feed any `looksLike*`, `extract*`, `normalize*`, `split*`, `strip*`, `prefer*`, `shouldPrefer*`, or `repairOpaque*` branch.
- Substring path classification (`"/node_modules/"`, `"\\node_modules\\"`) — use `ResolverContext::workspace_is_workspace_owned` / `workspace_is_package_backed`.
- Name-suffix role inference (`name.ends_with("Props")` / `"Emits"` / `"Events"` / `"Model"` / `"Slots"`). Type-role classification is structural, not nominal: a type is a prop/emit/model/slot type because a Vue SFC macro (`defineProps`, `defineEmits`, `defineModel`, `defineSlots`, `withDefaults`) consumes it — read from `AnalyzedMacro.kind` / `parsed_type_argument` / `type_references` on the analyzer snapshot.

OXC is a syntax/lowering front-end only and never resolves types at query time. Macro/JSDoc producer fields still lower at their producer boundary via `lower_ts_type(ts_type, source)` (stored alongside `Analyzed*Field`, `ResolvedLocalType.type_expr`, `ProjectedMacroSurfaces.*_expr`, surviving all caches); top-level declaration bodies lower LAZILY through the scheduler-retained parse snapshot (`DeclBodyMemo` → `DeclLoweringService`) and return owned typed IR before dispatch/reducers ever see them — no raw-string reparsing, no OXC resolver path. If a new requirement appears to need text manipulation inside the resolver, fix the producer (lower the right OXC node, store the right typed field, extend `@verter/type-ir` with a missing variant) rather than reparsing or pattern-matching on text.

Guards: `no_macro_string_heuristics_in_resolver_core`, `no_format_then_reparse`, `no_role_inference_from_name_suffix`, `no_node_modules_substring_outside_workspace_api`, `no_pick_or_omit_string_prefix_check`, `lazy_decl_lowering_uses_scheduler_snapshot_not_reparse`, plus the rest of the typed-IR guard cluster — full list in `CRITICAL_RULE_GUARDS`.

See `/component-meta` and `/type-resolution` skills for the typed schema contract, the producer-side lowering points, and the architecture-guard list.

### CodeTransform Is the Single Source of Truth (CRITICAL)

**All modifications to generated code MUST go through `CodeTransform` operations** (`overwrite`, `prepend_left`, `append_left`, `move_with_suffix`, etc.) — never string replacements, regex transforms, or manual splicing on the output of `build_string()` or content produced by a `CodeTransform`. `CodeTransform` generates source maps by tracking chunks (Original, Inserted, Moved, Overwritten); modifying the string after the transform desyncs byte offsets → LSP position mismatches (hover landing on the wrong token, go-to-definition jumping to wrong locations).

**Correct:** `ct.prepend_left(pos, ".ts")` — chunk list and source map stay consistent. **Wrong:** `content.replace(".vue'", ".vue.ts'")` on the built string — the source map still reflects pre-replace byte offsets.

Guards: `compile_audit_sourcemap`.

### Typeinfo Wire Contract (CRITICAL)

The typeinfo graph wire surface (`crates/verter_protocol/proto/verter/v1/typeinfo.proto`, its generated Rust and TS bindings, and the audit envelope on top) is a closed contract. Four invariants:

1. **Closed-enum discipline.** `GraphTypeNode.kind`, `StructuredTypeExpression.kind`, `TypeInfoGraphRequest.payload`, `TypeInfoRequestError.kind` are closed `oneof` taxonomies. Adding a variant bumps `SemanticTypeGraph.schema_version`; removing one requires `reserved` directives at the enclosing message scope (proto3 forbids `reserved` inside an `oneof` block).
2. **Wire-compat: field numbers never reused.** A retired variant's tag goes into the message's `reserved` list with its name (off-tree clients keep round-tripping the slot as an unknown field); new variants take the next free tag, never a recycled one.
3. **Audit envelope additions are purely additive.** Every new typeinfo audit field (`structured_event`, `kind_payload`, `RequestKind::TypeInfoGraph`) lands as a new arm or a default-zero field, never a replacement.
4. **Request validation runs before semantic execution.** `validate_type_info_graph_request` rejects malformed envelopes through a typed `TypeInfoRequestError`; the schema-version gate is closed-set (`SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS`); per-variant structured-expression validation is exhaustive over the `oneof` taxonomy.

Guards: `typeinfo_graph_taxonomy` (`crates/verter_session/tests/typeinfo_graph_taxonomy.rs` — proto/TS oneof parity), `typeinfo_proto_ts_freshness` (`crates/verter_protocol/tests/typeinfo_proto_ts_freshness.rs::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output` — regenerates the TS bindings via the workspace `buf` and `oxfmt` binaries and byte-compares), `request_kind_payload_parity` (`crates/verter_audit/tests/request_kind_payload_parity.rs`), `typeinfo_request_validation` (`crates/verter_session/tests/typeinfo_request_validation.rs` — closed-set schema-version + exhaustive structured-expression coverage), `typeinfo_wire_surface_guards`, `typeinfo_graph_contract_guards`, `typeinfo_request_contract_guards`, `typeinfo_audit_contract_guards`.

### Cross-Platform Portability (CRITICAL)

The codebase MUST build, test, and materialize on macOS, Windows, AND Linux. Platform-assuming code is a defect, not a nit.

Guard-enforced — `tracked_paths_are_portable` (`crates/verter_session/tests/tracked_paths_are_portable.rs`) enumerates `git ls-files -z` and enforces: valid UTF-8; no NTFS-illegal characters (`< > : " | ? * \` plus control chars); no trailing dot or space; no reserved device basenames (`CON`/`PRN`/`AUX`/`NUL`/`COM1`–`COM9`/`LPT1`–`LPT9`, with or without extension, plus `CONIN$`/`CONOUT$` — the `$`-suffixed forms only); no case-insensitive path collisions (lowercase-fold approximation of NTFS/APFS folding, not the exact filesystem fold tables); ≤200-byte relative paths.

Review-enforced (the guard does not cover these):

- Sanitize generated on-disk names (e.g. `blake3:<hash>` → `blake3-<hash>`) — logical identifiers are unconstrained; only the on-disk boundary is. The guard only sees tracked paths, so it catches a generated name once committed, not at generation time.
- Build paths with `Path`/`PathBuf`/`Path::join` — never string concatenation with hardcoded `/` or `\`.
- Byte-equality comparisons over checked-out text normalize line endings (CRLF ↔ LF) or compare as text — never raw bytes embedding EOL.
- OS-specific binaries (`tsgo`, `.exe` suffixes) are discovered platform-aware, never via a hardcoded per-OS name.
- Temp and cwd paths come from std abstractions, not literal paths.

Guards: `tracked_paths_are_portable`.

### Framework Adapter Substrate (CRITICAL)

Multi-framework component support is ONE shared adapter substrate, not a per-framework semantic fork. `verter_session::framework` owns the `FrameworkAdapterRegistry` (built once at `VerterHost` construction), the per-adapter `FrameworkAdapterDescriptor` (identity, supported surface kinds, carrier language, the `VirtualFileNaming` column), the facts/carrier-only `FrameworkAdapterCtx`, the `ComponentDefaultSynth` seam, and the two-pass script-fact seam. Vue is the REFERENCE adapter — re-housed as a true plan/normalize adapter (`VueFrameworkAdapter` + the relocated `vue_exec` resolution delegates), NOT a privileged hardcoded path.

Closed-contract rules:

- **One audited wire entry, validation-first.** `VerterHost::resolve_framework_surface_with_audit(TypeInfoGraphRequest)` is the SOLE entry for the `GRAPH_OPERATION_FRAMEWORK_SURFACES` operation. It runs `validate_type_info_graph_request` FIRST (op/payload-arm match, schema echo, the nested framework-surface validator) — a malformed envelope returns the typed wire `error` arm BEFORE any registry lookup or semantic dispatch. A bare-inner-request entry is forbidden. The operation rides the EXISTING typeinfo graph envelope, and its current `FrameworkSurfacePayload`/embedded-`SemanticTypeGraph` shape is PROVISIONAL — an interim wire pinned today, NOT a permanent "no schema change" guarantee. The hard gate `S5.B11/B12 → U8` was landed ahead of order, so U8 still OWES the retag of `FrameworkSurfacePayload.graph` to a `TypeInfoGraphPayload` carrier, the `SemanticTypeGraph.schema_version` bump, and reserving the old field per the Typeinfo Wire Contract (CRITICAL) above; until U8 lands this wire stays pinned but is not final. Guard `framework_surface_wire_executor_validates_first`.
- **Registry dispatch, no privileged framework branch.** The executor interns `selector.framework_adapter_id`, looks up the registry (unknown id ⇒ typed `MalformedPayload`, NO new error variant), and dispatches to the adapter. Every wire `FrameworkTag` maps to a registered adapter OR an explicit `TagDisposition` row (`DeferredVertical` / `OutOfScope`); a tag's existence is NOT a support guarantee — support is asserted only by a registered adapter and surfaced per-request via `FrameworkSurfaceKindStatus`. Guard `framework_registry_complete` (+ the `framework_surface_executor` integration suite).
- **Closed plan/resolve/result vocabulary.** The adapter PLANS demands (`plan_surfaces` ⇒ closed 4-variant `PlannedDemand` — `MacroPayload` / `PathProjection` / `ShallowSurface` plus the Svelte arm `SvelteSurface`; no `Custom`/`Raw` arm, no source text / OXC handles / raw `SemanticQueryKey`s) and NORMALIZES resolved data (`normalize`); it holds NO resolve entry point. The executor resolves each `PlannedDemand` through the module-private `ExecutorResolveCtx` (EXHAUSTIVE match, no wildcard) THROUGH the one shared type-resolution engine — it plans, dispatches, and encodes; it is never a second resolver. Per-kind status maps DIRECTLY onto `SUPPORTED`/`PARTIAL`/`UNSUPPORTED` via the typed `ResolvedOutcome` (a supported-empty kind stays distinct from an unsupported kind). The first `SemanticTypeGraph` encoder (`graph_export`) is a pure ZERO-DISPATCH shallow projection of resolved data — named refs mint `GraphSymbolNode` + `GraphReference{symbol_id}`, structural unencodables degrade to `GraphOpaque`, never a fabricated ref and never a re-resolution.
- **Facts/carrier-only adapter ctx.** `FrameworkAdapterCtx` exposes EXACTLY two ops — `carrier_for::<T>` (the adapter's typed parse carrier, `None` for a carrier-less adapter — never a forged token) and `script_facts_for::<T>` (resolved script facts on demand). It never resolves types, indexes a file, runs OXC, calls `ProjectSemanticDispatch`, or reads a `StoreView`. Guard `framework_adapter_ctx_closed_surface`.
- **Two-pass script-fact seam.** The syntax-capture half (`verter_semantic::analysis::framework_facts`) captures candidates from the live OXC program — SYNTAX-ONLY (may touch OXC + `lower_ts_type`, MUST NOT resolve imports or read capability bits; guard `script_fact_capture_is_syntax_only`). The resolved-validation half (`framework/script_facts`) drives provider `validate` on demand over neutral resolved-import + capability data, content-addresses candidates, and publishes resolved facts under a fact-rail + strict-same-generation gate with `SignatureAdmission::Cacheable`-only publication (overflow ⇒ `ReturnOnly`, no warm). An EMPTY active-provider set is byte-identical zero-cost (Vue does NOT move onto the seam). The `ActiveProviderIndex` is the shared gate authority. Guard `script_fact_providers_zero_cost_on_miss`. The framework-surface result caches (`FrameworkSurfaceStore` / `FrameworkScriptCaches`) are fact-validated today but live on the framework registry rows, NOT the single `ProjectTypeStore` — they are PROVISIONAL off-store caches to be consolidated onto `ProjectTypeStore` (and given true singleflight) at U10.
- **Parse-domain component-default synth.** `ComponentDefaultSynth` synthesises a component's default-export value symbol from PARSE-DOMAIN inputs only (macros + syntax-capture candidates); it never names the resolved-validation fact types. Registry-dispatched at the shallow-analysis injection points by the file's resolved language. Guard `component_default_synth_parse_domain_only`.
- **Generated virtual-file naming is descriptor-owned.** The `VirtualFileNaming` column is the single authority for an adapter's IDE / API / testing-API / sidecar suffixes; the committed TS mirror (`packages/typescript-plugin/src/generated/virtual-file-naming.ts`) is rendered from it and byte-pinned. Guard `virtual_file_naming_ts_freshness`.
- **No re-export shim for relocated Vue resolution.** The Vue resolution bodies relocated to `framework_surface::vue_exec`; `typeinfo/adapters/vue/{public_type,surface,store}.rs` are DELETED with no re-export shim or alias under `adapters::vue`, and `VueShallowMetadataStore` / `VueMacroDtoKey` are retired. Guards `vue_relocation_no_shim` + `retired_symbols_absent_from_production_source`.

See the `/framework-adapters` skill for the substrate's module map, the descriptor/registry/ctx/executor contracts, the script-fact seam, and Vue as the reference adapter.

### Project-Bound External-TS Contract (CRITICAL)

Production external-TypeScript results for carrier sources are project-bound. The result-producing backend path is `ExternalTsProjectResolver` → `CarrierRegistry` → `EngineBackend`: `EngineBackend::ensure_project` is reached only from a resolved `ProjectBinding`, and `publish_snapshot`, `query`, and `diagnostics` require the resulting `BoundProject` witness. No production external-TS result path may infer a project from a bare path, open a carrier into a config-less/inferred project, or fall back to an inferred backend. Path-shaped transport notifications may exist below this contract, but they cannot construct external-TS results or bypass `BoundProject`.

Ownership is TypeScript-correct. A carrier source (`.vue`, `.svelte`, or any adapter extension) is owned by a configured project only through the default include, a no-extension directory/bare-star glob, or a glob/`files` entry that explicitly covers that extension. An extension-specific `*.ts` glob does not own it. TypeScript include has no brace expansion: multi-extension coverage is separate entries, never `*.{vue,svelte}`.

`NoProject` and `Ambiguous` produce no production external-TS result; Verter-native non-external-TS features may still answer. `SyntheticScratch` is a separate, explicitly labelled scratch lane for non-cross-file features only. It never supplies configured-project semantics, batch typecheck, cross-file results, or project-cache warming.

Generated companion names are descriptor-owned and live in the user namespace. They are collision-free against different adapter source extensions in the normal case, but not resolution-unambiguous or reserved. A real user file at the exact `{name}.vue.tsx` / `{name}.svelte.tsx` companion path, or a same-stem Svelte rune module beside a component, is a detected resolution conflict: Verter marks the source ambiguous and fails closed, never overlay-shadows a real user file and never surfaces a silently wrong edge.

This rule becomes live for a backend only when that backend's real project-bound path lands; the inferred fallback for that backend is deleted in the same change.

Guards: `provider_op_requires_resolved_project`, `carrier_ownership_extension_rules`, `carrier_never_shadows_real_user_file`, `same_stem_svelte_component_rune_fails_closed`, `no_fallback_to_inferred_anywhere`.

See the `/host-session` skill for the contract's three-layer structure (`ProjectResolver`/`CarrierRegistry`/`EngineBackend`), the `BoundProject` witness type-state, and the carrier-publish path.

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

- Corpus audit-test regenerator (run after audit-record schema or fixture changes; idempotent): `node scripts/gen-corpus-audit-tests.mjs`

For TypeScript changes, also run `pnpm test`. Do not skip workspace-wide testing even for "small" changes.

**Agent test policy:** the canonical pair above — `cargo nextest run --workspace` (completeness) plus `cargo test -p verter_session --tests` (shared-process surface) — is the default Rust gate (the silent-skip trap is stated once in Running Tests above). Do not run bare `cargo test --workspace` (no `--tests`) by default: it pulls in doctests and example builds without improving the normal verification loop. Run doctests (`cargo test --workspace --doc`) only when rustdoc examples changed or the user explicitly asks.

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

**Architecture guards for critical rules**: every new `CRITICAL` architecture rule lands with a static architecture guard or a discriminating regression test in the same change; if a guard cannot be automated yet, the rule text names the planned guard/test and the gap is tracked in the owning skill/doc. The R6 meta-guard at `crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs` (`every_critical_rule_in_docs_has_registered_guard`) walks `CLAUDE.md` plus every `.claude/skills/*/SKILL.md` and asserts every `(CRITICAL)` heading has a `CRITICAL_RULE_GUARDS` registry row with at least one named guard — a prose-only `(CRITICAL)` section fails the gate.

**Rust test file organization**: When inline `#[cfg(test)]` exceeds ~400 lines, extract to a sibling `*_tests.rs` file.

### Testing-Hermeticity (MANDATORY)

Unit tests must only depend on locally-vendored fixtures. They must compile and run without any third-party repository (e.g., `nuxt-ui`, `element-plus`) checked out alongside this repository. Tests that need external corpora must be feature-gated (e.g., `#[cfg(feature = "external-corpus")]`) and excluded from the default canonical run (`cargo nextest run --workspace` + `cargo test -p verter_session --tests`).

A test that references `.integration-tests/repos/<third-party>/...` from a non-gated test file is a violation. The architecture guard `external_corpus_paths_not_present_outside_gated_tests` enforces this.

### No phase archaeology in production code (MANDATORY)

Source comments must not reference plan phases (`phase 5d`, `phase 11`, `post-cutover`, `pre-Phase`), cutover stages (`d-cutover`, `cutover`), deletion history (`deleted in 5g`, `retired in`), or any project-management vocabulary. Once a plan is over, the code reads as final-state.

Durable architecture insights belong in `.claude/skills/*` or `docs/arch/`, not in source comments. Test files named after retired phases must be renamed to describe the invariant they characterize, not the phase that produced them.

The architecture guard `no_phase_archaeology_in_production_code` enforces this on `crates/*/src/**`.

See `/testing` skill for full TS/Rust test patterns, sourcemap testing, and server cleanup.

### VS Code Extension Testing (MANDATORY)

Changes to the VS Code extension or the LSP server MUST be verified with automated tests, NOT manual testing. Unit tests (Vitest) for pure logic, E2E tests (Mocha) for LSP integration features.

See `/testing` and `/e2e-vscode-testing` skills for commands, fixture design, and helpers API.

## Agent Implementation Rules

### Codebase Navigation

Use semantic code-navigation tools (Serena or equivalent MCP: symbol overviews, symbol/reference lookup, rename/refactor ops) before broad source reads. Read full source files only when symbolic context is insufficient or the file is small enough that a full read is clearly the most direct path.

### Planning

Prefer architecturally correct, long-term solutions; evaluate by correctness and durability, not implementation speed. Time constraints, implementation size, migration breadth, anticipated breaking changes, or "a lot of work" are not valid reasons to weaken the design, preserve a compromised path, or diverge from the approved plan — if the correct implementation is larger or breaking, plan for it explicitly or raise it before execution; never silently ship an architectural deviation. Do not provide time estimates unless explicitly asked, and never use estimated effort/duration/perceived time cost as a factor for doing, not doing, or partially doing planned work.

Plans must include these sections:
1. **Context** — why this change is being made
2. **Changes** — specific files to modify with concrete modifications
3. **Legacy Deletions** — explicit list of files, functions, code paths, feature flags to remove
4. **Verification** — full workspace test commands and expected outcomes

Without explicit legacy deletion lists, agents skip deletions and leave dual paths alive.

### Execution

Execute approved plans fully in one pass, end-to-end, without intermediate checkpoints or mid-plan confirmation on already-approved steps. Do not pause, defer scope, leave planned work unfinished, or rewrite the plan into a smaller/safer variant because the correct path is breaking, broad, or labor-intensive. Approved plans land as written unless the user explicitly re-scopes them.

### Orchestrating Large Plans

For a large multi-block plan, refactor, migration, or staged cutover executed autonomously, drive it via the `/multi-agent-orchestration` skill rather than improvising: a pure orchestrator delegates blocks to implementer/reviewer/fix sub-agents, gates each on dual review (independent reviewer + `codex`), runs fix cycles until clean, and verifies sub-agent reports against git state (trust but verify).

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

Concrete anti-patterns, all forbidden on landed/mainline commits:

- **Empty `#[test]` bodies** — `#[test] fn verifies_cycle_guard_terminates_on_recursion() {}` passes trivially and falsely advertises coverage (worse than `#[ignore]`; keep `#[ignore]` until the body can be written).
- **Unconditional "unknown"/"default" returns as "scaffolding"** — `fn relate_nodes(...) -> RelationResult::Unknown` always-Unknown is a nop, not a scaffold; same for an always-`Opaque(Miss)` resolve. Write real logic, or use `todo!()` / `unimplemented!()` so the nop fails loudly.
- **"Real body deferred to follow-up commit"** — a stub satisfying a gate now with a later commit planned is a gate-bypass; the gate reflects the tree under review, not future intent.
- **Always-true assertions** — `assert!(true)`, `assert_eq!(1, 1)`, `assert!(result.is_ok() || true)`: any predicate that holds regardless of the code under test.
- **Non-discriminating characterization tests** — a characterization test must FAIL against the pre-change codebase AND PASS against the post-change codebase; otherwise it characterizes nothing.

**Rule of thumb:** for every committed assertion ask "would this test catch the bug the change was written to fix?" — if no, it is a stub.

**WIP exemption:** scratch branches that will be squashed (e.g. `staging/*` → squash-merge) may contain `todo!()` bodies, empty tests, placeholder returns. The rule applies to the squashed/landed commit, any PR branch, and any gate evaluated on the final tree; a landed commit message citing "stub satisfies gate mechanically" is a self-identified gate-bypass.

**Self-review obligation:** before concluding a step that un-ignores or adds tests, re-open each test file and verify bodies are non-empty and assertions discriminating; before concluding a step that implements a function, verify the body exercises its inputs rather than returning a constant.

Guards: `macro_impacting_constructs_fail_lowering_not_silent_skip`, `every_consumer_has_production_call_site`, `every_registry_entry_lists_at_least_one_guard`.

### Agent Feedback Capture

Agents MUST continuously log feedback to a per-conversation file at `.feedback/feedback-{YYYY-MM-DD}-{short-id}.md` (`.feedback/` is gitignored). One feedback file per conversation session; when delegating to subagents, pass the file path and instruct them to append.

Categories: `[issue]` (bugs, unexpected behavior, workarounds), `[improvement]` (code quality, performance, architecture ideas), `[debt]` (works but could be better), `[docs]` (missing/outdated documentation).

Format: `- [{category}] \`{file_path}\` — Brief description`

## Dependencies Policy

**Repo-owned toolchain is Rust + JS/Node only — no committed Python.** Repo-owned gate, build, CI, test,
code-generation, packaging, and release tooling is implemented as Rust bins or JS/Node scripts; Python is
not a committed implementation language for those paths.

- No tracked repo-owned `.py` file (outside third-party / non-toolchain trees `node_modules`,
  `.integration-tests`, `vendored`/`vendor`, `.claude`, `target`).
- No `python`/`python3`/`py -3` command invocation in `package.json`, `.github/workflows/*`, or tracked
  repo-owned command wrappers (`*.sh`/`*.bash`/`*.ps1`/`*.cmd`/`*.bat`). Thin shell/PowerShell/cmd wrappers
  are allowed as command-entry shims but must not invoke Python; Node/TS tool scripts must not spawn Python
  transitively.
- New or ported repo-owned tooling lands as a Rust bin (e.g. the `gen-typeinfo-manifest` cargo bin, the
  xtask `check-four-mode-terminology` bin) or a Node script — never a committed Python script.
- Agents may use Python transiently and locally for ad-hoc analysis, but such use is never committed and
  never on a gate/build/CI/test path.
- Committing repo-owned Python is allowed only if it is 100% necessary AND neither Rust nor JS/Node can do
  it, adopted via an architecture-reviewed change to this policy with a narrow documented justification.
  Until then, do not add Python.

- Keep dependencies at their latest versions
- Rust deps: update in `Cargo.toml`, run `cargo update`
- JS deps: `pnpm up -r -i -L` to interactively update all
- `workspace:^` deps are rewritten by `pnpm publish` automatically

## Commit Convention

This project uses **conventional commits** (`<type>(<scope>): <description>`) for automatic changelog generation via [git-cliff](https://git-cliff.org/).

Types: `feat` (new feature), `fix` (bug fix), `perf` (performance), `refactor` (no behavior change), `docs`, `test`, `chore` (build/CI/tooling), `release` (version bump).

Scopes: `core` (verter_compiler), `napi` (verter_napi / @verter/native), `wasm` (verter_wasm / @verter/wasm), `play` (playground), `unplugin` (@verter/unplugin), `lsp` (language-server), `types` (@verter/types), `ts` (@verter/core TypeScript), `meta` (@verter/component-meta), `ci` (CI/CD workflows), `*` (multiple areas).

Example: `feat(core): add v-memo directive support`

## CI/CD

See [docs/contributing/ci-cd.md](docs/contributing/ci-cd.md) for CI/CD documentation: workflow specifications (CI, nightly, release), pre-release versioning flow (alpha → beta → rc → stable), publishing (npm + crates.io), nightly WASM builds + playground deployment, required GitHub secrets configuration.

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
| `/framework-adapters`    | Framework-adapter substrate: registry, descriptor + virtual-file naming column, facts/carrier-only ctx, framework-surface executor, two-pass script-fact seam, Vue as the reference adapter |
| `/position-encoding`     | Span types, position encoding, coordinate conversions, path normalization                        |
| `/build-and-profiling`   | Build order, rebuild sequences, profiling, MCP server setup                                      |
| `/testing`               | Test patterns, TDD workflow, sourcemap testing, server cleanup                                   |
| `/e2e-vscode-testing`    | VS Code E2E test fixtures, helpers API, adding new tests                                         |
| `/wsl-e2e-testing`       | WSL E2E tests to reproduce Linux/CI failures, fixture matrix                                     |
| `/rust-performance`      | Rust optimization patterns, allocation hierarchy, CodeTransform API                              |
| `/multi-agent-orchestration` | Driving a large multi-block plan, refactor, migration, or staged cutover autonomously: pure orchestrator + implementer/reviewer/fix sub-agents, dual review (independent + codex), per-block fix cycles, trust-but-verify |
| `/scheduler`             | Scheduler submission/admission APIs (`submit_request`/`submit_batch`/`submit_batch_atomic`), CPU vs I/O pool routing, host CPU-pool coordination |
| `/debug-tooling`         | Hangs, unexpectedly slow paths, stack snapshots: backtrace watchdog, LLDB attach wrapper, release-dbg profile |
| `/agent-prompts`         | Generating implementation/continuation/review/fix prompts for driving separate agent sessions |
