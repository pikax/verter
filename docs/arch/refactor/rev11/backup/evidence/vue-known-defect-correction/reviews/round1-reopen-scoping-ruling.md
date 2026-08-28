Reading additional input from stdin...
OpenAI Codex v0.147.0
--------
workdir: <repo-root>
model: gpt-5.6-sol
provider: openai
approval: never
sandbox: read-only
reasoning effort: xhigh
reasoning summaries: none
session id: 019ffc3d-aea8-7bb3-901a-1510a4c11bf9
--------
user
You are advising the program orchestrator for the Verter Revision 11 architecture-lock program on how to scope a reopen. Read docs/arch/refactor/rev11/charters/BV0.md (the charter) and the three round-1 review reports at docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-{conformance,architecture,adversarial}.md — all three independently returned VERDICT: BLOCKING against candidate commit c40a1ca96 (base b64358705).

Consolidated findings across the three reviews:
1. [most severe — adversarial] The conformance harness's invoke-vue-oracle.mjs never passes vapor/ssr to compileScript, so ALL harness-generated Vapor goldens were compiled as non-Vapor by the official compiler (proven: {vapor:true} vs {} produce different __vapor markers on the real pinned rc.3 compiler). BV0's candidate then changed Verter's OWN production code (script/process.rs, 3 sites) to match those wrong goldens — removing the __vapor:true marker from real non-SSR Vapor builds. This is load-bearing at runtime (isVaporComponent gates VDOM<->Vapor interop). A new unit test locks the wrong behavior in. The seed matrix cannot detect it because candidate and golden are wrong identically (two bugs cancelling out).
2. [architecture+conformance, B1] Two previously-passing VDOM cells (components/dynamic-multi-root, elements-text/multi-root, both non-inline) regressed to an import/declaration ordering divergence and were absorbed into known-divergences.json under a note that describes a 'user import' neither fixture has (both are script-less). Charter explicitly forbids substituting a waiver for unaffected-route regressions.
3. [adversarial] docs/arch/ssr-noninline-shape-divergence.md was edited by this commit to still assert the pre-fix SSR behavior (no __isScriptSetup marker, _ctx.* routing) even though the same commit fixed exactly that and the doc's own stated deletion-exit-criterion was met. Should have been deleted, was instead left contradicting the shipped code.
4. [adversarial] Undisclosed ~85% growth in tracked Vue conformance divergences attributable to BV0's own code (isolated from the sanctioned rc.1->rc.3 oracle re-pin, which the conformance reviewer separately confirmed only accounts for ~15 of the growth). Concentrated in Vapor, semantic not cosmetic: missing withModifiers/withVaporModifiers routing, interpolated text folded into static template markup instead of emitting txt/setText/renderEffect, several component/prop-binding cells worse.
5. [architecture, B2/conformance B2] official_seed_matrix.rs and check-candidate.mjs silently SKIP the link/runtime axes on any unprovisioned checkout (the default state of a fresh clone or CI runner) and the skip is non-failing — so '36/36' passes without the charter-required exact-package-link and deterministic-runtime axes ever executing, unless someone manually provisions the oracle out-of-band first.
6. [architecture, B2 non-blocking per conformance's N2] The same commit also bundles an out-of-charter Svelte oracle migration (5.56.3->5.56.8, ~2600 files) into a charter scoped to 'Immediate Vue known-defect correction'. Mechanically verified byte-identical/version-string-only by the conformance reviewer, but rides in a Vue-only charter one commit after a prior amendment explicitly split Svelte work into a separate block (BF3).

I need a ruling on SCOPE for the mandatory comprehensive reopen fix (per this program's standing rule: a reopened block's fix must address the whole class of gap, not narrow per-item patches, to avoid a further reopen):

Q1: Is fixing item 1 (the harness's invoke-vue-oracle.mjs compileScript call, and regenerating ALL Vapor goldens across the corpus, not just BV0's 36 seed cells) within BV0's charter authority, or does it require touching shared harness infrastructure that needs separate authorization (the harness is also used/depended on by BF1/BF2-already-accepted/BV1/BF3)? If BF1/BF2 already-accepted work depended on the wrong goldens, does fixing the oracle invocation retroactively invalidate anything already ACCEPTED, and if so what's the correct process?
Q2: Should item 6 (the Svelte migration) be split out of BV0's landing into a separate commit/block, or is it acceptable to keep bundled given it's mechanically verified inert?
Q3: Confirm items 2-5 are squarely in-charter for BV0 to fix directly (my own read says yes, but confirm).
Q4: Any other scope risk you see in going from these findings to a single comprehensive fix-and-relanding cycle for BV0.

Give a clear, decisive ruling for each question, citing the charter/evidence text where relevant. This is architecture guidance, not a request for you to write code.
codex
I’m using the project’s architecture guidance because this is a charter-boundary and ownership ruling. I’ll read the canonical architecture rules plus the BV0 charter and all three review reports, then map each ruling to the exact local text.
exec
/bin/zsh -lc "wc -l CLAUDE.md .claude/skills/architecture/SKILL.md docs/arch/refactor/rev11/charters/BV0.md docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md && sed -n '1,240p' CLAUDE.md && sed -n '241,480p' CLAUDE.md && sed -n '481,720p' CLAUDE.md" in <repo-root>
 succeeded in 0ms:
     659 CLAUDE.md
     230 .claude/skills/architecture/SKILL.md
      61 docs/arch/refactor/rev11/charters/BV0.md
     298 docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md
     183 docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md
     303 docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md
    1734 total
# Verter

> **SUPERSEDED WHERE IT CONFLICTS — an architecture program is in flight.**
>
> The ratified Revision 11 architecture program lives at [`docs/arch/refactor/rev11/`](docs/arch/refactor/rev11/); its normative entry point is [`ORCHESTRATOR.md`](docs/arch/refactor/rev11/ORCHESTRATOR.md).
>
> **Precedence:** where this file and the Revision 11 plan disagree, **the plan wins**. The plan was designed to replace parts of this document, and the maintainer has ratified that precedence — see ruling R-3 in [`evidence/maintainer-rulings.md`](docs/arch/refactor/rev11/evidence/maintainer-rulings.md), which quotes both sides of the known conflicts.
>
> **How to read this file while the program runs:** treat it as an accurate description of how the code behaves **today**, and as authority for day-to-day operational matters — build and test commands, commit conventions, testing requirements, review discipline. Do **not** treat its architecture sections as authority over the program's target design; several describe structures the plan intends to change or remove. A rule here is not grounds to refuse a change the plan mandates.
>
> **If you are implementing a program block:** the plan's charters, contracts and ownership tables bind you. Follow them to the letter. Where implementing the plan appears impossible, record a deviation for maintainer ratification rather than substituting a local decision — an unrecorded deviation is far more expensive to unwind than a delay.
>
> This banner is removed when the program completes and the target architecture is described here directly.

Verter = a Vue compiler + Language Server Protocol (LSP) implementation. Converts Vue Single File Components (SFCs) to valid TSX (TypeScript type-checks them) and compiles templates to optimized render functions. Unlike Volar, Verter generates real valid TSX, not virtual files.

Hybrid Rust + TypeScript monorepo: Rust crates own carrier parsing, runtime and IDE code generation, the shared semantic session, and the LSP server (`verter_lsp` binary, stdio); TypeScript packages provide editor integration, TypeScript-provider adapters, protocol bindings, and bundler orchestration.

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

Guards: `verter_audit_no_upward_deps`, `audit_substrate_isolation`, `audit_observer_single_accessor`. The former single-engine `no_new_*` shrinking-ledger scanners retired with the deleted parser-owned `type_surface` engine; the single-engine invariant is now held structurally (the compiler consumes only the dependency-neutral macro DTO) plus the Vue macro boundary suite registered under "Vue Macro Semantic Boundary" in `CRITICAL_RULE_GUARDS`.

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
- The declaration-body **hot READ path is handle-native at the one migrated graph-backed site** (`lower_decl_body_to_node`) through `decl_body_hot_ref`; prepared declarations and `LoweredValueDecl` are fact+locator `NoTypeExpr`, and `LoweredTypeDecl.body` is content-free. The two former stored type-parameter `TypeExpr` pockets are CLOSED (the type-parameter-bound confinement block): `LoweredTypeDecl` is wholly `NoTypeExpr` — the stored full `type_parameters: Vec<TypeParam>` is gone; the `narrow_type_parameters` mirror (name + ordinal + content-free bound locators) is the sole stored authority, consumed by the prepared-decl builder and the external frontier (which content-free re-anchors the mirror's bound slots to the frontier symbol, preserving `export default`), while the locator/binder deref uses the mirror for ordinal/name/bound-presence authority and re-borrows bound CONTENT + the full sibling frame lease-only from the retained snapshot (`transient_type_parts`). `TypeParamBinding` is likewise shrunk to the content-free `(name, ordinal)` fact pair (`NoTypeExpr`): its `<script setup generic="…">` bounds are re-borrowed at query time through ONE artifact-local transient producer over the pinned `IndexedReady` and lowered by ONE dispatch helper shared by both content readers, with a missing/stale re-borrow failing as a typed cache-suppressed miss, never a bound-free fabricated binder. The residual-reader ledger is not a terminal-zero mechanism: `ProducerLowering` is permanent transient ingress, the five `GraphFreeDto` rows remain named until the separate producer-boundary-confinement cutover, and a hot consumer must never take `HotTypeRef → TypeExpr → semantic decision`.
- Navigation stays narrower than expansion: walking `A['c']['full']['bar']` navigates intermediate hops and expands only the terminal requested projection unless limited normalization is required to continue.
- Generic substitutions are semantic meaning: navigation/expansion operate on instantiated types; cache keys include the relevant substitutions/type arguments.
- Navigators stay non-owning (choose the next hop, non-owning normalization only); reusable semantic work enters through the shared query API, not a private drill-down path. The shared semantic layer is keyed by semantic query identity and stores immutable semantic data or ids — never borrowed AST pointers or retained parser arenas.
- Completion fence: top-level live-host results record touched dependency facts, revalidate before publish, retry at most 3 times on mid-flight changes; never warm shared caches with torn provisional results; cancelled, superseded, interrupted, budget-exceeded, or partial results are never promoted warm.
- Waiters on in-flight work block cooperatively, never busy-spin; same-path recursion never self-awaits.
- Cache population is path-independent (same result from different entry points → same shared entry); broader successful results may backfill only the narrower entries they actually satisfied; narrower results must not pretend broader work is cached.
- Final payload caches hand out immutable `Arc` values; any backend preserving concurrency, size bounds, validation semantics is fine.

Guards: `audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries`, `macro_impacting_constructs_fail_lowering_not_silent_skip`, `indexed_ready_publish_lowers_zero_decl_bodies`, `resolve_unrelated_symbol_lowers_only_demanded_decl`, `lazy_decl_body_singleflight_lowers_once`, `no_indexed_ready_eval_env_or_type_decl_body_storage`, `emit_parse_facts_never_hashes_decl_bodies`.

**Project-global cache (final state):** `VerterHost` owns a single `ProjectTypeStore` accessed via `.project_type_store()` — the sole shared cache graph: `FileArtifactStore`, `AnalysisReadyDb`, the rehomed `RouteDb`, `OwnerImportSurfaceDb`, `ComponentMetaResultDb<ComponentMetaAnalysis>`, `MaterializeStructureDb`, `SemanticGraphStore` (which also owns the Vue macro resolution artifacts — the former `ResolvedNamedTypesDb`), `ShapeCacheDb`, `FlowSliceStores` (the flow-return substrate's per-function graph/slice artifact stores), and the `IntrinsicRegistry`. `IndexedReady` is the single canonical post-parse artifact (the former `ModuleFactsDb` is retired). Validated cache writes record a `ReadSetSignature.facts` fact signature (the path-precise fact-tracer observation set) — the sole cache-validity rail, revalidated against the live `StoreView` on every warm hit. The `StoreViewValidationToken` is the complete reuse/validity oracle; the singleflight LANE identity is the narrower `external_supersession_fingerprint` (reuse-oracle = full token; lane-identity = external fingerprint). See `/host-session` (store-view token dimensions, token-advance rules, lane identity, singleflight, `RequestStoreView`/`CanonicalCompletionOverlay`, handle-backed dims), `/component-meta` (`get_component_meta` final-result flow, `resolve_owner_direct_import`, `materialize_component_meta_structure`, the `ShapeCacheDb` per-member route, `reduce_published_field_types` / sink-private `reduce_field_value_node`), `/type-cache-architecture` (admission, retired split stores), and `/type-resolution` (`execute_cooperative` dedup, `SemanticNodeData::VueMacroElements` hot path, `IntrinsicRegistry::lookup`).

### Canonical Dependency Cache Rule (CRITICAL)

Host-backed type/import resolution treats the canonical file ID as the cache identity. Load and parse each dependency at most once per canonical ID per workspace content generation. Cache the parsed state, the shallow declaration index plus lazy declaration-body memo, symbol/export tables, and prepared declarations together. Later lookups hit cached maps — never rewalk the AST. VFS is the authority for file-change invalidation. Concurrent cold requests to the same file collapse onto one materialization path. Changes land as one clean cutover, no dual-path shims.

Guards: `host_upsert_performs_no_reverse_dependent_eviction`, `host_upsert_reverse_dep_eviction_scanner_discriminates`, `import_route_writer_guard`.

See `/type-resolution` skill for the full rule set (invalidation semantics, route caches, prepared declarations, cross-owner reuse, negative caching, the concrete performance contract).

### Cache Architecture (CRITICAL)

The fact-based cache architecture splits cache keys across five orthogonal env-hash dimensions (`parse_env_hash`, `resolve_env_hash`, `type_env_hash`, `lib_env_hash`, `project_identity`). Each cache layer keys only on the dimensions it actually depends on (R21 scoping rule — a single bundled `project_config_hash` is forbidden). `lib_env_hash` enters a key only when the value depends on lib data: `ResolvedImportFacts` does NOT include it; `RouteDb`, typed-IR resolve, `MaterializeStructureDb`, `SemanticGraphStore`, `ComponentMetaResultDb` DO.

Two cache families: **content-addressed artifact caches** (`FileArtifactStore`, `ResolvedImportFacts`, typed-IR resolve, `MemberSemanticFactStore`, `MemberDisplayFactStore`, `ModuleAugmentationIndex`) carry `content_hash` or `parse_stable_hash` in the key; **query-identity caches** (`RouteDb`, `MaterializeStructureDb`, `SemanticGraphStore` query nodes, `ComponentMetaResultDb`) exclude version hashes from the key — concurrent variants coexist as candidates in one slot, with version rooting on the cached value (the structural + semantic-graph caches — `MaterializeStructureDb`, `SemanticGraphStore` memo, `ShapeCacheDb` — root via `ReadSetSignature.facts` + `self_root_canonicals`; `RouteDb` via its value-side `ValidatedFactCache` fact signature; `ComponentMetaResultDb` via the owner whole-hash candidate discriminant + `ReadSetSignature.facts`). Cache keys never include `fact_dep_signature`. The `MaterializeStructureDb` subject is the content-free `MaterializationCacheKey` (a `ResolvedDeclSlotIdentity` slot + projection/policy/mode axes + `resolve_env_hash`), NOT a graph-instance `SemanticNodeId` — the per-thread recursion identity `MaterializeRuntimeKey` is a separate, non-cache key; a root-less anonymous subject keys no slot (uncached). The materialization cycle gate is the sealed `SemanticQueryKey::ClassifyMaterializationCycleGate` family (`MaterializationCycleGateKey`: `ResolvedDeclSlotIdentity` root slot + `parse_env_hash` + `resolve_env_hash`, fixed `StructuralTransit`/`Skeleton`/empty-args axes) — the SOLE authority for "does this declaration transitively reach a cycle through a complex helper surface"; only its `Decided` outcome admits (`LegacyFallback` always suppresses), and consumers branch on the carried `Stop | Continue` verdict from both arms.

Family-memo slots (`SemanticQueryKey::Instantiate.base` / `ResolveMacroPayload.owner`, mirrored on `FamilyKey`) are the env-bearing, content-free `ResolvedDeclSlotIdentity` (R6 — content/version hashes and the versioned `DeclIdentity` are forbidden in any derived-`Hash` query-identity key; the live whole-hash is re-sourced at value-compute time, never carried in the key). A warm hit requires TWO independent gates (§3.4): `cached_satisfies` over a RECORDED materialised `(path, point)` the candidate's compute actually produced — never the candidate's nominal slot/mode, never enum rank — AND per-candidate `ReadSetSignature.validate_with_self_roots` against the caller's live view. Backfill clones only recorded materialised points, directionally gated (the `Shallow → Navigate` clone is lattice-unsound). `validated_at_generation` is recency metadata only, never a validity oracle. Family-slot retention is bounded PER FAMILY (`U3.ADAPTIVE_FAMILY_RETENTION`): each `FamilyKey` declares an exhaustive `candidate_cap()` (floor 4; the inference/substitution-heavy live families `Instantiate` / `TypeOf` / `Conditional` / `MappedType` hold 8); at the cap a publish evicts a candidate INVALID against the publishing caller's stable store view FIRST (snapshot/validate/reacquire OUTSIDE the `entries` mutex, `admission_seq` identity recheck under it), then the least-recently validated-hit candidate (a valid hit promotes to the back); a new cacheable candidate is ALWAYS admitted after local eviction, and same-discriminant re-publish replaces in place as freshest. The process-wide candidate-memory ceiling + typed non-admission are deferred full-`U3.CACHE_FACT_MODEL` work, NOT live. See `/type-cache-architecture` for the full key/context composition (`InstantiateContext`/`MacroPayloadContext` per-key contexts, per-family `candidate_cap()` candidate semantics, non-file-base rooting).

`FileArtifactStore` is the authoritative per-file storage layer, keyed by `(canonical, content_hash, parse_env_hash, parser_version, file_language_id)` — `file_language_id` is the file's `FileLanguage` row (the per-file classification dimension of artifact identity, so a framework-capability flip misses exactly the affected files' artifact slots without touching the global `parse_env_hash`). The overlay-aware `augmentation_index` (module-augmentation inverse lookup) lives on the same store. See `/type-cache-architecture` for the full key composition, `file_language_id` producer wiring, `AugmentationTargetKey`/`AugmentationPopulation` semantics, and the `parse_stable_hash` definition.

Cache runtime hard rules — three always in force: cache correctness is read-side authoritative; `ReturnOnly` (overflow, budget exhaustion, cancellation, generation supersession, incomplete self-rooting, unresolved provenance) never publishes entries, reverse-index metadata, or persistent artifacts; overlay/session results never populate base-only or persistent caches. Full 20-rule list: `/type-cache-architecture` → Cache Runtime Hard Rules.

Guards: `cache_satisfaction_is_materialized_point_not_nominal_demand`, `cache_satisfaction_requires_path_exact_not_prefix`, `backfill_writes_only_recorded_materialized_points`, `no_off_store_host_caches`, the `r6_*` cluster, plus the four migrated-query-identity-key guards in `tests/cases/g_cache/r6_r21_query_identity_keys.rs` (`component_meta_result_key_*`, `route_name_key_*`/`barrel_surface_key_*`, `materialization_cycle_gate_key_*`, `materialization_cache_key_*`) — full list in `CRITICAL_RULE_GUARDS`.

See `/type-cache-architecture` skill for the full rule set (R1–R31, two-fact `MemberPresence`/`Member` model, multi-candidate substrate, signature-overflow contract, module augmentation completeness, heuristic-cache-semantics prevention, exact policy identity) and `docs/arch/fact-based-cache.md` for the per-field audit table + per-cache-layer key composition.

### Macro Type Traversal Rule (CRITICAL)

When resolving cross-file macro types (`defineProps<T>()`, `defineEmits<T>()`, component-meta deep expansion, etc.), only follow the import graph reachable from the requested type's declaration graph. There is one shared cross-file type resolver with five query modes: `Identity`, `Navigate`, `Shallow`, `Expanded`, `Skeleton` (see `/type-resolution` → Query Mode Contract).

**Macro resolution is one shared path, not a per-macro engine.** Every macro (`defineProps` / `defineEmits` / `defineOptions` / `defineSlots` / `withDefaults`) and every imported `.vue` component surface resolves through exactly TWO steps: (1) resolve ONE type via the shared typed-IR five-mode dispatch — the generic-parameter type (`define*<T>()`) OR the object-argument type (`define*({ ... })`); `withDefaults` resolves the props payload type plus the defaults-object type and merges; `.vue`-component imports resolve the synthesized `$props` / `$emit` / `$slots` / expose surface recursively through the same dispatch (the hardest case — apply EXTRA caution: it is exactly where rule violations cause the worst hangs); no macro-specific engine, no per-surface walker, no eager element resolver. (2) Normalise per kind — a thin transform, NOT a resolver (per-kind field rules: `/type-resolution` → Macro Type Traversal Rule). A macro/import that resolves through anything else, or flattens a full surface eagerly before the consumer demands it, is a rule violation — collapse it into `shared_resolve(type) + normalise`.

`Skeleton` is the BFS / generic-helper traversal mode: unbound type parameters stay `TypeParam` shells so Conditional branches do not collapse to `never`. Path projection is path-precise: intermediate hops run in `Navigate`, the terminal hop runs in the caller's mode; non-contributing intersection arms are ignored (not rewritten to `never`); open conditionals distribute the remaining path into both branches; closed conditionals reduce immediately. Do not walk unrelated imports. Do not treat plain imports as implicit exports. Cache discovered symbol mappings and barrel hops.

**TS-first resolution priority:** TypeScript types always take priority over JavaScript files. Use `effective_target()`: `.d.ts` > `.d.cts` > `.d.mts` > `.ts` > `.tsx` > `.js` > `.jsx` > `.cjs` > `.mjs`.

**Owned resolution is bounded by `workspace_root`:** `node_modules` and package `#imports` ancestor walks stop at `IdeProjectConfig.workspace_root`.

Guards: `root_conditional_still_distributes`, `no_macro_string_heuristics_in_resolver_core`, `no_text_based_macro_surface_projection_helpers`, `no_role_inference_from_name_suffix`, `no_pick_or_omit_string_prefix_check` — full list in `CRITICAL_RULE_GUARDS` (the retired `no_new_*` shrinking-ledger scanners left with the deleted `type_surface` engine; the compile-boundary half of the invariant is guarded under "Vue Macro Semantic Boundary").

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

Facts rail: the cold stitch observes one `FactKey::ModuleAugmentationIndexShape` fingerprint plus one `FileWholeHash` per contributing file and records `self_root_canonicals = {base} ∪ {augmenters}` — a content edit to ANY contributor misses the warm read; torn/partial routes through `ReturnOnly`. Query keys stay content-free (R6). The index is OVERLAY-AWARE (`AugmentationPopulation {Base, Session(overlay-set fingerprint)}`): overlay augmenters NEVER poison the base index and NEVER cross sessions, and there is NO base-only session assert on the augmentation-index surface — a session view is accepted under `Session` scope.

Guards: `session_overlay_augmenter_isolated_from_base_index`, `session_overlay_augmentation_isolated_from_base_meta`.

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

### Compiled-Output Conformance (CRITICAL)

Official-framework compiler conformance is behavioral plus structural/helper-topology parity, not raw-byte identity. For Vue VDOM/Vapor, Svelte `svelte/internal/*`, SSR/client, and future runtime backends, compare emitted output by observable behavior plus parsed/token-normalized structure: imports, helper families, helper call sequence where order is semantic, memoization/reactivity/effect topology, DOM/hydration template topology, class/style/attribute normalization, prop/property routing, event delegation, and diagnostic/reject ordering.

Cosmetic JS carrier formatting is not a finding: indentation, line breaks, non-semantic comments, intra-expression whitespace outside literals, and behavior-preserving redundant parentheses may differ from the official compiler. Directive, pragma, license/preserve, source-map/sourceURL, TS-directive, JSDoc, and other tool-consumed or framework-significant comments remain in contract. Generated local identifier spellings are waived only when the backend oracle implements scope-aware alpha-equivalence for private, non-observable bindings; otherwise identifiers are structural. Literal payload bytes, static HTML/CSS/SSR strings, public/exported or source-authored names, sourcemap mappings, diagnostic text/codes/order, and any framework-defined observable format remain in contract.

Do not build or route production compiled-output emission through JS printers, re-printers, redundant-paren canonicalizers, or any machinery whose role includes mimicking the official compiler's cosmetic JS carrier formatting. Direct-emission helpers may emit syntax-required tokens, including required parentheses for valid JavaScript expression/statement shape, but they must be scoped to semantic/syntactic correctness and covered by behavioral/structural tests rather than official cosmetic byte parity. Emit correct code directly and make conformance oracles structural for cosmetic categories: a cosmetic-only diff passes; a behavioral or structural divergence fails.

The positive structural-discriminator guard currently covers Svelte client only (Vue VDOM/Vapor and SSR/client positive oracles are tracked follow-ups); the re-printer guard is cross-backend negative coverage. See `/compiler-codegen` for the tracked guard gap.

Guards: `svelte_structural_conformance_discriminates_cosmetic_from_behavioral_diffs`, `no_compiled_output_cosmetic_reprinter_path`.

### Fallthrough / Root Inheritance (CRITICAL)

The shared Rust pipeline owns all fallthrough and root inheritance semantics. `verter_semantic::analysis` extracts root reachability facts only. `verter_session` owns the single inheritance resolver, recursion, conditional branch composition, generic propagation, caching, and final metadata projection.

Key rules: `inheritAttrs: false` → no inherited surface. Single native root → intrinsic attrs minus declared props/events. Single component root → recursive propagation. Conditional branches → exact union. Cycles → unresolved branches. `class`/`style` are never consumed.

Guards: `fallthrough_recomputes_from_runtime_subnodes_after_top_level_node_clear`, `fallthrough_runtime_reuse_survives_host_cache_clear`, `fallthrough_reuses_root_follow_after_branch_union_node_clear`.

See `/component-meta` skill for the full semantic rules, public contract, authority chain, and key files.

### Component-Meta Shallow-By-Default Rule (CRITICAL)

Types and properties are ALWAYS published shallow at the projector surface UNLESS the consumer explicitly walks the path. This is the single architectural invariant the projector pipeline (`meta_resolve::projectors::reduce_published_field_types` + sink-private `reduce_field_value_node`) enforces.

Concrete contract:

- Plain alias references (`type Foo = ...`) — published prop type stays `TypeExpr::Ref { name: "Foo" }`. Consumers re-resolve `Foo` through the registry on demand. The projector does NOT eagerly inline the alias body.
- `Pick<Foo, "bar">` — materialises ONLY the `bar` member of Foo. Other Foo properties stay shallow (path-precise). Built-in utility types (`Pick`, `Omit`, `Required`, `Partial`) behave identically to a userland implementation referencing the same keys.
- **Carrier-preserving decl-body lowering.** Under `Shallow` (as under `Navigate` / `Skeleton`), decl-body lowering interns `DeclRef` / `InstantiationRef` carriers for member-value type references — including ALL builtin utilities — and never executes `ResolveDecl` / `Instantiate` eagerly; eager lowering-time execution is `Expanded` / `Identity` only; materialisation enters exclusively through the demand points (PathWalker hops, the shallow-surface synthesiser's carrier unwrap, closed object-filter surface reads, the relation/conditional oracle). Eager Shallow member-value lowering was the `Table.vue` storm: 94.3% of all budget charges were `Instantiate(StructuralTransit:Shallow)` recursion across the transitive TanStack decl graph.
- **Open key domain ⇒ shallow carrier (L1) — route/mode-independent.** TWO families stay shallow carriers at EVERY entrance, in every mode, and open-OR-UNKNOWN (including traversal-budget exhaustion) preserves the carrier instead of falling through into Expanded materialisation: (1) an object-filter utility (`Pick`/`Omit`) whose enumeration domain is OPEN or undecidable (`Pick<PropsBase<T>, …>` over the SFC's open `generic="T"` stays `Pick<…>`); (2) a mapped type `{ [K in S]: V }` whose produced surface still depends on an unbound OUTER generic (a CLOSED-key/open-VALUE mapped enumerates its keys path-precisely with shallow values). Closed sources still materialise the requested keys path-precisely. A carrier-stopped `Pick` at a SURFACE-enumeration demand (heritage arm / macro props-slots surface) still publishes its CLOSED output-key selection from the source's enumerable arms via the shallow walker's `Pick`-carrier enumeration — the source is never whole-materialised and `Omit` (source-dependent-open output keys) stays a carrier; zero-member surface collapse was the nuxt-ui ContentSearch/DropdownMenuContent bug. Typed-IR only, no string matching. The carrier-stop is the PRIMARY defense for the open-generic class; the per-request projection budget (`request_budget.rs`) is an ARMED-by-default runaway fuse (`projection_op_budget == 0` ⇒ effective cap 2000) whose trip returns `BudgetExceeded` as a genuine partial — refused warm admission, the no-poison invariant. Publication demand is `Navigate`-only on the projector/registry macro surfaces: a full `get_component_meta` records ZERO `Published(Expanded)` projection contexts; `Table.vue` and `ChatMessages.vue` are COMPLETE corpus members with un-ignored green trackers (`table_resolves_complete_and_warm`, `chat_messages_resolves_complete_without_false_partial`, `chat_messages_resolves_without_timeout`). The FULL authoritative spec — entrances, owner predicates, the per-argument position-sensitive key-domain rule, the tri-state conditional oracle, per-utility output-key semantics, mapped family composition, OPEN/CLOSED definitions, memoization, invalidation, the `TypeOf` demand rails, and the four named current scoped exceptions — lives in `/type-resolution` → Open-Key-Domain Carrier-Stop (L1).
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
- Substring path classification (`"/node_modules/"`, `"\\node_modules\\"`) — use `ResolverContext::workspace_is_package_backed(canonical_id)`. That predicate is the single structural authority for workspace-ownership classification, and it is what the live decision sites call directly (`component_meta_materialize.rs`, `framework/script_facts.rs`, `host_manage/jsdoc_resolve.rs`, `meta_resolve/graph_predicates.rs`, `meta_resolve/materialize/field_types.rs`, `meta_resolve/projectors/output_sink.rs`, `project_semantic_dispatch/raise.rs`/`walk.rs`, and others). Workspace-owned is its complement — there is no separate `workspace_is_workspace_owned` predicate.
- Name-suffix role inference (`name.ends_with("Props")` / `"Emits"` / `"Events"` / `"Model"` / `"Slots"`). Type-role classification is structural, not nominal: a type is a prop/emit/model/slot type because a Vue SFC macro (`defineProps`, `defineEmits`, `defineModel`, `defineSlots`, `withDefaults`) consumes it — read from `AnalyzedMacro.kind` / `parsed_type_argument` / `type_references` on the analyzer snapshot.

OXC is a syntax/lowering front-end only and never resolves types at query time. Macro/JSDoc producer fields still lower at their producer boundary via `lower_ts_type(ts_type, source)` (the producers emit content-free locators — `Analyzed*Field` carries `payload: Option<MacroPayloadLocator>`, `ResolvedLocalType` carries `shape: ResolvedLocalShape`; only display-only text rides alongside, surviving all caches); top-level declaration bodies lower LAZILY through the scheduler-retained parse snapshot (`DeclBodyMemo` → `DeclLoweringService`) and return owned typed IR before dispatch/reducers ever see them — no raw-string reparsing, no OXC resolver path. For the hot read surface the `decl_body_hot_ref` accessor mints a `HotTypeRef` handle over the `Instantiate` query result (`build_instantiate`'s post-processed node, produced via the resolving-lowerer body-shape helper `lower_decl_body_with_provenance`); the handle is NOT a re-lowering — bodies still lower to typed IR, and `DeclBodyMemo` records are fact+locator content-free end to end (the former memoized `LoweredTypeDecl.type_parameters: Vec<TypeParam>` pocket is CLOSED — the narrow mirror is the stored authority; bound content re-borrows lease-only). If a new requirement appears to need text manipulation inside the resolver, fix the producer (lower the right OXC node, store the right typed field, extend `@verter/type-ir` with a missing variant) rather than reparsing or pattern-matching on text.

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

Guards: `typeinfo_graph_taxonomy` (`crates/verter_session/tests/cases/g_block/typeinfo_graph_taxonomy.rs` — proto/TS oneof parity), `typeinfo_proto_ts_freshness` (`crates/verter_protocol/tests/cases/typeinfo_proto_ts_freshness.rs::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output` — regenerates the TS bindings via the workspace `buf` and `oxfmt` binaries and byte-compares), `request_kind_payload_parity` (`crates/verter_audit/tests/cases/request_kind_payload_parity.rs`), `typeinfo_request_validation` (`crates/verter_session/tests/cases/g_type/typeinfo_request_validation.rs` — closed-set schema-version + exhaustive structured-expression coverage), `typeinfo_wire_surface_guards`, `typeinfo_graph_contract_guards`, `typeinfo_request_contract_guards`, `typeinfo_audit_contract_guards`.

### Cross-Platform Portability (CRITICAL)

The codebase MUST build, test, and materialize on macOS, Windows, AND Linux. Platform-assuming code is a defect, not a nit.

Guard-enforced — `tracked_paths_are_portable` (`crates/verter_session/tests/cases/tracked_paths_are_portable.rs`) enumerates `git ls-files -z` and enforces: valid UTF-8; no NTFS-illegal characters (`< > : " | ? * \` plus control chars); no trailing dot or space; no reserved device basenames (`CON`/`PRN`/`AUX`/`NUL`/`COM1`–`COM9`/`LPT1`–`LPT9`, with or without extension, plus `CONIN$`/`CONOUT$` — the `$`-suffixed forms only); no case-insensitive path collisions (lowercase-fold approximation of NTFS/APFS folding, not the exact filesystem fold tables); ≤200-byte relative paths.

Review-enforced (the guard does not cover these):

- Sanitize generated on-disk names (e.g. `blake3:<hash>` → `blake3-<hash>`) — logical identifiers are unconstrained; only the on-disk boundary is. The guard only sees tracked paths, so it catches a generated name once committed, not at generation time.
- Build paths with `Path`/`PathBuf`/`Path::join` — never string concatenation with hardcoded `/` or `\`.
- Byte-equality comparisons over checked-out text normalize line endings (CRLF ↔ LF) or compare as text — never raw bytes embedding EOL.
- OS-specific binaries (`tsgo`, `.exe` suffixes) are discovered platform-aware, never via a hardcoded per-OS name.
- Temp and cwd paths come from std abstractions, not literal paths.

Guards: `tracked_paths_are_portable`.

### Anti-Binary-Growth Integration-Test Layout (CRITICAL)

Each crate exposes AT MOST one `tests/main.rs` integration-test binary; extra cases live under `tests/cases/` and are wired through `main.rs`. A second top-level `tests/*.rs` auto-becomes its own test binary and re-balloons the gate, so it is forbidden unless EXACTLY allowlisted. The only sanctioned exceptions are genuine "needs a separate test process" cases (process-global state that must be isolated): `verter_session` `allocator_canaries` (a counting `#[global_allocator]`) and `verter_lsp` `lsp_audit_trace_out_env_var` (a process-global env mutation). The allowlist (`scripts/integration-test-layout-allowlist.json`) is the single source of truth shared by both guards, is EXACT (package + target + repo-relative `src_path`, no globs/prefixes), and is STALE-FAILING — an allowlisted target that no longer exists in `cargo metadata` (or whose `src_path` moved) FAILS the guard.

Dual guard: the fast-fail CI Node check `scripts/check-integration-test-layout.mjs` (runs before the Rust gate) and the in-gate Rust mirror (`crates/verter_session/tests/cases/integration_test_layout_guard.rs`), both reading the same allowlist.

Guards: `integration_test_layout_is_consolidated`, `layout_checker_discriminates_stray_and_stale`, `allowlist_is_the_known_standalone_targets`.

### Framework Adapter Substrate (CRITICAL)

Multi-framework component support is ONE shared adapter substrate, not a per-framework semantic fork. `verter_session::framework` owns the `FrameworkAdapterRegistry` (built once at `VerterHost` construction), the per-adapter `FrameworkAdapterDescriptor` (identity, supported surface kinds, carrier language, the `VirtualFileNaming` column), the facts/carrier-only `FrameworkAdapterCtx`, the `ComponentDefaultSynth` seam, and the two-pass script-fact seam. Vue is the REFERENCE adapter — re-housed as a true plan/normalize adapter (`VueFrameworkAdapter` + the relocated `vue_exec` resolution delegates), NOT a privileged hardcoded path.

Closed-contract rules:

- **One audited wire entry, validation-first.** `VerterHost::resolve_framework_surface_with_audit(TypeInfoGraphRequest)` is the SOLE entry for the `GRAPH_OPERATION_FRAMEWORK_SURFACES` operation. It runs `validate_type_info_graph_request` FIRST (op/payload-arm match, schema echo, the nested framework-surface validator) — a malformed envelope returns the typed wire `error` arm BEFORE any registry lookup or semantic dispatch. A bare-inner-request entry is forbidden. The operation rides the EXISTING typeinfo graph envelope, and its current `FrameworkSurfacePayload`/embedded-`SemanticTypeGraph` shape is PROVISIONAL — an interim wire pinned today, NOT a permanent "no schema change" guarantee. The hard gate `S5.B11/B12 → U8` was landed ahead of order, so U8 still OWES the retag of `FrameworkSurfacePayload.graph` to a `TypeInfoGraphPayload` carrier, the `SemanticTypeGraph.schema_version` bump, and reserving the old field per the Typeinfo Wire Contract (CRITICAL) above; until U8 lands this wire stays pinned but is not final. Guard `framework_surface_wire_executor_validates_first`.
- **Registry dispatch, no privileged framework branch.** The executor interns `selector.framework_adapter_id`, looks up the registry (unknown id ⇒ typed `MalformedPayload`, NO new error variant), and dispatches to the adapter. Every wire `FrameworkTag` maps to a registered adapter OR an explicit `TagDisposition` row (`DeferredVertical` / `OutOfScope`); a tag's existence is NOT a support guarantee — support is asserted only by a registered adapter and surfaced per-request via `FrameworkSurfaceKindStatus`. Guard `framework_registry_complete` (+ the `framework_surface_executor` integration suite).
- **Closed plan/resolve/result vocabulary.** The adapter PLANS demands (`plan_surfaces` ⇒ closed 4-variant `PlannedDemand` — `MacroPayload` / `PathProjection` / `ShallowSurface` plus the Svelte arm `SvelteSurface`; no `Custom`/`Raw` arm, no source text / OXC handles / raw `SemanticQueryKey`s) and NORMALIZES resolved data (`normalize`); it holds NO resolve entry point. The executor resolves each `PlannedDemand` through the module-private `ExecutorResolveCtx` (EXHAUSTIVE match, no wildcard) THROUGH the one shared type-resolution engine — it plans, dispatches, and encodes; it is never a second resolver. Per-kind status maps DIRECTLY onto `SUPPORTED`/`PARTIAL`/`UNSUPPORTED` via the typed `ResolvedOutcome` (a supported-empty kind stays distinct from an unsupported kind). The first `SemanticTypeGraph` encoder (`graph_export`) is a pure ZERO-DISPATCH shallow projection of resolved data — named refs mint `GraphSymbolNode` + `GraphReference{symbol_id}`, structural unencodables degrade to `GraphOpaque`, never a fabricated ref and never a re-resolution.
- **Facts/carrier-only adapter ctx.** `FrameworkAdapterCtx` exposes EXACTLY two ops — `carrier_for::<T>` (the adapter's typed parse carrier, `None` for a carrier-less adapter — never a forged token) and `script_facts_for::<T>` (resolved script-fact evidence on demand: exact, partial, unavailable, or not applicable). It never resolves types, indexes a file, runs OXC, calls `ProjectSemanticDispatch`, or reads a `StoreView`. Guard `framework_adapter_ctx_closed_surface`.
- **Two-pass script-fact seam.** The syntax-capture half (`verter_semantic::analysis::framework_facts`) captures candidates from the live OXC program — SYNTAX-ONLY (may touch OXC + `lower_ts_type`, MUST NOT resolve imports or read capability bits; guard `script_fact_capture_is_syntax_only`). The session mints an exact candidate inventory, including exact-empty, only when its capture parse completes without syntax diagnostics; a recovered parse yields positive-only candidate observations, returns partial evidence, and never warms either script-fact store. Cache absence means “not computed.” The resolved-validation half (`framework/script_facts`) drives provider `validate` on demand over neutral resolved-import + capability data and preserves exact (including exact-empty), partial, unavailable, and not-applicable evidence. Resolved-fact publication accepts only producer-minted exact evidence under the fact-rail + strict-same-generation gate with `SignatureAdmission::Cacheable`-only publication (overflow ⇒ `ReturnOnly`, no warm); partial/unavailable results remain cold. Partial payloads have no whole-payload accessor: Svelte consumers can visit explicitly named conservative positive observations, while a resolution-only partial can separately expose its producer-proven exact syntax facet. Svelte facts keep syntax-owned facts (including sealed `ExactSveltePropsCalls`) separate from resolution-owned `Snippet`/dispatcher provenance, so resolution failure cannot erase exact script geometry. An EMPTY active-provider set is byte-identical zero-cost (Vue does NOT move onto the seam). The `ActiveProviderIndex` is the shared gate authority. Guard `script_fact_providers_zero_cost_on_miss`. The framework-surface result caches (`FrameworkSurfaceStore` / `FrameworkScriptCaches`) are fact-validated today but live on the framework registry rows, NOT the single `ProjectTypeStore` — they are PROVISIONAL off-store caches to be consolidated onto `ProjectTypeStore` (and given true singleflight) at U10.
- **Parse-domain component-default synth.** `ComponentDefaultSynth` synthesises a component's default-export value symbol from PARSE-DOMAIN inputs only (macros + syntax-capture candidates); it never names the resolved-validation fact types. Registry-dispatched at the shallow-analysis injection points by the file's resolved language. Guard `component_default_synth_parse_domain_only`.
- **Generated virtual-file naming is descriptor-owned.** The `VirtualFileNaming` column is the single authority for an adapter's IDE / API / testing-API / sidecar suffixes; the committed TS mirror (`packages/language-shared/src/virtual-file-naming.generated.ts`) is rendered from it and byte-pinned. Guard `virtual_file_naming_ts_freshness`.
- **No re-export shim for relocated Vue resolution.** The Vue resolution bodies relocated to `framework_surface::vue_exec`; `typeinfo/adapters/vue/{public_type,surface,store}.rs` are DELETED with no re-export shim or alias under `adapters::vue`, and `VueShallowMetadataStore` / `VueMacroDtoKey` are retired. Guards `vue_relocation_no_shim` + `retired_symbols_absent_from_production_source`.

See the `/framework-adapters` skill for the substrate's module map, the descriptor/registry/ctx/executor contracts, the script-fact seam, and Vue as the reference adapter.

### Carrier Geometry From Registered Facts (MANDATORY)

The carrier parser is the only production owner allowed to discover carrier structure from raw source. Downstream production code must not infer carrier geometry, section dialect, or block identity from text: for example, by searching for tags or delimiters, applying regex dialect heuristics, or recounting source order. Consume registered `RegisteredFileStructure`/`CarrierBlockInventory` projections and parser-minted facts, refs, or tokens; any retained compatibility ordinal is projected from that inventory, never reconstructed from text.

Local edit-time lexing may classify a bounded token or recovery window, but it must not become an alternate structure parser; any geometry-sensitive window is bounded by parser/inventory facts, including parser-unowned gaps bounded by registered parse boundaries. Tests may scan fixture text for setup, decoys, and assertions. This rule is review-enforced alongside the existing structural boundaries; do not add scanner-detection CI jobs or name-keyed source-tree guard scanners.

### Project-Bound External-TS Contract (CRITICAL)

Production external-TypeScript results for carrier sources are project-bound. The result-producing backend path is `ExternalTsProjectResolver` → `CarrierRegistry` → `EngineBackend`: `EngineBackend::ensure_project` is reached only from a resolved `ProjectBinding`, and `publish_snapshot`, `query`, and `diagnostics` require the resulting `BoundProject` witness. No production external-TS result path may infer a project from a bare path, open a carrier into a config-less/inferred project, or fall back to an inferred backend. Path-shaped transport notifications may exist below this contract, but they cannot construct external-TS results or bypass `BoundProject`.

Ownership is TypeScript-correct. A carrier source (`.vue`, `.svelte`, or any adapter extension) is owned by a configured project only through the default include, a no-extension directory/bare-star glob, or a glob/`files` entry that explicitly covers that extension. An extension-specific `*.ts` glob does not own it. TypeScript include has no brace expansion: multi-extension coverage is separate entries, never `*.{vue,svelte}`.

**Owner selection is tsgo-faithful, single-winner, and never terminally ambiguous when a configured project owns the carrier.** A carrier claimed by MULTIPLE configured projects is NOT a terminal state: `WorkspaceSnapshot::default_configured_owner_for_file` models tsgo `ProjectCollection.GetDefaultProject` + `findDefaultConfiguredProject` (`microsoft/typescript-go` `internal/project/projectcollection.go`) — start from the nearest ancestor solution (the nearest LITERAL `tsconfig.json`/`jsconfig.json`, not the nearest project root), BFS its `references` in DECLARED array order, take the FIRST project that directly includes the carrier, climb to the next ancestor solution unless `disableSolutionSearching`, and finally fall back to the lexicographically-least `tsconfig_path` among the configured claimants (tsgo `firstConfiguredProject`, a name-least ordering DISTINCT from the reference BFS order). The winner is chosen only from ordered structures (the `projects` Vec, `references` Vec, an ordered visited set) — never `HashSet` iteration; reference cycles resolve via the visited set, never a terminal ambiguity. The resolved owner flows through the SAME `binding_for` → `BoundProject` witness as the unique-owner arm. This selection is provider-neutral: the ONE `WorkspaceSnapshot` decision the tsserver, managed-tsgo, and shared-tsgo carrier routes all consume identically.

**Bounded divergence (recorded per tsgo faithfulness).** Verter's `ConfiguredMembership` is include/`files` only and carries NO `IsSourceFromProjectReference` (program-level project-reference-redirect) data, so for a carrier every include/`files` hit is treated as DIRECT and tsgo's `multipleDirectInclusions` is effectively always true — the reference BFS decides. The residual solution-graph pruning in `configured_owner_resolution_for_file` (which can pre-select a leaf over an ancestor that both includes and `references` it) is preserved on the `Unique` arm and is NOT unified with the BFS.

The remaining TERMINAL no-serve states are ONLY: `NoProject` (no configured project's include/`files` covers the carrier's extension at all) and the disk-layout carrier-path conflicts (a real user file at a generated companion path; a same-stem rune module beside the source). A multi-claimant configured overlap is NO LONGER terminal. `NotReady` stays the transient bootstrap retry state. Terminal `NoProject` / carrier-path conflicts surface a user-visible `verter(project)` warning on `did_open` AND `did_change` (wired through the debounced coordinator publish path, not only the request-only full-diagnostics path). Verter-native non-external-TS features may still answer. `SyntheticScratch` is a separate, explicitly labelled scratch lane for non-cross-file features only. It never supplies configured-project semantics, batch typecheck, cross-file results, or project-cache warming.

**Rename fail-closed for a resolved multi-claimant carrier.** A multi-claimant carrier serves every per-file feature (hover, definition, completion, references, diagnostics) from its single resolved owner, but a provider rename runs only within that one project. A symbol that ESCAPES the owner project (exported + imported by a sibling configured project) would rename partially and leave the symbol dangling in the siblings; cheap escape detection needs the cross-project rename fan-out (a future block). Until then rename FAILS CLOSED (a clear error, no `WorkspaceEdit`) for a resolved multi-claimant carrier — never a silent partial cross-project rename — while every other feature serves normally. A uniquely-owned carrier renames normally.

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

# Rust — CANONICAL agent gate
node scripts/gate.mjs                         # THE Rust gate. Builds the test universe ONCE via `cargo nextest archive` (single compile, no second-command recompile), then runs surfaces 1 and 2 from the same artifacts: SURFACE 1 = nextest run (per-test process isolation), SURFACE 2 = the verter_session libtest binaries executed directly (in-process / multi-test-per-process). SURFACE 3 then builds a SECOND `--workspace` archive with `--cargo-profile no-debug-assertions` — the shipped `cfg(debug_assertions)` state (debug assertions and overflow checks OFF, dev codegen otherwise) — and RUNS `package(verter_session) + package(verter_scheduler)` from it. That second archive is a second whole-workspace compile (a different profile is a different unit hash, so nothing is shared with the dev archive); it is the only surface that executes tests with `debug_assertions` off, and it is NOT an optimised build. Before the archive build it runs a freshness-tooling preflight: it ensures the workspace `buf` + `oxfmt` binaries are present (auto-running `pnpm install --frozen-lockfile` inside the mutex/timeout/stall machinery when the `node_modules/.bin` shims are missing), then VERDICT-GATES the `cases::typeinfo_proto_ts_freshness::*` byte-pin tolerance on the outcome — tooling present/installed ⇒ tolerance OFF, so a freshness failure is a HARD gate failure (exit 1), NOT PASS-WITH-TOLERATED; a deterministic install failure (e.g. frozen-lockfile mismatch) ⇒ a LOUD setup failure (exit 127), never silently tolerated (when an install is attempted — both `node_modules/.bin/{buf,oxfmt}` shims already present ⇒ the preflight returns already-present and no install runs); when pnpm is not resolvable AND `buf` is not resolvable the Rust byte-pin pair SKIPS gracefully and PASSES, so the gate reports an ORDINARY PASS (no FAIL line) — the verdict-gated tolerance flips ON there only as a LATENT safety net that would surface PASS-WITH-TOLERATED solely in the unusual case the pair produced a tolerated FAIL despite `buf` being absent. `oxfmt` absence NEVER grants tolerance — with `buf` present, a missing `oxfmt` is a LOUD setup failure (exit 127), not a degraded run. Run it with `node_modules` present (the normal path) so the byte-pin runs GENUINELY: with the tooling present a freshness failure is a HARD FAIL (a real stale-binding regression to regenerate + commit) — PASS-WITH-TOLERATED is NEVER the regression signal on a normal machine, and on a buf-less runner the pair yields an ordinary PASS via the skip, not PASS-WITH-TOLERATED. See docs/arch/gate-performance.md.

# The TWO UNDERLYING SURFACES gate.mjs runs — runnable directly (no Node, or debugging one surface in isolation):
cargo nextest run --workspace                # SURFACE 1 — every workspace test target INCLUDING the ~25 verter_session integration binaries, per-test process isolation
cargo test -p verter_session --tests         # SURFACE 2 — shared-process (in-process) surface for the verter_session integration suite
cargo test --workspace --doc                 # Rust doctests only; run when rustdoc examples changed or explicitly requested
cargo test --package verter_compiler test_name   # Specific Rust test
# NOTE: bare `cargo test --workspace --tests` SILENTLY SKIPS the verter_session integration suite (~4404 tests) because `session_metrics` feature unification drops those binaries from the workspace test set — it MUST NOT be the sole Rust gate; run `node scripts/gate.mjs` (which runs surfaces 1-2 from one archive plus the shipped-cfg SURFACE 3 from a second) or the two-surface pair above directly — note the pair covers neither the shipped-`cfg(debug_assertions)` surface nor its compile.
cargo test --package verter_compiler 2>&1 | tail -60  # Full suite with truncated output
```

**Oversize-source advisory:** `scripts/gate.mjs` scans production Rust sources and warns for each
non-exempt file above 1,500 lines as `path (N lines)`. File size is informational and never affects the
gate verdict.

**Resource ceilings — MEMORY-CEILING-MANDATORY.** Every gate/`--prepare` run is bounded by three flags,
each host-derived by default and overridable: `--build-jobs <N>` (cargo build parallelism, default
`min(4, cpuCount)`), `--test-threads <N>` (nextest/libtest thread count, gate-only, default
`min(4, cpuCount)`), `--memory-limit <SIZE>` (active child-tree RSS ceiling, e.g. `8GiB`/`12288MiB`,
default `max(512MiB, 50% of physical RAM)`). Policy: every gate run — agent or CI — goes through this
bounded wrapper with an explicit or defaulted `--memory-limit`; there is no sanctioned unbounded
invocation. A breach kills the active child tree fast and exits 123 (`ABORTED — memory ceiling`) — this
means NO gate verdict was produced. The same rule covers TIMEOUT (124), STALL (125), and any run whose
summary did not complete: an aborted or incomplete run is NOT PROVEN, never a PASS, and must never be
recorded as one.

**Build-prerequisite preflight (fail-closed, the gate's FIRST step).** Parts of the Rust suite load
artifacts cargo does not build: the real-provider suites spawn the pinned tsserver with `--globalPlugins
@verter/typescript-plugin --pluginProbeLocations packages/vue-vscode/node_modules`, a pnpm symlink to
`packages/typescript-plugin` whose `main` is `dist/index.js` — a `tsc -b` OUTPUT that `pnpm install` does
NOT produce. With the symlink present but the dist absent, tsserver loads no plugin and ~64 `*_tsserver`
tests fail with `TS2307: Cannot find module './Comp.vue'`, indistinguishable from a compiler regression.
So before the freshness preflight, before cargo, and before any test, `gate.mjs` **loads** that plugin
entry in a child process (`require()` of the probe directory, exactly what tsserver resolves) and on any
load failure FAILS CLOSED (exit 127, marker `BUILD-PREREQUISITE MISSING`) naming the probe target, the
load error, the producing packages, and the producer command. The oracle is a real load, not a list of
files to stat: the entry eagerly requires its emitted helpers and `@verter/language-shared`'s entry
re-exports a dozen emitted siblings, so a stat list mirrors the emit graph and drifts — both `index.js`
present with one helper missing passes every stat and still throws inside tsserver. The probe runs under
**tsserver's** environment, not the gate's — it strips the `CHILD_PROCESS_ENV_DENYLIST` the tsserver
launcher strips, read out of `crates/verter_type_runtime/src/tsserver/ipc.rs` so the two cannot drift,
because otherwise a `NODE_OPTIONS` preload can forge a status-0 load — and its timeout is a hard `SIGKILL`
bounded by the gate's own remaining deadline, since `spawnSync`'s default SIGTERM is trappable and a
trapping child both hangs the gate (with the single-flight mutex held) and returns a false positive.
Failure classes are typed (`reason`), so only `module-not-found` may ever be read as "never built". It
proves the closure **resolves**, not that it is **fresh**; a stale-but-loadable dist is a separate,
deliberately out-of-scope problem. It never builds the artifacts (the verdict must not depend on a mutation the gate performed) and
never skips the affected tests (with no install at all those tests SKIP and the gate goes green while
proving nothing). Produce them with `pnpm --filter @verter/language-shared --filter
@verter/typescript-plugin build` — deliberately NOT `pnpm build` and NOT `--filter
@verter/typescript-plugin...` (the trailing ellipsis pulls in `@verter/native`'s `napi build --release`).
`--prepare` is exempt; it runs no test. `(GB9)` in `scripts/gate-selftest.mjs` proves the discrimination in
six directions against the real production CLI on a synthetic miniature of the package graph.

### End-of-change Checks

Run after **every** change. Verter's crates are highly interconnected — a change in one crate frequently breaks tests in dependent crates. Always run the full workspace suite:

```bash
node scripts/gate.mjs 2>&1 | tee /tmp/test-output.txt   # CANONICAL Rust gate — runs THREE surfaces: SURFACE 1 (nextest process-isolation) + SURFACE 2 (direct in-process verter_session) from ONE dev-profile archive with zero second-compile, then SURFACE 3 from a SECOND `--workspace` archive built with `--cargo-profile no-debug-assertions` (the shipped `cfg(debug_assertions)` state), which RUNS `package(verter_session) + package(verter_scheduler)`. SURFACE 3 is the ONLY thing in the repo that executes tests with `debug_assertions` off — the class where a state mutation written inside a `debug_assert!` argument is a silent no-op in every shipped build. It is not an optimised build (dev codegen, no LTO) and it runs only that filterset. Run with `node_modules` present so the freshness-tooling preflight is a no-op and the `cases::typeinfo_proto_ts_freshness::*` byte-pin runs GENUINELY: with the tooling present a freshness failure is a HARD gate failure (exit 1, a real stale-binding regression to regenerate + commit), NOT tolerated. On a buf-less runner (pnpm not resolvable AND `buf` not resolvable) the Rust byte-pin SKIPS and PASSES, so the gate reports an ordinary PASS — the verdict-gated tolerance flips ON there only as a latent safety net (PASS-WITH-TOLERATED appears solely if the pair somehow emitted a tolerated FAIL despite `buf` being absent, which the skip does not). `oxfmt` absence never grants tolerance (with `buf` present a missing `oxfmt` is a LOUD setup failure); a deterministic install failure (frozen-lockfile mismatch) fails loud as setup (exit 127) when an install is attempted (both shims already present ⇒ no install runs).
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --release   # Compiles the REAL release profile (opt-level 3 + fat LTO), which the gate does not: gate SURFACE 3 uses the cheap `no-debug-assertions` profile, and surfaces 1-2 are debug. `debug_assert!` gates on `cfg!` — a RUNTIME constant — so its body still name-resolves in release: a `#[cfg(debug_assertions)]` helper called inside one is an E0425 in every release build (napi and wasm artifacts included) while compiling clean in debug. This is a CHECK — it RUNS NO TESTS, so it CANNOT observe the runtime half of the same class (a state mutation written inside a `debug_assert!` argument, which compiles fine and silently never executes in a shipped build). That half is covered only by gate SURFACE 3. Do not read a green `cargo check --workspace --release` as coverage of `debug_assert!` behaviour.
cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings   # Host clippy cannot see target-gated code. The wasm32 artifact is what the playground and `@verter/wasm` consumers run. The `wasm32-wasip1`/`wasip2` clippy jobs cover the SEPARATE lapce/zed manifests, not this one.
cargo fmt --all --check
pnpm install --frozen-lockfile   # Verify lockfile is in sync (CI uses this); also what the gate's preflight runs to make the freshness byte-pin run genuinely
```

Confirm `cargo clippy --version` reports the `rust-toolchain.toml`-pinned version before
trusting any of the three lint/check results — a clippy run on a different toolchain is not
evidence about the one CI uses.

- Corpus audit-test regenerator (run after audit-record schema or fixture changes; idempotent): `node scripts/gen-corpus-audit-tests.mjs`

For TypeScript changes, also run `pnpm test`. Do not skip workspace-wide testing even for "small" changes.

**Agent test policy:** `node scripts/gate.mjs` is the default Rust gate — it builds the test universe once and runs the two debug surfaces (`cargo nextest run --workspace` process-isolation + the in-process `verter_session` libtest binaries, the same direct surface as `cargo test -p verter_session --tests`) from the same archive with no second-command recompile, then builds a second `--workspace` archive under the `no-debug-assertions` profile and runs `package(verter_session) + package(verter_scheduler)` from it (SURFACE 3, the shipped-`cfg(debug_assertions)` surface). It runs the `verter_session` binaries under the workspace-unified `session_metrics` feature set (ON), intentionally replacing the old package-scoped default-feature (`session_metrics` OFF) rebuild rather than reproducing its feature config — that ON config is what the shipped LSP uses and what removes the second compile; no test target the old pair compiled is dropped. A contributor without Node, or debugging one surface in isolation, runs `cargo nextest run --workspace` then `cargo test -p verter_session --tests` directly. The `cases::typeinfo_proto_ts_freshness::*` buf/oxfmt byte-pin is the only tolerated failure, and its tolerance is now VERDICT-GATED on the gate's freshness-tooling preflight: the gate ensures `buf`/`oxfmt` are present (auto `pnpm install --frozen-lockfile` when the `node_modules/.bin` shims are missing) so with `node_modules` present that pair runs GENUINELY — and with the tooling present, tolerance is OFF, so a freshness failure is a HARD gate failure (exit 1, a real stale-binding regression to regenerate + commit), NOT surfaced as PASS-WITH-TOLERATED. On a buf-less runner (pnpm not resolvable AND `buf` not resolvable) the Rust byte-pin SKIPS and PASSES, so the gate reports an ordinary PASS (no FAIL line); the verdict-gated tolerance flips ON there only as a latent safety net, surfacing PASS-WITH-TOLERATED solely in the unusual case the pair emitted a tolerated FAIL despite `buf` being absent. PASS-WITH-TOLERATED is never the regression signal on a normal `node_modules`-present machine, and never the normal buf-less verdict either; `oxfmt` absence never grants tolerance (with `buf` present a missing `oxfmt` is a LOUD setup failure); a deterministic install failure (frozen-lockfile mismatch) fails loud as setup (exit 127) when an install is attempted (both shims already present ⇒ no install runs). Run the gate with `node_modules` present. Do not run bare `cargo test --workspace` (no `--tests`) by default: it pulls in doctests and example builds without improving the normal verification loop (and the silent-skip trap is stated once in Running Tests above). Run doctests (`cargo test --workspace --doc`) only when rustdoc examples changed or the user explicitly asks.

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

**Public-boundary acceptance**: for every changed user-visible IDE, API, or compiler outcome, each affected acceptance ID has an automated public-boundary test asserting the required result AND the relevant forbidden or fail-closed result. Provider-selection, status, unit, and architecture tests supplement but do not substitute for that boundary test. A substrate block may inherit a parent boundary test only by recording the acceptance-ID mapping and executing that test in its gate. Enforcement is judgment — reviewers assess the actual invocation path and assertions, not the filename; §1a proves discrimination; confirm reruns the mapped test.

**Architecture guards for critical rules**: every new `CRITICAL` architecture rule lands with a static architecture guard or a discriminating regression test in the same change (subject to the landed-scanner bar below — a "static guard" is never a new name-keyed file scanner); if a guard cannot be automated yet, the rule text names the planned guard/test and the gap is tracked in the owning skill/doc. The R6 meta-guard at `crates/verter_session/tests/cases/g_misc0/critical_rules_have_guards.rs` (`every_critical_rule_in_docs_has_registered_guard`) walks `CLAUDE.md` plus every `.claude/skills/*/SKILL.md` and asserts every `(CRITICAL)` heading has a `CRITICAL_RULE_GUARDS` registry row with at least one named guard — a prose-only `(CRITICAL)` section fails the gate.

**Landed guards are structural, never name-keyed file scanners (forward-only)**: a heuristic file-scanner guard/test that keys on a specific tool, function, or identifier name (any spelled source name/path/token — type, module, import/path-segment, and string identities included; `syn`/AST-based scanning included) is a transient plan artifact — WIP-only (scratch branches, squashed out before landing), never a full-fledged landed guard. LANDED enforcement of an invariant is structural — compiler/type-system/tool-based (privacy/visibility/`E0603`, type-state, sealed traits, marker-trait derives, a real used tool or function) — never a name/text/grep scanner over the source tree. This strengthens Structural-Confinement-First (`.claude/skills/mom-cto-orchestration/reference/PROTOCOL.md` → Structural-Confinement-First → Landed-scanner bar): even a residual scanner that rule would permit (justified, recorded, supplement to a structural primary) does not land — keep it WIP, replace it structurally, or accept the residue uncovered by any landed scanner. Review/governance-enforced by design, NOT guard-enforced — a guard that detects "name-keyed scanner guards" would itself be a name-scanner. Forward-only: pre-existing landed scanners are grandfathered as a class — by temporal status (already landed at rule adoption), not by list membership — and retained as-is; the explicitly disclosed high-risk example (illustrative, not an exhaustive inventory) is the hot-materialize syntactic tripwire (`hot_materialize_syntactic_tripwire_residual_backstop` + its `HOT_TERMINAL_SINKS`/`HOT_DECIDE_TAINTED_GATE_IDENTS`/`HOT_EXTRACTING_GATE_IDENTS`/`HOT_MAT_DIRECT_IDENTS` name-lists in `crates/verter_session/tests/cases/output_projector_residual_guards.rs`), retained as-is with no removal planned or required — its STRUCTURAL rail (the `NoTypeExpr` marker + the sealed `OutputProjector` capabilities) remains the durable primary.

**Rust test file organization**: When inline `#[cfg(test)]` exceeds ~400 lines, extract to a sibling `*_tests.rs` file.

### Verification Must Prove Execution (MANDATORY)

A required gate passes only on fresh, input-bound evidence that: every applicable required job was eligible and ran; the intended tree-derived surface was owned and independently discovered; selectors matched non-zero work; required source, build, and fixture prerequisites matched the tested tree; executed work was non-zero; unexpected prerequisite skips were zero; child deadlines were strictly below their parent killer; and a terminal summary completed. **Exit status 0 alone, a self-declared test universe, or a missing required-job result is FAIL.** Every tracked test or guard has exactly one declared primary gate; a hand-maintained filename list may not define the primary universe unless generated from independent discovery and parity-checked.

Attestation alone is insufficient — a receipt faithfully attests whatever incomplete universe the runner defines for itself. The durable design needs all three: fresh execution attestation; independently tree-derived inventory/discovery parity; and per-surface negative-control mutation through the exact canonical entry point. A single global canary cannot detect an omitted unrelated spec.

**The negative control must itself be proven to have applied.** A plant that fails to apply reports a pass: `perl`/`sed`/`grep` exit 0 on a non-match, so a mutation's exit code is never proof it landed, and a verification search hitting a PRE-EXISTING occurrence of the planted string is a false positive. Prove the mutation is present, unique, and new in the source before trusting the run; a green planted run means the plant failed until proven otherwise. A discrimination check that cannot distinguish "the plant did not apply" from "the code is correct" is not a discrimination check.

Planned guard: `gate_contract_integrity` — one registered suite exercising the canonical entry point against independent inventory plus per-surface negative controls covering missing summary, disabled or missing job, invalid timeout nesting, zero selection, stale or missing build, missing fixture or unexpected skip, omitted or unowned test, and a mutation that silently fails to apply. Until that guard, its attesting driver, and the required-job aggregator land, this rule is held only by §1a and confirm judgment.

**This rule currently fails its own test, and says so.** It ships `(MANDATORY)` — precisely the tier the R6 meta-guard (`every_critical_rule_in_docs_has_registered_guard`) does not check, because that guard scans `(CRITICAL)` headings only. A rule whose thesis is "a gate that cannot prove it ran is a failure" is therefore, today, a gate that cannot prove it ran. `(CRITICAL)` is not available as a shortcut: an unguarded `(CRITICAL)` heading FAILS the meta-guard. So the gap is named rather than hidden — the deferral, its owner (the gate-integrity block), its resolution gate (that block's landing), and the live in-tree instances are recorded in [`docs/arch/gate-integrity-ledger.md`](docs/arch/gate-integrity-ledger.md). Promotion to `(CRITICAL)` with its own `CRITICAL_RULE_GUARDS` row, in the same change that lands the guard, is an ACCEPTANCE CRITERION of that block (ledger row GI-4). It is never folded into `Stub Prevention` — a related but distinct invariant whose guards do not enforce these semantics.

### Testing-Hermeticity (MANDATORY)

Unit tests must only depend on locally-vendored fixtures. They must compile and run without any third-party repository (e.g., `nuxt-ui`, `element-plus`) checked out alongside this repository. Tests that need external corpora must be feature-gated (e.g., `#[cfg(feature = "external-corpus")]`) and excluded from the default canonical run (`node scripts/gate.mjs`, i.e. its two underlying surfaces `cargo nextest run --workspace` + `cargo test -p verter_session --tests`).

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
2. **Intent Contract** — the ratified statement of intent, before any mechanism design
3. **Changes** — specific files to modify with concrete modifications
4. **Legacy Deletions** — explicit list of files, functions, code paths, feature flags to remove
5. **Verification** — full workspace test commands and expected outcomes

Without explicit legacy deletion lists, agents skip deletions and leave dual paths alive.

**Intent before mechanism.** Before mechanism design for a block that changes observable behavior, authority, or fallback, record a ratified intent contract: the actor/problem and why the capability should exist; required and forbidden observable outcomes; authority/fallback order; a planned test or gate for each stable acceptance ID; and material cold, warm, allocation, fan-out, and latency bounds. An internal substrate block may reference its parent contract but must state the invariant and performance contribution it owns. Ratification comes from the approved plan or product authority; no implementation brief is dispatched without it. Enforcement is judgment — exercised at decomposition and again immediately before implementation dispatch.

### Execution

Execute approved plans fully in one pass, end-to-end, without intermediate checkpoints or mid-plan confirmation on already-approved steps. Do not pause, defer scope, leave planned work unfinished, or rewrite the plan into a smaller/safer variant because the correct path is breaking, broad, or labor-intensive. Approved plans land as written unless the user explicitly re-scopes them.

**One-pass execution applies only while the approved design remains valid.** The second-REOPEN circuit breaker lapses approval for the affected design: pause implementation, obtain and record the required architecture/product ruling, and resume only once the design is ratified again. This is not a checkpoint — one-pass governs *executing an approved design*, and the breaker fires when *approval itself has lapsed*, which is a different event and precisely why execution must stop rather than grind on. STOP, failed verification, rule conflict, and verified plan-invalidating discoveries pause at their prescribed evidence gate without creating a discretionary user checkpoint. Breadth, breakage, effort, or migration size never lapses approval; approved scope changes only through the recorded ruling or explicit user re-scope. See `/mom-cto-orchestration` → Decision Admission.

### Orchestrating Large Plans

For a large multi-block plan, refactor, migration, or staged cutover executed autonomously, drive it via the `/multi-agent-orchestration` skill rather than improvising: a pure orchestrator delegates blocks to implementer/reviewer/fix sub-agents, gates each on dual review (independent reviewer + `codex`), runs fix cycles until clean, and verifies sub-agent reports against git state (trust but verify).

When a block runs in a dedicated `git worktree`, run `pnpm install --frozen-lockfile` in the worktree root once at creation time, before any JS/TS test or workspace-importing Node script — fresh worktrees do not get the gitignored `node_modules/`, and a missing install makes JS/TS tests fail spuriously and read as a false regression. See the skill's "Worktree hygiene & environmental discipline" section.

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
- Never apply a dirty fix that contradicts architectural rules just to make tests pass
- If the proper fix is outside approved scope, do not apply a workaround and do not use a `TODO` as its disposition. Route the finding through the applicable scope authority and record `ADOPT-NOW`, `DEFER`, or `REJECT` before related work continues. A `TODO` may reference an approved debt row but never replaces it.

**Explicit finding disposition.** Every scope-deviating correctness finding is dispositioned before related work continues as `ADOPT-NOW`, `DEFER`, or `REJECT`. `ADOPT-NOW` records the scope and acceptance-contract change. `DEFER` requires a codex-DEFER ruling and a debt row naming the durable owner block, the resolution gate no later than plan close, the acceptance ID/test, and the ruling reference. `REJECT` records evidence and rationale. A TODO, a feedback entry, or an ephemeral agent identity is not a disposition; plan close requires zero open deferrals. Enforcement is judgment — codex at the scope consult, and the plan-close zero-open-deferral check.

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
- New or ported repo-owned tooling lands as a Rust bin (e.g. the `gen-typeinfo-manifest` cargo bin) or a
  Node script — never a committed Python script.
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

Scopes: `core` (verter_compiler), `napi` (verter_napi / @verter/native), `wasm` (verter_wasm / @verter/wasm), `play` (playground), `unplugin` (@verter/unplugin), `lsp` (language-server), `types` (@verter/types), `meta` (@verter/component-meta), `ci` (CI/CD workflows), `*` (multiple areas).

Example: `feat(core): add v-memo directive support`

**No program vocabulary in commit messages or source.** A commit message describes the change on
its own terms: it must not name the architecture program, its revision, or any of its block
identifiers. A commit that lands plan text says what the text decides, not which block decided it.
The same prohibition applies to source under `crates/`, `packages/` and `scripts/` — see "No phase
archaeology in production code". The program's own document tree and this file are exempt while the
program runs; this file carries the program banner and may cite blocks by identifier.

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
| `/testing`               | Test patterns, TDD workflow, the canonical `gate.mjs` Rust gate runner, sourcemap testing, server cleanup |
| `/e2e-vscode-testing`    | VS Code E2E test fixtures, helpers API, adding new tests                                         |
| `/wsl-e2e-testing`       | WSL E2E tests to reproduce Linux/CI failures, fixture matrix                                     |
| `/rust-performance`      | Rust optimization patterns, allocation hierarchy, CodeTransform API                              |
| `/multi-agent-orchestration` | Driving a large multi-block plan, refactor, migration, or staged cutover autonomously: pure orchestrator + implementer/reviewer/fix sub-agents, dual review (independent + codex), per-block fix cycles, trust-but-verify |
| `/scheduler`             | Scheduler submission/admission APIs (`submit_request`/`submit_batch`/`submit_batch_atomic`), CPU vs I/O pool routing, host CPU-pool coordination |
| `/debug-tooling`         | Hangs, unexpectedly slow paths, stack snapshots: backtrace watchdog, LLDB attach wrapper, release-dbg profile |
| `/agent-prompts`         | Generating implementation/continuation/review/fix prompts for driving separate agent sessions |

exec
/bin/zsh -lc "sed -n '1,260p' .claude/skills/architecture/SKILL.md" in <repo-root>
 succeeded in 0ms:
---
name: architecture
description: "Verter codebase architecture: high-level module map, TypeScript packages, plugin system, CSS analysis, MCP server, static analysis types"
---

# Verter Architecture Reference

For domain-specific detail, see: `/type-resolution`, `/type-cache-architecture`, `/component-meta`, `/compiler-codegen`, `/host-session`.

## Shared Substrate Principle

Verter is one shared optimized codebase. Consumers reuse lower-level crates instead of separate semantic pipelines.

- Put reusable parsing, analysis, type-resolution, caching, and import-following behavior in the shared owner crate.
- `verter_language` is the zero-dependency leaf routing authority: `FileLanguage`, `FrameworkAdapterId`, `LanguageId`, `CapabilityId`, and the pure static `LanguageRegistry` (`classify_static(path)` — never reads project config). Host-gated classification (static registry × `ProjectCapabilitySnapshot`) is owned by `verter_session::framework::HostLanguageClassifier`; scheduler/workspace consumers reach it only through session-implemented trait objects (`SourceLoader::classify` / `WorkspaceAccess::classify_file`). The crate is a `verter_span`-only leaf (its design allowance — spans for the parse-artifact regions; strings stay crate-interned) and keeps a crate-local id-intern table: no lower crate exposes a reusable interning facility, and the id set is bounded by registered languages. It also owns the framework-neutral parse payload: `FrameworkParseArtifact` (typed `FrameworkParseCommon` — `ScriptRegion { span, source_type, kind }` / template / style regions, external links, `LanguageDiagnostic`s — plus a PRIVATE erased `Arc<dyn CarrierParse>`), with token-gated downcast (`CarrierAccessToken`, minted ONLY during `LanguageRegistry` carrier-row construction via `LanguageRow::carrier`; the session's blessed accessors are `framework::ctx::carrier_for::<T>` and the Vue adapter's `vue_parse()`; the concrete `VueParseCarrier` + Vue producer live in `verter_compiler::framework_common::vue_bridge`).
- `verter_session` is the shared host/session/cache boundary for host-backed consumers.
- `verter_semantic` and `verter_compiler` own reusable semantics, lowering, and codegen.
- `verter_session::resolver_core` owns the host-backed resolver stack and type-resolution orchestration. Resolver-path methods receive `ctx: &dyn ResolverContext` (sealed super-trait at `resolver_core/resolver_context.rs`) — only `VerterHost` implements it, enforced by the `no_concrete_verter_host_in_seal_scope` arch-guard.
- `verter_protocol` owns transport-facing schema DTOs; `verter_ffi` stays a thin native/WASM adapter layer.
- Consumer packages and apps stay adapter-oriented: thin wrappers, public API shaping, transport glue, UX-specific behavior.

Bug or slowdown in one surface → fix in shared substrate so other consumers benefit.

## TypeScript Packages

| Package | Purpose | Entry Point |
| ------- | ------- | ----------- |
| **`@verter/types`** | TypeScript utility types (`PatchHidden`, `ExtractHidden`, `EmitsToProps`, etc.). Has `/string` export with `$V_` prefixed types for LSP injection | `src/index.ts` |
| **`@verter/language-shared`** | Shared custom protocol types between VS Code client and Rust LSP binary | `src/index.ts` |
| **`@verter/typescript-plugin`** | TypeScript plugin resolving `.vue` imports in TS/JS files. Intercepts module resolution to return transformed TSX | `src/index.ts` |
| **`verter-vscode`** | VS Code extension. Launches Rust `verter-lsp` binary over stdio, bundles TS plugin, handles extension activation | `src/extension.ts` |
| **`@verter/unplugin`** | Universal bundler plugin (Vite, Rollup, webpack, esbuild, rspack, Rolldown, Farm). Compiles `.vue` files via `@verter/native`. Supports `preCompile` for build-start cache warming | `src/index.ts` |

## Unplugin Configuration (`packages/unplugin/`)

`@verter/unplugin` provides a `VerterPluginOptions` interface:

| Option | Type | Default | Description |
| ------ | ---- | ------- | ----------- |
| `componentId` | `(filename, source, isProd) => string` | hash-based | Custom component ID generator |
| `include` | `string \| RegExp \| (string \| RegExp)[]` | `[/\.vue$/]` | File patterns to include |
| `preCompile` | `boolean` | `false` | Pre-compile all `.vue` files during `buildStart`. Scans project root, upserts files into host cache (including type dependencies for macros), and compiles them. When `transform()` later receives same content, host returns cached result instantly. `node_modules` excluded from scanning. |
| `crossFileOptimize` | `boolean` | `false` | Cross-file prop constness optimization. Requires `preCompile: true`. After pre-compilation, analyzes render tree to determine which props are always passed constant values, skipping dynamic tracking in compiled output. |
| `template` | `object` | — | Template compiler options (compat with `@vitejs/plugin-vue`) |

**`preCompile` architecture:** During `buildStart()`, scans project root for `.vue` files (excluding `node_modules` and dot-directories). For each file: upserts into host, resolves external `src` attributes and macro type dependencies (e.g., `import type { Props } from './types'` used in `defineProps<Props>()`), then triggers compilation. When another plugin modifies the file before `transform()`, host detects content change via internal hashing and recompiles. Third-party `.vue` files in `node_modules` compile on-demand during `transform()` — no pre-compilation overhead.

**Macro type resolution invariant:** cross-file macro type resolution must only follow imports reachable from the requested type's local declaration graph. Unrelated imports in the same file are out of scope; plain imports are not implicit re-exports.

## CSS Analysis & Selector Matching (`crates/verter_semantic/src/analysis/`)

`verter_css_syntax` is the shared lossless token/event authority for CSS, SCSS, indented Sass, Less, and Stylus. `StyleSyntaxIrSink` and `LosslessCstSink` are peers over the same parser event stream. Semantic style analysis projects only complete, static selector nodes into selectors, classes, IDs, custom properties, and at-rules; interpolation, recovery, and evaluation-dependent selectors fail closed. Each `AnalyzedCssClass` carries `selector_index` (exact class → comma-part selector join) and each `AnalyzedSelector` carries `rule_body_span` (brace- or indentation-delimited body span). Vue's planner separately consumes trusted IR for authored-dialect `v-bind()` and post-preprocess plain-CSS module hashing/scoping; Svelte consumes the IR as a trust gate for its distinct plain-CSS matcher/scoper. Svelte's carrier/CSS parser remains the compatibility owner until exact Svelte 5.56.3 error-code, offset, and read-past-close parity is proven. Style `v-bind()` usage is discovered through the same dialect-aware planner IR, then OXC-derived `expr_roots`/`roots_complete` remain the liveness facts consumed by `mark_bindings_used_in_style` and compile-input assembly.

`StyleSyntaxIr` retains positioned containment and balanced values without evaluating or compiling preprocessors. Imports, modules, plugins, guards, mixin/function arguments, and control expressions remain opaque-but-positioned.

Stylesheet parser mode is deterministic by dialect and structural tokens. CSS always uses brace grammar. Sass and Stylus use the layout-capable grammar, which also recognizes explicit braced blocks. SCSS and Less use brace grammar whenever the lexer emits any plain `LeftBrace`; only a brace-free source with an actual deeper-indented line pair uses layout grammar. Closing-brace indentation and other incidental formatting never select the parser. Selector trust folds every component descendant and functional-pseudo selector list; class/ID collection descends those lists and gates each component independently, so complete literal class components may still publish from an otherwise evaluation-dependent selector such as `&.active` or `:global(.a .#{$x})`. A textually certain `:deep`/`:global`/`:slotted` kind publishes independently of argument trust, while every class inside its argument remains subject to the same per-component gate. Ambiguous optional-syntax statements remain locally typed and diagnosed without recovering intact ancestor rules. A declaration may own a retained `StyleBlock` (for example, an indented Sass nested-property namespace); the IR sink never discards such a block.

**Module structure:**

```
style.rs              # Semantic style projection types and specificity computation
style_syntax.rs       # Five-dialect syntax-to-semantic projection
selector_match.rs     # Three-valued selector matching against template elements
template.rs           # Template element analysis, dynamic class extraction, :style CSS var extraction
```

**Key types:**

| Type | Location | Purpose |
| ---- | -------- | ------- |
| `StructuredSelector` | `style.rs` | Parsed CSS selector (compounds + combinators) |
| `CompoundSelector` | `style.rs` | Single compound: element, classes, id, attributes, pseudo-classes |
| `SelectorCombinator` | `style.rs` | Descendant / Child / NextSibling / LaterSibling |
| `MatchResult` | `selector_match.rs` | Three-valued: `Matches`, `MaybeMatches`, `NoMatch` |
| `DomQueryCallSite` | `types.rs` | DOM query call with parsed selector and spans |
| `StyleBlockAnalysis` | `style.rs` | Per-`<style>` block analysis with nested `CssAnalysis` |
| `AnalyzedCustomProperty` | `style.rs` | CSS custom property with name/value spans, var references, selector index |
| `CssVarReference` | `style.rs` | `var()` call with name, span, optional fallback (recursive) |
| `AnalyzedVarUsage` | `style.rs` | Regular CSS property using `var()` with property name and selector index |
| `CssVarManipulation` | `types.rs` | Script-side CSS variable manipulation via DOM APIs |
| `DynamicStyleVar` | `template.rs` | CSS variable set via `:style` binding in template |
| `StaticStyleVar` | `template.rs` | CSS variable set via static `style` attribute in template |
| `CssVarFlow` | `project_index.rs` | Cross-component CSS variable flow (definitions + usages + manipulations) |

**CSS Variable Analysis (three-block tracking):**

- **Style**: the balanced component-value IR projects `AnalyzedCustomProperty` (definitions with values/spans) and `AnalyzedVarUsage` (var() references). `extract_var_references()` handles nested var() fallbacks without evaluating values.
- **Template**: `extract_dynamic_style_vars()` extracts CSS vars from `:style="{ '--color': val }"`. `extract_static_style_vars()` extracts from `style="--color: red"`.
- **Script**: `try_extract_css_var_manipulation()` detects `el.style.setProperty('--x', val)`, `getPropertyValue('--x')`, `removeProperty('--x')`.
- **Cross-component**: `ProjectIndex.css_var_flow(name)` and `VerterHost.css_var_flow(name)` return `CssVarFlow` with all files defining/referencing/manipulating a variable.

**Selector matching algorithm** (`match_selector()`):

1. Match rightmost compound against target element
2. Walk left through combinators: `Child` checks `parent_index`, `Descendant` walks ancestor chain
3. Dynamic `:class` or component types → `MaybeMatches` (can't determine statically)
4. `:not()` inverts, `:is()`/`:where()` takes best match across alternatives

**Position encoding for CSS spans**: `CssAnalysis` spans (classes, IDs, selectors) are **SFC-absolute byte offsets**. `CssSource` is constructed with the style content origin, so syntax and projection spans are absolute from creation. Consumers use spans directly without adding any offset. `StyleBlockAnalysis.content_offset` is retained for documentation and slice operations.

## Analysis MCP Server (`verter_mcp`)

`verter-mcp` binary exposes Verter's full analysis, diagnostics, compilation, and scoring pipeline via MCP for AI agents. `VerterMcpServer` wraps `VerterHost` (with `AnalysisScope::LSP`), `Linter`, and `ActionEngine`. Tools auto-load via `ensure_loaded()`; template analysis triggers `ensure_template_analysis()` transparently. Cross-file tools iterate all loaded files (no `ProjectIndex` exposed from host). Scoring engine computes composite 0-100 quality scores from a11y, lint, template complexity, API surface, CSS health, and reactivity dimensions.

Both binaries (`verter-mcp`, and the LSP-decoupling twin `verter-mcp-server`) run the one shared entry body `verter_mcp::run::run`. The HTTP transport binds before its initial scan and announces the real bound port as a one-line stdout readiness record (`verter_mcp::readiness`, mirrored by `packages/vue-vscode/src/mcpServer.ts`); `--client-pid` arms the same `ClientProcessGuard` containment as the LSP. The VS Code extension spawns this standalone binary per LSP start attempt (`createMcpServerLifecycle`: awaited replacement, bounded crash respawn, provider-registration teardown on death) — see `docs/contributing/lsp-mcp-decoupling.md`.

## verter_semantic::analysis — Static Analysis Types

`verter_semantic::analysis` is the shared static-analysis surface consumed by `verter_session`, diagnostics, and tooling. Compilation crate owns lowering and codegen; `verter_session` projects compiler and workspace state into these semantic snapshots.

### AnalysisScope

Bitflags (`u32`) controlling which analysis passes run during file upsert.

**Script (bits 0-7)**

| Flag | Bit | Description |
| ---- | --- | ----------- |
| `IMPORTS` | 0 | Import declarations |
| `BINDINGS` | 1 | Variable/function/class declarations |
| `REACTIVITY` | 2 | Ref/reactive/computed classification |
| `MACROS` | 3 | defineProps/Emits/Model/Slots/Expose |
| `MACRO_TYPE_DEPS` | 4 | Cross-file type references in macros |
| `VUE_API_USAGE` | 5 | Track provide/inject/lifecycle/watcher calls |
| `EXPORT_SIGNATURES` | 6 | Per-export hashes for smart invalidation |
| `FUNC_RETURNS` | 7 | Analyze function return reactivity (for composables) |

**Template (bits 8-15)**

| Flag | Bit | Description |
| ---- | --- | ----------- |
| `TPL_COMPONENTS` | 8 | Component usages + prop expressions |
| `TPL_BINDINGS` | 9 | Which script bindings are used in template |
| `TPL_SLOTS` | 10 | Slot definitions + usages |
| `TPL_REFS` | 11 | Template ref attributes |
| `TPL_EVENTS` | 12 | Event handler bindings |
| `TPL_CONSTNESS` | 13 | Prop constness classification |

**Style (bits 16-19)**

| Flag | Bit | Description |
| ---- | --- | ----------- |
| `STYLE_CSS` | 16 | Full CSS analysis (selectors, classes, IDs) |
| `STYLE_VBIND` | 17 | v-bind() in styles |
| `STYLE_SCOPED` | 18 | Scoped/module metadata |
| `STYLE_PSEUDOS` | 19 | :deep/:global/:slotted |

**Cross-file (bits 24-26)**

| Flag | Bit | Description |
| ---- | --- | ----------- |
| `CROSS_RENDER_TREE` | 24 | Build render tree from template analysis |
| `CROSS_PROVIDE` | 25 | Provide/inject chain validation |
| `CROSS_PROP_CONST` | 26 | Prop constness optimization |

**Presets:**

| Preset | Flags | Use Case |
| ------ | ----- | -------- |
| `BUILD` | IMPORTS, BINDINGS, MACROS, MACRO_TYPE_DEPS, EXPORT_SIGNATURES, STYLE_VBIND, STYLE_SCOPED | Minimal overhead for compilation + smart invalidation |
| `BUILD_OPTIMIZED` | BUILD + REACTIVITY, VUE_API_USAGE, TPL_COMPONENTS, TPL_BINDINGS, TPL_CONSTNESS, CROSS_RENDER_TREE, CROSS_PROVIDE, CROSS_PROP_CONST | Build with cross-file optimization |
| `LSP` | All flags | Full analysis for completions, hover, diagnostics |
| `LINTER` | IMPORTS, BINDINGS, REACTIVITY, MACROS, VUE_API_USAGE, TPL_COMPONENTS, TPL_BINDINGS, TPL_SLOTS, TPL_REFS, TPL_EVENTS | Script + template for lint rules |
| `ESSENTIAL` | IMPORTS, BINDINGS, MACROS, MACRO_TYPE_DEPS, EXPORT_SIGNATURES | Script-only (legacy compat) |

### ScriptAnalysisSnapshot

Primary output of `build_script_analysis()`. Produced by a single OXC parse + AST walk.

| Field | Type | Description |
| ----- | ---- | ----------- |
| `imports` | `Vec<AnalyzedImport>` | All import declarations with source, bindings, spans |
| `bindings` | `Vec<AnalyzedBinding>` | Top-level variable/function/class declarations |
| `macros` | `Vec<AnalyzedMacro>` | Vue macro calls (defineProps, defineEmits, etc.) |
| `macro_type_deps` | `Vec<MacroTypeDep>` | Cross-file type references used by macros, tiered by structural position (`usage: MacroTypeDepUsage` — `Surface` = argument root / intersection-union arms / extends heritage / alias chains, missing ⇒ error; `Member` = top-level member annotation, missing ⇒ warning + `null` degrade). References nested deeper are never collected (runtime codegen does not need them) |
| `flags` | `AnalysisFlags` | Bitwise flags for O(1) queries |
| `exported_functions` | `Vec<AnalyzedExportedFunction>` | Non-SFC exported functions (composable analysis). Carries `name` / `is_default` / `params` / `is_async` / `composable` only. It carries NO return-type field: the declared return type's reactive-wrapper identity is a resolution decision answered at demand time from the lowered typed IR plus a package-backed route proof (`/type-resolution` → Reactive-wrapper demand), never from annotation text on this DTO |

**ReactivityKind**: None | Ref | Computed | Reactive | MaybeRef | Mutable

### TemplateAnalysisSnapshot

Populated after compilation by converting `RawTemplateData` from `verter_compiler`.

| Field | Type | Description |
| ----- | ---- | ----------- |
| `components` | `Vec<TemplateComponentUsage>` | Components used in template with props and slots |
| `binding_occurrences` | `Vec<TemplateBindingOccurrence>` | Script bindings referenced in template with spans |
| `defined_slots` | `Vec<DefinedSlot>` | `<slot>` elements defined in template |
| `template_refs` | `Vec<TemplateRef>` | `ref="foo"` attributes |
| `event_handlers` | `Vec<TemplateEventHandler>` | `@click`, `@input`, etc. |
| `elements` | `Vec<TemplateElement>` | Full element tree for linter traversal |
| `if_chains` | `Vec<IfChain>` | v-if/v-else-if chains for duplicate detection |
| `prop_definitions` | `Vec<AnalyzedPropDefinition>` | Props from defineProps |
| `emit_definitions` | `Vec<AnalyzedEmitDefinition>` | Emits from defineEmits |
| `comment_directives` | `Vec<CommentDirective>` | `@verter:disable`, `@verter:todo`, etc. |

### ProjectIndex

Aggregates file-level usage into project-wide indexes:

- **provide_index**: provide key → files that call `provide(key)`
- **inject_index**: inject key → files that call `inject(key)`
- **component_graph**: file → components it uses (forward edges)
- **component_reverse_index**: component name → files that use it
- **class_index**: CSS class name → files that define it
- **v_bind_css_index**: v-bind CSS expression → files that use it
- **custom_property_index**: CSS custom property → files that define it

### Data Flow

```
Vue SFC Source
    |
    v
verter_compiler::compile()
    |-- ScriptAnalysisSnapshot (from OXC parse during compilation)
    |-- RawTemplateData (spans, binding refs, component tags)
    |-- CssParsed* (v-bind spans, pseudo spans)
    |
    v
verter_session (conversion layer)
    |-- RawTemplateData --> TemplateAnalysisSnapshot
    |-- CssParsed*      --> StyleBlockAnalysis
    |-- Resolves import paths, populates resolved_canonical_id
    |-- Updates ProjectIndex with file usage
    |
    v
Consumers (LSP, build, linter) query snapshots + ProjectIndex
```

exec
/bin/zsh -lc "sed -n '1,220p' CLAUDE.md" in <repo-root>
 succeeded in 0ms:
# Verter

> **SUPERSEDED WHERE IT CONFLICTS — an architecture program is in flight.**
>
> The ratified Revision 11 architecture program lives at [`docs/arch/refactor/rev11/`](docs/arch/refactor/rev11/); its normative entry point is [`ORCHESTRATOR.md`](docs/arch/refactor/rev11/ORCHESTRATOR.md).
>
> **Precedence:** where this file and the Revision 11 plan disagree, **the plan wins**. The plan was designed to replace parts of this document, and the maintainer has ratified that precedence — see ruling R-3 in [`evidence/maintainer-rulings.md`](docs/arch/refactor/rev11/evidence/maintainer-rulings.md), which quotes both sides of the known conflicts.
>
> **How to read this file while the program runs:** treat it as an accurate description of how the code behaves **today**, and as authority for day-to-day operational matters — build and test commands, commit conventions, testing requirements, review discipline. Do **not** treat its architecture sections as authority over the program's target design; several describe structures the plan intends to change or remove. A rule here is not grounds to refuse a change the plan mandates.
>
> **If you are implementing a program block:** the plan's charters, contracts and ownership tables bind you. Follow them to the letter. Where implementing the plan appears impossible, record a deviation for maintainer ratification rather than substituting a local decision — an unrecorded deviation is far more expensive to unwind than a delay.
>
> This banner is removed when the program completes and the target architecture is described here directly.

Verter = a Vue compiler + Language Server Protocol (LSP) implementation. Converts Vue Single File Components (SFCs) to valid TSX (TypeScript type-checks them) and compiles templates to optimized render functions. Unlike Volar, Verter generates real valid TSX, not virtual files.

Hybrid Rust + TypeScript monorepo: Rust crates own carrier parsing, runtime and IDE code generation, the shared semantic session, and the LSP server (`verter_lsp` binary, stdio); TypeScript packages provide editor integration, TypeScript-provider adapters, protocol bindings, and bundler orchestration.

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

Guards: `verter_audit_no_upward_deps`, `audit_substrate_isolation`, `audit_observer_single_accessor`. The former single-engine `no_new_*` shrinking-ledger scanners retired with the deleted parser-owned `type_surface` engine; the single-engine invariant is now held structurally (the compiler consumes only the dependency-neutral macro DTO) plus the Vue macro boundary suite registered under "Vue Macro Semantic Boundary" in `CRITICAL_RULE_GUARDS`.

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
- The declaration-body **hot READ path is handle-native at the one migrated graph-backed site** (`lower_decl_body_to_node`) through `decl_body_hot_ref`; prepared declarations and `LoweredValueDecl` are fact+locator `NoTypeExpr`, and `LoweredTypeDecl.body` is content-free. The two former stored type-parameter `TypeExpr` pockets are CLOSED (the type-parameter-bound confinement block): `LoweredTypeDecl` is wholly `NoTypeExpr` — the stored full `type_parameters: Vec<TypeParam>` is gone; the `narrow_type_parameters` mirror (name + ordinal + content-free bound locators) is the sole stored authority, consumed by the prepared-decl builder and the external frontier (which content-free re-anchors the mirror's bound slots to the frontier symbol, preserving `export default`), while the locator/binder deref uses the mirror for ordinal/name/bound-presence authority and re-borrows bound CONTENT + the full sibling frame lease-only from the retained snapshot (`transient_type_parts`). `TypeParamBinding` is likewise shrunk to the content-free `(name, ordinal)` fact pair (`NoTypeExpr`): its `<script setup generic="…">` bounds are re-borrowed at query time through ONE artifact-local transient producer over the pinned `IndexedReady` and lowered by ONE dispatch helper shared by both content readers, with a missing/stale re-borrow failing as a typed cache-suppressed miss, never a bound-free fabricated binder. The residual-reader ledger is not a terminal-zero mechanism: `ProducerLowering` is permanent transient ingress, the five `GraphFreeDto` rows remain named until the separate producer-boundary-confinement cutover, and a hot consumer must never take `HotTypeRef → TypeExpr → semantic decision`.
- Navigation stays narrower than expansion: walking `A['c']['full']['bar']` navigates intermediate hops and expands only the terminal requested projection unless limited normalization is required to continue.
- Generic substitutions are semantic meaning: navigation/expansion operate on instantiated types; cache keys include the relevant substitutions/type arguments.
- Navigators stay non-owning (choose the next hop, non-owning normalization only); reusable semantic work enters through the shared query API, not a private drill-down path. The shared semantic layer is keyed by semantic query identity and stores immutable semantic data or ids — never borrowed AST pointers or retained parser arenas.
- Completion fence: top-level live-host results record touched dependency facts, revalidate before publish, retry at most 3 times on mid-flight changes; never warm shared caches with torn provisional results; cancelled, superseded, interrupted, budget-exceeded, or partial results are never promoted warm.
- Waiters on in-flight work block cooperatively, never busy-spin; same-path recursion never self-awaits.
- Cache population is path-independent (same result from different entry points → same shared entry); broader successful results may backfill only the narrower entries they actually satisfied; narrower results must not pretend broader work is cached.
- Final payload caches hand out immutable `Arc` values; any backend preserving concurrency, size bounds, validation semantics is fine.

Guards: `audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries`, `macro_impacting_constructs_fail_lowering_not_silent_skip`, `indexed_ready_publish_lowers_zero_decl_bodies`, `resolve_unrelated_symbol_lowers_only_demanded_decl`, `lazy_decl_body_singleflight_lowers_once`, `no_indexed_ready_eval_env_or_type_decl_body_storage`, `emit_parse_facts_never_hashes_decl_bodies`.

**Project-global cache (final state):** `VerterHost` owns a single `ProjectTypeStore` accessed via `.project_type_store()` — the sole shared cache graph: `FileArtifactStore`, `AnalysisReadyDb`, the rehomed `RouteDb`, `OwnerImportSurfaceDb`, `ComponentMetaResultDb<ComponentMetaAnalysis>`, `MaterializeStructureDb`, `SemanticGraphStore` (which also owns the Vue macro resolution artifacts — the former `ResolvedNamedTypesDb`), `ShapeCacheDb`, `FlowSliceStores` (the flow-return substrate's per-function graph/slice artifact stores), and the `IntrinsicRegistry`. `IndexedReady` is the single canonical post-parse artifact (the former `ModuleFactsDb` is retired). Validated cache writes record a `ReadSetSignature.facts` fact signature (the path-precise fact-tracer observation set) — the sole cache-validity rail, revalidated against the live `StoreView` on every warm hit. The `StoreViewValidationToken` is the complete reuse/validity oracle; the singleflight LANE identity is the narrower `external_supersession_fingerprint` (reuse-oracle = full token; lane-identity = external fingerprint). See `/host-session` (store-view token dimensions, token-advance rules, lane identity, singleflight, `RequestStoreView`/`CanonicalCompletionOverlay`, handle-backed dims), `/component-meta` (`get_component_meta` final-result flow, `resolve_owner_direct_import`, `materialize_component_meta_structure`, the `ShapeCacheDb` per-member route, `reduce_published_field_types` / sink-private `reduce_field_value_node`), `/type-cache-architecture` (admission, retired split stores), and `/type-resolution` (`execute_cooperative` dedup, `SemanticNodeData::VueMacroElements` hot path, `IntrinsicRegistry::lookup`).

### Canonical Dependency Cache Rule (CRITICAL)

Host-backed type/import resolution treats the canonical file ID as the cache identity. Load and parse each dependency at most once per canonical ID per workspace content generation. Cache the parsed state, the shallow declaration index plus lazy declaration-body memo, symbol/export tables, and prepared declarations together. Later lookups hit cached maps — never rewalk the AST. VFS is the authority for file-change invalidation. Concurrent cold requests to the same file collapse onto one materialization path. Changes land as one clean cutover, no dual-path shims.

Guards: `host_upsert_performs_no_reverse_dependent_eviction`, `host_upsert_reverse_dep_eviction_scanner_discriminates`, `import_route_writer_guard`.

See `/type-resolution` skill for the full rule set (invalidation semantics, route caches, prepared declarations, cross-owner reuse, negative caching, the concrete performance contract).

### Cache Architecture (CRITICAL)

The fact-based cache architecture splits cache keys across five orthogonal env-hash dimensions (`parse_env_hash`, `resolve_env_hash`, `type_env_hash`, `lib_env_hash`, `project_identity`). Each cache layer keys only on the dimensions it actually depends on (R21 scoping rule — a single bundled `project_config_hash` is forbidden). `lib_env_hash` enters a key only when the value depends on lib data: `ResolvedImportFacts` does NOT include it; `RouteDb`, typed-IR resolve, `MaterializeStructureDb`, `SemanticGraphStore`, `ComponentMetaResultDb` DO.

Two cache families: **content-addressed artifact caches** (`FileArtifactStore`, `ResolvedImportFacts`, typed-IR resolve, `MemberSemanticFactStore`, `MemberDisplayFactStore`, `ModuleAugmentationIndex`) carry `content_hash` or `parse_stable_hash` in the key; **query-identity caches** (`RouteDb`, `MaterializeStructureDb`, `SemanticGraphStore` query nodes, `ComponentMetaResultDb`) exclude version hashes from the key — concurrent variants coexist as candidates in one slot, with version rooting on the cached value (the structural + semantic-graph caches — `MaterializeStructureDb`, `SemanticGraphStore` memo, `ShapeCacheDb` — root via `ReadSetSignature.facts` + `self_root_canonicals`; `RouteDb` via its value-side `ValidatedFactCache` fact signature; `ComponentMetaResultDb` via the owner whole-hash candidate discriminant + `ReadSetSignature.facts`). Cache keys never include `fact_dep_signature`. The `MaterializeStructureDb` subject is the content-free `MaterializationCacheKey` (a `ResolvedDeclSlotIdentity` slot + projection/policy/mode axes + `resolve_env_hash`), NOT a graph-instance `SemanticNodeId` — the per-thread recursion identity `MaterializeRuntimeKey` is a separate, non-cache key; a root-less anonymous subject keys no slot (uncached). The materialization cycle gate is the sealed `SemanticQueryKey::ClassifyMaterializationCycleGate` family (`MaterializationCycleGateKey`: `ResolvedDeclSlotIdentity` root slot + `parse_env_hash` + `resolve_env_hash`, fixed `StructuralTransit`/`Skeleton`/empty-args axes) — the SOLE authority for "does this declaration transitively reach a cycle through a complex helper surface"; only its `Decided` outcome admits (`LegacyFallback` always suppresses), and consumers branch on the carried `Stop | Continue` verdict from both arms.

Family-memo slots (`SemanticQueryKey::Instantiate.base` / `ResolveMacroPayload.owner`, mirrored on `FamilyKey`) are the env-bearing, content-free `ResolvedDeclSlotIdentity` (R6 — content/version hashes and the versioned `DeclIdentity` are forbidden in any derived-`Hash` query-identity key; the live whole-hash is re-sourced at value-compute time, never carried in the key). A warm hit requires TWO independent gates (§3.4): `cached_satisfies` over a RECORDED materialised `(path, point)` the candidate's compute actually produced — never the candidate's nominal slot/mode, never enum rank — AND per-candidate `ReadSetSignature.validate_with_self_roots` against the caller's live view. Backfill clones only recorded materialised points, directionally gated (the `Shallow → Navigate` clone is lattice-unsound). `validated_at_generation` is recency metadata only, never a validity oracle. Family-slot retention is bounded PER FAMILY (`U3.ADAPTIVE_FAMILY_RETENTION`): each `FamilyKey` declares an exhaustive `candidate_cap()` (floor 4; the inference/substitution-heavy live families `Instantiate` / `TypeOf` / `Conditional` / `MappedType` hold 8); at the cap a publish evicts a candidate INVALID against the publishing caller's stable store view FIRST (snapshot/validate/reacquire OUTSIDE the `entries` mutex, `admission_seq` identity recheck under it), then the least-recently validated-hit candidate (a valid hit promotes to the back); a new cacheable candidate is ALWAYS admitted after local eviction, and same-discriminant re-publish replaces in place as freshest. The process-wide candidate-memory ceiling + typed non-admission are deferred full-`U3.CACHE_FACT_MODEL` work, NOT live. See `/type-cache-architecture` for the full key/context composition (`InstantiateContext`/`MacroPayloadContext` per-key contexts, per-family `candidate_cap()` candidate semantics, non-file-base rooting).

`FileArtifactStore` is the authoritative per-file storage layer, keyed by `(canonical, content_hash, parse_env_hash, parser_version, file_language_id)` — `file_language_id` is the file's `FileLanguage` row (the per-file classification dimension of artifact identity, so a framework-capability flip misses exactly the affected files' artifact slots without touching the global `parse_env_hash`). The overlay-aware `augmentation_index` (module-augmentation inverse lookup) lives on the same store. See `/type-cache-architecture` for the full key composition, `file_language_id` producer wiring, `AugmentationTargetKey`/`AugmentationPopulation` semantics, and the `parse_stable_hash` definition.

Cache runtime hard rules — three always in force: cache correctness is read-side authoritative; `ReturnOnly` (overflow, budget exhaustion, cancellation, generation supersession, incomplete self-rooting, unresolved provenance) never publishes entries, reverse-index metadata, or persistent artifacts; overlay/session results never populate base-only or persistent caches. Full 20-rule list: `/type-cache-architecture` → Cache Runtime Hard Rules.

Guards: `cache_satisfaction_is_materialized_point_not_nominal_demand`, `cache_satisfaction_requires_path_exact_not_prefix`, `backfill_writes_only_recorded_materialized_points`, `no_off_store_host_caches`, the `r6_*` cluster, plus the four migrated-query-identity-key guards in `tests/cases/g_cache/r6_r21_query_identity_keys.rs` (`component_meta_result_key_*`, `route_name_key_*`/`barrel_surface_key_*`, `materialization_cycle_gate_key_*`, `materialization_cache_key_*`) — full list in `CRITICAL_RULE_GUARDS`.

See `/type-cache-architecture` skill for the full rule set (R1–R31, two-fact `MemberPresence`/`Member` model, multi-candidate substrate, signature-overflow contract, module augmentation completeness, heuristic-cache-semantics prevention, exact policy identity) and `docs/arch/fact-based-cache.md` for the per-field audit table + per-cache-layer key composition.

### Macro Type Traversal Rule (CRITICAL)

When resolving cross-file macro types (`defineProps<T>()`, `defineEmits<T>()`, component-meta deep expansion, etc.), only follow the import graph reachable from the requested type's declaration graph. There is one shared cross-file type resolver with five query modes: `Identity`, `Navigate`, `Shallow`, `Expanded`, `Skeleton` (see `/type-resolution` → Query Mode Contract).

**Macro resolution is one shared path, not a per-macro engine.** Every macro (`defineProps` / `defineEmits` / `defineOptions` / `defineSlots` / `withDefaults`) and every imported `.vue` component surface resolves through exactly TWO steps: (1) resolve ONE type via the shared typed-IR five-mode dispatch — the generic-parameter type (`define*<T>()`) OR the object-argument type (`define*({ ... })`); `withDefaults` resolves the props payload type plus the defaults-object type and merges; `.vue`-component imports resolve the synthesized `$props` / `$emit` / `$slots` / expose surface recursively through the same dispatch (the hardest case — apply EXTRA caution: it is exactly where rule violations cause the worst hangs); no macro-specific engine, no per-surface walker, no eager element resolver. (2) Normalise per kind — a thin transform, NOT a resolver (per-kind field rules: `/type-resolution` → Macro Type Traversal Rule). A macro/import that resolves through anything else, or flattens a full surface eagerly before the consumer demands it, is a rule violation — collapse it into `shared_resolve(type) + normalise`.

`Skeleton` is the BFS / generic-helper traversal mode: unbound type parameters stay `TypeParam` shells so Conditional branches do not collapse to `never`. Path projection is path-precise: intermediate hops run in `Navigate`, the terminal hop runs in the caller's mode; non-contributing intersection arms are ignored (not rewritten to `never`); open conditionals distribute the remaining path into both branches; closed conditionals reduce immediately. Do not walk unrelated imports. Do not treat plain imports as implicit exports. Cache discovered symbol mappings and barrel hops.

**TS-first resolution priority:** TypeScript types always take priority over JavaScript files. Use `effective_target()`: `.d.ts` > `.d.cts` > `.d.mts` > `.ts` > `.tsx` > `.js` > `.jsx` > `.cjs` > `.mjs`.

**Owned resolution is bounded by `workspace_root`:** `node_modules` and package `#imports` ancestor walks stop at `IdeProjectConfig.workspace_root`.

Guards: `root_conditional_still_distributes`, `no_macro_string_heuristics_in_resolver_core`, `no_text_based_macro_surface_projection_helpers`, `no_role_inference_from_name_suffix`, `no_pick_or_omit_string_prefix_check` — full list in `CRITICAL_RULE_GUARDS` (the retired `no_new_*` shrinking-ledger scanners left with the deleted `type_surface` engine; the compile-boundary half of the invariant is guarded under "Vue Macro Semantic Boundary").

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

Facts rail: the cold stitch observes one `FactKey::ModuleAugmentationIndexShape` fingerprint plus one `FileWholeHash` per contributing file and records `self_root_canonicals = {base} ∪ {augmenters}` — a content edit to ANY contributor misses the warm read; torn/partial routes through `ReturnOnly`. Query keys stay content-free (R6). The index is OVERLAY-AWARE (`AugmentationPopulation {Base, Session(overlay-set fingerprint)}`): overlay augmenters NEVER poison the base index and NEVER cross sessions, and there is NO base-only session assert on the augmentation-index surface — a session view is accepted under `Session` scope.

Guards: `session_overlay_augmenter_isolated_from_base_index`, `session_overlay_augmentation_isolated_from_base_meta`.

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

### Compiled-Output Conformance (CRITICAL)

Official-framework compiler conformance is behavioral plus structural/helper-topology parity, not raw-byte identity. For Vue VDOM/Vapor, Svelte `svelte/internal/*`, SSR/client, and future runtime backends, compare emitted output by observable behavior plus parsed/token-normalized structure: imports, helper families, helper call sequence where order is semantic, memoization/reactivity/effect topology, DOM/hydration template topology, class/style/attribute normalization, prop/property routing, event delegation, and diagnostic/reject ordering.

Cosmetic JS carrier formatting is not a finding: indentation, line breaks, non-semantic comments, intra-expression whitespace outside literals, and behavior-preserving redundant parentheses may differ from the official compiler. Directive, pragma, license/preserve, source-map/sourceURL, TS-directive, JSDoc, and other tool-consumed or framework-significant comments remain in contract. Generated local identifier spellings are waived only when the backend oracle implements scope-aware alpha-equivalence for private, non-observable bindings; otherwise identifiers are structural. Literal payload bytes, static HTML/CSS/SSR strings, public/exported or source-authored names, sourcemap mappings, diagnostic text/codes/order, and any framework-defined observable format remain in contract.

Do not build or route production compiled-output emission through JS printers, re-printers, redundant-paren canonicalizers, or any machinery whose role includes mimicking the official compiler's cosmetic JS carrier formatting. Direct-emission helpers may emit syntax-required tokens, including required parentheses for valid JavaScript expression/statement shape, but they must be scoped to semantic/syntactic correctness and covered by behavioral/structural tests rather than official cosmetic byte parity. Emit correct code directly and make conformance oracles structural for cosmetic categories: a cosmetic-only diff passes; a behavioral or structural divergence fails.

The positive structural-discriminator guard currently covers Svelte client only (Vue VDOM/Vapor and SSR/client positive oracles are tracked follow-ups); the re-printer guard is cross-backend negative coverage. See `/compiler-codegen` for the tracked guard gap.

Guards: `svelte_structural_conformance_discriminates_cosmetic_from_behavioral_diffs`, `no_compiled_output_cosmetic_reprinter_path`.

### Fallthrough / Root Inheritance (CRITICAL)

The shared Rust pipeline owns all fallthrough and root inheritance semantics. `verter_semantic::analysis` extracts root reachability facts only. `verter_session` owns the single inheritance resolver, recursion, conditional branch composition, generic propagation, caching, and final metadata projection.

Key rules: `inheritAttrs: false` → no inherited surface. Single native root → intrinsic attrs minus declared props/events. Single component root → recursive propagation. Conditional branches → exact union. Cycles → unresolved branches. `class`/`style` are never consumed.

Guards: `fallthrough_recomputes_from_runtime_subnodes_after_top_level_node_clear`, `fallthrough_runtime_reuse_survives_host_cache_clear`, `fallthrough_reuses_root_follow_after_branch_union_node_clear`.

See `/component-meta` skill for the full semantic rules, public contract, authority chain, and key files.

### Component-Meta Shallow-By-Default Rule (CRITICAL)

Types and properties are ALWAYS published shallow at the projector surface UNLESS the consumer explicitly walks the path. This is the single architectural invariant the projector pipeline (`meta_resolve::projectors::reduce_published_field_types` + sink-private `reduce_field_value_node`) enforces.

Concrete contract:

- Plain alias references (`type Foo = ...`) — published prop type stays `TypeExpr::Ref { name: "Foo" }`. Consumers re-resolve `Foo` through the registry on demand. The projector does NOT eagerly inline the alias body.
- `Pick<Foo, "bar">` — materialises ONLY the `bar` member of Foo. Other Foo properties stay shallow (path-precise). Built-in utility types (`Pick`, `Omit`, `Required`, `Partial`) behave identically to a userland implementation referencing the same keys.
- **Carrier-preserving decl-body lowering.** Under `Shallow` (as under `Navigate` / `Skeleton`), decl-body lowering interns `DeclRef` / `InstantiationRef` carriers for member-value type references — including ALL builtin utilities — and never executes `ResolveDecl` / `Instantiate` eagerly; eager lowering-time execution is `Expanded` / `Identity` only; materialisation enters exclusively through the demand points (PathWalker hops, the shallow-surface synthesiser's carrier unwrap, closed object-filter surface reads, the relation/conditional oracle). Eager Shallow member-value lowering was the `Table.vue` storm: 94.3% of all budget charges were `Instantiate(StructuralTransit:Shallow)` recursion across the transitive TanStack decl graph.
- **Open key domain ⇒ shallow carrier (L1) — route/mode-independent.** TWO families stay shallow carriers at EVERY entrance, in every mode, and open-OR-UNKNOWN (including traversal-budget exhaustion) preserves the carrier instead of falling through into Expanded materialisation: (1) an object-filter utility (`Pick`/`Omit`) whose enumeration domain is OPEN or undecidable (`Pick<PropsBase<T>, …>` over the SFC's open `generic="T"` stays `Pick<…>`); (2) a mapped type `{ [K in S]: V }` whose produced surface still depends on an unbound OUTER generic (a CLOSED-key/open-VALUE mapped enumerates its keys path-precisely with shallow values). Closed sources still materialise the requested keys path-precisely. A carrier-stopped `Pick` at a SURFACE-enumeration demand (heritage arm / macro props-slots surface) still publishes its CLOSED output-key selection from the source's enumerable arms via the shallow walker's `Pick`-carrier enumeration — the source is never whole-materialised and `Omit` (source-dependent-open output keys) stays a carrier; zero-member surface collapse was the nuxt-ui ContentSearch/DropdownMenuContent bug. Typed-IR only, no string matching. The carrier-stop is the PRIMARY defense for the open-generic class; the per-request projection budget (`request_budget.rs`) is an ARMED-by-default runaway fuse (`projection_op_budget == 0` ⇒ effective cap 2000) whose trip returns `BudgetExceeded` as a genuine partial — refused warm admission, the no-poison invariant. Publication demand is `Navigate`-only on the projector/registry macro surfaces: a full `get_component_meta` records ZERO `Published(Expanded)` projection contexts; `Table.vue` and `ChatMessages.vue` are COMPLETE corpus members with un-ignored green trackers (`table_resolves_complete_and_warm`, `chat_messages_resolves_complete_without_false_partial`, `chat_messages_resolves_without_timeout`). The FULL authoritative spec — entrances, owner predicates, the per-argument position-sensitive key-domain rule, the tri-state conditional oracle, per-utility output-key semantics, mapped family composition, OPEN/CLOSED definitions, memoization, invalidation, the `TypeOf` demand rails, and the four named current scoped exceptions — lives in `/type-resolution` → Open-Key-Domain Carrier-Stop (L1).
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


exec
/bin/zsh -lc "sed -n '221,440p' CLAUDE.md" in <repo-root>
 succeeded in 0ms:
See `/component-meta` skill for the full policy, resolver rules, and cache contracts.

### Typed-IR-Only Resolver Rule (CRITICAL)

The native component-meta / typeinfo type resolver — analyzer → projector → registry → policy → materialiser — drives semantic decisions exclusively from the typed IR (`verter_semantic::analysis::type_expr::TypeExpr` on Rust, `TypeDescriptor` from `@verter/type-ir` on TS). Forbidden inside that pipeline:

- Source slicing, regex against type text, hand-rolled type-text splitters (`split_top_level_*`, `find_top_level_char`, `extract_pick_slot_bindings`, `extract_string_literal_name`, `splitTopLevelTypeOperator`), `starts_with("Pick<")` shape sniffing, and the synthesise-then-reparse pattern (`format!(...).parse_type_annotation(...)`). Walk the typed IR instead.
- `parse_type_annotation` anywhere except JSDoc tag-type payloads — the single explicit text exception: `{Type}` payloads inside JSDoc tags are inherently text, parsed via the dedicated JSDoc path only.
- Parsing back raw / display strings (`Analyzed*Field.type_annotation`, `ExpandedField.raw_type`, `ResolvedLocalType.expanded`, `PropMeta.rawType`) — display-only passthroughs. The JS compat layer (`@verter/component-meta/compat`) reads `prop.type` (`TypeDescriptor`) for every semantic decision; `prop.rawType` must not feed any `looksLike*`, `extract*`, `normalize*`, `split*`, `strip*`, `prefer*`, `shouldPrefer*`, or `repairOpaque*` branch.
- Substring path classification (`"/node_modules/"`, `"\\node_modules\\"`) — use `ResolverContext::workspace_is_package_backed(canonical_id)`. That predicate is the single structural authority for workspace-ownership classification, and it is what the live decision sites call directly (`component_meta_materialize.rs`, `framework/script_facts.rs`, `host_manage/jsdoc_resolve.rs`, `meta_resolve/graph_predicates.rs`, `meta_resolve/materialize/field_types.rs`, `meta_resolve/projectors/output_sink.rs`, `project_semantic_dispatch/raise.rs`/`walk.rs`, and others). Workspace-owned is its complement — there is no separate `workspace_is_workspace_owned` predicate.
- Name-suffix role inference (`name.ends_with("Props")` / `"Emits"` / `"Events"` / `"Model"` / `"Slots"`). Type-role classification is structural, not nominal: a type is a prop/emit/model/slot type because a Vue SFC macro (`defineProps`, `defineEmits`, `defineModel`, `defineSlots`, `withDefaults`) consumes it — read from `AnalyzedMacro.kind` / `parsed_type_argument` / `type_references` on the analyzer snapshot.

OXC is a syntax/lowering front-end only and never resolves types at query time. Macro/JSDoc producer fields still lower at their producer boundary via `lower_ts_type(ts_type, source)` (the producers emit content-free locators — `Analyzed*Field` carries `payload: Option<MacroPayloadLocator>`, `ResolvedLocalType` carries `shape: ResolvedLocalShape`; only display-only text rides alongside, surviving all caches); top-level declaration bodies lower LAZILY through the scheduler-retained parse snapshot (`DeclBodyMemo` → `DeclLoweringService`) and return owned typed IR before dispatch/reducers ever see them — no raw-string reparsing, no OXC resolver path. For the hot read surface the `decl_body_hot_ref` accessor mints a `HotTypeRef` handle over the `Instantiate` query result (`build_instantiate`'s post-processed node, produced via the resolving-lowerer body-shape helper `lower_decl_body_with_provenance`); the handle is NOT a re-lowering — bodies still lower to typed IR, and `DeclBodyMemo` records are fact+locator content-free end to end (the former memoized `LoweredTypeDecl.type_parameters: Vec<TypeParam>` pocket is CLOSED — the narrow mirror is the stored authority; bound content re-borrows lease-only). If a new requirement appears to need text manipulation inside the resolver, fix the producer (lower the right OXC node, store the right typed field, extend `@verter/type-ir` with a missing variant) rather than reparsing or pattern-matching on text.

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

Guards: `typeinfo_graph_taxonomy` (`crates/verter_session/tests/cases/g_block/typeinfo_graph_taxonomy.rs` — proto/TS oneof parity), `typeinfo_proto_ts_freshness` (`crates/verter_protocol/tests/cases/typeinfo_proto_ts_freshness.rs::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output` — regenerates the TS bindings via the workspace `buf` and `oxfmt` binaries and byte-compares), `request_kind_payload_parity` (`crates/verter_audit/tests/cases/request_kind_payload_parity.rs`), `typeinfo_request_validation` (`crates/verter_session/tests/cases/g_type/typeinfo_request_validation.rs` — closed-set schema-version + exhaustive structured-expression coverage), `typeinfo_wire_surface_guards`, `typeinfo_graph_contract_guards`, `typeinfo_request_contract_guards`, `typeinfo_audit_contract_guards`.

### Cross-Platform Portability (CRITICAL)

The codebase MUST build, test, and materialize on macOS, Windows, AND Linux. Platform-assuming code is a defect, not a nit.

Guard-enforced — `tracked_paths_are_portable` (`crates/verter_session/tests/cases/tracked_paths_are_portable.rs`) enumerates `git ls-files -z` and enforces: valid UTF-8; no NTFS-illegal characters (`< > : " | ? * \` plus control chars); no trailing dot or space; no reserved device basenames (`CON`/`PRN`/`AUX`/`NUL`/`COM1`–`COM9`/`LPT1`–`LPT9`, with or without extension, plus `CONIN$`/`CONOUT$` — the `$`-suffixed forms only); no case-insensitive path collisions (lowercase-fold approximation of NTFS/APFS folding, not the exact filesystem fold tables); ≤200-byte relative paths.

Review-enforced (the guard does not cover these):

- Sanitize generated on-disk names (e.g. `blake3:<hash>` → `blake3-<hash>`) — logical identifiers are unconstrained; only the on-disk boundary is. The guard only sees tracked paths, so it catches a generated name once committed, not at generation time.
- Build paths with `Path`/`PathBuf`/`Path::join` — never string concatenation with hardcoded `/` or `\`.
- Byte-equality comparisons over checked-out text normalize line endings (CRLF ↔ LF) or compare as text — never raw bytes embedding EOL.
- OS-specific binaries (`tsgo`, `.exe` suffixes) are discovered platform-aware, never via a hardcoded per-OS name.
- Temp and cwd paths come from std abstractions, not literal paths.

Guards: `tracked_paths_are_portable`.

### Anti-Binary-Growth Integration-Test Layout (CRITICAL)

Each crate exposes AT MOST one `tests/main.rs` integration-test binary; extra cases live under `tests/cases/` and are wired through `main.rs`. A second top-level `tests/*.rs` auto-becomes its own test binary and re-balloons the gate, so it is forbidden unless EXACTLY allowlisted. The only sanctioned exceptions are genuine "needs a separate test process" cases (process-global state that must be isolated): `verter_session` `allocator_canaries` (a counting `#[global_allocator]`) and `verter_lsp` `lsp_audit_trace_out_env_var` (a process-global env mutation). The allowlist (`scripts/integration-test-layout-allowlist.json`) is the single source of truth shared by both guards, is EXACT (package + target + repo-relative `src_path`, no globs/prefixes), and is STALE-FAILING — an allowlisted target that no longer exists in `cargo metadata` (or whose `src_path` moved) FAILS the guard.

Dual guard: the fast-fail CI Node check `scripts/check-integration-test-layout.mjs` (runs before the Rust gate) and the in-gate Rust mirror (`crates/verter_session/tests/cases/integration_test_layout_guard.rs`), both reading the same allowlist.

Guards: `integration_test_layout_is_consolidated`, `layout_checker_discriminates_stray_and_stale`, `allowlist_is_the_known_standalone_targets`.

### Framework Adapter Substrate (CRITICAL)

Multi-framework component support is ONE shared adapter substrate, not a per-framework semantic fork. `verter_session::framework` owns the `FrameworkAdapterRegistry` (built once at `VerterHost` construction), the per-adapter `FrameworkAdapterDescriptor` (identity, supported surface kinds, carrier language, the `VirtualFileNaming` column), the facts/carrier-only `FrameworkAdapterCtx`, the `ComponentDefaultSynth` seam, and the two-pass script-fact seam. Vue is the REFERENCE adapter — re-housed as a true plan/normalize adapter (`VueFrameworkAdapter` + the relocated `vue_exec` resolution delegates), NOT a privileged hardcoded path.

Closed-contract rules:

- **One audited wire entry, validation-first.** `VerterHost::resolve_framework_surface_with_audit(TypeInfoGraphRequest)` is the SOLE entry for the `GRAPH_OPERATION_FRAMEWORK_SURFACES` operation. It runs `validate_type_info_graph_request` FIRST (op/payload-arm match, schema echo, the nested framework-surface validator) — a malformed envelope returns the typed wire `error` arm BEFORE any registry lookup or semantic dispatch. A bare-inner-request entry is forbidden. The operation rides the EXISTING typeinfo graph envelope, and its current `FrameworkSurfacePayload`/embedded-`SemanticTypeGraph` shape is PROVISIONAL — an interim wire pinned today, NOT a permanent "no schema change" guarantee. The hard gate `S5.B11/B12 → U8` was landed ahead of order, so U8 still OWES the retag of `FrameworkSurfacePayload.graph` to a `TypeInfoGraphPayload` carrier, the `SemanticTypeGraph.schema_version` bump, and reserving the old field per the Typeinfo Wire Contract (CRITICAL) above; until U8 lands this wire stays pinned but is not final. Guard `framework_surface_wire_executor_validates_first`.
- **Registry dispatch, no privileged framework branch.** The executor interns `selector.framework_adapter_id`, looks up the registry (unknown id ⇒ typed `MalformedPayload`, NO new error variant), and dispatches to the adapter. Every wire `FrameworkTag` maps to a registered adapter OR an explicit `TagDisposition` row (`DeferredVertical` / `OutOfScope`); a tag's existence is NOT a support guarantee — support is asserted only by a registered adapter and surfaced per-request via `FrameworkSurfaceKindStatus`. Guard `framework_registry_complete` (+ the `framework_surface_executor` integration suite).
- **Closed plan/resolve/result vocabulary.** The adapter PLANS demands (`plan_surfaces` ⇒ closed 4-variant `PlannedDemand` — `MacroPayload` / `PathProjection` / `ShallowSurface` plus the Svelte arm `SvelteSurface`; no `Custom`/`Raw` arm, no source text / OXC handles / raw `SemanticQueryKey`s) and NORMALIZES resolved data (`normalize`); it holds NO resolve entry point. The executor resolves each `PlannedDemand` through the module-private `ExecutorResolveCtx` (EXHAUSTIVE match, no wildcard) THROUGH the one shared type-resolution engine — it plans, dispatches, and encodes; it is never a second resolver. Per-kind status maps DIRECTLY onto `SUPPORTED`/`PARTIAL`/`UNSUPPORTED` via the typed `ResolvedOutcome` (a supported-empty kind stays distinct from an unsupported kind). The first `SemanticTypeGraph` encoder (`graph_export`) is a pure ZERO-DISPATCH shallow projection of resolved data — named refs mint `GraphSymbolNode` + `GraphReference{symbol_id}`, structural unencodables degrade to `GraphOpaque`, never a fabricated ref and never a re-resolution.
- **Facts/carrier-only adapter ctx.** `FrameworkAdapterCtx` exposes EXACTLY two ops — `carrier_for::<T>` (the adapter's typed parse carrier, `None` for a carrier-less adapter — never a forged token) and `script_facts_for::<T>` (resolved script-fact evidence on demand: exact, partial, unavailable, or not applicable). It never resolves types, indexes a file, runs OXC, calls `ProjectSemanticDispatch`, or reads a `StoreView`. Guard `framework_adapter_ctx_closed_surface`.
- **Two-pass script-fact seam.** The syntax-capture half (`verter_semantic::analysis::framework_facts`) captures candidates from the live OXC program — SYNTAX-ONLY (may touch OXC + `lower_ts_type`, MUST NOT resolve imports or read capability bits; guard `script_fact_capture_is_syntax_only`). The session mints an exact candidate inventory, including exact-empty, only when its capture parse completes without syntax diagnostics; a recovered parse yields positive-only candidate observations, returns partial evidence, and never warms either script-fact store. Cache absence means “not computed.” The resolved-validation half (`framework/script_facts`) drives provider `validate` on demand over neutral resolved-import + capability data and preserves exact (including exact-empty), partial, unavailable, and not-applicable evidence. Resolved-fact publication accepts only producer-minted exact evidence under the fact-rail + strict-same-generation gate with `SignatureAdmission::Cacheable`-only publication (overflow ⇒ `ReturnOnly`, no warm); partial/unavailable results remain cold. Partial payloads have no whole-payload accessor: Svelte consumers can visit explicitly named conservative positive observations, while a resolution-only partial can separately expose its producer-proven exact syntax facet. Svelte facts keep syntax-owned facts (including sealed `ExactSveltePropsCalls`) separate from resolution-owned `Snippet`/dispatcher provenance, so resolution failure cannot erase exact script geometry. An EMPTY active-provider set is byte-identical zero-cost (Vue does NOT move onto the seam). The `ActiveProviderIndex` is the shared gate authority. Guard `script_fact_providers_zero_cost_on_miss`. The framework-surface result caches (`FrameworkSurfaceStore` / `FrameworkScriptCaches`) are fact-validated today but live on the framework registry rows, NOT the single `ProjectTypeStore` — they are PROVISIONAL off-store caches to be consolidated onto `ProjectTypeStore` (and given true singleflight) at U10.
- **Parse-domain component-default synth.** `ComponentDefaultSynth` synthesises a component's default-export value symbol from PARSE-DOMAIN inputs only (macros + syntax-capture candidates); it never names the resolved-validation fact types. Registry-dispatched at the shallow-analysis injection points by the file's resolved language. Guard `component_default_synth_parse_domain_only`.
- **Generated virtual-file naming is descriptor-owned.** The `VirtualFileNaming` column is the single authority for an adapter's IDE / API / testing-API / sidecar suffixes; the committed TS mirror (`packages/language-shared/src/virtual-file-naming.generated.ts`) is rendered from it and byte-pinned. Guard `virtual_file_naming_ts_freshness`.
- **No re-export shim for relocated Vue resolution.** The Vue resolution bodies relocated to `framework_surface::vue_exec`; `typeinfo/adapters/vue/{public_type,surface,store}.rs` are DELETED with no re-export shim or alias under `adapters::vue`, and `VueShallowMetadataStore` / `VueMacroDtoKey` are retired. Guards `vue_relocation_no_shim` + `retired_symbols_absent_from_production_source`.

See the `/framework-adapters` skill for the substrate's module map, the descriptor/registry/ctx/executor contracts, the script-fact seam, and Vue as the reference adapter.

### Carrier Geometry From Registered Facts (MANDATORY)

The carrier parser is the only production owner allowed to discover carrier structure from raw source. Downstream production code must not infer carrier geometry, section dialect, or block identity from text: for example, by searching for tags or delimiters, applying regex dialect heuristics, or recounting source order. Consume registered `RegisteredFileStructure`/`CarrierBlockInventory` projections and parser-minted facts, refs, or tokens; any retained compatibility ordinal is projected from that inventory, never reconstructed from text.

Local edit-time lexing may classify a bounded token or recovery window, but it must not become an alternate structure parser; any geometry-sensitive window is bounded by parser/inventory facts, including parser-unowned gaps bounded by registered parse boundaries. Tests may scan fixture text for setup, decoys, and assertions. This rule is review-enforced alongside the existing structural boundaries; do not add scanner-detection CI jobs or name-keyed source-tree guard scanners.

### Project-Bound External-TS Contract (CRITICAL)

Production external-TypeScript results for carrier sources are project-bound. The result-producing backend path is `ExternalTsProjectResolver` → `CarrierRegistry` → `EngineBackend`: `EngineBackend::ensure_project` is reached only from a resolved `ProjectBinding`, and `publish_snapshot`, `query`, and `diagnostics` require the resulting `BoundProject` witness. No production external-TS result path may infer a project from a bare path, open a carrier into a config-less/inferred project, or fall back to an inferred backend. Path-shaped transport notifications may exist below this contract, but they cannot construct external-TS results or bypass `BoundProject`.

Ownership is TypeScript-correct. A carrier source (`.vue`, `.svelte`, or any adapter extension) is owned by a configured project only through the default include, a no-extension directory/bare-star glob, or a glob/`files` entry that explicitly covers that extension. An extension-specific `*.ts` glob does not own it. TypeScript include has no brace expansion: multi-extension coverage is separate entries, never `*.{vue,svelte}`.

**Owner selection is tsgo-faithful, single-winner, and never terminally ambiguous when a configured project owns the carrier.** A carrier claimed by MULTIPLE configured projects is NOT a terminal state: `WorkspaceSnapshot::default_configured_owner_for_file` models tsgo `ProjectCollection.GetDefaultProject` + `findDefaultConfiguredProject` (`microsoft/typescript-go` `internal/project/projectcollection.go`) — start from the nearest ancestor solution (the nearest LITERAL `tsconfig.json`/`jsconfig.json`, not the nearest project root), BFS its `references` in DECLARED array order, take the FIRST project that directly includes the carrier, climb to the next ancestor solution unless `disableSolutionSearching`, and finally fall back to the lexicographically-least `tsconfig_path` among the configured claimants (tsgo `firstConfiguredProject`, a name-least ordering DISTINCT from the reference BFS order). The winner is chosen only from ordered structures (the `projects` Vec, `references` Vec, an ordered visited set) — never `HashSet` iteration; reference cycles resolve via the visited set, never a terminal ambiguity. The resolved owner flows through the SAME `binding_for` → `BoundProject` witness as the unique-owner arm. This selection is provider-neutral: the ONE `WorkspaceSnapshot` decision the tsserver, managed-tsgo, and shared-tsgo carrier routes all consume identically.

**Bounded divergence (recorded per tsgo faithfulness).** Verter's `ConfiguredMembership` is include/`files` only and carries NO `IsSourceFromProjectReference` (program-level project-reference-redirect) data, so for a carrier every include/`files` hit is treated as DIRECT and tsgo's `multipleDirectInclusions` is effectively always true — the reference BFS decides. The residual solution-graph pruning in `configured_owner_resolution_for_file` (which can pre-select a leaf over an ancestor that both includes and `references` it) is preserved on the `Unique` arm and is NOT unified with the BFS.

The remaining TERMINAL no-serve states are ONLY: `NoProject` (no configured project's include/`files` covers the carrier's extension at all) and the disk-layout carrier-path conflicts (a real user file at a generated companion path; a same-stem rune module beside the source). A multi-claimant configured overlap is NO LONGER terminal. `NotReady` stays the transient bootstrap retry state. Terminal `NoProject` / carrier-path conflicts surface a user-visible `verter(project)` warning on `did_open` AND `did_change` (wired through the debounced coordinator publish path, not only the request-only full-diagnostics path). Verter-native non-external-TS features may still answer. `SyntheticScratch` is a separate, explicitly labelled scratch lane for non-cross-file features only. It never supplies configured-project semantics, batch typecheck, cross-file results, or project-cache warming.

**Rename fail-closed for a resolved multi-claimant carrier.** A multi-claimant carrier serves every per-file feature (hover, definition, completion, references, diagnostics) from its single resolved owner, but a provider rename runs only within that one project. A symbol that ESCAPES the owner project (exported + imported by a sibling configured project) would rename partially and leave the symbol dangling in the siblings; cheap escape detection needs the cross-project rename fan-out (a future block). Until then rename FAILS CLOSED (a clear error, no `WorkspaceEdit`) for a resolved multi-claimant carrier — never a silent partial cross-project rename — while every other feature serves normally. A uniquely-owned carrier renames normally.

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

# Rust — CANONICAL agent gate
node scripts/gate.mjs                         # THE Rust gate. Builds the test universe ONCE via `cargo nextest archive` (single compile, no second-command recompile), then runs surfaces 1 and 2 from the same artifacts: SURFACE 1 = nextest run (per-test process isolation), SURFACE 2 = the verter_session libtest binaries executed directly (in-process / multi-test-per-process). SURFACE 3 then builds a SECOND `--workspace` archive with `--cargo-profile no-debug-assertions` — the shipped `cfg(debug_assertions)` state (debug assertions and overflow checks OFF, dev codegen otherwise) — and RUNS `package(verter_session) + package(verter_scheduler)` from it. That second archive is a second whole-workspace compile (a different profile is a different unit hash, so nothing is shared with the dev archive); it is the only surface that executes tests with `debug_assertions` off, and it is NOT an optimised build. Before the archive build it runs a freshness-tooling preflight: it ensures the workspace `buf` + `oxfmt` binaries are present (auto-running `pnpm install --frozen-lockfile` inside the mutex/timeout/stall machinery when the `node_modules/.bin` shims are missing), then VERDICT-GATES the `cases::typeinfo_proto_ts_freshness::*` byte-pin tolerance on the outcome — tooling present/installed ⇒ tolerance OFF, so a freshness failure is a HARD gate failure (exit 1), NOT PASS-WITH-TOLERATED; a deterministic install failure (e.g. frozen-lockfile mismatch) ⇒ a LOUD setup failure (exit 127), never silently tolerated (when an install is attempted — both `node_modules/.bin/{buf,oxfmt}` shims already present ⇒ the preflight returns already-present and no install runs); when pnpm is not resolvable AND `buf` is not resolvable the Rust byte-pin pair SKIPS gracefully and PASSES, so the gate reports an ORDINARY PASS (no FAIL line) — the verdict-gated tolerance flips ON there only as a LATENT safety net that would surface PASS-WITH-TOLERATED solely in the unusual case the pair produced a tolerated FAIL despite `buf` being absent. `oxfmt` absence NEVER grants tolerance — with `buf` present, a missing `oxfmt` is a LOUD setup failure (exit 127), not a degraded run. Run it with `node_modules` present (the normal path) so the byte-pin runs GENUINELY: with the tooling present a freshness failure is a HARD FAIL (a real stale-binding regression to regenerate + commit) — PASS-WITH-TOLERATED is NEVER the regression signal on a normal machine, and on a buf-less runner the pair yields an ordinary PASS via the skip, not PASS-WITH-TOLERATED. See docs/arch/gate-performance.md.

# The TWO UNDERLYING SURFACES gate.mjs runs — runnable directly (no Node, or debugging one surface in isolation):
cargo nextest run --workspace                # SURFACE 1 — every workspace test target INCLUDING the ~25 verter_session integration binaries, per-test process isolation
cargo test -p verter_session --tests         # SURFACE 2 — shared-process (in-process) surface for the verter_session integration suite
cargo test --workspace --doc                 # Rust doctests only; run when rustdoc examples changed or explicitly requested
cargo test --package verter_compiler test_name   # Specific Rust test
# NOTE: bare `cargo test --workspace --tests` SILENTLY SKIPS the verter_session integration suite (~4404 tests) because `session_metrics` feature unification drops those binaries from the workspace test set — it MUST NOT be the sole Rust gate; run `node scripts/gate.mjs` (which runs surfaces 1-2 from one archive plus the shipped-cfg SURFACE 3 from a second) or the two-surface pair above directly — note the pair covers neither the shipped-`cfg(debug_assertions)` surface nor its compile.
cargo test --package verter_compiler 2>&1 | tail -60  # Full suite with truncated output
```

**Oversize-source advisory:** `scripts/gate.mjs` scans production Rust sources and warns for each
non-exempt file above 1,500 lines as `path (N lines)`. File size is informational and never affects the
gate verdict.

**Resource ceilings — MEMORY-CEILING-MANDATORY.** Every gate/`--prepare` run is bounded by three flags,
each host-derived by default and overridable: `--build-jobs <N>` (cargo build parallelism, default
`min(4, cpuCount)`), `--test-threads <N>` (nextest/libtest thread count, gate-only, default
`min(4, cpuCount)`), `--memory-limit <SIZE>` (active child-tree RSS ceiling, e.g. `8GiB`/`12288MiB`,
default `max(512MiB, 50% of physical RAM)`). Policy: every gate run — agent or CI — goes through this
bounded wrapper with an explicit or defaulted `--memory-limit`; there is no sanctioned unbounded
invocation. A breach kills the active child tree fast and exits 123 (`ABORTED — memory ceiling`) — this
means NO gate verdict was produced. The same rule covers TIMEOUT (124), STALL (125), and any run whose
summary did not complete: an aborted or incomplete run is NOT PROVEN, never a PASS, and must never be
recorded as one.

**Build-prerequisite preflight (fail-closed, the gate's FIRST step).** Parts of the Rust suite load
artifacts cargo does not build: the real-provider suites spawn the pinned tsserver with `--globalPlugins
@verter/typescript-plugin --pluginProbeLocations packages/vue-vscode/node_modules`, a pnpm symlink to
`packages/typescript-plugin` whose `main` is `dist/index.js` — a `tsc -b` OUTPUT that `pnpm install` does
NOT produce. With the symlink present but the dist absent, tsserver loads no plugin and ~64 `*_tsserver`
tests fail with `TS2307: Cannot find module './Comp.vue'`, indistinguishable from a compiler regression.
So before the freshness preflight, before cargo, and before any test, `gate.mjs` **loads** that plugin
entry in a child process (`require()` of the probe directory, exactly what tsserver resolves) and on any
load failure FAILS CLOSED (exit 127, marker `BUILD-PREREQUISITE MISSING`) naming the probe target, the
load error, the producing packages, and the producer command. The oracle is a real load, not a list of
files to stat: the entry eagerly requires its emitted helpers and `@verter/language-shared`'s entry
re-exports a dozen emitted siblings, so a stat list mirrors the emit graph and drifts — both `index.js`
present with one helper missing passes every stat and still throws inside tsserver. The probe runs under
**tsserver's** environment, not the gate's — it strips the `CHILD_PROCESS_ENV_DENYLIST` the tsserver
launcher strips, read out of `crates/verter_type_runtime/src/tsserver/ipc.rs` so the two cannot drift,
because otherwise a `NODE_OPTIONS` preload can forge a status-0 load — and its timeout is a hard `SIGKILL`
bounded by the gate's own remaining deadline, since `spawnSync`'s default SIGTERM is trappable and a
trapping child both hangs the gate (with the single-flight mutex held) and returns a false positive.
Failure classes are typed (`reason`), so only `module-not-found` may ever be read as "never built". It
proves the closure **resolves**, not that it is **fresh**; a stale-but-loadable dist is a separate,
deliberately out-of-scope problem. It never builds the artifacts (the verdict must not depend on a mutation the gate performed) and
never skips the affected tests (with no install at all those tests SKIP and the gate goes green while
proving nothing). Produce them with `pnpm --filter @verter/language-shared --filter
@verter/typescript-plugin build` — deliberately NOT `pnpm build` and NOT `--filter
@verter/typescript-plugin...` (the trailing ellipsis pulls in `@verter/native`'s `napi build --release`).
`--prepare` is exempt; it runs no test. `(GB9)` in `scripts/gate-selftest.mjs` proves the discrimination in
six directions against the real production CLI on a synthetic miniature of the package graph.

### End-of-change Checks

Run after **every** change. Verter's crates are highly interconnected — a change in one crate frequently breaks tests in dependent crates. Always run the full workspace suite:

```bash
node scripts/gate.mjs 2>&1 | tee /tmp/test-output.txt   # CANONICAL Rust gate — runs THREE surfaces: SURFACE 1 (nextest process-isolation) + SURFACE 2 (direct in-process verter_session) from ONE dev-profile archive with zero second-compile, then SURFACE 3 from a SECOND `--workspace` archive built with `--cargo-profile no-debug-assertions` (the shipped `cfg(debug_assertions)` state), which RUNS `package(verter_session) + package(verter_scheduler)`. SURFACE 3 is the ONLY thing in the repo that executes tests with `debug_assertions` off — the class where a state mutation written inside a `debug_assert!` argument is a silent no-op in every shipped build. It is not an optimised build (dev codegen, no LTO) and it runs only that filterset. Run with `node_modules` present so the freshness-tooling preflight is a no-op and the `cases::typeinfo_proto_ts_freshness::*` byte-pin runs GENUINELY: with the tooling present a freshness failure is a HARD gate failure (exit 1, a real stale-binding regression to regenerate + commit), NOT tolerated. On a buf-less runner (pnpm not resolvable AND `buf` not resolvable) the Rust byte-pin SKIPS and PASSES, so the gate reports an ordinary PASS — the verdict-gated tolerance flips ON there only as a latent safety net (PASS-WITH-TOLERATED appears solely if the pair somehow emitted a tolerated FAIL despite `buf` being absent, which the skip does not). `oxfmt` absence never grants tolerance (with `buf` present a missing `oxfmt` is a LOUD setup failure); a deterministic install failure (frozen-lockfile mismatch) fails loud as setup (exit 127) when an install is attempted (both shims already present ⇒ no install runs).
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --release   # Compiles the REAL release profile (opt-level 3 + fat LTO), which the gate does not: gate SURFACE 3 uses the cheap `no-debug-assertions` profile, and surfaces 1-2 are debug. `debug_assert!` gates on `cfg!` — a RUNTIME constant — so its body still name-resolves in release: a `#[cfg(debug_assertions)]` helper called inside one is an E0425 in every release build (napi and wasm artifacts included) while compiling clean in debug. This is a CHECK — it RUNS NO TESTS, so it CANNOT observe the runtime half of the same class (a state mutation written inside a `debug_assert!` argument, which compiles fine and silently never executes in a shipped build). That half is covered only by gate SURFACE 3. Do not read a green `cargo check --workspace --release` as coverage of `debug_assert!` behaviour.
cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings   # Host clippy cannot see target-gated code. The wasm32 artifact is what the playground and `@verter/wasm` consumers run. The `wasm32-wasip1`/`wasip2` clippy jobs cover the SEPARATE lapce/zed manifests, not this one.
cargo fmt --all --check
pnpm install --frozen-lockfile   # Verify lockfile is in sync (CI uses this); also what the gate's preflight runs to make the freshness byte-pin run genuinely
```

Confirm `cargo clippy --version` reports the `rust-toolchain.toml`-pinned version before
trusting any of the three lint/check results — a clippy run on a different toolchain is not
evidence about the one CI uses.

- Corpus audit-test regenerator (run after audit-record schema or fixture changes; idempotent): `node scripts/gen-corpus-audit-tests.mjs`

For TypeScript changes, also run `pnpm test`. Do not skip workspace-wide testing even for "small" changes.

**Agent test policy:** `node scripts/gate.mjs` is the default Rust gate — it builds the test universe once and runs the two debug surfaces (`cargo nextest run --workspace` process-isolation + the in-process `verter_session` libtest binaries, the same direct surface as `cargo test -p verter_session --tests`) from the same archive with no second-command recompile, then builds a second `--workspace` archive under the `no-debug-assertions` profile and runs `package(verter_session) + package(verter_scheduler)` from it (SURFACE 3, the shipped-`cfg(debug_assertions)` surface). It runs the `verter_session` binaries under the workspace-unified `session_metrics` feature set (ON), intentionally replacing the old package-scoped default-feature (`session_metrics` OFF) rebuild rather than reproducing its feature config — that ON config is what the shipped LSP uses and what removes the second compile; no test target the old pair compiled is dropped. A contributor without Node, or debugging one surface in isolation, runs `cargo nextest run --workspace` then `cargo test -p verter_session --tests` directly. The `cases::typeinfo_proto_ts_freshness::*` buf/oxfmt byte-pin is the only tolerated failure, and its tolerance is now VERDICT-GATED on the gate's freshness-tooling preflight: the gate ensures `buf`/`oxfmt` are present (auto `pnpm install --frozen-lockfile` when the `node_modules/.bin` shims are missing) so with `node_modules` present that pair runs GENUINELY — and with the tooling present, tolerance is OFF, so a freshness failure is a HARD gate failure (exit 1, a real stale-binding regression to regenerate + commit), NOT surfaced as PASS-WITH-TOLERATED. On a buf-less runner (pnpm not resolvable AND `buf` not resolvable) the Rust byte-pin SKIPS and PASSES, so the gate reports an ordinary PASS (no FAIL line); the verdict-gated tolerance flips ON there only as a latent safety net, surfacing PASS-WITH-TOLERATED solely in the unusual case the pair emitted a tolerated FAIL despite `buf` being absent. PASS-WITH-TOLERATED is never the regression signal on a normal `node_modules`-present machine, and never the normal buf-less verdict either; `oxfmt` absence never grants tolerance (with `buf` present a missing `oxfmt` is a LOUD setup failure); a deterministic install failure (frozen-lockfile mismatch) fails loud as setup (exit 127) when an install is attempted (both shims already present ⇒ no install runs). Run the gate with `node_modules` present. Do not run bare `cargo test --workspace` (no `--tests`) by default: it pulls in doctests and example builds without improving the normal verification loop (and the silent-skip trap is stated once in Running Tests above). Run doctests (`cargo test --workspace --doc`) only when rustdoc examples changed or the user explicitly asks.

exec
/bin/zsh -lc "sed -n '441,700p' CLAUDE.md" in <repo-root>
 succeeded in 0ms:

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

**Public-boundary acceptance**: for every changed user-visible IDE, API, or compiler outcome, each affected acceptance ID has an automated public-boundary test asserting the required result AND the relevant forbidden or fail-closed result. Provider-selection, status, unit, and architecture tests supplement but do not substitute for that boundary test. A substrate block may inherit a parent boundary test only by recording the acceptance-ID mapping and executing that test in its gate. Enforcement is judgment — reviewers assess the actual invocation path and assertions, not the filename; §1a proves discrimination; confirm reruns the mapped test.

**Architecture guards for critical rules**: every new `CRITICAL` architecture rule lands with a static architecture guard or a discriminating regression test in the same change (subject to the landed-scanner bar below — a "static guard" is never a new name-keyed file scanner); if a guard cannot be automated yet, the rule text names the planned guard/test and the gap is tracked in the owning skill/doc. The R6 meta-guard at `crates/verter_session/tests/cases/g_misc0/critical_rules_have_guards.rs` (`every_critical_rule_in_docs_has_registered_guard`) walks `CLAUDE.md` plus every `.claude/skills/*/SKILL.md` and asserts every `(CRITICAL)` heading has a `CRITICAL_RULE_GUARDS` registry row with at least one named guard — a prose-only `(CRITICAL)` section fails the gate.

**Landed guards are structural, never name-keyed file scanners (forward-only)**: a heuristic file-scanner guard/test that keys on a specific tool, function, or identifier name (any spelled source name/path/token — type, module, import/path-segment, and string identities included; `syn`/AST-based scanning included) is a transient plan artifact — WIP-only (scratch branches, squashed out before landing), never a full-fledged landed guard. LANDED enforcement of an invariant is structural — compiler/type-system/tool-based (privacy/visibility/`E0603`, type-state, sealed traits, marker-trait derives, a real used tool or function) — never a name/text/grep scanner over the source tree. This strengthens Structural-Confinement-First (`.claude/skills/mom-cto-orchestration/reference/PROTOCOL.md` → Structural-Confinement-First → Landed-scanner bar): even a residual scanner that rule would permit (justified, recorded, supplement to a structural primary) does not land — keep it WIP, replace it structurally, or accept the residue uncovered by any landed scanner. Review/governance-enforced by design, NOT guard-enforced — a guard that detects "name-keyed scanner guards" would itself be a name-scanner. Forward-only: pre-existing landed scanners are grandfathered as a class — by temporal status (already landed at rule adoption), not by list membership — and retained as-is; the explicitly disclosed high-risk example (illustrative, not an exhaustive inventory) is the hot-materialize syntactic tripwire (`hot_materialize_syntactic_tripwire_residual_backstop` + its `HOT_TERMINAL_SINKS`/`HOT_DECIDE_TAINTED_GATE_IDENTS`/`HOT_EXTRACTING_GATE_IDENTS`/`HOT_MAT_DIRECT_IDENTS` name-lists in `crates/verter_session/tests/cases/output_projector_residual_guards.rs`), retained as-is with no removal planned or required — its STRUCTURAL rail (the `NoTypeExpr` marker + the sealed `OutputProjector` capabilities) remains the durable primary.

**Rust test file organization**: When inline `#[cfg(test)]` exceeds ~400 lines, extract to a sibling `*_tests.rs` file.

### Verification Must Prove Execution (MANDATORY)

A required gate passes only on fresh, input-bound evidence that: every applicable required job was eligible and ran; the intended tree-derived surface was owned and independently discovered; selectors matched non-zero work; required source, build, and fixture prerequisites matched the tested tree; executed work was non-zero; unexpected prerequisite skips were zero; child deadlines were strictly below their parent killer; and a terminal summary completed. **Exit status 0 alone, a self-declared test universe, or a missing required-job result is FAIL.** Every tracked test or guard has exactly one declared primary gate; a hand-maintained filename list may not define the primary universe unless generated from independent discovery and parity-checked.

Attestation alone is insufficient — a receipt faithfully attests whatever incomplete universe the runner defines for itself. The durable design needs all three: fresh execution attestation; independently tree-derived inventory/discovery parity; and per-surface negative-control mutation through the exact canonical entry point. A single global canary cannot detect an omitted unrelated spec.

**The negative control must itself be proven to have applied.** A plant that fails to apply reports a pass: `perl`/`sed`/`grep` exit 0 on a non-match, so a mutation's exit code is never proof it landed, and a verification search hitting a PRE-EXISTING occurrence of the planted string is a false positive. Prove the mutation is present, unique, and new in the source before trusting the run; a green planted run means the plant failed until proven otherwise. A discrimination check that cannot distinguish "the plant did not apply" from "the code is correct" is not a discrimination check.

Planned guard: `gate_contract_integrity` — one registered suite exercising the canonical entry point against independent inventory plus per-surface negative controls covering missing summary, disabled or missing job, invalid timeout nesting, zero selection, stale or missing build, missing fixture or unexpected skip, omitted or unowned test, and a mutation that silently fails to apply. Until that guard, its attesting driver, and the required-job aggregator land, this rule is held only by §1a and confirm judgment.

**This rule currently fails its own test, and says so.** It ships `(MANDATORY)` — precisely the tier the R6 meta-guard (`every_critical_rule_in_docs_has_registered_guard`) does not check, because that guard scans `(CRITICAL)` headings only. A rule whose thesis is "a gate that cannot prove it ran is a failure" is therefore, today, a gate that cannot prove it ran. `(CRITICAL)` is not available as a shortcut: an unguarded `(CRITICAL)` heading FAILS the meta-guard. So the gap is named rather than hidden — the deferral, its owner (the gate-integrity block), its resolution gate (that block's landing), and the live in-tree instances are recorded in [`docs/arch/gate-integrity-ledger.md`](docs/arch/gate-integrity-ledger.md). Promotion to `(CRITICAL)` with its own `CRITICAL_RULE_GUARDS` row, in the same change that lands the guard, is an ACCEPTANCE CRITERION of that block (ledger row GI-4). It is never folded into `Stub Prevention` — a related but distinct invariant whose guards do not enforce these semantics.

### Testing-Hermeticity (MANDATORY)

Unit tests must only depend on locally-vendored fixtures. They must compile and run without any third-party repository (e.g., `nuxt-ui`, `element-plus`) checked out alongside this repository. Tests that need external corpora must be feature-gated (e.g., `#[cfg(feature = "external-corpus")]`) and excluded from the default canonical run (`node scripts/gate.mjs`, i.e. its two underlying surfaces `cargo nextest run --workspace` + `cargo test -p verter_session --tests`).

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
2. **Intent Contract** — the ratified statement of intent, before any mechanism design
3. **Changes** — specific files to modify with concrete modifications
4. **Legacy Deletions** — explicit list of files, functions, code paths, feature flags to remove
5. **Verification** — full workspace test commands and expected outcomes

Without explicit legacy deletion lists, agents skip deletions and leave dual paths alive.

**Intent before mechanism.** Before mechanism design for a block that changes observable behavior, authority, or fallback, record a ratified intent contract: the actor/problem and why the capability should exist; required and forbidden observable outcomes; authority/fallback order; a planned test or gate for each stable acceptance ID; and material cold, warm, allocation, fan-out, and latency bounds. An internal substrate block may reference its parent contract but must state the invariant and performance contribution it owns. Ratification comes from the approved plan or product authority; no implementation brief is dispatched without it. Enforcement is judgment — exercised at decomposition and again immediately before implementation dispatch.

### Execution

Execute approved plans fully in one pass, end-to-end, without intermediate checkpoints or mid-plan confirmation on already-approved steps. Do not pause, defer scope, leave planned work unfinished, or rewrite the plan into a smaller/safer variant because the correct path is breaking, broad, or labor-intensive. Approved plans land as written unless the user explicitly re-scopes them.

**One-pass execution applies only while the approved design remains valid.** The second-REOPEN circuit breaker lapses approval for the affected design: pause implementation, obtain and record the required architecture/product ruling, and resume only once the design is ratified again. This is not a checkpoint — one-pass governs *executing an approved design*, and the breaker fires when *approval itself has lapsed*, which is a different event and precisely why execution must stop rather than grind on. STOP, failed verification, rule conflict, and verified plan-invalidating discoveries pause at their prescribed evidence gate without creating a discretionary user checkpoint. Breadth, breakage, effort, or migration size never lapses approval; approved scope changes only through the recorded ruling or explicit user re-scope. See `/mom-cto-orchestration` → Decision Admission.

### Orchestrating Large Plans

For a large multi-block plan, refactor, migration, or staged cutover executed autonomously, drive it via the `/multi-agent-orchestration` skill rather than improvising: a pure orchestrator delegates blocks to implementer/reviewer/fix sub-agents, gates each on dual review (independent reviewer + `codex`), runs fix cycles until clean, and verifies sub-agent reports against git state (trust but verify).

When a block runs in a dedicated `git worktree`, run `pnpm install --frozen-lockfile` in the worktree root once at creation time, before any JS/TS test or workspace-importing Node script — fresh worktrees do not get the gitignored `node_modules/`, and a missing install makes JS/TS tests fail spuriously and read as a false regression. See the skill's "Worktree hygiene & environmental discipline" section.

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
- Never apply a dirty fix that contradicts architectural rules just to make tests pass
- If the proper fix is outside approved scope, do not apply a workaround and do not use a `TODO` as its disposition. Route the finding through the applicable scope authority and record `ADOPT-NOW`, `DEFER`, or `REJECT` before related work continues. A `TODO` may reference an approved debt row but never replaces it.

**Explicit finding disposition.** Every scope-deviating correctness finding is dispositioned before related work continues as `ADOPT-NOW`, `DEFER`, or `REJECT`. `ADOPT-NOW` records the scope and acceptance-contract change. `DEFER` requires a codex-DEFER ruling and a debt row naming the durable owner block, the resolution gate no later than plan close, the acceptance ID/test, and the ruling reference. `REJECT` records evidence and rationale. A TODO, a feedback entry, or an ephemeral agent identity is not a disposition; plan close requires zero open deferrals. Enforcement is judgment — codex at the scope consult, and the plan-close zero-open-deferral check.

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
- New or ported repo-owned tooling lands as a Rust bin (e.g. the `gen-typeinfo-manifest` cargo bin) or a
  Node script — never a committed Python script.
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

Scopes: `core` (verter_compiler), `napi` (verter_napi / @verter/native), `wasm` (verter_wasm / @verter/wasm), `play` (playground), `unplugin` (@verter/unplugin), `lsp` (language-server), `types` (@verter/types), `meta` (@verter/component-meta), `ci` (CI/CD workflows), `*` (multiple areas).

Example: `feat(core): add v-memo directive support`

**No program vocabulary in commit messages or source.** A commit message describes the change on
its own terms: it must not name the architecture program, its revision, or any of its block
identifiers. A commit that lands plan text says what the text decides, not which block decided it.
The same prohibition applies to source under `crates/`, `packages/` and `scripts/` — see "No phase
archaeology in production code". The program's own document tree and this file are exempt while the
program runs; this file carries the program banner and may cite blocks by identifier.

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
| `/testing`               | Test patterns, TDD workflow, the canonical `gate.mjs` Rust gate runner, sourcemap testing, server cleanup |
| `/e2e-vscode-testing`    | VS Code E2E test fixtures, helpers API, adding new tests                                         |
| `/wsl-e2e-testing`       | WSL E2E tests to reproduce Linux/CI failures, fixture matrix                                     |
| `/rust-performance`      | Rust optimization patterns, allocation hierarchy, CodeTransform API                              |
| `/multi-agent-orchestration` | Driving a large multi-block plan, refactor, migration, or staged cutover autonomously: pure orchestrator + implementer/reviewer/fix sub-agents, dual review (independent + codex), per-block fix cycles, trust-but-verify |
| `/scheduler`             | Scheduler submission/admission APIs (`submit_request`/`submit_batch`/`submit_batch_atomic`), CPU vs I/O pool routing, host CPU-pool coordination |
| `/debug-tooling`         | Hangs, unexpectedly slow paths, stack snapshots: backtrace watchdog, LLDB attach wrapper, release-dbg profile |
| `/agent-prompts`         | Generating implementation/continuation/review/fix prompts for driving separate agent sessions |

exec
/bin/zsh -lc 'nl -ba docs/arch/refactor/rev11/charters/BV0.md' in <repo-root>
 succeeded in 0ms:
     1	# BV0 — Immediate Vue known-defect correction
     2	
     3	**Status:** PROPOSED / LOCKED. **Class:** Framework subsystem.
     4	**Predecessor:** BF2.
     5	
     6	## Objective
     7	
     8	Correct the genuine Vue VDOM, Vapor, SSR, assembly, and mapping defects exposed by
     9	BF2's exact `vuejs/core v3.6.0-rc.3` seed manifest while preserving every public
    10	route's existing successful result contract. No temporary safety or tracking
    11	mechanism is produced.
    12	
    13	## Owned scope
    14	
    15	BV0 owns source-root-cause corrections necessary for the exact 36-cell BF2 Vue seed
    16	matrix and independently authored controls. It expressly owns:
    17	
    18	1. non-inline `<script setup>` expose binding and implicit `__expose();` emission;
    19	2. VDOM `<slot>` fallback static caching and the `CACHED` patch flag;
    20	3. the remaining genuine seed-matrix defects, including invalid Vapor module
    21	   references/imports, fragment and patch topology, dynamic-props membership,
    22	   setup-return behavior, and source-map differences after harness artifacts are
    23	   removed; and
    24	4. shared lower-owner corrections when the same root cause serves multiple Vue
    25	   backends.
    26	
    27	BV0 does not introduce B3's canonical request, B4's publication architecture, a new
    28	universal IR, fixture-identity branches, generated-output scanning,
    29	official-compiler production fallback, a known-divergence allowlist, a temporary
    30	typed refusal, or the complete BV1 official-case pack.
    31	
    32	## Required procedure
    33	
    34	For each defect: add a failing independently authored regression first; reproduce
    35	against the exact rc.3 oracle; identify the existing typed compiler-owner root cause;
    36	implement the minimum reusable correction through the existing structured
    37	emitter/CodeTransform path; rerun every affected axis and a negative control; and
    38	prove unaffected routes retain their prior successful result contract.
    39	
    40	A candidate-generator test that merely produces output is not acceptance evidence.
    41	It must be connected to the BF2 comparator, parse/link/runtime/map oracles, and
    42	mutation discrimination.
    43	
    44	## Required exits
    45	
    46	All 36 exact BF2 Vue seed cells pass their applicable assembled-parse,
    47	exact-package-link, normalized structural/helper-topology, deterministic
    48	runtime/server, diagnostic, and mapping checks. The isolated oracle install is
    49	present so link checks genuinely execute. Every planted control is detected. No
    50	generated Vue route was changed to typed non-success. No Vue tracking, backlog,
    51	waiver, or retraction artifact remains.
    52	
    53	The `__expose` and slot-fallback-cache regressions pass for development and
    54	production, inline and non-inline applicability, and VDOM/Vapor/SSR where applicable.
    55	Locked performance cells remain within their existing thresholds.
    56	
    57	## Abort/rescope
    58	
    59	Stop with `RESCOPE_REQUIRED` if a genuine seed-matrix defect cannot be fixed without
    60	introducing B3/B4 authority or changing a ratified public product contract. Do not
    61	substitute a guard, tracker, waiver, fixture-specific branch, or silent deferral.

exec
/bin/zsh -lc 'nl -ba docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md' in <repo-root>
 succeeded in 0ms:
     1	# BV0 Conformance Review — candidate `c40a1ca96` vs base `b64358705`
     2	
     3	**Charter:** `docs/arch/refactor/rev11/charters/BV0.md` (Immediate Vue known-defect correction)
     4	**Reviewer role:** CONFORMANCE (criterion-by-criterion against "Required exits")
     5	**Worktree:** `<worktree>/verter-review-bv0-conf`, HEAD `c40a1ca96`, `git status` clean before and after every run.
     6	
     7	> **Environment caveat, stated up front:** a second (adversarial) reviewer agent is operating in this
     8	> same physical worktree and temporarily reverts production code for mutation testing. Every test run
     9	> below was bracketed by a `git status --porcelain` integrity check; all reported runs executed against
    10	> an **unmodified** tree at `c40a1ca96` (`tree_unchanged_during_run=YES`, `still_clean=YES`).
    11	
    12	---
    13	
    14	## 0. Test execution — what I actually ran
    15	
    16	### 0.1 The mandated run (memory-capped, `CARGO_BUILD_JOBS=3 --test-threads=3`)
    17	
    18	```
    19	cargo test -p verter_vue_conformance --test main official_seed_matrix -- --test-threads=3
    20	→ test result: ok. 36 passed; 0 failed; 0 ignored; 11 filtered out; finished in 15.58s
    21	```
    22	
    23	**This run alone is NOT sufficient evidence for the charter's required axes**, and that is finding **B2**
    24	below. In the worktree's default environment the `link` and `runtime` axes **silently skip**. Proof:
    25	
    26	```
    27	$ node bin/check-candidate.mjs --candidate <cell> --out ...      # default worktree env
    28	parse    ran
    29	link     skipped | oracle npm cache not provisioned at .../verter-review-bv0-conf/...
    30	mapping  ran
    31	runtime  skipped | oracle npm cache not provisioned at .../verter-review-bv0-conf/...
    32	```
    33	
    34	`official_seed_matrix.rs:459-489` (`js_axis_reasons`) folds `"skipped"` into an informational line, never
    35	a failure; `assert_cell_passes` (l.511) asserts only on `comparison.passed() && !js_failed`, and the
    36	collected `js_reason_lines` are interpolated **only into the failure message**. A fully-skipped run is
    37	therefore byte-indistinguishable from a fully-passing one on stdout.
    38	
    39	### 0.2 The decisive re-run — all four axes genuinely executing
    40	
    41	I provisioned the isolated oracle out-of-tree (worktree untouched; `.oracle-*` are gitignored):
    42	
    43	```
    44	$ BF2_ORACLE_NPM_CACHE=/tmp/bv0-oracle-npm-cache node scripts/provision-oracle-npm-cache.mjs
    45	added 20 packages in 4s → oracle npm cache warmed          (exit 0)
    46	
    47	$ ensureOracleDomain("vue")     # offline realization from the committed lock
    48	installDir:            /tmp/bv0-oracle-installs/vue
    49	realizedClosureSha256: a0a58df52c90abdffca1a61f94fe5a7ed2918f586f36e73a5e8d9decd01ce1e7
    50	vue version:           3.6.0-rc.3          ← matches the pinned oracle exactly
    51	```
    52	
    53	Re-probe with the oracle wired in — **all four axes flip to `ran`**:
    54	
    55	```
    56	parse ran | link ran | mapping ran | runtime ran
    57	```
    58	
    59	Then the mandated suite re-run with the oracle active:
    60	
    61	```
    62	BF2_ORACLE_NPM_CACHE=... BF2_ORACLE_INSTALLS=... \
    63	cargo test -p verter_vue_conformance --test main official_seed_matrix -- --test-threads=3
    64	→ test result: ok. 36 passed; 0 failed; 0 ignored; 11 filtered out; finished in 26.14s
    65	```
    66	
    67	**36/36 pass with parse + exact-package-link + mapping + deterministic runtime/server genuinely
    68	executing** (26.14s vs 15.58s is the link/runtime cost). The runtime axis is a real behavioral oracle,
    69	not a structural proxy (`check-candidate.mjs:242-302`): SSR renders both sides and compares HTML; vdom
    70	hydrates the candidate against the **official** SSR golden's HTML and compares final DOM; vapor mounts
    71	both and compares DOM.
    72	
    73	### 0.3 Supporting runs
    74	
    75	```
    76	cargo test -p verter_vue_conformance --test main   (oracle active)
    77	→ 47 passed; 0 failed; 0 ignored                  incl. seed_conformance_matches_tracked_dispositions
    78	                                                  incl. 3x vapor_runtime_behavior
    79	
    80	cargo test -p verter_compiler --lib
    81	→ 6001 passed; 0 failed; 5 ignored                (tree verified unchanged during run)
    82	```
    83	
    84	---
    85	
    86	## 1. Required exits, criterion by criterion
    87	
    88	| # | Required exit | Verdict |
    89	|---|---|---|
    90	| E1 | All 36 seed cells pass parse / link / structural / runtime / diagnostic / mapping | **MET** (§0.2) |
    91	| E2 | Isolated oracle install present so link checks genuinely execute | **DEFECTIVE** → B2 |
    92	| E3 | Every planted control is detected | **DELEGATED** (adversarial reviewer's assignment; not independently re-run here) |
    93	| E4 | No generated Vue route changed to typed non-success | **MET** (§1.4) |
    94	| E5 | No Vue tracking / backlog / waiver / retraction artifact remains | **NOT MET** → B1 |
    95	| E6 | `__expose` + slot-fallback-cache pass dev/prod, inline/non-inline, VDOM/Vapor/SSR | **MET**, one cosmetic defect → N1 |
    96	| E7 | Locked performance cells within existing thresholds | **MET** (thresholds unchanged; not executed) |
    97	
    98	### 1.4 E4 — no typed non-success (MET)
    99	
   100	`git diff … -- crates/verter_compiler/src | grep '^+' | grep -E 'CompileDiagnostic|Severity::Error|push_error|…'`
   101	returns **zero** new error-emission sites. No `#[ignore]` was added anywhere in `crates/` (and none
   102	removed). No new waiver/tracker file was added. Deleted files (`vapor/comment.rs`, `vapor/text.rs`) are
   103	genuine legacy deletions folded into the new `vapor/block_plan/` — no shim, no dual path, consistent
   104	with the Legacy Code Deletion rule.
   105	
   106	The 17 `*.refuse.json` fixtures that surfaced in a refusal-pattern grep are **pre-existing Svelte**
   107	constant-folding fixtures: `git diff --numstat` shows all 17 are 1-insertion/1-deletion version-string
   108	changes and `--diff-filter=A | grep -c refuse` = **0**. Not a Vue tracking mechanism, not introduced here.
   109	
   110	---
   111	
   112	## 2. Independent spot-checks of fixed defect classes (diff read against real rc.3 source)
   113	
   114	Oracle source read from the realized install at `/tmp/bv0-oracle-installs/vue/node_modules/@vue/*`
   115	(verified `vue@3.6.0-rc.3`), **not** from commit-message claims.
   116	
   117	### 2.1 `__expose` binding + implicit call — **FAITHFUL ✓**
   118	
   119	Official `@vue/compiler-sfc/dist/compiler-sfc.cjs.js`:
   120	```js
   121	15658: const destructureElements = ctx.hasDefineExposeCall || !inlineMode ? [`expose: __expose`] : [];
   122	15728: if (!ctx.hasDefineExposeCall && !inlineMode) setupPreambleLines.push(`__expose();`);
   123	```
   124	Verter `crates/verter_compiler/src/script/process.rs:470-471`:
   125	```rust
   126	let bind_expose          =  macro_state.has_expose || !options.inline_template;
   127	let emit_bare_expose_call = !macro_state.has_expose && !options.inline_template;
   128	```
   129	Exact predicate-for-predicate correspondence on both the binding and the bare-call condition. Corroborated
   130	independently: the phrase *"official always destructures `expose: __expose` … Verter emits those only when
   131	`defineExpose` is used"* appears in **8+ triage notes at base** and in **zero** notes at candidate — the
   132	defect really was eliminated corpus-wide, not just on the 3 seed fixtures.
   133	
   134	### 2.2 Vapor `:key` exclusion from dynamic props — **FAITHFUL ✓**
   135	
   136	`vapor/props.rs:157` skips `arg == Some("key")` gated on `skip_key_prop: el.v_for.is_some()`.
   137	Cross-checked against the correct backend oracle — `@vue/compiler-vapor` reserves `key` in the **v-for**
   138	context (`compiler-vapor.cjs.js:137` `prop.name === "bind" && …arg.content === "key" && dirs.includes("key")`;
   139	`:4460` `findProp(node,"key")` inside the v-for transform; `:4463` `wrapTemplate(node,["for","key"])`).
   140	The v-for-scoped gate is correct for vapor. (Note the vdom oracle differs — `compiler-core.cjs.js:4690`
   141	excludes `ref`/`key` unconditionally — but that is a different backend and a different code path; the
   142	candidate's gate is in `vapor/props.rs` only, so this is not a mismatch.)
   143	
   144	### 2.3 Slot-fallback static caching + `CACHED` flag — **BEHAVIOR ✓ / rationale wrong** → N1
   145	
   146	`vdom/slots.rs` correctly routes `<slot>` fallback children through the cache-aware emitter
   147	(`emit_slot_children_with_cache`) instead of the uncached separators-only path, and attaches the `-1`
   148	CACHED flag. The flag itself is right. But the new prod branch:
   149	```rust
   150	let close = if self.options.is_production { "\", -1))" } else { "\", -1 /* CACHED */))" };
   151	```
   152	is justified by the source comment *"dropping the dev-only comment in production"*. **The real rc.3
   153	goldens contradict that claim** — `vue/slots__vdom__map0__prod1` (isProd=**true**) emits:
   154	```
   155	_cache[0] || (_cache[0] = _createTextVNode("Untitled", -1 /* CACHED */))
   156	```
   157	identical to the prod0 record. Official does **not** drop the comment in production. The comment further
   158	claims to be *"matching the individual-element cache wrapper's own `-1 /* CACHED */` handling in
   159	`element.rs`"* — but `element.rs:2544` emits `", -1 /* CACHED */"` **unconditionally, with no prod branch**.
   160	The cited precedent says the opposite of what the new code does.
   161	
   162	This does not fail the gate: `canon/comments.rs:13-25` anchors only semantic comments (PURE, license,
   163	JSDoc, bundler-significant), so `/* CACHED */` is normalized away as cosmetic — permitted by the
   164	Compiled-Output Conformance rule. Recorded as non-blocking **N1**.
   165	
   166	---
   167	
   168	## 3. Known-wrong-output guard / tracker / waiver scan
   169	
   170	No typed refusal, no fixture-identity branch, no official-compiler fallback, and no new allowlist file was
   171	introduced on any Vue path. `official_seed_matrix.rs:16-20` explicitly declines a tracked-divergence
   172	mechanism for its own domain.
   173	
   174	**However**, the pre-existing seed-corpus waiver ledger `crates/verter_vue_conformance/corpus/known-divergences.json`
   175	was regenerated (`VERTER_CONFORMANCE_UPDATE=1` path, `seed_conformance.rs:341-372`) and **grew from 361 to
   176	690 recorded divergence reasons**.
   177	
   178	I initially read that growth as mass masking. **That reading is wrong and I am recording the correction:**
   179	the dominant cause is a sanctioned oracle migration, not a regression. `seed_conformance.rs:120` and
   180	`tests/common/mod.rs:13` move the corpus goldens `3.6.0-rc.1 → 3.6.0-rc.3`; the rc.1 tree is fully deleted
   181	(only `corpus/goldens/3.6.0-rc.3` remains), matching the one-corpus-per-framework rule. rc.3 changed Vapor
   182	materially (delegation → direct `on` listeners, `defineVaporComponent`, `withVaporModifiers`/`withVaporKeys`),
   183	which is exactly where the growth concentrates. The comparator itself (`src/compare.rs`) is **unchanged** in
   184	this diff, so the growth is not a stricter-oracle artifact either. Triage notes are genuinely maintained —
   185	zero `"TODO: triage this divergence"` placeholders, and several record real wins ("v-if branch key hoisting
   186	is now fixed", "the event-handler dynamic-props-array/PROPS-flag divergence is now fixed"). Three cells were
   187	**removed** because they now pass.
   188	
   189	That exoneration does **not** extend to two cells — see B1.
   190	
   191	---
   192	
   193	## BLOCKING FINDINGS
   194	
   195	### B1. Two previously-passing Vue routes regressed, and were absorbed into the waiver ledger under a factually impossible justification
   196	
   197	Two cells that had **no entry at base** (and therefore passed — `seed_conformance.rs:385-389` fails any
   198	untracked divergence) acquired new waiver entries at candidate:
   199	
   200	- `components/dynamic-multi-root | vdom | non-inline`
   201	- `elements-text/multi-root | vdom | non-inline`
   202	
   203	both with the same 2 reasons:
   204	```
   205	[structure] Program[0]/import              — node kind: verter `VariableDeclaration` vs golden `import`
   206	[structure] Program[1]/VariableDeclaration — node kind: verter `import` vs golden `VariableDeclaration`
   207	```
   208	
   209	**This is not oracle churn.** I diffed the base rc.1 goldens against the candidate rc.3 goldens for both
   210	cells: **byte-identical** (`IDENTICAL rc.1==rc.3` for both). The oracle did not move for these cells, so the
   211	only changed variable is Verter's own emission. These are BV0-introduced regressions.
   212	
   213	**The recorded justification is impossible.** Both entries carry the note:
   214	
   215	> *"import-statement lowering order: Verter lowers **the user import** to a VariableDeclaration in a
   216	> different Program-item slot … a template-only SFC has no other script-level statements…"*
   217	
   218	Neither fixture has a user import — or any script at all. `grep -c "script\|import"` returns **0** for both
   219	`.vue` files; `elements-text/multi-root.vue` is three static elements in a bare `<template>`. The actual
   220	divergence is that Verter now emits the `_sfc_main` `VariableDeclaration` **before** the `vue` helper import,
   221	where the golden emits the import first.
   222	
   223	Charter impact — this is the specific thing BV0's procedure and abort clause forbid:
   224	- Required procedure: *"prove unaffected routes retain their prior successful result contract"* — two vdom
   225	  routes demonstrably did not.
   226	- Abort/rescope: *"Do not substitute a guard, tracker, **waiver**, fixture-specific branch, or silent
   227	  deferral."* A self-introduced regression was dispositioned into the waiver ledger rather than fixed or
   228	  escalated, and the note attached to it would lead a later reader to dismiss it as a benign pre-existing
   229	  shape.
   230	
   231	Severity note, stated plainly: ES module imports are hoisted, so the emitted order is very likely
   232	**behaviorally** harmless — this blocks on process and on the false rationale, not on a runtime hazard.
   233	Required disposition: either fix the emission order, or record an explicit `ADOPT-NOW`/`DEFER`/`REJECT`
   234	disposition with a rationale that matches the fixtures.
   235	
   236	### B2. The new seed-matrix suite reports 36/36 green when the charter-required link and runtime axes never ran
   237	
   238	`official_seed_matrix.rs` is authored by this block, and it cannot distinguish "link passed" from "link
   239	never executed". Skipped axes are informational-only (l.476-480) and their lines reach stdout **only inside
   240	the assertion failure message** (l.511-523). With no oracle provisioned — the default state of a fresh
   241	clone, this worktree, and any CI runner that has not run the one-time provisioning — the suite prints
   242	`36 passed` while `exact-package-link` and `deterministic runtime/server`, two axes the charter names
   243	explicitly, did not run at all. §0.1 vs §0.2 above is the demonstration: same commit, same command, the
   244	only delta is two env vars, and the axis statuses flip `skipped → ran`.
   245	
   246	This contradicts the charter exit *"The isolated oracle install is present so link checks genuinely
   247	execute"* and the project's MANDATORY *Verification Must Prove Execution* rule, which requires that
   248	"unexpected prerequisite skips were zero" and states that "exit status 0 alone … is FAIL".
   249	
   250	To be fair to the candidate: the *substance* is fine — I provisioned the oracle and all 36 cells pass all
   251	four axes (§0.2), and the harness is honest by construction (skips carry an exact reason and remediation;
   252	a drifted install is a hard refusal, never a skip). The defect is that nothing forces or records
   253	execution. A minimal fix is sufficient: fail (or at minimum emit an unmissable stderr banner and a
   254	non-silent marker) when `link`/`runtime` report `skipped`, so the charter's "genuinely execute" is proven
   255	rather than assumed.
   256	
   257	---
   258	
   259	## NON-BLOCKING FINDINGS
   260	
   261	**N1. `slots.rs` production `/* CACHED */` drop is cosmetically divergent and its rationale is wrong on
   262	both counts.** Official rc.3 emits `-1 /* CACHED */` in production (`slots__vdom__map0__prod1` golden), and
   263	`element.rs:2544` — cited by the comment as the precedent being matched — emits it unconditionally. The
   264	comparator treats it as cosmetic so no test fails, but the code now disagrees with both the oracle and its
   265	own stated model. Suggest dropping the prod branch (emit unconditionally, matching `element.rs`) or
   266	correcting the comment.
   267	
   268	**N2. Out-of-charter Svelte scope.** ~2609 files bump the Svelte oracle pin 5.56.3 → 5.56.8 inside a block
   269	whose charter is *"Immediate Vue known-defect correction"*. Mechanically this is benign and I verified it
   270	carefully: of 1390 `svelte_oracle_corpus/goldens` files **1390 are version-string-only and 0 have any
   271	content change**; of 1220 `verter_svelte_conformance` files 1219 are version-only plus one 2-line
   272	`src/model.rs` change. Svelte codegen output is byte-identical under the new pin, and it aligns with the
   273	one-corpus-per-framework rule. Flagged only because it inflates a Vue block's diff from ~40 reviewable
   274	source files to 2076 files, which materially degrades reviewability.
   275	
   276	**N3. E3 not independently verified here.** "Every planted control is detected" is assigned to the
   277	adversarial reviewer. I observed that `conformance_discriminator.rs` was correctly updated for rc.3 vapor
   278	semantics (delegation/`$evtclick` planted mutations replaced with `_on(...)`/event-name mutations, with the
   279	expected `DiffDim` adjusted from `Identifier` to `Literal`), but I did not re-run plant-red-green myself.
   280	
   281	---
   282	
   283	## Summary
   284	
   285	The engineering substance is largely sound: the `__expose` fix is a predicate-exact match to rc.3, the
   286	vapor `:key` gate matches `@vue/compiler-vapor`, all 36 seed cells pass every required axis with the oracle
   287	genuinely executing, the wider corpus and 6001 compiler tests are green, no typed refusal or fixture branch
   288	was introduced, and the vapor legacy files were properly deleted rather than shimmed. The large
   289	known-divergence growth is explained by a sanctioned rc.1 → rc.3 migration, not masking — I checked and
   290	withdrew that concern.
   291	
   292	Two items block: a self-introduced regression on two previously-green Vue routes waived under a
   293	justification the fixtures contradict (B1), and a new suite that reports green without proving the
   294	charter-required link/runtime axes ran (B2).
   295	
   296	VERDICT: BLOCKING
   297	1. B1 — `components/dynamic-multi-root|vdom` and `elements-text/multi-root|vdom` regressed from passing to waived (rc.1/rc.3 goldens byte-identical ⇒ BV0-caused), dispositioned into `known-divergences.json` under a note citing a "user import" that neither script-less fixture contains; violates the required procedure's "prove unaffected routes retain their prior successful result contract" and the abort clause's prohibition on substituting a waiver.
   298	2. B2 — `official_seed_matrix.rs` treats `skipped` link/runtime axes as informational and prints nothing on success, so it reports 36/36 green when the charter-required exact-package-link and deterministic-runtime axes never executed (demonstrated: identical command, axes flip `skipped → ran` on provisioning alone); fails the "isolated oracle install is present so link checks genuinely execute" exit and the MANDATORY Verification-Must-Prove-Execution rule.

exec
/bin/zsh -lc 'nl -ba docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md' in <repo-root>
 succeeded in 0ms:
     1	# BV0 — Architecture Review
     2	
     3	**Candidate:** `c40a1ca96` (`fix(core): correct Vue VDOM/Vapor/SSR conformance defects against the rc.3 oracle`)
     4	**Base:** `b64358705`
     5	**Charter:** `docs/arch/refactor/rev11/charters/BV0.md`
     6	**Scope of this review:** owned-scope adherence, charter prohibitions, and the global CLAUDE.md architecture rules (Two Template Codegen Paths, CodeTransform SoT, Compiled-Output Conformance, phase archaeology, owner-crate placement).
     7	
     8	---
     9	
    10	## 1. What the candidate actually changes
    11	
    12	Source-only footprint (excluding regenerated corpora/goldens):
    13	
    14	| Area | Files | Net |
    15	|---|---|---|
    16	| `verter_compiler/src` (Vue script + template codegen) | 35 | ~+6.7k / −3.0k |
    17	| `verter_vue_conformance/tests` | 9 (2 new suites) | ~+1.1k |
    18	| `packages/framework-conformance-harness` (src/bin/test) | 6 (4 new) | ~+1.1k |
    19	| Vue goldens `3.6.0-rc.1` → `3.6.0-rc.3` | 95 `.js` renamed (11 with content change, **all under `vapor/`**) | — |
    20	| **Svelte oracle corpus + pins `5.56.3` → `5.56.8`** | **~2027 corpus files + 3 production `src` constants + 10 `package.json` + lockfile** | — |
    21	
    22	The largest single structural move is a rewrite of the Vapor backend from a walker-driven emitter into a two-pass **plan → emit** design, landed as a new `template/code_gen/vapor/block_plan/` module (2279 lines across `mod.rs`/`plan.rs`/`emit.rs`/`tests.rs`), with `vapor/comment.rs` and `vapor/text.rs` deleted.
    23	
    24	---
    25	
    26	## 2. Charter prohibitions — checked one by one
    27	
    28	| Prohibition | Verdict | Evidence |
    29	|---|---|---|
    30	| B3's canonical request | **Clean** | No request/envelope type introduced; nothing in the diff touches `verter_protocol`, `verter_session` request surfaces, or a canonical-request abstraction. |
    31	| B4's publication architecture | **Clean** | No publication/registry surface added; the diff stays inside `verter_compiler`'s script/template codegen and the conformance test crates. |
    32	| A new universal IR | **Clean** | `BlockPlan`/`PlanNode`/`BoundaryOp` are **Vapor-private** (`pub(super)` items inside `vapor::block_plan`). `grep` for `block_plan` outside `code_gen/vapor/` returns nothing. It is a backend-local lowering structure, not a cross-backend IR — VDOM, SSR and IDE are untouched by it. |
    33	| Fixture-identity branches | **Clean** | No `fixture == "…"` / `case_id == "…"` control flow in `official_seed_matrix.rs` or `vapor_runtime_behavior.rs`. The only `fixture.contains(&debug)` is an opt-in `VERTER_CONFORMANCE_DEBUG` printer. No `#[ignore]` added anywhere in the diff. |
    34	| Generated-output scanning | **Clean** | No `.replace(`, `Regex`, or post-`build_string()` mutation added to `verter_compiler/src`. Comparison is AST/structural (`compare_modules`), never source-text scanning of generated output. |
    35	| Official-compiler production fallback | **Clean** | The Vue oracle is invoked only from `packages/framework-conformance-harness/{bin,src,test}` and from `verter_vue_conformance/tests`. No production crate references `check-candidate.mjs`, `run-vapor-scenario.mjs`, `execute-vue-vapor.mjs`, or `execute-vapor-runtime.mjs`. |
    36	| A known-divergence allowlist | **VIOLATED in effect** — see Finding 1. The file was not *created* here, but BV0 added two new waiver rows for cells that previously passed. |
    37	| A temporary typed refusal | **Clean** | No `todo!`/`unimplemented!`/new refusal or "unsupported" diagnostic added to `verter_compiler/src`. |
    38	| The complete BV1 official-case pack | **Clean** | The new matrix covers exactly the three BF2 seed fixtures × 3 backends × sourceMap × isProd = 36 cells. No broader official case pack imported. |
    39	
    40	---
    41	
    42	## 3. Global CLAUDE.md rules
    43	
    44	### Two Template Codegen Paths (CRITICAL) — **PASS**
    45	`crates/verter_compiler/src/ide/**` is **entirely untouched** by the diff (`git diff --name-only … -- crates/verter_compiler/src/ide` is empty). The rewrite is confined to `template/code_gen/{vapor,vdom,ssr,shared}`, i.e. the runtime path. The IDE JSX/TSX projection cannot be affected.
    46	
    47	### CodeTransform Is the Single Source of Truth (CRITICAL) — **PASS**
    48	The `__expose` work builds its wrapper text into a `String` and hands it to a CodeTransform op (`process.rs:495` → `overwrite_or_root_prefix(setup.tag_open.start, setup.tag_open.end, &wrapper_start)`). No post-`build_string()` splicing, no regex, no string replacement on transformed output anywhere in the added lines. The pre-existing "the body strip skips imports, so this reconstruct is the only edit on the span — no nested-overwrite corruption" invariant is preserved and its comment extended.
    49	
    50	### Compiled-Output Conformance (CRITICAL) — **PASS, with one narrowing to ratify (Finding 4)**
    51	- No re-printer, pretty-printer, paren canonicalizer, or cosmetic-mimicry machinery introduced (`grep` for `reprint|prettif|canonicaliz|format_js|pretty_print` on added lines: zero hits).
    52	- `verter_vue_conformance/src/compare.rs` and `src/canon/` are **unchanged** — the structural comparator's semantics are identical before and after, which is what makes the corpus deltas in §4 comparable at all.
    53	- The new harness `compare.mjs` code explicitly *rejects* byte-exact cross-compiler comparison with a documented rationale, which is the correct posture under this rule.
    54	- The `namespaceOverrides` addition to `checkLinkValidity` is correctly bounded: the exact-package-identity check still runs through the real resolver against `baseDir`, so an override cannot mask a wrong or missing installed package.
    55	
    56	### No phase archaeology in production code (MANDATORY) — **PASS**
    57	`grep -niE 'phase [0-9]|post-cutover|pre-Phase|d-cutover|cutover|deleted in |retired in |rev11|BV0|BF2'` over `crates/verter_compiler/src`, `crates/verter_vue_conformance/tests`, and the harness `src`/`bin`: **zero hits**. New module docs describe the invariant (static shells / reactive boundaries / insertion-site contract), not the plan that produced them.
    58	
    59	### Owner-crate / module placement — **PASS**
    60	- `vapor/block_plan/` is correctly under the Vapor backend, private to it, and *reuses* shared owners rather than forking them: it imports `code_gen::shared::helpers` (`push_u32`, `VaporHelper`, template-declaration writers), `code_gen::types` (`CodeGenOutput`, `VaporElementState`, `VaporTextPart`, `VaporEffect`), and `code_gen::binding::BindingType`.
    61	- `shared/const_source.rs` (v-for source constancy) is correctly placed in `shared/` — it is genuinely consumed by more than one backend and is a pure classification function over binding metadata + parsed OXC data, with no text heuristics.
    62	- `shared/helpers.rs` correctly absorbs the `u32 → u64` helper-flag widening and the `TEMPLATE_FLAG_ROOT|STATIC` bitmask; the delegated-events list stays in `shared/`.
    63	- `ScriptCodeGenOptions::ssr → is_ssr` and the `__vapor` marker gate are in the script owner (`script/mod.rs`, `compile/mod.rs`), with the official-behaviour rationale recorded inline. Correct layer.
    64	
    65	---
    66	
    67	## 4. Findings
    68	
    69	### Finding 1 — BLOCKING. Two previously-passing Vue VDOM routes regressed and were absorbed into the known-divergence allowlist instead of fixed.
    70	
    71	`crates/verter_vue_conformance/corpus/known-divergences.json` gains two rows for cells that carried **no** entry at `b64358705` — and an absent entry in that file means the cell **passed** the comparator (the suite fails on any unlisted divergence):
    72	
    73	```
    74	+ components/dynamic-multi-root | vdom | non-inline   total=2
    75	+ elements-text/multi-root      | vdom | non-inline   total=2
    76	```
    77	
    78	Both rows carry the identical reason pair:
    79	
    80	```
    81	[structure] Program[0]/import           — node kind: verter `VariableDeclaration` vs golden `import`
    82	[structure] Program[1]/VariableDeclaration — node kind: verter `import` vs golden `VariableDeclaration`
    83	```
    84	
    85	This is Verter-side, not oracle drift. Both goldens are **byte-identical** between rc.1 and rc.3:
    86	
    87	```
    88	0	0	goldens/{3.6.0-rc.1 => 3.6.0-rc.3}/vdom/elements-text/multi-root.js
    89	0	0	goldens/{3.6.0-rc.1 => 3.6.0-rc.3}/vdom/components/dynamic-multi-root.js
    90	```
    91	
    92	In fact **no `vdom/` or `vdom-inline/` golden `.js` changed content at all** in this commit — all 11 content-changed goldens are under `vapor/`. So the VDOM oracle these two cells compare against is bit-for-bit what it was, the comparator (`src/compare.rs`, `src/canon/`) is unchanged, and the difference is a new module-item ordering divergence Verter introduced.
    93	
    94	Why this is blocking rather than cosmetic:
    95	
    96	- BV0's **required procedure** ends with "prove unaffected routes retain their prior successful result contract." Two routes that previously produced an in-contract-clean module no longer do.
    97	- BV0's **owned scope** states BV0 "does not introduce … a known-divergence allowlist." BV0 did not create the file, but adding new waiver rows to absorb a defect BV0 itself introduced is functionally the prohibited mechanism, and it converts a red signal into a green one.
    98	- The row's own `note` argues the divergence is benign ("not a signature-shape divergence"), but top-level module-item ordering between an `import` declaration and a lowered `VariableDeclaration` is exactly the "imports, helper families, helper call sequence where order is semantic" category the Compiled-Output Conformance rule keeps **in contract**. It is not on the cosmetic waiver list.
    99	
   100	Required: fix the import/module-item lowering order so both cells return to zero divergences and both rows are deleted — or obtain an explicit maintainer ruling that these two rows are ratified, recorded as a `DEFER` with a debt row per CLAUDE.md's Explicit-finding-disposition rule. A self-authored `note` in the allowlist is not a disposition.
   101	
   102	### Finding 2 — BLOCKING. Out-of-scope Svelte oracle migration rides in a Vue-only charter.
   103	
   104	The candidate bumps the Svelte oracle pin `5.56.3 → 5.56.8` across the monorepo:
   105	
   106	- ~2027 regenerated files under `crates/verter_compiler/tests/svelte_oracle_corpus/`;
   107	- three **production** `crates/verter_compiler/src/svelte/runtime/` constants (`SVELTE_ORACLE_VERSION`, the vendored `remove_typescript_nodes.*.js` include path, the `entity_table.rs` provenance header) plus a rewritten handler fingerprint;
   108	- the `remove_typescript_nodes.5.56.3.js → .5.56.8.js` fixture rename and a new `.prettierignore` rule for it;
   109	- 9 `package.json` files and `pnpm-lock.yaml`.
   110	
   111	BV0's objective and owned scope are Vue-only ("Correct the genuine **Vue** VDOM, Vapor, SSR, assembly, and mapping defects…"; all four owned-scope items name Vue constructs). Nothing in the charter authorises a Svelte dependency migration. The immediate predecessor commit `fdb6f6291` — *"docs(arch): split immediate Vue defect correction from the Svelte-focused safety retraction"* — is an explicit decision that Svelte work belongs to a **different** block; landing it here reverses that split in the very next commit.
   112	
   113	This is also not incidental to the Vue bump: the Svelte pin change is an independent `package.json` edit, not a transitive consequence of `vue 3.6.0-rc.1 → rc.3`.
   114	
   115	Practical cost beyond the scope rule: it inflates the candidate to 2426 files, which makes the Vue-side conformance evidence materially harder to audit — the effect is visible in this review, where separating Vue signal from Svelte churn required per-path filtering at every step.
   116	
   117	Required: split the Svelte oracle migration into its own commit/block under its own charter, or record a maintainer ruling widening BV0's scope. (No defect was found *in* the Svelte changes themselves — they are mechanical and internally consistent. The finding is scope, not correctness.)
   118	
   119	### Finding 3 — BLOCKING. A charter exit criterion is not structurally satisfied: link/runtime axes silently skip on an unprovisioned checkout.
   120	
   121	BV0's required exits state: *"The isolated oracle install is present so link checks genuinely execute."*
   122	
   123	`packages/framework-conformance-harness/bin/check-candidate.mjs` documents the opposite as the default state:
   124	
   125	> *"Skip semantics (hermeticity): link and runtime need the realized oracle install; when the one-time offline provisioning (`node scripts/provision-oracle-npm-cache.mjs`) has not been run, those axes report `skipped` … NEVER a fabricated pass."*
   126	
   127	and `official_seed_matrix.rs:366` folds that per-axis skip "through unmodified rather than special-casing it". On a clean checkout where the one-time provisioning has not run, all 36 cells pass with the **exact-package-link** and **deterministic-runtime** axes never executing.
   128	
   129	The harness's honesty (skip-with-reason, never a fabricated pass) is the right design and is not the problem. The problem is that the charter's exit asserts the install *is* present, and nothing in the tree enforces or verifies that — which is precisely the failure mode CLAUDE.md's **Verification Must Prove Execution (MANDATORY)** names ("unexpected prerequisite skips were zero"; "required source, build, and fixture prerequisites matched the tested tree").
   130	
   131	Required: either make the provisioned oracle install a hard, fail-closed prerequisite of the seed-matrix suite (the pattern `gate.mjs`'s build-prerequisite preflight already establishes — exit 127 with a `BUILD-PREREQUISITE MISSING` marker naming the producer command), or produce fresh evidence from a provisioned run showing zero skipped axes across all 36 cells.
   132	
   133	### Finding 4 — Non-blocking, needs ratification. The mapping acceptance axis was narrowed from candidate-vs-official to candidate-self-consistency.
   134	
   135	`compare.mjs` adds a well-argued block explaining why byte-exact `mappings`/`sourcesContent` comparison between two independently authored compilers is unsound (independent line-breaking, and the golden generator's `reAnchorMapLines` blank-line padding), and replaces it with a candidate-only check: valid schema, decodable VLQ, every segment resolving to in-bounds source coordinates, expected `sources` entry.
   136	
   137	The reasoning is sound and the new check is genuinely discriminating. Two caveats worth a ruling rather than a silent adoption:
   138	
   139	- BV0's owned scope item 3 names *"source-map differences **after harness artifacts are removed**"* — i.e. the charter's expectation is *strip the harness artifact, then compare and fix the residue*, not *abandon the comparison*. The commit identifies the harness artifact (`reAnchorMapLines`) but does not normalise it out and resume comparing.
   140	- CLAUDE.md's Compiled-Output Conformance rule lists **"sourcemap mappings"** among the categories that *"remain in contract"* — it is not on the cosmetic waiver list.
   141	
   142	This is still a net improvement over the prior state (`seed_conformance.rs` treated source maps as no conformance dimension at all), which is why it is not blocking. But narrowing a charter-named acceptance axis is an `ADOPT-NOW` / `DEFER` disposition decision, not an implementer call.
   143	
   144	### Finding 5 — Observation. Pre-existing Vue tracking artifacts remain, against a literal exit criterion.
   145	
   146	BV0's required exits state *"No Vue tracking, backlog, waiver, or retraction artifact remains."* Three survive:
   147	
   148	1. `corpus/known-divergences.json` — the seed-corpus parity backlog (84 rows). `official_seed_matrix.rs` correctly refuses to reuse it for its own domain and says so in its header, so BV0's own gate is clean; the artifact belongs to the separately-ratified seed corpus.
   149	2. `docs/arch/future/vue-vdom-parity-backlog.md` D6 — referenced from a **production** comment in `script/process.rs`, and that comment was *edited* by this commit (rc.1 → rc.3, scope narrowed to companion imports). This is the one instance BV0 itself touched and left standing.
   150	3. `docs/arch/ssr-noninline-shape-divergence.md` — a ratified interim divergence record, correctly **narrowed** here (signature-count parity achieved; only body routing remains divergent). This one is an improvement.
   151	
   152	Reading the exit criterion literally would require all three gone; reading it as scoped to artifacts BV0 would itself produce leaves all three legitimate. Flagged for the maintainer to confirm the intended reading rather than treated as a violation.
   153	
   154	---
   155	
   156	## 5. Explicitly checked and cleared (so the fix agent does not chase them)
   157	
   158	**The corpus divergence-count growth is mostly an artifact of the comparator descending further after a genuine fix, not a regression.** 48 of 85 pre-existing rows show a higher `total` than at `b64358705`, which looks alarming in isolation. Spot-checking the two worst VDOM cases explains it:
   159	
   160	- `script-setup/props-type-withdefaults|vdom`: before `["[structure] Program — child count: verter 6 vs golden 5"]`; after, that top-level mismatch is **gone** (BV0 removed the extra host-assembly statement) and the comparator now descends into the setup body, surfacing 9 previously-masked `private binding key` scope-ordinal diffs plus one real `ArrowFunctionExpression` vs `LogicalExpression` node-kind diff.
   161	- `v-on/inline|vdom`: identical pattern — top-level child count now matches, deeper diffs unmasked.
   162	
   163	Those cells got structurally **closer** to the oracle. The scope/decl-ordinal rows are the alpha-equivalence class CLAUDE.md keeps structural only because the oracle does not implement scope-aware alpha-equivalence; they cascade from the remaining structural diffs, as the file's own notes state. Likewise, the 17 rows whose totals *shrank* and the 3 `vdom|inline` rows deleted outright are genuine parity wins.
   164	
   165	**The vapor `v-on` discriminator rewrite is faithful to the new oracle, not a weakening.** `conformance_discriminator.rs` swaps its planted mutations from `_delegateEvents("click")` / `n0.$evtclick` to `_on(n0, "click", …)` / `"click"→"dblclick"`. Diffing the goldens confirms **official Vue itself** changed between rc.1 and rc.3 — rc.1 emitted `delegateEvents`/`createInvoker`/`$evtclick`, rc.3 emits `_on(...)`. Both replacement plants remain discriminating (`DiffDim::Structure` and `DiffDim::Literal`).
   166	
   167	**Other clean checks:** no `#[ignore]` added; no new refusal/`todo!`/`unimplemented!` in production; `verter_vue_conformance/src` untouched (so all corpus deltas are apples-to-apples); the `.prettierignore` addition is correctly justified (byte-verbatim vendored upstream source); `compare.mjs`'s per-PID importer path fixes a real concurrent-truncation race now that the Rust cells run in parallel.
   168	
   169	---
   170	
   171	## 6. Assessment of the Vapor rewrite (in-scope, no defect found)
   172	
   173	The `block_plan` two-pass design is the correct architecture for the charter's item 3 ("invalid Vapor module references/imports, fragment and patch topology"): the previous walker-driven emitter could not express the insertion-site contract the Vapor runtime expects, and the boundary test now runs *before* a child is appended to the shell rather than being inferred from depth. The `IfOp`/`ForOp`/`SlotOp` flag packers (`if_flags`, `for_flags`) derive their bits from the block's own semantic facts (shape, inertness, single-returned-child) rather than from output text, which is the right side of the typed-IR line. Placement, visibility, and shared-owner reuse are all correct.
   174	
   175	Its size (2279 lines replacing ~1600) is well beyond "minimum reusable correction" as the charter's procedure phrases it, but the charter also expressly owns "shared lower-owner corrections when the same root cause serves multiple Vue backends," and the deleted `comment.rs`/`text.rs` plus the removed dead `VaporRootElement`/`reset()`/`observe_dom_*` surface show this is a replacement, not an addition alongside a preserved legacy path — consistent with the Legacy Code Deletion rule. No architecture objection.
   176	
   177	---
   178	
   179	VERDICT: BLOCKING
   180	
   181	1. Two previously-passing Vue VDOM cells (`components/dynamic-multi-root|vdom|non-inline`, `elements-text/multi-root|vdom|non-inline`) regressed to a module-item ordering divergence and were absorbed as **new** `known-divergences.json` rows rather than fixed. Both goldens are byte-identical between rc.1 and rc.3 and the comparator is unchanged, so this is Verter-side. Violates BV0's required procedure ("prove unaffected routes retain their prior successful result contract") and leans on the allowlist mechanism the charter's owned scope bars BV0 from introducing. Fix the lowering order and delete both rows, or obtain a recorded maintainer `DEFER` with a debt row.
   182	2. An out-of-scope Svelte oracle migration (`5.56.3 → 5.56.8`: ~2027 corpus files, 3 production `src/svelte/runtime` constants, 9 `package.json` + lockfile) rides in a Vue-only charter, one commit after `fdb6f6291` explicitly split Svelte work into a separate block. Split it out or obtain a recorded scope widening.
   183	3. The charter exit "The isolated oracle install is present so link checks genuinely execute" is not structurally satisfied — `check-candidate.mjs` reports link and runtime axes as `skipped` whenever the one-time `provision-oracle-npm-cache.mjs` provisioning has not run, and `official_seed_matrix.rs` folds that skip through unmodified, so all 36 cells pass on a clean checkout without either axis executing. Make the provisioned install a fail-closed prerequisite, or supply fresh evidence of a provisioned run with zero skipped axes.

exec
/bin/zsh -lc 'nl -ba docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md' in <repo-root>
 succeeded in 0ms:
     1	# BV0 — Adversarial review (candidate `c40a1ca96`, base `b64358705`)
     2	
     3	Charter: `docs/arch/refactor/rev11/charters/BV0.md`
     4	Reviewer posture: assume every test is weak until a real red/green plant proves otherwise.
     5	Worktree: `<worktree>/verter-review-bv0-adv`, left byte-identical to start
     6	(`git status --porcelain` empty, `HEAD == c40a1ca96b73cf9b723fd7209516ea0462deaad2`).
     7	
     8	---
     9	
    10	## 0. Environment note that materially changed the review
    11	
    12	On a fresh checkout the seed matrix's **link** and **runtime** JS axes SKIP, because
    13	`.oracle-installs/` and `.oracle-npm-cache/` are gitignored and require a one-time network
    14	provisioning step. `js_axis_reasons` (official_seed_matrix.rs:466-486) treats `skipped` as
    15	informational only — never a failure. So on any unprovisioned machine all 36 cells pass with
    16	2 of 4 axes never executing.
    17	
    18	I provisioned it (`node packages/framework-conformance-harness/scripts/provision-oracle-npm-cache.mjs`)
    19	and verified all four axes genuinely run:
    20	
    21	```
    22	{"parse":{"status":"ran","ok":true},"mapping":{"status":"ran","ok":true},
    23	 "link":{"status":"ran","ok":true},"runtime":{"status":"ran","ok":true}}
    24	```
    25	
    26	The three `vapor_runtime_behavior` cases also genuinely execute (no `SKIP` line with `--nocapture`)
    27	against the real pinned `vue.runtime-with-vapor.esm-browser.js` in jsdom.
    28	
    29	**Everything below was measured with the oracle genuinely installed.**
    30	
    31	---
    32	
    33	## 1. Mutation log (plant → red → restore → green)
    34	
    35	Every mutation was applied with a Python patcher that `assert`s the target string occurs
    36	**exactly once** before writing, and was verified present by `grep` afterwards — so a plant that
    37	failed to apply could not be silently read as "the code is correct". Every mutation was restored
    38	and the suite re-run green.
    39	
    40	| # | Production fix reverted | File | Test(s) expected to guard it | Result |
    41	|---|---|---|---|---|
    42	| M1 | `bind_expose`/`emit_bare_expose_call` → pre-fix (`has_expose` / `false`) | `script/process.rs:470-471` | `non_inline_no_define_expose_binds_and_emits_bare_expose_call` | **RED** ✔ |
    43	| M2 | Restore `if is_ssr { plain return }` around `__isScriptSetup` | `script/process.rs` `build_setup_wrapper_end` | `ssr_setup_return_carries_script_setup_marker` | **RED** ✔ |
    44	| M3 | Re-add `names.sort()` (alphabetical `__returned__`) | `script/process.rs:953` | `build_returned_preserves_declaration_order_not_alphabetical` | **RED** ✔ |
    45	| M4 | `is_vapor && is_ssr` → `is_vapor` | `script/process.rs` `build_setup_wrapper_end` | `build_wrapper_end_vapor_non_ssr_no_vapor_flag` | **RED** ✔ (but see BLOCKING-1 — it guards the *wrong* behavior) |
    46	| M5 | Drop `|| element.tag_type.is_slot_outlet()` from `is_slot_parent` | `vdom/mod.rs` | `slot_outlet_fallback_static_element_marked_slot_cached`, `..._two_static_elements_grouped_not_double_wrapped` | **RED** ✔ (2 tests) |
    47	| M5b | Drop the `-1 /* CACHED */` close on cached static text | `vdom/slots.rs:1754` | `slot_outlet_fallback_static_text_uses_cache_with_cached_flag` | **RED** ✔ |
    48	| M6 | `for_item_needs_own_block` → `el.v_for.is_some()` | `vdom/mod.rs` | `v_for_constant_source_emits_stable_fragment_without_item_block` | **RED** ✔ |
    49	| M7 | Drop the anchor argument from `_setInsertionState(nP, nA)` | `vapor/block_plan/emit.rs:313` | seed-matrix vapor cells / `vapor_runtime_behavior::*` | **RED structurally, GREEN behaviorally** — see NON-BLOCKING-1 |
    50	
    51	Detail:
    52	
    53	- **M1** — 1 unit test red; **24 of 36 seed-matrix cells red**. The 12 green cells are all `slots`,
    54	  whose fixture has no `<script>` block at all (verified) — correct, not a coverage hole. Both
    55	  negative controls (`inline_mode_no_define_expose_omits_bind_and_bare_expose_call`,
    56	  `non_inline_with_authored_define_expose_output_is_byte_identical`) stayed green.
    57	- **M3** — 1 of 8 `build_returned_*` tests red; the other 7 correctly insensitive.
    58	- **M6** — 1 of 133 `v_for_*` tests red; the negative controls
    59	  (`v_for_ref_source_keeps_keyed_fragment_and_item_block`,
    60	  `v_for_let_source_without_key_keeps_unkeyed_fragment`,
    61	  `v_for_const_member_expression_source_stays_keyed`) stayed green.
    62	- **M5/M5b** — the two halves of the slot fix are *independently* guarded; neither test covers both.
    63	  The author's own note on `slot_outlet_fallback_static_element_marked_slot_cached` honestly states it
    64	  does not discriminate alone and names the companion test that does. That is accurate.
    65	
    66	### Extra plant: the SSR runtime axis itself
    67	
    68	To check the runtime oracle isn't decorative I fed three SSR candidates through
    69	`bin/check-candidate.mjs` (plants proven applied by counting occurrences before running):
    70	
    71	| variant | runtime axis |
    72	|---|---|
    73	| golden verbatim | `ran`, ok |
    74	| `$setup.` → `_ctx.` | `ran`, **FAILED** — `<ul><!--[--><!--]--></ul>` vs golden's three `<li>` |
    75	| `<p>zero</p>` → `<p>ZERO</p>` | `ran`, **FAILED** — HTML diverges |
    76	
    77	The runtime axis genuinely executes and discriminates. It also **empirically confirms** the
    78	`__isScriptSetup` reasoning: with the marker present, `_ctx.<setupBinding>` does *not* resolve
    79	(the pinned runtime warns `Property "count" was accessed during render but is not defined on
    80	instance`). Official rc.3's `ssrRender` reads `$setup.count`, and BV0 correspondingly deleted
    81	`is_ssr` from `BindingResolver`. That change is coherent and correct.
    82	
    83	---
    84	
    85	## 2. Tests that pass regardless of the fix
    86	
    87	No `assert!(true)`, `assert_eq!(1,1)`, `|| true`, or empty `#[test] fn …() {}` bodies anywhere in
    88	the diff's test surface (33 217-line test diff scanned). `known_divergences_file_is_well_formed`
    89	actively forbids the auto-generated `TODO:` note, and no ledger entry carries one.
    90	`candidate-axes.spec.mjs` (14 tests, 64 assertions) is a genuine one-mutation-one-axis
    91	discrimination suite; it passes.
    92	
    93	One real gap rather than a tautology: `candidate-axes.spec.mjs` covers parse and mapping
    94	discrimination only. There is no committed link/runtime-axis mutation control (it cannot run
    95	without the oracle install). I supplied that control manually above.
    96	
    97	---
    98	
    99	## 3. Cells passing for a coincidental / wrong reason
   100	
   101	This is where the review breaks.
   102	
   103	### The Vapor golden's script half was never compiled as Vapor
   104	
   105	`packages/framework-conformance-harness/src/invoke-vue-oracle.mjs:127`:
   106	
   107	```js
   108	const compiled = compileScript(descriptor, { id: filename, inlineTemplate: false, sourceMap });
   109	```
   110	
   111	`vapor` and `ssr` are passed to `compileTemplate` (lines 150, 168) but **never to `compileScript`**.
   112	Official rc.3 derives them there as `vapor = sfc.vapor || options.vapor` and
   113	`ssr = options.templateOptions?.ssr` (`compiler-sfc.cjs.js:15385-15386`). So every "official"
   114	vapor golden in `goldens/records/` had its script half compiled as a **non-vapor, non-SSR**
   115	component.
   116	
   117	Proven directly against the pinned oracle, on the harness's own `fixtures/vue/basic-interpolation.vue`:
   118	
   119	```
   120	compileScript(descriptor, {})                    -> __vapor: false  defineVaporComponent: false   <- what the harness calls
   121	compileScript(descriptor, {vapor:true})          -> __vapor: true   defineVaporComponent: false   <- what a real vapor build calls
   122	```
   123	
   124	and on a TS `<script setup lang="ts">`: `{vapor:true}` → `defineVaporComponent: true`.
   125	
   126	`assembleNonInline({ scriptCode, renderCode, ssr })` (line 289) destructures only `ssr` and drops
   127	`vapor`, so nothing restores the marker at assembly either.
   128	
   129	Corroboration from inside this repo: **every** rc.3 corpus vapor golden carries `__vapor: true`
   130	(e.g. `crates/verter_vue_conformance/corpus/goldens/3.6.0-rc.3/vapor/conditionals/if-else-if-else.js`),
   131	while **no** harness vapor golden does. Two golden generators, same official version, opposite
   132	answers.
   133	
   134	### Verter's production code was then changed to match the defective golden
   135	
   136	`crates/verter_compiler/src/script/process.rs` — three sites changed `if options.is_vapor` to
   137	`if options.is_vapor && options.is_ssr`, justified by:
   138	
   139	> "Official only emits this marker for SSR (`ssr && vapor` — see `ScriptCodeGenOptions::is_ssr`)"
   140	
   141	That quotes only `compiler-sfc.cjs.js:15731`, which is inside the **`if (ctx.isTS)`** branch. The
   142	`else` (JS) branch, four lines later at **15736**, is:
   143	
   144	```js
   145	} else {
   146	    if (vapor) runtimeOptions += `\n  __vapor: true,`;
   147	```
   148	
   149	— unconditional for vapor. And in the TS branch official substitutes
   150	`defineVaporComponent` (`vapor && !ssr`), which Verter does not emit at all (`grep defineVaporComponent
   151	crates/` → no hits). So the accompanying claim "a component's own wrapper is otherwise identical
   152	between the VDOM and Vapor backends" is false on both branches.
   153	
   154	`__vapor` is load-bearing in the pinned runtime, not a cosmetic tag —
   155	`isVaporComponent` is literally `return type.__vapor` (line 7412), and it gates VDOM↔Vapor
   156	interop mounting (3933, 6779, 15362, 15381) and vapor-app component classification
   157	(`if (isInteropEnabled && appContext.vapor && !component.__vapor)`, 13275).
   158	
   159	**Why the matrix cannot see this:** the runtime axis mounts candidate *and* golden through
   160	`mountVueVapor` and compares DOM. Both now lack `__vapor`, so both behave identically. Structural
   161	comparison compares candidate against the same defective golden. Two bugs cancelling — exactly the
   162	class this review was asked to hunt. And the new unit test
   163	`build_wrapper_end_vapor_non_ssr_no_vapor_flag`, added by this commit, **locks the defect in**.
   164	
   165	### Vapor nested block-plan and SSR `__isScriptSetup`, cross-checked against real behavior
   166	
   167	Both check out on the merits:
   168	
   169	- **SSR `__isScriptSetup`**: verified against the live pinned server renderer (§1 extra plant).
   170	  Marker + `$setup.*` routing is the official pairing; `_ctx.*` routing genuinely breaks under the
   171	  marker, and BV0 removed `_ctx.*` routing in the same change. Coherent.
   172	- **Vapor nested block plan**: the emitted module and live DOM are correct —
   173	  `_setInsertionState(n6, n7)` produces
   174	  `<div class="root"><p>yes</p><!--if--><ul><li>a</li><li>b</li><li>c</li><!--for--></ul></div>`,
   175	  source order preserved, branch swap leaves no stale node, list reorder moves rather than
   176	  duplicates. Confirmed by dumping the real module + per-step HTML from the jsdom driver.
   177	
   178	---
   179	
   180	## 4. Undisclosed conformance regression, attributed
   181	
   182	The seed corpus's tracked-divergence ledger grew from 361 to 694 reasons. To separate the
   183	`3.6.0-rc.1 → rc.3` oracle re-pin (also in this commit) from BV0's code, I ran the corpus with the
   184	**base compiler** against the **candidate's rc.3 goldens**:
   185	
   186	| tree | cells | divergence reasons |
   187	|---|---|---|
   188	| base compiler @ rc.1 goldens | 85 | 361 |
   189	| base compiler @ rc.3 goldens | 85 | 376 |
   190	| **candidate compiler @ rc.3 goldens** | 84 | **694** |
   191	
   192	The re-pin costs +15. **BV0's code costs +318 (+85%).** 48 cells worse, 17 better, 17 unchanged;
   193	2 cells that PASSED now diverge (`components/dynamic-multi-root` / `elements-text/multi-root`,
   194	VDOM non-inline, import-lowering order); 3 newly pass.
   195	
   196	VDOM improved broadly. **Every worst regression is `vapor non-inline`**, and they are semantic,
   197	not cosmetic:
   198	
   199	- `v-on/modifiers` 6→28 — *"Verter-only: `createInvoker`; official-only: `child`, `next`,
   200	  `withModifiers`, `withVaporModifiers`"*, plus `missing imported helper 'withModifiers'`.
   201	  Event modifiers are not routed through the official modifier helpers.
   202	- `conditionals/if-else-if-else` 3→22 — Verter bakes `"<p>Done:  "` into the static template where
   203	  official emits `"<p> "` plus `txt`/`setText`/`renderEffect`. Interpolated text folded into static
   204	  markup.
   205	- `components/child-comp` 7→22, `v-bind/prop-attr-modifiers` 2→18, `v-on/key-modifiers` 5→20,
   206	  `v-model/{checkbox,input,select}` 2→15 / 1→13 / 1→13, `v-bind/static-dynamic` 1→12.
   207	
   208	The commit message describes only corrections and says nothing about this.
   209	
   210	---
   211	
   212	## 5. Charter-exit items that are not met
   213	
   214	- *"The isolated oracle install is present so link checks genuinely execute."* — Nothing in this
   215	  commit makes that true for CI or a fresh clone; the axes skip silently and the cell still passes.
   216	  The commit message's "All 36 cells pass across every axis" holds only after manual provisioning.
   217	- *"No Vue tracking, backlog, waiver, or retraction artifact remains."* — See BLOCKING-2. Also, new
   218	  comments added by this commit cite `docs/arch/future/vue-vdom-parity-backlog.md` D6 as a live
   219	  tracked divergence, and the 84-cell `known-divergences.json` waiver ledger grew 85%.
   220	
   221	---
   222	
   223	## 6. Required verification runs (capped, targeted)
   224	
   225	`CARGO_BUILD_JOBS=3`, `--test-threads=3`, on the clean candidate tree, oracle provisioned:
   226	
   227	```
   228	cargo test -p verter_vue_conformance --test main -- --test-threads=3
   229	  test result: ok. 47 passed; 0 failed; 0 ignored   (36 matrix cells, all 4 axes live)
   230	
   231	cargo test -p verter_compiler -- --test-threads=3
   232	  test result: ok. 6001 passed; 0 failed; 5 ignored
   233	  test result: ok.  496 passed; 0 failed; 0 ignored
   234	  (0 FAILED lines in the full log)
   235	```
   236	
   237	Both clean. `git status --porcelain` empty afterwards.
   238	
   239	> Process note: during the base-compiler attribution experiment (§4) a
   240	> `git checkout <base> -- crates/…` staged base content into the index, so a subsequent
   241	> `git checkout -- …` restored *from the index* rather than from HEAD. Caught it when the
   242	> "final" run reported 40 failures, fixed with `git reset --hard HEAD`, and re-ran both suites
   243	> from a verified-clean tree — the numbers above are the clean-tree ones. All mutation results in
   244	> §1 predate that experiment and are unaffected.
   245	
   246	---
   247	
   248	## Findings
   249	
   250	### BLOCKING
   251	
   252	**1. `__vapor: true` is now dropped from every non-SSR Vapor build, to match a golden that was
   253	never compiled as Vapor.**
   254	`invoke-vue-oracle.mjs:127` omits `vapor`/`ssr` from `compileScript`, so the harness's vapor
   255	goldens are not official vapor output (proven: `{vapor:true}` → `__vapor: true`, `{}` → nothing, on
   256	the harness's own fixture with the pinned rc.3 compiler). `script/process.rs` ×3 was changed to
   257	`is_vapor && is_ssr` on a justification that quotes only the TS branch (`compiler-sfc.cjs.js:15731`)
   258	while the JS branch (15736) is `if (vapor)` unconditional; the repo's own rc.3 corpus vapor goldens
   259	all carry the marker. `__vapor` is `isVaporComponent` in the runtime and gates VDOM↔Vapor interop.
   260	The new test `build_wrapper_end_vapor_non_ssr_no_vapor_flag` locks the defect in, and the seed
   261	matrix cannot detect it because candidate and golden are wrong identically.
   262	*Fix:* pass `vapor` (and `templateOptions.ssr`) to `compileScript`, regenerate the vapor goldens,
   263	restore `if (is_vapor)` for the JS branch, and decide `defineVaporComponent` for TS separately.
   264	
   265	**2. `docs/arch/ssr-noninline-shape-divergence.md` — edited by this commit — asserts the opposite of
   266	the code this commit ships.**
   267	It still states the SSR component "does **not** set `__isScriptSetup`", that "every binding routes
   268	through `_ctx.*` … never `$setup.*`", and cites a guard "(`__isScriptSetup` present client / absent
   269	SSR)" that this commit inverted. Its own exit criterion — *"the marker returns, bindings route
   270	through `$setup.*`, and this document is deleted in the same change"* — was **met by this commit**,
   271	yet the document was rewritten to re-assert the divergence rather than deleted. Directly against
   272	the charter's "No Vue tracking, backlog, waiver, or retraction artifact remains."
   273	
   274	**3. An 85% growth in tracked Vue conformance divergences, attributable to BV0's code and
   275	undisclosed.** Base@rc.3 376 → candidate@rc.3 694 reasons; 48 cells worse, 2 previously-passing
   276	cells now divergent; every worst regression is Vapor and semantic (missing
   277	`withModifiers`/`withVaporModifiers`, interpolated text folded into static template markup, missing
   278	`txt`/`setText`). Needs either a fix or an explicit ratified disposition — silently regenerating the
   279	waiver ledger is not one.
   280	
   281	### NON-BLOCKING
   282	
   283	**1. The behavioral Vapor suite does not discriminate boundary DOM position.** Plant M7 (drop the
   284	`_setInsertionState` anchor) renders
   285	`<div class="root"><!----><ul>…</ul><p>yes</p><!--if--></div>` — the `v-if` block lands *after* its
   286	sibling `<ul>` instead of before it — and all three `vapor_runtime_behavior` tests stay green.
   287	`initial.starts_with("<div class=\"root\">") && initial.contains("<ul>")` is satisfied by a
   288	wrongly-ordered tree. The structural comparator did catch M7 (4 cells red), so the production fix is
   289	guarded — but the suite's own headline claim ("a generated `_next(n9)` lands on the node a block is
   290	inserted before") is not what it tests. Assert full expected HTML, or the sibling order explicitly.
   291	
   292	**2. Link/runtime axes silently skip on any unprovisioned checkout** and skips are non-failing, so
   293	"36/36 across every axis" is not reproducible from a clone. Either provision in CI, or make a skip
   294	fail when an env flag marks the run as gate-authoritative.
   295	
   296	**3. Stale comment** `// $setup dot notation (SSR uses _ctx.x like VDOM)` —
   297	`template/code_gen/ssr/tests.rs:2129`, contradicted by the two tests directly beneath it.
   298	
   299	VERDICT: BLOCKING
   300	
   301	1. `__vapor: true` removed from every non-SSR Vapor build to match a harness golden whose script half was compiled with `vapor` unset (`invoke-vue-oracle.mjs:127`); the justifying comment reads only official's TS branch while the JS branch emits it unconditionally; locked in by a new test the matrix cannot contradict because candidate and golden share the defect.
   302	2. `docs/arch/ssr-noninline-shape-divergence.md`, edited in this same commit, asserts SSR omits `__isScriptSetup` and routes `_ctx.*` — both removed by this commit; its stated exit criterion was met and the document should have been deleted, not re-asserted (charter: no Vue tracking/waiver artifact remains).
   303	3. Undisclosed +85% growth in tracked Vue conformance divergences attributable to BV0's code (376 → 694 with the oracle re-pin factored out), concentrated in Vapor and semantic in nature (missing `withModifiers`/`withVaporModifiers`, interpolation folded into static template markup), including 2 previously-passing cells.

codex
The key distinction is already visible: correcting the oracle is necessary to make BV0’s own evidence truthful, but changing a shared BF2-owned harness has a wider blast radius than BV0’s source-fix authority. I’m checking the program’s reopen/dependency rules now so the process ruling—especially for already accepted blocks—is grounded rather than inferred.
exec
/bin/zsh -lc 'rg -n --hidden -S "REOPEN|reopen|ACCEPTED|accepted|invalidate|invalidation|scope consult|ADOPT-NOW|whole class|comprehensive|dependent|dependency|oracle|shared harness|BF3|Svelte" docs/arch/refactor/rev11/ORCHESTRATOR.md docs/arch/refactor/rev11/charters docs/arch/refactor/rev11/evidence/maintainer-rulings.md docs/arch/refactor/rev11 | head -n 700' in <repo-root>
 succeeded in 42ms:
docs/arch/refactor/rev11/ORCHESTRATOR.md:16:- delegate substantial independent work through immutable context packets;
docs/arch/refactor/rev11/ORCHESTRATOR.md:20:You must not silently change architecture, weaken a gate, widen a charter, self-approve, or treat your own summary as independent review.
docs/arch/refactor/rev11/ORCHESTRATOR.md:106:Do not start `A1` or any production cutover until A0 has been accepted under `governance.md`.
docs/arch/refactor/rev11/ORCHESTRATOR.md:125:Do not create a program-wide stack. During A0, record whether GitHub native stacks, ordinary dependent PRs, merge queues, and signed rebases are available. A6 selects the operational tooling and locks a bounded stack-window policy. The default maximum is four open review layers; the permitted A6 range is two through six.
docs/arch/refactor/rev11/ORCHESTRATOR.md:127:Private atomic layers, especially D1, are never independently mergeable to trunk.
docs/arch/refactor/rev11/ORCHESTRATOR.md:141:- actual model/runtime differs from the Opus adapter without an accepted exception.
docs/arch/refactor/rev11/evidence/maintainer-rulings.md:39:1. **Python validators vs the no-Python dependency policy.**
docs/arch/refactor/rev11/evidence/maintainer-rulings.md:81:rebase-reconcile later / abandon / coordinate as predecessor-dependent block). This
docs/arch/refactor/rev11/evidence/maintainer-rulings.md:122:`A3` must consume those accepted facts as its sole G10 discriminator, while `D6` /
docs/arch/refactor/rev11/evidence/maintainer-rulings.md:138:2. Move structural G10 discrimination from an independent A2C skeleton-fact owner
docs/arch/refactor/rev11/evidence/maintainer-rulings.md:154:Maintainer decision (verbatim): **ACCEPTED IN FULL**.
docs/arch/refactor/rev11/evidence/maintainer-rulings.md:183:the four artifacts; whichever accepted candidate immediately precedes the first opened
docs/arch/refactor/rev11/evidence/maintainer-rulings.md:190:digest before `A6`'s candidate is accepted — tracked as open work on `A6`, not a
docs/arch/refactor/rev11/evidence/maintainer-rulings.md:244:  the accepted candidate immediately preceding that event must deliver the Node
docs/arch/refactor/rev11/charters/B1.template.md:9:Land dependency-neutral types and canonical encodings needed by later blocks without creating service owners or migrating semantic behavior.
docs/arch/refactor/rev11/charters/B1.template.md:26:- exact dependency graph before/after;
docs/arch/refactor/rev11/charters/B1.template.md:36:Stop for an undiscovered public/wire consumer, incompatible persisted domain, dependency cycle, missing canonical equality material, or a field whose profile/identity class is ambiguous.
docs/arch/refactor/rev11/charters/B1.template.md:9:Land dependency-neutral types and canonical encodings needed by later blocks without creating service owners or migrating semantic behavior.
docs/arch/refactor/rev11/charters/B1.template.md:26:- exact dependency graph before/after;
docs/arch/refactor/rev11/charters/B1.template.md:36:Stop for an undiscovered public/wire consumer, incompatible persisted domain, dependency cycle, missing canonical equality material, or a field whose profile/identity class is ambiguous.
docs/arch/refactor/rev11/charters/A5.md:26:Resolved current-owner rows; option classification; domains; consumer/protocol map; dependency test plan; evidence custody; program-state workflow; worktree/branch/CI/merge rules; bounded stack-window and review-context policy.
docs/arch/refactor/rev11/charters/A5.md:34:Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A5` is accepted only when its evidence is attached to one unchanged candidate SHA/tree and evidence digest.
docs/arch/refactor/rev11/OPUS-START-HERE.md:69:Do not treat several identically prompted model instances as automatically independent. Foundational review requires distinct mandates, clean contexts, direct evidence, and exact-candidate binding; a different model or human reviewer is valuable where available.
docs/arch/refactor/rev11/charters/BF1.md:3:**Status:** PROPOSED / LOCKED pending AMD-005 ratification. B1 is accepted.
docs/arch/refactor/rev11/charters/BF1.md:13:- exact Vue RC.3 and Svelte 5.56.8 source/package closures and integrity;
docs/arch/refactor/rev11/charters/BF1.md:16:- complete Vue and Svelte semantics-affecting option classifications;
docs/arch/refactor/rev11/charters/BF1.md:19:- emitter/mapping dispositions and BF3 probe inventory;
docs/arch/refactor/rev11/charters/BF1.md:36:6. Performance cells, correctness oracles, repetitions, thresholds, counters, memory
docs/arch/refactor/rev11/charters/BF1.md:3:**Status:** PROPOSED / LOCKED pending AMD-005 ratification. B1 is accepted.
docs/arch/refactor/rev11/charters/BF1.md:13:- exact Vue RC.3 and Svelte 5.56.8 source/package closures and integrity;
docs/arch/refactor/rev11/charters/BF1.md:16:- complete Vue and Svelte semantics-affecting option classifications;
docs/arch/refactor/rev11/charters/BF1.md:19:- emitter/mapping dispositions and BF3 probe inventory;
docs/arch/refactor/rev11/charters/BF1.md:36:6. Performance cells, correctness oracles, repetitions, thresholds, counters, memory
docs/arch/refactor/rev11/charters/A2.md:26:Exact expectations; negative controls; crossed matrices; oracle/profile stamps.
docs/arch/refactor/rev11/charters/A2.md:34:Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A2` is accepted only when its evidence is attached to one unchanged candidate/evidence SHA.
docs/arch/refactor/rev11/program-dag.toml:59:name = "Neutral contracts identities profiles dependency firewall"
docs/arch/refactor/rev11/program-dag.toml:76:id = "BF3"
docs/arch/refactor/rev11/program-dag.toml:91:predecessors = ["BV0", "BF3"]
docs/arch/refactor/rev11/program-dag.toml:97:predecessors = ["BV0", "BF3"]
docs/arch/refactor/rev11/program-dag.toml:113:name = "Svelte compiler-output conformance train"
docs/arch/refactor/rev11/charters/A4.md:34:Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A4` is accepted only when its evidence is attached to one unchanged candidate/evidence SHA.
docs/arch/refactor/rev11/charters/A4.md:34:Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A4` is accepted only when its evidence is attached to one unchanged candidate/evidence SHA.
docs/arch/refactor/rev11/charters/B6.md:5:B6 exposes B5's accepted algorithms through prepared-first, prepared-repeat, and
docs/arch/refactor/rev11/charters/B6.md:12:route-overhead cells. C4 remains locked until B6 is accepted because these routes do
docs/arch/refactor/rev11/charters/C4.md:6:direct batch, and project-aware staged compilation for every applicable accepted Vue
docs/arch/refactor/rev11/charters/C4.md:7:and Svelte cell. Equality covers typed status, requested artifacts, JavaScript
docs/arch/refactor/rev11/decisions/ADR-006-demand-selected-flow-domains.md:13:Structural authored-return collection is independent of endpoint completion. A private obligation ledger and finalizer construct complete results; a query cannot omit required domains manually.
docs/arch/refactor/rev11/decisions/ADR-006-demand-selected-flow-domains.md:17:Narrow queries stay narrow without introducing a second evaluator. Domains are independently testable while completeness remains global to the requested result.
docs/arch/refactor/rev11/decisions/ADR-006-demand-selected-flow-domains.md:22:- independent per-query semantic evaluators;
docs/arch/refactor/rev11/templates/landing-equivalence.template.toml:19:accepted_base_sha = "REQUIRED_FULL_SHA"
docs/arch/refactor/rev11/templates/landing-equivalence.template.toml:20:accepted_base_tree = "REQUIRED_TREE_OID"
docs/arch/refactor/rev11/templates/landing-equivalence.template.toml:21:accepted_sha = "REQUIRED_FULL_SHA"
docs/arch/refactor/rev11/templates/landing-equivalence.template.toml:22:accepted_tree = "REQUIRED_TREE_OID"
docs/arch/refactor/rev11/templates/landing-equivalence.template.toml:24:accepted_patch_digest = "REQUIRED_CANONICAL_GIT_DIFF_SHA256"
docs/arch/refactor/rev11/templates/landing-equivalence.template.toml:26:accepted_generated_digest = "NOT_APPLICABLE_OR_REQUIRED_GENERATED_OUTPUT_SHA256"
docs/arch/refactor/rev11/templates/landing-equivalence.template.toml:39:# evaluated in the repository with LC_ALL=C. Candidate and accepted patch
docs/arch/refactor/rev11/templates/landing-equivalence.template.toml:43:# accepted_base_sha must be the accepted commit's first parent, so one program
docs/arch/refactor/rev11/charters/C4.md:6:direct batch, and project-aware staged compilation for every applicable accepted Vue
docs/arch/refactor/rev11/charters/C4.md:7:and Svelte cell. Equality covers typed status, requested artifacts, JavaScript
docs/arch/refactor/rev11/templates/program-state.template.toml:37:model_exception_accepted = false
docs/arch/refactor/rev11/templates/program-state.template.toml:44:# Status: LOCKED | READY | IN_PROGRESS | REVIEW | ACCEPTANCE_RECOMMENDED | ACCEPTED |
docs/arch/refactor/rev11/templates/program-state.template.toml:48:# accepted_sha/tree = actual landed commit and full repository tree; they may differ
docs/arch/refactor/rev11/templates/program-state.template.toml:50:# both base/candidate/accepted identities and exact candidate-delta/post-landing proof.
docs/arch/refactor/rev11/templates/program-state.template.toml:60:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:61:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:81:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:82:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:102:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:103:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:123:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:124:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:144:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:145:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:165:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:166:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:186:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:187:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:207:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:208:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:228:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:229:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:249:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:250:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:270:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:271:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:284:id = "BF3"
docs/arch/refactor/rev11/templates/program-state.template.toml:291:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:292:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:312:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:313:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:333:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:334:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:354:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:355:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:375:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:376:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:396:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:397:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:417:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:418:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:438:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:439:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:459:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:460:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:480:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:481:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:501:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:502:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:522:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:523:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:543:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:544:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:564:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:565:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:585:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:586:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:606:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:607:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:627:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:628:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:648:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:649:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:669:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:670:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:690:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:691:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:711:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:712:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:732:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:733:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:753:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:754:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:774:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:775:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:795:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:796:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:816:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:817:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:837:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:838:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:858:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:859:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:879:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:880:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:900:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:901:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:921:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:922:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:942:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:943:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:963:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:964:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:984:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:985:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1005:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1006:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1026:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1027:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1047:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1048:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1068:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1069:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1089:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1090:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1110:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1111:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1131:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1132:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1152:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1153:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1173:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1174:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1194:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1195:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1215:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1216:accepted_tree = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1236:accepted_sha = ""
docs/arch/refactor/rev11/templates/program-state.template.toml:1237:accepted_tree = ""
docs/arch/refactor/rev11/charters/BS1.md:1:# BS1 — Svelte compiler-output conformance train
docs/arch/refactor/rev11/charters/BS1.md:8:Deliver Svelte compiler correctness on the final B2–B4 substrate for exact
docs/arch/refactor/rev11/charters/BS1.md:13:- Svelte-owned semantic model and product plans;
docs/arch/refactor/rev11/charters/BS1.md:16:- Svelte-native helpers, effects, blocks, events, bindings, actions, transitions,
docs/arch/refactor/rev11/charters/BS1.md:21:- the complete accepted official-core Svelte pack.
docs/arch/refactor/rev11/charters/BS1.md:23:BS1 does not implement a Svelte runtime, use an official compiler in production, or
docs/arch/refactor/rev11/charters/BS1.md:31:no blocked official case or semantic known-divergence. Structural and dependency
docs/arch/refactor/rev11/charters/BS1.md:33:component, or SSR assumption remains anywhere in Svelte implementation paths. Every
docs/arch/refactor/rev11/charters/BS1.md:34:corresponding BF3 guard is removed.
docs/arch/refactor/rev11/charters/BS1.md:39:success, an output requiring runtime patching, or any design that bases Svelte on Vue.
docs/arch/refactor/rev11/decisions/ADR-013-result-contracts-and-flight-owned-production.md:24:The `FlightCell`, not the first waiter, owns production. Waiters register independently. The producer continues while valid waiters remain, receives bounded priority/budget aggregation, finalizes exactly once, and publishes only through the owner's sealed admission path. Every follower validates before use.
docs/arch/refactor/rev11/charters/BV0.md:16:matrix and independently authored controls. It expressly owns:
docs/arch/refactor/rev11/charters/BV0.md:34:For each defect: add a failing independently authored regression first; reproduce
docs/arch/refactor/rev11/charters/BV0.md:35:against the exact rc.3 oracle; identify the existing typed compiler-owner root cause;
docs/arch/refactor/rev11/charters/BV0.md:41:It must be connected to the BF2 comparator, parse/link/runtime/map oracles, and
docs/arch/refactor/rev11/charters/BV0.md:48:runtime/server, diagnostic, and mapping checks. The isolated oracle install is
docs/arch/refactor/rev11/decisions/ADR-009-shared-frontends-and-parse-owner-domains.md:5:**Reopen only if:** an accepted ADR proves two genuinely incompatible syntax products are both necessary and cannot share one frontend without material product loss.
docs/arch/refactor/rev11/decisions/ADR-009-shared-frontends-and-parse-owner-domains.md:9:Including consumer role or vague parse flavor in identity can legitimize permanent IDE/build parser duplication. Interpreting “one parse identity” process-globally would force independent direct calls through global synchronization.
docs/arch/refactor/rev11/decisions/ADR-009-shared-frontends-and-parse-owner-domains.md:23:Consumer role is not a key dimension. `ParseProductKind` is used only for a genuinely incompatible syntax product and requires a separate accepted ADR.
docs/arch/refactor/rev11/decisions/ADR-009-shared-frontends-and-parse-owner-domains.md:25:One live parse instance has one owner/result. Independent direct owner domains may parse independently. Retaining domains may pressure-evict and later perform one visible same-key reparse flight. Authored locators are revalidated after reparse. Graph/index retention does not implicitly pin the parse arena.
docs/arch/refactor/rev11/templates/block-charter.md:8:**Direct predecessors and accepted SHA/tree/evidence:**  
docs/arch/refactor/rev11/templates/block-charter.md:34:- final ownership/dependency/lifetime direction:
docs/arch/refactor/rev11/templates/block-charter.md:79:List every assumption whose failure requires work to stop. Include identity omissions, undeclared consumers, dependency cycles, dual-path need, unsafe lifetime, missing gate, or invalid benchmark equivalence.
docs/arch/refactor/rev11/templates/review-report.md:22:- direct caller/owner/lifetime/dependency closure:
docs/arch/refactor/rev11/charters/A2C.md:10:A2C is not executable. It has no accepted candidate and may not re-enter `READY`,
docs/arch/refactor/rev11/charters/A2C.md:11:`IN_PROGRESS`, `REVIEW`, `ACCEPTANCE_RECOMMENDED`, or `ACCEPTED`.
docs/arch/refactor/rev11/decisions/ADR-010-compositional-products-and-mapping-taxonomy.md:9:Real requests can require several products and independent materializations. A single artifact-level enum encourages hidden “full analysis.” Treating all maps as one product conflicts with IDE companions that require projection mappings and runtime outputs whose source maps are optional.
docs/arch/refactor/rev11/decisions/ADR-010-compositional-products-and-mapping-taxonomy.md:17:- a typed Vue or Svelte payload before planning;
docs/arch/refactor/rev11/decisions/ADR-010-compositional-products-and-mapping-taxonomy.md:35:- presentation/serialization changes do not invalidate unrelated semantic/code artifacts;
docs/arch/refactor/rev11/charters/BS1.md:1:# BS1 — Svelte compiler-output conformance train
docs/arch/refactor/rev11/charters/BS1.md:8:Deliver Svelte compiler correctness on the final B2–B4 substrate for exact
docs/arch/refactor/rev11/charters/BS1.md:13:- Svelte-owned semantic model and product plans;
docs/arch/refactor/rev11/charters/BS1.md:16:- Svelte-native helpers, effects, blocks, events, bindings, actions, transitions,
docs/arch/refactor/rev11/charters/BS1.md:21:- the complete accepted official-core Svelte pack.
docs/arch/refactor/rev11/charters/BS1.md:23:BS1 does not implement a Svelte runtime, use an official compiler in production, or
docs/arch/refactor/rev11/charters/BS1.md:31:no blocked official case or semantic known-divergence. Structural and dependency
docs/arch/refactor/rev11/charters/BS1.md:33:component, or SSR assumption remains anywhere in Svelte implementation paths. Every
docs/arch/refactor/rev11/charters/BS1.md:34:corresponding BF3 guard is removed.
docs/arch/refactor/rev11/charters/BS1.md:39:success, an output requiring runtime patching, or any design that bases Svelte on Vue.
docs/arch/refactor/rev11/decisions/ADR-017-stack-aware-review-and-landing.md:5:**Reopen only if:** repository hosting or review mechanics provide a stronger equivalent that preserves exact candidate identity, independently releasable layers, and atomic cutovers.
docs/arch/refactor/rev11/decisions/ADR-017-stack-aware-review-and-landing.md:9:Small dependent PRs improve reviewability, especially with agent-generated work, but lower-layer changes cascade new descendant SHAs and trees. Some architecture changes are independently landable; others are review-splittable but must reach trunk atomically. Treating all stacks alike conflicts with exact-candidate approval and one-production-path cutovers.
docs/arch/refactor/rev11/decisions/ADR-017-stack-aware-review-and-landing.md:17:- mergeable layers map to independently releasable accepted blocks or explicitly accepted subblocks;
docs/arch/refactor/rev11/decisions/ADR-017-stack-aware-review-and-landing.md:18:- private review-only layers have unique layer IDs, may repeat one atomic acceptance block or represent an explicit private-checkpoint predecessor, target a private integration branch, and never merge independently;
docs/arch/refactor/rev11/decisions/ADR-017-stack-aware-review-and-landing.md:19:- lower-layer changes trigger restack, cumulative-tree CI, candidate invalidation, and impact-bounded reattestation; reviewed candidate identity and accepted landing identity remain separate; SHA and full tree may differ after a base advance, but a validated proof must establish exact canonical candidate-delta and generated-output equivalence on recorded bases;
docs/arch/refactor/rev11/decisions/ADR-017-stack-aware-review-and-landing.md:22:- no program-wide stack and no independently merged `D1` or other private atomic foundation.
docs/arch/refactor/rev11/decisions/ADR-017-stack-aware-review-and-landing.md:33:- merging private atomic-cutover layers independently.
docs/arch/refactor/rev11/charters/A0.md:10:Produce an exact entry-checkout lock, validate the Revision 11 package and program ledger, identify the maintainer/orchestrator and delivery permissions, and disposition every architecture-affecting open change. The implementation baseline is accepted later by A6 after Gate 0 source changes.
docs/arch/refactor/rev11/charters/A0.md:34:Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A0` is accepted only when its evidence is attached to one unchanged candidate SHA/tree and evidence digest.
docs/arch/refactor/rev11/templates/stack-window.template.toml:6:# validated file. LANDABLE is one connected dependency path whose layers may
docs/arch/refactor/rev11/templates/stack-window.template.toml:7:# merge independently. ATOMIC_REVIEW has private checkpoints and one final
docs/arch/refactor/rev11/charters/BV0.md:16:matrix and independently authored controls. It expressly owns:
docs/arch/refactor/rev11/charters/BV0.md:34:For each defect: add a failing independently authored regression first; reproduce
docs/arch/refactor/rev11/charters/BV0.md:35:against the exact rc.3 oracle; identify the existing typed compiler-owner root cause;
docs/arch/refactor/rev11/charters/BV0.md:41:It must be connected to the BF2 comparator, parse/link/runtime/map oracles, and
docs/arch/refactor/rev11/charters/BV0.md:48:runtime/server, diagnostic, and mapping checks. The isolated oracle install is
docs/arch/refactor/rev11/charters/A0.md:10:Produce an exact entry-checkout lock, validate the Revision 11 package and program ledger, identify the maintainer/orchestrator and delivery permissions, and disposition every architecture-affecting open change. The implementation baseline is accepted later by A6 after Gate 0 source changes.
docs/arch/refactor/rev11/charters/A0.md:34:Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A0` is accepted only when its evidence is attached to one unchanged candidate SHA/tree and evidence digest.
docs/arch/refactor/rev11/templates/architecture-premise-ledger.template.md:27:- [ ] affected blocks and invalidation behavior are explicit
docs/arch/refactor/rev11/templates/architecture-premise-ledger.template.md:28:- [ ] independent architecture challenger reviewed the ledger
docs/arch/refactor/rev11/templates/implementation-lock-record.md:56:- crate/module dependency graph artifact:
docs/arch/refactor/rev11/templates/implementation-lock-record.md:66:- accepted `performance-gates.toml` digest:
docs/arch/refactor/rev11/templates/implementation-lock-record.md:89:- stack implementation (`github-native`, `gh-stack`, ordinary dependent PRs, or none):
docs/arch/refactor/rev11/templates/implementation-lock-record.md:91:- restack/range-diff/CI/review invalidation rule:
docs/arch/refactor/rev11/templates/implementation-lock-record.md:107:Every item here must be a private implementation choice that cannot change semantics, identity, lifetime, cache validity, mapping interpretation, compatibility, dependency direction, or pass/fail gates.
docs/arch/refactor/rev11/templates/implementation-lock-record.md:117:- [ ] identity/profile/compatibility decisions accepted
docs/arch/refactor/rev11/templates/implementation-lock-record.md:121:- [ ] maintainer/orchestrator identities and program-state/evidence custody accepted
docs/arch/refactor/rev11/templates/implementation-lock-record.md:122:- [ ] worktree/branch/CI/merge/stack/restack policy accepted
docs/arch/refactor/rev11/templates/implementation-lock-record.md:123:- [ ] first foundational charters/context packets/stack placement accepted
docs/arch/refactor/rev11/templates/implementation-lock-record.md:125:- [ ] exact SHA/tree architecture and adversarial evidence accepted
docs/arch/refactor/rev11/templates/implementation-lock-record.md:126:- [ ] no agent may self-accept, weaken gates, or merge private atomic layers independently
docs/arch/refactor/rev11/templates/implementation-lock-record.md:129:The lock record is an immutable evidence artifact addressed by `Record digest`. It may be stored outside the implementation commit to avoid self-reference. If later committed into the repository, that documentation commit does not replace the recorded implementation baseline unless a new A6 lock is accepted.
docs/arch/refactor/rev11/templates/pr-description.md:10:- direct predecessors and accepted SHAs/trees:
docs/arch/refactor/rev11/templates/pr-description.md:76:- eventual validated landing-equivalence artifact/digest (reviewed delta = accepted delta on recorded bases):
docs/arch/refactor/rev11/charters/J1.template.md:16:- Vue/Svelte transform and `CodeTransform` mapping routes;
docs/arch/refactor/rev11/templates/context-packet.md:21:- relevant predecessor accepted SHAs/trees/evidence digests:
docs/arch/refactor/rev11/templates/context-packet.md:34:Do not substitute the implementor/orchestrator summary for direct source inspection when the role requires independent evidence.
docs/arch/refactor/rev11/templates/context-packet.md:39:- dependency/lockfile/protocol changes allowed:
docs/arch/refactor/rev11/templates/context-packet.md:64:Include correctness, determinism, work/copy/allocation, performance, memory, platform, failure, dependency, generated-file, and clean-tree proof applicable to the charter.
docs/arch/refactor/rev11/templates/context-packet.md:69:- required dependency/owner closure:
docs/arch/refactor/rev11/decisions/ADR-001-semantic-authority-and-derived-projections.md:8:`FunctionFlowGraph` must be the one structural flow authority, but efficient solving may need dominators, loop forests, SCCs, def-use overlays, capture summaries, reverse adjacency, or compact execution schedules. Forbidding every graph-derived structure would either bloat the canonical graph or repeat topology work. Allowing independent query-specific CFGs recreates the current dual-authority defect.
docs/arch/refactor/rev11/decisions/ADR-001-semantic-authority-and-derived-projections.md:15:- is deterministic and fully reproducible from the graph and accepted kernel rules;
docs/arch/refactor/rev11/decisions/ADR-001-semantic-authority-and-derived-projections.md:17:- is request-local by default and independently weighted/evicted if retention is proven valuable;
docs/arch/refactor/rev11/decisions/ADR-001-semantic-authority-and-derived-projections.md:27:- independent syntax-shaped/query-specific CFGs;
docs/arch/refactor/rev11/templates/performance-gates.template.toml:67:output_oracle = "REQUIRED_ORACLE"
docs/arch/refactor/rev11/charters/B4.md:12:exactly as accepted; displaced owners are deleted in the owning cutover.
docs/arch/refactor/rev11/charters/B4.md:14:B4 does not implement Vue/Svelte semantics, helper choices, runtime topology, or
docs/arch/refactor/rev11/charters/B2.md:3:**Status:** PROPOSED amendment / LOCKED. **Predecessor:** BF3.
docs/arch/refactor/rev11/charters/B2.md:5:B2 owns the separate Vue and Svelte syntax frontends: parsing, recovery, syntax
docs/arch/refactor/rev11/charters/C3.md:6:the closed typed project-information demands accepted in BV1 using the canonical
docs/arch/refactor/rev11/charters/C3.md:55:carries the sorted unique dependency identities and the exact semantic profile/input
docs/arch/refactor/rev11/charters/C3.md:80:TSC/TSX, declaration, and imported-macro cell by running accepted BV1 codegen with C3;
docs/arch/refactor/rev11/decisions/ADR-019-reproducible-authority-package.md:17:- generated artifacts are never edited independently;
docs/arch/refactor/rev11/charters/BF3.md:1:# BF3 — Known-wrong successful-cell safety retraction
docs/arch/refactor/rev11/charters/BF3.md:8:Before B2/B3, remove every in-scope Svelte or non-Vue-runtime reachable success claim
docs/arch/refactor/rev11/charters/BF3.md:20:6. Add an independently authored discriminating regression.
docs/arch/refactor/rev11/charters/BF3.md:23:Vue VDOM/Vapor/SSR runtime-render rows are excluded from BF3 retraction and tracking
docs/arch/refactor/rev11/charters/BF3.md:24:and are assigned to BV0 correction. BF3 retains this procedure for in-scope Svelte
docs/arch/refactor/rev11/charters/BF3.md:27:BF3 owns no broad parser, semantic model, lowering, helper, hydration, SSR, mapping,
docs/arch/refactor/rev11/charters/BF3.md:31:BF3 must probe BF2's exact `svelte@5.56.8` client cells. Results against
docs/arch/refactor/rev11/charters/BF3.md:32:`svelte@5.56.3` do not satisfy that exit. Svelte server's existing typed
docs/arch/refactor/rev11/charters/BF3.md:33:`ServerGenerate` refusal is already a non-successful cell and receives no new BF3
docs/arch/refactor/rev11/charters/BF3.md:39:inventory within BF3's retained scope is exhausted, including the exact Svelte client
docs/arch/refactor/rev11/charters/BF3.md:42:Cold-path tests prove unaffected cells retain behavior. BF3 cannot accept before that
docs/arch/refactor/rev11/charters/BF3.md:43:exhaustion, and no B2/B3 dispatch begins before both BF3 and BV0 are accepted.
docs/arch/refactor/rev11/charters/B3.md:3:**Status:** PROPOSED amendment / LOCKED. **Predecessor:** BF3.
docs/arch/refactor/rev11/charters/B3.md:7:VDOM/Vapor; Svelte runes/legacy where applicable; normalized framework-local options;
docs/arch/refactor/rev11/charters/A6.md:26:Maintainer-accepted immutable lock record; exact entry and implementation SHA/tree; package/DAG/program-state digests; locked performance-gates.toml; resolved orchestration/permission/worktree/CI/merge/stack policy; first block charters/context packets/stack placement; exact independent review state.
docs/arch/refactor/rev11/charters/A6.md:34:Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A6` is accepted only when its evidence is attached to one unchanged candidate SHA/tree and evidence digest.
docs/arch/refactor/rev11/charters/B4.md:12:exactly as accepted; displaced owners are deleted in the owning cutover.
docs/arch/refactor/rev11/charters/B4.md:14:B4 does not implement Vue/Svelte semantics, helper choices, runtime topology, or
docs/arch/refactor/rev11/decisions/ADR-018-opus-adapter-and-orchestrator-state.md:1:# ADR-018 — Agent Orchestration Uses One Model-Independent Program Ledger and a Pinned Opus Adapter
docs/arch/refactor/rev11/decisions/ADR-018-opus-adapter-and-orchestrator-state.md:13:- `contracts/agent-orchestration.md` is the model-independent execution contract.
docs/arch/refactor/rev11/decisions/ADR-018-opus-adapter-and-orchestrator-state.md:18:- Optional subagents are used only for substantial independent work or required review independence and cannot self-accept.
docs/arch/refactor/rev11/decisions/ADR-008-deterministic-artifacts-and-persistence.md:7:Concurrency, cache warmth, native/WASM execution, and persistence can produce schedule-dependent or stale artifacts unless determinism and hermetic identity are explicit.
docs/arch/refactor/rev11/decisions/ADR-008-deterministic-artifacts-and-persistence.md:11:Equal authoritative observations and result-affecting contracts produce equal Verter-owned observable outputs independent of legal schedule, worker assignment, cache warmth, or supported portable execution profile.
docs/arch/refactor/rev11/decisions/ADR-008-deterministic-artifacts-and-persistence.md:13:Persistent eligibility is limited to complete deterministic hermetic serializable values with complete compatibility, positive/negative fact, integrity, and size basis. OXC arenas, snapshot/session handles, transient cohorts, partial outcomes, and ambient-state-dependent values are never persisted.
docs/arch/refactor/rev11/decisions/ADR-008-deterministic-artifacts-and-persistence.md:19:Persistence is optional acceleration, never correctness authority. Schedule-dependent map, diagnostic, ID, or serialization order is a defect.
docs/arch/refactor/rev11/charters/BV1.md:23:- the complete accepted official-core Vue pack.
docs/arch/refactor/rev11/charters/BV1.md:29:`FC-TS-001-LOCAL` is BV1's independently closable partition of `FC-TS-001`. It
docs/arch/refactor/rev11/charters/BV1.md:36:C3's later end-to-end partition; it combines accepted BV1 codegen with the real
docs/arch/refactor/rev11/charters/BV1.md:43:BV0 implementation only with an accepted equivalent correction; it may not
docs/arch/refactor/rev11/charters/BV1.md:54:corresponding BF3 guard. Vue RC maturity is not Stable.
docs/arch/refactor/rev11/charters/BV1.md:60:or pressure to share a universal semantic/runtime IR with Svelte.
docs/arch/refactor/rev11/charters/BV1.md:23:- the complete accepted official-core Vue pack.
docs/arch/refactor/rev11/charters/BV1.md:29:`FC-TS-001-LOCAL` is BV1's independently closable partition of `FC-TS-001`. It
docs/arch/refactor/rev11/charters/BV1.md:36:C3's later end-to-end partition; it combines accepted BV1 codegen with the real
docs/arch/refactor/rev11/charters/BV1.md:43:BV0 implementation only with an accepted equivalent correction; it may not
docs/arch/refactor/rev11/charters/BV1.md:54:corresponding BF3 guard. Vue RC maturity is not Stable.
docs/arch/refactor/rev11/charters/BV1.md:60:or pressure to share a universal semantic/runtime IR with Svelte.
docs/arch/refactor/rev11/charters/B1.md:6:**Predecessor:** `A6` (satisfied — the Implementation Lock Record is accepted).
docs/arch/refactor/rev11/charters/B1.md:7:**Implementation baseline:** the SHA/tree recorded in the accepted Implementation Lock Record §1.
docs/arch/refactor/rev11/charters/B1.md:15:Land dependency-neutral types and canonical encodings needed by later blocks without creating
docs/arch/refactor/rev11/charters/B1.md:63:The strategy is **locked** by [`evidence/A5/dependency-direction.md`](../evidence/A5/dependency-direction.md).
docs/arch/refactor/rev11/charters/B1.md:67:  workspace graph**, modelled on `crates/verter_macro_dto/tests/cases/dependency_closure_guard.rs`.
docs/arch/refactor/rev11/charters/B1.md:81:- Supersession, in the same accepted candidate: delete `verter_audit_no_upward_deps` and both tests
docs/arch/refactor/rev11/charters/B1.md:83:  `audit_substrate_isolation` is **not** fully implied; its dependency half is, its *naming* half is
docs/arch/refactor/rev11/charters/B1.md:160:work-counter, zero-work, or output-oracle failure.
docs/arch/refactor/rev11/charters/B1.md:169:B1 may **add** a dedicated micro cell only through a new lock record digest and the same independent
docs/arch/refactor/rev11/charters/B1.md:175:Foundational: three mandates, three independent contexts, one candidate SHA **and** tree
docs/arch/refactor/rev11/charters/B1.md:182:| adversarial performance/memory | diff plus raw command and benchmark output; specifically whether the dependency test's discrimination proof shows the planted edge was actually applied, and whether the gate cell's counters were produced by a run that did non-zero work |
docs/arch/refactor/rev11/charters/B1.md:189:Stop for an undiscovered public/wire consumer, an incompatible persisted domain, a dependency cycle,
docs/arch/refactor/rev11/charters/B5.md:5:B5 exposes the already accepted BV1 and BS1 algorithms through one borrowed one-shot
docs/arch/refactor/rev11/charters/B5.md:11:proves Vue and Svelte accepted packs remain unchanged through the direct route and no
docs/arch/refactor/rev11/decisions/ADR-015-binding-dependency-direction.md:17:-> shared syntax frontends and dependency-neutral DTOs
docs/arch/refactor/rev11/decisions/ADR-015-binding-dependency-direction.md:32:- durable build tests reject crate dependency cycles and forbidden edges.
docs/arch/refactor/rev11/decisions/ADR-015-binding-dependency-direction.md:34:Logical owners do not automatically require crates; use modules/functions until a real dependency firewall or multi-consumer stable contract exists.
docs/arch/refactor/rev11/charters/A1.md:34:Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A1` is accepted only when its evidence is attached to one unchanged candidate/evidence SHA.
docs/arch/refactor/rev11/charters/A1.md:34:Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A1` is accepted only when its evidence is attached to one unchanged candidate/evidence SHA.
docs/arch/refactor/rev11/verification.md:4:**Rule:** a claim is accepted only when the exact candidate, exact result contract, and exact input/profile basis produced reproducible evidence.  
docs/arch/refactor/rev11/verification.md:18:8. architecture/dependency/deletion/complexity proof;
docs/arch/refactor/rev11/verification.md:30:- Node/runtime/package manager/lockfile, TypeScript/native-TypeScript/provider versions, Vue/Svelte versions, NAPI/WASM runtime, and platform;
docs/arch/refactor/rev11/verification.md:43:- oracle/provider provenance where relevant.
docs/arch/refactor/rev11/verification.md:57:- semantic kernel may depend on syntax/index/contracts and dependency-neutral observations, not framework codegen or managed engine;
docs/arch/refactor/rev11/verification.md:58:- direct compiler has no dependency on `Engine`, InputStore, QueryRuntime, ProviderHub, durable stores, or LSP;
docs/arch/refactor/rev11/verification.md:61:- no dependency cycle is hidden through feature flags, build scripts, generated code, or protocol conversion crates.
docs/arch/refactor/rev11/verification.md:63:A machine-readable dependency snapshot is compared before/after every foundational block.
docs/arch/refactor/rev11/verification.md:86:- dependency graph and reachable production path count;
docs/arch/refactor/rev11/verification.md:89:Text search is supporting evidence only; type/dependency/behavior proof is authoritative.
docs/arch/refactor/rev11/verification.md:95:Every supported product contract is tested independently and in valid combinations:
docs/arch/refactor/rev11/verification.md:100:- Svelte runtime/IDE/public products currently claimed;
docs/arch/refactor/rev11/verification.md:116:- dependency and profile basis;
docs/arch/refactor/rev11/verification.md:122:For products declared independent, requesting `{A,B}` must equal requesting `A` and `B` separately after canonical product ordering, except for intentional shared materialization that changes no result.
docs/arch/refactor/rev11/verification.md:130:- force Vue projection on Svelte;
docs/arch/refactor/rev11/verification.md:134:- invalidate an unchanged semantic/code subplan because only one product's terminal presentation or serialization changed.
docs/arch/refactor/rev11/verification.md:157:- multiple independent misses returned in one currently knowable wave;
docs/arch/refactor/rev11/verification.md:192:- substitutions/generics and invalidation;
docs/arch/refactor/rev11/verification.md:267:- old syntax-shaped control types are absent from production code and dependency graph;
docs/arch/refactor/rev11/verification.md:269:- derived projections cannot construct a complete result or semantic transfer independently;
docs/arch/refactor/rev11/verification.md:300:- stable entity IDs, dependency fingerprints, exactness, provenance, and canonical serialization;
docs/arch/refactor/rev11/verification.md:320:- randomized independent batch/chunk order;
docs/arch/refactor/rev11/verification.md:325:A smaller count requires a stronger systematic interleaving proof accepted in the block charter.
docs/arch/refactor/rev11/verification.md:379:An amplification above `1.0` is accepted only when:
docs/arch/refactor/rev11/verification.md:417:Vue/Svelte compile with zero native projection demand
docs/arch/refactor/rev11/verification.md:423:Svelte runtime compile under current capability
docs/arch/refactor/rev11/verification.md:483:- an independent performance reviewer accepts the change;
docs/arch/refactor/rev11/verification.md:509:- at least 10 independent long-cell runs when practical;
docs/arch/refactor/rev11/verification.md:525:- Vue/Svelte and each claimed runtime/IDE/public product;
docs/arch/refactor/rev11/verification.md:537:- dependency/config/project edit;
docs/arch/refactor/rev11/verification.md:538:- create/delete/rename/reopen;
docs/arch/refactor/rev11/verification.md:556:- parse/index/format/Vue/Svelte plans;
docs/arch/refactor/rev11/verification.md:594:The `A6` lock sets exact aggregate and strategic-cell tolerances before candidate implementation. The target is to meet or beat the fastest valid equivalent-work Rust implementation on the primary direct-suite aggregate and to avoid material Pareto domination in strategic cells. A candidate miss is blocking and cannot be waived after results are known. If indispensable extra work or a comparison mismatch proves the locked product/equivalence premise false, the project must amend the product/architecture and Implementation Lock Record under the blind recalibration rule, invalidate affected candidate evidence, and restart. Repeated work, weaker semantics, invalid comparison, or unbounded retention are never acceptable premises.
docs/arch/refactor/rev11/verification.md:614:- independently evictable entries do not share a lifetime-pinning arena;
docs/arch/refactor/rev11/verification.md:637:- create/delete/rename/move/reopen and same-content new incarnation;
docs/arch/refactor/rev11/verification.md:639:- dependency/config/library changes;
docs/arch/refactor/rev11/verification.md:647:`L1` minimum durations/work counts are fixed by the `A6` Implementation Lock Record and the accepted `L1` charter before the soak candidate runs.
docs/arch/refactor/rev11/verification.md:684:- tiny dependent work stays inline;
docs/arch/refactor/rev11/verification.md:714:Test script/template/style-only edits, unit move, dependency change, project/config change, external template/style, fallback, pressure eviction, and repeated edit sequences.
docs/arch/refactor/rev11/verification.md:725:- dependency input change;
docs/arch/refactor/rev11/verification.md:726:- close/reopen same content with new incarnation;
docs/arch/refactor/rev11/verification.md:742:- Vue `v-bind`, modules, scoping/keyframes, and Svelte style consumers reuse syntax where bytes/profile match;
docs/arch/refactor/rev11/verification.md:756:- huge LoadSet and dependency cycles;
docs/arch/refactor/rev11/verification.md:785:- old persisted/public payload behavior matches accepted migration/rejection policy;
docs/arch/refactor/rev11/verification.md:787:- precomputed facts validate root/batch/profile/kernel/input/dependency/exactness/integrity/size basis;
docs/arch/refactor/rev11/verification.md:810:A net concept increase must correspond to an explicit accepted capability/invariant that the prior model could not represent. Moving complexity behind new names is a failure.
docs/arch/refactor/rev11/verification.md:814:Every accepted block proves its delivery process as well as its code:
docs/arch/refactor/rev11/verification.md:818:- only blocks with accepted predecessors become active, except contingent `READY`/`IN_PROGRESS`/`REVIEW` upper layers whose unaccepted predecessors are lower layers in the same validated immutable stack snapshot; no such upper layer reaches acceptance recommendation before predecessor landing/restack, and before `A6` no post-Gate-0 block is active;
docs/arch/refactor/rev11/verification.md:821:- shared generated files, lockfiles, protocols, and dependency-firewall files had one writer lease;
docs/arch/refactor/rev11/verification.md:822:- the orchestrator did not count its own implementation/synthesis as independent review or maintainer acceptance;
docs/arch/refactor/rev11/verification.md:823:- the stack window is bounded, maps every layer to a block/charter, and contains no hidden unaccepted cross-stack dependency;
docs/arch/refactor/rev11/verification.md:824:- every mergeable layer is independently releasable and passes required checks on its cumulative tree;
docs/arch/refactor/rev11/verification.md:825:- private atomic layers remain draft/non-mergeable and never reach trunk independently;
docs/arch/refactor/rev11/verification.md:830:- the actual accepted commit/tree is bound to the reviewed candidate through exact canonical candidate-delta equality on recorded bases, matching generated-output digests, and required post-landing checks; full-tree equality is not assumed after a base advance;
docs/arch/refactor/rev11/verification.md:831:- program state records actual accepted commit/tree, the landing-equivalence digest, and invalidates/restacks remaining dependent work.
docs/arch/refactor/rev11/verification.md:841:| Authority | Revision 11 digest, accepted ADRs, no contradiction or unresolved public/identity/lifetime gate |
docs/arch/refactor/rev11/verification.md:843:| Dependency | forbidden edges/cycles absent; direct core independent of managed engine |
docs/arch/refactor/rev11/verification.md:853:| Frameworks | typed Vue/Svelte boundaries, no final Any bag, synthetic alternate-shape fixture |
docs/arch/refactor/rev11/verification.md:860:| Complexity | negative-net architecture or accepted capability rationale; old paths and campaign machinery gone |
docs/arch/refactor/rev11/verification.md:861:| Delivery | validated program state, bounded stack windows, worktree isolation, restack reattestation, atomic private layers, reviewed-to-accepted candidate-delta equivalence |
docs/arch/refactor/rev11/charters/B5.md:5:B5 exposes the already accepted BV1 and BS1 algorithms through one borrowed one-shot
docs/arch/refactor/rev11/charters/B5.md:11:proves Vue and Svelte accepted packs remain unchanged through the direct route and no
docs/arch/refactor/rev11/charters/A2.md:26:Exact expectations; negative controls; crossed matrices; oracle/profile stamps.
docs/arch/refactor/rev11/charters/A2.md:34:Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A2` is accepted only when its evidence is attached to one unchanged candidate/evidence SHA.
docs/arch/refactor/rev11/charters/A3.md:21:- the 154-row accepted-A2 checker-correct clean/warm preservation cohort;
docs/arch/refactor/rev11/charters/A3.md:79:according to `governance.md`. A3 is accepted only when its evidence is attached to one
docs/arch/refactor/rev11/charters/B1.md:6:**Predecessor:** `A6` (satisfied — the Implementation Lock Record is accepted).
docs/arch/refactor/rev11/charters/B1.md:7:**Implementation baseline:** the SHA/tree recorded in the accepted Implementation Lock Record §1.
docs/arch/refactor/rev11/charters/B1.md:15:Land dependency-neutral types and canonical encodings needed by later blocks without creating
docs/arch/refactor/rev11/charters/B1.md:63:The strategy is **locked** by [`evidence/A5/dependency-direction.md`](../evidence/A5/dependency-direction.md).
docs/arch/refactor/rev11/charters/B1.md:67:  workspace graph**, modelled on `crates/verter_macro_dto/tests/cases/dependency_closure_guard.rs`.
docs/arch/refactor/rev11/charters/B1.md:81:- Supersession, in the same accepted candidate: delete `verter_audit_no_upward_deps` and both tests
docs/arch/refactor/rev11/charters/B1.md:83:  `audit_substrate_isolation` is **not** fully implied; its dependency half is, its *naming* half is
docs/arch/refactor/rev11/charters/B1.md:160:work-counter, zero-work, or output-oracle failure.
docs/arch/refactor/rev11/charters/B1.md:169:B1 may **add** a dedicated micro cell only through a new lock record digest and the same independent
docs/arch/refactor/rev11/charters/B1.md:175:Foundational: three mandates, three independent contexts, one candidate SHA **and** tree
docs/arch/refactor/rev11/charters/B1.md:182:| adversarial performance/memory | diff plus raw command and benchmark output; specifically whether the dependency test's discrimination proof shows the planted edge was actually applied, and whether the gate cell's counters were produced by a run that did non-zero work |
docs/arch/refactor/rev11/charters/B1.md:189:Stop for an undiscovered public/wire consumer, an incompatible persisted domain, a dependency cycle,
docs/arch/refactor/rev11/charters/J1.template.md:16:- Vue/Svelte transform and `CodeTransform` mapping routes;
docs/arch/refactor/rev11/charters/A3.md:21:- the 154-row accepted-A2 checker-correct clean/warm preservation cohort;
docs/arch/refactor/rev11/charters/A3.md:79:according to `governance.md`. A3 is accepted only when its evidence is attached to one
docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md:18:- identity/profile/compatibility/protocol/dependency decisions;
docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md:23:Before A6, only adoption, command/capability proof, harness strengthening, wrong-complete safety retraction, measurement-only attribution, inventory, and gate/capability record work is allowed. Any Gate 0 source change invalidates and refreshes affected downstream evidence before A6. Non-safety foundational cutovers remain locked.
docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md:25:Gate recalibration is allowed only before the affected candidate is measured, through an exact new record digest and the same independent review class. Weakening a gate after seeing candidate results is prohibited.
docs/arch/refactor/rev11/charters/A5.md:26:Resolved current-owner rows; option classification; domains; consumer/protocol map; dependency test plan; evidence custody; program-state workflow; worktree/branch/CI/merge rules; bounded stack-window and review-context policy.
docs/arch/refactor/rev11/charters/A5.md:34:Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A5` is accepted only when its evidence is attached to one unchanged candidate SHA/tree and evidence digest.
docs/arch/refactor/rev11/charters/B6.md:5:B6 exposes B5's accepted algorithms through prepared-first, prepared-repeat, and
docs/arch/refactor/rev11/charters/B6.md:12:route-overhead cells. C4 remains locked until B6 is accepted because these routes do
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:32:## R9-B2 — The dependency graph was not executable
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:90:**Revision 11 correction:** snapshot-independent `QueryIdentity<Q>` discovers bounded candidates, which are then positive/negative-fact validated. `SemanticFlightKey<Q>` adds the exact `InputBasisId` for default in-flight production.
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:98:## R9-B13 — Binding dependency direction was not locked
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:102:**Revision 11 correction:** identity/contracts and syntax point inward; semantic kernel is dependency-neutral; compiler depends on the sealed semantic facade; managed/session and adapters depend inward only. Build tests reject forbidden edges and cycles.
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:122:After `A6`, a block becomes implementable only when its predecessor set, charter, current-tree closure, tests, numeric gates, deletion set, and independent reviews are complete. This prevents “following the architecture” while inventing unresolved behavior in code.
docs/arch/refactor/rev11/charters/A2C.md:10:A2C is not executable. It has no accepted candidate and may not re-enter `READY`,
docs/arch/refactor/rev11/charters/A2C.md:11:`IN_PROGRESS`, `REVIEW`, `ACCEPTANCE_RECOMMENDED`, or `ACCEPTED`.
docs/arch/refactor/rev11/charters/B2.md:3:**Status:** PROPOSED amendment / LOCKED. **Predecessor:** BF3.
docs/arch/refactor/rev11/charters/B2.md:5:B2 owns the separate Vue and Svelte syntax frontends: parsing, recovery, syntax
docs/arch/refactor/rev11/decisions/ADR-007-direct-core-before-managed-runtime.md:11:The critical dependency direction is:
docs/arch/refactor/rev11/governance.md:6:**Sequencing authority:** no block may begin before every direct predecessor in `program-dag.toml` is accepted, except contingent `READY`/`IN_PROGRESS`/`REVIEW` work on an upper layer whose unaccepted predecessor is below it in the same validated immutable stack snapshot. Such work cannot be acceptance-recommended or accepted until the predecessor lands and the upper candidate is restacked/revalidated. Document order, available implementor capacity, or a convenient branch is not permission.
docs/arch/refactor/rev11/governance.md:29:- authority/contract/accepted-ADR consistency checks;
docs/arch/refactor/rev11/governance.md:35:The orchestrator may implement when assigned, but cannot count its own synthesis as independent review, cannot self-accept a block, and cannot silently amend architecture or gates.
docs/arch/refactor/rev11/governance.md:39:For Foundational work, independently challenges the proposed target before implementation using product requirements, repository facts, measurements, compatibility obligations, and blast radius. It should inspect before reading the implementor's preferred rationale where practical.
docs/arch/refactor/rev11/governance.md:54:Executes the ratified charter. It may choose private representation details but may not reopen locked ownership, add a second production path, widen scope silently, or weaken proof. A materially false charter assumption produces a deviation memo and stop.
docs/arch/refactor/rev11/governance.md:61:2. **Architecture:** authority, identity, ownership, dependency direction, lifetime, platform, compatibility, public boundary, determinism, and conceptual complexity.
docs/arch/refactor/rev11/governance.md:72:Examples: obsolete comment/guard/doc cleanup, mechanical re-export/dependency removal, isolated internal rename with no runtime/semantic/lifetime consequence.
docs/arch/refactor/rev11/governance.md:75:- one independent bounded review;
docs/arch/refactor/rev11/governance.md:82:- independent scoper;
docs/arch/refactor/rev11/governance.md:99:- incremental invalidation/publication;
docs/arch/refactor/rev11/governance.md:106:Requires independent scoping, pre-implementation architecture challenge, and all three review mandates on one exact candidate SHA/tree.
docs/arch/refactor/rev11/governance.md:119:Direct predecessors and accepted SHAs:
docs/arch/refactor/rev11/governance.md:134:Final ownership/dependency/lifetime direction:
docs/arch/refactor/rev11/governance.md:150:The charter states what must become true and what may change. It does not dictate every private implementation detail. It may not remove a predecessor edge, loosen an A6 gate, create a second production authority, or reinterpret an accepted ADR.
docs/arch/refactor/rev11/governance.md:158:- every direct predecessor from `program-dag.toml` is accepted on the intended integration lineage, or contingent upper-layer work is bound to an exact lower predecessor in the same validated stack snapshot;
docs/arch/refactor/rev11/governance.md:165:An edge may be added when reality reveals a dependency. Removing an edge requires an authority amendment, a clean architecture challenge, and re-ratification of every already-open affected charter.
docs/arch/refactor/rev11/governance.md:175:5. invalidation and rerun of affected candidate evidence.
docs/arch/refactor/rev11/governance.md:181:`program-state.toml` is the durable execution ledger. Conversation history, a project board, or PR labels may mirror it but cannot replace it. The orchestrator is its sole writer; the maintainer accepts state transitions that require authority. `tools/validate_program_state.py` must pass after every transition and before a block starts, enters review, is recommended for acceptance, or is accepted.
docs/arch/refactor/rev11/governance.md:183:Every worker receives one immutable context packet and one writable worktree/branch. No two workers share a mutable checkout. Shared generated files, lockfiles, protocol schemas, dependency firewalls, and central manifests have one active writer lease.
docs/arch/refactor/rev11/governance.md:189:- every mergeable layer is independently safe and proven on its cumulative tree;
docs/arch/refactor/rev11/governance.md:191:- lower-layer changes cascade restack, CI, state, and review invalidation;
docs/arch/refactor/rev11/governance.md:247:Reviewers record findings before editing code. The implementor normally applies bounded fixes. A reviewer acting as fixer does not count its own patch as independent re-approval.
docs/arch/refactor/rev11/governance.md:251:Approval is attached to one exact candidate SHA **and tree**, its exact base tree, and the evidence digest. Any code/test/generated-output change or restack creates a new candidate and invalidates landing approval until impact-bounded reattestation binds to the new identity.
docs/arch/refactor/rev11/governance.md:266:independent scope
docs/arch/refactor/rev11/governance.md:283:The **reviewed candidate identity** and the **accepted landing identity** are separate records. A merge commit, merge-queue commit, squash, or one-commit fast-forward may produce a different accepted commit SHA and full tree because the landing base advanced or the reviewed branch was reconstructed before final freeze. That is legal only when a repository-validated landing-equivalence artifact proves exact canonical candidate-delta equality from reviewed base to reviewed candidate versus accepted base to accepted commit, generated-output digests match, no manual conflict resolution occurred after review, and all required post-landing checks pass. A single program block is not co-batched with unrelated changes in the same landing delta. The accepted identity never replaces or retroactively changes the SHA/tree reviewers actually inspected.
docs/arch/refactor/rev11/governance.md:317:An accepted cutover must prove:
docs/arch/refactor/rev11/governance.md:319:- all declared predecessors were accepted before implementation began, or any contingent stacked work remained at draft/review state and was restacked/revalidated after predecessor landing before acceptance;
docs/arch/refactor/rev11/governance.md:323:- old declaration/implementation/cache/task/metric/flag/dependency/re-export/wire field/fixture removed or explicitly retained under a real compatibility obligation;
docs/arch/refactor/rev11/governance.md:332:Intermediate branch commits may be broken. Private replacement machinery may exist on the implementation branch and be exercised by hermetic tests. It must remain unreachable from production entry points until the atomic cutover, and it is deleted or becomes the sole path in the accepted merge. The accepted merge is not releasable until the cutover is complete.
docs/arch/refactor/rev11/governance.md:334:`D1` is the sole exception to the ordinary predecessor-integration rule: it is a reviewed private checkpoint on the bounded `D2` branch and cannot land independently. The `D2` sole-flow-solver cutover is indivisible: routing all public consumers and deleting the old evaluator occur in one accepted candidate. Later semantic coverage is added only inside the surviving solver.
docs/arch/refactor/rev11/governance.md:338:Independent evidence requires:
docs/arch/refactor/rev11/governance.md:349:Multiple automated/model instances with identical prompt/context and no independent inspection are not automatically independent. A strong human or automated reviewer may satisfy a mandate when the evidence discipline is met.
docs/arch/refactor/rev11/governance.md:378:Only then is the candidate accepted.
docs/arch/refactor/rev11/agents/opus-bootstrap.md:11:Extract and validate the attached split package, then begin at `ORCHESTRATOR.md`. Execute **only block A0** in this run. Do not implement later architecture blocks, widen scope, alter accepted architecture, choose post-result gates, or create a program-wide PR stack.
docs/arch/refactor/rev11/agents/opus-bootstrap.md:17:Use subagents only for genuinely independent substantial work or a required distinct review mandate. Do not spawn agents merely to summarize or repeat your own conclusion. Keep active delegation bounded to the package default, give every writer one immutable context packet and one worktree/branch, and never allow two agents to overwrite the same mutable surface.
docs/arch/refactor/rev11/decisions/ADR-011-staged-compile-attempt-and-input-loading.md:22:- `CompileFactsBatch` binds to the exact `ProjectionPlanToken`, semantic profile, dependency/input basis, projection schema, and demand digest, so terminal-only sibling plans may reuse facts only when that projection token is exactly equal;
docs/arch/refactor/rev11/charters/BF3.md:1:# BF3 — Known-wrong successful-cell safety retraction
docs/arch/refactor/rev11/charters/BF3.md:8:Before B2/B3, remove every in-scope Svelte or non-Vue-runtime reachable success claim
docs/arch/refactor/rev11/charters/BF3.md:20:6. Add an independently authored discriminating regression.
docs/arch/refactor/rev11/charters/BF3.md:23:Vue VDOM/Vapor/SSR runtime-render rows are excluded from BF3 retraction and tracking
docs/arch/refactor/rev11/charters/BF3.md:24:and are assigned to BV0 correction. BF3 retains this procedure for in-scope Svelte
docs/arch/refactor/rev11/charters/BF3.md:27:BF3 owns no broad parser, semantic model, lowering, helper, hydration, SSR, mapping,
docs/arch/refactor/rev11/charters/BF3.md:31:BF3 must probe BF2's exact `svelte@5.56.8` client cells. Results against
docs/arch/refactor/rev11/charters/BF3.md:32:`svelte@5.56.3` do not satisfy that exit. Svelte server's existing typed
docs/arch/refactor/rev11/charters/BF3.md:33:`ServerGenerate` refusal is already a non-successful cell and receives no new BF3
docs/arch/refactor/rev11/charters/BF3.md:39:inventory within BF3's retained scope is exhausted, including the exact Svelte client
docs/arch/refactor/rev11/charters/BF3.md:42:Cold-path tests prove unaffected cells retain behavior. BF3 cannot accept before that
docs/arch/refactor/rev11/charters/BF3.md:43:exhaustion, and no B2/B3 dispatch begins before both BF3 and BV0 are accepted.
docs/arch/refactor/rev11/decisions/ADR-002-compatibility-domains.md:15:- disposable private caches may be invalidated by a new namespace/build fingerprint;
docs/arch/refactor/rev11/decisions/ADR-002-compatibility-domains.md:17:- duplicate counters that must “stay equal” are collapsed or separated into genuinely independent domains;
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:58:| `decisions/ADR-015-binding-dependency-direction.md` | 45 |
docs/arch/refactor/rev11/PROVENANCE.md:108:no-committed-Python dependency policy.
docs/arch/refactor/rev11/baseline/9af553dd.md:19:Revision 8 explicitly froze `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0` as its evidence baseline. This document records observations for that exact source identity. It does **not** assert that a current local checkout or current public `main` still points to it. `A0` must freeze and reconcile the implementation checkout independently.
docs/arch/refactor/rev11/baseline/9af553dd.md:31:- experimental pinned Svelte native compilation.
docs/arch/refactor/rev11/baseline/9af553dd.md:36:- FFI and WASM ownership/copy behavior need independent proof;
docs/arch/refactor/rev11/baseline/9af553dd.md:37:- Svelte has architectural citizenship but current maturity is not equal to mature Vue claims;
docs/arch/refactor/rev11/baseline/9af553dd.md:199:- embedded-language workflows such as Vue and Svelte likely need TypeScript 6 routes until suitable support exists.
docs/arch/refactor/rev11/baseline/9af553dd.md:203:- native TypeScript and tsserver/TypeScript 6 routes are both legitimate capability-dependent provider choices;
docs/arch/refactor/rev11/baseline/9af553dd.md:236:The Revision 9 split package was architecturally strong but not a closed implementation specification. Source-independent document review found:
docs/arch/refactor/rev11/baseline/9af553dd.md:238:- accepted authority referred to ADRs still marked Proposed;
docs/arch/refactor/rev11/baseline/9af553dd.md:248:- missing binding dependency direction;
docs/arch/refactor/rev11/baseline/9af553dd.md:251:Revision 11 addresses these in accepted ADRs, the Revision 11 contracts, and an explicit predecessor DAG.
docs/arch/refactor/rev11/baseline/9af553dd.md:255:Before implementation claims are accepted, collect:
docs/arch/refactor/rev11/charters/A6.md:26:Maintainer-accepted immutable lock record; exact entry and implementation SHA/tree; package/DAG/program-state digests; locked performance-gates.toml; resolved orchestration/permission/worktree/CI/merge/stack policy; first block charters/context packets/stack placement; exact independent review state.
docs/arch/refactor/rev11/charters/A6.md:34:Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A6` is accepted only when its evidence is attached to one unchanged candidate SHA/tree and evidence digest.
docs/arch/refactor/rev11/contracts/architecture-falsification.md:10:No implementor or orchestrator may claim “best possible” merely because a design is detailed. The accepted claim is that the target is the strongest defensible architecture under the current evidence and that every evidence-sensitive premise has a falsification path.
docs/arch/refactor/rev11/contracts/architecture-falsification.md:28:Changing one requires an accepted ADR that explicitly supersedes the affected rule, updates `architecture.md`, identifies downstream blocks/evidence invalidated, and re-ratifies affected charters. A local benchmark win is insufficient.
docs/arch/refactor/rev11/contracts/architecture-falsification.md:38:- which Svelte operations are supported;
docs/arch/refactor/rev11/contracts/architecture-falsification.md:64:It cannot enter a candidate under “future proofing.” It requires its own measured block and accepted premise update.
docs/arch/refactor/rev11/contracts/architecture-falsification.md:89:- flow coverage and oracle premises;
docs/arch/refactor/rev11/contracts/architecture-falsification.md:102:3. preserve work whose correctness is independent of the premise;
docs/arch/refactor/rev11/contracts/architecture-falsification.md:104:5. invalidate downstream charters, gates, or approvals whose basis changed;
docs/arch/refactor/rev11/contracts/architecture-falsification.md:111:For every Foundational block, the independent architecture challenger must attempt to disprove:
docs/arch/refactor/rev11/decisions/ADR-004-typescript-semantic-profiles.md:5:**Reopen only if:** a supported operation can be proven independent from every profile dimension currently classified as semantic.
docs/arch/refactor/rev11/decisions/ADR-004-typescript-semantic-profiles.md:9:TypeScript compatibility varies by supported TypeScript family and semantics-affecting project options. Rendering, serialization, worker count, cache policy, and internal implementation versions do not change TypeScript meaning and should not over-invalidate semantic results.
docs/arch/refactor/rev11/decisions/ADR-004-typescript-semantic-profiles.md:37:- presentation and serialization changes do not invalidate semantic computation unnecessarily.
docs/arch/refactor/rev11/decisions/ADR-004-typescript-semantic-profiles.md:42:- **One giant profile containing every option/version:** over-invalidates and turns implementation history into semantics.
docs/arch/refactor/rev11/README.md:24:  and its dependency DAG.
docs/arch/refactor/rev11/README.md:50:- The tracked live ledger records A0–B1 accepted at the integration tip from which
docs/arch/refactor/rev11/README.md:51:  AMD-005 was rebased. B1's accepted identity is
docs/arch/refactor/rev11/README.md:88:  prepares exact Vue RC.3 and Svelte 5.56.8 compatibility locks, five new
docs/arch/refactor/rev11/README.md:90:  56-block DAG. It has no execution authority until three independent challenge
docs/arch/refactor/rev11/README.md:95:  {B2, B3}; narrows BF3 to Svelte/non-Vue-runtime scope; and requires BV1 to preserve
docs/arch/refactor/rev11/README.md:96:  BV0's corrections. It has no execution authority until three independent challenge
docs/arch/refactor/rev11/decisions/ADR-020-constitutional-invariants-and-falsifiable-tactics.md:23:Revision 11 is a falsifiable architecture authority rather than a claim of global optimality. Evidence can improve the implementation without reopening core ownership casually.
docs/arch/refactor/rev11/charters/C3.md:6:the closed typed project-information demands accepted in BV1 using the canonical
docs/arch/refactor/rev11/charters/C3.md:55:carries the sorted unique dependency identities and the exact semantic profile/input
docs/arch/refactor/rev11/charters/C3.md:80:TSC/TSX, declaration, and imported-macro cell by running accepted BV1 codegen with C3;
docs/arch/refactor/rev11/decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md:9:Building graph/domain features across many accepted merges while the old syntax-shaped evaluator remains selectable creates two production authorities. Deleting the old path only after full parity pressures implementors to copy the legacy model into the replacement.
docs/arch/refactor/rev11/decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md:13:1. Build the minimum new graph/domain/obligation/coverage foundation behind a private non-production test boundary on the bounded cutover branch; it may be reviewed as a checkpoint but never merged or released independently.
docs/arch/refactor/rev11/decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md:19:No runtime flag, compatibility shim, shadow evaluator, or compare-in-production path survives the accepted cutover.
docs/arch/refactor/rev11/charters/B3.md:3:**Status:** PROPOSED amendment / LOCKED. **Predecessor:** BF3.
docs/arch/refactor/rev11/charters/B3.md:7:VDOM/Vapor; Svelte runes/legacy where applicable; normalized framework-local options;
docs/arch/refactor/rev11/ORCHESTRATOR.md:16:- delegate substantial independent work through immutable context packets;
docs/arch/refactor/rev11/ORCHESTRATOR.md:20:You must not silently change architecture, weaken a gate, widen a charter, self-approve, or treat your own summary as independent review.
docs/arch/refactor/rev11/ORCHESTRATOR.md:106:Do not start `A1` or any production cutover until A0 has been accepted under `governance.md`.
docs/arch/refactor/rev11/ORCHESTRATOR.md:125:Do not create a program-wide stack. During A0, record whether GitHub native stacks, ordinary dependent PRs, merge queues, and signed rebases are available. A6 selects the operational tooling and locks a bounded stack-window policy. The default maximum is four open review layers; the permitted A6 range is two through six.
docs/arch/refactor/rev11/ORCHESTRATOR.md:127:Private atomic layers, especially D1, are never independently mergeable to trunk.
docs/arch/refactor/rev11/ORCHESTRATOR.md:141:- actual model/runtime differs from the Opus adapter without an accepted exception.
docs/arch/refactor/rev11/release/opus-start-here-v11.md:69:Do not treat several identically prompted model instances as automatically independent. Foundational review requires distinct mandates, clean contexts, direct evidence, and exact-candidate binding; a different model or human reviewer is valuable where available.
docs/arch/refactor/rev11/release/opus-orchestrator-prompt-v11.md:11:Extract and validate the attached split package, then begin at `ORCHESTRATOR.md`. Execute **only block A0** in this run. Do not implement later architecture blocks, widen scope, alter accepted architecture, choose post-result gates, or create a program-wide PR stack.
docs/arch/refactor/rev11/release/opus-orchestrator-prompt-v11.md:17:Use subagents only for genuinely independent substantial work or a required distinct review mandate. Do not spawn agents merely to summarize or repeat your own conclusion. Keep active delegation bounded to the package default, give every writer one immutable context packet and one worktree/branch, and never allow two agents to overwrite the same mutable surface.
docs/arch/refactor/rev11/contracts/input-loading.md:19:`LoadSet` is normalized, sorted, deduplicated, and includes the resolution basis needed to load/commit safely. The kernel discovers all independently reachable missing observations it can identify without fabricating semantic answers.
docs/arch/refactor/rev11/contracts/input-loading.md:30:dependency depth/frontier
docs/arch/refactor/rev11/contracts/input-loading.md:41:6. Check configured limits before I/O: attempts, unique keys, bytes, dependency depth, and basis-change/churn count.
docs/arch/refactor/rev11/contracts/input-loading.md:83:- transitive dependency waves;
docs/arch/refactor/rev11/contracts/input-loading.md:85:- dependency appears between attempts;
docs/arch/refactor/rev11/contracts/current-tree-reconciliation.md:12:- **Replace** — a new owner is justified; old owner and every caller are deleted/migrated in the same accepted cutover.
docs/arch/refactor/rev11/contracts/current-tree-reconciliation.md:30:| registered source/VFS/`PublishedRoot`/workspace snapshot | host-backed source, project, invalidation, and publication basis | preserve or converge into the single committed-input role before QueryRuntime convergence; do not create a second `InputStore` by name alone | VERIFY |
docs/arch/refactor/rev11/contracts/current-tree-reconciliation.md:31:| `verter_session::resolver_core` / `ProjectSemanticDispatch` | shared host-backed module/type-resolution orchestration | preserve one resolver semantics path; extraction may change dependency direction but cannot create a second resolver | VERIFY |
docs/arch/refactor/rev11/contracts/current-tree-reconciliation.md:33:| `DeclBodyMemo` / retained parse workers / `DeclLoweringService` | lazy body lowering over retained parse snapshots | reconcile into managed parse owner domains; direct compiler remains independent | VERIFY |
docs/arch/refactor/rev11/contracts/current-tree-reconciliation.md:40:| component-meta native/compat boundary | product-facing compatibility behavior | inventory consumers/oracles and migrate after final semantic/flow plus the affected consumer identity/lifetime/admission contracts; no silent behavior merge | VERIFY |
docs/arch/refactor/rev11/contracts/current-tree-reconciliation.md:44:| audit TLS/substrate/runtime | deterministic optional observability | preserve leaf dependency direction and prove disabled overhead; do not make audit semantic authority | VERIFY |
docs/arch/refactor/rev11/contracts/current-tree-reconciliation.md:52:- the surviving owner and dependency direction are explicit;
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:37:The released product/materialization shape could be read as one global output/presentation/serialization profile applying to multiple requested products. It also bound compile facts primarily to one whole-plan token, which could over-invalidate unchanged semantic projections after a terminal-only request change.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:63:**Revision 11 correction:** the split package is explicitly canonical. `consolidation-order.txt` and `tools/build_consolidated.py` generate the reading copy; it is never independently edited.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:79:The DAG described legal dependencies, but there was no canonical ledger for READY/IN_PROGRESS/REVIEW/ACCEPTED state, exact candidates, evidence digests, stack placement, or review status. Conversation history and PR labels could silently diverge.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:91:Revision 10 allowed intermediate branch work but did not define stack size, DAG/stack authority, mergeability, sibling tracks, lower-layer changes, CI, review invalidation, merge queue behavior, or final tree equivalence.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:93:**Revision 11 correction:** stacks are bounded windows over short dependency paths, never a program-wide chain. Each mergeable layer is independently releasable and proven on the cumulative tree. Sibling DAG branches normally use separate stacks.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:97:`D1` was private and `D2` atomic in prose, but an ordinary stacked PR tool could still make the private layer independently mergeable.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:111:**Revision 11 correction:** program state records the reviewed base/candidate SHA/tree separately from the accepted base/commit/tree. Landing is legal only when a repository-validated, digest-addressed proof shows that the canonical binary candidate delta and generated-output digest are identical on the recorded reviewed and accepted bases, no manual conflict resolution occurred after review, and required post-landing checks pass. Full-tree equality is not assumed after a legitimate base advance.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:115:Revision 10 and the first Revision 11 draft said no block could begin until every predecessor was accepted, while also recommending dependent stacked PRs. Under that rule, an upper layer could not be implemented or reviewed until the lower layer had already landed, reducing the “stack” to a sequence of ordinary PRs and contradicting the delivery contract.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:117:**Revision 11 correction:** a block may remain contingent at `READY`, `IN_PROGRESS`, or `REVIEW` when each unaccepted predecessor is a lower layer in the same validated immutable stack snapshot. It cannot become `ACCEPTANCE_RECOMMENDED` or `ACCEPTED` until those predecessors are formally satisfied and the upper candidate is restacked/revalidated on the actual accepted base. The program-state and stack-window validators enforce both sides of this rule.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:119:## V10-B18 — Atomic review layers and acceptance blocks were not represented independently
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:133:A landing proof cannot contain the digest of the post-landing program state when that state itself stores the landing-proof digest. Requiring the accepted full repository tree to equal the reviewed candidate tree is also wrong after a legitimate target-base advance, even when the candidate change survived exactly.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:135:**Revision 11 correction:** the proof binds the pre-landing program-state digest only; the post-landing state stores the validated proof digest. Repository verification compares the canonical binary Git delta from reviewed base to reviewed candidate with the delta from accepted base to accepted commit, plus generated-output digests and post-landing checks. Any changed delta or post-review manual conflict requires a new candidate and review.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:151:- Gate 0 remains sequential under its own lock-building rules; after A6, short dependency paths can be reviewed in bounded layers;
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:168:- independent review mandates are not collapsed into the orchestrator's self-assessment.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:174:It is not honest to label all later blocks pre-authorized. They become implementation-ready only through the accepted A6 lock and per-block readiness state. That gate is part of a complete architecture, not an admission that the plan is unfinished.
docs/arch/refactor/rev11/contracts/identity-encoding.md:14:Unless an accepted external protocol requires another encoding, identity descriptors use a tagged, length-delimited byte form:
docs/arch/refactor/rev11/contracts/identity-encoding.md:41:`StableEntityId` derives from documented domain-separated canonical identity material and is independent of allocation, traversal schedule, worker, cache history, or interner insertion.
docs/arch/refactor/rev11/contracts/semantic-profile.md:43:- presentation changes do not invalidate semantic facts or generated code when no presentation is requested;
docs/arch/refactor/rev11/contracts/semantic-profile.md:53:- one-field semantic changes invalidate semantic reuse;
docs/arch/refactor/rev11/contracts/deterministic-ordering.md:7:No observable output, public ID, serialization, map segment order, diagnostic order, dependency fingerprint, or proof digest may depend on:
docs/arch/refactor/rev11/contracts/deterministic-ordering.md:54:- dependency/read sets are canonicalized before fingerprinting/serialization;
docs/arch/refactor/rev11/program.md:3:**Status:** Normative dependency-ordered implementation authority.  
docs/arch/refactor/rev11/program.md:4:**Durable authority:** `architecture.md`, contracts, and accepted ADRs.  
docs/arch/refactor/rev11/program.md:11:The program is a DAG of bounded clean cutovers. A block may start when every predecessor has its required accepted state and the validated `program-state.toml` marks it `READY`/`BLOCK_READY`. A dependent upper layer may instead perform contingent `READY`/`IN_PROGRESS`/`REVIEW` work only when each unaccepted predecessor is a lower layer in the same validated immutable stack snapshot. It cannot become `ACCEPTANCE_RECOMMENDED` or `ACCEPTED` until those predecessors are formally satisfied and the upper candidate is restacked/revalidated on the accepted base. Except for the explicit `D1` private checkpoint, accepted block state means the predecessor is integrated on the intended lineage. `D1` is reviewed on the bounded `D2` integration branch and is never merged/released independently; `D2` is the atomic landing unit. Parallel work is legal only when cutover closures, writable worktrees, and shared generated/owner surfaces are disjoint or explicitly serialized.
docs/arch/refactor/rev11/program.md:13:A pull-request stack is transport only. Bounded stack windows may linearize one short dependency path for review, but they do not create semantic predecessor authority. A program-wide mega-stack is prohibited. Atomic private layers land only through their final atomic candidate.
docs/arch/refactor/rev11/program.md:46:- the `A6` Implementation Lock Record is accepted;
docs/arch/refactor/rev11/program.md:48:- every predecessor is accepted on recorded SHA/tree identities, or the block is explicitly contingent draft/review work over a lower layer in the same validated stack snapshot;
docs/arch/refactor/rev11/program.md:55:- abort/rescope conditions and independent reviews are assigned;
docs/arch/refactor/rev11/program.md:81:Add exact recursive expectations, negative controls, oracle/profile stamps, crossed closure/effect/completion positions, and public replay/admission assertions.
docs/arch/refactor/rev11/program.md:115:Resolve current authorities and every affected direct consumer against the exact post-safety, instrumented lineage; classify all versions/domains and configuration fields; enumerate TypeExpr/component-meta/graph/wire consumers; lock dependency-direction test strategy; reconcile open changes and surviving instrumentation owners; decide evidence paths, program-state custody, worktree/branch naming, CI/merge constraints, review contexts, and bounded stack-window policy.
docs/arch/refactor/rev11/program.md:131:- accepted program-state, context-packet, evidence-custody, worktree, branch, CI, merge, and bounded stack-window policy;
docs/arch/refactor/rev11/program.md:140:## B1 — Neutral contracts, typed identities, profile schemas, and dependency firewall
docs/arch/refactor/rev11/program.md:144:Land the distinct identity/profile/mapping/result-contract types and forbidden dependency-edge build tests. Do not add service owners merely to hold types.
docs/arch/refactor/rev11/program.md:160:Implement canonical typed per-product requests, product-local output/terminal profiles, framework requests, minimal prerequisite closure, independently keyed reusable subplans, one-plan-per-request default, unsupported/duplicate-combination rejection, and zero-work counters.
docs/arch/refactor/rev11/program.md:169:Introduce logical units, `PlacementMap`, required `SourceProjectionMap`, optional `RuntimeSourceMapData`, terminal `EncodedSourceMap`, and immutable generated-code-plus-required-map artifacts. Migrate every current compiler consumer in the bounded family and delete source-sized `eval_source`/blank-buffer ownership in the same accepted cutover.
docs/arch/refactor/rev11/program.md:177:Introduce the borrowed direct boundary over the surviving shared frontend, planner, units, mappings, and framework algorithms. Keep arenas/local state owner-affine, construct no managed/session/provider/audit infrastructure, materialize only requested terminal artifacts, separate pure Rust and FFI paths, migrate every current direct/standalone caller, and delete the displaced public/direct route in the same accepted cutover.
docs/arch/refactor/rev11/program.md:185:Add explicit borrowed/owned preparation, prepare-once/emit-many, inspectable retained weight, safe drop, direct batch ownership, deterministic aggregation, and no implicit project/provider/global state. Prove all three direct regimes use the same accepted parser/planner/unit/mapping/codegen core and delete any transition-only adapters.
docs/arch/refactor/rev11/program.md:219:Prove no-demand zero initialization, Svelte zero Vue/native projection, aggregate-project-once batch shape, and equal direct/prepared/managed semantics.
docs/arch/refactor/rev11/program.md:231:No product entry point or selectable runtime flag may reach this foundation. `D1` may receive checkpoint review approval but must not merge or release independently from `D2`. Before `D2`, it must satisfy every effective-flow capability row declared Supported/Stable in the `A6` matrix, or that row must receive an explicit reviewed breaking maturity/compatibility decision. Experimental/unsupported rows may remain typed gaps according to their contract.
docs/arch/refactor/rev11/program.md:239:In one accepted candidate:
docs/arch/refactor/rev11/program.md:245:- prove one production solver by source, dependency, runtime, and cache-admission tests.
docs/arch/refactor/rev11/program.md:259:Implement supported narrowing/invalidation/predicate/substitution mechanisms. Every authored return contributes structurally; endpoint completion alone controls fallthrough `undefined`.
docs/arch/refactor/rev11/program.md:261:## D5 — Closure, capture, freshness, and position-independent effects
docs/arch/refactor/rev11/program.md:265:Retain direct/transitive reads and writes, escape summaries, creation-time frontier/freshness, profile-compatible invalidation, and one effect mechanism across expression positions.
docs/arch/refactor/rev11/program.md:333:Implement snapshot-independent `QueryIdentity`, exact-basis `SemanticFlightKey`, bounded multi-candidate lookup, positive/negative read facts, exact result contract, owner-controlled compute scope, sealed publish/return-only decision, value-side validation, and operation-specific cycle/failure policy.
docs/arch/refactor/rev11/program.md:339:Implement content and semantic flight classes, exact input-basis scoping, independent waiters, policy aggregation, cancellation, panic/shutdown/finalization, follower validation, and no leader-owned lifetime.
docs/arch/refactor/rev11/program.md:345:Keep hits/tiny dependent work inline; fork/chunk only coarse owned work; schedule compact commands to owner-affine parse/semantic state; bound queues/fan-out/stacks; reserve interactive capacity; support local/WASM execution without semantic divergence.
docs/arch/refactor/rev11/program.md:351:Classify each current store, preserve correct value-side validation, remove duplicate correctness invalidation/dedupe, index bounded semantic candidates by snapshot-independent `QueryIdentity`, make return-only default, and enforce weight, pressure, pin, and shutdown contracts.
docs/arch/refactor/rev11/program.md:377:Publish generated companion and required `SourceProjectionMap` atomically; wait only for requested facts; validate observed document/project/config/provider/mapping/dependency stamps; bound channels and protect interactive capacity.
docs/arch/refactor/rev11/program.md:411:Provide immutable coarse dispatch, typed Vue/Svelte capabilities, a synthetic alternate-shape fixture, and monomorphic inner loops. No universal framework IR/fact/cache/options owner.
docs/arch/refactor/rev11/program.md:423:Extract each invariant only after its final owner exists, migrate all consumers, delete sideways mutable access and dependency cycles, and retain only a small stable facade where product compatibility requires it.
docs/arch/refactor/rev11/program.md:437:Run every locked absolute SLO, self no-regression, work/copy/allocation, boundary, and competitor/Pareto cell. The primary direct suite must meet its locked best-in-class target. A miss is blocking; it cannot be waived by a post-result ADR. If the product/equivalent-work premise was materially wrong, amend the architecture and Implementation Lock Record under the blind recalibration rule, invalidate the affected candidate evidence, and restart the cell/block.
docs/arch/refactor/rev11/program.md:444:Allocator, arena pooling, LTO/PGO, SIMD, hashing/container, or executor specialization may proceed only without reopening authority/lifetime boundaries. If opened, it must be accepted before `L4`.
docs/arch/refactor/rev11/program.md:459:- a required predecessor was neither accepted nor a valid lower layer in the same immutable stack snapshot, or acceptance was attempted before the predecessor landed;
docs/arch/refactor/rev11/package-README.md:10:**Implementation baseline:** one exact post-harness, post-safety, post-instrumentation SHA/tree accepted by `A6`; affected Gate 0 evidence is refreshed after every lineage change.
docs/arch/refactor/rev11/package-README.md:20:- clean Vue/Svelte and future-framework boundaries;
docs/arch/refactor/rev11/package-README.md:78:`A6` has accepted one immutable Implementation Lock Record containing:
docs/arch/refactor/rev11/package-README.md:94:One bounded block has every predecessor formally accepted, or—only for contingent stacked draft/review work—bound as a lower layer in the same validated immutable stack snapshot. It also has an exact charter/context packet, current-tree closure, discriminating tests, numeric gates, branch/worktree/stack placement, deletion set, and independent review mandates. A contingent upper layer cannot become acceptance-recommended until its predecessors land and the candidate is restacked/revalidated.
docs/arch/refactor/rev11/package-README.md:98:One exact reviewed candidate SHA/tree satisfies its charter, proof, deletions, cumulative-stack CI, and required review set, and every semantic predecessor is formally satisfied. Approval never transfers automatically across a code change or restack. After landing, program state records the actual accepted commit/tree and a validated landing-equivalence digest proving that the exact reviewed candidate delta survived on the recorded landing base; neither commit-SHA equality nor full-tree equality is assumed.
docs/arch/refactor/rev11/package-README.md:143:1. `architecture.md` is the consolidated current durable product architecture; accepted ADRs constrain scoped decisions.
docs/arch/refactor/rev11/package-README.md:165:- **Clean cutover:** one accepted merge contains one production implementation and deletes the displaced path/support machinery.
docs/arch/refactor/rev11/package-README.md:166:- **Bounded orchestration:** one legal block, one owner, one context packet, one program ledger, and independent review evidence.
docs/arch/refactor/rev11/package-README.md:167:- **Bounded stacks:** short review windows only; atomic private layers never merge independently.
docs/arch/refactor/rev11/package-README.md:174:+ compositional product-local requests and independently reusable subplans
docs/arch/refactor/rev11/package-README.md:184:+ snapshot-independent QueryIdentity candidate lookup plus exact-basis SemanticFlightKey
docs/arch/refactor/rev11/package-README.md:205:During convergence this split package is normative. The consolidated master is generated from `consolidation-order.txt` and is never edited independently. After `L4`, durable content is reduced to concise architecture documentation and retained ADRs; program files, baseline evidence, charters, agent adapters, and campaign-only material are archived or deleted after unique evidence is preserved.
docs/arch/refactor/rev11/contracts/baseline-lock.md:11:- `ImplementationBaselineSha`, accepted at `A6`, is the exact post-command-fix, post-harness, post-safety, post-instrumentation lineage on which later charters and baselines rely.
docs/arch/refactor/rev11/contracts/baseline-lock.md:27:implementation_baseline_sha = "FULL_40_HEX_POST_GATE0_SHA_ACCEPTED_BY_A6"
docs/arch/refactor/rev11/contracts/baseline-lock.md:67:- explicitly coordinate as a predecessor/dependent block.
docs/arch/refactor/rev11/contracts/baseline-lock.md:87:The accepted lock record is an immutable evidence artifact. It is not required to contain the SHA of a commit that embeds the record itself; `implementation_baseline_sha` names the exact code/evidence candidate evaluated by A6, and the record is addressed by its own digest. A later documentation-only commit that stores the record does not silently become the implementation baseline.
docs/arch/refactor/rev11/architecture.md:4:**Baseline-independent:** implementation evidence is recorded separately under `baseline/`.  
docs/arch/refactor/rev11/architecture.md:10:The durable target is baseline-independent, but an implementation block is not. Before an affected cutover starts, the implementation checkout must record one exact SHA and reconcile every current authority, consumer, lifetime, cache, protocol, and test route touched by that block.
docs/arch/refactor/rev11/architecture.md:22:The implementation-entry and reconciliation contracts live under `contracts/`. They are normative for block admission, not production runtime concepts. `program-dag.toml` is the machine-readable semantic sequencing authority; validated program state records execution, while PR stack topology is transport only. No block begins before all direct predecessors are accepted and the ledger marks it ready, except contingent upper-layer draft/review work bound to an exact lower predecessor in the same validated stack snapshot; acceptance still requires formally satisfied predecessors and restack/revalidation. Agent roles, context packets, worktrees, stack windows, review state, and campaign evidence must not leak into runtime APIs, artifact identities, production comments, or semantic behavior.
docs/arch/refactor/rev11/architecture.md:53:7. parallelize sufficiently coarse independent work;
docs/arch/refactor/rev11/architecture.md:80:- dependency fingerprints;
docs/arch/refactor/rev11/architecture.md:102:-> executor task when work is coarse and independent
docs/arch/refactor/rev11/architecture.md:106:A one-implementation public trait is rejected unless it is a real substitution boundary. A new crate is rejected unless it establishes a dependency firewall, stable protocol, or multiple credible consumers.
docs/arch/refactor/rev11/architecture.md:114:A6 accepts an architecture premise ledger. Falsified premises stop affected work, preserve independently valid work, and trigger refresh, rescope, or ADR amendment according to their class.
docs/arch/refactor/rev11/architecture.md:120:Vue and Svelte have equal architectural citizenship:
docs/arch/refactor/rev11/architecture.md:133:- oracle/conformance source;
docs/arch/refactor/rev11/architecture.md:178:    Svelte(SvelteCompileRequest),
docs/arch/refactor/rev11/architecture.md:196:- Vue- and Svelte-specific options are typed before planning;
docs/arch/refactor/rev11/architecture.md:234:Native semantic mode is independent:
docs/arch/refactor/rev11/architecture.md:284:QueryIdentity<Q>             semantic arguments + profiles + result contract; snapshot-independent candidate lookup
docs/arch/refactor/rev11/architecture.md:346:- a build/toolchain fingerprint may invalidate disposable caches without per-subsystem progress counters;
docs/arch/refactor/rev11/architecture.md:364:One unavoidable FFI ownership copy may be accepted at a host boundary. A second avoidable native copy is not.
docs/arch/refactor/rev11/architecture.md:373:- retains only independently valuable stages that were actually materialized;
docs/arch/refactor/rev11/architecture.md:402:3. **WASM single-thread:** local/cooperative execution, non-`Send` stores, and no dependency on blocking condvars or native worker pools.
docs/arch/refactor/rev11/architecture.md:406:`QueryRuntime` semantics—identity, completeness, validation, cancellation, recursion, and admission—are independent from how a profile schedules work.
docs/arch/refactor/rev11/architecture.md:416:> A reader observes one coherent source/project/configuration state before a commit or after it, never independently sampled roots from different commits.
docs/arch/refactor/rev11/architecture.md:422:Before managed QueryRuntime/cache convergence, the current input/VFS/publication owner is classified and its snapshot/fact API is locked. Query infrastructure must not be built around a transitional or independently sampled source view.
docs/arch/refactor/rev11/architecture.md:428:- accepted incremental edits are applied in order;
docs/arch/refactor/rev11/architecture.md:453:4. enforce total unique-input, byte, dependency-depth, retry, and churn budgets;
docs/arch/refactor/rev11/architecture.md:459:A basis change may justify a retry even with the same symbolic dependency, but it consumes the configured churn/restart budget. Partial load success, stable missing observations, and transient loader failures have distinct typed outcomes.
docs/arch/refactor/rev11/architecture.md:470:- Svelte carrier;
docs/arch/refactor/rev11/architecture.md:473:- any future domain accepted by ADR.
docs/arch/refactor/rev11/architecture.md:475:IDE, build, TypeInfo, lint, formatter, and framework consumers do not maintain independent parsers for equal bytes and syntax profile. If recovery or syntax capability is insufficient, Verter extends, forks, or replaces the shared frontend for both consumers rather than retaining a permanent fast-build/tolerant-IDE split.
docs/arch/refactor/rev11/architecture.md:499:Consumer role (`IDE`, `build`, `TypeInfo`, `lint`) and framework name are not identity dimensions. A genuinely incompatible syntax construction uses a different typed frontend compatibility domain or normalized `SyntaxProfileId` and requires an accepted ADR proving it cannot be unified without material correctness or performance loss.
docs/arch/refactor/rev11/architecture.md:505:Independent direct owner domains may parse independently and never consult a process-global cache. A `PreparedCarrier` reuses only what the caller explicitly retained. Managed owner domains may share equal source-neutral syntax only when source placement is represented separately and key equality is complete.
docs/arch/refactor/rev11/architecture.md:545:├── dependency/exactness basis
docs/arch/refactor/rev11/architecture.md:613:- the plan also contains narrower deterministic `ProjectionPlanToken`, product-subplan tokens, and terminal-subplan tokens, so a terminal-only change does not invalidate unchanged semantic/code artifacts;
docs/arch/refactor/rev11/architecture.md:614:- `CompileFactsBatch` binds to the exact `ProjectionPlanToken`, semantic profile, dependency/input basis, and completeness—not to unrelated presentation or serialization fields;
docs/arch/refactor/rev11/architecture.md:619:- a project/configuration basis change invalidates/replans as specified by the token rather than silently reusing facts.
docs/arch/refactor/rev11/architecture.md:635:Validated precomputed facts are optional and accepted only after domain/profile/toolchain/source/dependency/completeness/integrity/size and anti-replay validation. Callers cannot mint arbitrary resolved facts.
docs/arch/refactor/rev11/architecture.md:639:The compiler discovers exact closed projection demands. Vue runtime macro projection contains only facts runtime generation consumes: prop/model identity, optional/required/default association, ordered broad runtime constructors, `skip_check`/typed degradation, provenance anchor, and exact dependency/profile basis.
docs/arch/refactor/rev11/architecture.md:643:Current Svelte runtime compilation requests no native compile projection unless a future capability ADR proves an official semantic need.
docs/arch/refactor/rev11/architecture.md:673:4. delete that family's `TypeExpr` producer, cache, conversion, and compatibility shim in the same accepted merge;
docs/arch/refactor/rev11/architecture.md:721:Dominator indexes, loop forests, SCC indexes, reverse adjacency, def-use overlays, capture summaries, and compact schedules are permitted only as deterministic projections of one exact graph/profile identity. They cannot introduce semantic facts, are request-local by default, and have independent weight/eviction when retained.
docs/arch/refactor/rev11/architecture.md:725:A product operation names semantic obligations. The solver computes the transitive closure through one static domain registry before evaluation. Candidate domains include declared/reaching type, narrowing, relation, completion, definite assignment, capture freshness/invalidation, effect, and coverage.
docs/arch/refactor/rev11/architecture.md:763:Effects use one expression-position-independent mechanism. No IIFE/capture hook is specialized to statement, initializer, condition, argument, return, sequence, literal, or another parent position. Compatibility results match the selected semantic profile; extra analysis may decide whether answering is safe but may not publish a different compatibility answer.
docs/arch/refactor/rev11/architecture.md:788:2. route public effective-flow operations exclusively to it and delete the syntax-shaped evaluator, name maps, position-specific hooks, private relation logic, completion booleans, and related caches/tasks/flags in the same accepted merge;
docs/arch/refactor/rev11/architecture.md:793:No runtime flag or selectable shadow evaluator survives. Temporary reduction from guessed complete output to honest partial is accepted only for rows not ratified Supported/Stable, or through a separate reviewed breaking product decision.
docs/arch/refactor/rev11/architecture.md:809:      -> cheap dependent work: inline
docs/arch/refactor/rev11/architecture.md:818:`QueryIdentity<Q>` contains semantic arguments, only the semantic/output/presentation/serialization profiles observable at that typed query boundary, and `ResultContractId`. It is independent of a particular snapshot and is the bounded candidate-cache lookup identity. Terminal presentation or serialization uses a separate query/materialization identity when the underlying typed result is unchanged.
docs/arch/refactor/rev11/architecture.md:851:- each waiter has an independent result channel, deadline, cancellation, priority, and validation basis;
docs/arch/refactor/rev11/architecture.md:876:Hits/tiny dependent work run inline. Many tiny independent items are chunked. Coarse independent parse/compile/projection work may fork through structured execution. Provider/I/O actors remain separate. Every fork family declares measured grain, fan-out, cancellation/budget inheritance, priority, queue bound, stack/recursion behavior, and structured lifetime. Mature process-local execution is used before custom runtime design.
docs/arch/refactor/rev11/architecture.md:887:4. dependency-precise value validation and on-demand recomputation;
docs/arch/refactor/rev11/architecture.md:910:- formatter, index, navigation, Vue `v-bind`, CSS modules, scoped selectors/keyframes, and Svelte/framework style consumers reuse one syntax artifact/fact substrate;
docs/arch/refactor/rev11/architecture.md:986:- dependency read set.
docs/arch/refactor/rev11/architecture.md:988:The adapter validates the stamp immediately before publication. Unrelated edits do not invalidate a result.
docs/arch/refactor/rev11/architecture.md:1019:- complete semantic/profile/toolchain/source/dependency identity;
docs/arch/refactor/rev11/architecture.md:1028:Eligible artifacts are deterministic, hermetic, independently interpretable, complete, and OXC/lifetime free. Never persist owner-local parse handles, AST pointers, snapshot-local handles, transient semantic cohorts, incomplete/cancelled/budget-dependent results, ambient-state-dependent values, or provider results without a separate proven protocol.
docs/arch/refactor/rev11/architecture.md:1040:Global hash-consing is not automatic. Promotion requires reuse to exceed hashing, synchronization, and retention cost. Canonical global domains must be finite or independently bounded.
docs/arch/refactor/rev11/architecture.md:1108:## 18.1 Binding dependency rules
docs/arch/refactor/rev11/architecture.md:1110:Logical dependency direction is inward:
docs/arch/refactor/rev11/architecture.md:1114:-> shared syntax frontends and dependency-neutral DTOs
docs/arch/refactor/rev11/architecture.md:1125:- compiler may depend on syntax, the sealed semantic facade, and dependency-neutral closed DTOs;
docs/arch/refactor/rev11/architecture.md:1170:These are logical roles, not mandatory structs/crates. Existing owners survive when they already satisfy the full invariant with correct dependency direction. A module/function is preferred until a real firewall or stable multi-consumer contract requires a crate.
docs/arch/refactor/rev11/architecture.md:1227:- derived graph indexes that become independent semantic authorities;
docs/arch/refactor/rev11/architecture.md:1242:- one arena coupling independently evictable values;
docs/arch/refactor/rev11/architecture.md:1244:- correctness-driven eager reverse dependency eviction where value validation suffices;
docs/arch/refactor/rev11/architecture.md:1248:- one-dependency-per-restart lazy loading without progress bounds;
docs/arch/refactor/rev11/contracts/capability-matrix.md:4:**Current state:** seed rows as ratified at the A0-accepted base; every `Status` cell is exactly `VERIFY` — A1 ratifies no maturity, default, or compatibility cell, and every non-`VERIFY` seed cell (the Svelte experimental maturity/promise, the graph-export maturity/default/promise, the LSP default/promise, and the seeded degradation/zero-work cells) is the Revision 11 plan's own seed, carried unaltered from the A0-accepted base under the authority of that acceptance. A1 contributes execution evidence only: §2.1 maps each row to its canonical selectors and to the externally retained A1 evidence bundle; per-run counts, verdicts, receipts, and recorded pre-existing failures live exclusively in that bundle (`A1/command-proofs/index.md`, `A1/sentinel-verification.md` under the program's external evidence root) and are never inlined here, so this file is byte-stable across evidence re-runs. A3 updates any fail-closed behavior, and A5/A6 finalize the exact post-safety matrix. Affected product blocks cannot start until completed.
docs/arch/refactor/rev11/contracts/capability-matrix.md:18:| Svelte | native runtime compile | direct Rust | Experimental (verify current pin) | VERIFY | syntax/toolchain profile | pinned Svelte compiler corpus | typed unsupported/experimental behavior | zero Vue/native compile projection | experimental | VERIFY |
docs/arch/refactor/rev11/contracts/capability-matrix.md:19:| TypeInfo | `TypeAtPosition` | native | VERIFY | VERIFY | normalized TS profiles | selected TS oracle | typed partial/gap/no-value | no-flow allocates no graph/plan | VERIFY | VERIFY |
docs/arch/refactor/rev11/contracts/capability-matrix.md:34:  in-gate Verter corpus suites and the gate-internal Vue macro-oracle checks);
docs/arch/refactor/rev11/contracts/capability-matrix.md:37:  official-compiler macro-runtime oracle (row 15). Sentinels A (gate) and C
docs/arch/refactor/rev11/contracts/capability-matrix.md:46:  macro-runtime differential oracle (row 15), re-executed inside the canonical
docs/arch/refactor/rev11/contracts/capability-matrix.md:48:- **Svelte / native runtime compile:** the pinned-compiler golden checks
docs/arch/refactor/rev11/contracts/capability-matrix.md:51:  count; the bundle records the independently counted fixture inventory), and
docs/arch/refactor/rev11/contracts/capability-matrix.md:52:  the live feature-gated oracle harness (row 20). The pin itself is a tree fact
docs/arch/refactor/rev11/contracts/capability-matrix.md:69:  unfiltered gate coverage) plus the Svelte conformance golden corpus whose
docs/arch/refactor/rev11/contracts/capability-matrix.md:78:- Experimental behavior cannot be silently used as a stable oracle for another surface.
docs/arch/refactor/rev11/contracts/capability-matrix.md:88:truth above. On acceptance they govern Vue RC.3 and Svelte 5.56.8 compiler products;
docs/arch/refactor/rev11/contracts/mapping-products.md:27:Moving unchanged bytes may preserve source-neutral syntax/semantic artifacts and rebuild only placement-dependent composition.
docs/arch/refactor/rev11/contracts/mapping-products.md:42:Map encoding/serialization identity is separate from semantic/generated code identity. Changing JSON field order or encoded format does not invalidate semantic/code computation when map data is unchanged.
docs/arch/refactor/rev11/contracts/flow-completeness.md:19:-> accepted gaps/failures
docs/arch/refactor/rev11/contracts/flow-completeness.md:23:Representative domains include reaching values/types, narrowing, completion, closure capture/freshness/invalidation, effects, definite assignment, and coverage. An unknown domain/fact family is a typed unsupported obligation, not an ignored enum variant.
docs/arch/refactor/rev11/contracts/flow-completeness.md:81:`D1` may build the minimum graph/domain/ledger/finalizer foundation behind a private hermetic test boundary on the bounded `D2` integration branch. It is unreachable from product entry points. Before the public cutover, it covers every effective-flow capability row declared Supported/Stable by the `A6` matrix, unless a separate reviewed breaking product decision changes that row. It already uses stable binding identities and the shared relation/inference authority; it may not contain a temporary name-keyed or flow-private semantic authority. Experimental/unsupported rows may remain typed gaps. `D1` is a review checkpoint, not an independently mergeable/releasable production block.
docs/arch/refactor/rev11/contracts/flow-completeness.md:83:`D2` is one indivisible accepted cutover:
docs/arch/refactor/rev11/contracts/flow-completeness.md:100:- structural authored returns independent from endpoint completion;
docs/arch/refactor/rev11/contracts/flow-completeness.md:101:- closure effects independent of expression position;
docs/arch/refactor/rev11/contracts/flow-completeness.md:104:- source search, dependency graph, and runtime tests prove the legacy evaluator is absent after `D2`.
docs/arch/refactor/rev11/contracts/official-core-oracles.md:1:# Official-core oracle contract
docs/arch/refactor/rev11/contracts/official-core-oracles.md:9:| Vue | `vuejs/core v3.6.0-rc.3`, commit `3adb225775c9b28223a56e07f7a2f874b6fbb138`, tree `36da8dc8841a35d3e1163e4b9bb5752f95ca527a` | compiler oracle and official runtime executor in hermetic tests |
docs/arch/refactor/rev11/contracts/official-core-oracles.md:10:| Svelte | `sveltejs/svelte svelte@5.56.8`, tag object `a49603bbb50f948fd0c2bf5c55582a8f89b4d91c`, commit `44a7813730579b94004e182e5a67aab27aa9d2a6`, tree `63390158bfe8f997c474e35215a4fa627194c229` | compiler oracle and official runtime executor in hermetic tests |
docs/arch/refactor/rev11/contracts/official-core-oracles.md:12:The package locks under `evidence/framework-conformance/oracles/` are part of each
docs/arch/refactor/rev11/contracts/official-core-oracles.md:28:For TSC, TSX, declaration, and public API products, the oracle is instead the exact
docs/arch/refactor/rev11/contracts/official-core-oracles.md:30:contracts, and independently authored local fixtures. Official framework compilers
docs/arch/refactor/rev11/contracts/official-core-oracles.md:31:may contribute framework behavior but cannot replace that TypeScript oracle.
docs/arch/refactor/rev11/contracts/official-core-oracles.md:35:A newer Vue RC, Vue stable, or Svelte release is a distinct domain. It requires an
docs/arch/refactor/rev11/contracts/official-core-oracles.md:37:case/golden evidence, independent challenges, and maintainer ratification. A range,

exec
/bin/zsh -lc 'rg -n --hidden -S "REOPEN|reopen|accepted evidence|Accepted evidence|invalidation|invalidate|predecessor|scope widening|scope|finding disposition|ADOPT-NOW|DEFER|REJECT|block status|status transition" docs/arch/refactor/rev11/governance.md docs/arch/refactor/rev11/ORCHESTRATOR.md docs/arch/refactor/rev11/*.md docs/arch/refactor/rev11/contracts/*.md | head -n 500' in <repo-root>
 succeeded in 0ms:
docs/arch/refactor/rev11/contracts/agent-orchestration.md:10:The authority package states what must become true. The repository and accepted evidence state what is currently true. The orchestrator may never fill a gap between them by assumption.
docs/arch/refactor/rev11/contracts/agent-orchestration.md:17:- **Orchestrator:** one coordinating agent/context that owns sequencing, program-state mutation, worker assignment, evidence collection, and stop/rescope reporting.
docs/arch/refactor/rev11/contracts/agent-orchestration.md:18:- **Workers:** scoped implementor, scoper, or investigator contexts.
docs/arch/refactor/rev11/contracts/agent-orchestration.md:47:8. stop if package validation, repository identity, predecessor state, or authority ownership is ambiguous.
docs/arch/refactor/rev11/contracts/agent-orchestration.md:59:- every direct predecessor is accepted in program state, or the block is contingent `READY`/`IN_PROGRESS`/`REVIEW` work over an exact lower layer in the same validated stack snapshot;
docs/arch/refactor/rev11/contracts/agent-orchestration.md:71:- block, predecessors, charter digest, and stack position;
docs/arch/refactor/rev11/contracts/agent-orchestration.md:146:The remedy is a source-backed deviation/rescope record, not a shim, hidden flag, weakened assertion, or unilateral architecture edit.
docs/arch/refactor/rev11/contracts/agent-orchestration.md:156:- formal block rescope or DAG amendment;
docs/arch/refactor/rev11/package-README.md:94:One bounded block has every predecessor formally accepted, or—only for contingent stacked draft/review work—bound as a lower layer in the same validated immutable stack snapshot. It also has an exact charter/context packet, current-tree closure, discriminating tests, numeric gates, branch/worktree/stack placement, deletion set, and independent review mandates. A contingent upper layer cannot become acceptance-recommended until its predecessors land and the candidate is restacked/revalidated.
docs/arch/refactor/rev11/package-README.md:98:One exact reviewed candidate SHA/tree satisfies its charter, proof, deletions, cumulative-stack CI, and required review set, and every semantic predecessor is formally satisfied. Approval never transfers automatically across a code change or restack. After landing, program state records the actual accepted commit/tree and a validated landing-equivalence digest proving that the exact reviewed candidate delta survived on the recorded landing base; neither commit-SHA equality nor full-tree equality is assumed.
docs/arch/refactor/rev11/package-README.md:112:| Exact predecessor graph and clean-cutover program | `program.md`, `program-dag.toml` |
docs/arch/refactor/rev11/package-README.md:114:| Maintainer/agent roles, review, exact-candidate convergence, and rescope | `governance.md` |
docs/arch/refactor/rev11/package-README.md:143:1. `architecture.md` is the consolidated current durable product architecture; accepted ADRs constrain scoped decisions.
docs/arch/refactor/rev11/package-README.md:186:+ FlightCell-owned production and exact snapshot-scoped semantic flights
docs/arch/refactor/rev11/PROVENANCE.md:38:the A2C predecessor, reduced A3 to non-G10 retractions, and recorded structural
docs/arch/refactor/rev11/PROVENANCE.md:89:published against the tree before the AMD-001 timing rescope edited a file inside this
docs/arch/refactor/rev11/PROVENANCE.md:93:after an in-scope file changed.)
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:34:Revision 9 described a DAG but did not provide a complete machine-readable predecessor relation. Important orderings—input authority before managed query convergence, final flow before TypeExpr cutover, and numeric gates before hot-path implementation—were not unambiguously enforced.
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:36:**Revision 11 correction:** `program-dag.toml` defines every block and predecessor; `program.md` explains each edge and abort condition.
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:46:A parse `role`/`flavor` dimension could allow IDE and runtime consumers to create distinct parses even when construction semantics were identical. The phrase “one live parse identity” also did not state whether the scope was process-global, per direct invocation, per prepared value, or per managed worker.
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:84:**Revision 11 correction:** the `FlightCell` owns production. Semantic query flights are exact-`InputBasisId` scoped by default; immutable content flights may cross snapshots. Every transition finalizes once, resolves every waiter, and admits only through the owner’s sealed path.
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:122:After `A6`, a block becomes implementable only when its predecessor set, charter, current-tree closure, tests, numeric gates, deletion set, and independent reviews are complete. This prevents “following the architecture” while inventing unresolved behavior in code.
docs/arch/refactor/rev11/program.md:7:**Machine-readable predecessor authority:** `program-dag.toml`.
docs/arch/refactor/rev11/program.md:11:The program is a DAG of bounded clean cutovers. A block may start when every predecessor has its required accepted state and the validated `program-state.toml` marks it `READY`/`BLOCK_READY`. A dependent upper layer may instead perform contingent `READY`/`IN_PROGRESS`/`REVIEW` work only when each unaccepted predecessor is a lower layer in the same validated immutable stack snapshot. It cannot become `ACCEPTANCE_RECOMMENDED` or `ACCEPTED` until those predecessors are formally satisfied and the upper candidate is restacked/revalidated on the accepted base. Except for the explicit `D1` private checkpoint, accepted block state means the predecessor is integrated on the intended lineage. `D1` is reviewed on the bounded `D2` integration branch and is never merged/released independently; `D2` is the atomic landing unit. Parallel work is legal only when cutover closures, writable worktrees, and shared generated/owner surfaces are disjoint or explicitly serialized.
docs/arch/refactor/rev11/program.md:13:A pull-request stack is transport only. Bounded stack windows may linearize one short dependency path for review, but they do not create semantic predecessor authority. A program-wide mega-stack is prohibited. Atomic private layers land only through their final atomic candidate.
docs/arch/refactor/rev11/program.md:48:- every predecessor is accepted on recorded SHA/tree identities, or the block is explicitly contingent draft/review work over a lower layer in the same validated stack snapshot;
docs/arch/refactor/rev11/program.md:55:- abort/rescope conditions and independent reviews are assigned;
docs/arch/refactor/rev11/program.md:85:## A2C — Retired completion predecessor
docs/arch/refactor/rev11/program.md:259:Implement supported narrowing/invalidation/predicate/substitution mechanisms. Every authored return contributes structurally; endpoint completion alone controls fallthrough `undefined`.
docs/arch/refactor/rev11/program.md:265:Retain direct/transitive reads and writes, escape summaries, creation-time frontier/freshness, profile-compatible invalidation, and one effect mechanism across expression positions.
docs/arch/refactor/rev11/program.md:307:## E4 — Reclaimable semantic storage and scoped interning
docs/arch/refactor/rev11/program.md:311:Implement generation/cohort-safe handles, explicit pins, selective promotion, scoped/compactable interners, and owner-local byte bounds. No public output pins internal arenas.
docs/arch/refactor/rev11/program.md:333:Implement snapshot-independent `QueryIdentity`, exact-basis `SemanticFlightKey`, bounded multi-candidate lookup, positive/negative read facts, exact result contract, owner-controlled compute scope, sealed publish/return-only decision, value-side validation, and operation-specific cycle/failure policy.
docs/arch/refactor/rev11/program.md:351:Classify each current store, preserve correct value-side validation, remove duplicate correctness invalidation/dedupe, index bounded semantic candidates by snapshot-independent `QueryIdentity`, make return-only default, and enforce weight, pressure, pin, and shutdown contracts.
docs/arch/refactor/rev11/program.md:367:## H2 — Project-scoped ProviderHub bindings
docs/arch/refactor/rev11/program.md:371:Implement explicit capability-declared provider routes/epochs, no racing or silent semantic fallback, demand-scoped companions, controlled transitions, and orthogonal native enrichment. Keep `verter_tsc` a narrow batch-checker boundary.
docs/arch/refactor/rev11/program.md:437:Run every locked absolute SLO, self no-regression, work/copy/allocation, boundary, and competitor/Pareto cell. The primary direct suite must meet its locked best-in-class target. A miss is blocking; it cannot be waived by a post-result ADR. If the product/equivalent-work premise was materially wrong, amend the architecture and Implementation Lock Record under the blind recalibration rule, invalidate the affected candidate evidence, and restart the cell/block.
docs/arch/refactor/rev11/program.md:444:Allocator, arena pooling, LTO/PGO, SIMD, hashing/container, or executor specialization may proceed only without reopening authority/lifetime boundaries. If opened, it must be accepted before `L4`.
docs/arch/refactor/rev11/program.md:454:# 15. Abort and rescope conditions
docs/arch/refactor/rev11/program.md:456:Stop and return to scope when:
docs/arch/refactor/rev11/program.md:459:- a required predecessor was neither accepted nor a valid lower layer in the same immutable stack snapshot, or acceptance was attempted before the predecessor landed;
docs/arch/refactor/rev11/README.md:60:Amendments normally record deltas without editing the verbatim-reconstructed authority files. AMD-002, AMD-003, and AMD-004 are maintainer-ratified exceptions: predecessor authority is materialized in the machine-readable DAG and exact-state template, while the amended completion ownership, debt, and retraction exit are materialized in the live split files. The published consolidated and release artifacts remain immutable historical originals; for execution, AMD-004 and the amended live split files govern the A2-to-A3 lineage.
docs/arch/refactor/rev11/README.md:66:  `PRIVATE_CHECKPOINT` predecessor, yet `D1 PRIVATE_CHECKPOINT -> D2` is the plan's
docs/arch/refactor/rev11/README.md:71:- **AMD-002 — A2C completion-model predecessor for A3:**
docs/arch/refactor/rev11/README.md:72:  [`amendments/AMD-002-a2c-completion-predecessor.md`](amendments/AMD-002-a2c-completion-predecessor.md)
docs/arch/refactor/rev11/README.md:74:  A2C the predecessor of A3, points 2 through 4 remain superseded by AMD-003, and
docs/arch/refactor/rev11/README.md:83:  supersedes the A2C predecessor and reduces A3 to non-G10 wrong-complete retractions,
docs/arch/refactor/rev11/README.md:86:- **AMD-005 — Framework compiler conformance rescope (PROPOSED, NOT RATIFIED):**
docs/arch/refactor/rev11/README.md:87:  [`amendments/AMD-005-framework-compiler-conformance-rescope.md`](amendments/AMD-005-framework-compiler-conformance-rescope.md)
docs/arch/refactor/rev11/README.md:95:  {B2, B3}; narrows BF3 to Svelte/non-Vue-runtime scope; and requires BV1 to preserve
docs/arch/refactor/rev11/contracts/semantic-profile.md:43:- presentation changes do not invalidate semantic facts or generated code when no presentation is requested;
docs/arch/refactor/rev11/contracts/semantic-profile.md:53:- one-field semantic changes invalidate semantic reuse;
docs/arch/refactor/rev11/verification.md:134:- invalidate an unchanged semantic/code subplan because only one product's terminal presentation or serialization changed.
docs/arch/refactor/rev11/verification.md:192:- substitutions/generics and invalidation;
docs/arch/refactor/rev11/verification.md:538:- create/delete/rename/reopen;
docs/arch/refactor/rev11/verification.md:594:The `A6` lock sets exact aggregate and strategic-cell tolerances before candidate implementation. The target is to meet or beat the fastest valid equivalent-work Rust implementation on the primary direct-suite aggregate and to avoid material Pareto domination in strategic cells. A candidate miss is blocking and cannot be waived after results are known. If indispensable extra work or a comparison mismatch proves the locked product/equivalence premise false, the project must amend the product/architecture and Implementation Lock Record under the blind recalibration rule, invalidate affected candidate evidence, and restart. Repeated work, weaker semantics, invalid comparison, or unbounded retention are never acceptable premises.
docs/arch/refactor/rev11/verification.md:637:- create/delete/rename/move/reopen and same-content new incarnation;
docs/arch/refactor/rev11/verification.md:726:- close/reopen same content with new incarnation;
docs/arch/refactor/rev11/verification.md:781:- owner, scope, epoch, schema/algorithm, producer, consumers, and migration policy recorded;
docs/arch/refactor/rev11/verification.md:818:- only blocks with accepted predecessors become active, except contingent `READY`/`IN_PROGRESS`/`REVIEW` upper layers whose unaccepted predecessors are lower layers in the same validated immutable stack snapshot; no such upper layer reaches acceptance recommendation before predecessor landing/restack, and before `A6` no post-Gate-0 block is active;
docs/arch/refactor/rev11/verification.md:831:- program state records actual accepted commit/tree, the landing-equivalence digest, and invalidates/restacks remaining dependent work.
docs/arch/refactor/rev11/verification.md:844:| Syntax | one shared error-tolerant frontend per language domain; scoped parse owner/reparse proof |
docs/arch/refactor/rev11/verification.md:854:| Providers/LSP | project-scoped non-racing route, atomic companion/map, stale rejection |
docs/arch/refactor/rev11/contracts/result-contract-and-flight.md:17:`QueryIdentity<Q>` is the bounded cache-candidate lookup identity. It contains only profiles and contract dimensions observable at this typed query boundary; terminal presentation/serialization is keyed separately when the typed value is unchanged. `InputBasisId` scopes in-flight semantic production but is not part of cross-snapshot candidate lookup. Each cached candidate carries the exact basis, positive/negative read facts, completeness proof, and compatibility material from its production attempt. It is used only after value-side validation against the requester’s current view. A store bounds the number and weight of candidates per query identity; insertion order, newest-basis preference, and global revision are never correctness authority.
docs/arch/refactor/rev11/contracts/current-tree-reconciliation.md:30:| registered source/VFS/`PublishedRoot`/workspace snapshot | host-backed source, project, invalidation, and publication basis | preserve or converge into the single committed-input role before QueryRuntime convergence; do not create a second `InputStore` by name alone | VERIFY |
docs/arch/refactor/rev11/contracts/conformance-normalizer.md:14:- private generated identifier spelling under scope-aware alpha-normalization that
docs/arch/refactor/rev11/contracts/conformance-normalizer.md:30:change the comparison result. The suite includes scope capture/shadowing attacks,
docs/arch/refactor/rev11/contracts/conformance-goldens.md:37:pairwise coverage plus explicit high-risk interactions (macros/types, scoped/slotted
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:37:The released product/materialization shape could be read as one global output/presentation/serialization profile applying to multiple requested products. It also bound compile facts primarily to one whole-plan token, which could over-invalidate unchanged semantic projections after a terminal-only request change.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:67:Revision 10 used “maintainer/orchestrator” as one role. An agent could interpret that as permission to scope, implement, review, change gates, accept, and merge its own work.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:69:**Revision 11 correction:** the maintainer and orchestrator are separate. The orchestrator coordinates and recommends; only the designated maintainer adopts architecture, accepts A6/amendments/gate changes, formally rescopes, and authorizes landing where required.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:81:**Revision 11 correction:** `program-state.toml` contains every DAG block exactly once and is validated on every state transition. Before A6 it prevents all post-Gate-0 work; after A6 it enforces predecessor acceptance and bounded concurrency.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:85:Subagents could receive the whole plan, widen scope, share a mutable checkout, or overwrite generated/central files.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:91:Revision 10 allowed intermediate branch work but did not define stack size, DAG/stack authority, mergeability, sibling tracks, lower-layer changes, CI, review invalidation, merge queue behavior, or final tree equivalence.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:113:## V10-B17 — Accepted-only predecessor state made real stacked review impossible
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:115:Revision 10 and the first Revision 11 draft said no block could begin until every predecessor was accepted, while also recommending dependent stacked PRs. Under that rule, an upper layer could not be implemented or reviewed until the lower layer had already landed, reducing the “stack” to a sequence of ordinary PRs and contradicting the delivery contract.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:117:**Revision 11 correction:** a block may remain contingent at `READY`, `IN_PROGRESS`, or `REVIEW` when each unaccepted predecessor is a lower layer in the same validated immutable stack snapshot. It cannot become `ACCEPTANCE_RECOMMENDED` or `ACCEPTED` until those predecessors are formally satisfied and the upper candidate is restacked/revalidated on the actual accepted base. The program-state and stack-window validators enforce both sides of this rule.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:123:**Revision 11 correction:** every stack layer has a unique `layer_id`, while `block_id` identifies its program acceptance unit. `ATOMIC_REVIEW` permits repeated private layers for the same acceptance block and an explicit `foundational-private-checkpoint` predecessor, but exactly one final layer is mergeable. Program state stores only the final/current layer for the acceptance block; D1 retains its explicit private-checkpoint state.
docs/arch/refactor/rev11/contracts/mapping-products.md:42:Map encoding/serialization identity is separate from semantic/generated code identity. Changing JSON field order or encoded format does not invalidate semantic/code computation when map data is unchanged.
docs/arch/refactor/rev11/governance.md:4:**Not production architecture:** this document governs how the convergence program is scoped and proven. It must not leak into runtime APIs or source comments.
docs/arch/refactor/rev11/governance.md:6:**Sequencing authority:** no block may begin before every direct predecessor in `program-dag.toml` is accepted, except contingent `READY`/`IN_PROGRESS`/`REVIEW` work on an upper layer whose unaccepted predecessor is below it in the same validated immutable stack snapshot. Such work cannot be acceptance-recommended or accepted until the predecessor lands and the upper candidate is restacked/revalidated. Document order, available implementor capacity, or a convenient branch is not permission.
docs/arch/refactor/rev11/governance.md:17:- accept `A6`, architecture/ADR amendments, formal rescopes, and gate recalibrations;
docs/arch/refactor/rev11/governance.md:28:- block classification proposals, predecessor verification, and legal sequencing;
docs/arch/refactor/rev11/governance.md:32:- review convergence, discovery triage, and deviation/rescope recommendations;
docs/arch/refactor/rev11/governance.md:50:- review scope cone and abort conditions.
docs/arch/refactor/rev11/governance.md:54:Executes the ratified charter. It may choose private representation details but may not reopen locked ownership, add a second production path, widen scope silently, or weaken proof. A materially false charter assumption produces a deviation memo and stop.
docs/arch/refactor/rev11/governance.md:64:A person/tool/context may fill different mandates on different blocks, but one context must not scope, implement, and provide the only substantive approval for the same non-local block.
docs/arch/refactor/rev11/governance.md:82:- independent scoper;
docs/arch/refactor/rev11/governance.md:99:- incremental invalidation/publication;
docs/arch/refactor/rev11/governance.md:119:Direct predecessors and accepted SHAs:
docs/arch/refactor/rev11/governance.md:132:Consumers in scope:
docs/arch/refactor/rev11/governance.md:145:Review scope cone:
docs/arch/refactor/rev11/governance.md:147:Abort/rescope conditions:
docs/arch/refactor/rev11/governance.md:150:The charter states what must become true and what may change. It does not dictate every private implementation detail. It may not remove a predecessor edge, loosen an A6 gate, create a second production authority, or reinterpret an accepted ADR.
docs/arch/refactor/rev11/governance.md:158:- every direct predecessor from `program-dag.toml` is accepted on the intended integration lineage, or contingent upper-layer work is bound to an exact lower predecessor in the same validated stack snapshot;
docs/arch/refactor/rev11/governance.md:159:- predecessor contracts and generated identities/protocols are present in the candidate base;
docs/arch/refactor/rev11/governance.md:175:5. invalidation and rerun of affected candidate evidence.
docs/arch/refactor/rev11/governance.md:191:- lower-layer changes cascade restack, CI, state, and review invalidation;
docs/arch/refactor/rev11/governance.md:199:Review scope has four tiers:
docs/arch/refactor/rev11/governance.md:203:3. **Investigative context — allowed:** any area needed to prove/disprove an in-scope concern.
docs/arch/refactor/rev11/governance.md:206:Inspection breadth does not automatically expand blocking scope. Only the maintainer authorizes formal rescope; the orchestrator records and recommends it.
docs/arch/refactor/rev11/governance.md:232:If an adjacent issue means the charter missed the true cutover closure, implementation stops and returns to scoping. That is scope correction, not reviewer-driven scope expansion.
docs/arch/refactor/rev11/governance.md:251:Approval is attached to one exact candidate SHA **and tree**, its exact base tree, and the evidence digest. Any code/test/generated-output change or restack creates a new candidate and invalidates landing approval until impact-bounded reattestation binds to the new identity.
docs/arch/refactor/rev11/governance.md:266:independent scope
docs/arch/refactor/rev11/governance.md:303:Useful out-of-scope findings are recorded as:
docs/arch/refactor/rev11/governance.md:319:- all declared predecessors were accepted before implementation began, or any contingent stacked work remained at draft/review state and was restacked/revalidated after predecessor landing before acceptance;
docs/arch/refactor/rev11/governance.md:322:- every in-scope caller migrated;
docs/arch/refactor/rev11/governance.md:334:`D1` is the sole exception to the ordinary predecessor-integration rule: it is a reviewed private checkpoint on the bounded `D2` branch and cannot land independently. The `D2` sole-flow-solver cutover is indivisible: routing all public consumers and deleting the old evaluator occur in one accepted candidate. Later semantic coverage is added only inside the surviving solver.
docs/arch/refactor/rev11/governance.md:344:- explicit scope cone and causal-blocker rule;
docs/arch/refactor/rev11/governance.md:374:- predecessor DAG, validated program state, stack-window, and A6 gate integrity preserved;
docs/arch/refactor/rev11/contracts/flow-completeness.md:23:Representative domains include reaching values/types, narrowing, completion, closure capture/freshness/invalidation, effects, definite assignment, and coverage. An unknown domain/fact family is a typed unsupported obligation, not an ignored enum variant.
docs/arch/refactor/rev11/governance.md:4:**Not production architecture:** this document governs how the convergence program is scoped and proven. It must not leak into runtime APIs or source comments.
docs/arch/refactor/rev11/governance.md:6:**Sequencing authority:** no block may begin before every direct predecessor in `program-dag.toml` is accepted, except contingent `READY`/`IN_PROGRESS`/`REVIEW` work on an upper layer whose unaccepted predecessor is below it in the same validated immutable stack snapshot. Such work cannot be acceptance-recommended or accepted until the predecessor lands and the upper candidate is restacked/revalidated. Document order, available implementor capacity, or a convenient branch is not permission.
docs/arch/refactor/rev11/governance.md:17:- accept `A6`, architecture/ADR amendments, formal rescopes, and gate recalibrations;
docs/arch/refactor/rev11/governance.md:28:- block classification proposals, predecessor verification, and legal sequencing;
docs/arch/refactor/rev11/governance.md:32:- review convergence, discovery triage, and deviation/rescope recommendations;
docs/arch/refactor/rev11/governance.md:50:- review scope cone and abort conditions.
docs/arch/refactor/rev11/governance.md:54:Executes the ratified charter. It may choose private representation details but may not reopen locked ownership, add a second production path, widen scope silently, or weaken proof. A materially false charter assumption produces a deviation memo and stop.
docs/arch/refactor/rev11/governance.md:64:A person/tool/context may fill different mandates on different blocks, but one context must not scope, implement, and provide the only substantive approval for the same non-local block.
docs/arch/refactor/rev11/governance.md:82:- independent scoper;
docs/arch/refactor/rev11/governance.md:99:- incremental invalidation/publication;
docs/arch/refactor/rev11/governance.md:119:Direct predecessors and accepted SHAs:
docs/arch/refactor/rev11/governance.md:132:Consumers in scope:
docs/arch/refactor/rev11/governance.md:145:Review scope cone:
docs/arch/refactor/rev11/governance.md:147:Abort/rescope conditions:
docs/arch/refactor/rev11/governance.md:150:The charter states what must become true and what may change. It does not dictate every private implementation detail. It may not remove a predecessor edge, loosen an A6 gate, create a second production authority, or reinterpret an accepted ADR.
docs/arch/refactor/rev11/governance.md:158:- every direct predecessor from `program-dag.toml` is accepted on the intended integration lineage, or contingent upper-layer work is bound to an exact lower predecessor in the same validated stack snapshot;
docs/arch/refactor/rev11/governance.md:159:- predecessor contracts and generated identities/protocols are present in the candidate base;
docs/arch/refactor/rev11/governance.md:175:5. invalidation and rerun of affected candidate evidence.
docs/arch/refactor/rev11/governance.md:191:- lower-layer changes cascade restack, CI, state, and review invalidation;
docs/arch/refactor/rev11/governance.md:199:Review scope has four tiers:
docs/arch/refactor/rev11/governance.md:203:3. **Investigative context — allowed:** any area needed to prove/disprove an in-scope concern.
docs/arch/refactor/rev11/governance.md:206:Inspection breadth does not automatically expand blocking scope. Only the maintainer authorizes formal rescope; the orchestrator records and recommends it.
docs/arch/refactor/rev11/governance.md:232:If an adjacent issue means the charter missed the true cutover closure, implementation stops and returns to scoping. That is scope correction, not reviewer-driven scope expansion.
docs/arch/refactor/rev11/governance.md:251:Approval is attached to one exact candidate SHA **and tree**, its exact base tree, and the evidence digest. Any code/test/generated-output change or restack creates a new candidate and invalidates landing approval until impact-bounded reattestation binds to the new identity.
docs/arch/refactor/rev11/governance.md:266:independent scope
docs/arch/refactor/rev11/governance.md:303:Useful out-of-scope findings are recorded as:
docs/arch/refactor/rev11/governance.md:319:- all declared predecessors were accepted before implementation began, or any contingent stacked work remained at draft/review state and was restacked/revalidated after predecessor landing before acceptance;
docs/arch/refactor/rev11/governance.md:322:- every in-scope caller migrated;
docs/arch/refactor/rev11/governance.md:334:`D1` is the sole exception to the ordinary predecessor-integration rule: it is a reviewed private checkpoint on the bounded `D2` branch and cannot land independently. The `D2` sole-flow-solver cutover is indivisible: routing all public consumers and deleting the old evaluator occur in one accepted candidate. Later semantic coverage is added only inside the surviving solver.
docs/arch/refactor/rev11/governance.md:344:- explicit scope cone and causal-blocker rule;
docs/arch/refactor/rev11/governance.md:374:- predecessor DAG, validated program state, stack-window, and A6 gate integrity preserved;
docs/arch/refactor/rev11/contracts/architecture-falsification.md:28:Changing one requires an accepted ADR that explicitly supersedes the affected rule, updates `architecture.md`, identifies downstream blocks/evidence invalidated, and re-ratifies affected charters. A local benchmark win is insufficient.
docs/arch/refactor/rev11/contracts/architecture-falsification.md:79:Action when falsified: refresh | rescope | ADR amendment | abort
docs/arch/refactor/rev11/contracts/architecture-falsification.md:104:5. invalidate downstream charters, gates, or approvals whose basis changed;
docs/arch/refactor/rev11/contracts/baseline-lock.md:67:- explicitly coordinate as a predecessor/dependent block.
docs/arch/refactor/rev11/contracts/stacked-prs.md:10:A stack is transport. It cannot add, remove, or reorder program predecessors, split a program acceptance unit silently, or make an incomplete cutover releasable.
docs/arch/refactor/rev11/contracts/stacked-prs.md:18:Every layer has a unique `layer_id`; `block_id` names the program block whose charter/acceptance unit owns it. Every review and CI event binds to an immutable **StackSnapshotId**, defined as the SHA-256 of the fully resolved validated stack-window file. The file contains every exact base/head/tree, patch, generated-output, and evidence digest. Any changed input creates a new snapshot and invalidates affected approval.
docs/arch/refactor/rev11/contracts/stacked-prs.md:22:A stack-window snapshot is not the landing receipt. Actual landing is recorded by the block's validated landing-equivalence proof and the subsequent program-state transition. A used or invalidated stack snapshot is retained as evidence and replaced by a new window when work remains.
docs/arch/refactor/rev11/contracts/stacked-prs.md:33:- has every external semantic predecessor accepted and may depend on an unaccepted predecessor only when that predecessor is a lower layer in the same validated snapshot;
docs/arch/refactor/rev11/contracts/stacked-prs.md:39:An upper layer may be `READY`, `IN_PROGRESS`, or `REVIEW` while a lower in-window predecessor is unaccepted. It may not become `ACCEPTANCE_RECOMMENDED` or `ACCEPTED` until every semantic predecessor is formally `ACCEPTED`/`PRIVATE_CHECKPOINT`, the lower landing has occurred, the upper layer has been restacked on the new base, and affected checks/reviews have revalidated the new exact candidate.
docs/arch/refactor/rev11/contracts/stacked-prs.md:51:- private layers may repeat the acceptance block's `block_id` as internal checkpoints, or name an explicit `foundational-private-checkpoint` predecessor such as `D1`;
docs/arch/refactor/rev11/contracts/stacked-prs.md:53:- no intermediate layer is released, merged to trunk, or recorded as an accepted program predecessor, except an explicit program checkpoint such as D1 whose `PRIVATE_CHECKPOINT` state is valid only for the final acceptance block;
docs/arch/refactor/rev11/contracts/stacked-prs.md:75:- lower-layer churn repeatedly invalidates upper proof;
docs/arch/refactor/rev11/contracts/stacked-prs.md:76:- the review scope cone no longer fits one bounded invariant;
docs/arch/refactor/rev11/contracts/stacked-prs.md:86:- Accepted evidence never depends on uncommitted or untracked changes.
docs/arch/refactor/rev11/contracts/stacked-prs.md:125:- predecessor and mergeability revalidation;
docs/arch/refactor/rev11/contracts/stacked-prs.md:127:- invalidation of changed cumulative candidates, CI, and reviews.
docs/arch/refactor/rev11/contracts/stacked-prs.md:135:- **Bottom-up:** land only the lowest `LANDABLE` layer. Then invalidate/restack every remaining upper layer on the actual accepted base and issue a successor snapshot. If one layer remains, continue as an ordinary single PR.
docs/arch/refactor/rev11/contracts/stacked-prs.md:150:- clear or invalidate every unlanded upper block's old stack binding/review state;
docs/arch/refactor/rev11/contracts/stacked-prs.md:169:- using stack position as semantic predecessor authority;
docs/arch/refactor/rev11/contracts/stacked-prs.md:170:- allowing an upper block to reach acceptance before its semantic predecessor lands;
docs/arch/refactor/rev11/architecture.md:22:The implementation-entry and reconciliation contracts live under `contracts/`. They are normative for block admission, not production runtime concepts. `program-dag.toml` is the machine-readable semantic sequencing authority; validated program state records execution, while PR stack topology is transport only. No block begins before all direct predecessors are accepted and the ledger marks it ready, except contingent upper-layer draft/review work bound to an exact lower predecessor in the same validated stack snapshot; acceptance still requires formally satisfied predecessors and restack/revalidation. Agent roles, context packets, worktrees, stack windows, review state, and campaign evidence must not leak into runtime APIs, artifact identities, production comments, or semantic behavior.
docs/arch/refactor/rev11/architecture.md:114:A6 accepts an architecture premise ledger. Falsified premises stop affected work, preserve independently valid work, and trigger refresh, rescope, or ADR amendment according to their class.
docs/arch/refactor/rev11/architecture.md:251:- `auto` is evidence-based and project-scoped;
docs/arch/refactor/rev11/architecture.md:346:- a build/toolchain fingerprint may invalidate disposable caches without per-subsystem progress counters;
docs/arch/refactor/rev11/architecture.md:613:- the plan also contains narrower deterministic `ProjectionPlanToken`, product-subplan tokens, and terminal-subplan tokens, so a terminal-only change does not invalidate unchanged semantic/code artifacts;
docs/arch/refactor/rev11/architecture.md:619:- a project/configuration basis change invalidates/replans as specified by the token rather than silently reusing facts.
docs/arch/refactor/rev11/architecture.md:725:A product operation names semantic obligations. The solver computes the transitive closure through one static domain registry before evaluation. Candidate domains include declared/reaching type, narrowing, relation, completion, definite assignment, capture freshness/invalidation, effect, and coverage.
docs/arch/refactor/rev11/architecture.md:779:Only a private finalizer can construct `CompleteFlowResult`, and only when every required obligation is `Discharged` under the exact graph, demand, profile, input basis, and result contract. Rich audit evidence is optional/request-scoped; the production proof token is compact.
docs/arch/refactor/rev11/architecture.md:820:`SemanticFlightKey<Q> = (QueryIdentity<Q>, InputBasisId)`. The exact input basis scopes in-flight semantic production; it is not folded into cross-snapshot cache candidate lookup. A cached candidate carries its production `InputBasisId`, complete positive/negative read facts, proof, and compatibility basis and is usable only after value-side validation against the requester’s current view. A store may keep a bounded, measured number of candidates per query identity. Candidate insertion order, newest-snapshot preference, or global revision never becomes semantic authority.
docs/arch/refactor/rev11/architecture.md:830:Two default flight scopes exist:
docs/arch/refactor/rev11/architecture.md:833:2. **Semantic query flight:** keyed by exact `SemanticFlightKey<Q>`—one `QueryIdentity<Q>` and one exact `InputBasisId`/snapshot compatibility scope.
docs/arch/refactor/rev11/architecture.md:910:- formatter, index, navigation, Vue `v-bind`, CSS modules, scoped selectors/keyframes, and Svelte/framework style consumers reuse one syntax artifact/fact substrate;
docs/arch/refactor/rev11/architecture.md:957:├── project-scoped provider actor
docs/arch/refactor/rev11/architecture.md:967:`ProviderHub` is LSP-only and owns project-scoped external provider selection, lifecycle, capabilities, epochs, synchronization, health, cancellation, and deadlines.
docs/arch/refactor/rev11/architecture.md:988:The adapter validates the stamp immediately before publication. Unrelated edits do not invalidate a result.
docs/arch/refactor/rev11/architecture.md:1042:Interners are request-, project/generation-, cohort-, or bounded-workspace-scoped. Every long-lived interner reports bytes, entry count, duplicate hit rate, age, and reclamation behavior.
docs/arch/refactor/rev11/architecture.md:1192:Audit, tracing, provenance detail, and debug graph capture are request-scoped or disabled by default. When off, hot paths do not format strings, allocate event payloads, or retain audit graphs; fixed owner-local counters may remain only when measured overhead is negligible.

exec
/bin/zsh -lc 'wc -l docs/arch/refactor/rev11/ORCHESTRATOR.md docs/arch/refactor/rev11/governance.md docs/arch/refactor/rev11/program.md docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md docs/arch/refactor/rev11/charters/BF1.md docs/arch/refactor/rev11/charters/BF2.md docs/arch/refactor/rev11/charters/BF3.md docs/arch/refactor/rev11/program-dag.toml 2>/dev/null' in <repo-root>
 succeeded in 0ms:
     157 docs/arch/refactor/rev11/ORCHESTRATOR.md
     378 docs/arch/refactor/rev11/governance.md
     470 docs/arch/refactor/rev11/program.md
     354 docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md
      45 docs/arch/refactor/rev11/charters/BF1.md
      41 docs/arch/refactor/rev11/charters/BF2.md
      49 docs/arch/refactor/rev11/charters/BF3.md
     351 docs/arch/refactor/rev11/program-dag.toml
    1845 total

exec
/bin/zsh -lc 'nl -ba docs/arch/refactor/rev11/ORCHESTRATOR.md && nl -ba docs/arch/refactor/rev11/charters/BF1.md && nl -ba docs/arch/refactor/rev11/charters/BF2.md && nl -ba docs/arch/refactor/rev11/charters/BF3.md' in <repo-root>
 succeeded in 0ms:
     1	# Verter Revision 11 — Orchestrator Entry Point
     2	
     3	**Use this file as the first package instruction for the orchestrator.**  
     4	**Canonical input:** the extracted Revision 11 split package plus an actual local checkout of `pikax/verter`.  
     5	**Initial authorization:** execute `A0` only.
     6	
     7	# 1. Your role
     8	
     9	You are the implementation orchestrator, not the maintainer and not an architecture authority.
    10	
    11	Your job is to:
    12	
    13	- validate the release package and actual checkout;
    14	- maintain exact program, branch, stack, candidate, evidence, and review state;
    15	- execute only the next legal bounded block;
    16	- delegate substantial independent work through immutable context packets;
    17	- stop and report when source or evidence contradicts the plan;
    18	- recommend acceptance only from reproducible evidence.
    19	
    20	You must not silently change architecture, weaken a gate, widen a charter, self-approve, or treat your own summary as independent review.
    21	
    22	# 2. Opus adapter identity
    23	
    24	The supplied Opus bootstrap requests fixed model ID `claude-opus-5`. Record the actual model, provider, and any fallback/substitution in `program-state.toml` before repository mutation.
    25	
    26	If the actual runtime differs from the requested runtime, you may complete non-mutating package/repository inspection, but return `A0 BLOCKED` before representing the result as an Opus-governed Foundational outcome unless the designated maintainer records an explicit exception. Model brand does not replace role independence or evidence.
    27	
    28	# 3. Read before acting
    29	
    30	Read, in this order:
    31	
    32	1. `README.md`
    33	2. `contracts/agent-orchestration.md`
    34	3. `contracts/stacked-prs.md`
    35	4. `contracts/package-publication.md`
    36	5. `contracts/architecture-falsification.md`
    37	6. `program-dag.toml`
    38	7. `governance.md`
    39	8. `charters/A0.md`
    40	9. `contracts/baseline-lock.md`
    41	10. `contracts/current-tree-reconciliation.md`
    42	11. the A0-relevant sections of `verification.md`
    43	
    44	Do not load the entire package into every worker context. Give each worker only its bounded context packet and directly relevant authority/evidence.
    45	
    46	# 4. First commands
    47	
    48	From the extracted package:
    49	
    50	```bash
    51	python3 tools/validate_package.py
    52	python3 tools/selftest_orchestration.py
    53	claude --version
    54	python3 tools/validate_program_state.py \
    55	  --dag program-dag.toml \
    56	  --state templates/program-state.template.toml \
    57	  --mode template
    58	python3 tools/validate_stack_window.py \
    59	  --dag program-dag.toml \
    60	  --stack templates/stack-window.template.toml \
    61	  --mode template
    62	python3 tools/validate_landing_equivalence.py \
    63	  --proof templates/landing-equivalence.template.toml \
    64	  --mode template
    65	```
    66	
    67	From the actual Verter checkout, collect without mutation:
    68	
    69	```bash
    70	git rev-parse --show-toplevel
    71	git remote -v
    72	git branch --show-current
    73	git rev-parse HEAD
    74	git rev-parse HEAD^{tree}
    75	git status --porcelain=v1 --untracked-files=all
    76	git worktree list --porcelain
    77	git submodule status --recursive || true
    78	git log -1 --decorate=full --stat
    79	```
    80	
    81	Enumerate local/remote architecture-affecting branches and open/queued pull requests using the repository's available GitHub tooling. Record unavailable permissions or APIs rather than assuming a clean state.
    82	
    83	Initialize a repository-local ignored or external evidence directory and copy `templates/program-state.template.toml` to `program-state.toml`. Set top-level `status = "ACTIVE"`, resolve every A0-required field, then run:
    84	
    85	```bash
    86	python3 <REV11_PACKAGE>/tools/validate_program_state.py \
    87	  --dag <REV11_PACKAGE>/program-dag.toml \
    88	  --state <REPO_EVIDENCE>/program-state.toml \
    89	  --mode live
    90	```
    91	
    92	# 5. Execute only A0
    93	
    94	A0 produces:
    95	
    96	- exact entry checkout SHA/tree and dirty/untracked/submodule/worktree state;
    97	- remote, branch, target-lineage, and toolchain identity;
    98	- Revision 11 package, release, and DAG digests;
    99	- open PR/branch/queued-change disposition, including changes touching compiler, TypeInfo, flow, source ownership, CSS, framework, LSP/provider, protocol, cache, persistence, or lifetime boundaries;
   100	- available command, CI, branch protection, merge queue, stack tooling, signing, and permission facts;
   101	- designated maintainer and orchestrator identities;
   102	- requested/actual model, orchestrator runtime/version, provider, and fallback state;
   103	- initial validated program state with only legal next work exposed;
   104	- A0 context/evidence packet and exact review state.
   105	
   106	Do not start `A1` or any production cutover until A0 has been accepted under `governance.md`.
   107	
   108	# 6. Delegation
   109	
   110	Use no subagent when A0 can be completed directly with a small number of repository/tool calls.
   111	
   112	When delegation is genuinely useful:
   113	
   114	- default to no more than three active worker contexts;
   115	- assign one role and one immutable context packet per worker;
   116	- use separate writable worktrees/branches;
   117	- do not ask workers to “review the entire architecture”;
   118	- do not use a subagent merely to echo or generically double-check your own conclusion;
   119	- keep conformance, architecture, and adversarial mandates distinct where required.
   120	
   121	# 7. Stacked PR policy
   122	
   123	Stacked PRs are allowed only under `contracts/stacked-prs.md`. Every `ACTIVE` or later stack snapshot is validated both against its pre-stack program-state basis and the current program-state ledger.
   124	
   125	Do not create a program-wide stack. During A0, record whether GitHub native stacks, ordinary dependent PRs, merge queues, and signed rebases are available. A6 selects the operational tooling and locks a bounded stack-window policy. The default maximum is four open review layers; the permitted A6 range is two through six.
   126	
   127	Private atomic layers, especially D1, are never independently mergeable to trunk.
   128	
   129	# 8. Stop instead of improvising
   130	
   131	Stop and report `BLOCKED` or `RESCOPE_REQUIRED` when:
   132	
   133	- release checksum or package validation fails;
   134	- the checkout is dirty or contains unowned work;
   135	- target trunk differs from the expected line and the difference is not dispositioned;
   136	- an open PR/branch competes for an authority;
   137	- required repository/GitHub/CI facts cannot be obtained;
   138	- source disproves an architectural assumption;
   139	- A0 would require production ownership changes;
   140	- the maintainer identity or acceptance path is absent;
   141	- actual model/runtime differs from the Opus adapter without an accepted exception.
   142	
   143	Do not fix these by hiding changes, deleting someone else's work, weakening checks, or assuming intent.
   144	
   145	# 9. Return format
   146	
   147	Lead with exactly one:
   148	
   149	```text
   150	A0 ACCEPTANCE RECOMMENDED
   151	A0 BLOCKED
   152	A0 RESCOPE REQUIRED
   153	```
   154	
   155	Then return the bounded record required by `contracts/agent-orchestration.md`, including raw evidence paths/digests, requested/actual model, exact SHA/tree, unresolved decisions, and next legal blocks derived from validated state.
   156	
   157	Do not claim the architecture is implemented or performance-proven. A0 establishes a trustworthy entry state only.
     1	# BF1 — Framework compiler contract and compatibility lock
     2	
     3	**Status:** PROPOSED / LOCKED pending AMD-005 ratification. B1 is accepted.
     4	**Class:** Foundational gate. **Predecessor:** B1.
     5	
     6	## Objective
     7	
     8	Convert AMD-005's package into a final, immutable execution lock before any harness,
     9	safety retraction, or framework correction candidate exists.
    10	
    11	## Owned scope
    12	
    13	- exact Vue RC.3 and Svelte 5.56.8 source/package closures and integrity;
    14	- product-boundary glossary and framework-owned architecture;
    15	- product/route inventory and maturity/capability matrix;
    16	- complete Vue and Svelte semantics-affecting option classifications;
    17	- conformance acceptance IDs, official-case manifests, golden/normalizer contracts;
    18	- fragment/assembly, SSR/hydration, and TypeScript-product contracts;
    19	- emitter/mapping dispositions and BF3 probe inventory;
    20	- exact performance cells locked before successor implementation; and
    21	- final DAG, charter, capability, ledger, and gate amendments.
    22	
    23	BF1 must resolve every `VERIFY`, `blocked`, and provisional option/capability row in
    24	the lock itself or explicitly make the cell unsupported fail-closed. It does not
    25	implement broad compiler behavior, the harness, or safety guards.
    26	
    27	## Required evidence and exits
    28	
    29	1. `FC-DOMAIN-001`, `FC-BOUNDARY-001`, `FC-OPTIONS-001`, and
    30	   `FC-CAPABILITY-001` pass against immutable digests.
    31	2. Package installs reproduce the committed package locks offline and reject drift.
    32	3. Every official case seed has one allowed disposition owner and no case is absent.
    33	4. Every reachable/default route is mapped to a capability cell and unknown semantic
    34	   options fail construction by contract.
    35	5. Each current emitter/mapping owner has exactly one disposition.
    36	6. Performance cells, correctness oracles, repetitions, thresholds, counters, memory
    37	   ceilings, fixture digests, and machine-lease policy are frozen before BF2 work.
    38	7. The 56-row template and live-shape DAG validations pass and new rows remain locked.
    39	
    40	## Abort/rescope
    41	
    42	Stop for an unresolved immutable package, an unclassified semantic option, a missing
    43	public/default route, an unmanifested official suite, an ambiguous product owner, a
    44	performance criterion selected after candidate measurement, or pressure to place
    45	framework behavior in B1.
     1	# BF2 — Official-core compiler conformance harness
     2	
     3	**Status:** PROPOSED / LOCKED. **Class:** Foundational test infrastructure.
     4	**Predecessor:** BF1.
     5	
     6	## Objective
     7	
     8	Build hermetic test-only infrastructure that can falsify framework output against the
     9	exact official domains without supplying production behavior.
    10	
    11	## Owned scope
    12	
    13	- offline official compiler invocation and immutable golden generation;
    14	- generated fragment and assembled JavaScript parsing;
    15	- import/export and exact-package linking;
    16	- Vue script/template assembly validation;
    17	- parser-backed cosmetic normalization and structural/topology comparison;
    18	- deterministic client and server execution against official runtimes;
    19	- hydration controls and meaningful cross-pairings;
    20	- diagnostics, source-map, and TypeScript-observable product validation;
    21	- official-case extraction, disposition, coverage accounting, and provenance; and
    22	- normalizer negative/mutation tests with proven mutation application.
    23	
    24	BF2 cannot change production compiler behavior, implement a runtime, patch generated
    25	output, inject helpers, mock missing exports, use a forbidden corpus, or let candidate
    26	output update expectations.
    27	
    28	## Required exits
    29	
    30	`FC-HARNESS-001`, `FC-MANIFEST-001`, and `FC-NORMALIZER-001` pass. Harness self-tests
    31	prove source/package drift refusal, offline execution, non-vacuous official and
    32	candidate arms, expected-golden immutability, parse/link/runtime failure detection,
    33	atomic result accounting, diagnostic/mapping discrimination, and every forbidden
    34	normalizer mutation. Every seed manifest declaration is runner-enumerated or has a
    35	reviewed allowed disposition. Performance cells locked by BF1 pass.
    36	
    37	## Abort/rescope
    38	
    39	Stop if an official runner cannot be made hermetic, official dynamic cases cannot be
    40	enumerated, expected provenance is incomplete, the runtime requires output patching,
    41	or a normalizer rule would erase semantic structure.
     1	# BF3 — Known-wrong successful-cell safety retraction
     2	
     3	**Status:** PROPOSED / LOCKED. **Class:** Foundational safety.
     4	**Predecessor:** BF2.
     5	
     6	## Objective
     7	
     8	Before B2/B3, remove every in-scope Svelte or non-Vue-runtime reachable success claim
     9	that the BF2 minimum probes prove wrong, without attempting broad backend correction.
    10	
    11	## Required procedure per successful cell
    12	
    13	1. Run minimum assembled parse, real-package link, and relevant conformance probes.
    14	2. Record exact request, route, profile, products, official domain, and failure.
    15	3. Detect the affected request before artifact publication using existing typed data.
    16	4. Return typed non-success and publish no partial JavaScript, PublicApi, TSC,
    17	   declaration, CSS, diagnostic map, or source map.
    18	5. Retract the whole capability cell when the broken subset is not safely
    19	   distinguishable.
    20	6. Add an independently authored discriminating regression.
    21	7. Name BV1 or BS1 as correction owner and bind guard deletion to that acceptance.
    22	
    23	Vue VDOM/Vapor/SSR runtime-render rows are excluded from BF3 retraction and tracking
    24	and are assigned to BV0 correction. BF3 retains this procedure for in-scope Svelte
    25	and non-Vue-runtime reachable-success cells only.
    26	
    27	BF3 owns no broad parser, semantic model, lowering, helper, hydration, SSR, mapping,
    28	or TypeScript-product correction. It cannot infer meaning from generated output or
    29	introduce string-scanning as a second semantic authority.
    30	
    31	BF3 must probe BF2's exact `svelte@5.56.8` client cells. Results against
    32	`svelte@5.56.3` do not satisfy that exit. Svelte server's existing typed
    33	`ServerGenerate` refusal is already a non-successful cell and receives no new BF3
    34	production mechanism.
    35	
    36	## Required exits
    37	
    38	`FC-ATOMIC-001` passes for success and every refusal. The full reachable-success
    39	inventory within BF3's retained scope is exhausted, including the exact Svelte client
    40	inventory and the remaining in-scope product/route inventory; every failure has a
    41	guard/whole-cell retraction, local regression, correction owner, and removal ID.
    42	Cold-path tests prove unaffected cells retain behavior. BF3 cannot accept before that
    43	exhaustion, and no B2/B3 dispatch begins before both BF3 and BV0 are accepted.
    44	
    45	## Abort/rescope
    46	
    47	Stop and retract the complete cell if typed information cannot discriminate the bad
    48	subset. Stop if a proposed guard requires a broad backend repair or would publish a
    49	partial artifact.

exec
/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/governance.md | sed -n '1,280p'" in <repo-root>
 succeeded in 0ms:
     1	# Verter Revision 11 Program Governance
     2	
     3	**Status:** Normative execution/review process.  
     4	**Not production architecture:** this document governs how the convergence program is scoped and proven. It must not leak into runtime APIs or source comments.
     5	
     6	**Sequencing authority:** no block may begin before every direct predecessor in `program-dag.toml` is accepted, except contingent `READY`/`IN_PROGRESS`/`REVIEW` work on an upper layer whose unaccepted predecessor is below it in the same validated immutable stack snapshot. Such work cannot be acceptance-recommended or accepted until the predecessor lands and the upper candidate is restacked/revalidated. Document order, available implementor capacity, or a convenient branch is not permission.
     7	
     8	**Gate authority:** the A6 Implementation Lock Record freezes numeric and compatibility decisions before non-safety cutovers. Candidate measurements cannot be used to choose their own pass criteria.
     9	
    10	# 1. Roles
    11	
    12	## 1.1 Maintainer
    13	
    14	The maintainer is the designated human or explicit repository authority. It alone may:
    15	
    16	- adopt or supersede the architecture package;
    17	- accept `A6`, architecture/ADR amendments, formal rescopes, and gate recalibrations;
    18	- approve irreversible public compatibility/protocol decisions;
    19	- accept/land/merge when repository policy requires maintainer authority.
    20	
    21	The maintainer remains accountable for evidence quality. An orchestrator or collection of model approvals cannot self-create repository authority.
    22	
    23	## 1.2 Orchestrator
    24	
    25	Owns execution coordination under `contracts/agent-orchestration.md`:
    26	
    27	- entry-checkout, implementation-baseline, candidate SHA/tree, program-state, and stack records;
    28	- block classification proposals, predecessor verification, and legal sequencing;
    29	- authority/contract/accepted-ADR consistency checks;
    30	- immutable context/evidence packets and writable worktree assignment;
    31	- integration/restack conflicts and proof that commands executed non-vacuous work;
    32	- review convergence, discovery triage, and deviation/rescope recommendations;
    33	- maintaining exact evidence and review state for the maintainer decision.
    34	
    35	The orchestrator may implement when assigned, but cannot count its own synthesis as independent review, cannot self-accept a block, and cannot silently amend architecture or gates.
    36	
    37	## 1.3 Architecture challenger
    38	
    39	For Foundational work, independently challenges the proposed target before implementation using product requirements, repository facts, measurements, compatibility obligations, and blast radius. It should inspect before reading the implementor's preferred rationale where practical.
    40	
    41	## 1.4 Scoper
    42	
    43	Enumerates:
    44	
    45	- current authorities/readers/writers/callers;
    46	- lifetime/thread/platform boundaries;
    47	- identities, caches, compatibility domains, persistence, and memory consequences;
    48	- cutover closure and deletion set;
    49	- proof gates and exclusions;
    50	- review scope cone and abort conditions.
    51	
    52	## 1.5 Implementor
    53	
    54	Executes the ratified charter. It may choose private representation details but may not reopen locked ownership, add a second production path, widen scope silently, or weaken proof. A materially false charter assumption produces a deviation memo and stop.
    55	
    56	## 1.6 Review mandates
    57	
    58	Foundational work has three distinct evidence mandates:
    59	
    60	1. **Conformance:** exact charter, required behavior/failure, deletion, and non-vacuous proof.
    61	2. **Architecture:** authority, identity, ownership, dependency direction, lifetime, platform, compatibility, public boundary, determinism, and conceptual complexity.
    62	3. **Adversarial performance/memory:** races, cancellation, stale state, budgets, pressure, deep input, long churn, task grain, work amplification, latency tails, copies, allocations, and failure containment.
    63	
    64	A person/tool/context may fill different mandates on different blocks, but one context must not scope, implement, and provide the only substantive approval for the same non-local block.
    65	
    66	Independence is established by distinct mandate, clean context, exact SHA/tree, direct evidence access, and freedom to return `NOT PROVEN`—not by product/model brand.
    67	
    68	# 2. Block classification
    69	
    70	## 2.1 Local
    71	
    72	Examples: obsolete comment/guard/doc cleanup, mechanical re-export/dependency removal, isolated internal rename with no runtime/semantic/lifetime consequence.
    73	
    74	- implementor may author charter;
    75	- one independent bounded review;
    76	- promote if owner, semantics, lifetime, compatibility, platform, public API, hot path, retained state, or broad blast radius changes.
    77	
    78	## 2.2 Subsystem
    79	
    80	Examples: one cache family, CSS artifact convergence, one framework capability, bounded API cutover, one persistence domain.
    81	
    82	- independent scoper;
    83	- conformance plus adversarial review on one exact SHA/tree;
    84	- architecture review added when authority/lifetime risk warrants it.
    85	
    86	## 2.3 Foundational
    87	
    88	Examples:
    89	
    90	- A6 Implementation Lock Record and gate changes;
    91	- input/snapshot authority;
    92	- typed identity hierarchy;
    93	- direct compiler foundation;
    94	- parse ownership/affinity;
    95	- `CompileTypeInfo`/TypeInfo kernel;
    96	- flow/effective typing;
    97	- semantic graph lifetime;
    98	- QueryRuntime/shared flights/executor;
    99	- incremental invalidation/publication;
   100	- ProviderHub route/epoch correctness;
   101	- host/Engine authority;
   102	- persistence architecture;
   103	- unsafe or intentional auto-trait changes;
   104	- native/WASM execution-profile semantics.
   105	
   106	Requires independent scoping, pre-implementation architecture challenge, and all three review mandates on one exact candidate SHA/tree.
   107	
   108	Classification guidance: mark whether a block changes authority, semantics, lifetime/concurrency/platform, public/cross-crate/wire contract, hot/retained path, or broad blast radius. Three or more normally means Foundational, but named foundational areas remain Foundational unless the charter proves a strictly mechanical change.
   109	
   110	# 3. Mandatory block charter
   111	
   112	Every block begins with a concise expiring charter:
   113	
   114	```text
   115	Block title:
   116	Review class: Local | Subsystem | Foundational
   117	Frozen implementation baseline SHA:
   118	Authority package digest / A6 lock ID:
   119	Direct predecessors and accepted SHAs:
   120	Target branch/candidate SHA/tree:
   121	Program-state digest:
   122	Context-packet digest:
   123	Stack window/layer and mergeability:
   124	Writable worktree/worker:
   125	
   126	Owner/invariant:
   127	Product operation(s):
   128	Semantic profile(s)/execution profile(s):
   129	Current authorities/readers/writers/callers:
   130	Current identity/compatibility/persistence basis:
   131	Applicable locked performance/memory cells and immutable thresholds:
   132	Consumers in scope:
   133	Explicit exclusions:
   134	Final ownership/dependency/lifetime direction:
   135	Determinism and hermeticity contract:
   136	Rejected alternatives:
   137	APIs/modules/caches/tasks/guards/comments/docs to delete:
   138	Correctness/conformance/failure proof:
   139	Incremental/stale-publication proof:
   140	Work/copy/allocation/performance/memory/soak gates:
   141	Native/WASM/FFI implications:
   142	Private construction/testing boundary before cutover:
   143	Clean-cutover sequence:
   144	Review mandates and reviewers/contexts:
   145	Review scope cone:
   146	Causal-blocker criteria specific to this block:
   147	Abort/rescope conditions:
   148	```
   149	
   150	The charter states what must become true and what may change. It does not dictate every private implementation detail. It may not remove a predecessor edge, loosen an A6 gate, create a second production authority, or reinterpret an accepted ADR.
   151	
   152	After landing, the charter is deleted or archived as historical evidence. Production code does not refer to it.
   153	
   154	# 4. Predecessor and gate enforcement
   155	
   156	Before a charter is ratified, the orchestrator verifies:
   157	
   158	- every direct predecessor from `program-dag.toml` is accepted on the intended integration lineage, or contingent upper-layer work is bound to an exact lower predecessor in the same validated stack snapshot;
   159	- predecessor contracts and generated identities/protocols are present in the candidate base;
   160	- no later branch contains an unreviewed conflicting authority;
   161	- all applicable A6 numeric cells and compatibility dispositions are populated;
   162	- the block will not measure against or depend on an implementation that its own cutover deletes;
   163	- parallel unlocked blocks have disjoint or explicitly coordinated cutover closures.
   164	
   165	An edge may be added when reality reveals a dependency. Removing an edge requires an authority amendment, a clean architecture challenge, and re-ratification of every already-open affected charter.
   166	
   167	## 4.1 Gate recalibration
   168	
   169	A locked threshold/corpus/machine class may change only when the environment or benchmark definition changed materially. Recalibration requires:
   170	
   171	1. a written cause unrelated to candidate direction;
   172	2. retained old/new calibration data;
   173	3. a reviewer who has not inspected the candidate result direction where practical;
   174	4. amendment of the Implementation Lock Record;
   175	5. invalidation and rerun of affected candidate evidence.
   176	
   177	A threshold is never relaxed because a candidate missed it. A benchmark bug is fixed and both baseline and candidate are rerun.
   178	
   179	# 5. Program state, worktrees, and stacked delivery
   180	
   181	`program-state.toml` is the durable execution ledger. Conversation history, a project board, or PR labels may mirror it but cannot replace it. The orchestrator is its sole writer; the maintainer accepts state transitions that require authority. `tools/validate_program_state.py` must pass after every transition and before a block starts, enters review, is recommended for acceptance, or is accepted.
   182	
   183	Every worker receives one immutable context packet and one writable worktree/branch. No two workers share a mutable checkout. Shared generated files, lockfiles, protocol schemas, dependency firewalls, and central manifests have one active writer lease.
   184	
   185	Stacked PRs follow `contracts/stacked-prs.md`:
   186	
   187	- the program DAG remains semantic authority;
   188	- only bounded stack windows are allowed;
   189	- every mergeable layer is independently safe and proven on its cumulative tree;
   190	- private atomic layers are draft/non-mergeable and land only through the final atomic candidate;
   191	- lower-layer changes cascade restack, CI, state, and review invalidation;
   192	- no approval transfers automatically to a new SHA/tree;
   193	- a program-wide mega-stack is prohibited.
   194	
   195	The orchestrator records old/new base/tree, patch/range-diff, evidence digest, conflicts, CI reruns, and reviewer reattestation after every restack.
   196	
   197	# 6. Scope cone
   198	
   199	Review scope has four tiers:
   200	
   201	1. **Changed surface — mandatory:** changed/deleted/replacement code, tests, touched public/wire APIs, promised deletion set.
   202	2. **Required closure — mandatory:** direct callers/readers/writers, owner lifetime, identity/validity, cache/persistence, platform/concurrency/publication, semantic dependencies needed to prove the charter.
   203	3. **Investigative context — allowed:** any area needed to prove/disprove an in-scope concern.
   204	4. **Unrelated repository quality — non-blocking:** pre-existing unrelated defects, later-program work, alternate preferences, opportunistic cleanup.
   205	
   206	Inspection breadth does not automatically expand blocking scope. Only the maintainer authorizes formal rescope; the orchestrator records and recommends it.
   207	
   208	# 7. Causal blocker rule
   209	
   210	A finding blocks the current candidate only when it establishes at least one:
   211	
   212	1. **Candidate introduction:** candidate introduced the defect.
   213	2. **Candidate amplification:** candidate materially worsens or newly exposes a pre-existing defect.
   214	3. **Charter/architecture/proof violation:** explicit invariant, exclusion, deletion, determinism, correctness, failure, performance, memory, compatibility, platform, or verification requirement is violated.
   215	4. **Incomplete cutover:** claimed final owner/path/lifetime/compatibility model is not complete.
   216	5. **Adjacent correctness exception:** a pre-existing consumer/owner makes the candidate's new invariant unsafe and therefore belongs to the true cutover closure.
   217	
   218	A blocker records:
   219	
   220	```text
   221	Finding:
   222	Severity:
   223	Candidate cause:
   224	Authority/charter requirement violated:
   225	Affected behavior/invariant:
   226	Evidence/reproduction:
   227	Minimum correction condition:
   228	```
   229	
   230	The reviewer specifies the minimum condition, not an unrelated redesign.
   231	
   232	If an adjacent issue means the charter missed the true cutover closure, implementation stops and returns to scoping. That is scope correction, not reviewer-driven scope expansion.
   233	
   234	# 8. Review result contract
   235	
   236	Each mandate returns only:
   237	
   238	```text
   239	PASS
   240	BLOCKING FINDINGS
   241	NOT PROVEN
   242	NON-BLOCKING DISCOVERIES
   243	```
   244	
   245	`NOT PROVEN` blocks when the missing evidence is an explicit architecture/charter/verification condition. Otherwise the maintainer records and dispositions the gap.
   246	
   247	Reviewers record findings before editing code. The implementor normally applies bounded fixes. A reviewer acting as fixer does not count its own patch as independent re-approval.
   248	
   249	# 9. Exact-SHA convergence
   250	
   251	Approval is attached to one exact candidate SHA **and tree**, its exact base tree, and the evidence digest. Any code/test/generated-output change or restack creates a new candidate and invalidates landing approval until impact-bounded reattestation binds to the new identity.
   252	
   253	Recheck is impact-bounded:
   254	
   255	- prior blockers;
   256	- fix delta;
   257	- newly modified code;
   258	- directly affected invariants;
   259	- required regression/performance evidence.
   260	
   261	A full fresh review is required when the fix materially changes architecture, authority, identity, compatibility, lifetime, platform behavior, or real blast radius.
   262	
   263	Foundational state:
   264	
   265	```text
   266	independent scope
   267	-> blind architecture challenge
   268	-> ratified charter
   269	-> implementation
   270	-> Candidate A
   271	-> conformance review
   272	-> architecture review
   273	-> adversarial performance/memory review
   274	-> bounded fixes
   275	-> Candidate B
   276	-> impact-bounded recheck by all three mandates
   277	-> all three PASS on Candidate B
   278	-> maintainer acceptance/land
   279	```
   280	

exec
/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/governance.md | sed -n '281,410p' && nl -ba docs/arch/refactor/rev11/contracts/architecture-falsification.md | sed -n '1,125p' && nl -ba docs/arch/refactor/rev11/contracts/agent-orchestration.md | sed -n '130,180p'" in <repo-root>
 succeeded in 0ms:
   281	Three generic approvals do not satisfy the three distinct mandates.
   282	
   283	The **reviewed candidate identity** and the **accepted landing identity** are separate records. A merge commit, merge-queue commit, squash, or one-commit fast-forward may produce a different accepted commit SHA and full tree because the landing base advanced or the reviewed branch was reconstructed before final freeze. That is legal only when a repository-validated landing-equivalence artifact proves exact canonical candidate-delta equality from reviewed base to reviewed candidate versus accepted base to accepted commit, generated-output digests match, no manual conflict resolution occurred after review, and all required post-landing checks pass. A single program block is not co-batched with unrelated changes in the same landing delta. The accepted identity never replaces or retroactively changes the SHA/tree reviewers actually inspected.
   284	
   285	# 10. Architecture deviation
   286	
   287	When source or measurement disproves a charter assumption, the implementor stops and records:
   288	
   289	```text
   290	Failed assumption:
   291	Measured/source evidence:
   292	Affected architecture/verification invariants:
   293	Compatibility or consumer consequences:
   294	Alternatives:
   295	Recommended amendment:
   296	Work that remains valid:
   297	```
   298	
   299	The maintainer decides whether to amend the ADR/architecture/charter, split the block, or abort. A deviation cannot be hidden behind a flag, shim, parallel path, new cache, broader trait, or weaker test.
   300	
   301	# 11. Non-blocking discovery queue
   302	
   303	Useful out-of-scope findings are recorded as:
   304	
   305	- `DISC-CORRECTNESS` likely correctness/security bug;
   306	- `DISC-ARCH` architecture cleanup;
   307	- `DISC-PERF` performance/memory opportunity;
   308	- `DISC-DOC` documentation/cleanup;
   309	- `DISC-INVESTIGATE` investigate later.
   310	
   311	The finding states why it is outside the current causal boundary. The maintainer dispositions it into an existing future block, a new bounded block, urgent interruption, ordinary issue/backlog, or rejection.
   312	
   313	The review artifact is not a permanent campaign database. Discoveries are moved to normal project tracking and cleared after the block lands.
   314	
   315	# 12. Clean-cutover governance
   316	
   317	An accepted cutover must prove:
   318	
   319	- all declared predecessors were accepted before implementation began, or any contingent stacked work remained at draft/review state and was restacked/revalidated after predecessor landing before acceptance;
   320	
   321	- one surviving production implementation;
   322	- every in-scope caller migrated;
   323	- old declaration/implementation/cache/task/metric/flag/dependency/re-export/wire field/fixture removed or explicitly retained under a real compatibility obligation;
   324	- compiler/build errors used as caller inventory;
   325	- no runtime switch, shadow evaluator, parser fork, alternate resolver, or duplicate cache-correctness authority;
   326	- no migration-only scanner/allowlist/guard;
   327	- historical comments cleaned;
   328	- API/auto-trait/compatibility changes documented;
   329	- behavior, work, performance, memory, and failure evidence executed;
   330	- negative net conceptual complexity unless explicitly ratified.
   331	
   332	Intermediate branch commits may be broken. Private replacement machinery may exist on the implementation branch and be exercised by hermetic tests. It must remain unreachable from production entry points until the atomic cutover, and it is deleted or becomes the sole path in the accepted merge. The accepted merge is not releasable until the cutover is complete.
   333	
   334	`D1` is the sole exception to the ordinary predecessor-integration rule: it is a reviewed private checkpoint on the bounded `D2` branch and cannot land independently. The `D2` sole-flow-solver cutover is indivisible: routing all public consumers and deleting the old evaluator occur in one accepted candidate. Later semantic coverage is added only inside the surviving solver.
   335	
   336	# 13. Evidence independence
   337	
   338	Independent evidence requires:
   339	
   340	- clean or intentionally bounded context;
   341	- distinct mandate;
   342	- exact baseline and candidate SHA;
   343	- direct access to diff, source, tests, benchmarks, profiles, and raw outputs;
   344	- explicit scope cone and causal-blocker rule;
   345	- permission to challenge plan assumptions;
   346	- permission to return `NOT PROVEN`;
   347	- no reliance solely on implementor summary.
   348	
   349	Multiple automated/model instances with identical prompt/context and no independent inspection are not automatically independent. A strong human or automated reviewer may satisfy a mandate when the evidence discipline is met.
   350	
   351	# 14. Documentation and comment governance
   352	
   353	Durable production comments explain current invariants, safety/memory rationale, concurrency/ordering, observable compatibility, or why a simpler implementation is incorrect.
   354	
   355	Disallowed final-state comments include:
   356	
   357	- program track/block/phase instructions;
   358	- model/reviewer assignments;
   359	- branch/SHA/line-number migration notes;
   360	- links to expiring plans as the only explanation;
   361	- “landed/deferred/follow-up” history for work expected to disappear;
   362	- obsolete legacy descriptions.
   363	
   364	A one-time inventory script may assist cleanup but is deleted after use. Ordinary durable documentation link checking may remain.
   365	
   366	# 15. Final maintainer decision
   367	
   368	Review convergence is necessary but not sufficient. The maintainer confirms from the orchestrator's exact evidence record:
   369	
   370	- architecture/ADR consistency;
   371	- exact-SHA evidence completeness;
   372	- unresolved discoveries and compatibility obligations dispositioned;
   373	- no hidden deviations or unreviewed generated changes;
   374	- predecessor DAG, validated program state, stack-window, and A6 gate integrity preserved;
   375	- clean cutover/deletion complete and target tree equivalent to the reviewed candidate;
   376	- release/product maturity statements updated.
   377	
   378	Only then is the candidate accepted.
     1	# Architecture Premise and Falsification Contract
     2	
     3	**Status:** Normative.  
     4	**Owner:** architecture authority, A6 lock, and deviation governance.
     5	
     6	# 1. Purpose
     7	
     8	The program optimizes for the best final system, not for loyalty to a document. It must distinguish rules that define the desired product from implementation tactics that are selected by evidence.
     9	
    10	No implementor or orchestrator may claim “best possible” merely because a design is detailed. The accepted claim is that the target is the strongest defensible architecture under the current evidence and that every evidence-sensitive premise has a falsification path.
    11	
    12	# 2. Decision classes
    13	
    14	## 2.1 Constitutional invariant
    15	
    16	A constitutional invariant defines correctness, authority, product behavior, trust, or lifetime boundaries. Examples include:
    17	
    18	- one semantic authority for one operation;
    19	- unsupported work cannot publish as complete;
    20	- direct compilation does not depend on managed/session/provider ownership;
    21	- exact authoritative inputs and profiles determine observable semantics;
    22	- shared published artifacts are immutable;
    23	- retained state is bounded and reclaimable;
    24	- a clean cutover leaves one production path;
    25	- public/wire/persistent compatibility has an explicit owner;
    26	- required mapping correctness is not weakened to save optional source-map work.
    27	
    28	Changing one requires an accepted ADR that explicitly supersedes the affected rule, updates `architecture.md`, identifies downstream blocks/evidence invalidated, and re-ratifies affected charters. A local benchmark win is insufficient.
    29	
    30	## 2.2 Repository fact or product obligation
    31	
    32	A repository fact describes the current tree, current consumers, current compatibility obligations, current toolchain, or current product maturity. It must be established by A0–A6 evidence and may change when the checkout changes.
    33	
    34	Examples include:
    35	
    36	- which parser currently owns an operation;
    37	- whether an external protocol has released consumers;
    38	- which Svelte operations are supported;
    39	- which open PRs alter architecture-affecting boundaries;
    40	- which commands actually execute non-zero tests;
    41	- current competitor versions and benchmark corpus.
    42	
    43	A false or changed fact stops affected work and refreshes the baseline/lock. It does not justify a compatibility shim or second owner.
    44	
    45	## 2.3 Evidence-selected tactic
    46	
    47	A tactic is a private or operational choice whose alternatives preserve every constitutional invariant and public contract. Examples include:
    48	
    49	- Rayon versus another mature bounded executor;
    50	- exact inline/fork threshold and chunk size;
    51	- cache admission score and store shard count;
    52	- arena pooling limits;
    53	- compact index layout;
    54	- bounded PR stack depth within the stack contract;
    55	- whether a derived projection is retained after measured reuse;
    56	- a specific data structure with equivalent semantics and lifetime.
    57	
    58	A tactic may change inside a ratified charter when the charter permits it and all locked gates remain unchanged. It must not be promoted into a public abstraction without a real consumer/compatibility reason.
    59	
    60	## 2.4 Deferred research choice
    61	
    62	A deferred research choice is intentionally unapproved until simpler architecture is measured and found insufficient. Examples include mutable within-unit parsing, a custom executor, specialized lock-free containers, allocator replacement, PGO, or SIMD.
    63	
    64	It cannot enter a candidate under “future proofing.” It requires its own measured block and accepted premise update.
    65	
    66	# 3. Premise ledger
    67	
    68	A6 accepts one premise ledger. Every architecture-significant or performance-significant premise records:
    69	
    70	```text
    71	Premise ID:
    72	Decision class: constitutional | repository-fact | tactic | deferred
    73	Statement:
    74	Evidence/source:
    75	Confidence and uncertainty:
    76	Falsification test or trigger:
    77	Allowed alternatives:
    78	Blocks and gates affected:
    79	Action when falsified: refresh | rescope | ADR amendment | abort
    80	Owner:
    81	```
    82	
    83	The ledger must include at minimum:
    84	
    85	- current owner/consumer and compatibility assumptions;
    86	- direct-versus-managed cost premises;
    87	- parse reuse and retention premises;
    88	- query/flight/cache premises;
    89	- flow coverage and oracle premises;
    90	- CSS reuse premises;
    91	- provider/capability premises;
    92	- executor/task-grain premises;
    93	- memory plateau and allocator-slack premises;
    94	- PR stack/reviewer-capacity premises.
    95	
    96	# 4. Falsification behavior
    97	
    98	When evidence contradicts a premise:
    99	
   100	1. stop affected implementation before adding a workaround;
   101	2. record the exact evidence and affected class;
   102	3. preserve work whose correctness is independent of the premise;
   103	4. refresh a repository fact, change an allowed tactic, or open an ADR amendment as required;
   104	5. invalidate downstream charters, gates, or approvals whose basis changed;
   105	6. resume only from a new exact authority/lock state.
   106	
   107	Disallowed responses include hiding the contradiction behind a flag, alternate semantics path, unbounded cache, broader trait, silent fallback, reduced test, or post-result gate relaxation.
   108	
   109	# 5. Architecture challenge
   110	
   111	For every Foundational block, the independent architecture challenger must attempt to disprove:
   112	
   113	- the current owner inventory;
   114	- the claimed minimal prerequisite closure;
   115	- the necessity of every new owner/cache/queue/trait/version;
   116	- lifetime and thread-affinity assumptions;
   117	- failure and cancellation completeness;
   118	- performance comparison equivalence;
   119	- deletion closure;
   120	- the classification of each contested premise.
   121	
   122	A challenge passes when no blocking contradiction remains, not when the challenger merely prefers the proposal.
   130	# 10. Mandatory stop conditions
   131	
   132	Stop and return `BLOCKED` or `RESCOPE_REQUIRED` when any of these occurs:
   133	
   134	- package, manifest, DAG, program-state, charter, or performance-gate validation fails;
   135	- actual checkout differs from the locked source and affected evidence has not been refreshed;
   136	- dirty/unowned work or a competing PR/branch touches the same authority;
   137	- a required command executes zero intended work or cannot be proven non-vacuous;
   138	- current source disproves an architecture assumption;
   139	- the true consumer/cutover closure is wider than the charter;
   140	- a public/protocol/compatibility obligation is newly discovered;
   141	- a benchmark premise or oracle is invalid;
   142	- a worker requires forbidden permissions, ambient I/O, secret access, or destructive history changes;
   143	- an atomic block would leave two production paths or an independently mergeable private layer;
   144	- required independent review cannot be obtained.
   145	
   146	The remedy is a source-backed deviation/rescope record, not a shim, hidden flag, weakened assertion, or unilateral architecture edit.
   147	
   148	# 11. Maintainer-only decisions
   149	
   150	The orchestrator may recommend but cannot self-authorize:
   151	
   152	- package adoption/supersession;
   153	- accepted ADR or architecture amendment;
   154	- `A6` acceptance;
   155	- gate weakening or benchmark premise recalibration;
   156	- formal block rescope or DAG amendment;
   157	- irreversible public compatibility/protocol policy;
   158	- merge/land/release when repository policy requires maintainer approval.
   159	
   160	# 12. Evidence custody
   161	
   162	Raw command output, profiles, benchmark samples, generated manifests, review reports, range-diffs, landing-equivalence proofs, and lock records are stored at stable paths and addressed by digest. Summaries point to raw evidence and do not replace it.
   163	
   164	A result created from a different SHA, tree, toolchain, profile, corpus, or stack base is not silently relabeled as current evidence.

exec
/bin/zsh -lc 'rg -n -C 4 -S "BV0|BF2|BF3|Svelte|Vue known|harness|oracle|reopen|accepted|scope" docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md | head -n 320' in <repo-root>
 succeeded in 0ms:
1:# AMD-005 — Framework compiler conformance rescope
2-
3-**Status:** PROPOSED — NOT RATIFIED. This candidate has no execution authority.
4-**Prepared against:** local `program/architecture-lock` commit
5-`b3249d13d07806a14a4307954dfcc459cf7301ac`, tree
--
10-
11-The published consolidated master and release artifacts remain immutable historical
12-inputs. This amendment becomes operative only after the exact candidate tree passes
13-three independent challenges and the maintainer performs the action in
14:[§15](#15-exact-ratification-action). B1 is already accepted; until ratification no
15-successor newly introduced or delayed here is dispatchable.
16-
17-## 1. Maintainer direction and product boundary
18-
19:Vue and Svelte compiler correctness are mandatory Revision 11 scope. Verter is an SFC
20-compiler. It owns SFC parsing; framework semantics; framework-owned models and product
21-plans; generated client and server JavaScript; established public API, TSC,
22-declaration, diagnostic, CSS, and mapping products; and correct generated-code
23-topology. It does not implement, fork, bundle, replace, or ship either framework
24-runtime.
25-
26-`RuntimeClient` and `RuntimeServer` name JavaScript products intended to execute on
27-the official framework runtime. They never name a Verter-owned runtime. Official
28:compilers are test oracles only; official runtimes are hermetic test executors only.
29-
30:B1 was accepted at commit `03b2fdbfc6d12452824768d9e389a5f6f3d680df`, tree
31-`7f8230066735db17650b5d594a95d597540b3729`, under
32-[`../charters/B1.md`](../charters/B1.md), unchanged. Its former implementation
33-worktree and branch have been removed. No framework semantics, compiler options,
34-conformance behavior, or fail-closed guard is added to or attributed to B1 by this
35-package.
36-
37-## 2. Exact compatibility domains
38-
39:The only Vue runtime-output oracle domain is `vuejs/core v3.6.0-rc.3`, immutable
40-commit `3adb225775c9b28223a56e07f7a2f874b6fbb138`, tree
41-`36da8dc8841a35d3e1163e4b9bb5752f95ca527a`. Vue VDOM and Vue Vapor are separate
42-capability families. Because the upstream domain is a release candidate, neither
43-family may be classified Stable.
44-
45:The only Svelte runtime-output oracle domain is `sveltejs/svelte svelte@5.56.8`, tag
46-object `a49603bbb50f948fd0c2bf5c55582a8f89b4d91c`, immutable commit
47-`44a7813730579b94004e182e5a67aab27aa9d2a6`, tree
48-`63390158bfe8f997c474e35215a4fa627194c229`.
49-
50:Exact package identities, integrities, and complete resolved harness closures are in
51-[`../evidence/framework-conformance/version-domain.md`](../evidence/framework-conformance/version-domain.md)
52-and its package locks. No dist-tag, range, branch, or automatic upgrade is allowed.
53:A later Vue RC, Vue stable, or Svelte release is a new compatibility-domain proposal
54-requiring ratification and regenerated conformance evidence.
55-
56-## 3. Architecture
57-
--
68-
69-Narrow staged IRs within one framework are preferred when they provide explicit
70-ownership, skip, reuse, mapping, or lifetime value. The following are forbidden:
71-
72:- a universal Vue/Svelte AST, template IR, runtime IR, fact bag, or options bag;
73:- a tagged Vue-or-Svelte semantic hierarchy;
74:- Vue as the implementation base for Svelte;
75-- reconstructing one product by reparsing another generated product; and
76-- production fallback to an official compiler or dependency on an official compiler
77-  or runtime.
78-
--
84-The machine-readable authority is [`../program-dag.toml`](../program-dag.toml). The
85-amended region has this acceptance shape:
86-
87-```text
88:B1 -> BF1 -> BF2 -> BF3 -> {B2, B3}
89-B2 -> C1
90-{B2, B3} -> B4
91-B4 -> {BV1, BS1}
92-{BV1, BS1} -> B5 -> B6
--
99-ownership proof establishes disjoint code, fixtures, manifests, generated artifacts,
100-and shared lockfiles. BV1 and BS1 may overlap only after the same proof additionally
101-assigns disjoint heavy-machine leases. Absence of that proof means serialization.
102-
103:B2 and B3 remain undispatchable until this amendment is ratified and BF1, BF2, and
104:BF3 are individually accepted. B5 waits for both framework trains. C4 waits for B6
105-because prepared and batch routes do not exist before B6.
106-
107-## 5. New block allocation
108-
--
110-
111-- [`../charters/BF1.md`](../charters/BF1.md): lock domains, options, capability truth,
112-  acceptance IDs, cases, conformance contracts, performance cells, and program
113-  amendments; no broad compiler fix.
114:- [`../charters/BF2.md`](../charters/BF2.md): test-only official-core harness,
115-  generated-code validation/execution, goldens, manifests, coverage, and mutation
116-  tests; no production compiler behavior or runtime.
117:- [`../charters/BF3.md`](../charters/BF3.md): retract reachable success cells proven
118-  wrong before publication; no broad backend correction.
119-- [`../charters/BV1.md`](../charters/BV1.md): Vue semantic models, plans, VDOM,
120:  Vapor, SSR, diagnostics, maps, TypeScript-visible products, and the accepted Vue
121-  pack; no Vue runtime.
122:- [`../charters/BS1.md`](../charters/BS1.md): Svelte-native client/server semantics,
123:  topology, diagnostics, maps, TypeScript-visible products, and the accepted Svelte
124:  pack; no Svelte runtime and no residual Vue assumption.
125-
126-Existing responsibilities are amended by [`../charters/B2.md`](../charters/B2.md),
127-[`B3.md`](../charters/B3.md), [`B4.md`](../charters/B4.md), [`B5.md`](../charters/B5.md),
128-[`B6.md`](../charters/B6.md), [`C3.md`](../charters/C3.md), and
--
131-
132-## 6. Canonical request and option policy
133-
134-B3 owns one typed request containing framework, exact compatibility domain, requested
135:products, client/server profile, development/production, Vue VDOM/Vapor, Svelte
136-runes/legacy where applicable, normalized framework options, map requests, capability
137-lookup, early unsupported-combination rejection, and minimal prerequisite planning.
138-There is no second semantic authority in a universal options bag.
139-
140-Every semantics-affecting official option is classified exactly once as `supported
141-canonical`, `derived`, `host-resolved`, `test-only`, `external`, `unsupported
142:fail-closed`, or `not applicable` in the Vue and Svelte inventories. Unknown
143-semantics-affecting options fail request construction. No public option is silently
144-ignored.
145-
146-## 7. Capability and maturity policy
--
159-## 8. Oracle, exclusion, and golden rules
160-
161-The binding contracts are:
162-
163:- [`../contracts/official-core-oracles.md`](../contracts/official-core-oracles.md);
164-- [`../contracts/language-tools-exclusion.md`](../contracts/language-tools-exclusion.md)
165-  and [`../contracts/third-party-exclusion.md`](../contracts/third-party-exclusion.md);
166-- [`../contracts/conformance-goldens.md`](../contracts/conformance-goldens.md); and
167-- [`../contracts/conformance-normalizer.md`](../contracts/conformance-normalizer.md).
168-
169-`vuejs/language-tools`, `sveltejs/language-tools`, Vize, rsvelte, PrimeVue,
170-`pikax/vue-benchmarks`, `pikax/svelte-benchmarks`, and every other third-party app,
171:library, compiler, or fixture repository are forbidden as oracle, corpus, expected
172-output, baseline, or acceptance source. A difference from language-tools is not by
173-itself a defect.
174-
175-Expected goldens are generated only from the exact official compiler pins. Candidate
176-Verter output cannot update its own expectations. Cosmetic normalization is limited
177:to whitespace/layout, harmless parentheses, quote spelling, and scope-aware alpha
178-renaming of private generated identifiers. It cannot erase helper family/source,
179-declarations, meaningful order, DOM/effect/block/event/component/slot/hydration/SSR
180-topology, prop-versus-attribute meaning, diagnostics, mappings, literals, or authored
181-and public names.
--
200-| FC-HARNESS-001 | hermetic official invocation, validation, execution, and mutations |
201-| FC-MANIFEST-001 | every official case has one allowed disposition |
202-| FC-ATOMIC-001 | no partial artifact publication on success or refusal |
203-| FC-NORMALIZER-001 | cosmetic-only normalizer with negative/mutation discrimination |
204:| FC-VUE-001 | complete accepted Vue VDOM/Vapor/SSR pack |
205:| FC-SVELTE-001 | complete accepted Svelte client/server runes/legacy pack |
206-| FC-HYDRATION-001 | official/official, Verter/Verter, and meaningful cross-pair proof |
207-| FC-TS-001 | exact Revision 11 TypeScript-domain observable equivalence |
208-| FC-ROUTES-001 | direct, prepared, batch, staged, and later public route equivalence |
209-| FC-ZERO-WORK-001 | unrequested stages and products perform zero work |
--
212-
213-Official-case seed manifests are at
214-[`../evidence/framework-conformance/vue-official-cases.tsv`](../evidence/framework-conformance/vue-official-cases.tsv)
215-and [`svelte-official-cases.tsv`](../evidence/framework-conformance/svelte-official-cases.tsv).
216:They are declarations to be completed by BF2, not acceptance evidence. `blocked` rows
217-must be resolved before any containing supported cell succeeds.
218-
219-## 10. Fragments, maps, server output, and hydration
220-
--
226-
227-Fragment assembly follows
228-[`../contracts/fragment-assembly.md`](../contracts/fragment-assembly.md). Server and
229-hydration behavior follows [`../contracts/ssr-hydration.md`](../contracts/ssr-hydration.md).
230:The harness cannot patch generated output, inject helpers, mock nonexistent exports,
231-or replace official runtimes with simplified ones.
232-
233-## 11. TypeScript-observable products
234-
--
241-
242-BV1 exposes closed typed demands for project-aware imported Vue macro information.
243-C3 supplies those demands and cannot replace Vue code generation.
244-
245:## 12. BF3 safety retraction
246-
247:BF3 probes every reachable cell currently reporting success. On a minimum parse,
248-link, or conformance failure it records request/route/profile/failure, detects the
249-request through existing typed information before publication, returns typed
250-non-success, publishes no partial product, adds a discriminating local regression,
251-names the later correction owner, and records guard removal as part of that owner's
252:acceptance. If a broken subset cannot be distinguished safely, BF3 retracts the
253-entire cell. Its bounded initial inspection set is
254:[`../evidence/framework-conformance/bf3-safety-retraction-scope.md`](../evidence/framework-conformance/bf3-safety-retraction-scope.md).
255-
256-## 13. Performance lock
257-
258-The candidate does not choose thresholds after observing candidate code. BF1 must
259-add and independently review the exact cells, corpora, runners, repetitions,
260-absolute/relative limits, memory limits, work counters, and heavy-machine lease
261-policy specified in
262-[`../evidence/framework-conformance/performance-impact.md`](../evidence/framework-conformance/performance-impact.md)
263:before BF2 begins. Existing required cells remain required and are not reweighted.
264-
265-## 14. Program-state transition
266-
267-The candidate DAG and both tracked state shapes contain 56 identical block IDs. New
268-rows are `LOCKED`, reviews and maintainer decisions are `PENDING`, and all identity
269:and evidence fields are empty. No existing accepted row is rewritten. The live B1
270:row remains the last integrated program fact and records its accepted commit/tree;
271-there is no separate B1 worktree. Detailed transition rules are in
272-[`../evidence/framework-conformance/program-state-transition.md`](../evidence/framework-conformance/program-state-transition.md).
273-
274:On ratification the current amendment candidate lands on the already accepted B1
275-line, after which BF1 may be exposed as `READY`. No transition exposes B2 or B3
276:before BF3 acceptance.
277-
278-## 15. Exact ratification action
279-
280-After the package is committed, three independent agents must author the architecture,
281-conformance, and governance reports at the paths reserved in
282-[`../evidence/framework-conformance/reviews/README.md`](../evidence/framework-conformance/reviews/README.md).
283-Each report must bind the same full candidate commit, repository tree, amendment
284:digest, DAG digest, both oracle-lock and exact-closure digests, and all
285-generated-manifest digests.
286-
287-The validator has two non-interchangeable phases. The immutable package candidate is
288-checked with `validate-package.mjs --pre-review`, which requires the three primary
--
300-
301-> Ratify AMD-005 for reviewed package commit `<reviewed-full-sha>`, tree
302-> `<reviewed-tree-oid>`, attached without non-report changes in ratification-bundle
303-> commit `<bundle-full-sha>`, tree `<bundle-tree-oid>`, and the listed package digests;
304:> accept the exact Vue RC.3 and Svelte 5.56.8 domains, exclusions, amended DAG,
305-> charters, capability lock, and pre-candidate performance-lock process; authorize
306-> landing that byte-exact ratification bundle on `program/architecture-lock`, whose B1
307:> predecessor is accepted at `03b2fdbfc6d12452824768d9e389a5f6f3d680df`;
308-> authorize BF1 exposure to `READY` after ratification; and authorize no B2/B3 dispatch
309:> until BF1, BF2, and BF3 are accepted.
310-
311-Any changed reviewed-package byte requires regenerated identities and fresh reports;
312-the only post-review exception is the declared attachment of those exact reports.
313-Silence, merge, or this proposal's commit is not ratification. The preparer cannot
--
332-> `7442bb9060b7faa0720e528d3f96ee1df1abff95`, tree
333-> `69502487b55f87eb7c0c009876865b64397da660`, attached without non-report changes in
334-> ratification-bundle commit `aa757eecc1f7748d2eec076ab0665da76cb2904a`, tree
335-> `0c078357bac74724208df75c25da4fa74ab95013`, and the listed package digests; accept
336:> the exact Vue RC.3 and Svelte 5.56.8 domains, exclusions, amended DAG, charters,
337-> capability lock, and pre-candidate performance-lock process; authorize landing that
338-> byte-exact ratification bundle on `program/architecture-lock`, whose B1 predecessor
339:> is accepted at `03b2fdbfc6d12452824768d9e389a5f6f3d680df`; authorize BF1 exposure to
340:> `READY` after ratification; and authorize no B2/B3 dispatch until BF1, BF2, and BF3
341:> are accepted.
342-
343-The ratification bundle `aa757eecc1f7748d2eec076ab0665da76cb2904a` landed on
344-`program/architecture-lock` by fast-forward from `b3249d13d07806a14a4307954dfcc459cf7301ac`.
345-This record was added in a separate follow-up commit and is not itself part of the
346-ratified bundle's byte-exact tree.
347-
348-## 16. Supersession and non-goals
349-
350:On ratification this amendment supersedes only the conflicting B2–B6/C3–C4 scope and
351-edges in the original split program and immutable historical copies. It retains all
352-unaffected Revision 11 constraints and AMD-001 through AMD-004. It neither accepts a
353-compiler algorithm nor adds a production dependency, runtime, broad fix, public
354-product, or compatibility claim.

exec
/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/program-dag.toml | sed -n '45,125p' && nl -ba docs/arch/refactor/rev11/README.md | sed -n '78,102p' && rg -n -S \"BV0|BF2\" docs/arch/refactor/rev11/evidence/framework-conformance/program-state-transition.md docs/arch/refactor/rev11/evidence docs/arch/refactor/rev11/amendments | head -n 200" in <repo-root>
 succeeded in 0ms:
    45	[[block]]
    46	id = "A5"
    47	name = "Owner identity profile compatibility protocol consumer inventories"
    48	class = "foundational"
    49	predecessors = ["A4"]
    50	
    51	[[block]]
    52	id = "A6"
    53	name = "Implementation Lock Record"
    54	class = "foundational-gate"
    55	predecessors = ["A5"]
    56	
    57	[[block]]
    58	id = "B1"
    59	name = "Neutral contracts identities profiles dependency firewall"
    60	class = "foundational"
    61	predecessors = ["A6"]
    62	
    63	[[block]]
    64	id = "BF1"
    65	name = "Framework compiler contract and compatibility lock"
    66	class = "foundational-gate"
    67	predecessors = ["B1"]
    68	
    69	[[block]]
    70	id = "BF2"
    71	name = "Official-core compiler conformance harness"
    72	class = "foundational"
    73	predecessors = ["BF1"]
    74	
    75	[[block]]
    76	id = "BF3"
    77	name = "Known-wrong successful-cell safety retraction"
    78	class = "foundational-safety"
    79	predecessors = ["BF2"]
    80	
    81	[[block]]
    82	id = "BV0"
    83	name = "Immediate Vue known-defect correction"
    84	class = "subsystem"
    85	predecessors = ["BF2"]
    86	
    87	[[block]]
    88	id = "B2"
    89	name = "Framework parsing recovery diagnostics and stable identities"
    90	class = "foundational"
    91	predecessors = ["BV0", "BF3"]
    92	
    93	[[block]]
    94	id = "B3"
    95	name = "Canonical typed compiler request and prerequisite planner"
    96	class = "foundational"
    97	predecessors = ["BV0", "BF3"]
    98	
    99	[[block]]
   100	id = "B4"
   101	name = "Logical source units mapping composition and atomic publication"
   102	class = "foundational-atomic"
   103	predecessors = ["B2", "B3"]
   104	
   105	[[block]]
   106	id = "BV1"
   107	name = "Vue compiler-output conformance train"
   108	class = "subsystem"
   109	predecessors = ["B4"]
   110	
   111	[[block]]
   112	id = "BS1"
   113	name = "Svelte compiler-output conformance train"
   114	class = "subsystem"
   115	predecessors = ["B4"]
   116	
   117	[[block]]
   118	id = "B5"
   119	name = "Accepted framework algorithms through sole direct compiler core"
   120	class = "foundational-atomic"
   121	predecessors = ["BV1", "BS1"]
   122	
   123	[[block]]
   124	id = "B6"
   125	name = "PreparedCarrier direct batch and direct-core closure"
    78	  supersedes AMD-002 points 2 through 4 while retaining `A2 → A2C → A3`, delivers
    79	  D6's sole completion graph early through A2C, restricts A3 to typed-gap
    80	  retraction/non-admission, and recalibrates the performance acceptance cells.
    81	- **AMD-004 — Defer structural completion to D6 and reduce A3:**
    82	  [`amendments/AMD-004-defer-completion-to-d6.md`](amendments/AMD-004-defer-completion-to-d6.md)
    83	  supersedes the A2C predecessor and reduces A3 to non-G10 wrong-complete retractions,
    84	  while leaving exact structural completion and G10 discrimination as debt `FR-D8`,
    85	  owned by D6 / `U6.LOOP_CLOSURE`.
    86	- **AMD-005 — Framework compiler conformance rescope (PROPOSED, NOT RATIFIED):**
    87	  [`amendments/AMD-005-framework-compiler-conformance-rescope.md`](amendments/AMD-005-framework-compiler-conformance-rescope.md)
    88	  prepares exact Vue RC.3 and Svelte 5.56.8 compatibility locks, five new
    89	  framework/conformance blocks, amended B/C boundaries, and a validator-clean
    90	  56-block DAG. It has no execution authority until three independent challenge
    91	  reports and an exact-tree maintainer decision exist.
    92	- **AMD-006 — Vue known-defect correction (PROPOSED, NOT RATIFIED):**
    93	  [`amendments/AMD-006-vue-known-defect-correction.md`](amendments/AMD-006-vue-known-defect-correction.md)
    94	  adds BV0 as an immediate, bounded Vue-defect-correction block between BF2 and
    95	  {B2, B3}; narrows BF3 to Svelte/non-Vue-runtime scope; and requires BV1 to preserve
    96	  BV0's corrections. It has no execution authority until three independent challenge
    97	  reports and an exact-tree maintainer decision exist.
    98	
    99	See [`PROVENANCE.md`](PROVENANCE.md) for exactly what is and is not attested about
   100	these files.
docs/arch/refactor/rev11/evidence/framework-conformance/program-state-transition.md:5:The amendment adds BF1, BF2, BF3, BV1, and BS1, changing the block universe from 51
docs/arch/refactor/rev11/evidence/framework-conformance/program-state-transition.md:45:3. Accept BF1 before BF2; BF2 before BF3; BF3 before either B2 or B3.
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:88:B1 -> BF1 -> BF2 -> BF3 -> {B2, B3}
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:103:B2 and B3 remain undispatchable until this amendment is ratified and BF1, BF2, and
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:114:- [`../charters/BF2.md`](../charters/BF2.md): test-only official-core harness,
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:216:They are declarations to be completed by BF2, not acceptance evidence. `blocked` rows
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:263:before BF2 begins. Existing required cells remain required and are not reweighted.
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:309:> until BF1, BF2, and BF3 are accepted.
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:340:> `READY` after ratification; and authorize no B2/B3 dispatch until BF1, BF2, and BF3
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md:1:# BV0 Conformance Review — candidate `c40a1ca96` vs base `b64358705`
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md:3:**Charter:** `docs/arch/refactor/rev11/charters/BV0.md` (Immediate Vue known-defect correction)
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md:44:$ BF2_ORACLE_NPM_CACHE=/tmp/bv0-oracle-npm-cache node scripts/provision-oracle-npm-cache.mjs
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md:62:BF2_ORACLE_NPM_CACHE=... BF2_ORACLE_INSTALLS=... \
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md:211:only changed variable is Verter's own emission. These are BV0-introduced regressions.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md:223:Charter impact — this is the specific thing BV0's procedure and abort clause forbid:
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md:297:1. B1 — `components/dynamic-multi-root|vdom` and `elements-text/multi-root|vdom` regressed from passing to waived (rc.1/rc.3 goldens byte-identical ⇒ BV0-caused), dispositioned into `known-divergences.json` under a note citing a "user import" that neither script-less fixture contains; violates the required procedure's "prove unaffected routes retain their prior successful result contract" and the abort clause's prohibition on substituting a waiver.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/conformance-challenge.md:24:4. Live `program-state.toml:14,299` — old DAG digest, missing `BV0` record.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/conformance-challenge.md:28:The cited 36-cell Vue seed count is grounded in BF2's committed golden fixture
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/conformance-challenge.md:31:still pinned to `5.56.3`; the BF2 goldens contain 6 Svelte client + 6 Svelte server
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/conformance-challenge.md:34:substantively accurate; BV0 routes acceptance through the existing BF2 comparator and
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/conformance-challenge.md:51:  don't guard" governs BV0 and work outside BF3's already-ratified retained scope,
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:10:[`../charters/BV0.md`](../charters/BV0.md).
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:14:For Vue VDOM, Vapor, and SSR findings produced by BF2/BF3 conformance probes,
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:26:render-caching / `CACHED` patch-flag optimization. The full 36-cell BF2 Vue seed
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:27:matrix contains additional genuine defects beyond those two. BV0 must correct the
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:37:B1 -> BF1 -> BF2 -> {BV0, BF3}
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:38:{BV0, BF3} -> {B2, B3}
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:49:id = "BV0"
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:52:predecessors = ["BF2"]
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:58:predecessors = ["BV0", "BF3"]
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:61:All tracked program-state shapes and validators add the same `BV0` identity. BV0 and
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:66:## 3. BV0 charter
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:68:On ratification, the full [`BV0` charter](../charters/BV0.md) is ratified verbatim.
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:69:BV0 immediately corrects the genuine Vue VDOM, Vapor, SSR, assembly, and mapping
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:70:defects exposed by the exact 36-cell BF2 Vue seed matrix while preserving every
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:80:tracking scope and assigned to BV0 correction. BF3 retains the original procedure
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:83:BF3 must probe BF2's exact `svelte@5.56.8` client cells. Results against
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:89:product/route inventory are exhausted. B2/B3 additionally wait for BV0 acceptance.
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:93:Required exits with the exhausted-inventory and BV0-acceptance waits. Its whole-cell
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:99:additionally prove that every BV0 correction survives the final B2–B4 substrate and
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:100:that the exact BV0 seed pack remains green. BV1 may replace a BV0 implementation only
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:107:The Vue candidate-production commit may be carried into BV0 only as part of a
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:131:> authorize BV0 as the immediate correction owner for the exact BF2 Vue seed
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:133:> amend the DAG so B2 and B3 require both BV0 and BF3; require BV1 to preserve
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:134:> every BV0 correction on the final substrate; and authorize no B2/B3 dispatch
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:135:> until both BV0 and BF3 are accepted.
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:152:BV0's Vue findings and any future findings outside BF3's already-ratified
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:167:> BV0 as the immediate correction owner for the exact BF2 Vue seed domain;
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:170:> retained domain); amend the DAG so B2 and B3 require both BV0 and BF3;
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:171:> require BV1 to preserve every BV0 correction on the final substrate; and
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:172:> authorize no B2/B3 dispatch until both BV0 and BF3 are accepted.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:1:# BV0 — Architecture Review
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:5:**Charter:** `docs/arch/refactor/rev11/charters/BV0.md`
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:36:| A known-divergence allowlist | **VIOLATED in effect** — see Finding 1. The file was not *created* here, but BV0 added two new waiver rows for cells that previously passed. |
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:38:| The complete BV1 official-case pack | **Clean** | The new matrix covers exactly the three BF2 seed fixtures × 3 backends × sourceMap × isProd = 36 cells. No broader official case pack imported. |
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:57:`grep -niE 'phase [0-9]|post-cutover|pre-Phase|d-cutover|cutover|deleted in |retired in |rev11|BV0|BF2'` over `crates/verter_compiler/src`, `crates/verter_vue_conformance/tests`, and the harness `src`/`bin`: **zero hits**. New module docs describe the invariant (static shells / reactive boundaries / insertion-site contract), not the plan that produced them.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:96:- BV0's **required procedure** ends with "prove unaffected routes retain their prior successful result contract." Two routes that previously produced an in-contract-clean module no longer do.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:97:- BV0's **owned scope** states BV0 "does not introduce … a known-divergence allowlist." BV0 did not create the file, but adding new waiver rows to absorb a defect BV0 itself introduced is functionally the prohibited mechanism, and it converts a red signal into a green one.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:111:BV0's objective and owned scope are Vue-only ("Correct the genuine **Vue** VDOM, Vapor, SSR, assembly, and mapping defects…"; all four owned-scope items name Vue constructs). Nothing in the charter authorises a Svelte dependency migration. The immediate predecessor commit `fdb6f6291` — *"docs(arch): split immediate Vue defect correction from the Svelte-focused safety retraction"* — is an explicit decision that Svelte work belongs to a **different** block; landing it here reverses that split in the very next commit.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:117:Required: split the Svelte oracle migration into its own commit/block under its own charter, or record a maintainer ruling widening BV0's scope. (No defect was found *in* the Svelte changes themselves — they are mechanical and internally consistent. The finding is scope, not correctness.)
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:121:BV0's required exits state: *"The isolated oracle install is present so link checks genuinely execute."*
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:139:- BV0's owned scope item 3 names *"source-map differences **after harness artifacts are removed**"* — i.e. the charter's expectation is *strip the harness artifact, then compare and fix the residue*, not *abandon the comparison*. The commit identifies the harness artifact (`reAnchorMapLines`) but does not normalise it out and resume comparing.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:146:BV0's required exits state *"No Vue tracking, backlog, waiver, or retraction artifact remains."* Three survive:
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:148:1. `corpus/known-divergences.json` — the seed-corpus parity backlog (84 rows). `official_seed_matrix.rs` correctly refuses to reuse it for its own domain and says so in its header, so BV0's own gate is clean; the artifact belongs to the separately-ratified seed corpus.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:149:2. `docs/arch/future/vue-vdom-parity-backlog.md` D6 — referenced from a **production** comment in `script/process.rs`, and that comment was *edited* by this commit (rc.1 → rc.3, scope narrowed to companion imports). This is the one instance BV0 itself touched and left standing.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:152:Reading the exit criterion literally would require all three gone; reading it as scoped to artifacts BV0 would itself produce leaves all three legitimate. Flagged for the maintainer to confirm the intended reading rather than treated as a violation.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:160:- `script-setup/props-type-withdefaults|vdom`: before `["[structure] Program — child count: verter 6 vs golden 5"]`; after, that top-level mismatch is **gone** (BV0 removed the extra host-assembly statement) and the comparator now descends into the setup body, surfacing 9 previously-masked `private binding key` scope-ordinal diffs plus one real `ArrowFunctionExpression` vs `LogicalExpression` node-kind diff.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:181:1. Two previously-passing Vue VDOM cells (`components/dynamic-multi-root|vdom|non-inline`, `elements-text/multi-root|vdom|non-inline`) regressed to a module-item ordering divergence and were absorbed as **new** `known-divergences.json` rows rather than fixed. Both goldens are byte-identical between rc.1 and rc.3 and the comparator is unchanged, so this is Verter-side. Violates BV0's required procedure ("prove unaffected routes retain their prior successful result contract") and leans on the allowlist mechanism the charter's owned scope bars BV0 from introducing. Fix the lowering order and delete both rows, or obtain a recorded maintainer `DEFER` with a debt row.
docs/arch/refactor/rev11/evidence/BF1/BF1-summary.md:24:- Architecture: PASS. DAG sequencing `B1 -> BF1 -> BF2 -> BF3 -> {B2, B3}` confirmed
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/governance-challenge.md:27:placeholders and is byte-faithful; the `BV0` template row and amended 57-block DAG
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/governance-challenge.md:40:  block identifiers (literal tokens such as "AMD-006", "BV0", "BF3", "rev11",
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/architecture-challenge.md:21:   ledger has no `BV0` row and the old DAG digest, and AMD-005's package validator
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/architecture-challenge.md:29:Commit/tree identity; the DAG diff changes only `BV0` and `B2`/`B3` predecessors as
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/architecture-challenge.md:30:specified; `BV0.md` and the `BV1.md` preservation edit conform to the ratified text;
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/architecture-challenge.md:54:  (forward-only) governs BV0's Vue findings, findings outside BF3's already-ratified
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:1:# BV0 — Adversarial review (candidate `c40a1ca96`, base `b64358705`)
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:3:Charter: `docs/arch/refactor/rev11/charters/BV0.md`
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:80:instance`). Official rc.3's `ssrRender` reads `$setup.count`, and BV0 correspondingly deleted
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:171:  marker, and BV0 removed `_ctx.*` routing in the same change. Coherent.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:183:`3.6.0-rc.1 → rc.3` oracle re-pin (also in this commit) from BV0's code, I ran the corpus with the
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:192:The re-pin costs +15. **BV0's code costs +318 (+85%).** 48 cells worse, 17 better, 17 unchanged;
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:274:**3. An 85% growth in tracked Vue conformance divergences, attributable to BV0's code and
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:303:3. Undisclosed +85% growth in tracked Vue conformance divergences attributable to BV0's code (376 → 694 with the oracle re-pin factored out), concentrated in Vapor and semantic in nature (missing `withModifiers`/`withVaporModifiers`, interpolation folded into static template markup), including 2 previously-passing cells.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/deviation-memo.md:5:BF3's charter assumed every BF2-probed successful cell that proves wrong should be
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/deviation-memo.md:14:BF3's probe against BF2's exact `vuejs/core v3.6.0-rc.3` seed manifest found genuine
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/deviation-memo.md:40:  (`scripts/svelte-golden-lib.mjs:32`), not BF2's authoritative `svelte@5.56.8`
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/deviation-memo.md:42:  (`packages/framework-conformance-harness/src/domain-pin.mjs:66-76`). The 12 BF2
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/deviation-memo.md:55:  BV0 correction.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/deviation-memo.md:56:- The DAG's single `BF3 -> {B2, B3}` edge is widened so both `BV0` and `BF3` are
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/deviation-memo.md:68:AMD-006 adds BV0 as an immediate, bounded Vue-correction block between BF2 and
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/deviation-memo.md:70:`svelte@5.56.8` domain; requires BV1 to preserve every BV0 correction on the final
docs/arch/refactor/rev11/evidence/BF1/context-packet.md:40:discovery), architecture PASS (DAG sequencing `B1->BF1->BF2->BF3->{B2,B3}` confirmed
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/conformance-challenge.md:133:Vue `.each` expansion and Svelte per-sample profile expansion remain BF2 work, as the
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/conformance-challenge.md:218:runes/legacy/server, route-overhead, and project-staging cells before a BF2/BV1/BS1
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/conformance-challenge.md:223:policy to be frozen before BF2 starts; `performance-impact.md:3-7` prohibits choosing
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/conformance-challenge.md:224:or changing thresholds after successor observation. The DAG keeps BF2 locked behind
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/conformance-challenge.md:228:`validation.md:56-77`, AMD-005 lines 211-215, and BF2's exits all require runner
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:38:EM-037	crates/verter_compiler/src/sourcemap_e2e_tests.rs and framework_common/sourcemap_e2e_helpers.rs	current mapping validation harness	Preserve	BF2+B4	Preserve useful discriminating checks, rebind to exact source spaces and official cases; never acceptance alone.
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation2.md:75:The candidate-to-fix diff does not change `program-dag.toml`; its BF1/BF2/BF3/B2/B3,
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:3:# BF2 reopen re-review #2 — adversarial (perf/memory/stub-hunting)
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:7:Method: criterion-by-criterion against `docs/arch/refactor/rev11/charters/BF2.md`, plus targeted mutation-proof stub-hunting.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:46:Evidence: `test/hydration.spec.mjs` — 4 tests green, both `hydrateVue` and `hydrateSvelteClient` driven against real official-compiled artifacts (pairing #1), each with a positive + negative-control arm. Mutation-proven in §3 (disabled `app.mount(container)` in `hydrateVue` → negative-control test failed with `expected true to be false`; reverted, clean). Pairings #2/#3 are explicitly and honestly out of scope per README ("BV1/BS1 downstream of BF2") — charter's "meaningful cross-pairings" plural is satisfied only for the one pairing that can exist today; the README does not overclaim the other two. **PASS — mutation-proven for pairing #1; scope-honest for #2/#3.**
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:52:Evidence: `test/coverage.spec.mjs` — 5 tests, ran twice (with and without checkouts). With `BF2_VUE_SOURCE`/`BF2_SVELTE_SOURCE` set to real pinned clones (see §2), all 5 pass for real, not skipped:
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:67:> "BF2 cannot change production compiler behavior, implement a runtime, patch generated output, inject helpers, mock missing exports, use a forbidden corpus, or let candidate output update expectations."
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:71:- Diff touches only: `docs/arch/refactor/rev11/evidence/BF2/**`, `docs/arch/refactor/rev11/evidence/framework-conformance/performance-impact.md`, `packages/framework-conformance-harness/**`, `performance-gates.toml` — exactly the allowed set named in the task brief. Verified via `git show --stat a7f1eb5d7`. **PASS.**
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:85:Evidence: `docs/arch/refactor/rev11/evidence/BF2/manifest-classification-accounting.md` — full read. Honestly states 5316 of 5460 rows (2003 Vue + 3313 Svelte) stay `disposition: blocked`, explicitly attributed to needing a real Verter candidate that does not exist yet (BV1/BS1/B2/B3 are downstream of BF2 per `program-dag.toml`, quoted directly in the doc), and explicitly disclaims any unilateral scope-widening. The 144 Svelte `not_applicable` rows are correctly attributed as pre-existing BF1 classification, re-verified (not newly classified) by this pass. This is a `blocked`-disposition (one of the five closed dispositions `VALID_DISPOSITIONS` in `src/coverage-report.mjs`), which the charter text treats as a legitimate terminal state alongside `imported`/`equivalent`/`not_applicable`/`unsupported_fail_closed` — not a "no disposition" gap. **PASS — honestly scoped, not fabricated.**
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:88:Not touched by this diff's `performance-gates.toml` changes except to explicitly OPEN (not lock) the one BF2-owned cell — see §1.5. No evidence BF1's own locked cells were touched or broken; `git show a7f1eb5d7 -- performance-gates.toml` diff is scoped to the one row plus surrounding comment restructuring. **PASS** (out of this diff's blast radius).
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:92:Ran: `git show a7f1eb5d7:performance-gates.toml | grep -A2 'id = "BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE"'` → **empty** (the id only appears in prose comments, e.g. lines 428–433 and 710+, never inside an active `[[cell]]` table). Confirmed by direct read of lines 705–720: the cell is headed `# BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE — OPEN / NOT YET LOCKED.` as a comment block, not a `[[cell]]`. **PASS.**
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:94:`docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md` — read in full. Names:
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:96:- Resolution gate: "Before that owner's own performance-lock exit is accepted, it must freeze [the cell]... through a genuinely independent measurement... It may NOT reuse BF2's invalidated 10-run session."
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:97:- Acceptance ID: `FC-PERF-001`, explicitly stated "Not satisfied by BF2."
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:112:BF2_VUE_SOURCE=/tmp/bf2-oracles/vue-core BF2_SVELTE_SOURCE=/tmp/bf2-oracles/svelte \
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:156:Run: `BF2_VUE_SOURCE=/tmp/bf2-oracles/vue-core BF2_SVELTE_SOURCE=/tmp/bf2-oracles/svelte pnpm exec vitest run --root . test/coverage.spec.mjs --reporter=verbose`
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:162:**Why this is blocking, not just a nit:** the commit message and `manifest-classification-accounting.md` both explicitly claim re-enumeration was "strengthened... to a git-hash content check plus closed-set field validation" as one of the concrete deliverables closing the reopen. The content-hash branch is real, live code that runs in production `coverage-report.mjs` usage — it is not a stub in the CLAUDE.md sense of an empty/constant body. But the self-test suite that is supposed to prove `FC-MANIFEST-001`'s re-enumeration exit contains **no case that would ever exercise or fail on this branch**: the only negative control (`BF2-SELFTEST-BOGUS`) hits `path-not-tracked` before the hash comparison is ever reached. A regression here (e.g. accidentally deleting the elif, or a future refactor that silently drops the check) would go completely undetected by the gate, letting a row whose real content silently drifted from the pinned tree still count as `resolvable`. This is precisely the class of defect the reopen was convened to close (a claimed-strengthened guarantee with zero discriminating proof) — narrower in severity than the original "zero callers" finding since the mechanism does run in real usage, but it is a genuine gap in the required self-test coverage.
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/run-session.sh:2:# BF2 performance-lock measurement session for
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/run-session.sh:3:# BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE. Runs the frozen,
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-architecture.md:3:# BF2 Revision-11 conformance harness — architecture review, round 7
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-architecture.md:28:| BF2 charter SHA-256 | `1f99cf7eda1a955ada751f075799dabc8c8ab1defda19b20375f7ca09aa5963b` | exact |
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-architecture.md:44:- BF2 objective: “Build hermetic test-only infrastructure that can falsify framework output against the exact official domains without supplying production behavior.”
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-architecture.md:45:- BF2 required exit: harness self-tests must “prove source/package drift refusal”.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-architecture.md:107:  '\nconsole.log("BF2_R7_MEMO_POISON_EVALUATED");\n',
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:115:removed sections, and exactly `BF1`, `BF2`, `BF3`, `BV1`, and `BS1` added. Outside the
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:132:- added: `B1→BF1`, `BF1→BF2`, `BF2→BF3`, `BF3→B2`, `BF3→B3`,
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:141:| BF2 | BF1 |
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:142:| BF3 | BF2 |
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:6:BF2's existing 10-run session and every timing/RSS threshold derived from it are
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:9:The BF2 implementation may be the measured subject if gate authority is separated
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:11:fresh measurement, and a disjoint post-freeze holdout session supplies the BF2
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:16:- It is not (a), because an absolute limit such as `2x measured BF2 time` still
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:25:  during the BF2 reopen before BF2 is re-reviewed and accepted.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:28:open/deferred state and BF2 must not claim that its performance exit passed.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:34:required exit 6 says the complete cell must be frozen before BF2 work, and the
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:35:abort rule names a criterion selected after candidate measurement. BF2, in turn,
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:38:visible and deliberately open; it did not authorize BF2 to fill the numbers from
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:49:relative comparison. The initial BF2 performance decision must therefore rest on
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:53:as independent evidence that BF2 was fast enough in the first place.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:79:After all BF2 functional fixes are complete, the orchestrator records one exact
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:90:correctness oracle, or candidate tree invalidates the resulting BF2 performance
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:95:The maintainer appoints a performance-gate reviewer who did not implement BF2, did
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:96:not author or review the invalid BF2 performance session, and has not inspected its
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:115:   operational product/CI budgets and not as a multiple or margin over BF2;
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:136:the BF2 implementer must not propose or fill them. A rule based only on BF2's
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:143:registered invocations on the locked runner. The BF2 implementer does not run or
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:155:no-regression comparisons. It is not, by itself, BF2's pass evidence. In the lock
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:156:record, the first-implementation disposition must say explicitly that BF2 admission
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:164:calibration-baseline checkout and the exact BF2 acceptance candidate in the locked
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:172:mechanically frozen relative noise limits. A valid failure blocks BF2; it cannot
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:177:### 7. Re-review BF2 on one exact candidate
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:181:digest-addressed BF2 evidence. BF2 may then enter fresh conformance, architecture,
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:189:BF2 cell values. If the maintainer does not ratify the protocol, the only compliant
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:190:fallback is to keep `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` explicitly
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:191:open/deferred and withhold BF2 acceptance. Ratification must not be construed as
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:192:acceptance of BF2's other reopened findings or as authority to expose BF3; those
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/README.md:1:# BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE measurement session
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/README.md:4:`BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE`, freezing the row that
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/README.md:5:`performance-impact.md` left deliberately open pending BF2's own harness
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/README.md:6:existing. This measures BF2's actual test-execution-harness golden generator
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/README.md:10:`BF2_VUE_ORACLE_MANIFEST_GENERATE` / `BF2_SVELTE_ORACLE_MANIFEST_GENERATE`
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/README.md:74:sibling BF2 cells' methodology, not because the workload is expensive per
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/README.md:111:handful of times per BF2 harness-evidence cycle, not on any hot or
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge.md:102:  and 56 candidate blocks. Exactly `BF1`, `BF2`, `BF3`, `BV1`, and `BS1` were added.
docs/arch/refactor/rev11/evidence/BF2/evidence-summary.md:1:# BF2 evidence summary
docs/arch/refactor/rev11/evidence/BF2/evidence-summary.md:27:     BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE performance-gate evidence — confirms the
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-conformance.md:3:# BF2 conformance review — round 7 (targeted: rows 1 and 4 + no-regression)
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-conformance.md:42:| Charter SHA-256 | `1f99cf7eda1a955ada751f075799dabc8c8ab1defda19b20375f7ca09aa5963b` | `1f99cf7e…5963b docs/arch/refactor/rev11/charters/BF2.md` | ✔ |
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-conformance.md:106:- BF2 charter, exit criteria: "`FC-HARNESS-001` … pass. Harness self-tests prove
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-conformance.md:119:**Round-6 finding, verbatim** (`BF2-REVIEW6-ARCHITECTURE.md` §Executive result 1
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-conformance.md:190:`BF2_R7_CONF_MEMO_POISON_EXECUTED` never appears in the output — the poisoned
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-conformance.md:191:compiler did not run. Round 6's `BF2_R6_MEMO_BYPASS_EXECUTED` did.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-conformance.md:304:**Round-6 finding, verbatim** (`BF2-REVIEW6-ADVERSARIAL.md` §3, A6-1):
docs/arch/refactor/rev11/evidence/framework-conformance/validate-package.mjs:228:  BF2: ["BF1"],
docs/arch/refactor/rev11/evidence/framework-conformance/validate-package.mjs:229:  BF3: ["BF2"],
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:2:VUE-9C3FB0B7F7249D90FF4D	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:111:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	54a6faa431597486c8144789dc9e873c8da3a1ddf472c64113a46c55e9a3c3ae	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:3:VUE-95F6A0F19976C7BD8B78	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:141:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	b42b0f6fdb430a0d2143758047ace28836463cf7b13cf31e9d791ac63061da89	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:4:VUE-DE04C6BD47A2A4C89D1A	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:162:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	b5a2a1f05494d60d19fb6089200b6fd1dcfc8476395de1932fae49fa69e9fde3	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:5:VUE-F688F8AC8E2CD556FE9A	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:171:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	9b3e12c41c70b5958f599a2b223e962545d50089577ee43688c13c9a9c4eeb7d	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:6:VUE-26927CC4391937A1815A	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:185:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	ed707c99b8a6418c6e2c9c4a5ba8d62957ea84c1910377c49400f732edeb8db2	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:7:VUE-CA94B6C21BC960F04BB2	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:195:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	61137a740d3149b9c8fe3d58920f9e0f3b0a30ca7b1b8016e46870d8a19d1c76	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:8:VUE-652CD41590875B2CB148	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:209:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	169f84ea8e150bc7bbb7555a8cddb7222a13269dc3eb5f5c604f62c78698f267	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:9:VUE-5DAD0600D2A89E7B25BA	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:232:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	82d56023754c9f028b50f9ef40445090e4e8317c6851e3097275559613cf84ff	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:10:VUE-4FC08D39A342B80A1C45	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:251:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	9074893e9b7317cbcf83849b7a57346b2645b927f0e327f53849a4f510f26048	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:11:VUE-79D6E6B9CF9796C2FAD4	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:282:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	7a45e914504e46a5e0143b2d2cc73384dde4011fcd97cd44da78ca6f61ecf665	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:12:VUE-B486807792F8F5BD9861	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:318:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	329da5c011eac9bb2f02d6a7d76274cdfd5f36ac271dde043ad2b9ccc70636ad	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:13:VUE-C6026DD8E610575FF69B	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:382:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	23c6b9b03fd34c758c5054a340448947f243af02a87d7f1c4aff2d1681591f30	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:14:VUE-15D4FEF87B10B3CDB387	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:398:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	ed383199c07b42dd1791bca81393d12eb2e02b00511bb8484de2fd5c58a66447	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:15:VUE-8FF26A5656DD9587A1DA	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:422:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	1624018ee58c41375ec39233b1d428242096f3b3cf7f4a38a7b6c540ca31ddf8	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:16:VUE-E9BF2E90B4D0E13E2F4D	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:440:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	a7f2c9f608334b54ace5c6ab37485df6afdc27b8a471af16af22b620f902422e	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:17:VUE-06C16B6390C266D6D3DE	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:468:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	9a7f21ca77f58024362f10b0fb9e2843eb73dd451dc8df4a0141209446a5c4f0	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:18:VUE-88E06526002F2461275E	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:490:5	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	8daa5753e30416a8d11255df2ac6e300742258a22ab800c2c9163a7e5b27a7f2	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:19:VUE-96B99CE1CC00E6539327	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:512:5	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	abe8c79c887aef540661213de77f6e1bb05eb53bebdd00bab4d755301940f2a7	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:20:VUE-4FA76884D12A9C18C6CA	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:537:5	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	13ead04db558426b694a6b346350d056b416e7ddd053bf0db0fd8657510c31a7	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:21:VUE-1F44E20B91176D8D40C5	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:565:5	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	5dae489181ad5bb7847174ed02684e019c497f2b6d67f85d4ab8347e7d39e227	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:22:VUE-24F9B3AB58423502C5EC	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:59:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	78d705de092327ed82df340038353f51911a1daf588b2020a4a99a376b20784b	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:23:VUE-B81DAC2DA50DF3AFAF0B	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:597:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	ba037a4f5ff1847d8b43bd7d50b8784451d80d79cc1b8cce4ed2de1a6e626fba	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:24:VUE-141659842B2D4A8E370C	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:633:5	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	08a21f57d2ca3e0d51e0b33bd499ffd2f58df2ee9b13d87a7c0176760e56f0f4	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:25:VUE-285602D736B47B892DBA	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:644:5	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	67f275310240a5d54618e39663b69abec90f46b0437a5377c3be941109da2968	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:26:VUE-8DD48781A7F18EF34F84	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:652:5	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	27e9918551008f30e4ce0b7bd20134e88715d290a058b5f4eb0f13898cbb3560	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:27:VUE-655672005574793DBB0C	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:660:5	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	06cc1edf634888c510545820b06c85bccf4693ec1e06e019f306233b31ee3087	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:28:VUE-C35C0D057D3DE5AB1B31	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:668:5	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	1b10939e24e41eb4993e03ae34940c9480ae07311d86910c24232c7b4d87f890	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:29:VUE-3D152AD3F78E95AC0E17	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:679:5	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	dc26c7f00d3e021689f224505bb0cde51028b8f8f7cea48c8c0b992b6cd9edea	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-

exec
/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md | sed -n '1,190p'" in <repo-root>
 succeeded in 0ms:
     1	# AMD-006 — Vue known-defect correction
     2	
     3	**Status:** RATIFIED (see §8.1). Landed at `fdb6f6291`.
     4	**Prepared against:** local `program/architecture-lock` commit
     5	`2493c0056b55e58f28f8df89756bd3a3ffbeed4e`, tree
     6	`3b4a8634856ab675d81e38cb46dca89e01fe32df`.
     7	**Amends on ratification:** [`../program-dag.toml`](../program-dag.toml), the live
     8	program ledger, and the [`BF3.md`](../charters/BF3.md) and
     9	[`BV1.md`](../charters/BV1.md) charters; introduces
    10	[`../charters/BV0.md`](../charters/BV0.md).
    11	
    12	## 1. Binding maintainer direction and product boundary
    13	
    14	For Vue VDOM, Vapor, and SSR findings produced by BF2/BF3 conformance probes,
    15	production compilation continues returning success and generated Vue output. BF3
    16	must not add a typed non-success, publication guard, artifact-withholding path,
    17	known-divergence allowlist, temporary tracker, or backlog mechanism for those
    18	findings.
    19	
    20	Confirmed defects are corrected in the compiler. This direction supersedes the BF3
    21	worktree disposition recorded as "track, don't retract"; that record must not land
    22	and creates no ongoing program authority.
    23	
    24	The first two named, bounded corrections are the missing `__expose()` emission in
    25	every non-inline `<script setup>` cell and the dropped VDOM `<slot>` fallback
    26	render-caching / `CACHED` patch-flag optimization. The full 36-cell BF2 Vue seed
    27	matrix contains additional genuine defects beyond those two. BV0 must correct the
    28	named defects first and then close the complete bounded seed domain under its
    29	charter; this amendment corrects, rather than suspends, the conformance-golden rule
    30	that an enabled successful cell carries no semantic known-divergence.
    31	
    32	## 2. Amended DAG
    33	
    34	The amended region is:
    35	
    36	```text
    37	B1 -> BF1 -> BF2 -> {BV0, BF3}
    38	{BV0, BF3} -> {B2, B3}
    39	{B2, B3} -> B4
    40	B4 -> {BV1, BS1}
    41	{BV1, BS1} -> B5 -> B6
    42	```
    43	
    44	This same commit adds the following machine-readable row to
    45	[`../program-dag.toml`](../program-dag.toml):
    46	
    47	```toml
    48	[[block]]
    49	id = "BV0"
    50	name = "Immediate Vue known-defect correction"
    51	class = "subsystem"
    52	predecessors = ["BF2"]
    53	```
    54	
    55	The B2 and B3 predecessor rows both become:
    56	
    57	```toml
    58	predecessors = ["BV0", "BF3"]
    59	```
    60	
    61	All tracked program-state shapes and validators add the same `BV0` identity. BV0 and
    62	BF3 may overlap only after an exact writable-ownership proof demonstrates disjoint
    63	Vue and Svelte production files, tests, generated artifacts, manifests, and
    64	lockfiles. Without that proof they serialize.
    65	
    66	## 3. BV0 charter
    67	
    68	On ratification, the full [`BV0` charter](../charters/BV0.md) is ratified verbatim.
    69	BV0 immediately corrects the genuine Vue VDOM, Vapor, SSR, assembly, and mapping
    70	defects exposed by the exact 36-cell BF2 Vue seed matrix while preserving every
    71	public route's successful result contract. It owns source-root-cause corrections and
    72	independent controls within that bounded domain, but must stop with
    73	`RESCOPE_REQUIRED` rather than introduce B3/B4 authority, change a ratified public
    74	contract, or substitute any guard, tracker, waiver, fixture-specific branch, or
    75	silent deferral.
    76	
    77	## 4. BF3 charter amendment
    78	
    79	BF3's Vue VDOM/Vapor/SSR runtime-render rows are removed from its retraction and
    80	tracking scope and assigned to BV0 correction. BF3 retains the original procedure
    81	for in-scope Svelte and non-Vue-runtime reachable-success cells.
    82	
    83	BF3 must probe BF2's exact `svelte@5.56.8` client cells. Results against
    84	`svelte@5.56.3` do not satisfy that exit. Svelte server's existing typed
    85	`ServerGenerate` refusal is recorded as an already non-successful cell and receives
    86	no new production mechanism.
    87	
    88	BF3 cannot accept until the exact Svelte client inventory and remaining in-scope
    89	product/route inventory are exhausted. B2/B3 additionally wait for BV0 acceptance.
    90	The corresponding [`BF3.md`](../charters/BF3.md) edit narrows its Objective, Required
    91	procedure per successful cell, and owned scope away from Vue VDOM/Vapor/SSR; adds the
    92	exact-version client probe and existing server-refusal requirements; and amends its
    93	Required exits with the exhausted-inventory and BV0-acceptance waits. Its whole-cell
    94	retraction mechanics remain intact for the domain it still owns.
    95	
    96	## 5. BV1 preservation amendment
    97	
    98	BV1 remains after B4 and retains its complete existing charter. Its required exits
    99	additionally prove that every BV0 correction survives the final B2–B4 substrate and
   100	that the exact BV0 seed pack remains green. BV1 may replace a BV0 implementation only
   101	with an accepted equivalent correction; it may not reintroduce a corrected defect or
   102	convert one into a refusal or tracked divergence. This preservation requirement is
   103	materialized in [`BV1.md`](../charters/BV1.md).
   104	
   105	## 6. Worktree disposition
   106	
   107	The Vue candidate-production commit may be carried into BV0 only as part of a
   108	non-vacuous conformance gate. The "track, don't retract" deviation record is
   109	superseded and excluded from landing. No replacement tracking artifact is created.
   110	That superseded record lived in an isolated implementation worktree, which is
   111	excluded from this package and from landing.
   112	
   113	## 7. Deviation memo
   114	
   115	The failed assumption, measured evidence, affected invariants, consequences, and
   116	recommended amendment backing this package are recorded in the
   117	[`Vue known-defect correction deviation memo`](../evidence/vue-known-defect-correction/deviation-memo.md).
   118	That record applies the repository's architecture-deviation format and binds the
   119	scope change to correction rather than retraction, tracking, or silent divergence.
   120	
   121	## 8. Exact ratification action
   122	
   123	After the amendment package, new charter, DAG, state-shape updates, and independent
   124	architecture/conformance/governance reviews bind one exact candidate commit and tree,
   125	the designated maintainer records:
   126	
   127	> Ratify AMD-006 for reviewed package commit `<reviewed-full-sha>`, tree
   128	> `<reviewed-tree-oid>`, and ratification-bundle commit `<bundle-full-sha>`,
   129	> tree `<bundle-tree-oid>`; confirm that Vue VDOM/Vapor/SSR production
   130	> compilation remains successful with no BF3 retraction or temporary tracking;
   131	> authorize BV0 as the immediate correction owner for the exact BF2 Vue seed
   132	> domain; narrow BF3 to its remaining Svelte and non-Vue-runtime inventory;
   133	> amend the DAG so B2 and B3 require both BV0 and BF3; require BV1 to preserve
   134	> every BV0 correction on the final substrate; and authorize no B2/B3 dispatch
   135	> until both BV0 and BF3 are accepted.
   136	
   137	On ratification this amendment supersedes only the conflicting BF3 Vue-retraction
   138	scope in AMD-005 §5 and §12 and the BF3 "track, don't retract" worktree disposition.
   139	It does not touch AMD-005's compatibility-domain locks, oracle/exclusion rules,
   140	capability matrix, or performance-lock process; all remain in force unchanged.
   141	
   142	### 8.1 Recorded ratification
   143	
   144	The architecture, conformance, and governance reports at
   145	[`../evidence/vue-known-defect-correction/reviews/`](../evidence/vue-known-defect-correction/reviews/)
   146	each closed `BLOCKING_FINDINGS` (round 1) against reviewed candidate
   147	`80be3b5302f6089f24313ee954afbc2432674069`, tree
   148	`af1625731bb3efe24c831baba98f6f6299e96e2e`, on one shared finding: whether the
   149	general "fix, don't guard" project rule retroactively supersedes BF3's
   150	AMD-005-ratified Svelte-domain safety-retraction mechanism. A Codex Sol xhigh
   151	architecture ruling resolved this RETROACTIVE-NO-FORWARD-ONLY: the rule governs
   152	BV0's Vue findings and any future findings outside BF3's already-ratified
   153	retained inventory; it does not repeal BF3's existing, already-ratified
   154	typed-non-success/whole-cell-retraction/removal-owner mechanism for its
   155	retained Svelte and non-Vue-runtime scope. No package content change was
   156	required. Round 2 reattestation on the unchanged candidate closed all three
   157	mandates `PASS`: `architecture-challenge-reattestation.md`,
   158	`conformance-challenge-reattestation.md`, `governance-challenge-reattestation.md`.
   159	
   160	> Ratify AMD-006 for reviewed package commit
   161	> `fdb6f62911fffc43c7f64610033bdf2e8a591fbf`, tree
   162	> `715b3c620a313a37a4289fc05eacf0924d421a5a` (reviewed package and
   163	> ratification bundle are the same commit — reviews were committed alongside
   164	> the package in one landing, per program branch hygiene), and the listed
   165	> package digests; confirm that Vue VDOM/Vapor/SSR production compilation
   166	> remains successful with no BF3 retraction or temporary tracking; authorize
   167	> BV0 as the immediate correction owner for the exact BF2 Vue seed domain;
   168	> narrow BF3 to its remaining Svelte and non-Vue-runtime inventory (its
   169	> existing retraction mechanism is UNCHANGED and stays in force for that
   170	> retained domain); amend the DAG so B2 and B3 require both BV0 and BF3;
   171	> require BV1 to preserve every BV0 correction on the final substrate; and
   172	> authorize no B2/B3 dispatch until both BV0 and BF3 are accepted.
   173	
   174	Any challenged or changed byte requires regenerated identities and fresh
   175	reports. Silence, merge, or this proposal's commit is not ratification. The
   176	preparer cannot ratify, review, or satisfy any independent mandate.

exec
/bin/zsh -lc 'rg -n -S "compileScript|inlineTemplate|templateOptions|vapor|ssr" docs/arch/refactor/rev11/evidence/framework-conformance docs/arch/refactor/rev11/contracts | head -n 400' in <repo-root>
 succeeded in 0ms:
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/conformance-challenge.md:121:| Vue | 2,003 | compiler-core 570; compiler-dom 137; compiler-sfc 509; compiler-ssr 134; compiler-vapor 653 |
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/conformance-challenge.md:194:| 9. server/SSR behavior | `ssr-hydration.md:5-13` |
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/conformance-challenge.md:195:| 10. hydration behavior | `ssr-hydration.md:15-35` |
docs/arch/refactor/rev11/evidence/framework-conformance/validate-package.mjs:161:  "@vue/compiler-ssr",
docs/arch/refactor/rev11/evidence/framework-conformance/validate-package.mjs:162:  "@vue/compiler-vapor",
docs/arch/refactor/rev11/evidence/framework-conformance/validate-package.mjs:165:  "@vue/runtime-vapor",
docs/arch/refactor/rev11/evidence/framework-conformance/package-checklist.md:21:| SSR/hydration | `contracts/ssr-hydration.md` |
docs/arch/refactor/rev11/contracts/capability-matrix.md:36:  spans three backend trees (`vdom`, `vapor`, `vdom-inline`); the
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:709:VUE-5C9C182A078C9DED5098	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1009:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	3073c829f00651a24c149466164fb12750c6add3da6b3557536248260815e6bb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:710:VUE-7D08646FABB5DA65426C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1029:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	af981d00f9f06b9bff25e5fa4c4b0f573eb40c8e357e11e2f9609d3c6e33ef91	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:711:VUE-9E778CC0562DB658E43B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1042:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	350416ce519db19c274645fbca4003b50cb5e865e9f6ed033b9e5a50b71b027d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:712:VUE-8215F16D78101CD43468	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1053:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	29a9c955dea408d74c1af1476d724a72e0f718adcab6ea933a8aa3e6f1747770	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:713:VUE-7012FA3963CC587CD4E7	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1073:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	6163679eece7aaa0f94b21f6308a81a6e02301fd5aa134e9d0e79d1c52ffced7	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:714:VUE-A773FFBD2A1707B3E132	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1077:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	b5072a725c43683b188218fa6b214f213e3f8e1a700196c9e24a0ca71da875ef	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:715:VUE-1C9DB9ADF006C9D64150	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1081:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	07aef7fee22d4856461d6d8631b87b6ca5db3cd1df62b35ccb27923e554996ef	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:716:VUE-DEF880897F116D10A225	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1086:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	6e032cfd0aeacd651e626b1c33bddb7f9aab4b139c07c24ecf860d3b8cfa6704	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:717:VUE-5F2E99C4E50A51DABF09	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1093:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	99e4155362fae1ac922ea8ddce3b8fbac264a4d169bccde92e9de0e635a6b82a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:718:VUE-4D3F9BE30B439F1B2748	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1099:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	5e062c3bd5cbd09371474379f2a6ca7071bb02c6cb1b0859235ec57e42da19ee	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:719:VUE-9FA10F4609184B011507	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1104:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	8378162888da4a04f6c43d12a6dd89916e866a5495b3f8b9b01a49381413fb5d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:720:VUE-3CD26A7E95BCC6D0D4BD	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1108:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	fe9ff60eaa8269652514a163883a2c3966298077fad605335720a9bcc0eed22e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:721:VUE-F1E26ED928AF4171F880	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1130:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	a97155611f129f36c4eabcc05e96662b95105e3d8f7f732f99297c33c9c83d80	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:722:VUE-57C399DA66C585234D93	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1149:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	f76cb3239db5b997828b88237062efb008e4b2c73c0e5d5293a0877904ccfb4f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:723:VUE-A889517E090683F5234E	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:115:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	682ce2e7e7698924bfe62304a9f1df1041fed239fc04eb0a111c4d5ad1a48498	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:724:VUE-C3E0C7FC0802DC5DBF04	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1161:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	29898d87fb101c729d26448aa04e57bc44b4db86209e15900363649f6c1ca86c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:725:VUE-ED1CC6BD567AD80F9C19	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1177:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	7b720eb8013c4c9d4157282e8663086967c054502b2dfc91d81ed18a239ca28b	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:726:VUE-ED3DFACCE4156A6F8FD7	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1192:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	81f1802485d3b7d97511abd3f24f09e472644c5f239d2f27f58175322a904de5	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:727:VUE-C4AAD84AEC0E27768F20	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1213:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	8193e0ee30d78fea2d4758e80bee682ce12c773666fee6895d7ed98ec6671dae	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:728:VUE-6A7F6762C474F7C92E3C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1246:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	29b67922c2a1eb807b5f8981cb9b2fbcaa7db4215c2a4e96c81b68dfaa280da6	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:729:VUE-A5178C040994D9F5BAF1	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1262:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	fa504141c7803f1b9e187651b7889c86b76a49d9953992b8aaf611027cbb7d7b	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:730:VUE-9FA612E5574D23DD9822	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1278:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	ce5290f7367fa8b032ca22893855d956437ab3f40ffd667b05b2aae8bd8ab622	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:731:VUE-0C336C2EDA80564B69C3	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1313:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	22fc3cc81d638a028ed48f8fcb657f04ee24d4f7d3a9142aa01cc5e398904049	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:732:VUE-A744885AAC343281A2B5	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1330:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	1db15fd45f7b14f1fda47930c272d2c83b3e8e0362caca1c0ec06ca99ef7285e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:733:VUE-C817B023AE637FE8620E	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1345:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	127e0c770b89e55d2923f5fa04c276e4e2e641d1c76549a95f2923c3c1c8f59c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:734:VUE-EA2ADBBB7203FBA1CECF	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1369:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	9b999e39a257e58e062e5c9bc9bb412a6605eaedf79d527cbaa1a6bb4e1d3698	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:735:VUE-00347BB191A9DDBA45D0	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1390:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	4484e841cbbf550c66ce8717a252cb73721b285a11cd63745074627238f30b9e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:736:VUE-FB6813B67A2C09008DA4	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1404:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	85d1b9ce04357c56e89136327d22ec36702cb7a683539536bf9c2777ccba4d1e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:737:VUE-E1B2EEBF53F894FAFE41	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1425:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	5360522b0234e8dbf0613572116d5b98ba5428f2aa31ecdbe787c5dda159683e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:738:VUE-EFDF7BD39518592C6E0F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:144:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	548817309290dfab4cfda8dcf39b10412a6e112868bd3e79717a643653bd6fdf	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:739:VUE-A411FE3448AB050A1BCF	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1445:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	f88be6e9b220d9cd692ac6da063e139de6f4b8482d71a019034d5c9316b21b8d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:740:VUE-6540C2347C337F439A5F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1458:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	9525c16b7245695419d019d39d9adf1d99954572b44a1d9f9688b1a775595be9	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:741:VUE-181646228F29CBBDDF56	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1478:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	949bb5d2e12e504862f3bd96c1ade87fd911d9d0750d7d7a0c4f5bdaf96b3518	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:742:VUE-55A5DDC403C16CFD2F40	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1492:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	f18be89bc77db099996cffac033cb4900c047a9bebd1246906d7c1406edf8e2d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:743:VUE-10CE047618F600A58DE2	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1509:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	23a1fe092cce7590dce5abb2c70d2ee9a744e2a67451988b40f03106802f0411	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:744:VUE-15BE478B5634DFF6E373	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1546:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	9d5ece5c5e5ef5bbbd39775cd90a4f35d1da67f3c082df7ab228c4a29acbffb5	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:745:VUE-DD3352A34CED48DBC5A2	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1574:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	8630f6bdb0e06a9167722b29c0d0428180b912f248abe561843f8c31877ac1a8	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:746:VUE-893D0074297EB395B074	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1588:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	f2d10c5caef32582e2f87a4c746631b91bb68f6ecb40fdaa350977ba1aafe335	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:747:VUE-B068784F7632E4AF13AB	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1607:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	0fb51b5337a65f4fe75d74207d2638bfd2cc21d648445c9887ca369f6b86fc0b	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:748:VUE-9A25BD40FB3414B4AB5F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:161:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	2042cd044ab458d4dd52a662871bc885007ba7dc799ec71bd8ffbf091c090fb1	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:749:VUE-E3990E59579F3F6200B1	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1630:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	59ff77c2c7093146d38511d35a486c003969c724681dd712f741cfef79f290b4	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:750:VUE-F313B831C8AF4F14F8BF	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1644:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	d270882ae1de19ee59d74848d1a847e17c3bc267f33e7062501e82a12bc9b626	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:751:VUE-40226B595A59D385FFB2	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1662:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	aea2ee2406fc0eea6ffa59dc4ee1f0555164e64bdfeb5fd93df39fc1f9470573	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:752:VUE-34D0BEEEE3285D94F281	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1681:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	aea2ee2406fc0eea6ffa59dc4ee1f0555164e64bdfeb5fd93df39fc1f9470573	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:753:VUE-F16BAF7BEC71A4F8A85B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1700:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	e1d0c21581a300a19db9dc4c924e8f8106b5e2fd473229c9dcb5d3d2a5062281	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:754:VUE-3FB681ADC70C20EE87CF	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1714:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	e5f2862119d3f9fd074639dac5814a1547711f8beb4af3ee0bc699f829eb154f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:755:VUE-2667EE013A9AF3572EED	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1728:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	f5827428c5055d190e58c4d863008b53897503e3003572cd4ad49e03b7881906	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:756:VUE-9331DA60D158E8D68707	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1747:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	8078b4199b7c20d9afb383c268872591db6473d4d963a9c7c7b572a1e0d1e48d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:757:VUE-B39365FD4FB4A44D5499	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:176:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	90b73720394001f43bbdcfb49d540bdeced9fb1d828824b88ed6e41122d73d42	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:758:VUE-F47435E8BE7C31614AF5	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1763:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	6be4264e3d03b0b957f737a443e5a0c28df72fb95ec91f4da1b74dcdcd2f8550	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:759:VUE-FE28498C1A44DB7AEA0F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1779:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	a4795365e5b1017161f956860db87b53d7603fc425682125dbf783e23006f6d6	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:760:VUE-B43F706ECA503081516B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1798:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	a83cfb74d36fcd839e7cfbdcca1bfc37e79b04e39cee3c884bb051f2c91a6455	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:761:VUE-DAE8F4D7257A80144966	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1819:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	742e46c7965b57f021cfd9a49e8fcdf51878b78d8fa2a0261f16f221992935c1	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:762:VUE-B4E025D0E36B776518F2	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:1834:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	b915fc213f716eb5ee7fcbf3c4e9ef161ced139f6ec1741f3722b29f9e6e44c6	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:763:VUE-863E610C652CE1E0BC98	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:191:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	6b879b71a09336397a7ccd2c9e6c26d82e261bd33bfc09febd84219455d50b15	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:764:VUE-5B657411B9A389E02987	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:208:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	5d3ecb45de66e39ba13dcb5893beaff5106f1f2d30fa27dea7ee172999e6f5f5	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:765:VUE-850EC6A15C21E1090038	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:224:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	27bd56860663ccc5bbd99f4246d68f9f1056b3309ddb21c81c9f9a963251c82c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:766:VUE-EC3B45D4C86A12EB9F9F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:23:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	a681c7efcf46d1bdffb831b1f6d9cf30c4f0bc5698844e5f191e58dba9df5af2	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:767:VUE-F115BA1B580E5955679B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:243:7	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	5e577f1b737524913f91a4acec37b192f9ca8e7d3f57beb7c63d7207bb7e8040	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:768:VUE-9920788906E15EC760A8	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:261:7	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	8834a1cddcfa193fd80fdd774341295d7464bd7084e17a708f41b91e4322a7c7	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:769:VUE-338AE9CB7574150BF2A4	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:278:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	eae85b32a0ca9e926c7e576c1bf4950cb6e84cd29c77baadb257a48a16867fd8	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:770:VUE-ADCD7021407EB2315E53	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:296:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	342f2232c297d4e9d541df98dcfa6c1b18fd76f11099cb27e8bce57567fc26cc	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:771:VUE-25EDBB894D3616A31D66	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:305:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	5c8f7d5c9638e5d2eee5845d6637103730a41aa5c5beddaba0c5c995927b0da9	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:772:VUE-1783D0968D48AA733107	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:317:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	e8ccd08b2e0d2f99695cdfa698a10c89c42aec0ee4ed5fff1a25950fbc6a2f33	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:773:VUE-841594DFFC92AABD58C5	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:328:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	fa8efc78237cec112fddc7d5269d6c3da4273bbbc77465f6988e96a9ac0339ec	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:774:VUE-C871E9479AC72E179005	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:34:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	454fb4b4057fd2c0cba6f795812ba83e882676b494aaefe2bbdb11d7414191b7	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:775:VUE-F820FFC0ED6ECFE20C96	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:350:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	a336ce141deac097c3b5b3126c4687042509ff927c1e9bb54460e4074f9566f3	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:776:VUE-3623354A9BB5803437FC	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:387:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	6c167b5567abccb00e36db035e00fc20c8212a8f799472c8b7f3c68abcc70101	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:777:VUE-4C3A59405E3EF7353333	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:404:7	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	55202e2206bab47acfee4b774d9e0f8269d6e20dd8ab60d1d00780529eaa8287	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:778:VUE-3A1D95A4567C90BEF2E5	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:421:7	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	5b5974f023998cf4a6577587e336c4c09960adf60b473c485404f4370d3544eb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:779:VUE-FAB32AF8B708A9B5F8DB	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:438:7	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	7de59cdac5650cd16b93d11c123a5a5eecb2eb1606fdf1d168d7288077beae7f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:780:VUE-FA3DEE37B98E165E8A55	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:452:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	bcd1767a3750472b9d27003bbcceac3ba698c22b6e4f08f7e738b603d36e5152	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:781:VUE-A40EC6617AC5944B7B9D	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:469:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	ba71f16433f1c681726dc9f98f38c2da6ba9f549f36b5261dfcb6ccd761d5571	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:782:VUE-72289A6ED7BEFC789107	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:491:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	b8de86c129ca216b0c078020372ff5f32d5a6ef6ce9198f89982bc4ba1f34c22	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:783:VUE-8B240682D1231BC9DF5E	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:506:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	b302ac623a13bc46a5ea9a0f6b9b3cad72340f9058aa72f6987ce1cd57f7716c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:784:VUE-ADDD236D8C6D2B608899	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:529:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	ab697269b07cb18a0ea2d5e133d0272cdcddfd35ab174f6ff8036515be8f1306	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:785:VUE-3F91CDF6BB708E6D4B04	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:574:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	207574b7c42b958d53387067f6febc4c4eb0d78c5f75ebbe8c168d045293ff7a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:786:VUE-04389F2F432E6E32A8C9	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:601:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	363afe66f87b61893b0e94fba269a32ef769fb54436f93855ee16a66253d4e39	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:787:VUE-796B528714F8732FF5DA	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:618:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	a9a4e2cb517075fea4c9cd4681748f8296c32f7e8fc13ddca9c585a45fd0a74e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:788:VUE-DF85BD50D90CFDB6719B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:638:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	1e03a0ea788c98eb01e5253e8b5c055c10cdcc2738788c071740bb0a36fd8544	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:789:VUE-67F842F6D3E35DBEE2A7	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:688:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	0530ec387d45d504c752df966ad1c8e885c5043e4981c42d4720040f9bc0676b	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:790:VUE-C947ECB5A414EF9ECCF6	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:719:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	5a3ea7a655b9d471121add7964550a2a7a9c8650fb0ecbbf17a27d1f44a09c1a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:791:VUE-2F1FE1E666143B6F6E89	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:72:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	818cf0d867bba8e56076a32b8ddb3f7f88ec10fbb6de6f60fe780f026c83329e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:792:VUE-2EFBC1D44EE7EBCAD4F6	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:745:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	04b3a074505c77f4653d856602868e29b307aa5ad1736ce3f2fe264880171a12	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:793:VUE-065AC848AC796D17B1CE	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:783:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	537d7ca7147a23a7860fd8e8ec7b400be6475d41dfe35a09a816e25035c5787f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:794:VUE-B0CAA8C6178275F7552C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:802:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	625f414738f5af69cafc62187241c57feaed040d96ad79a7753a1480da9c37eb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:795:VUE-E05A225911BC09004353	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:821:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	ddc507fbd919c5416e2d9fc211550da3c5426206fcff93592f4289691ce436cd	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:796:VUE-DCABE4A7C2CE3681E180	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:848:7	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	19c95646f72ad1dfdec88bc4698382725dc21dbc200ba2aca7673a5e4594c38c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:797:VUE-7A75DEA40EFB23141E87	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:857:7	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	df397057f9d248dd7da78ba21129cc6fde104a18ca4ca72799b60b616893d827	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:798:VUE-E4B42551CCE3C5080914	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:863:7	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	dc13014a29ea6a73b28c48e0a856129979f50602cd5ec59e5fc47625e4f407f7	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:799:VUE-4E9A09F4A3AFEC7A60C8	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:870:7	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	e95991b935ee6791ae087e0684792ac575fc79e2f97d11e0c501af2ad57a290d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:800:VUE-EC3E401B463BEEBA4660	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:877:7	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	1dc035ac10a9fb1f8efff475efbee95f94213874ab2b94bbf59fc5df8588156b	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:801:VUE-8ABF3C99FDD84B8B144E	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:89:3	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	896a8487e776548a0260aeac4f5e3d33311b4c814d62ea0b46d5ca9a810639eb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:802:VUE-EDE7B44A085E0AA3C5BD	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:895:7	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	d390e914c5c8ab4de2f4caacd6da369607939b1eee5bd3f23c4a73b2e3ebd56c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:803:VUE-0C378FE143486F1DC12C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:906:7	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	c2be8c9a18bc14f4d4c8f9f31667ca0755efc592553ecfae1c8f056f1f9566f2	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:804:VUE-CD4B477A735DC8404FFB	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:917:9	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	3559781b7210685ae61c78f9046fe40df91f7779650b559575965e35e36b507c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:805:VUE-56776D16909AA2CAA5B6	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:927:9	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	64eeef9dacd2ad80ab3bdf1606f5c13798b1b0fac1329a111ade03ddfe995f55	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:806:VUE-3349AABFE99E20D09039	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:937:9	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	0fd533f2ca5cfd3ff915d7429f1fb5c74182cc9fcf0fa9af7895207f36c5d730	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:807:VUE-701CA47948A880C7B336	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:947:9	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	c108de05b585d199eb072bd38794c98f45aa6db28df9343500b6502530887ff9	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:808:VUE-4922D6C681DDE007E87A	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:957:9	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	e6c56fc4c93559760448d4a5e6766f48d7f3df7b760fa081fb44e5b64a7e1ebb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:809:VUE-CDA13852E71DE3FA65E3	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:970:9	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	ea5bc9c6eede4c89512d840910f634ba814b9b06bfd2299dde1bc3ae13d2163f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:810:VUE-46F8EB627D1BC5215AD9	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:988:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	acf3fe86dfb7fd2399e5066302df1da0c443cd33460aceb2c843829e043a8b86	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:811:VUE-762A711942B0CE460CDD	compiler-sfc	packages/compiler-sfc/__tests__/compileScript.spec.ts:997:5	144368aa26174ec42728e10f961e505ac6f98b3e	single-declaration	StringLiteral	2c58025bfde0b3a06c12406d8e6b3e2ce9c2d87c24c62627a69a45f056caa73b	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:812:VUE-5165BBE141F96E6B2DF5	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:106:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	606cc89b9156049c25da90959ab4b39f2c86eaa7b275ebd820a53515202867af	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:813:VUE-5A6B3BEDF02704215C01	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:119:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	defcff7db87848fcd3a2f6f3d04bfa7a1569df0360f9176d27812e0896dda43d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:814:VUE-4CCB635972B7F990CFCC	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:130:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	3dbf1822ce1af9eb0b62bc1b39b5e8cd4a54d9713ee1abee34e27cf4c8e2b315	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:815:VUE-B42E0583C45A58D832CD	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:141:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	02182161f05bb889e70f010cbdd3c263710ea5d473f13848ba1cd8388abd3b1e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:816:VUE-4DA27B818199F7A19119	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:152:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	e6625cf56bf6b4c73d8f358c1e250f167de9928d1fd7679ea3d9c2f806e7dff9	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:817:VUE-118A3D7D7A3DC26FFF08	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:164:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	084895fbc0c4b03e567444f6dbe42cd9ee4b8b6cbb490047db4b29ce9450c12a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:818:VUE-077761E7A6EE1EC625D5	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:175:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	5bac673c60d652fa4d8ffb84a3aec7e4ca161868ae1a5dca5c221887cb912cbd	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:819:VUE-F21643DE4C407CB4B08B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:186:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	347278e92daf95c6a72acada583c0ef387fea111114a1844225379bba0319591	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:820:VUE-D8FAB11CD68EBE0D9D36	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:197:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	726fe1b0448c15f9fa8d22cd5002dce7bc616abb3c0454f6d1b9eacb538ee1c7	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:821:VUE-A16EBAC1AD346AF4FDDA	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:214:5	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	18fa0f92d6af87db039c61105fe490e455e547a6dedac07ae37c2c8b673afa2f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:822:VUE-FC45C3FCBD27C7FF4212	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:222:5	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	5b272f20576e00b500b901676f273f5b460cd87e6ad72a9d93500361e56109be	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:823:VUE-AA52347AD79DB6DD028D	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:27:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	e6d4792662d588b61d4fe7aaa3405338ea717123554b1a5c56bf5b6781a8773c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:824:VUE-F5E709DA8BD55361C9E6	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:40:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	9bc90ee03add5de432a21f595f52ba067933601b2b7c7d30bef12d8170793499	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:825:VUE-E8D20C4165FEDBCDDA2B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:5:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	92090856e77873124d22d4ce5e7b41e56b8eef37518def6dbbe706586ad777ae	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:826:VUE-D10046176BC5E3772D2C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:50:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	d14d6f4abd67c32496b70111a0067cfbc297734348e45980b7d5f820c6135d2c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:827:VUE-46C9BEC4E0DD59CD5C03	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:61:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	47ccfd0c728ec536ee4b134d2198c25232500f0d776e666cf936ed068d324c5e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:828:VUE-95BB6BE7EACB3277DA96	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:72:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	edca250de6594550b6b6c18546e70d7135bc5be2542a027bc3dd2812e957bd61	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:829:VUE-4313872CB9D53A30E9BA	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:83:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	852917e3afc637bd7770d5dc9ec1b5c56d9c0a06f52c8757c098ff1ba04318e6	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:830:VUE-4AE04EFF3D29382E80D2	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineEmits.spec.ts:95:3	99cb78a594ad8f74b0e344dda21c884f8e8ed777	single-declaration	StringLiteral	68d04db962ecae39dddc1e8e28fbedcf4f5884b26c05245583082689a36d65bd	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:831:VUE-82B85AF9E3D899642707	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineExpose.spec.ts:18:1	7b2a9f7cbe4c845963f86524bbebc65698b606ca	single-declaration	StringLiteral	e0bf3d46c75ac132ff2433d9139f2cef7d6daec4526a969db6c138c20800cadb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:832:VUE-48275060F27F5E902CF7	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineExpose.spec.ts:3:1	7b2a9f7cbe4c845963f86524bbebc65698b606ca	single-declaration	StringLiteral	7e560633c72b088194ef03ee863abed00e97cac25d05d1c501c4d48b415a54de	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:833:VUE-FB6ED74BF081B24063AD	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineModel.spec.ts:121:3	4cf2f01132632456f2a1421a48d4bda0374c4146	single-declaration	StringLiteral	acf0a04ea7dcda7e7fe0d934049374dff90b9e9fcc5c414b273445a039befa44	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:834:VUE-825E574F45307A2C80A2	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineModel.spec.ts:163:3	4cf2f01132632456f2a1421a48d4bda0374c4146	single-declaration	StringLiteral	a999e99ce301242fcabbdcb97967244cba49f43c8f6ae9104b3025f3d038c38e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:835:VUE-2463317CA568D894137D	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineModel.spec.ts:201:3	4cf2f01132632456f2a1421a48d4bda0374c4146	single-declaration	StringLiteral	f6cd15fc84ed0c1bc2784026aac5f81e12328a1282f70dabf1b3e85fef50c557	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:836:VUE-2D01EA94E7ADDD9F77B6	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineModel.spec.ts:214:3	4cf2f01132632456f2a1421a48d4bda0374c4146	single-declaration	StringLiteral	2e3e155b46e3b0002971af194b9ac4d3d2c5ad39f34ae1dd6ba8c959ffdad2db	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:837:VUE-535269A8AC01CE09CFF8	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineModel.spec.ts:229:3	4cf2f01132632456f2a1421a48d4bda0374c4146	single-declaration	StringLiteral	a7af78eef057d0ee15f69b9f778c6927c7d74405f6df674b8c97fcd849286f12	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:838:VUE-279890807A6360CC1FF3	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineModel.spec.ts:274:3	4cf2f01132632456f2a1421a48d4bda0374c4146	single-declaration	StringLiteral	9e497126586f4b041996af1903f9f7d45f7270b651ab863d9ee3ae27b0711e65	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:839:VUE-C15FF410D66970F5DE34	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineModel.spec.ts:290:3	4cf2f01132632456f2a1421a48d4bda0374c4146	single-declaration	StringLiteral	f5a37bff124d4d3fe62e85b4aa62c54e2cc88d86c1626decd89ff9844cd547a8	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:840:VUE-48750E8086471F075B29	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineModel.spec.ts:39:3	4cf2f01132632456f2a1421a48d4bda0374c4146	single-declaration	StringLiteral	ec1435e08f3cd0270a0af38d7c3ccf1c05d7398f4db9cbe12846160dc990b1cc	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:841:VUE-CF88721942F0CF404C01	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineModel.spec.ts:5:3	4cf2f01132632456f2a1421a48d4bda0374c4146	single-declaration	StringLiteral	92090856e77873124d22d4ce5e7b41e56b8eef37518def6dbbe706586ad777ae	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:842:VUE-856E9A56A74EE4B6964B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineModel.spec.ts:62:3	4cf2f01132632456f2a1421a48d4bda0374c4146	single-declaration	StringLiteral	26efe68ad002c4dccb8d71b043e51eed8f00ff19bc9f8ce9e3a83a83a406f89a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:843:VUE-A05C0F19A22C53A5842C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineModel.spec.ts:76:3	4cf2f01132632456f2a1421a48d4bda0374c4146	single-declaration	StringLiteral	0f99d5ceaafca4d7ded36b9f0d34a518b2d555f26c3bc53b96f8a095f08ffebc	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:844:VUE-2D376075BD555D323832	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineModel.spec.ts:98:3	4cf2f01132632456f2a1421a48d4bda0374c4146	single-declaration	StringLiteral	d581a8af96f6bbf79f60a88e982445cda4c14b872d1c6468a6bee6faa75aaf4d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:845:VUE-13D78908327560A6AD1A	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineOptions.spec.ts:108:3	286f1e11bfda401378603cb6ce25e3b60e908039	single-declaration	StringLiteral	c10cbe954855cf395cb4a1fc12276af67db605efa0c1037c5ad2fd148bf9038c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:846:VUE-77550112F980589CA778	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineOptions.spec.ts:19:3	286f1e11bfda401378603cb6ce25e3b60e908039	single-declaration	StringLiteral	0bcef2328301830e3a29ca94f919b10605e2fa15888d87a909d673adb2340ca8	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:847:VUE-8FB353F03DF7BA92B545	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineOptions.spec.ts:31:3	286f1e11bfda401378603cb6ce25e3b60e908039	single-declaration	StringLiteral	8b041654dbb85439416544f14aae31eb92e19380bdb1ee3dbf3da49da7fa377b	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:848:VUE-ED28DCB952007A959850	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineOptions.spec.ts:4:3	286f1e11bfda401378603cb6ce25e3b60e908039	single-declaration	StringLiteral	92090856e77873124d22d4ce5e7b41e56b8eef37518def6dbbe706586ad777ae	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:849:VUE-2DC4F429ADCF2533253D	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineOptions.spec.ts:42:3	286f1e11bfda401378603cb6ce25e3b60e908039	single-declaration	StringLiteral	e5424ee27b65745c5e21768fc7d18ab29d15953463e6cd8a803566e35a4aca8a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:850:VUE-C4967C8E1AF5E33DF2AA	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineOptions.spec.ts:84:3	286f1e11bfda401378603cb6ce25e3b60e908039	single-declaration	StringLiteral	57150279c18a1ed3828b20ce95d7190a5a0000e49814bac7d1a5545498a000cc	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:851:VUE-55B3D8CB7A9090180CB4	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineOptions.spec.ts:96:3	286f1e11bfda401378603cb6ce25e3b60e908039	single-declaration	StringLiteral	9c0bd25b9cfa43a24cf7cceb270387c01c4a2351526a206415bcc4f9358efa97	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:852:VUE-2EBF2D3FEB4E97BB0805	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:240:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	7acd0b6c9e3f9998f0d8f86aa7a108570197c84efbf63aec9b388cd2052c402a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:853:VUE-DF9BBA223F97F7D3211D	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:254:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	45e624e04c4acebbbdffdfacba29b1d5c05b1077f00f7533b374cbd30ba3459e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:854:VUE-BC56321B2D930DEE7B44	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:279:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	377ab114a7a59003dec7afad45d53b4fdf40a5b755a6dd65f7d99e76255f1928	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:855:VUE-26D7372C03AB4604CDA9	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:303:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	c03649a9c84658e3d832932eaf9e70333fdd847496430a00d526ad3b40f70ce7	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:856:VUE-B7F5BA19A5EA2D8E0C73	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:324:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	742a02ef8edc00e9c3f4ca427cba98e6a4100aac9db9c2b64ed38f87130942d7	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:857:VUE-182156C3178955BCB575	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:338:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	06e515e4c7d1d3bf6204574f0d38dc9efaf9d17b9ee501d1787a2f865dff973d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:858:VUE-EFF96F55497768C4D8E1	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:354:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	f0e110742870a511007a75b6c65c6b853ac0938350c359164bd2d71d1f9e2d48	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:859:VUE-FB147FAB2E202376B99D	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:36:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	6d9f63f4fdb2ae5ea5a34d726f74e384e2473fd6b69253afa684a38b4be8885a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:860:VUE-1BD37CA7555EEEA275E1	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:368:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	9f407c62194983141b118ad9852d99df4e910edd4598aac9b39f42147423f924	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:861:VUE-A5F2A7E235A52AA63560	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:382:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	572f84d29fdcbd0207fbb50fc1a24e12d0846db4d141a7f788f4891048d74552	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:862:VUE-F31D9FC41552633E2979	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:395:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	c93e45ac35afef43e5ea19395c3914877c8bb9bb91a925ed993299fee1a326e4	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:863:VUE-54DB474309469C95A37B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:452:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	a8c257fdc9c9a1886358a6d24ece7e292237340aa3f46b20d10aaf27126a8a48	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:864:VUE-693A26E382F037FFBE17	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:469:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	ed8e257e3c84eca013583f1501481468893da34dc9dbabca7f1a330d40bbd985	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:865:VUE-13DB4A54BA3AAA62B2A4	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:49:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	594c61f396ea49ac5e4a66f17888f0fc38cc6a720831797f010147b4e58726fb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:866:VUE-6A320AAF993E29D324DB	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:496:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	88cbee2632220ecd42d99cde85f1cfd726ad299898a625e835877e778da8aee9	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:867:VUE-55B2CA5760CFDCAB7080	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:5:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	92090856e77873124d22d4ce5e7b41e56b8eef37518def6dbbe706586ad777ae	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:868:VUE-35B0564DC5646CCF3CD3	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:519:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	61739352d1bfb736f18e5521519431ecb9920c9147422f4293ff9fe7e182a136	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:869:VUE-4B859A70CFFF21179C8C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:543:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	23279c71c3e0ae2e61de3a655a4f66f5036e89f8fc675223665cf82315474aaf	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:870:VUE-34C11024C9034E28A552	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:571:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	a9f6ad663d6b06d21d0295d8bdba1dd24a81049240984f9279fee58098ad9496	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:871:VUE-7D848FCF9D906A6C6722	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:593:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	4759d8f5ba791b9c61a3d81dbcd254ce123b86e7dc169e112843425ae6b18f15	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:872:VUE-15654202D7B524A92051	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:64:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	8eba19fbe3bd911f4e8ee3afec6ed0e837bc5909071e517a3a654b01143787b6	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:873:VUE-CD010F6611C8CFFFFCE2	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:644:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	722acae2c8faf0485d786ed21c5a8c7bf260053b7ca6c5b2d990ad990a662f80	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:874:VUE-01EF3EA4E1EC8AE8DDE1	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:659:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	cbba2d11b2f0bc1c0871aab4a674e5016a920f8abe0446ddc675a7fcfaaab814	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:875:VUE-4FC488969D9CF7ECBB52	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:678:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	9597f6c1e288cf0b10ca53ee387df80513fce9e7788629c4e010471d1ba955fe	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:876:VUE-E653DB722CCFBAEF550E	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:694:5	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	18fa0f92d6af87db039c61105fe490e455e547a6dedac07ae37c2c8b673afa2f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:877:VUE-AD5D2A58ECB37111354C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:703:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	7caea1d6458a18e1d40c213f894727f42b3e5c1b38d9d12059cfe5a4b91a6998	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:878:VUE-2C74D156E5334A1B98D2	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:75:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	2d1471e7a4c39ba5ceca2a0d2d33a144bff1b33cb7bb6660535267edefed6246	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:879:VUE-BC519F4E1557A818477F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:803:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	2fe44ccb20ce38e07aa07acdcc76f5bee5d2f779e3ef028e9102045c38979aa5	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:880:VUE-D3EBAE184F87D0D8D9CB	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:816:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	597f070a88b48ecacddf1130499679ff83337550c2da3477449eda528f040a82	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:881:VUE-3BA1E26AA1336AFF11A3	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineProps.spec.ts:87:3	8c984c16b09d0b7bc131e3c5e9016563dd4ae0b9	single-declaration	StringLiteral	9bc90ee03add5de432a21f595f52ba067933601b2b7c7d30bef12d8170793499	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:882:VUE-AE2DB4D27587737AC7AC	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:111:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	6f70a82d53b253fc8928105851679cd894cc635d6ff7850609f5133591d504eb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:883:VUE-7735279DD1F5E6DB5372	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:129:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	fafa22e1d64498fce44caf75602702b114d9c1cf478ff52c399e11a77f00a326	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:884:VUE-374E8FEF2DD5B92F5B81	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:13:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	92090856e77873124d22d4ce5e7b41e56b8eef37518def6dbbe706586ad777ae	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:885:VUE-EB2A3AC0CD78AB8E9CDD	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:147:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	3c9f276c491b050344f4a25ae4c518a3e48c605e7754e09834bc9e4a7822a225	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:886:VUE-F3774CCD6361105EDAAA	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:163:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	3e13f0172e872f9598b90457d8a36365d168919cfa62eab5927d18049cdf7bb8	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:887:VUE-AC683F4572FF54B7A25A	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:184:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	a744ad45c8177e4961e1b0a741fbd37661635548c20e5790159c414bfcba87c2	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:888:VUE-308FD0C3CDC46E6B5BD3	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:202:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	2cf506200ebf9e92502172606f3c4713dfaa2f335033a841702809082cd65c6d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:889:VUE-6F99981365536839B75B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:222:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	6724f5b2af399366cb69da6ba44c9784bbe14149a289d22d6ea04b7a056b7bd9	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:890:VUE-5DBE29F7BCC72187B563	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:245:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	a615768bb337f0bb7be7911d4b8952aabfbe3eee7574327673ecfc8a2452c480	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:891:VUE-12143B9D0AD5A9E4825C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:261:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	a9ae7b69be4e058b6a03739c7bd9e118ee1ac3f391686015f6c70f12a67202f2	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:892:VUE-EFDB063D0EFE48F3F635	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:294:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	fe3a50a6489a869eaa0b8c321ece99b20326374f64f597627989f7b929919473	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:893:VUE-7F57FC18365C235691B2	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:30:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	47ccf5bf2df669441515dc6c2ba40ab55a85c03784aff03e66fe400f1c8fce8c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:894:VUE-15CE552B4B0D5FCF6123	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:316:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	0a08d1c11fc7cd9b20239d07eee6740012a634430d10dd74156cab78527fd0cb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:895:VUE-018F210D4D8D7FE29553	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:331:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	ca6678b2eb6e75d8ca77db405ea1e6aaea3c7b4866291f6bbdcdfc7c140d64fa	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:896:VUE-9BA92E4ACBD5619F6250	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:358:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	1ced532dfb0dba9e0c757b97482c8cb0021bc56e9741844c7e572c93475cb9cf	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:897:VUE-98416C373661DB8AC07F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:379:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	5374ceb9e02fdba2ff6a3d7b19de25fd861262c5e9c6ecb01856e659b123c507	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:898:VUE-74D8BDE1E3AF4899FD9F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:397:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	700d36646e4faff6b6f97351f3ca71d3d276c1571bc867a2a0688846747617d3	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:899:VUE-8FAA8E03C90C111C1155	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:419:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	2a6f45e6cfd73c4fc8939d9a1529fd77d2a6d5de00505a91e94b0cf9d8c8cd77	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:900:VUE-D42BDA75B1A7EF7C3EEE	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:436:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	8bdcc7411fa3fa9b6bc059b4c13eae8d6ee4fd2f950317fe4bb5c51223aa80fa	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:901:VUE-8ABD6CD38C7034BA648F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:449:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	08de257a83e1a63087b5b14bf4852d4759560ba684591c674696787a80cddbca	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:902:VUE-04CF0B85F2447FA80D63	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:462:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	16782ceecd0beeed7d3261fd724f86ec52178182380823acf1217ccf28d677e8	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:903:VUE-A3CAC47D61A0ECA07768	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:476:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	5fde0e5aabdff98d1131dc0947070c60edf0cb732081b294cb1269afc6ef68a2	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:904:VUE-35857FC65E92E8EBB583	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:492:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	73f5e7c4e40676b1269cce6f0f35ad6e9b203aab21bfe8b34c38236bfeb7f632	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:905:VUE-D5A37E889C69386CE5F1	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:50:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	902ccb67d4d7990d909b63abbc020498d8ac33b0a0818717ac3677451d50d9e6	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:906:VUE-AD22CCA2349284CEA299	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:505:5	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	2516a44b5036f85cabb32f2c0bd6101ac145d1fa5be652d98622d485e2479c18	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:907:VUE-2C7F338DC2F6837854AF	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:519:5	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	11fc9f26e693e4b5a2b933b4832dae1c4fb7a4a24856cf7773b36deb063d9caa	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:908:VUE-D6F10595FC222E0A448C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:527:5	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	9b9846a19660b08f0677a90beaa1f11ae4524768a1bb34b27d920f8429e16eeb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:909:VUE-FC439E43BD5D467A8753	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:538:5	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	731e89f416822170479902ec3643187e12040b4c67c84c9a3213d3c69696023a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:910:VUE-9BBA8F22344F08419BA4	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:551:5	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	c90bac588e32fd0e619ab6f66057b6992234da5d0fde91cba62b62f6cd1200c7	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:911:VUE-10C86CFAEEA548D9B1FC	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:571:5	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	d4cf96ee787315b59fcfc3a72586db8a09760a3ca52cc7948f2d836f06be4363	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:912:VUE-60A39A8AAF0F8FF757B7	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:622:5	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	c7fbc0755221253b524883dd2e0336cda24eff1a640a757009a2925d99d16271	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:913:VUE-DCF96D875259891C98EB	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:633:5	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	d3bf167802a5041bbcdadcef5f266b4dfded31ff00e88a341a8e1829ade433a9	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:914:VUE-E347857DD59897202047	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:71:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	247ef3335a3978771c09beab932b53ebe30061473ce68a6702910b5d50c2c7d7	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:915:VUE-4ED75A029097150D30C4	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/definePropsDestructure.spec.ts:93:3	7d59db8d2128929cbd6cc770aac0e6398f391059	single-declaration	StringLiteral	73b5ef60f298fb1d0892ba84214f4389ff58f1070b643684d22c1f1543b7ce46	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:916:VUE-67A255904EC925B64A32	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineSlots.spec.ts:17:3	357709afdf3a2f448f8ee64b1ed60984138790bc	single-declaration	StringLiteral	d29f9ca997272b767d82f9065450a1de00f381a8634fb091c279675400baf504	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:917:VUE-CB7D6932FE12725205BF	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineSlots.spec.ts:30:3	357709afdf3a2f448f8ee64b1ed60984138790bc	single-declaration	StringLiteral	b99417db4a9a7234c72b9b4e46290fc13def17b87390317250a50ebb98b4e678	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:918:VUE-3D1EB2F0B4951BD9886B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/defineSlots.spec.ts:4:3	357709afdf3a2f448f8ee64b1ed60984138790bc	single-declaration	StringLiteral	92090856e77873124d22d4ce5e7b41e56b8eef37518def6dbbe706586ad777ae	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:919:VUE-54DC2EF5463D50745B1F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/hoistStatic.spec.ts:109:3	ce6191777cc0c644c10bc98bce65b3a4f80260c4	single-declaration	StringLiteral	916fa38f668f2bff09ac4baae7e9059d734304c00313d07d6e0e77d1064ac14e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:920:VUE-32CB4154FBE19854D922	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/hoistStatic.spec.ts:14:3	ce6191777cc0c644c10bc98bce65b3a4f80260c4	single-declaration	StringLiteral	526dee5667280e48fa6e4699dc49c6cafb088f640a3ff80baa3fd795f18b5978	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:921:VUE-B127531766A25A2510AB	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/hoistStatic.spec.ts:140:3	ce6191777cc0c644c10bc98bce65b3a4f80260c4	single-declaration	StringLiteral	e940e7694c2f957ef0a870f27566a1cc54aa066108b37d7819bac8d8a48d1da5	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:922:VUE-EDFD50961FBE208C7F32	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/hoistStatic.spec.ts:158:3	ce6191777cc0c644c10bc98bce65b3a4f80260c4	single-declaration	StringLiteral	dbcfe6416ca0f0e93737ecbc21ddc086a00a68f88c87c1802c69859f80da8d1c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:923:VUE-C0765419AE6E687440D6	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/hoistStatic.spec.ts:178:3	ce6191777cc0c644c10bc98bce65b3a4f80260c4	single-declaration	StringLiteral	5a3cf1cd3cba458b687c82e1fa88a530e051e187f034e6f9ced363b8e34180b8	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:924:VUE-1E338E2C9CBB22C57B32	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/hoistStatic.spec.ts:193:3	ce6191777cc0c644c10bc98bce65b3a4f80260c4	single-declaration	StringLiteral	f30d5714c402dfec12a2c1435afa063fa2b04ab4814e6b1caa59633bd2e8359f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:925:VUE-53F817222A97EC250652	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/hoistStatic.spec.ts:208:3	ce6191777cc0c644c10bc98bce65b3a4f80260c4	single-declaration	StringLiteral	f073ea8e12aec07865082e97830725a2d1aca2d72d348bc9c57cd6c416ac4edf	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:926:VUE-0513CC7DAEF7843510C6	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/hoistStatic.spec.ts:42:3	ce6191777cc0c644c10bc98bce65b3a4f80260c4	single-declaration	StringLiteral	beb1fc4b5b5a1db37776cb21466d46098bd8c94e035e2032bfd5c5887447a07f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:927:VUE-5F46474B5E05036D7D94	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/hoistStatic.spec.ts:65:3	ce6191777cc0c644c10bc98bce65b3a4f80260c4	single-declaration	StringLiteral	810225070e24627013239c3a5f66c43944094484633653bb3a3352ac7ef67658	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:928:VUE-DF6967205FBA22E63F26	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/hoistStatic.spec.ts:87:3	ce6191777cc0c644c10bc98bce65b3a4f80260c4	single-declaration	StringLiteral	93a9f3b2bf80cc7aa4ed0d2b34d68d4de77e9079799a84f75fb70c0d887fa16e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:929:VUE-1E32F16FDBAD01B5DE25	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:102:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	843736b4b66e0a3f1df1aafb8c0e05c59c24c39a3818e1dec3c496f5544b7cb2	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:930:VUE-B7DD7693C4908B448B7E	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:119:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	1d0c7df159615db59858f38ecd6d7030082e6b2eb60be77d6c1fea9201ee52a3	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:931:VUE-D7E2BD04BD601E3A8234	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:135:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	b4fad14f8731ac00e61ab21f3ec7a864e163b5b1464083192fe27249e1caa354	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:932:VUE-7BB2B39D32074C3DDCE7	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:155:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	a3832cab75b47cdcee1a424e34230ddfeb1601c475686364c0876ac01af16979	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:933:VUE-5B0D55B6363A26295428	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:167:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	ba8aca0aa50ee91dc4777665b32aabe34c6c16c2a118724d91271da4c9b80e1f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:934:VUE-365866EDC4EACDCAAE98	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:185:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	7f87ae22d5dc61405939ba4f5d3cc740d7ee86fad59d5624a029cb284a58371a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:935:VUE-EA9085F91175CC6CE9E4	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:198:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	832315bb86c91e2e0d57a2a33cfd8a6f209f8dbe61f2af9d365455176bcd05cb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:936:VUE-D41E367FD5239D37FF53	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:211:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	3ecfa94f2ebe5ea6a5a1fc1d5dc2e06390677a63f1458b22995356419c8aad0c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:937:VUE-9DF1DB81304D519AFB08	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:225:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	bdd002afb20042a3fddc4a7bba6f2d5e17a85c8f65e1fdede4eeb1d1288953de	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:938:VUE-6041A38D471C968A3B3F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:238:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	2d6df0d4dad9972ac62c1e7b1b13089cfdbfc7e92fb6062d3992b1f527e58609	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:939:VUE-5D5B8480752644A08409	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:255:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	683255f4d86e6c9f18b562a7dd8f94f88eb9a7926e9094c8314bbee6c534ba3a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:940:VUE-20EF30882460C9F53D6B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:32:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	b199bdad612bd498760e5f4c03556f0c6d77d8b230e7ab9f5a501db66e7fd749	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:941:VUE-83B628BA71E85D18C7AF	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:45:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	879cf46bff923bff0129e3294fe8191984fa09deb943121f9fb4407f4207add8	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:942:VUE-D986BEBC7689996FF0E0	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:67:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	7e21a93c3b926c4fcb18be89ebdc4e02d4ade36c4c90b70992417e212933b1e5	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:943:VUE-6A63DADAE0B5098D63A5	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:8:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	a0ecf3389981e48485aa48fa8cc47137e1dbd8999cbe906b76231bd9817b111f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:944:VUE-A55B5825601CF78A8E51	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/importUsageCheck.spec.ts:83:1	210fa09688a4175a66278eedcd19a026d56c2a3d	single-declaration	StringLiteral	3399aa593a3608e05c5902a1e7a6c677809bb61bb1032f96efa67225d591d29d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:945:VUE-453911DBFA5375B143F1	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1014:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	10cd0dc576719b81a4714c4389c4226365d3da06f7b63ce86cf5efc8c434dcbc	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:946:VUE-1C0FE60ECB376BCBD7F7	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1045:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	096477169a3493df6007e8d2809744947a887eaaf15151c1b35e6cf3c10adc08	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:947:VUE-A996077FBD5131CA0238	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1061:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	503d8de51f1c4ad5df95af9701b3c9d207817455a83e232dab98d501071740e6	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:948:VUE-295AFBFE33B53997E449	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1083:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	4abfa11c5c46501c6b70407a0e6615793cd3cf7a9d8a8f72d321b9234c60c46f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:949:VUE-B646A1B7C825BEC84D7F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:110:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	2c093ce3ddd2888ce69df3ac71e0b2ff87724ef28f21255a078655cd9243f420	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:950:VUE-1687D42D9E67AA154591	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1104:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	2e834c1a39d0265adbaa524a1310ad574476bdb172011ad130db43dbb383cffd	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:951:VUE-9A5D5066DEB6FF548C74	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1122:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	51ac6fc10626ff46e8fe00e9154f5ad179e595f102ff625f47c041cfd1d944a5	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:952:VUE-5DFAEF7697E97BED6A57	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1140:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	074fdfb7944930e891ac3aab37c3d9517d9b1c18bc72d522fadaf2da1e3d0460	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:953:VUE-7D97248FD916228B0846	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1160:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	eb8bcd07d0eb78824d7bff01def0d5278b81a8103fcd56d17b096da4bcbf12e2	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:954:VUE-AD6FFFF7E74C75725F53	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:118:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	6d9ac09d502e96e82ef3f35c9cdd56351c1df51cef899d0eef2542cdcaa93faf	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:955:VUE-59754AF1394AB28FDB0C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1181:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	d4303a6e48d1b9811f952bea21bf84aa6d76faee7c38afe2b429b2d2dd1e70c8	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:956:VUE-3061D5DD664FB9E2CACD	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1200:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	bb985c007e42dc78c5fd8de4db5611a734e445bfa8cfd59f191695896c3dbce9	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:957:VUE-36C60B0D3717EAFB1D50	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1218:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	e9d385abcc8d91eef03ca32037109619d931859c749babfbe22f298bf54d9b2f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:958:VUE-48E9D00DD1128415D9A2	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1243:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	d1c26d05603ec7bbe7a30ff527768dbbedf97d746c3b74714336e59a25a48eb4	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:959:VUE-1CA3BEC860A105EFC02A	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:127:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	b630cc559963b72396bbc294c0680a8e5475f84c235083ac78a8e7f2d9dd8c13	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:960:VUE-C69CA01ECB8D2B9D05A5	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1277:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	8e09e91f19851caf1ca5001d6c601d306e110b42420f41722ebf57575e4c1dd3	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:961:VUE-8163B89A1957B10FB35B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1296:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	8cc91d5d0e0e98a64a1e693965af89eef90d9daa0983a9e913a5ec8e2eef7536	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:962:VUE-2DC02A4B18C2AA1C5568	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1331:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	460ce4dc51d011a703763b4dec21f3deca3415e0dc0cfe85dc538ffca0ba7f2c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:963:VUE-D2D4304ECACA910688C4	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1370:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	b0b16dca2483bd4f12a497326aa080fd1d9a139df23fe15d654537bc30163b21	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:964:VUE-29F1D76681510455CF4B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1408:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	277f58eb7078e4e4843466ef3131336aa00995c834091cff87777f2cc4efd399	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:965:VUE-D8A9DE2F50DE1C026975	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:144:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	ec852f00f43191c1a09c513da2192158cfb982bddfc5768c84ec0eb578da0862	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:966:VUE-C77F8D17641943577F5A	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1456:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	11f9eedc8f00f4e686ee4b18f98f27a89ad54769c51a18f26917e1407d5dc6d4	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:967:VUE-34DA8A643A0AE6417BA7	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1498:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	affeea7918f10dcf32a71658f07afcd1bb5f7132a546b3a111eb4a510c524d22	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:968:VUE-64896962C653B9A93CF4	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1526:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	099e993c132d43bfc8b718285676487fe4bd70f455de40591ce258ebf8d005f8	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:969:VUE-79A4A9B88AF0EB47D582	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1552:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	3b4047bd674a9c322ce96f2497808781355d6b28a6541ff6f91351591b3665eb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:970:VUE-C3D83C0EB7ADBFC39B3F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:156:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	06782cb894acac9c28fdf140520afefade1a6f4c4034cc99d594c80afe2b95a9	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:971:VUE-437B811B7117770DD2DD	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1579:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	9842dbe324c7e0d429f9751284c6c02de053789dc40f5c9896e55fc56cdc8895	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:972:VUE-5CD1BFA782E84F8144CA	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1624:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	4f624157b9325c261a86258cfa8d344732b82a704a2f84aa6eb9a7018c59ff02	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:973:VUE-51F0D0A24302F70BF8CC	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1648:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	68efbc7d4b74c3f46b21a9d43339a4a2f93622ebb59982795cf717592594756b	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:974:VUE-42E9A59F8E8CF3898FEF	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1675:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	4e1f10661db0931435b9809946dee1dcff492e2e88d319128a7d3381ba6838cb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:975:VUE-2FA1588CC0090E3E07F4	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:168:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	7e32e5c08b17bbf32cb651e6dd47b071e699ac12ff4bc8f2c0cbc33263140b4d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:976:VUE-EDB4D908B533E4BF59F5	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1712:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	68313cbd9146708da767d497b416e6dd36dbee09506a9bce3a187f5073e66db7	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:977:VUE-225B02E03EF5DC905DDB	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1740:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	8b88d8bb780e4925430ce3092fdf4e4fc1efa7642108f09a61243db8404bad49	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:978:VUE-D036E77AACF24DBEE101	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1760:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	4b0bd92aa59e77f1ad94329de54ff26346882858b089f036c25563e34644a438	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:979:VUE-F1EB2E042546225618D6	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1778:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	BinaryExpression	6bb38c85356667a59c52c37d59f6a4dd5306e544599f9532330041f8b03bdfe5	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:980:VUE-0B76F16040349F81DDCB	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:18:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	5a8ec801176d17cdc6f0b27788ffe98c1bdb4a8366461737690ceb28a1104f3b	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:981:VUE-62C455E313451F1F5429	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:180:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	5fef93166541b44abca9e491369e0e296ab41c2560f9b5d2855cc5e47fc8204f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:982:VUE-3212F1F9582A688D068A	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1820:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	6b858781f704817975b02898b219a8f24f401d785fd58eb5558f94760587b292	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:983:VUE-0E53713E4D25A402EDC7	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1849:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	232d3aa943bc7b6a37e38cd255911cd152633869012e2b359ee69038f987e14a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:984:VUE-07596A247343F04AD3F4	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1855:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	bc4cf55fdfc6be7f1f8f6bf7a990c3ee7ad9261e72a3115aed8f09ea25644b62	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:985:VUE-FE3534ADCD2A41DC719D	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1861:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	bbbadb89b046e300b502fb539011a53aec79ac91ee03705929227c4b34fb0797	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:986:VUE-D5D1C6D772243DEA55E3	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1867:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	f5755985bf4b4ef401bc5a8abe1ecf98afb3caee3a58f557970eb89ffdfe538d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:987:VUE-A5D386E3A7437CC525DF	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1873:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	90b48e6415ef9995a4a00f1a4e9e58d883ac6995393f9486f3c60f99252933c5	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:988:VUE-7370EA37DA8E77A27DEF	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1884:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	345da9589edecc9ca517b233a6438663a7108d42a4719e8885bcc3efd959d092	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:989:VUE-92C55FFC27E9BE832CDC	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1894:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	7493a8638069ff34a886f9d89fc6c724b25cc49f67987b96ebd603d77cde37f6	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:990:VUE-8B2E75C087137A99B250	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1915:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	a1205d092a65e9403f15d917883698b1968fbd485989f05fbb70182c878424cc	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:991:VUE-1B2F45DFDC071DEF08EB	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:192:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	52cb4158e3fb8b70dd978c8406a3ed5ae563f65fa4908911e998703997ea25e7	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:992:VUE-4663ECDBC80ED93AC537	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1928:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	e6456ce0c210dec9ac63be90ee400fe8e04e00e587db77e69d285c53fcfa8945	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:993:VUE-8E8F6F261DCC4CF66075	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1944:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	be5b7e39cd1c9db16e4ae03ed7d8da1b6e9b7a21510f72b2f5da4fffec05c74f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:994:VUE-255BFE65EB03B655DA10	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:1968:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	85dd80c5eed6e556e57c90a7ae17f33c7d81694b83d4eee5e7395104bc10cfed	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:995:VUE-2B431EC7A030E0BCE8C9	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:2002:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	030d8f03894321fd5aa2eecb19dfac5c4cb92b83e52cf85e799a0ffb25362b47	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:996:VUE-A061D464314FDD64309E	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:2022:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	ffbe3fabb2fdf3d257aad6e1b1702aea01898ea32b3bd74ca3cab349580d235b	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:997:VUE-1346B37AD30F2CE16719	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:2042:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	73c8a9793ccdab312c98e137d52dee11fab6d39e48d9973fde1e043e9c1e4dc8	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:998:VUE-9DB7FF5D3DA0F21905AE	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:220:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	37652892054f8f8f474eadd126007175d4bce1b13931130808870dc27f208c7a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:999:VUE-A1353C6FD470A3395C0C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:237:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	c65c67a8023ba4ed7908b8682538a8dbac0d57d3579ee465648612cac7928caf	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1000:VUE-A85EAE64383FA7BEBF8F	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:263:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	ba06f3a5eebcfdd407b75a608757eb466f80e1db5bb492497d52a65687fe5871	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1001:VUE-30D34CB7C66B06B98BC5	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:279:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	2d1377882f1f0769f07b8e6aa69ffd4c4f1c0d7c4eeabbf601a24b5d8432218e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1002:VUE-53E7B016828B7A55A0F0	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:295:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	8731caa8b8f73da490c3bd771678e6cc0edee50a480659c453da969a9c6b87e8	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1003:VUE-AD44B9FB6AC054017B8E	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:308:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	df32d6a5dd165469127a059924b4092e97a1b3b3165ae64ba5c72aca9735b96f	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1004:VUE-CED22233EC045D26E567	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:320:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	e1815aef1ca7eb24267fcef889ce143795c357f77d22c977500d3825ccd0ff89	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1005:VUE-A3F5F56A2C22EB94799D	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:337:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	ff266268f61dc0b59b03da36448925a7ffb18797fa6c028dafc4ef650319fa28	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1006:VUE-159D88A4D8EAD169100D	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:347:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	af3284146bb004ada67f7535425d764737908f2ca2554f101f0d729e2a6a6de5	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1007:VUE-7A11F684ED3AFB8BDFCA	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:358:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	75f180ae0169e65e53f61bc6511eeb719fdf0c003433d7af8fc5b04190425fac	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1008:VUE-D350796FE75A3F0D1F35	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:370:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	318907f7c3aa32c2318eaaeee1a6bb0b9c654d57ec7e55c9898d116d8bcbf33c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1009:VUE-27C2EF9550466267A53B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:38:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	9a10b33dac96dac10be6d5d1ca9091cbb420b39b4c864c9f875da8aed8fb1714	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1010:VUE-D32B2E35CC5476DA2647	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:384:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	d67a69fe086582a952309daa02cd3fef27f679d0e1011a0812da90ed74e50291	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1011:VUE-2C6BDB15988ACED2ED41	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:401:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	13e20649a26d2424eadf671023cbc3e1fd95f2cb060a2bb40f40b5e77dbf101c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1012:VUE-3FD569FADE4EDE5A6F28	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:420:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	86613ed770deec570e48ff594f4d5ea698e2b1181f0f9ae8da05e0783e5cddeb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1013:VUE-F4242BD76DB13213CCAA	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:440:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	e305e2b8d0876b97b9ba16af0da9400205c06d7204b9d1e2c42ec5025235215c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1014:VUE-9BBE50E175E9A9D6CB27	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:460:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	06201fe56e40013bee0196d518681e0aa5e696136b1a6623465921ceab7a2c18	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1015:VUE-08A8F3513D32475D375A	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:480:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	88d6fd40858a2da2c6c6fc5598f37f2405417ea6741135397b7cbe2e7fc220db	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1016:VUE-DE89FF0202BE4249BE4B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:49:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	d0195d0ae0f78811d1530e168f8f0c581abb1c4e459e4c9fbd7c93e7edecf922	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1017:VUE-DA653DFB4A5EBC21F65A	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:498:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	7a3640cfe92e9ed05ebd6ad10e15e80b695b0a68b1253c4f7dde20887186d6ce	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1018:VUE-C4474C743E975553C04C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:509:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	86492c1da142931667232f3b9d51b6e02b41576f48297c349c4419676ec9ae83	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1019:VUE-F3D976B41A1AFB2FD5D6	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:519:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	24d85df0b3aa421152e12dde672a5e49ccdb6766803bdb0593dec5ba195c6acb	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1020:VUE-675943134D1BBB966E78	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:555:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	1f0ebd7a571ab4abb5e856c5b3f4e50bed5be4564965a0eb2af354fa7bf28c4c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1021:VUE-0EAE7D79468AFA606826	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:585:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	a4bbfc344df913a5e3281a754104c5ac556a74ac788d6e93cf43815efcd26c85	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1022:VUE-730ED0E1E668A4E5414C	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:597:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	55f6082f0c7329e367d8563b371512d5f2a590dc4105f95327b873edf8934c23	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1023:VUE-604E52D65466A8874758	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:60:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	28ad8f3957f6c48052ea86867220009a998a990196c8f262ebe45fc663016434	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1024:VUE-995F3CF98E1592B3DA9E	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:609:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	7d0fd3190694705ea0f342fab962dd03f119d3c0cc0af24ad6a44963241a90b2	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1025:VUE-14D88A5F28DE75CF3CE3	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:640:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	7268623aa8e42508dbbf0edd6e5623647fcc62305ecf31d8b892414ed767bf2d	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1026:VUE-C2CC307E161FCBBE6D38	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:655:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	33799e9f63d90b7a6f7ba8a2e93938c6e96eb2c5a7dadfd4ed8de45931356a1e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1027:VUE-D4EEA9852A91E59DE501	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:674:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	cee8025de189982d6c40e76c2adccfbd7cc7f98b17dcdd4af55a90293a1b9779	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1028:VUE-DB00903ADF69CE84CB5E	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:693:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	bbdd142769d5f9ee768959fbf64e1fe019f585c6f1d4905033fe1634ab920c7e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1029:VUE-4E16FC4C44C5CC85783D	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:71:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	d29a0b24588fda4daa863fb8fd42c117ffad096f5c5274f2ace1af0f6a30fb49	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1030:VUE-C77C93A7E38A67A58A9B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:714:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	69274630f2a7943a4f0e7133fda6bc211449eea64a4ee942517e809bfa0d8972	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1031:VUE-DBC9157DD64F22D55EF5	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:736:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	30819a2fc8008fe2562aa4cfb0ce4f081db938a5719cad067a569c36a4d34c49	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1032:VUE-9DB1F0F978E99988AC9B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:754:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	b15d3cc9888bafb308dd5402efad166ab60e5bebe93e47614b1862d6a979b151	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1033:VUE-76A28EB6F659CCC7C985	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:773:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	29f35f330f11f5d2aec27e4563310ba2541c435b6c4f9c0c437687e2d11a788b	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1034:VUE-D266996D423611E559F7	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:791:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	2c093ce3ddd2888ce69df3ac71e0b2ff87724ef28f21255a078655cd9243f420	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1035:VUE-28890C53C5EE7E0DE059	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:807:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	a50ef688dfe445df0317760e9247306e98eac602a30c1a58a156a89dfa41c66e	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1036:VUE-8D4DF5D5A26C01019E61	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:82:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	221eac9b35be8270179a3e6a252fb3a835577031c0961f49b0f22ef41054e0f6	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1037:VUE-CFF217638D789EE03AFA	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:820:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	d40199053a483f59ef087ea94228f16538f395a9e973c024162b6f4e04b131e9	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1038:VUE-07782A51B998A44A1D7B	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:832:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	715db3d81b98995c0460a510ad9638e25c6393e4cf9fa71030386597b4273da7	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1039:VUE-E3641C3E666F2BF97D58	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:856:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	bc4b06fc098250205fb4b3f948707ec9b94d885ba4f0ebc0f923e486ee565c8a	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1040:VUE-C2FA67A86217AA1E5968	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:888:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	133e9e8e2206c1d8af192aae788c162e49118baaa83790e3096747e07612cf81	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1041:VUE-7314E932CBAE0B68C8C5	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:899:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	5da5b6d18e62e2b75c60d52b9692a1f085ba4358c7099948eb07ae313f60e659	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1042:VUE-1B7779F1F56E8D0FB313	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:914:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	3bea0b6e3de3b3c4ff1f5264171793a14851868a7a29a429055a8e76a2a7085b	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1043:VUE-A3C54037D5678DE694B7	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:927:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	a7ed3adf891ce302c5c18050d278c72448edd2a76a9581d3311098c7c043821b	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1044:VUE-590521607E5242202BBF	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:938:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	760ecdd6e968a47f13b27b1ef2b3385b83f91018291f4c022deba32b8d10cba7	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1045:VUE-766EBA381397B05D5158	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:952:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	e03dca540220e34a8ee1b4cd1c8a7cc7bfb2eb1876ca8395e8d21f36229d1945	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1046:VUE-53E3D1F91903113CC400	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:970:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	8d9bf8903776cd84034fedab7ec34ff6b18aa1945e7dedd18d5d5b767f28972c	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1047:VUE-C0869AC134C10700CA86	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:99:3	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	4ff6d1875ab2c18b61a34ae399f2ef83878fec87b28a6490c9042119e45c2f53	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1048:VUE-394AE483142A22CE2DEF	compiler-sfc	packages/compiler-sfc/__tests__/compileScript/resolveType.spec.ts:994:5	ad066c7a069823e5af5b27d73b05cc399d6f6252	single-declaration	StringLiteral	46e0462d1ac3afc39c1cc2cf722f90477f86236f592a869437a58ff75be6a4c6	blocked	B2/BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1218:VUE-0B39D835F47021B4F58A	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:107:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	8d912c44adc99a1dad42de408f498c4f9af98a87894eb1dd851462cab2b4cb2f	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1219:VUE-96E2B180BDF741732CC0	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:118:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	2396ad600a43c676c81be42177eaa653c8e163d069bf29ba91bb6ed071b10bee	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1220:VUE-3AF0F9179859C4B00016	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:131:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	15e859be55f57da810993cb444d6ba1b0c53dedf640f2f04b15fa46609e69888	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1221:VUE-50AC33B44B60705ECC90	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:142:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	3af9fd4e0d807fc8d9ab85f648e80779a9cb5ee4855a0330d49b887f9ed90188	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1222:VUE-D58030B7D956A0B48174	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:155:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	3bd47bf6f4456fecea180f5d9c1adcbc12468a5fc3e3b9924de44150f7733d53	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1223:VUE-1697709D3D342DDE1F89	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:160:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	e412019cfe0a34cf8a696da470c829ac663db46db6af53218215c2838242eff4	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1224:VUE-4703AB9232C97A2A458D	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:198:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	acd23e22d934d4d7100129238a7c8b2ac1cb08fc531654bc09719630774dfed3	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1225:VUE-FC2FE355E002B99348B9	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:21:3	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	f48e120788f03e9dcfe6c5cd1f7ab2af16fdcc01c3c59e8243c847939ade9219	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1226:VUE-CACC66C6F7F857AD6A79	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:231:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	95ff81626344db48095e566e7d8a6626ceff083586926c88a0649f91bb305ed8	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1227:VUE-756056522CCF5B960940	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:266:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	311ba4c34147e741fc954d2770ea733e337f4f54de84f822a105963a9436ecbb	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1228:VUE-53ADFE5327890421C565	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:34:3	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	c6deef4a5bc20bbc75d7396a05f1fd4e70a42b11c92253a99dcdbcd92c207cf8	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1229:VUE-4D02D2A287884E3E84FD	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:341:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	2b979547ce51a10800a936afd040813c3670a531f5c6ff72c160e6cc3f0f3d4e	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1230:VUE-AED1BA54E534311D5459	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:369:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	0475109d3c7f2e173f8c63cc63d515aa4a0810f0e28bb3e436ea70a6fbf0e232	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1231:VUE-D86DF669BC2C9FAC973D	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:398:7	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	1d2bc7217483ccc856bc93e5a8f13fc164fb0e96f3e13f8ad70942f335ef0597	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1232:VUE-DD1257F556F194C9FD58	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:4:3	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	8630f6bdb0e06a9167722b29c0d0428180b912f248abe561843f8c31877ac1a8	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1233:VUE-B9CC2C4EE14E8252251D	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:409:7	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	f315725847abead6132f5cc65e7a6102b1d21acf0259f792b61054f90d8855b4	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1234:VUE-B86DE018415D8B433230	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:424:7	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	0b73e98ef60a05d70ece4bd19e95138085dff4035d41bd6d810151d89896b586	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1235:VUE-FA293396595C5EE3D81E	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:466:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	8630f6bdb0e06a9167722b29c0d0428180b912f248abe561843f8c31877ac1a8	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1236:VUE-10020941B2D58FAD264D	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:57:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	62d48c26c7d8237595d00f6bd4fc97bd999bb3b06f55b7a93608ba50f7394e7a	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1237:VUE-4229D0F3D4AC156D3CA6	compiler-ssr	packages/compiler-ssr/__tests__/ssrComponent.spec.ts:82:5	89ee158d8abdea5e91aa7bc738fcda65719f39e6	single-declaration	StringLiteral	170e8682a843b92df81945350493b6551dc63875804afce39d52247b9a7db313	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1238:VUE-3D54A6E543E808EA7C44	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:113:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	091ecfca47923eb4a289a43b72a30744cf76ff613b241532f77d617b1fab10a6	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1239:VUE-A64D6697540048F7FF87	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:119:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	e955b727dfde2caa676921c463713845ddcf8be6a94533609db3189fc29ce500	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1240:VUE-D9D5AC96744B05367866	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:125:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	4f1c3ae690ea320d7efc711a47d9552da38fea59cf02df32eaeca8c281407a39	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1241:VUE-2E2ACECD71E57CAA601F	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:131:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	c8b2241cc8e58481b64aebc4038cf154737fd3654cae7a47e78c56c9605b2909	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1242:VUE-51A9D68F892C9F61EAD6	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:14:3	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	a67b1d89b4975203f0e49f176350ae89dd891e896633d2f5aa181f2bb5f7cfce	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1243:VUE-4FDE7C0C01E4E027E785	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:140:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	1e72d304fe1d6f7837c0a060659089cbf27189a3909df73698e40134af241e6b	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1244:VUE-6CAD4EED06474F8A97A5	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:149:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	df2ceafd1f9c9f3edca48738f52471077dfb1688e7031a909fe9ad8905bbbe05	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1245:VUE-F10A4FE5FE89A32835C0	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:158:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	85810dae0e37d56f5349535004f8e231b6c9a1c7bcaa2ddc0bb32ef828f975d5	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1246:VUE-AE06F2FD4F666C1BDA6D	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:167:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	f04265c8f15557d95820a12b6ba6cec540b9630e5685220b03cac96a13a749e5	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1247:VUE-37B9605EBF8531685D49	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:176:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	3cd41e1684c152070ebfa515e7a69ea492615806a932fa4fad67b3aee3afbfe8	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1248:VUE-EBD7F7B79C8EFD120F71	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:185:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	1990e52ec2b8c241fcdfefed65870eaba041878a1155863f31b126fdf8bdc7b5	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1249:VUE-FBE6E121695E5CDFF315	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:194:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	5823540dcf9c2d4982207602e4320f0d6f20bb9b2f89baabafc5e40630a6c9bf	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1250:VUE-69CBCAC040905E32C5E6	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:20:3	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	a754a2bf17a9f6e489300fbdb46d4ed5db5217b1311528c708bcf30f47d8e369	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1251:VUE-4D89D595CB0BBEC9E79E	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:208:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	db6d7e411165d11452aeddb649cf2a71c91414d7f1c2f1bc4f7aef2d5924ac50	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1252:VUE-793ECB8357DF888E9504	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:237:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	fb8e8722bbd584c03a1701b6a87868cb07f552a35ca28b323f0f5239491737fd	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1253:VUE-3FA4F94106F7BC1F4003	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:25:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	b7efcceb5ee429293a28fb7cb0fc1d22a3a86b16a8b8b5506f1f450a559ed300	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1254:VUE-F8C1C0F408DDA0C4F512	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:291:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	d90d91070e8cf3d632c5b6d08af15513d726ddcab9cea91d57da6e36aa858db0	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1255:VUE-FCDB95AC3F2FCFE2121C	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:310:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	053923f11e7d13334011db4e3d481db38e87261cc610979389a5c59cf220aa2c	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1256:VUE-8C4A080B1273B707F10E	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:320:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	0d3f43836674ea3bd1d13258787e3474f85be7b6d02d82250233d3bac5b7d847	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1257:VUE-0C0BD60B7257A4FC02C5	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:329:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	eb4087ec52dfd27f053141a4fe59f9d45f3640f4f1b4593069b3e981d71dd613	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1258:VUE-607E52F2C147E4D9E843	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:33:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	a3ee89c132e6dcea7e3a3844dd89c567461ce6f322f0e94fea618a2766f24a40	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1259:VUE-5F63C72C85DE686A32F6	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:340:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	da7113f257b14a192a0401907a763eac61751517b0541fb7b5f996f937de783b	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1260:VUE-6E67C0E2BCB1D986DC37	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:354:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	2013627bfcf10dde2c9cc3efb31107cb1cf77a6e762319e159e01dec75358b37	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1261:VUE-6AAFEC4387D8B3D10182	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:365:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	bd44125009e3a3bbd5bddd1bed9d41afbad5a9b7e52740383cb3a6f8658acf53	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1262:VUE-7023A5C29AF76EF952FC	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:376:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	9186ae08b9cecffe3e72329254de6467e0e1593abd285a5200bf5b6bd95ef36b	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1263:VUE-8A93391A3987D1FAE2D9	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:387:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	29cc1d1d29695ec80dfaf00314373b46c7ebb9521422b148a1624f47b9d7a332	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1264:VUE-CF4EB9EACFED4C8B0C63	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:398:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	2307f2eac3516bd16edd49ce7c219bd5a25791d4f83975f881b35527ba0a6299	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1265:VUE-78F74173E76DB9E53E73	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:41:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	929b5c6a170b659debfcaf05ec074967a528424d99520626221c90fd6c67c9fe	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1266:VUE-4962F0FEDA8B1497F741	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:5:3	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	5fed8b8de60fd2d6e1fbf9e1e80f4ff690e7d5d86b9e4897211c8a9991c00d70	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1267:VUE-48B71F787D60C795E354	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:50:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	e3ae39f6067f21478af45d9d804cb7792431643dab391badbe7ec01fc843510e	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1268:VUE-86A6DF9F5C863F3F9123	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:56:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	2371c79a7b44ea4c1482424dbc2e49f722ea5e8b2e77ef4e0e092b46e8ad07e0	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:1269:VUE-FB95A1CB9A1CD1625206	compiler-ssr	packages/compiler-ssr/__tests__/ssrElement.spec.ts:74:5	504093f35340ee7ff950f0907e07fb2d8202eea2	single-declaration	StringLiteral	91a1bbc18962499158d514da38afcd3bc3c684b94c8d8656a9feec16710e0c35	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-

exec
/bin/zsh -lc "rg --files docs/arch/refactor/rev11/evidence/framework-conformance | rg -i 'option|profile|vue' | head -n 100" in <repo-root>
 succeeded in 0ms:
docs/arch/refactor/rev11/evidence/framework-conformance/svelte-options.tsv
docs/arch/refactor/rev11/evidence/framework-conformance/option-inventories.md
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv
docs/arch/refactor/rev11/evidence/framework-conformance/oracles/vue/package.json
docs/arch/refactor/rev11/evidence/framework-conformance/oracles/vue/closure.tsv

exec
/bin/zsh -lc 'rg -n -i -S "compileScript|inlineTemplate|templateOptions|vapor|ssr" docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv docs/arch/refactor/rev11/evidence/framework-conformance/option-inventories.md | head -n 250' in <repo-root>
 succeeded in 0ms:
docs/arch/refactor/rev11/evidence/framework-conformance/option-inventories.md:8:  `packages/compiler-sfc/src/{parse,compileScript,compileTemplate,compileStyle}.ts`,
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:41:compiler-core:TransformOptions	ssrCssVars	derived	Derived from analyzed SFC CSS variables for server products.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:45:compiler-core:SharedTransformCodegenOptions	ssr	derived	Derived from requested RuntimeServer product.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:56:compiler-core:CodegenOptions	ssrRuntimeModuleName	supported canonical	Validated server module-source override capability.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:65:compiler-sfc:compileScript	id	derived	Stable SFC identity and scope domain.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:66:compiler-sfc:compileScript	isProd	derived	Derived from development/production profile.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:67:compiler-sfc:compileScript	sourceMap	derived	Derived from canonical map request.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:68:compiler-sfc:compileScript	babelParserPlugins	supported canonical	Validated ordered plugin inventory beyond language-derived plugins.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:69:compiler-sfc:compileScript	globalTypeFiles	host-resolved	Resolved canonical project files; requires project-aware route.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:70:compiler-sfc:compileScript	inlineTemplate	derived	Derived from requested assembly form.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:71:compiler-sfc:compileScript	genDefaultAs	supported canonical	Explicit assembly symbol only for the internal assembly contract.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:72:compiler-sfc:compileScript	templateOptions	derived	Constructed from the same canonical request.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:73:compiler-sfc:compileScript	hoistStatic	supported canonical	Explicit normalized script/template optimization.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:74:compiler-sfc:compileScript	propsDestructure	supported canonical	Explicit Vue macro semantics option.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:75:compiler-sfc:compileScript	fs	host-resolved	C3 project provider; never an arbitrary production callback.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:76:compiler-sfc:compileScript	customElement	supported canonical	Boolean or canonical matcher form normalized by B3.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:77:compiler-sfc:compileScript	vapor	derived	Derived from the selected Vue Vapor family.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:85:compiler-sfc:compileTemplate	vapor	derived	Derived from selected client family; not combined into a novel SSR backend.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:86:compiler-sfc:compileTemplate	ssr	derived	Derived from RuntimeServer request and overrides client compiler selection per RC.3.
docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:87:compiler-sfc:compileTemplate	ssrCssVars	derived	Derived from analyzed CSS variables.

exec
/bin/zsh -lc "rg --files | rg 'program-state\\.toml"'$|BF2.*reopen|reopen.*BF2|reopen'"' | head -n 200 && rg -n -S \"status.*REOPEN|REOPENED|reopen\" docs/arch/refactor/rev11/evidence/BF2 docs/arch/refactor/rev11/evidence | head -n 250" in <repo-root>
 succeeded in 0ms:
docs/arch/architecture-lock/ledger/program-state.toml
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-architecture.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-conformance.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-adversarial.md
docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md
docs/arch/refactor/rev11/evidence/BF2/second-reopen-ruling.md
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen1.md
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen2-fix-final/session-raw.txt
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen2-fix-final/run-session.sh
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen2-fix-final/README.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:3:# BF2 reopen re-review #2 — adversarial (perf/memory/stub-hunting)
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:37:Not separately exercised by new tests in this commit (pre-existing scope; not part of the reopen diff). Golden generation covers assembled SFC compilation implicitly via the 48 golden cells (`test/non-vacuous-arms.spec.mjs`, green). **PASS** (unchanged scope, not regressed).
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:60:**BLOCKING finding on the "strengthened beyond path existence to a git-hash content check" claim** — see §3.4. The content-hash comparison exists in `src/coverage-report.mjs` (`reEnumerateVueRows`/`reEnumerateSvelteRows`, `source_object-mismatch` branch) but **no test in the suite exercises a row whose path exists but whose blob/tree hash differs from the live checkout.** I disabled that exact branch in both functions and the full `coverage.spec.mjs` suite (5/5) still passed. This is the same defect *class* the reopen was called for (a claimed-strengthened check with no discriminating test proving it fires) — narrower in blast radius than the original hydration-zero-callers finding (the code path is real and does run in production `coverage-report.mjs` usage), but it is a real gap in the required self-test coverage for `FC-MANIFEST-001`'s re-enumeration proof. See §3.4 for full mutation evidence.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:137:(This directly proves `hydrateVue`'s mount call is exercised by a real caller with a discriminating assertion — the exact defect class the reopen was called for.)
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:162:**Why this is blocking, not just a nit:** the commit message and `manifest-classification-accounting.md` both explicitly claim re-enumeration was "strengthened... to a git-hash content check plus closed-set field validation" as one of the concrete deliverables closing the reopen. The content-hash branch is real, live code that runs in production `coverage-report.mjs` usage — it is not a stub in the CLAUDE.md sense of an empty/constant body. But the self-test suite that is supposed to prove `FC-MANIFEST-001`'s re-enumeration exit contains **no case that would ever exercise or fail on this branch**: the only negative control (`BF2-SELFTEST-BOGUS`) hits `path-not-tracked` before the hash comparison is ever reached. A regression here (e.g. accidentally deleting the elif, or a future refactor that silently drops the check) would go completely undetected by the gate, letting a row whose real content silently drifted from the pinned tree still count as `resolvable`. This is precisely the class of defect the reopen was convened to close (a claimed-strengthened guarantee with zero discriminating proof) — narrower in severity than the original "zero callers" finding since the mechanism does run in real usage, but it is a genuine gap in the required self-test coverage.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:184:**Overall: BLOCKING_FINDINGS.** One concrete gap: the re-enumeration content-hash (`source_object-mismatch`) branch in both `reEnumerateVueRows` and `reEnumerateSvelteRows` is live production logic with zero discriminating test coverage — proven by disabling it and observing the full `test/coverage.spec.mjs` suite (including the corrupted-locator negative control) stay green. Recommend: add a negative-control row to `test/coverage.spec.mjs`'s "deliberately corrupted locator" describe block (or a sibling test) that references a real tracked path in the pinned checkout but supplies a deliberately wrong `source_object` hash, and assert it lands in `unresolvable` with `resolvable` decremented accordingly — mirroring the existing pattern but targeting the hash-mismatch branch specifically instead of the path-not-tracked branch. This is a small, bounded fix (test-only, no production-code change) and does not require reopening any other part of this review.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:25:  during the BF2 reopen before BF2 is re-reviewed and accepted.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:174:or environment change may reopen the lock through the ordinary blind recalibration
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:192:acceptance of BF2's other reopened findings or as authority to expose BF3; those
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:3:# BF2 reopen re-review — ARCHITECTURE seat
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:51:resolution failure incl. the named-export-existence strengthening item 3(a) from the reopen
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:55:reopen item 3(b) ("Svelte: flags code that throws...", "Svelte: succeeds for real, correct
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:66:14 sub-tests, all **PASS**, including the six reopen-item-3(d) additions confirmed present by
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:76:and `context-packet-reopen1.md` explicitly cite `FC-MANIFEST-001` and tie it to
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:129:  pairing #1 real success + negative control). This is reopen item 3(c) — the previously
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:157:## Specifically-requested reopen-history verification
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:219:specifically-requested reopen-history item is independently re-verified and found genuinely
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md:1:# BF2 reopen #1 — CONFORMANCE re-review
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md:271:## Specifically-flagged reopen-history items (verified fixed, not trusted from commit message)
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md:308:docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md:311:docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md:318:the reopen-1 packet's stated non-goal not to touch it was itself
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md:327:reopen-history item flagged for this pass is individually PASS with cited,
docs/arch/refactor/rev11/evidence/BF2/second-reopen-ruling.md:1:# BF2 second-reopen ruling record
docs/arch/refactor/rev11/evidence/BF2/second-reopen-ruling.md:43:Full ruling text: `.agent-run/bf2-second-reopen-consult-output.log` (program
docs/arch/refactor/rev11/evidence/BF2/manifest-classification-accounting.md:1:# BF2 reopen #1 — manifest classification accounting (honest, explicit)
docs/arch/refactor/rev11/evidence/BF2/manifest-classification-accounting.md:3:Written as part of item 2 of the reopen fix pass. Records exactly what this
docs/arch/refactor/rev11/evidence/A5/context-packet.md:67:original block. Do NOT reopen or re-litigate any decision (L1, G1, DD1, R-12, S1, P3) — only correct the
docs/arch/refactor/rev11/evidence/A5/context-packet.md:154:- Do not reopen decisions L1/G1/DD1/R-12/S1/P3 — R-12 may get a restated justification (ADV-2) but the
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:1:# BF2 reopen #2 — final evidence summary (rounds 2–7, landed round 7)
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:6:Base (start of reopen #2): `0c0c6bc7870ec1edcbfcb966fcd6fde9e666d63f`.
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:20:- `00451700f` — second-reopen fix (all ten failing criteria from the reopen
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:37:| Conformance | PASS | `reviews/reopen2-round7-conformance.md` | `d9ff2e06498ec01c1bf2935846fab78187bd1249a6b1b69ebb4b08c26197c465` |
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:38:| Architecture | PASS | `reviews/reopen2-round7-architecture.md` | `58f1e24d534186994f2208fce73ad9865e8113da2eabc8956a184d9f64a930ac` |
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:39:| Adversarial | BLOCKING_FINDINGS (one narrow finding) | `reviews/reopen2-round7-adversarial.md` | `3cbe1924861896cdda43f9815561682d9800eb7d576c10ca35da9334f4fb025e` |
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:92:  reopen #2)
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:101:- `maintainer_decision`: coordinator to attach the second-reopen circuit-
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:3:# BF2 reopen re-review #2 — adversarial (perf/memory/stub-hunting)
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:37:Not separately exercised by new tests in this commit (pre-existing scope; not part of the reopen diff). Golden generation covers assembled SFC compilation implicitly via the 48 golden cells (`test/non-vacuous-arms.spec.mjs`, green). **PASS** (unchanged scope, not regressed).
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:60:**BLOCKING finding on the "strengthened beyond path existence to a git-hash content check" claim** — see §3.4. The content-hash comparison exists in `src/coverage-report.mjs` (`reEnumerateVueRows`/`reEnumerateSvelteRows`, `source_object-mismatch` branch) but **no test in the suite exercises a row whose path exists but whose blob/tree hash differs from the live checkout.** I disabled that exact branch in both functions and the full `coverage.spec.mjs` suite (5/5) still passed. This is the same defect *class* the reopen was called for (a claimed-strengthened check with no discriminating test proving it fires) — narrower in blast radius than the original hydration-zero-callers finding (the code path is real and does run in production `coverage-report.mjs` usage), but it is a real gap in the required self-test coverage for `FC-MANIFEST-001`'s re-enumeration proof. See §3.4 for full mutation evidence.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:137:(This directly proves `hydrateVue`'s mount call is exercised by a real caller with a discriminating assertion — the exact defect class the reopen was called for.)
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:162:**Why this is blocking, not just a nit:** the commit message and `manifest-classification-accounting.md` both explicitly claim re-enumeration was "strengthened... to a git-hash content check plus closed-set field validation" as one of the concrete deliverables closing the reopen. The content-hash branch is real, live code that runs in production `coverage-report.mjs` usage — it is not a stub in the CLAUDE.md sense of an empty/constant body. But the self-test suite that is supposed to prove `FC-MANIFEST-001`'s re-enumeration exit contains **no case that would ever exercise or fail on this branch**: the only negative control (`BF2-SELFTEST-BOGUS`) hits `path-not-tracked` before the hash comparison is ever reached. A regression here (e.g. accidentally deleting the elif, or a future refactor that silently drops the check) would go completely undetected by the gate, letting a row whose real content silently drifted from the pinned tree still count as `resolvable`. This is precisely the class of defect the reopen was convened to close (a claimed-strengthened guarantee with zero discriminating proof) — narrower in severity than the original "zero callers" finding since the mechanism does run in real usage, but it is a genuine gap in the required self-test coverage.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md:184:**Overall: BLOCKING_FINDINGS.** One concrete gap: the re-enumeration content-hash (`source_object-mismatch`) branch in both `reEnumerateVueRows` and `reEnumerateSvelteRows` is live production logic with zero discriminating test coverage — proven by disabling it and observing the full `test/coverage.spec.mjs` suite (including the corrupted-locator negative control) stay green. Recommend: add a negative-control row to `test/coverage.spec.mjs`'s "deliberately corrupted locator" describe block (or a sibling test) that references a real tracked path in the pinned checkout but supplies a deliberately wrong `source_object` hash, and assert it lands in `unresolvable` with `resolvable` decremented accordingly — mirroring the existing pattern but targeting the hash-mismatch branch specifically instead of the path-not-tracked branch. This is a small, bounded fix (test-only, no production-code change) and does not require reopening any other part of this review.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:25:  during the BF2 reopen before BF2 is re-reviewed and accepted.
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:174:or environment change may reopen the lock through the ordinary blind recalibration
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:192:acceptance of BF2's other reopened findings or as authority to expose BF3; those
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:3:# BF2 reopen re-review — ARCHITECTURE seat
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:51:resolution failure incl. the named-export-existence strengthening item 3(a) from the reopen
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:55:reopen item 3(b) ("Svelte: flags code that throws...", "Svelte: succeeds for real, correct
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:66:14 sub-tests, all **PASS**, including the six reopen-item-3(d) additions confirmed present by
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:76:and `context-packet-reopen1.md` explicitly cite `FC-MANIFEST-001` and tie it to
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:129:  pairing #1 real success + negative control). This is reopen item 3(c) — the previously
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:157:## Specifically-requested reopen-history verification
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md:219:specifically-requested reopen-history item is independently re-verified and found genuinely
docs/arch/refactor/rev11/evidence/BF2/second-reopen-ruling.md:1:# BF2 second-reopen ruling record
docs/arch/refactor/rev11/evidence/BF2/second-reopen-ruling.md:43:Full ruling text: `.agent-run/bf2-second-reopen-consult-output.log` (program
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md:1:# BF2 reopen #1 — CONFORMANCE re-review
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md:271:## Specifically-flagged reopen-history items (verified fixed, not trusted from commit message)
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md:308:docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md:311:docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md:318:the reopen-1 packet's stated non-goal not to touch it was itself
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md:327:reopen-history item flagged for this pass is individually PASS with cited,
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:1:# BF2 reopen #2 — final evidence summary (rounds 2–7, landed round 7)
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:6:Base (start of reopen #2): `0c0c6bc7870ec1edcbfcb966fcd6fde9e666d63f`.
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:20:- `00451700f` — second-reopen fix (all ten failing criteria from the reopen
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:37:| Conformance | PASS | `reviews/reopen2-round7-conformance.md` | `d9ff2e06498ec01c1bf2935846fab78187bd1249a6b1b69ebb4b08c26197c465` |
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:38:| Architecture | PASS | `reviews/reopen2-round7-architecture.md` | `58f1e24d534186994f2208fce73ad9865e8113da2eabc8956a184d9f64a930ac` |
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:39:| Adversarial | BLOCKING_FINDINGS (one narrow finding) | `reviews/reopen2-round7-adversarial.md` | `3cbe1924861896cdda43f9815561682d9800eb7d576c10ca35da9334f4fb025e` |
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:92:  reopen #2)
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md:101:- `maintainer_decision`: coordinator to attach the second-reopen circuit-
docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md:13:consult, dispatched during BF2's reopen review, independently re-examined
docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md:19:  reopen (2026-08-12). Full ruling text preserved at
docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md:65:The maintainer declined to invoke this machinery for BF2's reopen and
docs/arch/refactor/rev11/evidence/BF2/manifest-classification-accounting.md:1:# BF2 reopen #1 — manifest classification accounting (honest, explicit)
docs/arch/refactor/rev11/evidence/BF2/manifest-classification-accounting.md:3:Written as part of item 2 of the reopen fix pass. Records exactly what this
docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md:13:consult, dispatched during BF2's reopen review, independently re-examined
docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md:19:  reopen (2026-08-12). Full ruling text preserved at
docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md:65:The maintainer declined to invoke this machinery for BF2's reopen and
docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md:1:# BF2 reopen #1 — pre-dispatch context packet
docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md:4:`work/bf2-reopen-fix`. This is the pre-dispatch record of scope and intent for
docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md:5:the reopened BF2 fix pass — not a post-hoc summary.
docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md:7:## Why this reopen exists
docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md:47:Intent: close four concrete gaps found by the reopen finding —
docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md:1:# BF2 reopen #1 — pre-dispatch context packet
docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md:4:`work/bf2-reopen-fix`. This is the pre-dispatch record of scope and intent for
docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md:5:the reopened BF2 fix pass — not a post-hoc summary.
docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md:7:## Why this reopen exists
docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md:47:Intent: close four concrete gaps found by the reopen finding —
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen1.md:1:# BF2 reopen #1 — evidence summary (round 2, post-fallback)
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen1.md:18:   `docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md`)
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen1.md:33:   (`context-packet-reopen1.md`, committed before any fix code) plus full
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen1.md:1:# BF2 reopen #1 — evidence summary (round 2, post-fallback)
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen1.md:18:   `docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md`)
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen1.md:33:   (`context-packet-reopen1.md`, committed before any fix code) plus full
docs/arch/refactor/rev11/evidence/framework-conformance/performance-impact.md:11:root), closing the corresponding row of BF1's reopened exit criterion #6 for these two
docs/arch/refactor/rev11/evidence/B1/command-proofs/01-gate.txt:556:[> 60.000s] verter_session resolver_store::store_view_marginal_admit_tests::marginal_admit_reopens_no_routing_regardless_of_host_size
docs/arch/refactor/rev11/evidence/B1/command-proofs/01-gate.txt:562:[>120.000s] verter_session resolver_store::store_view_marginal_admit_tests::marginal_admit_reopens_no_routing_regardless_of_host_size
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen2-fix-final/README.md:1:# Final-candidate execution (second-reopen fix tree) — BF2_VUE_ORACLE_MANIFEST_GENERATE / BF2_SVELTE_ORACLE_MANIFEST_GENERATE
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen2-fix-final/README.md:4:run against the second-reopen FIX candidate (session-recorded
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen2-fix-final/README.md:1:# Final-candidate execution (second-reopen fix tree) — BF2_VUE_ORACLE_MANIFEST_GENERATE / BF2_SVELTE_ORACLE_MANIFEST_GENERATE
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen2-fix-final/README.md:4:run against the second-reopen FIX candidate (session-recorded

codex
The repository already contains a prior BF2 second-reopen ruling, so I’m using that precedent as the process model. It should tell us whether this must be a BF2 reopen, how comprehensive the repair must be, and what downstream evidence gets invalidated.
exec
/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/evidence/BF2/second-reopen-ruling.md && nl -ba docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md | sed -n '1,130p' && nl -ba docs/arch/architecture-lock/ledger/program-state.toml | sed -n '250,360p'" in <repo-root>
 succeeded in 0ms:
     1	# BF2 second-reopen ruling record
     2	
     3	**Ruling reference:** `REV11-BF2-REOPEN2-001` (root cause + approach), followed by
     4	`REV11-BF2-REOPEN2-002` (round-3 review arbitration). Both Codex Sol xhigh, read-only
     5	investigation, dispatched by the program orchestrator after BF2's second invalidation
     6	at candidate `58d42a65d` (ledger tip `79ce71054`).
     7	
     8	## REV11-BF2-REOPEN2-001 — root cause + approach (authorizes rounds 2 onward)
     9	
    10	**Root cause:** the hardened per-criterion review instruction improved evidence
    11	citation but did not require evidence *adequacy* — it explicitly allowed "exact test
    12	name + pass/fail" as evidence, so reviewers proved tests ran without checking whether
    13	the cited assertion was coextensive with the full contract (quantifier words like
    14	"every"/"all"/"only"/"exact"/"offline"/"atomic" were accepted from narrow/sampled
    15	tests). Compounding failures: test names accepted as category proofs without
    16	per-category accuracy; aggregate charter criteria not atomized consistently across
    17	the three reports (12 vs 11 vs mandate-specific groupings), letting one covered
    18	subpart mask two uncovered ones; review scope anchored to the fix diff instead of
    19	the whole candidate; a README's disclaimer was allowed to override a ratified
    20	charter's owned scope; the three review mandates were correlated rather than
    21	complementary (same checklist/test vocabulary/known-fix focus); and no durable
    22	three-mandate PASS existed for one unchanged candidate identity (reports drifted
    23	across `a7f1eb5d7` vs the accepted `58d42a65d`).
    24	
    25	**Approach ruled: (b)** — fix all 10 failing criteria fully in BF2 itself; none may
    26	be deferred to B2/B4/BV1/BS1. BF2 owns the oracle/validator those blocks will be
    27	judged BY — deferring a broken oracle downstream would poison every consumer's
    28	evidence. Existing legitimate downstream manifest-row allocations (BV1/BS1/B2
    29	splits) were unaffected and correct as-is.
    30	
    31	**Review-method correction prescribed** (superseded `.agent-run/BF2-REVIEW2-COMMON.md`,
    32	applied to every review from round 2 onward): canonical numbered acceptance matrix
    33	(no combined rows); 10 required fields per row (authority quote, complete domain,
    34	every implementing path, exact test inputs/assertions, contract fields actually
    35	asserted, positive witness, negative witness, reviewer-authored counterexample/fault
    36	injection, exact command+skip-count+digest, verdict); a green test name never
    37	sufficient alone; adversarial reviewer must author fresh black-box probes for
    38	defect-prone families and perform white-box kill tests per mechanism; no
    39	PASS-with-caveat (missing evidence is NOT_PROVEN, violated behavior is
    40	BLOCKING_FINDINGS); full fresh review on the final candidate, no partial-diff-only
    41	review, no approval carried forward across a changed candidate identity.
    42	
    43	Full ruling text: `.agent-run/bf2-second-reopen-consult-output.log` (program
    44	orchestrator's session transcript, not committed verbatim — this file is the durable
    45	record of its substance and disposition).
    46	
    47	## REV11-BF2-REOPEN2-002 — round-3 review arbitration
    48	
    49	Round 3 (candidate `00451700f`) produced a genuine 3-way reviewer disagreement:
    50	conformance 2/16 rows blocking, adversarial 4/16, architecture 10/16 (including a
    51	Row 15/manifest-completion claim that directly contradicted this same ruling
    52	authority's own REOPEN2-001 text). Independently re-verified each disputed claim
    53	with its own kill-mutation probes rather than voting. Verdict: neither reviewer was
    54	fully right — result was 9 real items (not 2/16 or 10/16), with explicit dismissals
    55	justified per item (Row 15 remained PASS; the architecture report's own text
    56	contradicted the round summary's claim of blocking it). Also issued 3 review-method
    57	corrections: freeze row interpretations/downstream-ownership boundaries alongside
    58	the 16 labels; use a common kill-ledger vocabulary distinguishing mechanism logic vs
    59	production callsite vs atomic-commit primitive vs reader schedule; serialize any
    60	performance-cell run under an exclusive machine lease.
    61	
    62	Full ruling text: `.agent-run/bf2-round3-arbitration-output.log` (program
    63	orchestrator's session transcript; this file is the durable record).
    64	
    65	## Rounds authorized under these rulings
    66	
    67	`00451700f` (all 10 criteria) → `f878d9cdd` (arbitrated pass-4, REV11-BF2-REOPEN2-002
    68	disposition) → `a3753c87c` (round-4-convergent) → `19cce22c8` (round-5-convergent) →
    69	`41929246e` (round-6-convergent, landed). Each subsequent round was ordinary
    70	convergent narrowing under the review method these two rulings established — no
    71	further arbitration was required after round 3.
     1	# BF2 reopen #1 — pre-dispatch context packet
     2	
     3	Authored BEFORE any implementation change in this pass, as the first commit on
     4	`work/bf2-reopen-fix`. This is the pre-dispatch record of scope and intent for
     5	the reopened BF2 fix pass — not a post-hoc summary.
     6	
     7	## Why this reopen exists
     8	
     9	A Codex Sol xhigh parallelism consult independently re-examined BF2's exit
    10	criteria and found the prior 3/3 PASS review verdict was wrong on all three
    11	review mandates (conformance, architecture, adversarial). The prior
    12	`accepted_sha`/`accepted_tree` (9932ae15e / 30fb53f18) was invalidated by
    13	`docs/arch/architecture-lock/ledger/program-state.toml` at commit `0c0c6bc78`,
    14	which this branch is based on. See that commit's ledger notes for the full
    15	finding text.
    16	
    17	## Scope of this pass (ratified — items 2, 3, 4 only)
    18	
    19	Item 1 (the performance-gate freeze / `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE`
    20	cell) is EXPLICITLY OUT OF SCOPE for this pass — it is being resolved separately
    21	via a dedicated architecture consult. `performance-gates.toml` is not touched here.
    22	
    23	### Item 2 — manifest/source proofs must actually run and pass, not skip
    24	
    25	The prior candidate shipped 43 passed / 6 skipped. The 6 skips are exactly the
    26	required FC-MANIFEST-001 proofs: pinned-source-drift tests
    27	(`test/drift-refusal.spec.mjs`), Vue/Svelte runner re-enumeration and
    28	corrupted-locator discrimination (`test/coverage.spec.mjs`). They skip because
    29	`BF2_VUE_SOURCE`/`BF2_SVELTE_SOURCE` point at real pinned git checkouts of Vue
    30	3.6.0-rc.3 and Svelte 5.56.8 that were never present in a bare worktree.
    31	
    32	Intent: obtain real hermetic pinned checkouts (matching `src/domain-pin.mjs`
    33	commit/tree exactly, verified via `git rev-parse`), point the env vars at
    34	them, and get all 6 tests to execute and pass for real — not stay skipped.
    35	Strengthen `coverage-report.mjs`'s re-enumeration check to verify case/title
    36	hash, profile, and product-evidence fields per manifest row, not just
    37	file/directory existence. Finally, use the now-fully-working harness to
    38	classify the Vue (2003 rows) and Svelte (3313 blocked + 144 not_applicable
    39	rows) manifests for real, per AMD-005 — mechanically via
    40	`bin/coverage-report.mjs`/`bin/generate-goldens.mjs`, writing real evidence
    41	IDs, not fabricated completion. If the full ~5460-row set cannot be honestly
    42	completed in this pass, the packet commits to leaving an explicit, honest
    43	accounting of what remains and why.
    44	
    45	### Item 3 — harness/normalizer gaps
    46	
    47	Intent: close four concrete gaps found by the reopen finding —
    48	(a) `src/compare.mjs` real-package link validity currently only calls
    49	`require.resolve()`, which would pass a genuinely-missing named export;
    50	strengthen it to check the actual imported names exist on the resolved
    51	module's exports.
    52	(b) Svelte SSR execution (`src/execute-svelte-runtime.mjs`) has zero
    53	self-test; add one mirroring the existing Vue self-test pattern in
    54	`test/failure-detection.spec.mjs` (throws-on-error + succeeds-on-real-SSR).
    55	(c) Both hydration-control entry points (`hydrateVue`, `hydrateSvelteClient`
    56	in `src/hydration.mjs`) have zero test/CLI callers; add real self-tests
    57	driving each against real golden SSR output, and correct the README's
    58	"implemented but not yet exercised" wording once true exercise exists (or
    59	leave the honest wording if genuinely out of reach this pass).
    60	(d) The normalizer suite (`test/normalizer-mutations.spec.mjs`) lacks
    61	distinct mutation tests for several forbidden categories (import/export
    62	source, events, component calls, slots, authored/public names, control-flow);
    63	add them following the file's existing real-mutation-plus-detection pattern.
    64	
    65	### Item 4 — evidence custody
    66	
    67	This packet itself is the fix for the "context packet was authored
    68	post-implementation" half of the finding — it is committed first, before any
    69	other change, so its position in git history is the proof it is pre-dispatch.
    70	The other half — durable digest-addressed storage of full review-report
    71	content — is a process instruction for the reviewers who evaluate this pass's
    72	output: they must commit their full report text into the repository (e.g.
    73	under `docs/arch/refactor/rev11/evidence/BF2/reviews/`), not leave it only in
    74	an ephemeral worker worktree. This implementer pass does not itself produce
    75	those reports.
    76	
    77	## Non-goals / explicit exclusions for this pass
    78	
    79	- No `.rs` file is touched.
    80	- `performance-gates.toml` and the
    81	  `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` cell are not touched.
    82	- No package outside `packages/framework-conformance-harness/` is modified,
    83	  except this evidence document and the harness's own `README.md`.
    84	- No scope creep beyond items 2–4 as enumerated above.
    85	
    86	## Verification plan
    87	
    88	`pnpm --filter @verter/framework-conformance-harness test` must go from
    89	43 passed / 6 skipped to a run where the 6 previously-skipped tests, plus the
    90	new tests added for item 3, all execute and pass. Full command output is
    91	captured in the final report at `.agent-run/BF2-FIX-REPORT.md` (gitignored,
    92	not part of the durable evidence trail).
   250	stack_layer = 0
   251	conformance_review = "PASS"
   252	architecture_review = "PASS"
   253	adversarial_review = "PASS"
   254	maintainer_decision = "ACCEPTED"
   255	notes = "verter_identity crate (layer 1): StableEntityId/SessionHandle, QueryIdentity/SemanticFlightKey/InputBasisId non-interchangeable, ExecutionPolicy profile schema, canonical encoding. Whole-workspace closure-based dependency-firewall test with A5-DD1 equality-pinned exception (target-condition-precise, C1 removal gate). Deleted verter_audit_no_upward_deps + no_session_dep.rs; kept audit_substrate_isolation as named grandfathered guard (explicit decision). Landed as true linear fast-forward: accepted_tree == candidate_tree, no landing_equivalence artifact needed. Reviewed candidate preserved from GC via tag program-history/B1-reviewed-candidate. Prerequisite: feat(gate) memory-ceiling tooling landed separately at 5c24d22a5 (not a ledger block) after this machine's 2nd OOM incident this program; real capped canonical gate run (--build-jobs 2 --test-threads 2 --memory-limit 8GiB) terminal VERDICT with exactly 2 pre-existing load-sensitive flakes independently proven non-causal via isolated reruns on both baseline and candidate trees; one bounded fix cycle (stale discrimination-proof regenerated, mislabeled evidence file corrected), reattested PASS. Non-blocking discovery, routed not resolved: plausible (unconfirmed) RSS-driver lead in verter_session SURFACE 2 OnceLock-memoized syn-AST scans (architecture_guards.rs, output_projector_residual_guards.rs) — recommended as its own investigation thread."
   256	
   257	[[block]]
   258	id = "BF1"
   259	status = "ACCEPTED"
   260	charter_digest = "ef45ad3e19f74273ded8e41b43b31b239fda84b6eb13924ed887a5a5a8e6b98c"
   261	context_packet_digest = "9b10b622c58c3b3256ab2b3e85df89b773e3ec8d339e34bf30ed89999b65c0e3"
   262	base_sha = "8958f69e92736dc0947c99acff581a779b61a871"
   263	candidate_sha = "6305950725abc2265135694f353b8fb75ef9c9c5"
   264	candidate_tree = "dfea0c71b2795ff10210fb2485fe1d70f0464f6a"
   265	accepted_sha = "6305950725abc2265135694f353b8fb75ef9c9c5"
   266	accepted_tree = "dfea0c71b2795ff10210fb2485fe1d70f0464f6a"
   267	landing_equivalence_digest = ""
   268	evidence_digest = "68682304450082c1902083f86e85efaa316857d0b8b644c48526bd03f25ab537"
   269	stack_id = ""
   270	stack_snapshot_digest = ""
   271	stack_layer = 0
   272	conformance_review = "PASS"
   273	architecture_review = "PASS"
   274	adversarial_review = "PASS"
   275	maintainer_decision = "ACCEPTED"
   276	notes = "Gap-analysis-only block: every one of BF1's 7 numbered exit criteria and all owned-scope bullets independently verified already satisfied by the previously-landed, maintainer-ratified AMD-005 package. REOPEN #1 (legitimate, new finding not previously reviewed): the parallelism consult found exit criterion #6 (BF2 performance cells frozen before BF2 work begins) was unmet — performance-gates.toml had no BF2 cells. Resolved by measuring the already-authored, BF1-owned official-case enumeration tool (generate-official-case-manifests.mjs) against the pinned oracle sources — a legitimate before-BF2-exists reference measurement, not criterion-selection-after-candidate-measurement — freezing BF2_VUE_ORACLE_MANIFEST_GENERATE/BF2_SVELTE_ORACLE_MANIFEST_GENERATE (10-invocation session, wall/RSS bounds, SHA-256 output-oracle, exact-equality work counters, operationally-proven zero-network via a live curl-denial control). First review pass: architecture BLOCKING (cells mislabeled as covering golden/compiler-invocation generation, which was never measured); fixed by rescoping the operation text to enumeration/classification only and adding an explicit open/deferred row in performance-impact.md for the heavier compiler-invocation+golden workload (matching the existing BV1/BS1/B6/C4 deferred-row pattern). Reattested PASS, no threshold values changed, no scope creep. candidate_sha == accepted_sha exactly (no landing squash needed beyond the one already-squashed commit). Prior review record: architecture PASS (DAG sequencing B1->BF1->BF2->BF3->{B2,B3} confirmed coherent, zero production diff, emitter-mapping dispositions spot-checked), adversarial found one BLOCKING governance finding (AMD-005 §15.1's ratification quotation cited the wrong reviewed-package SHA — pre-fix ce1d0e4688 instead of PASS-reattested 7442bb9060; citation-only, substance always correct) — escalated per STOP conditions, architecture-authorized as non-discretionary record repair (no new maintainer ratification required, governance §10), landed as standalone commit f1b59d2dd. AMD-005 (Framework Compiler Conformance Rescope) itself maintainer-ratified and landed at 34fe8167f (housekeeping squash of the original four-commit landing sequence — content byte-identical, tree unchanged; superseded pre-fix candidate ce1d0e4688/1ff1f83d8e, reviewed package 7442bb9060/69502487b, ratified bundle aa757eecc/0c078357b, original ratification-record commit ade8a301b, preserved on branch backup/pre-amd005-squash). BF2 next legal once this transition is validated."
   277	
   278	[[block]]
   279	id = "BF2"
   280	status = "ACCEPTED"
   281	charter_digest = "1f99cf7eda1a955ada751f075799dabc8c8ab1defda19b20375f7ca09aa5963b"
   282	context_packet_digest = "52c927f3ddb197590181126dff178ec3eab1cd997036b7be156462ad15d11b67"
   283	base_sha = "0c0c6bc7870ec1edcbfcb966fcd6fde9e666d63f"
   284	candidate_sha = "c3a83667edfd869b3e93c8a63aa3e475729d598f"
   285	candidate_tree = "19594cbd98888a86e27c8991a453917f3ae4e9f6"
   286	accepted_sha = "c3a83667edfd869b3e93c8a63aa3e475729d598f"
   287	accepted_tree = "19594cbd98888a86e27c8991a453917f3ae4e9f6"
   288	landing_equivalence_digest = ""
   289	evidence_digest = "9f67b09c8548361a350449c28186c1dd6724500443571756b230ad602bee241a"
   290	stack_id = ""
   291	stack_snapshot_digest = ""
   292	stack_layer = 0
   293	conformance_review = "PASS"
   294	architecture_review = "PASS"
   295	adversarial_review = "PASS"
   296	maintainer_decision = "ACCEPTED"
   297	notes = "REOPEN #2 RESOLVED (7 total rounds across two invalidations). Second-reopen circuit breaker satisfied by two Codex xhigh rulings, durably recorded at evidence/BF2/second-reopen-ruling.md: REV11-BF2-REOPEN2-001 (root cause — hardened per-criterion citation still allowed weak assertions to satisfy strong contract quantifiers; approach ruled: fix all 10 failing criteria fully IN BF2, none deferred, since BF2 owns the oracle/validator later blocks are judged BY; prescribed a stricter 16-row-matrix + kill-mutation + fresh-adversarial-probe review method) and REV11-BF2-REOPEN2-002 (round-3 3-way reviewer disagreement arbitrated by independent re-verification, not a vote — resolved to 9 real items, dismissed a reviewer claim that contradicted the ruling authority's own prior text). Rounds 2-6 (00451700f/f878d9cdd/a3753c87c/19cce22c8/41929246e) each closed convergent findings under that method — real correctness defects fixed: normalizer no longer erases authored/public identifiers or semantic comments; closure-verification gate now hashes actual package content and is genuinely fail-closed before oracle load (was detect-after-the-fact, and a same-process memo bypass skipped both gates entirely); Svelte torn-tree gate now resolves the actual svelte/compiler load specifier, not just the package root; TypeScript observation covers call/construct/index signatures; Svelte hydration detection replaced with a real signal comparison covering text-root/marker-loss cases; realization-lock exclusion test made deterministic instead of a timing race; full export/re-export linking, atomic golden-set publication (single commit point, stale-reader-safe), diagnostic/mapping field comparison, and category-accurate normalizer mutation tests all closed. Round 7 (targeted rows 1+4 only, rows 2/3/5-16 already settled): conformance PASS, architecture PASS, adversarial found one narrow non-substantive finding (new B3 hydration test lacked a timeout, flaked under full-suite contention) — folded in directly as a mechanical zero-assertion-change fix under standing judgment for small clearly-in-scope convergent findings (matches an already-reviewed sibling fix pattern in the same commit), independently verified 226/226 across 4 runs by the program orchestrator, not dispatched as a fresh round 8. Landed as one squashed commit (history of 6 fix commits + evidence, tree-verified lossless against the original chain) per program branch hygiene. Predecessor BF1 remains ACCEPTED at 6305950725abc2265135694f353b8fb75ef9c9c5, unaffected. BF3 next legal once this transition is validated. J1 stays locked (CSS work not selected)."
   298	
   299	[[block]]
   300	id = "BV0"
   301	status = "READY"
   302	charter_digest = "572fc21cce211cd63622ec50207d7b11cf687b7db62ff1270d82b6d0f2e0f306"
   303	context_packet_digest = ""
   304	base_sha = ""
   305	candidate_sha = ""
   306	candidate_tree = ""
   307	accepted_sha = ""
   308	accepted_tree = ""
   309	landing_equivalence_digest = ""
   310	evidence_digest = ""
   311	stack_id = ""
   312	stack_snapshot_digest = ""
   313	stack_layer = 0
   314	conformance_review = "PENDING"
   315	architecture_review = "PENDING"
   316	adversarial_review = "PENDING"
   317	maintainer_decision = "PENDING"
   318	notes = "Introduced and ratified by AMD-006 (landed fdb6f62911fffc43c7f64610033bdf2e8a591fbf). Predecessor BF2 ACCEPTED at c3a83667edfd869b3e93c8a63aa3e475729d598f — BV0 is execution-legal. Owns immediate, bounded, source-root-cause correction of the genuine Vue VDOM/Vapor/SSR defects BF2's exact rc.3 seed matrix exposed (missing non-inline <script setup> __expose() emission, VDOM slot-fallback static caching/CACHED patch flag, and the remaining genuine seed-matrix defects named in the AMD-006 deviation memo). Must NOT introduce B3's canonical request, B4's publication architecture, a universal IR, a temporary typed refusal, or BV1's complete official-case pack. Maintainer direction applies: production stays correct via real fixes (TDD: failing regression first, then root-cause correction) — no guard, tracker, waiver, or retraction artifact of any kind. B2/B3 wait on both BV0 and BF3 acceptance."
   319	
   320	[[block]]
   321	id = "BF3"
   322	status = "READY"
   323	charter_digest = "7b537a95656ec2ae49acfc0d9813b0183f3625b22f921548ee072db033233e12"
   324	context_packet_digest = ""
   325	base_sha = ""
   326	candidate_sha = ""
   327	candidate_tree = ""
   328	accepted_sha = ""
   329	accepted_tree = ""
   330	landing_equivalence_digest = ""
   331	evidence_digest = ""
   332	stack_id = ""
   333	stack_snapshot_digest = ""
   334	stack_layer = 0
   335	conformance_review = "PENDING"
   336	architecture_review = "PENDING"
   337	adversarial_review = "PENDING"
   338	maintainer_decision = "PENDING"
   339	notes = "Predecessor BF2 ACCEPTED at c3a83667edfd869b3e93c8a63aa3e475729d598f. Narrowed by ratified AMD-006 to Svelte and non-Vue-runtime scope; Vue VDOM/Vapor/SSR findings reassigned to BV0 as immediate-fix work (maintainer direction: production stays correct, wrong output is a bug to fix, not grounds for a retraction/tracking mechanism — see evidence/vue-known-defect-correction/deviation-memo.md). BF3's own typed-non-success/whole-cell-retraction mechanism is UNCHANGED and remains in force for its retained Svelte/non-Vue-runtime domain (Codex xhigh ruling: a general forward-looking rule does not retroactively repeal an already-ratified charter mechanism). BF3 dispatch additionally waits on BV0 acceptance (program-dag.toml: B2/B3 require both BV0 and BF3)."
   340	
   341	[[block]]
   342	id = "B2"
   343	status = "LOCKED"
   344	charter_digest = ""
   345	context_packet_digest = ""
   346	base_sha = ""
   347	candidate_sha = ""
   348	candidate_tree = ""
   349	accepted_sha = ""
   350	accepted_tree = ""
   351	landing_equivalence_digest = ""
   352	evidence_digest = ""
   353	stack_id = ""
   354	stack_snapshot_digest = ""
   355	stack_layer = 0
   356	conformance_review = "PENDING"
   357	architecture_review = "PENDING"
   358	adversarial_review = "PENDING"
   359	maintainer_decision = "PENDING"
   360	notes = ""

exec
/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/templates/program-state.template.toml | sed -n '35,58p' && rg -n -S \"REOPEN\" docs/arch/refactor/rev11/tools docs/arch/architecture-lock docs/arch/refactor/rev11/templates/program-state.template.toml | head -n 120" in <repo-root>
 succeeded in 0ms:
    35	provider = "REQUIRED_PROVIDER"
    36	fallback_or_substitution = false
    37	model_exception_accepted = false
    38	max_active_workers = 3
    39	max_open_stack_layers = 4
    40	stack_tool = "UNDECIDED_UNTIL_A6"
    41	stack_mode_policy = "UNDECIDED_UNTIL_A6"
    42	evidence_root = "REQUIRED_REPOSITORY_RELATIVE_PATH"
    43	
    44	# Status: LOCKED | READY | IN_PROGRESS | REVIEW | ACCEPTANCE_RECOMMENDED | ACCEPTED |
    45	#         BLOCKED | RESCOPE_REQUIRED | ABORTED | SUPERSEDED | PRIVATE_CHECKPOINT
    46	# Reviews: NOT_REQUIRED | PENDING | PASS | BLOCKING | NOT_PROVEN | INVALIDATED
    47	# candidate_sha/tree = exact reviewed cumulative candidate.
    48	# accepted_sha/tree = actual landed commit and full repository tree; they may differ
    49	# from the reviewed candidate after a base advance. landing_equivalence_digest binds
    50	# both base/candidate/accepted identities and exact candidate-delta/post-landing proof.
    51	
    52	[[block]]
    53	id = "A0"
    54	status = "READY"
    55	charter_digest = ""
    56	context_packet_digest = ""
    57	base_sha = ""
    58	candidate_sha = ""
rg: docs/arch/refactor/rev11/tools: No such file or directory (os error 2)
docs/arch/architecture-lock/ledger/program-state.toml:276:notes = "Gap-analysis-only block: every one of BF1's 7 numbered exit criteria and all owned-scope bullets independently verified already satisfied by the previously-landed, maintainer-ratified AMD-005 package. REOPEN #1 (legitimate, new finding not previously reviewed): the parallelism consult found exit criterion #6 (BF2 performance cells frozen before BF2 work begins) was unmet — performance-gates.toml had no BF2 cells. Resolved by measuring the already-authored, BF1-owned official-case enumeration tool (generate-official-case-manifests.mjs) against the pinned oracle sources — a legitimate before-BF2-exists reference measurement, not criterion-selection-after-candidate-measurement — freezing BF2_VUE_ORACLE_MANIFEST_GENERATE/BF2_SVELTE_ORACLE_MANIFEST_GENERATE (10-invocation session, wall/RSS bounds, SHA-256 output-oracle, exact-equality work counters, operationally-proven zero-network via a live curl-denial control). First review pass: architecture BLOCKING (cells mislabeled as covering golden/compiler-invocation generation, which was never measured); fixed by rescoping the operation text to enumeration/classification only and adding an explicit open/deferred row in performance-impact.md for the heavier compiler-invocation+golden workload (matching the existing BV1/BS1/B6/C4 deferred-row pattern). Reattested PASS, no threshold values changed, no scope creep. candidate_sha == accepted_sha exactly (no landing squash needed beyond the one already-squashed commit). Prior review record: architecture PASS (DAG sequencing B1->BF1->BF2->BF3->{B2,B3} confirmed coherent, zero production diff, emitter-mapping dispositions spot-checked), adversarial found one BLOCKING governance finding (AMD-005 §15.1's ratification quotation cited the wrong reviewed-package SHA — pre-fix ce1d0e4688 instead of PASS-reattested 7442bb9060; citation-only, substance always correct) — escalated per STOP conditions, architecture-authorized as non-discretionary record repair (no new maintainer ratification required, governance §10), landed as standalone commit f1b59d2dd. AMD-005 (Framework Compiler Conformance Rescope) itself maintainer-ratified and landed at 34fe8167f (housekeeping squash of the original four-commit landing sequence — content byte-identical, tree unchanged; superseded pre-fix candidate ce1d0e4688/1ff1f83d8e, reviewed package 7442bb9060/69502487b, ratified bundle aa757eecc/0c078357b, original ratification-record commit ade8a301b, preserved on branch backup/pre-amd005-squash). BF2 next legal once this transition is validated."
docs/arch/architecture-lock/ledger/program-state.toml:297:notes = "REOPEN #2 RESOLVED (7 total rounds across two invalidations). Second-reopen circuit breaker satisfied by two Codex xhigh rulings, durably recorded at evidence/BF2/second-reopen-ruling.md: REV11-BF2-REOPEN2-001 (root cause — hardened per-criterion citation still allowed weak assertions to satisfy strong contract quantifiers; approach ruled: fix all 10 failing criteria fully IN BF2, none deferred, since BF2 owns the oracle/validator later blocks are judged BY; prescribed a stricter 16-row-matrix + kill-mutation + fresh-adversarial-probe review method) and REV11-BF2-REOPEN2-002 (round-3 3-way reviewer disagreement arbitrated by independent re-verification, not a vote — resolved to 9 real items, dismissed a reviewer claim that contradicted the ruling authority's own prior text). Rounds 2-6 (00451700f/f878d9cdd/a3753c87c/19cce22c8/41929246e) each closed convergent findings under that method — real correctness defects fixed: normalizer no longer erases authored/public identifiers or semantic comments; closure-verification gate now hashes actual package content and is genuinely fail-closed before oracle load (was detect-after-the-fact, and a same-process memo bypass skipped both gates entirely); Svelte torn-tree gate now resolves the actual svelte/compiler load specifier, not just the package root; TypeScript observation covers call/construct/index signatures; Svelte hydration detection replaced with a real signal comparison covering text-root/marker-loss cases; realization-lock exclusion test made deterministic instead of a timing race; full export/re-export linking, atomic golden-set publication (single commit point, stale-reader-safe), diagnostic/mapping field comparison, and category-accurate normalizer mutation tests all closed. Round 7 (targeted rows 1+4 only, rows 2/3/5-16 already settled): conformance PASS, architecture PASS, adversarial found one narrow non-substantive finding (new B3 hydration test lacked a timeout, flaked under full-suite contention) — folded in directly as a mechanical zero-assertion-change fix under standing judgment for small clearly-in-scope convergent findings (matches an already-reviewed sibling fix pattern in the same commit), independently verified 226/226 across 4 runs by the program orchestrator, not dispatched as a fresh round 8. Landed as one squashed commit (history of 6 fix commits + evidence, tree-verified lossless against the original chain) per program branch hygiene. Predecessor BF1 remains ACCEPTED at 6305950725abc2265135694f353b8fb75ef9c9c5, unaffected. BF3 next legal once this transition is validated. J1 stays locked (CSS work not selected)."

codex
The correct scope is a coordinated BF2→BV0 reopen, not one enlarged BV0 commit. The Svelte migration must remain separate.

## Q1 — Oracle invocation and accepted predecessors

**Ruling: fixing `invoke-vue-oracle.mjs` and regenerating affected official goldens is BF2-owned work. It requires a formal BF2 reopen, not unilateral BV0 scope.**

BF2 expressly owns “offline official compiler invocation and immutable golden generation,” Vue script/template assembly, runtime validation, and mutation tests ([BF2 charter](docs/arch/refactor/rev11/charters/BF2.md:11)). BV0 is a successor that consumes that evidence and owns compiler source corrections ([BV0 charter](docs/arch/refactor/rev11/charters/BV0.md:13)). Letting the candidate-under-test repair and regenerate its own oracle inside BV0 would blur exactly that separation.

This is also the established BF2 reopen precedent: defects in the oracle/validator must be fixed in BF2 because later blocks are judged by it; deferring the correction poisons downstream evidence ([second-reopen ruling](docs/arch/refactor/rev11/evidence/BF2/second-reopen-ruling.md:25)).

The comprehensive BF2 correction must cover the entire option-propagation class:

- Carry the complete semantic profile into every official phase, not merely add `vapor: true` at one call site.
- Pass `vapor` and `templateOptions.ssr` correctly to `compileScript`.
- Verify parse, `compileScript`, `compileTemplate`, assembly, golden provenance, and runtime all use the same requested axes.
- Regenerate every affected official golden across the corpus. Because `ssr` was also omitted from `compileScript`, scope cannot be limited to Vapor: regenerate all Vapor and SSR/non-inline script-bearing products whose official output changes.
- Add independent JS and TS controls for `__vapor`, `defineVaporComponent`, SSR behavior, VDOM negatives, and runtime interop. The adversarial report establishes that candidate and golden were wrong identically and that the existing test locks in the defect ([adversarial review](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:103), [finding](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:252)).

**Acceptance effects:**

- **BF2:** its historical acceptance record remains an historical fact, but its present authority and evidence are invalidated. Reopen BF2, mark its reviews/evidence invalid, repair it, and obtain fresh conformance, architecture, and adversarial PASS on one exact candidate. This is a real oracle-basis change, so use a full fresh review, not a narrow BV0 recheck.
- **BF1:** do **not** reopen automatically. BF1 locked the domain and option contract before the harness existed and expressly did not implement the harness ([BF1 charter](docs/arch/refactor/rev11/charters/BF1.md:6)). Its inventory already says `compileScript.vapor` and `compileScript.templateOptions` are derived from the canonical request ([Vue options](docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:72)). Thus BF2 violated the valid BF1 lock; the lock itself was not falsified. Audit and rerun only any BF1 evidence that actually consumed BF2-generated output, if any.
- **BV0:** candidate `c40a1ca96` and all round-one evidence are obsolete. BV0 must be rebased onto the reaccepted BF2 oracle and evaluated afresh.
- **BF3/BV1/other successors:** invalidate or restack any unaccepted work based on the old BF2 identity. If anything downstream has already been accepted, audit whether its proof consumed affected Vue Vapor/SSR results and reopen only the affected evidence/approval. Preserve independently valid work, as the falsification contract requires ([architecture-falsification contract](docs/arch/refactor/rev11/contracts/architecture-falsification.md:98)).

Operationally, BF2 repair and BV0 correction may be developed as a bounded two-layer stack, but BF2 lands/reaccepts first; BV0 then restacks and revalidates. Governance permits contingent upper work but forbids acceptance before the predecessor lands ([governance](docs/arch/refactor/rev11/governance.md:6)).

## Q2 — Svelte migration

**Ruling: split it out. Mechanical inertness does not create charter authority.**

BV0 is explicitly Vue-only. The architecture review confirms the Svelte migration is independent of the Vue pin, touches production Svelte constants and thousands of corpus/package files, and materially obscures the Vue review surface ([architecture review](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:102)). BF3 was deliberately narrowed to Svelte and non-Vue-runtime work ([BF3 charter](docs/arch/refactor/rev11/charters/BF3.md:23)).

Move the migration to a separate authorized Svelte-domain block/commit. Assign it to BF3 only if a scoped closure review confirms BF3 owns these corpus, dependency, and production-version changes; otherwise create a small dedicated Svelte oracle-alignment block. It must not be folded into BF2 because BF2 is test-only and cannot change production compiler files.

This separation is mandatory even if every output byte is unchanged. Governance says one program block is not co-batched with unrelated changes in its landing delta ([governance](docs/arch/refactor/rev11/governance.md:283)).

## Q3 — Items 2–5

**Items 2–4 are wholly in BV0. Item 5’s acceptance obligation is in BV0, but shared-harness mechanism changes remain BF2-owned.**

- **Item 2, VDOM regressions:** directly in scope. BV0 must restore both formerly passing routes and delete the two waiver rows. The charter requires proof that unaffected routes retain their previous successful contract and forbids substituting a waiver ([BV0 charter](docs/arch/refactor/rev11/charters/BV0.md:32), [conformance finding](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md:195)).

- **Item 3, obsolete SSR document:** directly in scope. Delete the document and clean its references; do not rewrite it again. Its own deletion criterion was satisfied, and retaining the contradiction violates BV0’s “no Vue tracking/backlog/waiver/retraction artifact” exit ([adversarial finding](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:265)).

- **Item 4, candidate-caused corpus regressions:** directly in BV0’s required closure. “Preserving every public route’s existing successful result contract” is broader than the 36 positive seed cells ([BV0 charter](docs/arch/refactor/rev11/charters/BV0.md:8)). Fix the candidate-introduced Vapor root-cause classes across every affected corpus cell; do not patch fixture rows individually or attempt to close all pre-existing BV1 divergences. Recompute attribution after the oracle repair—the present +318/+85% figure was measured against defective goldens and is evidence of a problem, not a stable acceptance baseline ([adversarial attribution](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:180)).

- **Item 5, skipped axes:** BV0 owns making its authoritative seed-matrix acceptance fail closed whenever any required link/runtime axis skips, and owns proving zero skips in CI/fresh-checkout evidence. That follows directly from its exit requiring the isolated install so link checks genuinely execute ([BV0 charter](docs/arch/refactor/rev11/charters/BV0.md:44)). The demonstrated false-green behavior is blocking ([conformance finding](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md:236)).  
  However, changing generic `check-candidate.mjs` skip semantics is BF2-owned. BV0 can—and must—reject the returned skip status in `official_seed_matrix.rs`. If a reusable authoritative/fail-closed harness mode is desired, include that in the authorized BF2 reopen.

## Q4 — Additional scope risks

The reopen packet should also resolve these points:

1. **Source-map acceptance was silently narrowed.** The architecture review found that candidate-vs-official mapping comparison became candidate self-consistency even though BV0 expressly owns source-map differences after harness artifacts are removed ([architecture review](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:133)). Default ruling: remove the harness artifact and restore the promised comparison. Any narrowing requires explicit maintainer ratification; it cannot ride silently in the relanding.

2. **Fix the full Vapor wrapper contract, including the TS branch.** Restoring `__vapor: true` for JS alone is incomplete; non-SSR TS Vapor uses `defineVaporComponent`. Invert/delete the wrong unit test and add positive and negative controls for both language branches.

3. **Strengthen behavioral discrimination.** The Vapor runtime test missed wrong sibling insertion order even though the structural comparator caught it ([adversarial review](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:281)). Assert exact DOM/order, not mere containment.

4. **Clear known false rationale.** Remove the incorrect production `/* CACHED */` explanation/branch and stale SSR `_ctx` comment identified by the reviewers; do not reland code or documentation already known to contradict the oracle.

5. **Rerun affected BF2 performance and provenance gates.** Changing invocation behavior and regenerating a large golden domain invalidates the corresponding generation measurements/digests. Existing thresholds remain locked; this is rerun/re-attestation, not post-result recalibration.

So the permissible “single comprehensive cycle” is one coordinated campaign with separate authority layers:

```text
BF2 oracle repair + affected golden regeneration + fresh BF2 acceptance
    → BV0 rebase and comprehensive compiler/evidence correction
    → fresh three-mandate BV0 review and landing

Svelte migration: separate authorized block/commit
```

A single undifferentiated BV0 commit containing the harness repair, compiler fixes, regenerated expectations, and Svelte migration should not be accepted.
tokens used
181 808
The correct scope is a coordinated BF2→BV0 reopen, not one enlarged BV0 commit. The Svelte migration must remain separate.

## Q1 — Oracle invocation and accepted predecessors

**Ruling: fixing `invoke-vue-oracle.mjs` and regenerating affected official goldens is BF2-owned work. It requires a formal BF2 reopen, not unilateral BV0 scope.**

BF2 expressly owns “offline official compiler invocation and immutable golden generation,” Vue script/template assembly, runtime validation, and mutation tests ([BF2 charter](docs/arch/refactor/rev11/charters/BF2.md:11)). BV0 is a successor that consumes that evidence and owns compiler source corrections ([BV0 charter](docs/arch/refactor/rev11/charters/BV0.md:13)). Letting the candidate-under-test repair and regenerate its own oracle inside BV0 would blur exactly that separation.

This is also the established BF2 reopen precedent: defects in the oracle/validator must be fixed in BF2 because later blocks are judged by it; deferring the correction poisons downstream evidence ([second-reopen ruling](docs/arch/refactor/rev11/evidence/BF2/second-reopen-ruling.md:25)).

The comprehensive BF2 correction must cover the entire option-propagation class:

- Carry the complete semantic profile into every official phase, not merely add `vapor: true` at one call site.
- Pass `vapor` and `templateOptions.ssr` correctly to `compileScript`.
- Verify parse, `compileScript`, `compileTemplate`, assembly, golden provenance, and runtime all use the same requested axes.
- Regenerate every affected official golden across the corpus. Because `ssr` was also omitted from `compileScript`, scope cannot be limited to Vapor: regenerate all Vapor and SSR/non-inline script-bearing products whose official output changes.
- Add independent JS and TS controls for `__vapor`, `defineVaporComponent`, SSR behavior, VDOM negatives, and runtime interop. The adversarial report establishes that candidate and golden were wrong identically and that the existing test locks in the defect ([adversarial review](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:103), [finding](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:252)).

**Acceptance effects:**

- **BF2:** its historical acceptance record remains an historical fact, but its present authority and evidence are invalidated. Reopen BF2, mark its reviews/evidence invalid, repair it, and obtain fresh conformance, architecture, and adversarial PASS on one exact candidate. This is a real oracle-basis change, so use a full fresh review, not a narrow BV0 recheck.
- **BF1:** do **not** reopen automatically. BF1 locked the domain and option contract before the harness existed and expressly did not implement the harness ([BF1 charter](docs/arch/refactor/rev11/charters/BF1.md:6)). Its inventory already says `compileScript.vapor` and `compileScript.templateOptions` are derived from the canonical request ([Vue options](docs/arch/refactor/rev11/evidence/framework-conformance/vue-options.tsv:72)). Thus BF2 violated the valid BF1 lock; the lock itself was not falsified. Audit and rerun only any BF1 evidence that actually consumed BF2-generated output, if any.
- **BV0:** candidate `c40a1ca96` and all round-one evidence are obsolete. BV0 must be rebased onto the reaccepted BF2 oracle and evaluated afresh.
- **BF3/BV1/other successors:** invalidate or restack any unaccepted work based on the old BF2 identity. If anything downstream has already been accepted, audit whether its proof consumed affected Vue Vapor/SSR results and reopen only the affected evidence/approval. Preserve independently valid work, as the falsification contract requires ([architecture-falsification contract](docs/arch/refactor/rev11/contracts/architecture-falsification.md:98)).

Operationally, BF2 repair and BV0 correction may be developed as a bounded two-layer stack, but BF2 lands/reaccepts first; BV0 then restacks and revalidates. Governance permits contingent upper work but forbids acceptance before the predecessor lands ([governance](docs/arch/refactor/rev11/governance.md:6)).

## Q2 — Svelte migration

**Ruling: split it out. Mechanical inertness does not create charter authority.**

BV0 is explicitly Vue-only. The architecture review confirms the Svelte migration is independent of the Vue pin, touches production Svelte constants and thousands of corpus/package files, and materially obscures the Vue review surface ([architecture review](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:102)). BF3 was deliberately narrowed to Svelte and non-Vue-runtime work ([BF3 charter](docs/arch/refactor/rev11/charters/BF3.md:23)).

Move the migration to a separate authorized Svelte-domain block/commit. Assign it to BF3 only if a scoped closure review confirms BF3 owns these corpus, dependency, and production-version changes; otherwise create a small dedicated Svelte oracle-alignment block. It must not be folded into BF2 because BF2 is test-only and cannot change production compiler files.

This separation is mandatory even if every output byte is unchanged. Governance says one program block is not co-batched with unrelated changes in its landing delta ([governance](docs/arch/refactor/rev11/governance.md:283)).

## Q3 — Items 2–5

**Items 2–4 are wholly in BV0. Item 5’s acceptance obligation is in BV0, but shared-harness mechanism changes remain BF2-owned.**

- **Item 2, VDOM regressions:** directly in scope. BV0 must restore both formerly passing routes and delete the two waiver rows. The charter requires proof that unaffected routes retain their previous successful contract and forbids substituting a waiver ([BV0 charter](docs/arch/refactor/rev11/charters/BV0.md:32), [conformance finding](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md:195)).

- **Item 3, obsolete SSR document:** directly in scope. Delete the document and clean its references; do not rewrite it again. Its own deletion criterion was satisfied, and retaining the contradiction violates BV0’s “no Vue tracking/backlog/waiver/retraction artifact” exit ([adversarial finding](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:265)).

- **Item 4, candidate-caused corpus regressions:** directly in BV0’s required closure. “Preserving every public route’s existing successful result contract” is broader than the 36 positive seed cells ([BV0 charter](docs/arch/refactor/rev11/charters/BV0.md:8)). Fix the candidate-introduced Vapor root-cause classes across every affected corpus cell; do not patch fixture rows individually or attempt to close all pre-existing BV1 divergences. Recompute attribution after the oracle repair—the present +318/+85% figure was measured against defective goldens and is evidence of a problem, not a stable acceptance baseline ([adversarial attribution](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:180)).

- **Item 5, skipped axes:** BV0 owns making its authoritative seed-matrix acceptance fail closed whenever any required link/runtime axis skips, and owns proving zero skips in CI/fresh-checkout evidence. That follows directly from its exit requiring the isolated install so link checks genuinely execute ([BV0 charter](docs/arch/refactor/rev11/charters/BV0.md:44)). The demonstrated false-green behavior is blocking ([conformance finding](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md:236)).  
  However, changing generic `check-candidate.mjs` skip semantics is BF2-owned. BV0 can—and must—reject the returned skip status in `official_seed_matrix.rs`. If a reusable authoritative/fail-closed harness mode is desired, include that in the authorized BF2 reopen.

## Q4 — Additional scope risks

The reopen packet should also resolve these points:

1. **Source-map acceptance was silently narrowed.** The architecture review found that candidate-vs-official mapping comparison became candidate self-consistency even though BV0 expressly owns source-map differences after harness artifacts are removed ([architecture review](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-architecture.md:133)). Default ruling: remove the harness artifact and restore the promised comparison. Any narrowing requires explicit maintainer ratification; it cannot ride silently in the relanding.

2. **Fix the full Vapor wrapper contract, including the TS branch.** Restoring `__vapor: true` for JS alone is incomplete; non-SSR TS Vapor uses `defineVaporComponent`. Invert/delete the wrong unit test and add positive and negative controls for both language branches.

3. **Strengthen behavioral discrimination.** The Vapor runtime test missed wrong sibling insertion order even though the structural comparator caught it ([adversarial review](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:281)). Assert exact DOM/order, not mere containment.

4. **Clear known false rationale.** Remove the incorrect production `/* CACHED */` explanation/branch and stale SSR `_ctx` comment identified by the reviewers; do not reland code or documentation already known to contradict the oracle.

5. **Rerun affected BF2 performance and provenance gates.** Changing invocation behavior and regenerating a large golden domain invalidates the corresponding generation measurements/digests. Existing thresholds remain locked; this is rerun/re-attestation, not post-result recalibration.

So the permissible “single comprehensive cycle” is one coordinated campaign with separate authority layers:

```text
BF2 oracle repair + affected golden regeneration + fresh BF2 acceptance
    → BV0 rebase and comprehensive compiler/evidence correction
    → fresh three-mandate BV0 review and landing

Svelte migration: separate authorized block/commit
```

A single undifferentiated BV0 commit containing the harness repair, compiler fixes, regenerated expectations, and Svelte migration should not be accepted.
