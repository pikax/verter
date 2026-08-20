---
ruling_id: "BV2-FINDING-A-REPAIR-AND-PLACEMENT"
type: "architecture-ruling"
date: "2026-08-20"
date_source: "file-mtime (no in-document date)"
binds: ["BV2", "BV1", "BS1", "B5", "B6"]
source_file: "ARCH-RULING-BV2-FINDING-A-REPAIR-AND-PLACEMENT.md"
summary: "Rules on the release-blocking VDOM template-codegen panic (overwrite_segmented precondition violated, types.rs:712): repair by giving leave_template's root-prefix owner sole structural ownership of comment removal within its claimed header range (deferring/absorbing visit_comment's independent overwrite, no CodeTransform reordering/narrowing/widening); assigns this repair plus a newly-discovered sibling SSR comment-only collision to a new block BV2, inserted into the DAG as {BV1, BS1} -> BV2 -> B5 (the prior direct BV1/BS1 -> B5 edge is replaced). No accepted ADR or final program outcome changes."
supersedes: []
superseded_by: []
contradicts: []
notes: "BV2 is a provisional/newly-minted block id ratified by this document, not a pre-existing charter (no BV2.md charter file existed prior to this ruling). 7,622-line transcript; the closing verdict appears duplicated once at the very end (transcript-rendering artifact)."
---

Reading prompt from stdin...
OpenAI Codex v0.148.0
--------
[1mworkdir:[0m <MACHINE_ROOT>/verter
[1mmodel:[0m gpt-5.6-sol
[1mprovider:[0m openai
[1mapproval:[0m never
[1msandbox:[0m read-only
[1mreasoning effort:[0m xhigh
[1mreasoning summaries:[0m none
[1msession id:[0m 01a02032-732b-70a2-9856-3d42fe99240f
--------
[36muser[0m
Architecture DECISION requested, read-only, on <MACHINE_ROOT>/verter
(branch program/architecture-lock). The maintainer has delegated this decision to you: choose the best
repair and its correct placement. Be decisive — a ruling, not a menu.

## The defect (release blocker, regression vs published beta.3)

Production VDOM template codegen panics at
`crates/verter_compiler/src/template/code_gen/types.rs:712`:
`overwrite_segmented precondition violated at [0,21): ReplacedContentSplit { offset: 0 }`

## Established root cause — reproduced in-repo, instrumented, confirmed

Full report: `/private/tmp/claude-501/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/FINDING-A-ROOT-CAUSE.md`. A reproducing test exists at
`crates/verter_compiler/tests/cases/vdom_root_static_class_comment_prefix_panic.rs` (in the
investigation worktree, not landed): the bug SFC fails red with the identical panic; three negative
controls (no static class / no comment / dev mode) pass, matching every known ablation.

Confirmed failing path: **`find_sole_containing_original` returns `None`** — NOT the
`anchored_present` branch (that machinery is unreached; no template-codegen call site uses
`append_left`/`prepend_right`/`append_right`).

Two producers write overlapping subranges of the template header:

1. `visit_comment` → `comment::process_comment`, walker-ordered BEFORE `leave_template`, does an
   unconditional plain `overwrite(comment.start, comment.end, "")` on `[10,18)` when
   `comments: false`. Lands in the `overwrites` channel.
2. `leave_template`'s single-root block path, needing to carry hoisted-static-class anchors, does
   `overwrite_or_root_prefix_segmented` on `[root.tag_open.start, child.start) = [0,21)` — a SUPERSET
   structurally containing producer 1's range. Lands in `segmented_overwrites`.

`CodeGenOutput::apply_to` flushes `overwrites` before `segmented_overwrites` unconditionally, so by
the time the segmented overwrite runs its target is no longer one untouched `Original` chunk and the
(correctly strict) precondition fires. Both conditions are individually necessary and jointly sufficient.

## The investigator's recommended repair

Merge ownership: teach `leave_template` — which already computes the drained leading-children set — to
suppress/absorb the independent comment-strip overwrite for any comment falling inside its own claimed
header range, since the segmented prefix's synthetic content already elides those bytes. This leaves the
strict single-chunk precondition intact rather than weakening it.

## Decide

1. **Is that the correct repair, or is a different class better?** Alternatives on the table: reorder so
   the segmented overwrite consumes the original range first; narrow one overwrite to avoid overlap;
   represent an edit as an anchored insertion rather than a replacement; carry the original segment
   through the combined operation; eliminate duplicate ownership of the region. Rule, with reasoning
   grounded in the transform-ownership contracts — not in "what stops the panic".
2. **Is the flush order in `CodeGenOutput::apply_to` itself the defect?** An unconditional
   `overwrites`-then-`segmented_overwrites` flush means any producer pair with this shape collides.
   Is the correct fix local to this call site, or is the channel model wrong?
3. **Blast radius the investigator did NOT audit:** Vapor and SSR backends, and whether any shared
   `code_transform` change would require Svelte regression coverage. Determine whether the chosen repair
   touches shared machinery.
4. **Placement.** The maintainer's directive creates a bounded post-BV1 Vue correction block
   (provisional id `BV2`) with dependency shape `BV1 + BS1 -> BV2 -> B5`, and explicitly forbids putting
   Vue semantic repair into B5/B6. Confirm BV2 is the right owner for this repair, or name the correct
   owner if the evidence says otherwise.
5. State whether your ruling changes an accepted ADR, a DAG edge, or a program outcome.

## Hard constraints (maintainer-level, non-negotiable)

Do NOT propose: restoring the removed whole-block overwrite fallback (it reconstructed whole-block
provenance and could emit silently incorrect source mappings); catching the panic and publishing partial
output; converting the invariant violation into an apparent success; globally disabling comment removal
or static-class optimization; any benchmark-marker, fixture-name or allowlist special case; or lowering
source-map correctness. Landed enforcement must be structural — never a name-keyed source scanner.

Concise, file:line throughout. End with a single REPAIR ruling line and a single PLACEMENT ruling line.

[35m[3mcodex[0m[0m
I’m applying the repository’s shared architecture and compiler-codegen guidance because this ruling hinges on transform ownership and backend boundaries. I’ll inspect the confirmed report, the mutation/flush contracts, and VDOM/Vapor/SSR call paths, then give one repair and one owner.
[35m[3mexec[0m[0m
[1m/bin/zsh -lc "wc -l CLAUDE.md .claude/skills/compiler-codegen/SKILL.md && sed -n '1,260p' CLAUDE.md && sed -n '1,260p' .claude/skills/compiler-codegen/SKILL.md"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
     659 CLAUDE.md
     487 .claude/skills/compiler-codegen/SKILL.md
    1146 total
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

`FileArtifactStore` is the authoritative per-file storage layer, keyed by `(canonical, content_hash, parse_env_hash, parse_key, build_toolchain_fingerprint, file_language_id)` — `parse_key` is the stable exact source-bytes/language/compatibility-domain/syntax-profile identity (there is no separate parser-version counter dimension), and `file_language_id` is the file's `FileLanguage` row (the per-file classification dimension of artifact identity, so a framework-capability flip misses exactly the affected files' artifact slots without touching the global `parse_env_hash`). The overlay-aware `augmentation_index` (module-augmentation inverse lookup) lives on the same store. See `/type-cache-architecture` for the full key composition, `file_language_id` producer wiring, `AugmentationTargetKey`/`AugmentationPopulation` semantics, and the `parse_stable_hash` definition.

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
---
name: compiler-codegen
description: "Rust compiler pipeline, template codegen (VDOM/IDE), CodeTransform, cached directives, strict slots, IDE error recovery, style preprocessing, CompileTarget"
---

# Compiler & Codegen

## Rust Compiler Architecture

AST-based pipeline. `compile()` orchestrator drives a linear 5-phase pipeline:

```
Vue SFC Source
    |
[Tokenizer]  byte-level SFC tokenization (tokenizer/byte.rs)
    |
[Parser]     builds arena-based template AST + extracts script/style blocks (parser/)
    |
[Style]      typed Vue/Svelte rewrite planning over StyleSyntaxIr
    |
[Script]     macro expansion, binding extraction, component wrapper (script/)
    |
[Template]   render function codegen -- VDOM or Vapor backends (template/)
    |
[Compile]    orchestrates the above, applies CodeTransform, emits output (compile.rs)
```

**Module overview:**

```
compile.rs                # Pipeline orchestrator, options, result types
tokenizer/
+-- byte.rs               # Zero-copy byte-level SFC tokenizer (production)
+-- helpers.rs            # Tokenizer utility functions
+-- types.rs              # Event, QuoteType
parser/
+-- mod.rs                # Syntax state machine (tokenizer events -> AST)
+-- types.rs              # RootNodeScript, RootNodeStyle, RootNodeTemplate
ast/
+-- mod.rs                # TemplateAst (flat arena with O(1) navigation)
+-- builder.rs            # TemplateAstBuilder (incremental AST construction)
+-- types.rs              # AstNode, ElementNode, NodeId, pre-computed flags
script/
+-- mod.rs                # generate_script() entry point
+-- process.rs            # Script setup processing, companion script merging
+-- macros.rs             # defineProps/Emits/Model/Slots/Expose/Options
+-- css_vars.rs           # _useCssVars() injection for v-bind() in styles
template/
+-- oxc/                  # OXC expression parsing for template bindings
|   +-- mod.rs            # parse_template_expressions()
|   +-- types.rs          # OxcParsedAst, OxcParsedElement, OxcParsedExpression
+-- code_gen/             # Render function codegen
    +-- mod.rs            # generate_template() entry point
    +-- walker.rs         # DFS tree walker (shared by all backends)
    +-- types.rs          # TemplateCodeGen trait, CodeGenOutput
    +-- binding.rs        # BindingResolver (_ctx./$setup. prefix resolution)
    +-- shared/           # Shared codegen helpers
    +-- vdom/             # VDOM render function output (_createElementVNode, etc.)
    +-- vapor/            # Vapor mode output (_template, _renderEffect, etc.)
ide/                      # IDE codegen: TSX or JSX+JSDoc (for LSP/TSGO type checking)
+-- mod.rs                # generate_ide_template() -- Vue template -> valid JSX; IdeScriptOptions, IdeTemplateOptions
+-- script.rs             # generate_ide_script() -- script block -> TS or JS+JSDoc wrapper
+-- script_recover.rs     # Token scanner for macro binding recovery from broken script tails
+-- condition.rs          # v-if/v-else-if/v-else condition chain codegen
+-- template/
    +-- mod.rs            # walk_element/walk_node, cached directive removal, ref conversion
    +-- directives.rs     # v-if -> ternary, v-for -> .map(), v-show -> style
    +-- props.rs          # :prop -> prop={}, @event -> onEvent={}, v-bind spread
style_planner.rs          # Vue authored-v-bind, CSS Modules, and plain-CSS scoping stages
css/
+-- mod.rs                # legacy processStyle/CSS-modules compatibility surface
+-- prepass.rs            # retained until the NAPI/provider cutover
+-- scoped.rs             # retained until the NAPI/provider cutover
+-- modules.rs            # CSS Modules: hash class names
+-- walk.rs               # String-level CSS selector walking
+-- types.rs              # ProcessStyleOptions, ProcessStyleResult
code_transform/
+-- code_transform.rs     # Chunk-based deferred mutation engine (MagicString equivalent)
+-- chunk.rs              # Chunk types (Original, Overwritten, Inserted, InsertedMapped)
+-- source_map.rs         # Source map generation from chunk positions
utils/
+-- oxc/                  # OXC parser utilities
|   +-- bindings/         # Expression binding extraction
|   +-- vue/              # Vue-specific OXC helpers (macro syntax, v-for, v-slot)
+-- vue/                  # Vue runtime helpers (tag detection, patch flags)
```

## Arena-Based Template AST

Parser builds a flat `Vec<AstNode>` arena with O(1) navigation:

```rust
pub struct TemplateAst {
    nodes: Vec<AstNode>,        // flat arena
    root: RootNodeTemplate,
}

pub struct AstNode {
    kind: AstNodeKind,          // Element | Text | Comment | Interpolation
    parent: Option<NodeId>,     // O(1) parent lookup
    index_in_parent: usize,     // O(1) sibling lookup
}
```

`ElementNode` pre-computes metadata during parsing to avoid re-scanning in codegen:

- `tag_type`: Element / Component / SlotOutlet / Template
- `prop_flag`: Bitset of prop characteristics (has class, style, spread, etc.)
- `children_flag`: Bitset of children characteristics (has text, elements, v-if, etc.)
- `children_mode`: Enum for codegen branching (Empty, TextOnly, SingleElement, Mixed, etc.)
- Cached directives: `v_condition`, `v_for`, `v_slot`, `v_once`, `v_ref`

## CodeTransform (Deferred Mutations)

All codegen phases use `CodeTransform` -- a chunk-based deferred mutation engine:

```rust
let mut ct = CodeTransform::new(input, &allocator);
ct.overwrite(start, end, replacement);  // deferred
ct.prepend_left(pos, content);          // deferred
let output = ct.build_string();         // single-pass concatenation
```

Key features:

- `cursor_hint`: Accelerates forward-progressing access patterns to amortized O(1)
- `output_delta`: Incremental length tracking avoids full scan
- Pre-allocated chunk capacity: `source_len / 13` (empirically tuned)

## CodeTransform Is the Single Source of Truth (CRITICAL)

**All modifications to generated code MUST go through `CodeTransform` operations** (`overwrite`, `prepend_left`, `append_left`, `move_with_suffix`, etc.). Never apply string replacements, regex transforms, or manual splicing to the output of `build_string()` or to content produced by a `CodeTransform`.

Post-hoc string manipulation breaks sourcemap accuracy: `CodeTransform` generates source maps by tracking chunks (Original, Inserted, Moved, Overwritten). Modifying the string after the transform makes byte offsets in the source map no longer match the content, causing position mismatches in the LSP (e.g. hover landing on the wrong token, go-to-definition jumping to wrong locations).

**Correct:** Use `ct.prepend_left(pos, ".ts")` to insert text at a known position -- chunk list and source map stay consistent.

**Wrong:** Call `content.replace(".vue'", ".vue.ts'")` on the built string -- the source map still reflects the pre-replace byte offsets.

The rule has NO scoped exceptions: the Svelte scoped-CSS renderer (`crates/verter_compiler/src/svelte/runtime/css/render.rs`) edits the original component source through the shared `CodeTransform`'s checked (`try_*`) operations -- whose insertion-affinity chunk model carries the `magic-string` semantics the official `svelte@5.56.3` `render_stylesheet` depends on (content-only `try_update` preserving the replaced range's first-chunk boundary insertions, left/right insertion affinity with per-affinity stacking, `try_remove` clearing interior insertions; pinned by `code_transform/edit_semantics_tests.rs`) -- and generates the css source map (`css.map`) from the SAME transform that built `css.code`. The guard `svelte_css_renderer_uses_code_transform` (`crates/verter_compiler/tests/`) asserts the renderer stays on the shared transform and bans any private edit buffer from the css matcher/render tree.

## Multi-unit carrier lowering

Runtime and IDE output blocks carry a `RuntimeOutputDescriptor` naming the generated destination space, emitted content artifact, declared input spaces, raw map, and honest `Exact`/`Approximate` fidelity. A separately lowered template receives `TemplateBindingMetadata` from its script pass (bindings, `has_script`, const props, and ref-bindable imports), matching Vue's official `bindingMetadata` mechanism.

When an output genuinely merges spaces, each source is parsed and lowered independently. The compiler exposes typed generated-template holes/chunks, and `framework_common/generated_chunk.rs` assembles those generated bytes while rebuilding a multi-source V3 map. Do not concatenate authored inputs, synthesize carrier markup, or reparse a fabricated whole carrier. A missing chunk boundary or uncomposable map is a typed unavailable result.

Generated-hole geometry is registered through `CodeTransform` chunk identity and resolved during the authoritative chunk walk. Never locate a generated hole by scanning built output for marker text; authored source may contain identical text.

Current Vue capability gates:

- Style analysis publishes carrier-absolute CSS/v-bind spans only for native carrier content. External or validated-supplied style bytes keep their availability/provenance but publish `css: None` and no located rows until LSP consumers understand the declared source space. Native SCSS/Sass/Less/Stylus does not mint an optional supplied-output request.
- Runtime lowering supports external/validated-supplied templates with transferred carrier script metadata, projected setup script with a carrier template, and the validated supplied-inline template topology. Projected plain script and simultaneous projected plain+setup remain `BlockContentRuntimeUnavailable`.
- IDE lowering supports native carriers and external/validated-supplied templates with composed maps when the carrier has no script, a setup script, or both plain and setup scripts. An external/validated-supplied template with only a carrier plain script remains `BlockContentIdeUnavailable` because its registered template hole is mid-module. Any projected plain or setup script also remains `BlockContentIdeUnavailable`, including the simultaneous case.
- Every lifted IDE class must pass the real TypeScript syntax gate; the gate must prove tsc analyzed the emitted file and may tolerate only missing-module/intrinsic-environment diagnostics TS2307 and TS7026. Every lifted runtime class must pass `node --check`, with both gates carrying invalid-output and non-execution controls. A compiler `Ok` result alone is not proof of capability.

### Svelte IDE structural projection

Svelte block projectors consume parser-owned structural spans. In particular, `{#snippet ...}` uses `SvelteBlock.head_span`, whose end is the grammar-balanced outer brace; downstream code must not rediscover the head with a first-`}` scan because destructured/defaulted parameters can contain nested braces. Element-owned snippets lower into a lexical IIFE so same-name snippets in sibling elements do not collide and forward, mutual, and recursive references remain valid. Unchanged snippet names, parameter lists, and bodies move as original `CodeTransform` chunks; only punctuation, annotations, or parameter text that genuinely requires a store/await-default rewrite is synthetic.

Private component-call checks may map only byte-identical authored tokens. Synthetic scaffolding, quoted/escaped property spellings, rewritten spreads, and transformed directive names stay unmapped. Legacy intrinsic `on:event|modifier={handler}` projects to the lowercase Svelte DOM attribute (`onevent={handler}`); modifiers are runtime listener behavior and never survive as TSX attribute syntax.

The Svelte IDE carrier's public facade inlines the syntactic `$props()` annotation directly into `Component<Props, Exports, Bindings>`. Facade scaffolding stays unmapped; a byte-identical annotation is emitted as one mapped insertion per authored line because V3 source-map state does not carry across generated newlines. This lets tsgo definitions on a public prop land on the authored annotation member without assigning provenance to synthetic facade text. The higher-layer Svelte public-API projector prefers the resolved semantic contract when available (captured syntax is the fallback) and records prop-name anchors from typed local declaration origins, so tsserver targets in `.svelte.verter.ts` map through that surface's own source map to the same authored member.

Vue IDE self-instance declarations reference the public API as `InstanceType<typeof import('./Foo.vue.verter')['default']>` (and the JSDoc equivalent). The relative specifier is basename-only and omits the physical `.ts` extension, avoiding `allowImportingTsExtensions` diagnostics while resolving the exact virtual `.verter.ts` surface. Do not use `InstanceType<import(...)['default']>`: `import(...)` there is not a value query and forces the old `@ts-ignore` workaround.

## Binding Metadata Flow

1. `script/process.rs` parses `<script setup>` -> walks AST -> classifies bindings as `BindingType` (SetupConst, SetupRef, Props, etc.)
2. Bindings passed to `template/code_gen/` via `generate_template()` parameter
3. `BindingResolver` determines correct accessor prefix (`_ctx.`, `$setup.`, `__props.`) and suffix (`.value` for refs)
4. Binding patches accumulated in `CodeGenOutput`, batch-applied to `CodeTransform`

## Vue Macro Semantic Boundary (CRITICAL)

The compiler owns Vue macro syntax and code emission, not typed macro
resolution. Parser macro facts are limited to authored spans, runtime
object/array constructors, defaults-object shape, model names/options, and
other syntax needed to preserve the source. Typed `defineProps`,
`defineEmits`, and `defineModel` surfaces arrive from TypeInfo through the
explicit `VueMacroSemanticInput` compile argument:

- `Unavailable`
- `Runtime(Arc<MacroRuntimeBundle>)`
- `Tsc(Arc<MacroTscBundle>)`
- `RuntimeAndTsc { runtime, tsc }`

Runtime and TSC are independent demands. Bundler script emission consumes only
`MacroRuntimeBundle`; declaration emission consumes only `MacroTscBundle`.
Entries join macro syntax by stable `syntax_index`. Runtime entries contain
the normalized props/emits/model shapes. TSC entries contain terminal splice
text and are emitted directly; the compiler does not parse or reinterpret the
splice. A property-form emits tuple remains one terminal rest-tuple parameter
(`...args: [value: T]`) in both `TscEmitRow.emit_parameters` and
`handler_parameters`; flattening it to `value: T` loses the authored tuple
shape and is forbidden.

Profile-aware public-API projection treats a script/content block override as
an immutable one-file session overlay. The batch fixed view, TypeInfo macro
producer (`SessionResolverContext`), syntax extraction, and whole-hash revision
fence must all observe that exact overlay source. Override extraction is
request-local and must not populate the raw-source `cached_tsc_extract` slot.

Resolved invalid roots cross this boundary only as closed
`MacroInvalidReason` facts. The compiler renders their public diagnostic once,
using the typed reason plus the parser-owned macro role and authored type span;
authored type text is presentation data and must never be used to reclassify
the semantic outcome. Runtime and TSC invalid outcomes share this renderer.

Local declaration carriers preserve TypeInfo refusal detail through
`TscDependencyDeclaration.declaration_failure`: structural inference budgets
remain the closed depth/work variants, while deterministic unsupported and
unresolved declaration shapes remain distinct. The compiler forwards that
typed detail in `TscDeclarationShapeReason`; it never collapses the carrier to
a generic semantic-inference failure or a diagnostic string.

The compiler must never resolve a typed macro parameter, build a companion
type environment, accept a compiler-owned external-type map, or merge
host-resolved types into parser state. `PreparedScript` parses setup and
companion blocks once for syntax reuse only. Typed prop bindings are registered
from the runtime DTO; runtime-form object/array bindings remain parser-owned
syntax facts.

A target that encounters a typed macro without its required bundle, with a
degraded entry, or with a projection for the wrong macro role fails closed at
the authored macro/type anchor using `XMissingMacroSemanticBundle` or
`XUnavailableMacroSemanticResult`. Before runtime codegen, the compiler
structurally validates the whole bundle: syntax/effective macro identities,
roles, `withDefaults` association, public names, authored-member ordinals, and
synthesized model-row anchors must all match parser-owned syntax. Any invalid
row suppresses the entire runtime bundle; a `Complete` row with a degraded
member remains usable, emits `type: null`, and reports the typed reason/detail
at the exact authored key (or model-name/type) span.

Parser model-name facts carry both an OXC-decoded semantic value and the exact
authored literal span. Runtime/TSC joins compare the decoded value, retain the
span only for mappings and diagnostics, and serialize typed emit/model public
names with the canonical JavaScript string escaper.

`withDefaults` syntax remains parser-owned. A statically eligible object
(supported keys, no spread) is folded into each DTO-derived prop row, preserving
the first duplicate and method/default expression syntax. Dynamic, spread, or
unsupported-key defaults preserve the whole authored expression and emit
exactly one `_mergeDefaults`. Runtime prop rendering follows three independent
profiles: development emits `type`, `required: true|false`, `skipCheck`, then a
static default; production retains only Vue-required Boolean/Function types and
defaults; production custom-element mode retains every `type` field, including
`type: null`. `CodegenOptions.custom_element` selects the script policy and is
independent of template tag matching in `custom_elements`. Model props use
Vue's separate model policy (no synthesized `required`; custom-element mode
does not widen production model types).

## IDE Prefixed-Expression Emit Substrate (`ide/template/emit.rs`)

IDE template codegen emits a Vue binding value as JSX through the typed `EmitOp` vocabulary so the user expression keeps an exact source-map mapping while synthetic JSX scaffolding stays unmapped. `EmitText` (`Static`/`Borrowed`/`Owned`) is the text payload; `EmitOp` variants: `InsertUnmapped` (order-preserving unmapped insert, lowers via `prepend_ordered_unmapped`), `InsertMapped` (`InsertedMapped` chunk, mapped at `source_start`+`content_offset`), `PreserveOriginal` (pure no-op — bytes stay an `Original` 1:1 chunk), `OverwriteSyntheticBoundary` (delete + unmapped insert; NEVER a mapped `out.overwrite`), `MoveOriginal`. `emit_op` is the single lowering point. `emit_jsx_binding_value` emits a `JsxBindingValue` (`source_expr`/`prefix`/`suffix`/`occurrences`/`bindings`) `occurrences` times for RELOCATED emission (native `v-model` emits the expression 2-3x); in-place sites (v-html, v-text, `:[key]`, `.foo=`, `v-bind="obj"`, static `:prop`) preserve the bytes and emit `OverwriteSyntheticBoundary` + `collect_binding_patches` around them. A function-typed `:prop` under a v-if scope (e.g. `<div v-if="ok" :onX="() => handle()">`) gets a type-narrowing guard: `compute_function_guard_injection` (props.rs) locates the injection point in SOURCE coordinates from the OXC AST (arrow-EXPRESSION body start → ternary `!((cond))?undefined:`; arrow-BLOCK / `function` body `{`+1 → block `if(!((cond))) return;`), then the value is kept IN PLACE (boundary split + `collect_binding_patches`) and the guard is an UNMAPPED `prepend_alloc` spliced into the middle — emitted BEFORE `collect_binding_patches` so an arrow-expr body identifier at the injection offset stable-sorts as `<guard><accessor-prefix><identifier>`. The guard is never baked into a mapped overwrite. The v-on inline-handler guard (von.rs) is likewise a synthetic PREFIX inside `out.overwrite(prop.start, trimmed_vs, …)` with the handler body preserved in place — it never bakes the resolved value, so it is not migrated.

Bug this replaces: baking `prefix + identifier` into one `out.overwrite(prop.start, prop_end, &format!(...))` produced a `Chunk::Overwritten` mapping the whole run back to the prop start (identifier hover/go-to-definition landed on the prop name). The flat-string IDE producers `resolve_prefixed_expr`/`resolve_prefixed_dynamic_arg` were deleted; wrapped/transformed flat-string consumers (v-on spreads, dynamic event-name keys, v-show) call the shared `build_prefixed_expr` directly. Guard: `crates/verter_compiler/tests/cases/ide_no_baked_prefix_overwrite.rs` — scans `ide/template/**` for both the INLINE bake (`out.overwrite(.., &format!(..<resolver-var>..))`) and the `let`-INDIRECTION (`let v = …format!(..<resolver-var>..)… / build_prefixed_expr(..) / resolve_simple_expr(..); out.overwrite(.., &v)`), EXCLUDING self-anchored overwrites (`out.overwrite(base + node.start, base + node.end, &v)` replaces one node's own span → navigable; partial-interpolation recovery path is the canonical example). The allowlist is EMPTY.

## Template Codegen Backends


[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n '"'^#{1,4} '"' CLAUDE.md .claude/skills/compiler-codegen/SKILL.md"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
CLAUDE.md:1:# Verter
CLAUDE.md:19:## Architecture
CLAUDE.md:23:### Shared Optimized Codebase (CRITICAL)
CLAUDE.md:46:### Build Philosophy (CRITICAL)
CLAUDE.md:63:### Shallow File Processing Core Invariant (CRITICAL)
CLAUDE.md:88:### Canonical Dependency Cache Rule (CRITICAL)
CLAUDE.md:96:### Cache Architecture (CRITICAL)
CLAUDE.md:112:### Macro Type Traversal Rule (CRITICAL)
CLAUDE.md:128:### Declaration Merging (CRITICAL)
CLAUDE.md:140:### Declaration Augmentation (CRITICAL)
CLAUDE.md:152:### Two Template Codegen Paths (CRITICAL)
CLAUDE.md:160:### Carrier IDE TS Surface Principle
CLAUDE.md:170:### Compiled-Output Conformance (CRITICAL)
CLAUDE.md:182:### Fallthrough / Root Inheritance (CRITICAL)
CLAUDE.md:192:### Component-Meta Shallow-By-Default Rule (CRITICAL)
CLAUDE.md:213:### Component-Meta Native Vs Compat (CRITICAL)
CLAUDE.md:223:### Typed-IR-Only Resolver Rule (CRITICAL)
CLAUDE.md:239:### CodeTransform Is the Single Source of Truth (CRITICAL)
CLAUDE.md:247:### Typeinfo Wire Contract (CRITICAL)
CLAUDE.md:258:### Cross-Platform Portability (CRITICAL)
CLAUDE.md:274:### Anti-Binary-Growth Integration-Test Layout (CRITICAL)
CLAUDE.md:282:### Framework Adapter Substrate (CRITICAL)
CLAUDE.md:299:### Carrier Geometry From Registered Facts (MANDATORY)
CLAUDE.md:305:### Project-Bound External-TS Contract (CRITICAL)
CLAUDE.md:327:## Build
CLAUDE.md:346:## Development
CLAUDE.md:354:## Testing
CLAUDE.md:356:### Running Tests
CLAUDE.md:359:# TypeScript / JavaScript
CLAUDE.md:364:# Rust — CANONICAL agent gate
CLAUDE.md:367:# The TWO UNDERLYING SURFACES gate.mjs runs — runnable directly (no Node, or debugging one surface in isolation):
CLAUDE.md:372:# NOTE: bare `cargo test --workspace --tests` SILENTLY SKIPS the verter_session integration suite (~4404 tests) because `session_metrics` feature unification drops those binaries from the workspace test set — it MUST NOT be the sole Rust gate; run `node scripts/gate.mjs` (which runs surfaces 1-2 from one archive plus the shipped-cfg SURFACE 3 from a second) or the two-surface pair above directly — note the pair covers neither the shipped-`cfg(debug_assertions)` surface nor its compile.
CLAUDE.md:419:### End-of-change Checks
CLAUDE.md:442:### Documentation Updates
CLAUDE.md:454:### Testing Requirements
CLAUDE.md:475:### Verification Must Prove Execution (MANDATORY)
CLAUDE.md:487:### Testing-Hermeticity (MANDATORY)
CLAUDE.md:493:### No phase archaeology in production code (MANDATORY)
CLAUDE.md:503:### VS Code Extension Testing (MANDATORY)
CLAUDE.md:509:## Agent Implementation Rules
CLAUDE.md:511:### Codebase Navigation
CLAUDE.md:515:### Planning
CLAUDE.md:530:### Execution
CLAUDE.md:536:### Orchestrating Large Plans
CLAUDE.md:542:### Self-Review
CLAUDE.md:549:### Legacy Code Deletion
CLAUDE.md:553:### Fix Quality
CLAUDE.md:562:### Stub Prevention (CRITICAL)
CLAUDE.md:582:### Agent Feedback Capture
CLAUDE.md:590:## Dependencies Policy
CLAUDE.md:615:## Commit Convention
CLAUDE.md:632:## CI/CD
CLAUDE.md:636:## Skills Reference
.claude/skills/compiler-codegen/SKILL.md:6:# Compiler & Codegen
.claude/skills/compiler-codegen/SKILL.md:8:## Rust Compiler Architecture
.claude/skills/compiler-codegen/SKILL.md:88:## Arena-Based Template AST
.claude/skills/compiler-codegen/SKILL.md:113:## CodeTransform (Deferred Mutations)
.claude/skills/compiler-codegen/SKILL.md:130:## CodeTransform Is the Single Source of Truth (CRITICAL)
.claude/skills/compiler-codegen/SKILL.md:142:## Multi-unit carrier lowering
.claude/skills/compiler-codegen/SKILL.md:157:### Svelte IDE structural projection
.claude/skills/compiler-codegen/SKILL.md:167:## Binding Metadata Flow
.claude/skills/compiler-codegen/SKILL.md:174:## Vue Macro Semantic Boundary (CRITICAL)
.claude/skills/compiler-codegen/SKILL.md:253:## IDE Prefixed-Expression Emit Substrate (`ide/template/emit.rs`)
.claude/skills/compiler-codegen/SKILL.md:259:## Template Codegen Backends
.claude/skills/compiler-codegen/SKILL.md:266:## Two Template Codegen Paths (CRITICAL)
.claude/skills/compiler-codegen/SKILL.md:277:## Compiled-Output Conformance (CRITICAL)
.claude/skills/compiler-codegen/SKILL.md:289:### Deliberate documented deviations (Svelte client)
.claude/skills/compiler-codegen/SKILL.md:295:### Svelte client text interpolation
.claude/skills/compiler-codegen/SKILL.md:299:## Svelte Compile-Options Resolver
.claude/skills/compiler-codegen/SKILL.md:318:## Svelte Conformance Trace (`conformance-trace` feature)
.claude/skills/compiler-codegen/SKILL.md:328:## Strict Slot Children Type Checking (Experimental)
.claude/skills/compiler-codegen/SKILL.md:344:## Cached Directive Fields on ElementNode
.claude/skills/compiler-codegen/SKILL.md:358:## GlobalComponents Fallback Consts + Kebab Tag Rewrite (IDE surface)
.claude/skills/compiler-codegen/SKILL.md:376:## IDE Script Error Recovery
.claude/skills/compiler-codegen/SKILL.md:396:## Style Rewrite Stages
.claude/skills/compiler-codegen/SKILL.md:409:## Style Preprocessing in Bundler Mode
.claude/skills/compiler-codegen/SKILL.md:419:## Vue Style Planner
.claude/skills/compiler-codegen/SKILL.md:430:## CompileTarget (Selective Pipeline)
.claude/skills/compiler-codegen/SKILL.md:455:## TypeExpr Lowering To The Semantic Graph (session boundary)
.claude/skills/compiler-codegen/SKILL.md:459:## Key Files

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "sed -n '1,260p' .claude/skills/compiler-codegen/SKILL.md"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
---
name: compiler-codegen
description: "Rust compiler pipeline, template codegen (VDOM/IDE), CodeTransform, cached directives, strict slots, IDE error recovery, style preprocessing, CompileTarget"
---

# Compiler & Codegen

## Rust Compiler Architecture

AST-based pipeline. `compile()` orchestrator drives a linear 5-phase pipeline:

```
Vue SFC Source
    |
[Tokenizer]  byte-level SFC tokenization (tokenizer/byte.rs)
    |
[Parser]     builds arena-based template AST + extracts script/style blocks (parser/)
    |
[Style]      typed Vue/Svelte rewrite planning over StyleSyntaxIr
    |
[Script]     macro expansion, binding extraction, component wrapper (script/)
    |
[Template]   render function codegen -- VDOM or Vapor backends (template/)
    |
[Compile]    orchestrates the above, applies CodeTransform, emits output (compile.rs)
```

**Module overview:**

```
compile.rs                # Pipeline orchestrator, options, result types
tokenizer/
+-- byte.rs               # Zero-copy byte-level SFC tokenizer (production)
+-- helpers.rs            # Tokenizer utility functions
+-- types.rs              # Event, QuoteType
parser/
+-- mod.rs                # Syntax state machine (tokenizer events -> AST)
+-- types.rs              # RootNodeScript, RootNodeStyle, RootNodeTemplate
ast/
+-- mod.rs                # TemplateAst (flat arena with O(1) navigation)
+-- builder.rs            # TemplateAstBuilder (incremental AST construction)
+-- types.rs              # AstNode, ElementNode, NodeId, pre-computed flags
script/
+-- mod.rs                # generate_script() entry point
+-- process.rs            # Script setup processing, companion script merging
+-- macros.rs             # defineProps/Emits/Model/Slots/Expose/Options
+-- css_vars.rs           # _useCssVars() injection for v-bind() in styles
template/
+-- oxc/                  # OXC expression parsing for template bindings
|   +-- mod.rs            # parse_template_expressions()
|   +-- types.rs          # OxcParsedAst, OxcParsedElement, OxcParsedExpression
+-- code_gen/             # Render function codegen
    +-- mod.rs            # generate_template() entry point
    +-- walker.rs         # DFS tree walker (shared by all backends)
    +-- types.rs          # TemplateCodeGen trait, CodeGenOutput
    +-- binding.rs        # BindingResolver (_ctx./$setup. prefix resolution)
    +-- shared/           # Shared codegen helpers
    +-- vdom/             # VDOM render function output (_createElementVNode, etc.)
    +-- vapor/            # Vapor mode output (_template, _renderEffect, etc.)
ide/                      # IDE codegen: TSX or JSX+JSDoc (for LSP/TSGO type checking)
+-- mod.rs                # generate_ide_template() -- Vue template -> valid JSX; IdeScriptOptions, IdeTemplateOptions
+-- script.rs             # generate_ide_script() -- script block -> TS or JS+JSDoc wrapper
+-- script_recover.rs     # Token scanner for macro binding recovery from broken script tails
+-- condition.rs          # v-if/v-else-if/v-else condition chain codegen
+-- template/
    +-- mod.rs            # walk_element/walk_node, cached directive removal, ref conversion
    +-- directives.rs     # v-if -> ternary, v-for -> .map(), v-show -> style
    +-- props.rs          # :prop -> prop={}, @event -> onEvent={}, v-bind spread
style_planner.rs          # Vue authored-v-bind, CSS Modules, and plain-CSS scoping stages
css/
+-- mod.rs                # legacy processStyle/CSS-modules compatibility surface
+-- prepass.rs            # retained until the NAPI/provider cutover
+-- scoped.rs             # retained until the NAPI/provider cutover
+-- modules.rs            # CSS Modules: hash class names
+-- walk.rs               # String-level CSS selector walking
+-- types.rs              # ProcessStyleOptions, ProcessStyleResult
code_transform/
+-- code_transform.rs     # Chunk-based deferred mutation engine (MagicString equivalent)
+-- chunk.rs              # Chunk types (Original, Overwritten, Inserted, InsertedMapped)
+-- source_map.rs         # Source map generation from chunk positions
utils/
+-- oxc/                  # OXC parser utilities
|   +-- bindings/         # Expression binding extraction
|   +-- vue/              # Vue-specific OXC helpers (macro syntax, v-for, v-slot)
+-- vue/                  # Vue runtime helpers (tag detection, patch flags)
```

## Arena-Based Template AST

Parser builds a flat `Vec<AstNode>` arena with O(1) navigation:

```rust
pub struct TemplateAst {
    nodes: Vec<AstNode>,        // flat arena
    root: RootNodeTemplate,
}

pub struct AstNode {
    kind: AstNodeKind,          // Element | Text | Comment | Interpolation
    parent: Option<NodeId>,     // O(1) parent lookup
    index_in_parent: usize,     // O(1) sibling lookup
}
```

`ElementNode` pre-computes metadata during parsing to avoid re-scanning in codegen:

- `tag_type`: Element / Component / SlotOutlet / Template
- `prop_flag`: Bitset of prop characteristics (has class, style, spread, etc.)
- `children_flag`: Bitset of children characteristics (has text, elements, v-if, etc.)
- `children_mode`: Enum for codegen branching (Empty, TextOnly, SingleElement, Mixed, etc.)
- Cached directives: `v_condition`, `v_for`, `v_slot`, `v_once`, `v_ref`

## CodeTransform (Deferred Mutations)

All codegen phases use `CodeTransform` -- a chunk-based deferred mutation engine:

```rust
let mut ct = CodeTransform::new(input, &allocator);
ct.overwrite(start, end, replacement);  // deferred
ct.prepend_left(pos, content);          // deferred
let output = ct.build_string();         // single-pass concatenation
```

Key features:

- `cursor_hint`: Accelerates forward-progressing access patterns to amortized O(1)
- `output_delta`: Incremental length tracking avoids full scan
- Pre-allocated chunk capacity: `source_len / 13` (empirically tuned)

## CodeTransform Is the Single Source of Truth (CRITICAL)

**All modifications to generated code MUST go through `CodeTransform` operations** (`overwrite`, `prepend_left`, `append_left`, `move_with_suffix`, etc.). Never apply string replacements, regex transforms, or manual splicing to the output of `build_string()` or to content produced by a `CodeTransform`.

Post-hoc string manipulation breaks sourcemap accuracy: `CodeTransform` generates source maps by tracking chunks (Original, Inserted, Moved, Overwritten). Modifying the string after the transform makes byte offsets in the source map no longer match the content, causing position mismatches in the LSP (e.g. hover landing on the wrong token, go-to-definition jumping to wrong locations).

**Correct:** Use `ct.prepend_left(pos, ".ts")` to insert text at a known position -- chunk list and source map stay consistent.

**Wrong:** Call `content.replace(".vue'", ".vue.ts'")` on the built string -- the source map still reflects the pre-replace byte offsets.

The rule has NO scoped exceptions: the Svelte scoped-CSS renderer (`crates/verter_compiler/src/svelte/runtime/css/render.rs`) edits the original component source through the shared `CodeTransform`'s checked (`try_*`) operations -- whose insertion-affinity chunk model carries the `magic-string` semantics the official `svelte@5.56.3` `render_stylesheet` depends on (content-only `try_update` preserving the replaced range's first-chunk boundary insertions, left/right insertion affinity with per-affinity stacking, `try_remove` clearing interior insertions; pinned by `code_transform/edit_semantics_tests.rs`) -- and generates the css source map (`css.map`) from the SAME transform that built `css.code`. The guard `svelte_css_renderer_uses_code_transform` (`crates/verter_compiler/tests/`) asserts the renderer stays on the shared transform and bans any private edit buffer from the css matcher/render tree.

## Multi-unit carrier lowering

Runtime and IDE output blocks carry a `RuntimeOutputDescriptor` naming the generated destination space, emitted content artifact, declared input spaces, raw map, and honest `Exact`/`Approximate` fidelity. A separately lowered template receives `TemplateBindingMetadata` from its script pass (bindings, `has_script`, const props, and ref-bindable imports), matching Vue's official `bindingMetadata` mechanism.

When an output genuinely merges spaces, each source is parsed and lowered independently. The compiler exposes typed generated-template holes/chunks, and `framework_common/generated_chunk.rs` assembles those generated bytes while rebuilding a multi-source V3 map. Do not concatenate authored inputs, synthesize carrier markup, or reparse a fabricated whole carrier. A missing chunk boundary or uncomposable map is a typed unavailable result.

Generated-hole geometry is registered through `CodeTransform` chunk identity and resolved during the authoritative chunk walk. Never locate a generated hole by scanning built output for marker text; authored source may contain identical text.

Current Vue capability gates:

- Style analysis publishes carrier-absolute CSS/v-bind spans only for native carrier content. External or validated-supplied style bytes keep their availability/provenance but publish `css: None` and no located rows until LSP consumers understand the declared source space. Native SCSS/Sass/Less/Stylus does not mint an optional supplied-output request.
- Runtime lowering supports external/validated-supplied templates with transferred carrier script metadata, projected setup script with a carrier template, and the validated supplied-inline template topology. Projected plain script and simultaneous projected plain+setup remain `BlockContentRuntimeUnavailable`.
- IDE lowering supports native carriers and external/validated-supplied templates with composed maps when the carrier has no script, a setup script, or both plain and setup scripts. An external/validated-supplied template with only a carrier plain script remains `BlockContentIdeUnavailable` because its registered template hole is mid-module. Any projected plain or setup script also remains `BlockContentIdeUnavailable`, including the simultaneous case.
- Every lifted IDE class must pass the real TypeScript syntax gate; the gate must prove tsc analyzed the emitted file and may tolerate only missing-module/intrinsic-environment diagnostics TS2307 and TS7026. Every lifted runtime class must pass `node --check`, with both gates carrying invalid-output and non-execution controls. A compiler `Ok` result alone is not proof of capability.

### Svelte IDE structural projection

Svelte block projectors consume parser-owned structural spans. In particular, `{#snippet ...}` uses `SvelteBlock.head_span`, whose end is the grammar-balanced outer brace; downstream code must not rediscover the head with a first-`}` scan because destructured/defaulted parameters can contain nested braces. Element-owned snippets lower into a lexical IIFE so same-name snippets in sibling elements do not collide and forward, mutual, and recursive references remain valid. Unchanged snippet names, parameter lists, and bodies move as original `CodeTransform` chunks; only punctuation, annotations, or parameter text that genuinely requires a store/await-default rewrite is synthetic.

Private component-call checks may map only byte-identical authored tokens. Synthetic scaffolding, quoted/escaped property spellings, rewritten spreads, and transformed directive names stay unmapped. Legacy intrinsic `on:event|modifier={handler}` projects to the lowercase Svelte DOM attribute (`onevent={handler}`); modifiers are runtime listener behavior and never survive as TSX attribute syntax.

The Svelte IDE carrier's public facade inlines the syntactic `$props()` annotation directly into `Component<Props, Exports, Bindings>`. Facade scaffolding stays unmapped; a byte-identical annotation is emitted as one mapped insertion per authored line because V3 source-map state does not carry across generated newlines. This lets tsgo definitions on a public prop land on the authored annotation member without assigning provenance to synthetic facade text. The higher-layer Svelte public-API projector prefers the resolved semantic contract when available (captured syntax is the fallback) and records prop-name anchors from typed local declaration origins, so tsserver targets in `.svelte.verter.ts` map through that surface's own source map to the same authored member.

Vue IDE self-instance declarations reference the public API as `InstanceType<typeof import('./Foo.vue.verter')['default']>` (and the JSDoc equivalent). The relative specifier is basename-only and omits the physical `.ts` extension, avoiding `allowImportingTsExtensions` diagnostics while resolving the exact virtual `.verter.ts` surface. Do not use `InstanceType<import(...)['default']>`: `import(...)` there is not a value query and forces the old `@ts-ignore` workaround.

## Binding Metadata Flow

1. `script/process.rs` parses `<script setup>` -> walks AST -> classifies bindings as `BindingType` (SetupConst, SetupRef, Props, etc.)
2. Bindings passed to `template/code_gen/` via `generate_template()` parameter
3. `BindingResolver` determines correct accessor prefix (`_ctx.`, `$setup.`, `__props.`) and suffix (`.value` for refs)
4. Binding patches accumulated in `CodeGenOutput`, batch-applied to `CodeTransform`

## Vue Macro Semantic Boundary (CRITICAL)

The compiler owns Vue macro syntax and code emission, not typed macro
resolution. Parser macro facts are limited to authored spans, runtime
object/array constructors, defaults-object shape, model names/options, and
other syntax needed to preserve the source. Typed `defineProps`,
`defineEmits`, and `defineModel` surfaces arrive from TypeInfo through the
explicit `VueMacroSemanticInput` compile argument:

- `Unavailable`
- `Runtime(Arc<MacroRuntimeBundle>)`
- `Tsc(Arc<MacroTscBundle>)`
- `RuntimeAndTsc { runtime, tsc }`

Runtime and TSC are independent demands. Bundler script emission consumes only
`MacroRuntimeBundle`; declaration emission consumes only `MacroTscBundle`.
Entries join macro syntax by stable `syntax_index`. Runtime entries contain
the normalized props/emits/model shapes. TSC entries contain terminal splice
text and are emitted directly; the compiler does not parse or reinterpret the
splice. A property-form emits tuple remains one terminal rest-tuple parameter
(`...args: [value: T]`) in both `TscEmitRow.emit_parameters` and
`handler_parameters`; flattening it to `value: T` loses the authored tuple
shape and is forbidden.

Profile-aware public-API projection treats a script/content block override as
an immutable one-file session overlay. The batch fixed view, TypeInfo macro
producer (`SessionResolverContext`), syntax extraction, and whole-hash revision
fence must all observe that exact overlay source. Override extraction is
request-local and must not populate the raw-source `cached_tsc_extract` slot.

Resolved invalid roots cross this boundary only as closed
`MacroInvalidReason` facts. The compiler renders their public diagnostic once,
using the typed reason plus the parser-owned macro role and authored type span;
authored type text is presentation data and must never be used to reclassify
the semantic outcome. Runtime and TSC invalid outcomes share this renderer.

Local declaration carriers preserve TypeInfo refusal detail through
`TscDependencyDeclaration.declaration_failure`: structural inference budgets
remain the closed depth/work variants, while deterministic unsupported and
unresolved declaration shapes remain distinct. The compiler forwards that
typed detail in `TscDeclarationShapeReason`; it never collapses the carrier to
a generic semantic-inference failure or a diagnostic string.

The compiler must never resolve a typed macro parameter, build a companion
type environment, accept a compiler-owned external-type map, or merge
host-resolved types into parser state. `PreparedScript` parses setup and
companion blocks once for syntax reuse only. Typed prop bindings are registered
from the runtime DTO; runtime-form object/array bindings remain parser-owned
syntax facts.

A target that encounters a typed macro without its required bundle, with a
degraded entry, or with a projection for the wrong macro role fails closed at
the authored macro/type anchor using `XMissingMacroSemanticBundle` or
`XUnavailableMacroSemanticResult`. Before runtime codegen, the compiler
structurally validates the whole bundle: syntax/effective macro identities,
roles, `withDefaults` association, public names, authored-member ordinals, and
synthesized model-row anchors must all match parser-owned syntax. Any invalid
row suppresses the entire runtime bundle; a `Complete` row with a degraded
member remains usable, emits `type: null`, and reports the typed reason/detail
at the exact authored key (or model-name/type) span.

Parser model-name facts carry both an OXC-decoded semantic value and the exact
authored literal span. Runtime/TSC joins compare the decoded value, retain the
span only for mappings and diagnostics, and serialize typed emit/model public
names with the canonical JavaScript string escaper.

`withDefaults` syntax remains parser-owned. A statically eligible object
(supported keys, no spread) is folded into each DTO-derived prop row, preserving
the first duplicate and method/default expression syntax. Dynamic, spread, or
unsupported-key defaults preserve the whole authored expression and emit
exactly one `_mergeDefaults`. Runtime prop rendering follows three independent
profiles: development emits `type`, `required: true|false`, `skipCheck`, then a
static default; production retains only Vue-required Boolean/Function types and
defaults; production custom-element mode retains every `type` field, including
`type: null`. `CodegenOptions.custom_element` selects the script policy and is
independent of template tag matching in `custom_elements`. Model props use
Vue's separate model policy (no synthesized `required`; custom-element mode
does not widen production model types).

## IDE Prefixed-Expression Emit Substrate (`ide/template/emit.rs`)

IDE template codegen emits a Vue binding value as JSX through the typed `EmitOp` vocabulary so the user expression keeps an exact source-map mapping while synthetic JSX scaffolding stays unmapped. `EmitText` (`Static`/`Borrowed`/`Owned`) is the text payload; `EmitOp` variants: `InsertUnmapped` (order-preserving unmapped insert, lowers via `prepend_ordered_unmapped`), `InsertMapped` (`InsertedMapped` chunk, mapped at `source_start`+`content_offset`), `PreserveOriginal` (pure no-op — bytes stay an `Original` 1:1 chunk), `OverwriteSyntheticBoundary` (delete + unmapped insert; NEVER a mapped `out.overwrite`), `MoveOriginal`. `emit_op` is the single lowering point. `emit_jsx_binding_value` emits a `JsxBindingValue` (`source_expr`/`prefix`/`suffix`/`occurrences`/`bindings`) `occurrences` times for RELOCATED emission (native `v-model` emits the expression 2-3x); in-place sites (v-html, v-text, `:[key]`, `.foo=`, `v-bind="obj"`, static `:prop`) preserve the bytes and emit `OverwriteSyntheticBoundary` + `collect_binding_patches` around them. A function-typed `:prop` under a v-if scope (e.g. `<div v-if="ok" :onX="() => handle()">`) gets a type-narrowing guard: `compute_function_guard_injection` (props.rs) locates the injection point in SOURCE coordinates from the OXC AST (arrow-EXPRESSION body start → ternary `!((cond))?undefined:`; arrow-BLOCK / `function` body `{`+1 → block `if(!((cond))) return;`), then the value is kept IN PLACE (boundary split + `collect_binding_patches`) and the guard is an UNMAPPED `prepend_alloc` spliced into the middle — emitted BEFORE `collect_binding_patches` so an arrow-expr body identifier at the injection offset stable-sorts as `<guard><accessor-prefix><identifier>`. The guard is never baked into a mapped overwrite. The v-on inline-handler guard (von.rs) is likewise a synthetic PREFIX inside `out.overwrite(prop.start, trimmed_vs, …)` with the handler body preserved in place — it never bakes the resolved value, so it is not migrated.

Bug this replaces: baking `prefix + identifier` into one `out.overwrite(prop.start, prop_end, &format!(...))` produced a `Chunk::Overwritten` mapping the whole run back to the prop start (identifier hover/go-to-definition landed on the prop name). The flat-string IDE producers `resolve_prefixed_expr`/`resolve_prefixed_dynamic_arg` were deleted; wrapped/transformed flat-string consumers (v-on spreads, dynamic event-name keys, v-show) call the shared `build_prefixed_expr` directly. Guard: `crates/verter_compiler/tests/cases/ide_no_baked_prefix_overwrite.rs` — scans `ide/template/**` for both the INLINE bake (`out.overwrite(.., &format!(..<resolver-var>..))`) and the `let`-INDIRECTION (`let v = …format!(..<resolver-var>..)… / build_prefixed_expr(..) / resolve_simple_expr(..); out.overwrite(.., &v)`), EXCLUDING self-anchored overwrites (`out.overwrite(base + node.start, base + node.end, &v)` replaces one node's own span → navigable; partial-interpolation recovery path is the canonical example). The allowlist is EMPTY.

## Template Codegen Backends


[35m[3mexec[0m[0m
[1m/bin/zsh -lc "sed -n '261,520p' .claude/skills/compiler-codegen/SKILL.md"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
Three backends implement the `TemplateCodeGen` trait, called by `walker::walk_template()` in DFS order:

- **VDOM** (`vdom/`): In-place source overwrites producing `_createElementVNode()` calls
- **Vapor** (`vapor/`): Replaces entire template block with direct DOM manipulation code

## Two Template Codegen Paths (CRITICAL)

The Rust compiler has **two separate template codegen paths**. Modifying one does NOT affect the other:

| Path           | Module                    | Purpose                                     | Output                           |
| -------------- | ------------------------- | ------------------------------------------- | -------------------------------- |
| **VDOM/Vapor** | `template/code_gen/vdom/` | Runtime render functions for bundler output | `_createElementVNode(...)` calls |
| **IDE**        | `ide/template/`           | Valid JSX/TSX for LSP/TSGO type checking    | `<div prop={expr}>` JSX elements |

The **LSP uses the IDE path** via `host.ensure_compiled()` with `CompileTarget::IDE`. TSGO type-checks this output. Changes to VDOM codegen do NOT affect LSP hover/completions. IDE codegen auto-detects the script language: TS SFCs produce `.tsx` (TypeScript + JSX); JS SFCs (no `lang` or `lang="js"`) produce `.jsx` (JavaScript + JSDoc annotations).

## Compiled-Output Conformance (CRITICAL)

Official-framework compiler conformance is behavioral plus structural/helper-topology parity, not raw-byte identity. For Vue VDOM/Vapor, Svelte `svelte/internal/*`, SSR/client, and future runtime backends, compare emitted output by observable behavior plus parsed/token-normalized structure: imports, helper families, helper call sequence where order is semantic, memoization/reactivity/effect topology, DOM/hydration template topology, class/style/attribute normalization, prop/property routing, event delegation, and diagnostic/reject ordering.

Cosmetic JS carrier formatting is not a finding: indentation, line breaks, non-semantic comments, intra-expression whitespace outside literals, and behavior-preserving redundant parentheses may differ from the official compiler. Directive, pragma, license/preserve, source-map/sourceURL, TS-directive, JSDoc, and other tool-consumed or framework-significant comments remain in contract. Generated local identifier spellings are waived only when the backend oracle implements scope-aware alpha-equivalence for private, non-observable bindings; otherwise identifiers are structural. Literal payload bytes, static HTML/CSS/SSR strings, public/exported or source-authored names, sourcemap mappings, diagnostic text/codes/order, and any framework-defined observable format remain in contract.

Do not build or route production compiled-output emission through JS printers, re-printers, redundant-paren canonicalizers, or any machinery whose role includes mimicking the official compiler's cosmetic JS carrier formatting. Direct-emission helpers may emit syntax-required tokens, including required parentheses for valid JavaScript expression/statement shape, but they must be scoped to semantic/syntactic correctness and covered by behavioral/structural tests rather than official cosmetic byte parity. Emit correct code directly and make conformance oracles structural for cosmetic categories: a cosmetic-only diff passes; a behavioral or structural divergence fails.

Byte-equality tests remain valid only where bytes are the actual contract, such as generated binding freshness, source-map exactness, or self-characterization during a refactor; they are not official-compiler conformance oracles.

Tracked guard gap: the positive structural-discriminator guard currently covers Svelte client only. Add backend-owned positive structural conformance oracles for Vue VDOM/Vapor and SSR/client outputs before those backends are considered fully guard-covered by this rule; the re-printer guard is cross-backend negative coverage.

### Deliberate documented deviations (Svelte client)

Default is parity with official's observable-correct behavior. A deviation is a DELIBERATE final-state choice to differ from official's correct behavior, recorded with a deviation record, durable code comment, and landed note; silent divergence is never a deviation. The native Svelte client backend currently has no deliberate deviations. This does not mean zero divergences: known structural/helper-topology divergences, behavior-equivalent topology differences, and unconverged SSR/unimplemented surfaces remain tracked in `crates/verter_compiler/src/svelte/runtime/diff_oracle_divergences.rs` or their owning tests and must be converged or kept fail-closed before promotion. `<svelte:head>` attributes fail closed matching official's `svelte_head_illegal_attribute`; that is reject-parity, not a deviation.

Guards: `svelte_structural_conformance_discriminates_cosmetic_from_behavioral_diffs`, `no_compiled_output_cosmetic_reprinter_path`.

### Svelte client text interpolation

Template text expressions are classified from the canonical retained OXC AST. Supported roots are identifier, member/optional-member, call/optional-call, binary, logical, conditional, template, `new`, and primitive literals. Rewriting, call memoization, binding impurity, D-14 constant evaluation, and nullish-coalescing analysis must consume that retained carrier rather than reparsing or scanning source text. Exact static runs use `textContent` (sole element child) or `nodeValue` (reached sibling text node) without an effect; mixed static/live chunks share one text update, and call-bearing values use the official deps-array `$.template_effect` topology. Each/await aliases retain their signal-root rewrite. Unsupported nested constructs preserve their precise typed refusal.

## Svelte Compile-Options Resolver

`resolve_svelte_compile_options(source, parsed, opts) -> Result<ResolvedSvelteCompileOptions, UnsupportedSvelteRuntimeSurface>` (`svelte/runtime/compile_options.rs`) is the SINGLE fold point for Svelte compile options. It runs ONCE per compile request from the single guarded call site at the top of `compile_client` (`svelte/runtime/client_compile.rs`) — every downstream consumer reads the resolved struct, never the raw `SvelteRuntimeOptions`.

**The fold** — compile-option side (`SvelteRuntimeOptions`) ∪ the inline `<svelte:options>` attributes, INLINE WINS per admitted key (matching `svelte@5.56.3` precedence). Inline values are read through the typed AST via the shared parser value authority (`options_namespace_value` / `options_boolean_value`), never a raw rescan. Only the keys the inline syntax admits (`namespace`, `preserveWhitespace`) fold; the resolver runs AFTER the official-reject gate, so it only ever sees official-accepted `<svelte:options>` shapes. The folded `namespace` is used ONLY to fail closed (see below) — the backend emits HTML-namespace roots ONLY, so no namespace value is threaded to codegen.

**Resolved struct** `ResolvedSvelteCompileOptions { fragments: SvelteFragments{Html,Tree}, preserve_whitespace: bool (default false), preserve_comments: bool (default false), disclose_version: bool (default true) }` — HTML-only, four fields. There is NO resolved `namespace` field (svg/mathml fail closed, so the emitted root is always html-namespaced), NO `component_name` field, and NO `css_hash_override` field: the component name is derived during LOWERING (`derive_component_name` in `naming.rs`, reading `opts.name` ?? filename, then `Scope.generate` sanitization + deconfliction against the canonical `ComponentScopeFacts` binder — `component_scope_facts.rs`, `source_declarations ∪ free_references` from one lexical pass over the module/instance scripts plus the template's authored declarations and stored expression references; the single scope authority, replacing the earlier selective `all_declared_names` + reparse approximations) and fed onto `ComponentIr`, and the `cssHash` override rides the carrier channel into the single style-plan scope point (see `/host-session` for the cache-identity seam).

**Namespace fail-close (html-only).** A `namespace: 'svg' | 'mathml'` selection (compile-option OR inline) fails closed at the resolver with a typed `UnsupportedSvelteRuntimeSurface::NamespaceUnsupported { namespace: SvelteNamespace, origin: CompileOptionOrigin{CompileProfile,Inline}, span: Option<Span> }` (stable code `svelte-runtime-unsupported-namespace`) → NO runtime module; an inline `namespace="html"` masks a compile-option `svg`/`mathml` (inline wins). svg/mathml ELEMENT emission (the `$.from_svg` / `$.from_mathml` root-helper family, the `TEMPLATE_USE_SVG` / `TEMPLATE_USE_MATHML` flag bits) is a separate deferred element-emission surface — see the svelte-native-compiler-plan D-62 row. There is NO ns×fragments matrix: every supported root, in every fragments mode, is html-namespaced.

**Per-option codegen consumers** (all read the resolved struct):

- `fragments` → the root template factory. `emit_root_hoist` (`client_module_frame.rs`) picks `$.from_html` (the backtick clone) or `$.from_tree` (the array-literal objectifier) under `fragments: 'tree'`; the root is always html-namespaced.
- `preserve_whitespace` → seeds the root `CleanContext { preserve_ws }` threaded through region synthesis.
- `preserve_comments` → a drop-set gate on retained comments, which serialize as `<!--data-->` (bare `<!>` for empty) in `template_serialize.rs` with the node-path shift applied.
- `disclose_version` → `ImportPlan.disclose_version` (`helpers.rs`), toggling the `import 'svelte/internal/disclose-version'` side-effect import.

**Fail-closed unsupported carrier.** Four officially-accepted options this backend does not support — `compatibility.componentApi` (any explicit value other than `5`), `hmr`, `accessors`, `immutable` — are demoted out of the essential surface. Any EXPLICIT presence (including a `false` / default-equivalent value, from EITHER the compile-option origin OR an inline `<svelte:options>` origin, even a value later masked by inline) fails closed with a typed `UnsupportedSvelteRuntimeSurface::CompileOptionUnsupported { option: UnsupportedSvelteCompileOption, origin: CompileOptionOrigin{CompileProfile,Inline}, span: Option<Span> }` (distinct stable codes `svelte-runtime-unsupported-{compatibility-component-api,hmr,accessors,immutable}`) → NO runtime module. This is a FEATURE refusal, NOT an official compile-error. The deprecated inline `tag` key stays the parser-first `svelte_options_deprecated_tag` HARD error, with a defensive unreachable resolver arm. `runes` is NOT folded here — it flows through the existing mode-inference plumbing (`forced_runes_option` + `opts.runes`); `css` / `customElement` / `dev` / `generate` / `experimental.async` stay delegated to their owners.

## Svelte Conformance Trace (`conformance-trace` feature)

`verter_compiler`'s `conformance-trace` Cargo feature (default OFF) enables the typed conformance-observability side channel `verter_compiler::svelte::runtime::conformance_trace` — CONFORMANCE-TOOLING-ONLY, consumed by the `verter_svelte_conformance` crate (which dev-deps `verter_compiler` with the feature on). It is not a production API surface: the default build compiles the module, its producer hooks, and every trace collection site out entirely, and production IR structs carry no trace state under either setting.

**API surface** (feature-gated): `compile_client_with_conformance_trace(...)` runs the production `compile_client` pipeline under a capture and returns the compile outcome together with the trace (a refused/rejected fixture still returns what was observed up to the failure); `capture(f)` installs a thread-local trace around a closure (captures nest, unwind-safe restore); `ConformanceTrace { static_attrs, style_matches }` carries static-attribute lexical provenance (quoting + HTML entity source representation, folded from the attribute-lowering producer boundary's single decode pass — never a second source scan) plus per-`<style>` matcher facts (per-selector tri-state certainty rows, used/scoped selector spans, scoped element identities); `MatchCertainty` is re-exported.

**`MatchCertainty` tri-state** (`svelte/runtime/css/match.rs`, always-on — NOT feature-gated): `No < Maybe < Yes`, `and` = min, `or` = max. Production projects through `might_match()`: `Yes | Maybe ⇒ true`, `No ⇒ false` — byte-identical to the pre-tri-state boolean matcher (`Maybe` was `true`; it is never treated as `No`). The per-selector certainty rows on the match sink exist only under `cfg(any(test, feature = "conformance-trace"))`.

**Zero cost when off**: by `#[cfg]` gating plus a monomorphized no-op entity-decode observer that compiles away in the default path. Guarded by `crates/verter_compiler/tests/svelte_conformance_trace_zero_cost_guard.rs` (prod-IR trace-mention ban, feature-gated module declaration, closed `AttrIr`/match-sink field inventories, decoder-mention ban, manifest keeps the default build feature-off with no dev-dependency re-enable channel) and an isolated feature-off CI gate (`cargo build`/`cargo test -p verter_compiler --lib` with no conformance crate in the dependency graph, so workspace feature unification cannot mask the default build).

## Strict Slot Children Type Checking (Experimental)

When `strict_slots: true` (VS Code: `verter.experimental.strictSlots`), the IDE template codegen emits `strictRenderSlot` calls after the JSX tree, enforcing that slot children match the parent component's `defineSlots()` type signature ([RFC #733](https://github.com/vuejs/rfcs/discussions/733)).

**Generated pattern** (inside the block scope, after JSX):

```tsx
___VERTER___strictRenderSlot({} as NonNullable<ReturnType<typeof ___VERTER___Comp{offset}>['$slots']['{slot}']>, [TabItem, {} as HTMLElementTagNameMap["input"], "" as string]);
```

**Child type references**: Component -> constructor name, HTML element -> `HTMLElementTagNameMap["tag"]`, text/interpolation -> `"" as string`. Each child is a sourcemapped `InsertedMapped` chunk pointing to its template position.

**Skipped cases**: self-closing components (no children), `is_jsx` mode, `<component :is>` (deferred), whitespace-only text, comments.

**Key files**: `ide/template/mod.rs` (`StrictSlotEntry`, `collect_strict_slot_children`, `emit_strict_slot_checks`), `ide/script.rs` (ambient `strictRenderSlot` type declarations).

## Cached Directive Fields on ElementNode

Parser extracts structural directives from `el.props` via `prop.take()` and caches them as dedicated fields on `ElementNode` (`ast/types.rs`):

| Field         | Directive                     | In `el.props`? | Notes                                            |
| ------------- | ----------------------------- | -------------- | ------------------------------------------------ |
| `v_condition` | `v-if`, `v-else-if`, `v-else` | **No** (taken) | Contains `ElementNodeCondition` with kind + prop |
| `v_for`       | `v-for`                       | **No** (taken) | Contains the full `NodeProp`                     |
| `v_slot`      | `v-slot`, `#name`             | **No** (taken) | Contains the full `NodeProp`                     |
| `v_once`      | `v-once`                      | **No** (taken) | Contains the full `NodeProp`                     |
| `v_ref`       | `ref`, `:ref`                 | **No** (taken) | Contains the full `NodeProp`                     |

**Consequence**: Code iterating `el.props` will **never see** these directives. Both codegen paths must handle them explicitly. The IDE module removes `v-if/v-for/v-slot/v-once` attributes (they become JSX wrappers/removals) and converts `ref` to JSX expression syntax (`ref={"name"}`).

## GlobalComponents Fallback Consts + Kebab Tag Rewrite (IDE surface)

Globally-registered components (registered only through a `GlobalComponents` augmentation, never imported) type in the template via per-tag **fallback consts** emitted into every script arm (`ide/script/wrapper.rs`):

- Collection (`collect_global_component_fallbacks`) walks the template once per arm: every non-builtin, non-member-expression, non-`<component>` component tag that is NOT already bound and NOT a configured custom element yields one `GlobalComponentFallback { pascal, authored_non_pascal }`, deduplicated by Pascal name in first-seen order. A `<component is="Name">` STATIC target contributes too. The same list feeds the emitted consts AND the `TemplateComponentBindings` inventory (tag rewrite, `@event` spread payloads, simple-handler param inference) so one component types identically everywhere.
- Emission is **authoring-form-sensitive** (the custom-element/fail-open contract):
  - Pascal-authored anywhere → fail-closed `const Pascal = {} as ___VERTER___GlobalComponentType<'Pascal'>;` — an unregistered name types `unknown` and produces a real TS2604 at the tag (never silent `any`).
  - Kebab/lowercase-authored ONLY → fail-open `const Pascal = {} as ___VERTER___GlobalComponentKebabType<'Pascal', 'authored-tag'>;` — a registered member (Pascal key, then the authored key) resolves the component type; an UNREGISTERED tag degrades to a function component over `JSX.IntrinsicElements['authored-tag']` (a user's web-component `IntrinsicElements` augmentation keeps typing it; Vue's `[name: string]: any` index otherwise yields `any`) — never a false TS2604 on a web-component tag.
  - Each TS const carries a go-to-definition **NAV PROBE** (`void ___VERTER___globalComponentsNav().Pascal;`); `global_component_nav_probe_offset` byte-verifies BOTH emission shapes and fails closed on any mismatch.
- **Configured custom elements** (`CompileOptions::custom_elements` prefix match, threaded as `IdeScriptOptions::custom_elements` / `IdeTemplateOptions::custom_elements`, shared predicate `ide::matches_custom_element`) are native by contract: excluded from collection AND from the kebab rewrite — the tag stays authored even when a same-name local binding exists.
- **Kebab tag rewrite** (`ide/template/mod.rs`): a dashed component tag with an inventory/local resolution rewrites to its Pascal identifier via `emit_mapped_kebab_pascal_rewrite` — PER-SEGMENT mapped edits (delete each `-`, overwrite only case-changing segment heads; every other byte stays an `Original` chunk). A whole-name overwrite mapped only up to the generated (Pascal) length, leaving the authored tag TAIL unmapped (dead hover/definition/rename); per-segment keeps every LETTER column mapped, including the last — only the removed `-` separators stay unmapped. Composition mismatch falls back to the whole-span mapped overwrite.

The conditional types live in `@verter/types` — five synchronized copies: `packages/types/index.d.ts`, `packages/types/src/components/components.ts`, `packages/typescript-plugin/src/helpers/verterTypesStub.ts`, `crates/verter_lsp/src/verter_types_stub.d.ts`, and both constants in `crates/verter_compiler/src/ide/script/type_constructs.rs` (`VERTER_TYPES_AMBIENT_MODULE` + `VERTER_TYPES_STANDALONE_DTS`). The shipped empty `declare module "vue" { interface GlobalComponents {} }` augmentation guarantees the surface exists on every Vue version (introduce-on-absence + user-augmentation merge proven by `verterTypesStub.spec.ts`'s ≤3.4 leg and its discrimination control).

**Declaration-surface parity.** Hand-maintained copies drift: the `runCustomDirective` fix that carried the directive's `Arg` type parameter (`Directive<HostElement, Value, Modifiers, Arg>`) into `arg` landed in the two `type_constructs.rs` constants and stayed missing from the three copies a real editor actually serves. `verterTypesStub.spec.ts` → `declaration-surface parity` now compiles ONE contract against every copy a TypeScript test can read as a whole artifact (`VERTER_TYPES_STUB`, `crates/verter_lsp/src/verter_types_stub.d.ts`, `packages/types/index.d.ts`) plus a deliberately reverted pre-fix copy, all in a single program: the TypeScript compiler is the oracle, so formatting and `Directive` vs `import("vue").Directive` spelling differences are invisible and only BEHAVIOURAL divergence fails. The published `@verter/types` source is covered by its own type test (`packages/types/src/directives/directives.spec.ts`); the two `type_constructs.rs` constants keep `verter_types_surface_carries_the_directive_arg_type_parameter`. Generating all copies from one source of truth would delete the drift class outright and remains the durable fix.

Design + deferred-debt rows: [`docs/arch/global-components-ide-typing.md`](../../../docs/arch/global-components-ide-typing.md).

## IDE Script Error Recovery

OXC parses the original `<script setup>` content exactly ONCE (`ide/script/setup.rs`). There is a single recovery surface — no truncate-and-reparse, no clean-prefix reparse authority, no file-scope error mode.

- **Clean parse** → the full codegen path runs unchanged (import/type-decl hoisting, binding extraction, macro lowering, the `___VERTER___TemplateBindingFN` wrapper).
- **Genuine syntax error** (both the TSX parse AND a TS-mode parse fail — a TSX-only failure is an angle-bracket assertion already handled by `rewrite_ts_type_assertions`) → a single token scan of the REAL source produces a `ScriptSetupRecoveryPlan` (`ide/script_recover.rs`, `ScriptTokenScanner::recover_plan`). The plan carries **top-level (bracket depth 0)** original-span `imports` / `macros` / `functions` / `variables` (reused for hoisting + binding registration) plus OUTPUT-ONLY recovery chunks (detected over the WHOLE source at any depth):
  - **member holes** — a dangling `a.` / `a?.` gets a universal member placeholder (`valueOf`) right after the operator so the dot cannot absorb the following token;
  - **expression holes** — a trailing operator / assignment RHS / conditional arm / arrow body gets an operand placeholder (`(undefined)`);
  - **scope closers** + a statement terminator at the recovery boundary (the `</script>` overwrite) — close the brackets the user left open so the generated scaffolding starts cleanly. A delimiter that requires a non-empty body but was left empty (a grouping/arrow-body paren `const x = (`, a computed-member bracket `foo[`) gets a placeholder operand BEFORE its closer (`undefined)`, `undefined]`); call args `foo()`, array literals `[]`, and blocks/objects `{}` are valid empty and get a bare closer.

**Top-level fact gate.** Recovered facts are gated to bracket depth 0, mirroring the clean top-level parser's `block_depth == 0` rule. A block-local declaration (`function f(){ const inner = 1; }`) is NEVER recovered as a setup binding/import; only the whole-source holes/closers fire inside nested scopes.

**Recovered macro = clean-lowering parity.** A recovered `defineProps`/`withDefaults` binding is registered `Props` AND emits the same `const __props = <binding>;` alias as clean macro lowering, so a template `props.x` (lowered to `__props.x`) resolves instead of dangling against a `__props` that was never declared.

The user's body STAYS inside the `___VERTER___TemplateBindingFN` wrapper in both cases; the broken-tail member access (`count.`) keeps hover/completion/go-to-definition working for declarations above the cursor.

**No synthesize-then-reparse.** Synthetic recovery chunks are output-only and unmapped; they are NEVER bindings, macros, imports, or any other source fact. Recovery metadata comes only from the original clean OXC AST or from original-span token recovery over the real source — a reparsed synthetic view is never an authority.

Guard: `crates/verter_compiler/tests/cases/ide_script_recovery_guard.rs` (scans `ide/script/setup.rs` for the deleted dual-recovery identifiers and the synthesize-then-reparse anti-pattern), plus `crates/verter_compiler/tests/cases/repro_member_access_ide_codegen.rs` (recovery shapes + clean-path preservation) and the negative-metadata tests in `script_recover.rs`.

## Style Rewrite Stages

Framework style rewrites use `verter_css_syntax::StyleSyntaxIr` and are deliberately split:

1. `transform_vue_v_bind(AuthoredStyleInput)` runs on authored CSS, SCSS, indented Sass, Less, or Stylus and returns the same dialect. It never preprocesses or evaluates.
2. `transform_vue_css_modules(PlainCssInput)` hashes CSS class selectors and publishes the original-to-hashed mapping from typed selector spans.
3. `transform_vue_scoped_css(PlainCssInput)` runs only on plain CSS after preprocessing. `PlainCssInput::try_new` typed-refuses every non-CSS dialect before planning.
4. Svelte owns a separate plain-CSS consumer for `:global`, selector matching/pruning, scope insertion, and keyframe rewriting. Authored preprocessor dialects are typed-refused.

Only complete trusted nodes publish edits. Rewrite uncertainty fails closed; style-liveness uncertainty fails open. Every emitted edit and its source map comes from the same `CodeTransform`. Supplied or external inputs retain their host-minted source-space/artifact identity in `RuntimeOutputDescriptor`; carrier-absolute positions are never fabricated.

The sealed `applyBlockOverrides()` handoff remains the input channel for caller-preprocessed content. External/supplied semantic analysis continues to publish `css: None` until its source-space-aware analysis consumer lands; compiler rewrite code must not route around that gate.

## Style Preprocessing in Bundler Mode

Style blocks with `lang="scss"`, `lang="sass"`, `lang="less"`, or `lang="stylus"` require caller-owned preprocessing before the plain-CSS module/scoping stages.

**Vite mode:** the unplugin caches raw authored style content and preserves its authored `lang` in the style request. Vite's CSS pipeline performs preprocessing; Vue-specific post-processing then consumes the resulting plain CSS.

**Non-Vite mode:** `preprocessBlock()` / `preprocessStyle()` use Vite's `preprocessCSS()` when configured. The result returns through the sealed `applyBlockOverrides()` channel with validated artifact, revision, source-space, and content hashes before the compiler runs the plain-CSS stages.

`vue/compiler-sfc` is resolved once per plugin instance from the project root and is shared by SFC parsing and bundler-side style post-processing. The relevant owners are `packages/unplugin/src/index.ts`, `packages/unplugin/src/core/preprocessor.ts`, `crates/verter_session/src/block_content.rs`, and `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`.

## Vue Style Planner

```
authored dialect bytes
    | StyleSyntaxIr -> trusted v-bind plan -> CodeTransform
    | caller-owned preprocessing (when needed)
plain CSS bytes
    | StyleSyntaxIr -> trusted module plan -> CodeTransform
    | StyleSyntaxIr -> trusted scoped/pseudo/keyframe plan -> CodeTransform
```

## CompileTarget (Selective Pipeline)

`CompileTarget` (bitflags in `verter_compiler::compile::types`) controls which compilation steps run:

| Flag            | Controls                                             | Used By           |
| --------------- | ---------------------------------------------------- | ----------------- |
| `STYLE`         | Style codegen (CSS scoping, modules, v-bind)         | Bundler           |
| `SCRIPT`        | Script codegen (macro expansion, binding extraction) | Bundler, Analysis |
| `TEMPLATE`      | Template VDOM/Vapor render function codegen          | Bundler           |
| `TSX`           | TSX template codegen for type checking               | LSP/IDE           |
| `TSC`           | TSC declaration file generation                      | TSC               |
| `TEMPLATE_DATA` | Template data extraction (binding occurrences)       | LSP, Analysis     |

**Presets:**

| Preset     | Flags                         | Consumer                    |
| ---------- | ----------------------------- | --------------------------- |
| `BUNDLER`  | `STYLE \| SCRIPT \| TEMPLATE` | `@verter/unplugin`, default |
| `IDE`      | `TSX`                         | LSP, TSGO                   |
| `ANALYSIS` | `SCRIPT \| TEMPLATE_DATA`     | MCP analysis                |

**Key API**: `VerterHost::ensure_compiled(canonical_id, profile)` compiles with the given profile's target. Used by LSP and MCP to populate the cache. `get_virtual_file()` still exists for retrieving specific virtual file outputs.

**Empty SFC = valid empty component.** A completely block-less `.vue` file (0 bytes / whitespace / comments only) compiles to a minimal synthetic shell — `defineComponent({ __name: "<Filename>" })` + `export default` — through a dedicated synthetic-script branch (`empty_sfc_script_block` in `compile/helpers.rs`) adjacent to the scoped-style/vapor/SSR one, so the host assembles a `Main` virtual node instead of erroring `MissingVirtualNode`, and the imported public surface is empty (`$props: {}`, no slots). Zero-block files also count the whole input as one inter-block gap (`remove_inter_block_gaps`), so stray top-level comments never leak into generated module output. Template-only SFCs keep their existing no-synthetic-script shape.

## TypeExpr Lowering To The Semantic Graph (session boundary)

The OXC worker and the semantic-lowering surface produce owned `TypeExpr` IR (and worker-local OXC AST) ONLY — they never emit a session semantic-graph node (`SemanticNodeData` / `SemanticNodeId` / `HotTypeRef`); that crate barrier (`verter_semantic` never depends on `verter_session`) is locked from the worker side by the `oxc_worker_emits_no_session_graph_node` guard. Downstream, a session-owned, query-free **structural lowerer** (`crates/verter_session/src/structural_carrier_producer/lower.rs`, entry `lower_type_expr_structural`) consumes that owned `TypeExpr` and emits the dormant semantic-graph carriers (`BareRef` / `ImportType` / `RawFallback` / `SyntheticBinding`, with a construct-signature type lowered to `Signature { kind: Construct }` and tuple rest preserved on `TupleElement.rest`) plus the structural shells, NodeScopeId-rooted, performing NO name / import / type resolution: `Foo<Arg>` becomes a `BareRef` whose `type_args` are structurally lowered (never an `InstantiationRef`), and `keyof` / indexed-access / conditional / mapped / `typeof` stay deferred shells even where the eager path would reduce them. It is intern-only — it makes no host / dispatch query (`session_graph_lowerer_makes_no_query`) and never materializes a carrier back to `TypeExpr` during emission (`unresolved_carriers_not_materialized_during_emission`). It stays dormant / demand-time (never pulled into publish or indexing). Carrier RESOLUTION is a separate demand-time engine — see the type-resolution skill.

## Key Files

| File | Purpose |
| --- | --- |
| `crates/verter_compiler/src/compile.rs` | Pipeline orchestrator (tokenize -> parse -> style -> script -> template) |
| `crates/verter_compiler/src/parser/mod.rs` | SFC parser: tokenizer events -> root nodes + template AST |
| `crates/verter_compiler/src/ast/types.rs` | AstNode, ElementNode, NodeId, PropFlags |
| `crates/verter_compiler/src/script/macros.rs` | defineProps/Emits/Model/Slots/Expose/Options |
| `crates/verter_compiler/src/script/process.rs` | Script setup processing, companion script merging |
| `crates/verter_compiler/src/template/code_gen/mod.rs` | Template codegen entry point |
| `crates/verter_compiler/src/template/code_gen/walker.rs` | DFS tree walker (shared by VDOM/Vapor backends) |
| `crates/verter_compiler/src/template/code_gen/binding.rs` | BindingResolver (\_ctx./$setup. prefix resolution) |
| `crates/verter_compiler/src/template/code_gen/vdom/` | VDOM render function codegen |
| `crates/verter_compiler/src/template/code_gen/vapor/` | Vapor mode codegen |
| `crates/verter_compiler/src/ide/mod.rs` | IDE codegen entry: TSX (TS SFCs) or JSX+JSDoc (JS SFCs) |
| `crates/verter_compiler/src/ide/script.rs` | IDE script codegen: TS annotations or JSDoc equivalents |
| `crates/verter_compiler/src/ide/script_recover.rs` | Token scanner for macro binding recovery from broken tails |
| `crates/verter_compiler/src/ide/condition.rs` | v-if/v-else-if/v-else condition chain codegen |
| `crates/verter_compiler/src/ide/template/mod.rs` | IDE template codegen: Vue -> JSX, StrictSlotEntry, emit_strict_slot_checks |
| `crates/verter_compiler/src/ide/template/directives.rs` | IDE: v-if -> ternary, v-for -> .map(), v-show -> style |
| `crates/verter_compiler/src/ide/template/props.rs` | IDE: :prop -> prop={}, @event -> onEvent={} |
| `crates/verter_compiler/src/ide/template/emit.rs` | IDE typed prefixed-expression emit substrate (`EmitOp`, `emit_jsx_binding_value`) |
| `crates/verter_compiler/src/style_planner.rs` | Typed Vue stage-1/stage-2 planners over shared style IR |
| `crates/verter_compiler/src/css/mod.rs` | Legacy NAPI/CSS-modules compatibility owner pending provider cutover |
| `crates/verter_compiler/src/css/modules.rs` | CSS Modules: hash class names |
| `crates/verter_compiler/src/code_transform/code_transform.rs` | Chunk-based deferred mutation engine |
| `crates/verter_compiler/src/code_transform/chunk.rs` | Chunk types (Original, Overwritten, Inserted, InsertedMapped) |
| `crates/verter_compiler/src/code_transform/source_map.rs` | Source map generation from chunk positions |
| `crates/verter_compiler/src/framework_common/generated_chunk.rs` | Generated-chunk assembly and multi-source map composition |

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "git branch --show-current && git status --short && wc -l /private/tmp/claude-501/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/FINDING-A-ROOT-CAUSE.md && sed -n '1,260p' /private/tmp/claude-501/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/FINDING-A-ROOT-CAUSE.md"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 107ms:[0m
     319 /private/tmp/claude-501/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/FINDING-A-ROOT-CAUSE.md
# Root-cause investigation: `overwrite_segmented precondition violated at [0,N)`

Worktree: `<MACHINE_ROOT>/verter-findA`, detached at `6ab1000bd`
(`origin/program/architecture-lock`). No push, no land. All findings below are
established by reproduction + instrumented evidence, not asserted.

## 1. Reproducing test (in-repo, no external harness)

New file: `crates/verter_compiler/tests/cases/vdom_root_static_class_comment_prefix_panic.rs`,
registered in `crates/verter_compiler/tests/cases/mod.rs`.

Run:

```
cargo test -p verter_compiler --test main vdom_root_static_class_comment_prefix_panic
```

4 tests, using `StandaloneCompiler::compile_source` (the canonical R1 one-shot
compile entry) with `CompileProduct::RuntimeClient(RuntimeProductRequest::default()-ish)`,
`FrameworkCompileRequest::Vue(VueCompileRequest { backend: VueBackendRequest::Vdom, .. })`,
`is_production: true`, `force_js: false` — i.e. exactly `target: runtime-render`,
`isProduction: true`, `forceVapor: false`, no source map, default `hmrStrategy`/
`runtimeModuleName`.

- `root_static_class_with_leading_comment_does_not_panic_in_production_vdom` — the
  exact bug-report SFC. **FAILS at `6ab1000bd`**:
  ```
  thread '...' panicked at crates/verter_compiler/src/template/code_gen/types.rs:712:17:
  overwrite_segmented precondition violated at [0,21): ReplacedContentSplit { offset: 0 }
  ```
  (Matches the reported family exactly — same message shape, `offset` always `0`,
  end varies with header length; `[0,21)` here vs. `[0,54)`/`[0,56)` in the original
  report, difference is just script-block/header length in the fixture.)
- `root_without_static_class_and_leading_comment_compiles` — negative control A
  (drop `class="x"`) — PASSES.
- `root_with_static_class_and_no_leading_comment_compiles` — negative control B
  (drop the leading comment) — PASSES.
- `root_static_class_with_leading_comment_compiles_outside_production` — negative
  control C (`is_production: false`) — PASSES.

This reproduces the exact ablation matrix from the task description using only
`StandaloneCompiler` — no `vue-benchmarks`, no NAPI/host layer, no panic-catcher
(the test asserts a clean `Result`, so it legitimately fails red at `6ab1000bd`
via unwind; nothing catches or launders the panic).

I did **not** add code-transform-unit / native-`compileMany` / source-map-validation
level coverage — see "Not determined" below; the one Vue-compiler-integration test
above was sufficient to pin the root cause precisely and is the natural fix-owning
layer. Recommend adding a `code_transform::segmented` unit test replaying the
same chunk sequence once the repair is designed (see §4).

## 2. Exact failing path

**`find_sole_containing_original` returns `None`** — confirmed by temporary
`eprintln!` instrumentation in both failure branches of
`crates/verter_compiler/src/code_transform/segmented.rs::try_overwrite_segmented`
(added, run, and fully reverted — `git diff` on that file is empty; only the new
test file + one `mod.rs` line remain in the tree).

Captured chunk state at the moment of failure, target range `[0,21)`:

```
DEBUG-TRACE: try_overwrite_segmented failed via find_sole_containing_original=None
at [0,21); chunks=[
  Original { start: 0, end: 10 },
  Original { start: 18, end: 21 },
  Overwritten { start: 21, end: 36, content: "_createElementBlock(\"div\", _hoisted_1, _toDisplayString" },
  Overwritten { start: 36, end: 38, content: "(" },
  Original { start: 38, end: 45 },
  Overwritten { start: 45, end: 47, content: ")" },
  Overwritten { start: 47, end: 53, content: ", 1)" },
  Original { start: 53, end: 54 },
  Overwritten { start: 54, end: 65, content: ")\n}" },
  Overwritten { start: 65, end: 146, content: "" },
]
```

`[0, 10)` = `<template>` (the literal open-tag text, 10 bytes). `[10, 18)` = the
8-byte `<!--c-->` comment span — **already consumed out of the chunk list** (no
"Original" or "Overwritten" chunk starts at 10; the next chunk jumps straight to
`Original{18,21}` = the whitespace immediately before `<div`). `[18,21)` = the
newline + two-space indent before `<div`. `child.start = 21` = the byte offset of
`<div`'s own `<`.

`find_sole_containing_original(0, 21)` walks chunks from the front: the first
`Original{0,10}` has `ce(10) < end(21)` → **returns `None` immediately** — the
target range spans across chunk boundaries the comment-removal overwrite already
introduced, so it is not a single live `Original` chunk.

The `anchored_present` early-exit branch (line 69, same `ReplacedContentSplit`
error shape) is **not** the one that fires — instrumented and confirmed silent.
This is expected: `anchored_present` is only set by `try_insert_anchored`
(`append_left`/`prepend_right`/`append_right` on `CodeTransform`), and grepping
the VDOM template codegen module (`template/code_gen/vdom/*.rs`) shows **zero**
call sites for those methods — that machinery belongs to script-side/macro
type-param handling, unreached by a template-only-affecting compile with a
classic (non-`<script setup>`) script block.

## 3. Full edit plan for the failing span, producers + ordering

Walk order is fixed by `walker::walk_template`: `enter_template` → DFS over
`<template>`'s children (comment, then whitespace-text — dropped without an
overwrite, then `<div>` and its descendants) → `leave_template`, called **last**.

Two producers write into the `[0, 21)` region, in this order:

1. **Comment strip — plain `overwrite` channel.**
   `walker.rs` visits the comment node (`Phase::Enter`) *before* `leave_template`
   runs. `VdomCodeGen::visit_comment`
   (`crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1741`) is not an
   interstitial-condition node (no v-if chain here), so it falls through to
   `comment::process_comment` (`crates/verter_compiler/src/template/code_gen/vdom/comment.rs:22`).
   With `self.options.comments == false` (resolved from `comments.unwrap_or(!is_production)`
   = `false` since `is_production = true`), that function does exactly:
   ```rust
   out.overwrite(comment.start, comment.end, "");   // [10, 18) → ""
   ```
   `CodeGenOutput::overwrite` (`types.rs:165`) pushes into `self.overwrites`
   (the plain-overwrite Vec, NOT the segmented channel).

2. **Template-header prefix — segmented-overwrite channel.**
   `VdomCodeGen::leave_template` (`vdom/mod.rs:822`) builds `children` from
   `build_child_records` (`vdom/mod.rs:640`). Because `self.options.comments == false`,
   `build_child_records`'s `AstNodeKind::Comment` arm (`vdom/mod.rs:709`) never
   pushes a `ChildRecord` for the comment at all — it is invisible to this
   level's own bookkeeping. The whitespace-text between the (already-stripped)
   comment and `<div>` becomes a `ChildRecord`, classified whitespace-kind, and
   is dropped as **leading whitespace with no overwrite** (`vdom/mod.rs:840-845`,
   explicitly "WITHOUT overwrites — the combined open/close tag overwrites
   below cover those source regions" — this comment is the bug's own
   documented false assumption: it does *not* account for the comment already
   having split the chunk list).
   After dropping, `children = [div]`, `effective_count == 1`, `child = &div`.
   `div` has no `v-if`, no `v-memo`, no directive-wrap → single-root block path
   (`vdom/mod.rs:1059-1084`). Because the static `class="x"` attribute was
   reserved as a production hoist candidate earlier in the walk
   (`try_reserve_element_hoist` / `reserve_hoist`, `vdom/mod.rs:537`/`621`,
   pushing into `self.hoisted_constants`), `leave_template` builds
   `hoisted_preamble_anchors` (non-empty) and — since `!self.options.is_inline`
   (true here: classic `<script>`, no `<script setup>`, so `inline_active` is
   `false` regardless of `is_production`) — takes the segmented branch:
   ```rust
   out.overwrite_or_root_prefix_segmented(
       tag_open.start,      // = root.tag_open.start = 0 (the `<template>` tag itself)
       child.start,          // = div's own tag_open.start = 21
       &prefix,               // function sig + hoisted `_hoisted_1` const + "return (_openBlock(), "
       &hoisted_preamble_anchors,
       SegmentedOverwriteAuthority::new(),
   );
   ```
   This is `overwrite_or_root_prefix_segmented` (`types.rs:266`), non-zero-width
   → `self.overwrite_segmented(0, 21, ...)` (`types.rs:200`) → pushed into
   `self.segmented_overwrites`.

   **`tag_open` here is `RootNodeTemplate.tag_open`** — confirmed from
   `crates/verter_parser/src/parser/types.rs:288-301`: the literal `<template>`
   opening-tag span, not the root element's. So the segmented overwrite's
   target `[0, 21)` is `[<template>'s own tag start, <div>'s own tag start)` —
   a range that **structurally contains** the comment span `[10, 18)` that
   producer 1 already overwrote.

3. **Flush order — `CodeGenOutput::apply_to`** (`types.rs`):
   ```rust
   ct.batch_overwrite(&self.overwrites);          // line 678 — comment-strip applied HERE
   ...
   for &(start, end, content, anchors) in &self.segmented_overwrites {   // line 704
       if let Err(err) = ct.try_overwrite_segmented(...) { panic!(...) }  // line 705-713
   }
   ```
   `self.overwrites` (plain channel, includes the comment-strip) is applied via
   `batch_overwrite` **unconditionally before** `self.segmented_overwrites` is
   walked, regardless of push order — the two are separate `Vec`s flushed in a
   fixed sequence, not by timestamp. By the time `try_overwrite_segmented(0, 21, ...)`
   runs, the comment-strip has already split the `Original` chunk that used to
   span `[0, 21)` (or further) into `Original{0,10}` + (comment's own now-gone
   region) + `Original{18,21}` — exactly the chunk list captured above.

## 4. Root cause (established, not hypothesized)

Two independent producers target overlapping subranges of the same template
header:

- **Producer A** (`visit_comment` → `comment::process_comment`, walker-ordered
  *before* `leave_template`): a plain, unconditional `overwrite(comment.start,
  comment.end, "")` on `[10, 18)`, landing in the `overwrites` channel — applied
  first by `apply_to`.
- **Producer B** (`leave_template`'s single-root block-root branch): a
  segmented overwrite on `[0, 21)` — a **superset** range that structurally
  contains Producer A's target — landing in the `segmented_overwrites` channel,
  applied second, and requiring (by `try_overwrite_segmented`'s own documented
  precondition) that its *entire* target still be one untouched `Original`
  chunk.

Neither producer is wrong about its own local slice: the comment must be
stripped in production, and the header prefix does need to be one atomic
segmented replacement to carry the hoisted-constant's authored-lexeme anchors
for correct source-mapping. The bug is that **the two producers do not
coordinate ownership of the shared byte range**: Producer B's range computation
(`[root.tag_open.start, child.start)`) is derived purely from AST/child-record
geometry and has no awareness that Producer A already carved a hole out of
that same span at the chunk level. The `!self.options.comments` code path
silently strips the comment through the *unconditional*, low-level `overwrite`
call regardless of whether some other producer is about to claim the same
region atomically; `leave_template`'s "leading whitespace dropped WITHOUT
overwrites" comment documents an assumption ("the combined open/close tag
overwrites below cover those source regions") that is only true when nothing
upstream already touched part of that region — which the comment-strip does,
whenever a leading comment is present under `comments: false`. Both conditions
(static class → segmented path taken at all; leading comment → prior split
exists) are individually necessary and jointly sufficient, matching every
ablation in the task brief.

This is a genuine ownership/ordering conflict at the `CodeGenOutput`
overwrite-channel level, **not** a mistake in the segmented-overwrite
primitive itself — `try_overwrite_segmented`'s strict single-Original-chunk
precondition is doing exactly its documented job (refusing to silently splice
across an already-mutated region and risk a wrong source map); the fix belongs
in the producers/ownership, not in relaxing that precondition.

## 5. Candidate repair classes (report only — NOT implemented)

Evaluated against the hard prohibitions (no restored whole-block-overwrite
fallback, no panic-catching in production, no disabling comment stripping or
static-class hoisting, no benchmark-name special-casing, no source-map
regression):

1. **Narrow the segmented overwrite so it never overlaps the comment's own
   range.** Instead of `[root.tag_open.start, child.start)`, split into (a) an
   unmapped/plain overwrite over `[tag_open.start, comment.start)` +
   `[comment.end, child.start)` stitched around any already-stripped
   subranges, with the segmented anchors re-based onto the surviving piece —
   OR (b) compute the segmented range from the position *after* whichever
   sibling nodes already claimed part of the header (i.e., make Producer B's
   range computation chunk-state-aware, not purely AST-geometry-derived).
   Non-trivial: the header can contain an arbitrary run of stripped
   comments/whitespace before the real root, so "narrow around one hole" does
   not generalize to N holes without effectively reimplementing chunk-gap
   discovery in the producer.

2. **Merge ownership: let `leave_template`'s header producer also own comment
   stripping for comments inside its own claimed prefix range**, i.e. suppress
   `visit_comment`'s independent overwrite for any comment that falls inside
   the root element's leading (pre-first-real-child) header region, and fold
   the comment's removal into the *content* of the segmented overwrite itself
   (the comment bytes simply don't appear in `prefix`, so they're naturally
   dropped as part of the one atomic replacement — no second producer, no
   second chunk mutation). This directly resolves the two-producers-one-range
   conflict rather than trying to make the ranges disjoint. Requires
   `leave_template` to know (or `visit_comment` to know) whether a given
   comment sits in the yet-to-be-emitted root-prefix header vs. elsewhere
   (interior/trailing) — some form of "is this comment before the effective
   root's first real child" classification, which the walker/AST already
   makes available at `leave_template` time (it's exactly `children`'s
   drained leading segment) but is not yet available at `visit_comment`'s
   time (comment is visited before `leave_template` computes `children`).

3. **Reorder: defer the comment-strip overwrite until after `leave_template`
   has claimed (or explicitly declined) the header range**, so the segmented
   overwrite runs first against the still-whole `Original` chunk, and the
   comment-strip (if the comment survives outside the header claim) applies

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "sed -n '261,380p' /private/tmp/claude-501/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/FINDING-A-ROOT-CAUSE.md"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
   after, against whatever chunk state now exists — using the ordinary
   (non-segmented) `overwrite`'s more permissive multi-chunk splice instead of
   the strict single-chunk segmented path. This flips today's fixed
   `overwrites`-then-`segmented_overwrites` flush order specifically for
   header-region comments, or moves comment-stripping for header-region
   comments into the same channel/ordering tier as the segmented overwrite.

4. **Represent the comment's removal as a no-op absorbed into the segmented
   content** (closely related to #2): since the *content* of Producer B's
   segmented overwrite is synthetic (`function render(...) { ... return
   (_openBlock(), `), it never echoes any source bytes from `[10,18)` anyway —
   the only reason Producer A's overwrite exists is to zero out that span for
   any OTHER producer that might read the "leftover" original text (e.g. the
   plain multi-root/`overwrite_or_root_prefix` paths, or a case where the
   comment isn't inside a segmented-overwritten region at all). So the
   cleanest fix is likely **not emitting Producer A's overwrite when the
   comment falls inside a range `leave_template` is about to atomically own** —
   effectively an instance of #2, phrased as "the segmented producer widens
   its own claimed content to already exclude the comment's bytes, and the
   comment-strip step skips comments inside that claim."

**Recommendation: repair class 2/4 (merge ownership under the header
producer).** It addresses the actual defect — two producers independently
believing they own the same bytes — rather than papering over symptoms by
reordering or narrowing ranges (class 1 doesn't generalize past a single
comment; class 3 changes global flush ordering with unclear blast radius on
every other segmented-overwrite call site in `vdom`/`vapor`/`ssr`, none of
which I have re-audited for the same conflict shape). Concretely: teach
`leave_template` (which already computes the exact drained-leading-children
set, `AstNodeKind::Comment` included when `comments == true`, excluded when
`comments == false`) to identify which *root-level* comments fall strictly
before `child.start`, and suppress/absorb their independent `visit_comment`
overwrite for exactly that set — the segmented prefix's synthetic content
already elides them. This keeps the strict single-chunk precondition on
`try_overwrite_segmented` completely intact (no weakening of the safety
property that legitimately caught this bug) and needs no new fallback path.
I have not designed the exact mechanism (e.g., a pre-pass that scans
`root_children` for leading comments before the walk starts, vs. an
after-the-fact "retract this already-pushed comment overwrite" API on
`CodeGenOutput`) — that's the next design step, out of scope for this
investigation per the task brief.

## 6. Could not determine / not done

- Did not audit `vapor/mod.rs` or `ssr/mod.rs` for whether the same
  two-producer-one-range shape exists there too (they also call
  `overwrite_segmented` extensively, per `grep`). The task's trigger
  conditions pin VDOM/`runtime-render` specifically; SSR/Vapor were not
  reproduced or ruled in/out.
- Did not add code-transform-unit-level (`segmented.rs` `#[cfg(test)]`),
  native-`compileMany`, or source-map-validation-level coverage — only the
  Vue-compiler-integration level (`crates/verter_compiler/tests/cases/`). The
  task asked for "more than one level where practical"; I judged the
  integration test sufficient to pin root cause precisely, and recommend the
  code-transform-unit test be added alongside whichever repair class is
  chosen (replaying the captured chunk sequence directly against
  `try_overwrite_segmented` is now trivial given the state dump in §2).
- Have not designed or implemented any fix — explicitly out of scope per the
  task.

[35m[3mexec[0m[0m
[1m/bin/zsh -lc 'rg -n "overwrite_or_root_prefix_segmented|overwrite_segmented|segmented_overwrites|process_comment|fn visit_comment|fn leave_template|build_child_records|batch_overwrite" crates/verter_compiler/src crates/verter_compiler/tests | head -n 400'[0m in <MACHINE_ROOT>/verter
[32m succeeded in 256ms:[0m
crates/verter_compiler/src/code_transform/tests.rs:1457:fn test_batch_overwrite_adjacent_ranges() {
crates/verter_compiler/src/code_transform/tests.rs:1461:    ct.batch_overwrite(&[(2, 5, "XX"), (5, 8, "YY")]);
crates/verter_compiler/src/code_transform/tests.rs:1475:fn test_batch_overwrite_empty_content_removal() {
crates/verter_compiler/src/code_transform/tests.rs:1477:    // batch_overwrite skips emitting empty-content chunks for efficiency,
crates/verter_compiler/src/code_transform/tests.rs:1481:    ct.batch_overwrite(&[(2, 5, ""), (7, 9, "")]);
crates/verter_compiler/src/code_transform/tests.rs:1487:fn test_batch_overwrite_single_item() {
crates/verter_compiler/src/code_transform/tests.rs:1497:    ct2.batch_overwrite(&[(3, 7, "XX")]);
crates/verter_compiler/src/code_transform/tests.rs:1514:fn test_batch_overwrite_preserves_unaffected_chunks() {
crates/verter_compiler/src/code_transform/tests.rs:1518:    ct.batch_overwrite(&[(1, 3, "A"), (5, 7, "B"), (8, 9, "C")]);
crates/verter_compiler/src/code_transform/tests.rs:1531:    // primitive (`try_overwrite_segmented`). Previously 32 bytes with Overwritten/Moved
crates/verter_compiler/src/code_transform/tests.rs:1604:fn test_output_delta_batch_overwrite() {
crates/verter_compiler/src/code_transform/tests.rs:1607:    ct.batch_overwrite(&[(1, 3, "xx"), (5, 7, "yyy")]); // +0 and +1 = +1
crates/verter_compiler/src/code_transform/tests.rs:1700:/// @ai-generated — batch_overwrite after a prior overwrite: the Overwritten chunk
crates/verter_compiler/src/code_transform/tests.rs:1704:fn test_batch_overwrite_after_prior_overwrite() {
crates/verter_compiler/src/code_transform/tests.rs:1716:    ct.batch_overwrite(&[(2, 4, "YY"), (6, 8, "ZZ")]);
crates/verter_compiler/src/code_transform/tests.rs:1723:        "output_delta must be accurate after batch_overwrite with prior overwrite"
crates/verter_compiler/src/code_transform/tests.rs:1727:/// @ai-generated — batch_overwrite preserves existing Overwritten chunk
crates/verter_compiler/src/code_transform/tests.rs:1730:fn test_batch_overwrite_preserves_prior_overwrite_content() {
crates/verter_compiler/src/code_transform/tests.rs:1740:    ct.batch_overwrite(&[(0, 2, "11"), (8, 10, "22")]);
crates/verter_compiler/src/code_transform/tests.rs:1744:/// @ai-generated — batch_overwrite with a fully contained range: the inner
crates/verter_compiler/src/code_transform/tests.rs:1749:fn test_batch_overwrite_contained_range_is_noop() {
crates/verter_compiler/src/code_transform/tests.rs:1753:    ct.batch_overwrite(&[(0, 5, "VNODE"), (3, 5, "")]);
crates/verter_compiler/src/code_transform/tests.rs:1764:/// @ai-generated — batch_overwrite with duplicate ranges at the same position:
crates/verter_compiler/src/code_transform/tests.rs:1768:fn test_batch_overwrite_duplicate_range() {
crates/verter_compiler/src/code_transform/tests.rs:1772:    ct.batch_overwrite(&[(3, 6, ""), (3, 6, "")]);
crates/verter_compiler/src/code_transform/tests.rs:1783:/// @ai-generated — batch_overwrite with trailing contained range: the close
crates/verter_compiler/src/code_transform/tests.rs:1787:fn test_batch_overwrite_trailing_contained() {
crates/verter_compiler/src/code_transform/tests.rs:1791:    ct.batch_overwrite(&[(7, 8, ""), (7, 10, ")")]);
crates/verter_compiler/src/code_transform/code_transform.rs:1401:    // `batch_overwrite`) live in the sibling `batch_ops` module.
crates/verter_compiler/src/code_transform/segmented.rs:8://! `try_overwrite_segmented` is a separate crate-private entry; it does
crates/verter_compiler/src/code_transform/segmented.rs:56:    pub(crate) fn try_overwrite_segmented(
crates/verter_compiler/src/code_transform/source_map_tests.rs:46:/// `try_overwrite_segmented` call anywhere — and pins both `content` and the
crates/verter_compiler/src/code_transform/source_map_tests.rs:76:/// channel for one range and the new opt-in `try_overwrite_segmented`
crates/verter_compiler/src/code_transform/source_map_tests.rs:99:    ct_b.try_overwrite_segmented(
crates/verter_compiler/src/code_transform/batch_ops.rs:400:    pub fn batch_overwrite(&mut self, overwrites: &[(u32, u32, &'a str)]) -> &mut Self {
crates/verter_compiler/src/code_transform/batch_ops.rs:411:            "batch_overwrite requires sorted ranges"
crates/verter_compiler/src/code_transform/segmented_tests.rs:1://! Direct primitive tests for `CodeTransform::try_overwrite_segmented`.
crates/verter_compiler/src/code_transform/segmented_tests.rs:33:    ct.try_overwrite_segmented(
crates/verter_compiler/src/code_transform/segmented_tests.rs:64:    ct.try_overwrite_segmented(
crates/verter_compiler/src/code_transform/segmented_tests.rs:94:    ct.try_overwrite_segmented(
crates/verter_compiler/src/code_transform/segmented_tests.rs:125:    ct.try_overwrite_segmented(
crates/verter_compiler/src/code_transform/segmented_tests.rs:151:    let Err(err) = ct.try_overwrite_segmented(
crates/verter_compiler/src/code_transform/segmented_tests.rs:173:    ct.try_overwrite_segmented(
crates/verter_compiler/src/code_transform/segmented_tests.rs:202:    ct.try_overwrite_segmented(
crates/verter_compiler/src/code_transform/segmented_tests.rs:225:    let Err(err) = ct.try_overwrite_segmented(
crates/verter_compiler/src/code_transform/segmented_tests.rs:245:    let Err(err) = ct.try_overwrite_segmented(
crates/verter_compiler/src/code_transform/segmented_tests.rs:266:    let Err(err) = ct.try_overwrite_segmented(
crates/verter_compiler/src/code_transform/segmented_tests.rs:284:        ct.try_overwrite_segmented(1, 1, "x", &[], SegmentedOverwriteAuthority::new_for_test())
crates/verter_compiler/src/code_transform/segmented_tests.rs:298:    let Err(err) = ct.try_overwrite_segmented(
crates/verter_compiler/src/code_transform/segmented_tests.rs:314:/// transform, `try_overwrite_segmented` fails closed — the narrow-shape
crates/verter_compiler/src/code_transform/segmented_tests.rs:322:        ct.try_overwrite_segmented(1, 2, "x", &[], SegmentedOverwriteAuthority::new_for_test())
crates/verter_compiler/src/code_transform/segmented_tests.rs:339:    ct.try_overwrite_segmented(
crates/verter_compiler/src/code_transform/chunk.rs:77:    /// [`CodeTransform::try_overwrite_segmented`](super::code_transform::CodeTransform::try_overwrite_segmented).
crates/verter_compiler/src/code_transform/chunk.rs:161:    /// [`CodeTransform::try_overwrite_segmented`](super::code_transform::CodeTransform::try_overwrite_segmented)
crates/verter_compiler/src/template/code_gen/vdom/text.rs:26:/// modify `CodeGenOutput`. Used by `build_child_records` to construct child
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:155:fn leave_template_empty_returns_null() {
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:196:fn leave_template_single_root_prepends_return() {
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:252:fn leave_template_multi_root_wraps_in_fragment() {
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:308:fn leave_template_multi_root_production_no_comment() {
crates/verter_compiler/src/compile_tests.rs:6632:/// skipped in visit_comment but not overwritten, and build_child_records excluded them
crates/verter_compiler/src/template/code_gen/vdom/block.rs:134:    pub(super) fn leave_template_fragment(
crates/verter_compiler/src/template/code_gen/vdom/block.rs:147:        let mut children = self.build_child_records(el_children, source);
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:118:    /// consumed by `build_child_records` (which only sees AST data).
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:120:    /// `) ? ` wrapper); `build_child_records` wraps it per element.
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:640:    pub(super) fn build_child_records(
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:822:    fn leave_template(
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:833:        let mut children = self.build_child_records(root_children, source);
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1073:                        out.overwrite_or_root_prefix_segmented(
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1188:            // build_child_records only sees AST data (no OXC), so we resolve
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1226:            // It is emitted by the parent's separator logic (build_child_records
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1583:        let mut children = self.build_child_records(el_children, source);
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1724:        // Child classification is handled by build_child_records from the AST.
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1737:        // Child classification is handled by build_child_records from the AST.
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1741:    fn visit_comment(
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1757:        // build_child_records excludes comments, so strip_interstitial_condition_nodes
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1764:        // Child classification is handled by build_child_records from the AST.
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1765:        let _ = comment::process_comment(comment_node, source, self.options.comments, out);
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:149:        let mut children = self.build_child_records(el_children, source);
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:401:        let mut children = self.build_child_records(el_children, source);
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:545:        let mut children = self.build_child_records(el_children, source);
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:1128:        let mut children = self.build_child_records(el_children, source);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:22:pub fn process_comment<'alloc>(
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:97:        let record = process_comment(&comment, source, true, &mut out).unwrap();
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:117:        process_comment(&comment, source, true, &mut out).unwrap();
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:131:        let record = process_comment(&comment, source, true, &mut out).unwrap();
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:147:        process_comment(&comment, source, true, &mut out);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:165:        let record = process_comment(&comment, source, true, &mut out).unwrap();
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:184:        process_comment(&comment, source, true, &mut out).unwrap();
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:197:        process_comment(&comment, source, true, &mut out);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:212:        process_comment(&comment, source, true, &mut out);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:225:        process_comment(&comment, source, true, &mut out);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:240:        process_comment(&comment, source, true, &mut out);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:255:        let result = process_comment(&comment, source, false, &mut out);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:273:        let record = process_comment(&comment, source, true, &mut out).unwrap();
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:289:        process_comment(&comment, source, true, &mut out).unwrap();
crates/verter_compiler/src/template/code_gen/mod.rs:149:    fn leave_template(
crates/verter_compiler/src/template/code_gen/mod.rs:200:    fn visit_comment(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4205:    fn leave_template(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4291:            out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4300:            out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4307:            out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4714:                        out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4774:                        out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4800:                        out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4828:                        out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4905:                        out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4934:                    out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5012:                    out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5429:                out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5495:                out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5579:                out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5594:                out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5643:                out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5660:                out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5684:                out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5692:                out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5720:            out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5742:            out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5775:                    out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5797:                    out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5816:            out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5875:        out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5929:                    out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5958:                        out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5998:                        out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6069:                    out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6142:                    out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6186:                    out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6211:                    out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6238:                    out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6271:                    out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6286:                    out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6328:                    out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6410:                out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6419:                out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6436:                out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6479:            out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6514:        out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6523:    fn visit_comment(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6531:            out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6546:            out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/walker.rs:145:        fn leave_template(
crates/verter_compiler/src/template/code_gen/walker.rs:198:        fn visit_comment(
crates/verter_compiler/src/template/code_gen/vapor/mod.rs:3343:    fn leave_template(
crates/verter_compiler/src/template/code_gen/vapor/mod.rs:3366:        out.overwrite_segmented(
crates/verter_compiler/src/template/code_gen/vapor/mod.rs:4153:    fn visit_comment(
crates/verter_compiler/src/template/code_gen/vapor/mod.rs:4173:            comment::process_comment(comment_node, source, self.options.comments, &mut self.html);
crates/verter_compiler/src/template/code_gen/vapor/comment.rs:14:pub fn process_comment(comment: &CommentNode, source: &str, comments: bool, html: &mut String) {
crates/verter_compiler/src/template/code_gen/vapor/comment.rs:38:        process_comment(&comment, source, true, &mut html);
crates/verter_compiler/src/template/code_gen/vapor/comment.rs:53:        process_comment(&comment, source, false, &mut html);
crates/verter_compiler/src/template/code_gen/vapor/comment.rs:74:        process_comment(&c1, source, true, &mut html);
crates/verter_compiler/src/template/code_gen/vapor/comment.rs:75:        process_comment(&c2, source, true, &mut html);
crates/verter_compiler/src/template/code_gen/types.rs:18:/// Capability token for [`CodeTransform::try_overwrite_segmented`].
crates/verter_compiler/src/template/code_gen/types.rs:53:    /// Applied via `ct.batch_overwrite()` in sorted order.
crates/verter_compiler/src/template/code_gen/types.rs:65:    /// `ct.try_overwrite_segmented()`. Vue VDOM/Vapor/SSR only — interpolation
crates/verter_compiler/src/template/code_gen/types.rs:67:    segmented_overwrites: Vec<(u32, u32, &'alloc str, &'alloc [SegmentAnchor])>,
crates/verter_compiler/src/template/code_gen/types.rs:145:            segmented_overwrites: Vec::new(),
crates/verter_compiler/src/template/code_gen/types.rs:200:    pub fn overwrite_segmented(
crates/verter_compiler/src/template/code_gen/types.rs:214:        self.segmented_overwrites
crates/verter_compiler/src/template/code_gen/types.rs:265:    /// [`overwrite_segmented`](Self::overwrite_segmented).
crates/verter_compiler/src/template/code_gen/types.rs:266:    pub fn overwrite_or_root_prefix_segmented(
crates/verter_compiler/src/template/code_gen/types.rs:278:            self.overwrite_segmented(start, end, content, anchors, authority);
crates/verter_compiler/src/template/code_gen/types.rs:678:        ct.batch_overwrite(&self.overwrites);
crates/verter_compiler/src/template/code_gen/types.rs:702:        self.segmented_overwrites
crates/verter_compiler/src/template/code_gen/types.rs:704:        for &(start, end, content, anchors) in &self.segmented_overwrites {
crates/verter_compiler/src/template/code_gen/types.rs:705:            if let Err(err) = ct.try_overwrite_segmented(
crates/verter_compiler/src/template/code_gen/types.rs:712:                panic!("overwrite_segmented precondition violated at [{start},{end}): {err:?}");
crates/verter_compiler/src/ide/template/mod.rs:2506:fn visit_comment(
crates/verter_compiler/tests/cases/segmented_overwrite_compile_fail.rs:7://! overwrite_segmented` now REQUIRES a `SegmentedOverwriteAuthority` value
crates/verter_compiler/tests/cases/compile-fail/segmented_overwrite_authority_unreachable.rs:24:// `overwrite_segmented` directly, which is exactly the false-provenance

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n \"\\bBV2\\b|\\bBV1\\b|\\bBS1\\b|\\bB5\\b|\\bB6\\b|architecture-lock|Vue correction|segmented|VDOM\" docs/arch/refactor/rev11 .claude/skills CLAUDE.md | head -n 500"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
CLAUDE.md:154:The Rust compiler has two separate template codegen paths; modifying one does NOT affect the other: **VDOM/Vapor** (`template/code_gen/vdom/`) for runtime render functions, and **IDE** (`ide/template/`) for valid JSX/TSX used by LSP/TSGO type checking. The LSP uses the IDE path via `CompileTarget::IDE`.
CLAUDE.md:172:Official-framework compiler conformance is behavioral plus structural/helper-topology parity, not raw-byte identity. For Vue VDOM/Vapor, Svelte `svelte/internal/*`, SSR/client, and future runtime backends, compare emitted output by observable behavior plus parsed/token-normalized structure: imports, helper families, helper call sequence where order is semantic, memoization/reactivity/effect topology, DOM/hydration template topology, class/style/attribute normalization, prop/property routing, event delegation, and diagnostic/reject ordering.
CLAUDE.md:178:The positive structural-discriminator guard currently covers Svelte client only (Vue VDOM/Vapor and SSR/client positive oracles are tracked follow-ups); the re-printer guard is cross-backend negative coverage. See `/compiler-codegen` for the tracked guard gap.
CLAUDE.md:645:| `/compiler-codegen`      | Template codegen (VDOM/IDE), CodeTransform, cached directives, strict slots, style preprocessing |
docs/arch/refactor/rev11/program-dag.toml:136:id = "BV1"
docs/arch/refactor/rev11/program-dag.toml:142:id = "BS1"
docs/arch/refactor/rev11/program-dag.toml:148:id = "B5"
docs/arch/refactor/rev11/program-dag.toml:151:predecessors = ["BV1", "BS1"]
docs/arch/refactor/rev11/program-dag.toml:154:id = "B6"
docs/arch/refactor/rev11/program-dag.toml:157:predecessors = ["B5"]
docs/arch/refactor/rev11/program-dag.toml:169:predecessors = ["B3", "B5", "C1"]
docs/arch/refactor/rev11/program-dag.toml:181:predecessors = ["B6", "C3"]
docs/arch/refactor/rev11/program-dag.toml:259:predecessors = ["A6", "B6", "C4", "D2"]
docs/arch/refactor/rev11/program-dag.toml:301:predecessors = ["B6", "F1", "G4"]
docs/arch/refactor/rev11/program-dag.toml:331:predecessors = ["B6", "J2"]
docs/arch/refactor/rev11/program-dag.toml:349:predecessors = ["B6", "K1"]
docs/arch/refactor/rev11/program-dag.toml:367:predecessors = ["B6", "C4", "D8", "E4", "H3", "J4", "K3", "L1"]
docs/arch/refactor/rev11/templates/program-state.template.toml:591:id = "BV1"
docs/arch/refactor/rev11/templates/program-state.template.toml:616:id = "BS1"
docs/arch/refactor/rev11/templates/program-state.template.toml:641:id = "B5"
docs/arch/refactor/rev11/templates/program-state.template.toml:666:id = "B6"
.claude/skills/compiler-codegen/SKILL.md:3:description: "Rust compiler pipeline, template codegen (VDOM/IDE), CodeTransform, cached directives, strict slots, IDE error recovery, style preprocessing, CompileTarget"
.claude/skills/compiler-codegen/SKILL.md:23:[Template]   render function codegen -- VDOM or Vapor backends (template/)
.claude/skills/compiler-codegen/SKILL.md:58:    +-- vdom/             # VDOM render function output (_createElementVNode, etc.)
.claude/skills/compiler-codegen/SKILL.md:263:- **VDOM** (`vdom/`): In-place source overwrites producing `_createElementVNode()` calls
.claude/skills/compiler-codegen/SKILL.md:272:| **VDOM/Vapor** | `template/code_gen/vdom/` | Runtime render functions for bundler output | `_createElementVNode(...)` calls |
.claude/skills/compiler-codegen/SKILL.md:275:The **LSP uses the IDE path** via `host.ensure_compiled()` with `CompileTarget::IDE`. TSGO type-checks this output. Changes to VDOM codegen do NOT affect LSP hover/completions. IDE codegen auto-detects the script language: TS SFCs produce `.tsx` (TypeScript + JSX); JS SFCs (no `lang` or `lang="js"`) produce `.jsx` (JavaScript + JSDoc annotations).
.claude/skills/compiler-codegen/SKILL.md:279:Official-framework compiler conformance is behavioral plus structural/helper-topology parity, not raw-byte identity. For Vue VDOM/Vapor, Svelte `svelte/internal/*`, SSR/client, and future runtime backends, compare emitted output by observable behavior plus parsed/token-normalized structure: imports, helper families, helper call sequence where order is semantic, memoization/reactivity/effect topology, DOM/hydration template topology, class/style/attribute normalization, prop/property routing, event delegation, and diagnostic/reject ordering.
.claude/skills/compiler-codegen/SKILL.md:287:Tracked guard gap: the positive structural-discriminator guard currently covers Svelte client only. Add backend-owned positive structural conformance oracles for Vue VDOM/Vapor and SSR/client outputs before those backends are considered fully guard-covered by this rule; the re-printer guard is cross-backend negative coverage.
.claude/skills/compiler-codegen/SKILL.md:438:| `TEMPLATE`      | Template VDOM/Vapor render function codegen          | Bundler           |
.claude/skills/compiler-codegen/SKILL.md:469:| `crates/verter_compiler/src/template/code_gen/walker.rs` | DFS tree walker (shared by VDOM/Vapor backends) |
.claude/skills/compiler-codegen/SKILL.md:471:| `crates/verter_compiler/src/template/code_gen/vdom/` | VDOM render function codegen |
.claude/skills/host-session/SKILL.md:192:Vue macro code actions (`features/macro_actions.rs` B3/B4/B5, and the cross-file
docs/arch/refactor/rev11/PROVENANCE.md:7:| `consolidated/verter-architecture-lock-master-plan-v11.md` | `3303834589df23cd04338801374857e685d9961df3d323c60c4b58db54ce62ce` | MATCHES the published Revision 11 validation report |
docs/arch/refactor/rev11/program.md:173:## B5 — Borrowed one-shot compiler atomic cutover
docs/arch/refactor/rev11/program.md:181:## B6 — `PreparedCarrier`, direct batch, and direct-core closure
docs/arch/refactor/rev11/program.md:183:**Predecessors:** `B5`.
docs/arch/refactor/rev11/program.md:201:**Predecessors:** `B3`, `B5`, `C1`.
docs/arch/refactor/rev11/program.md:217:**Predecessors:** `B5`, `C3`.
docs/arch/refactor/rev11/program.md:317:**Predecessors:** `A6`, `B6`, `C4`, `D2`.
docs/arch/refactor/rev11/program.md:363:**Predecessors:** `B6`, `F1`, `G4`.
docs/arch/refactor/rev11/program.md:395:**Predecessors:** `B6`, `J2`.
docs/arch/refactor/rev11/program.md:415:**Predecessors:** `B6`, `K1`.
docs/arch/refactor/rev11/program.md:435:**Predecessors:** `B6`, `C4`, `D8`, `E4`, `H3`, `J4`, `K3`, `L1`.
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:4:**Prepared against:** local `program/architecture-lock` commit
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:10:inline `BV0A` charter in [§3](#3-bv0a-charter). It does not amend the B4, BV1,
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:11:or B5 charters or the emitter/mapping disposition ledger.
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:31:publication architecture. B4, BV1, and B5 are still locked. BV0 therefore cannot
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:37:slice for the immediate Vue correction train, preserves every ratified block ID,
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:38:and does not present the work as an early BV1 conformance train or an early B4
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:64:B4 -> {BV1, BS1}
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:65:{BV1, BS1} -> B5 -> B6
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:171:architecture, atomic artifact set, or publication transaction; BV1's complete Vue
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:172:semantic/conformance train; B5's sole direct compiler core; a universal or
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:264:VDOM and Vapor client output, SSR output, and inline/non-inline applicability where
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:339:existing thresholds. The change is Vue-only and adds no B3/B4/BV1/B5 authority, no
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:347:architecture, BV1's complete Vue plan, B5's direct-core cutover, a universal or
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:391:## 5. B4, BV1, B5, and EM-038 preservation
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:393:The [`B4.md`](../charters/B4.md), [`BV1.md`](../charters/BV1.md), and
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:394:[`B5.md`](../charters/B5.md) charters are not amended. Their final ownership remains:
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:396:- BV1 owns Vue semantic models, plans, script/template assembly semantics, and the
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:400:- B5 exposes the accepted framework algorithms through the sole direct compiler core
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:405:remains `Replace` with acceptance owner `BV1+B4+B5`. BV0A neither changes that row to
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:411:the current session assembler map-capable for the bounded predecessor need; BV1+B4+B5
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:413:B4 assembly/map composition and atomic publication, and the sole B5 direct core. No
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:416:BV1's existing BV0-preservation exit already requires the exact BV0 seed pack to
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:418:authored-source mapping correctness made executable by BV0A. BV1 or B4 may replace
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:447:correction direction, BF3 narrowing, BV1 preservation rule, or literal 36-cell exit.
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:477:> EM-038's final `Replace` disposition and the complete BV1+B4+B5 cutover authority;
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:525:> `Replace` disposition and the complete BV1+B4+B5 cutover authority; expose BV0A,
docs/arch/refactor/rev11/contracts/conformance-goldens.md:35:development/production, source maps on/off, Vue VDOM/Vapor, Svelte runes/legacy where
docs/arch/refactor/rev11/contracts/package-publication.md:17:- `verter-architecture-lock-master-plan-v11.md`;
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:3:**Review target:** the released `verter-architecture-lock-master-plan-v10.md` and `verter-architecture-v10.zip`.  
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:33:**Revision 11 correction:** `B4` atomically introduces compact units/mapping taxonomy and deletes blanking before `B5` cuts over the borrowed direct compiler. `B6` then adds prepared/batch reuse over the surviving direct core.
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:41:## V10-B5 — External observation trust was not explicit enough
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:47:## V10-B6 — Late waiter and budget semantics were incomplete
docs/arch/refactor/rev11/implementation-readiness-review-v10.md:99:**Revision 11 correction:** private review layers target a private integration branch, remain draft/non-mergeable, and reach trunk only through the final atomic candidate. D1/D2 is the canonical pattern; internal layers of B4/B5/D2 or another atomic block follow the same rule.
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:4:**Prepared against:** local `program/architecture-lock` commit
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:9:[`BV1.md`](../charters/BV1.md) charters; introduces
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:14:For Vue VDOM, Vapor, and SSR findings produced by BF2/BF3 conformance probes,
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:25:every non-inline `<script setup>` cell and the dropped VDOM `<slot>` fallback
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:40:B4 -> {BV1, BS1}
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:41:{BV1, BS1} -> B5 -> B6
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:69:BV0 immediately corrects the genuine Vue VDOM, Vapor, SSR, assembly, and mapping
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:79:BF3's Vue VDOM/Vapor/SSR runtime-render rows are removed from its retraction and
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:91:procedure per successful cell, and owned scope away from Vue VDOM/Vapor/SSR; adds the
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:96:## 5. BV1 preservation amendment
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:98:BV1 remains after B4 and retains its complete existing charter. Its required exits
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:100:that the exact BV0 seed pack remains green. BV1 may replace a BV0 implementation only
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:103:materialized in [`BV1.md`](../charters/BV1.md).
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:129:> tree `<bundle-tree-oid>`; confirm that Vue VDOM/Vapor/SSR production
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:133:> amend the DAG so B2 and B3 require both BV0 and BF3; require BV1 to preserve
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:165:> package digests; confirm that Vue VDOM/Vapor/SSR production compilation
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:171:> require BV1 to preserve every BV0 correction on the final substrate; and
docs/arch/refactor/rev11/charters/BCSS0.md:20:to BS0, BS1, or a host-route owner.
docs/arch/refactor/rev11/evidence/BRT0/context-packet.md:12:- **Base:** `program/architecture-lock` @ `dd84e5fa2`.
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:8:> embargo is discharged: BF3's unit landed on `program/architecture-lock` first, and
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:16:**Prepared against:** local `program/architecture-lock` commit
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:23:(`docs/arch/architecture-lock/ledger/program-state.toml`), so no source citation
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:75:replacement, the Svelte output-liveness DEFER to BS1) are implementation guidance for
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:169:`predecessors = ["B6", "K1"]`.
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:174:Three are recorded as absent before B5/B6 (`:31-33`: direct one-shot final core,
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:203:acceptance owner **`B5`**. Options to B3; route to B5. This amendment makes B3's
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:499:`program-dag.toml:321-325` with `predecessors = ["B6", "K1"]`.
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:507:  -> BV1 (:111-115) / BS1 (:117-121), both predecessors = ["B4"]
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:508:  -> B5          (:123-127, predecessors = ["BV1", "BS1"])
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:509:  -> B6          (:129-133, predecessors = ["B5"])
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:510:  -> K2          (:321-325, predecessors = ["B6", "K1"])
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:515:(`:315-319`) depends on `C4`, which depends on `B6` and `C3`, and `C2`
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:521:routes that exist by B6/C3; NAPI, WASM, bundler, and managed publication retain their
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:603:| `evidence/framework-conformance/vue-official-cases.tsv` (2004 lines; `provisional_owner` = col 9) | `provisional_owner` | `B2/BV1` | **509** | all `blocked` |
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:604:| `evidence/framework-conformance/svelte-official-cases.tsv` (3458 lines; `provisional_owner` = col 7) | `provisional_owner` | `B2/BS1` | **589** | all `blocked` |
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:607:distributions confirm there is no B2-sole row anywhere: Vue is `{BV1: 1494,
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:608:B2/BV1: 509}`; Svelte is `{BS1: 2695, B2/BS1: 589, BF1: 144, BS1/B4: 29}`. All 1098
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:629:places `BV1` (`:111-115`) and `BS1` (`:117-121`) after `B4` (`:105-109`), which is
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:663:  mapping, product set, TypeScript observation, and route equivalence. Owner: `BV1`
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:664:  for `B2/BV1` rows, `BS1` for `B2/BS1` rows.
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:720:  any respect — AMD-006's BV0 allocation, BF3 narrowing, and BV1 preservation
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:730:  `charter_digest` (`docs/arch/architecture-lock/ledger/program-state.toml:363-365`
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:756:(`emitter-mapping-dispositions.tsv:8`, acceptance `B2+B3+BV1+B5`) and EM-020
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:757:`crates/verter_compiler/src/svelte/carrier.rs` (`:21`, acceptance `B2+B3+BS1+B5`) —
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:803:`program/architecture-lock` is the program's single landing branch, and BF3 is
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:814:1. **BF3 lands** on `program/architecture-lock` as its own unit.
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:835:  empty `charter_digest` (`docs/arch/architecture-lock/ledger/program-state.toml:363-365`,
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:879:589  B2/BS1   blocked
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:881:2695 BS1      blocked
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:882:29   BS1/B4   blocked
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:887:further work. The 29 `BS1/B4` rows are `blocked` but their owner column contains no
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:890:only. Whether `BS1/B4` needs its own facet split is a question for BS1's and B4's
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:918:`standalone.rs`) assign the orchestration and the route to **B5**. The division is
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:922:unreachable for that combination and B5 deletes the site later. B3 is not read as
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:50:## R9-B5 — Compiler products were not fully compositional
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:56:## R9-B6 — Project-aware compile was not a closed transaction
docs/arch/refactor/rev11/contracts/typescript-product-conformance.md:24:Source-local Vue macros are BV1-owned. Imported/project-aware macro information is a
docs/arch/refactor/rev11/contracts/typescript-product-conformance.md:25:closed typed demand emitted by BV1 and fulfilled by C3; C3 cannot replace Vue
docs/arch/refactor/rev11/contracts/framework-compiler-boundary.md:32:| compatibility family | Vue VDOM, Vue Vapor, Vue SSR, Svelte client, or Svelte server as separately claimed |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:3:**Source:** `<DOWNLOADS>/verter-architecture-lock-master-plan-v11.md` (8947 lines, 402905 bytes), where `<DOWNLOADS>` is the operator's local downloads directory; the same file is landed at `consolidated/verter-architecture-lock-master-plan-v11.md`.
docs/arch/refactor/rev11/amendments/AMD-009-bf3-audit-and-immediate-correction-blocks.md:109:Svelte and non-Vue-runtime successful cells. AMD-006's Vue correction direction,
docs/arch/refactor/rev11/amendments/AMD-009-bf3-audit-and-immediate-correction-blocks.md:110:BV0 ownership, and BV1 preservation requirement likewise remain in force; only its
docs/arch/refactor/rev11/amendments/AMD-009-bf3-audit-and-immediate-correction-blocks.md:197:  rather than waiting for post-B4 BS1.
docs/arch/refactor/rev11/amendments/AMD-009-bf3-audit-and-immediate-correction-blocks.md:225:BS0, or post-B4 BS1, and this package adds no pin-migration block.
docs/arch/refactor/rev11/charters/C4.md:3:**Status:** PROPOSED amendment / LOCKED. **Predecessors:** B6 and C3.
docs/arch/refactor/rev11/README.md:9:This directory contains the Revision 11 architecture-lock program: the split authority
docs/arch/refactor/rev11/README.md:34:- [`consolidated/verter-architecture-lock-master-plan-v11.md`](consolidated/verter-architecture-lock-master-plan-v11.md)
docs/arch/refactor/rev11/README.md:95:  {B2, B3}; narrows BF3 to Svelte/non-Vue-runtime scope; and requires BV1 to preserve
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation2.md:21:1. **BV1→C3 defaults-object boundary — genuinely resolved.**
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation2.md:29:   defaults object, eligibility, application, and merge in BV1 and forbids their
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation2.md:32:   This is executable on the retained architecture. BV1 already owns source-local
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation2.md:34:   (`charters/BV1.md:22-27`), while the compiler contract keeps runtime object/array
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation2.md:49:   - EM-041 covers `svelte/runtime/expr_rewrite/*` as `Converge`, owned by `BS1+B4`.
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation2.md:53:     correctly keeps Svelte rewrite semantics in BS1 and moves final source-space
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation2.md:55:   - EM-042 covers `svelte/runtime/client_event.rs` as `Converge`, owned by `BS1+B4`.
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation2.md:58:     Its disposition preserves Svelte event topology under BS1 and routes fragments
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation2.md:60:   - EM-043 covers `svelte/runtime/client_effect.rs` as `Converge`, owned by `BS1+B4`.
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation2.md:63:     `:90-104` and `:121-173`. Its split between BS1 effect semantics and B4 final
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation2.md:66:     `BV1+B4`. The helper is a live synthetic Vue runtime-module emitter at `:13-52`,
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation2.md:68:     component semantics/topology in BV1 and gives B4 final fragment assembly and
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:4:**Prepared against:** local `program/architecture-lock` commit
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:8:[`../program-dag.toml`](../program-dag.toml), the live program ledger, the B2–B6 and
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:41:`36da8dc8841a35d3e1163e4b9bb5752f95ca527a`. Vue VDOM and Vue Vapor are separate
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:91:B4 -> {BV1, BS1}
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:92:{BV1, BS1} -> B5 -> B6
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:93:{C1, B3, B5} -> C2 -> C3
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:94:{B6, C3} -> C4
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:100:and shared lockfiles. BV1 and BS1 may overlap only after the same proof additionally
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:104:BF3 are individually accepted. B5 waits for both framework trains. C4 waits for B6
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:105:because prepared and batch routes do not exist before B6.
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:119:- [`../charters/BV1.md`](../charters/BV1.md): Vue semantic models, plans, VDOM,
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:122:- [`../charters/BS1.md`](../charters/BS1.md): Svelte-native client/server semantics,
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:127:[`B3.md`](../charters/B3.md), [`B4.md`](../charters/B4.md), [`B5.md`](../charters/B5.md),
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:128:[`B6.md`](../charters/B6.md), [`C3.md`](../charters/C3.md), and
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:138:products, client/server profile, development/production, Vue VDOM/Vapor, Svelte
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:213:| FC-VUE-001 | complete accepted Vue VDOM/Vapor/SSR pack |
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:251:BV1 exposes closed typed demands for project-aware imported Vue macro information.
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:315:> landing that byte-exact ratification bundle on `program/architecture-lock`, whose B1
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:347:> byte-exact ratification bundle on `program/architecture-lock`, whose B1 predecessor
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:353:`program/architecture-lock` by fast-forward from `b3249d13d07806a14a4307954dfcc459cf7301ac`.
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:359:On ratification this amendment supersedes only the conflicting B2–B6/C3–C4 scope and
docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:4:**Prepared against:** local `program/architecture-lock` commit
docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:27:BV1, B5, or EM-038.
docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:82:script/VDOM/Vapor/SSR paths is a separate, broader emitter-fidelity gap. Both
docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:329:   existing thresholds; no B3/B4/BV1/B5 authority, no universal IR, no waiver
docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:334:   BV1's complete Vue plan, B5's direct-core cutover, a universal or
docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:401:composition faithfully carried forward to zero, across script, VDOM, Vapor, and
docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:459:list (still forbidding B3/B4/BV1/B5 authority, both rejected identifier
docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:490:> rename, no B3/B4/BV1/B5 authority) is otherwise unchanged; redefine BV0A's
docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:672:> identifier rename, no B3/B4/BV1/B5 authority) is otherwise unchanged;
docs/arch/refactor/rev11/evidence/BA0/landing-record.md:132:| `segmented_overwrite_authority_is_unreachable_outside_the_crate` | FAILS on base |
docs/arch/refactor/rev11/charters/BS1.md:1:# BS1 — Svelte compiler-output conformance train
docs/arch/refactor/rev11/charters/BS1.md:23:BS1 does not implement a Svelte runtime, use an official compiler in production, or
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/conformance-challenge.md:102:The matrix contains 34 unique proposed cells and separates Vue VDOM, Vapor, and SSR;
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/conformance-challenge.md:217:`performance-impact.md:9-22` names harness, Vue VDOM/Vapor/SSR, Svelte
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/conformance-challenge.md:218:runes/legacy/server, route-overhead, and project-staging cells before a BF2/BV1/BS1
docs/arch/refactor/rev11/evidence/BA0/context-packet.md:10:`program/architecture-lock` at `dd84e5fa2`; BF3 ACCEPTED at `c6da941ee`. BA0, BS0,
docs/arch/refactor/rev11/package-README.md:4:**Supersedes:** Revision 10 and every earlier architecture-lock plan.  
docs/arch/refactor/rev11/charters/B6.md:1:# B6 — Prepared, direct-batch, and direct-core closure
docs/arch/refactor/rev11/charters/B6.md:3:**Status:** PROPOSED amendment / LOCKED. **Predecessor:** B5.
docs/arch/refactor/rev11/charters/B6.md:5:B6 exposes B5's accepted algorithms through prepared-first, prepared-repeat, and
docs/arch/refactor/rev11/charters/B6.md:12:route-overhead cells. C4 remains locked until B6 is accepted because these routes do
docs/arch/refactor/rev11/charters/C3.md:6:the closed typed project-information demands accepted in BV1 using the canonical
docs/arch/refactor/rev11/charters/C3.md:10:## Closed BV1→C3 demand protocol
docs/arch/refactor/rev11/charters/C3.md:27:still names only the inner props type argument. BV1 retains the wrapping syntax,
docs/arch/refactor/rev11/charters/C3.md:35:source-local macro syntax remain parser/BV1-owned. They never lower to `TypeExpr`,
docs/arch/refactor/rev11/charters/C3.md:66:BV1 converts an omitted result or any top-level `NotFound`, `Stale`, or `Error` arm
docs/arch/refactor/rev11/charters/C3.md:80:TSC/TSX, declaration, and imported-macro cell by running accepted BV1 codegen with C3;
docs/arch/refactor/rev11/charters/C3.md:82:BV1 outputs except for the declared projected information.
docs/arch/refactor/rev11/charters/B3.md:8:VDOM/Vapor; Svelte runes/legacy where applicable; normalized framework-local options;
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation.md:22:- `docs/arch/refactor/rev11/charters/BV1.md`
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation.md:37:### 1. BV1/C3 acceptance deadlock — resolved
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation.md:39:`charters/BV1.md:29-37` defines `FC-TS-001-LOCAL` as an independently closable BV1
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation.md:42:neither a C3 implementation nor a live project resolver. BV1's exit now requires that
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation.md:43:local criterion rather than the unsplit `FC-TS-001` (`charters/BV1.md:39-45`) and
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation.md:49:the retained DAG is BV1 -> B5 -> C2 -> C3 (`program-dag.toml:100-139`), but BV1's
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation.md:51:BV1 codegen. Both exits are explicit and independently satisfiable at their respective
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation.md:54:### 2. BV1-to-C3 demand protocol — resolved
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation.md:74:BV1 independently proves its side with typed deterministic stubs
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation.md:75:(`charters/BV1.md:29-35`), while C3 proves the real substrate behavior
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation.md:98:  to BV1+B4+B5.
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation.md:101:  plan-to-fragment topology under BS1, and assigns final assembly/map composition and
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation.md:104:  binds atomic artifact replacement to B4+B5 plus project-aware route identity to C4,
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-conformance.md:95:| E6 | `__expose` + slot-fallback-cache pass dev/prod, inline/non-inline, VDOM/Vapor/SSR | **MET**, one cosmetic defect → N1 |
docs/arch/refactor/rev11/charters/BV0.md:8:Correct the genuine Vue VDOM, Vapor, SSR, assembly, and mapping defects exposed by
docs/arch/refactor/rev11/charters/BV0.md:19:2. VDOM `<slot>` fallback static caching and the `CACHED` patch flag;
docs/arch/refactor/rev11/charters/BV0.md:36:typed refusal, or the complete BV1 official-case pack.
docs/arch/refactor/rev11/charters/BV0.md:67:production, inline and non-inline applicability, and VDOM/Vapor/SSR where applicable.
docs/arch/refactor/rev11/charters/BV0.md:73:script, VDOM, Vapor, and SSR paths, before this exit is met.
docs/arch/refactor/rev11/charters/BF3.md:39:Vue VDOM/Vapor/SSR runtime rows already owned and corrected by BV0 stay outside this
docs/arch/refactor/rev11/charters/B5.md:1:# B5 — Accepted framework algorithms through the sole direct compiler core
docs/arch/refactor/rev11/charters/B5.md:3:**Status:** PROPOSED amendment / LOCKED. **Predecessors:** BV1 and BS1.
docs/arch/refactor/rev11/charters/B5.md:5:B5 exposes the already accepted BV1 and BS1 algorithms through one borrowed one-shot
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/conformance-challenge-reattestation.md:17:removes Vue VDOM/Vapor/SSR probing and `BF3-RET-VUE-*` while preserving Svelte/
docs/arch/refactor/rev11/charters/BV1.md:1:# BV1 — Vue compiler-output conformance train
docs/arch/refactor/rev11/charters/BV1.md:14:- client VDOM and client Vapor generated JavaScript as separate families;
docs/arch/refactor/rev11/charters/BV1.md:25:BV1 does not implement Vue runtime code, invent `SSR x Vapor`, use an official
docs/arch/refactor/rev11/charters/BV1.md:29:`FC-TS-001-LOCAL` is BV1's independently closable partition of `FC-TS-001`. It
docs/arch/refactor/rev11/charters/BV1.md:31:BV1 side of the closed BV1→C3 protocol in `C3.md` with typed deterministic
docs/arch/refactor/rev11/charters/BV1.md:36:C3's later end-to-end partition; it combines accepted BV1 codegen with the real
docs/arch/refactor/rev11/charters/BV1.md:41:BV1's required exits additionally prove that every BV0 correction survives the final
docs/arch/refactor/rev11/charters/BV1.md:42:B2–B4 substrate and that the exact BV0 seed pack remains green. BV1 may replace a
docs/arch/refactor/rev11/charters/BV1.md:49:`FC-ZERO-WORK-001`, and applicable `FC-PERF-001` cells pass. Every BV1-owned or
docs/arch/refactor/rev11/charters/C1.md:5:`ACCEPTED` (`docs/arch/architecture-lock/ledger/program-state.toml:216,237,447`).
docs/arch/refactor/rev11/charters/C1.md:68:(`docs/arch/architecture-lock/ledger/program-state.toml:594`, `status = "READY"`). The program executes
docs/arch/refactor/rev11/charters/C1.md:69:one block at a time, and the ledger's `current_block` is `BV1`, `status = "IN_PROGRESS"`
docs/arch/refactor/rev11/charters/C1.md:70:(`program-state.toml:22,510-513`). **C1 dispatch follows BV1's landing.** This charter does not propose
docs/arch/refactor/rev11/charters/C1.md:71:running C1 in parallel with BV1 or with any other in-flight block.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/conformance-challenge.md:19:   (at review time) untouched scope document still ordered Vue VDOM/Vapor/SSR
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/conformance-challenge.md:32:cells; every cited implementation location (`MacroState.has_expose`, the VDOM slot
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/conformance-challenge.md:42:  follow-up commit to remove the Vue VDOM/Vapor/SSR probe-matrix rows and the
docs/arch/refactor/rev11/charters/BV0A.md:116:architecture, atomic artifact set, or publication transaction; BV1's complete Vue
docs/arch/refactor/rev11/charters/BV0A.md:117:semantic/conformance train; B5's sole direct compiler core; a universal or
docs/arch/refactor/rev11/charters/BV0A.md:237:existing thresholds. The change is Vue-only and adds no B3/B4/BV1/B5 authority, no
docs/arch/refactor/rev11/charters/BV0A.md:245:architecture, BV1's complete Vue plan, B5's direct-core cutover, a universal or
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation2.md:42:   `program/architecture-lock` at
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge-reattestation2.md:76:B4/BV1/BS1/B5/B6, and C1/C2/C3/C4 predecessor lists match AMD-005 `:82-105` and the
docs/arch/refactor/rev11/charters/BS0.md:13:owners before B2/B3. BS0 does not wait for the post-B4 BS1 conformance train and
docs/arch/refactor/rev11/charters/BS0.md:62:BS0 must not implement BA0, BCSS0, BRT0, B3, B4, or BS1 work; add production
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/governance-challenge-reattestation.md:19:VDOM/Vapor/SSR probing and `BF3-RET-VUE-*` while preserving Svelte/non-Vue-runtime
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation.md:24:- `docs/arch/refactor/rev11/charters/BV1.md`;
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation.md:31:1. **BV1/C3 acceptance deadlock — resolved.** `charters/BV1.md:29-37` now
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation.md:32:   defines `FC-TS-001-LOCAL` as BV1's independently closable producer/local
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation.md:33:   criterion and expressly requires neither C3 nor a live project resolver. BV1's
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation.md:34:   required exit at `charters/BV1.md:41-45` requires that local criterion while
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation.md:38:   the unchanged DAG remains BV1 → B5 → C2 → C3, but no BV1 prerequisite now
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation.md:41:2. **BV1→C3 demand protocol — resolved.** `charters/C3.md:12-21` closes the
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation.md:50:   prohibits empty or member-dropping silent success. BV1's local criterion also
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation.md:52:   `charters/BV1.md:29-35`.
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation.md:67:     BV1+B4+B5 and requires removal of the second session assembly path. The
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation.md:73:     owned by BS1+B4. The cited files exist: `client.rs:98-115` enters the module
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge-reattestation.md:77:   - EM-040 gives session `compile_entry` a `Replace` disposition owned by B4+B5+C4
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/README.md:17:Inspect framework separation, product ownership, B2/B3/B4/B5/B6/C3/C4 boundaries,
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/architecture-challenge-reattestation.md:17:removes Vue VDOM/Vapor/SSR probing and `BF3-RET-VUE-*` while preserving Svelte/
docs/arch/refactor/rev11/release/validation-report-v11.md:30:3303834589df23cd04338801374857e685d9961df3d323c60c4b58db54ce62ce  verter-architecture-lock-master-plan-v11.md
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:152:between the VDOM and Vapor backends" is false on both branches.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:155:`isVaporComponent` is literally `return type.__vapor` (line 7412), and it gates VDOM↔Vapor
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:194:VDOM non-inline, import-lowering order); 3 newly pass.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:196:VDOM improved broadly. **Every worst regression is `vapor non-inline`**, and they are semantic,
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:259:all carry the marker. `__vapor` is `isVaporComponent` in the runtime and gates VDOM↔Vapor interop.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md:296:**3. Stale comment** `// $setup dot notation (SSR uses _ctx.x like VDOM)` —
docs/arch/refactor/rev11/evidence/framework-conformance/validation.md:11:OK: program-state.toml (docs/arch/architecture-lock/ledger/program-state.toml) — validated 56 blocks (non-zero work asserted) against docs/arch/refactor/rev11/program-dag.toml in mode live
docs/arch/refactor/rev11/evidence/framework-conformance/validation.md:23:  --state docs/arch/architecture-lock/ledger/program-state.toml \
docs/arch/refactor/rev11/evidence/B4/debt-FC-B4-001-generated-chunk-callsite-migration.md:34:B4 / `verter_compiler::assembly`, jointly with the Vue chunk producers (BV1) — per the ruling
docs/arch/refactor/rev11/evidence/B4/debt-FC-B4-001-generated-chunk-callsite-migration.md:71:1. Its producing Vue chunk producer (BV1) declares the fragment's actual contextual grammar —
docs/arch/refactor/rev11/evidence/A0-preflight-blocked.md:54:| `verter-architecture-lock-master-plan-v11.md` | `3303834589df23cd04338801374857e685d9961df3d323c60c4b58db54ce62ce` | MATCH |
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:13:`program/architecture-lock` checkout inspected read-only at
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:23:  `docs/arch/architecture-lock/ledger/program-state.toml:236-255`
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:115:removed sections, and exactly `BF1`, `BF2`, `BF3`, `BV1`, and `BS1` added. Outside the
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:131:- removed: `B1→B2`, `B1→B3`, `B4→B5`, `B5→C4`;
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:133:  `B4→BV1`, `B4→BS1`, `BV1→B5`, `BS1→B5`, and `B6→C4`.
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:146:| BV1 | B4 |
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:147:| BS1 | B4 |
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:148:| B5 | BV1, BS1 |
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:149:| B6 | B5 |
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:151:| C2 | B3, B5, C1 |
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:153:| C4 | B6, C3 |
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:155:This matches the maintainer's required semantic shape, including both `B6` and `C3`
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:163:`vue_bridge.rs` spans B2/B3/BV1/B5, `strip_types/*` spans BV1/BS1, and the Svelte
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:164:carrier spans B2/B3/BS1/B5 (`emitter-mapping-dispositions.tsv:8,20-21`). For a future
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:165:BV1/BS1 overlap, `performance-impact.md:33-43` enumerates writable code, fixtures,
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/governance-challenge.md:170:The direct parent-to-candidate diff changes only `docs/arch/architecture-lock/ledger`
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:3:VUE-VDOM-CLIENT	Vue	core@3.6.0-rc.3	RuntimeClient-VDOM	dev/prod; inline/separate; maps on/off; normal/setup; JS/TS	reachable/default in current Vue client routes	VERIFY and BF3 probe	supported	Supported	BV1	FC-VUE-001	Exhaustive core axes; secondary options pairwise/high-risk.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:4:VUE-VAPOR-CLIENT	Vue	core@3.6.0-rc.3	RuntimeClient-Vapor	dev/prod; inline/separate; maps on/off; normal/setup; JS/TS	reachable when force_vapor/profile selects it	VERIFY and BF3 probe	supported	Preview	BV1	FC-VUE-001	Separate capability family; upstream RC prevents Stable.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:5:VUE-SSR	Vue	core@3.6.0-rc.3	RuntimeServer-SSR	dev/prod; maps on/off; normal/setup; JS/TS	reachable when server/ssr product requested	VERIFY and BF3 probe	supported	Supported	BV1	FC-VUE-001	Official SSR compiler topology and runtime executor.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:7:VUE-MACRO-LOCAL	Vue	core@3.6.0-rc.3	Runtime+TypeProducts	script setup source-local macros; dev/prod; client/server	reachable	VERIFY and BF3 probe	supported	Supported	BV1	FC-VUE-001	No project provider required.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:8:VUE-MACRO-IMPORTED	Vue	core@3.6.0-rc.3	Runtime+TypeProducts	imported/project-aware types	reachable on project-aware routes	VERIFY	projection-required	Preview	BV1+C3	FC-TS-001	BV1 emits closed typed demands; C3 fulfils them.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:9:VUE-SCOPED-SLOTTED	Vue	core@3.6.0-rc.3	RuntimeClient+RuntimeServer	scoped/slotted metadata; CSS vars; maps on/off	reachable	VERIFY and BF3 probe	supported	Supported	BV1	FC-VUE-001	CSS variables that affect generated code are semantic input.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:10:VUE-CUSTOM-ELEMENT	Vue	core@3.6.0-rc.3	RuntimeClient	dev/prod; normal/setup; maps on/off	reachable via current custom-element settings	VERIFY and BF3 probe	supported	Preview	BV1	FC-VUE-001	Canonical matcher/options only.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:11:VUE-TEMPLATE-OPTIONS	Vue	core@3.6.0-rc.3	RuntimeClient+RuntimeServer	comments; whitespace; delimiters; hoist; cache; bindings; components; directives; events; props/attrs; class/style; slots; fragments; Teleport; Suspense	reachable through mixed current flags/defaults	VERIFY and BF3 probe	supported	Supported	BV1	FC-VUE-001	No public option silently ignored.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:12:VUE-ASYNC-SETUP	Vue	core@3.6.0-rc.3	RuntimeClient+RuntimeServer	async setup; Suspense; hydration	reachable	VERIFY and BF3 probe	supported	Preview	BV1	FC-HYDRATION-001	Requires client/server/runtime and cross-pair evidence.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:13:VUE-PUBLIC-API	Vue	core@3.6.0-rc.3 + owned TS domain	PublicApi	public/testing where established; local/project-aware	reachable through host/NAPI/WASM products	VERIFY and BF3 probe	supported	Supported	BV1+C3	FC-TS-001	Testing-only API modes remain test-only, not public widening.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:14:VUE-TSC	Vue	core@3.6.0-rc.3 + owned TS domain	Tsc/TSX	local/project-aware; JS/TS; macro variants	reachable	VERIFY and BF3 probe	supported	Supported	BV1+C3	FC-TS-001	TypeScript compiler/API oracle; language-tools excluded.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:15:VUE-DECLARATION	Vue	core@3.6.0-rc.3 + owned TS domain	Declaration	local/project-aware; JS/TS; macros	reachable	VERIFY and BF3 probe	supported	Preview	BV1+C3	FC-TS-001	Established declaration product only.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:19:SVELTE-CLIENT-RUNES	Svelte	svelte@5.56.8	RuntimeClient	runes; dev/prod; maps on/off; comments/whitespace; HTML/SVG/MathML where claimed	reachable through current Svelte client route	VERIFY and BF3 probe	supported	Supported	BS1	FC-SVELTE-001	Svelte-native helpers/effects/blocks/events/component/DOM topology.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:20:SVELTE-CLIENT-LEGACY	Svelte	svelte@5.56.8	RuntimeClient	legacy where official/applicable; dev/prod; maps on/off	reachable/default behavior may select legacy	VERIFY and BF3 probe	supported	Supported	BS1	FC-SVELTE-001	Every claimed legacy form needs manifest coverage.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:21:SVELTE-SERVER-RUNES	Svelte	svelte@5.56.8	RuntimeServer	runes; dev/prod; maps on/off	current backend explicitly unavailable/fail-closed	unsupported today	supported	Preview	BS1	FC-SVELTE-001	No success until complete official server pack.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:22:SVELTE-SERVER-LEGACY	Svelte	svelte@5.56.8	RuntimeServer	legacy where official/applicable; dev/prod; maps on/off	current backend explicitly unavailable/fail-closed	unsupported today	supported	Preview	BS1	FC-SVELTE-001	Separate capability evidence from runes.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:23:SVELTE-COMPONENT	Svelte	svelte@5.56.8	RuntimeClient+RuntimeServer	component compile; scripts/styles/templates	reachable	VERIFY and BF3 probe	supported	Supported	BS1	FC-SVELTE-001	Established SFC/component compilation.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:24:SVELTE-MODULE	Svelte	svelte@5.56.8	ModuleJavaScript	module compilation; client/server	claim not proven at package preparation	VERIFY	unsupported fail-closed	Experimental	B3+BS1	FC-CAPABILITY-001	May be promoted only if BF1 proves an established product claim; no automatic widening.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:25:SVELTE-SEMANTIC-CORE	Svelte	svelte@5.56.8	RuntimeClient+RuntimeServer	snippets; blocks; effects; events; bindings; actions; transitions/animations; stores; context; slots/children; components; dynamic elements/components; boundaries; async	reachable in portions	VERIFY and BF3 probe	supported	Supported	BS1	FC-SVELTE-001	Every claimed feature uses Svelte-native topology.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:26:SVELTE-CUSTOM-ELEMENT	Svelte	svelte@5.56.8	RuntimeClient	customElement boolean; inline tag/shadow/per-prop attribute/reflect/type; dev/prod; maps on/off; extend fails closed	current exposure incomplete	VERIFY	supported	Preview	BS1	FC-SVELTE-001	Canonical source-authored custom-element descriptor fields are claimed; arbitrary extend callback/identifier remains unsupported fail-closed.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:27:SVELTE-ASYNC-EXPERIMENTAL	Svelte	svelte@5.56.8	RuntimeClient+RuntimeServer	experimental.async; boundaries; hydration	current claim not proven	VERIFY	experimental	Experimental	BS1	FC-HYDRATION-001	Explicit request only; never enabled by unknown option.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:28:SVELTE-HYDRATION	Svelte	svelte@5.56.8	RuntimeClient+RuntimeServer	dev/prod; runes/legacy where applicable	reachable client; server absent	VERIFY	supported	Preview	BS1	FC-HYDRATION-001	Official/official control, Verter/Verter, meaningful cross-pair.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:29:SVELTE-PUBLIC-API	Svelte	svelte@5.56.8 + owned TS domain	PublicApi	component public/declaration modes	reachable through host projections	VERIFY and BF3 probe	supported	Preview	BS1	FC-TS-001	No output widening from extra official artifacts.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:30:SVELTE-TSC	Svelte	svelte@5.56.8 + owned TS domain	Tsc/TSX	component API and diagnostics	reachable	VERIFY and BF3 probe	supported	Preview	BS1	FC-TS-001	TypeScript observable oracle; language-tools excluded.
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:31:SVELTE-DECLARATION	Svelte	svelte@5.56.8 + owned TS domain	Declaration	component declaration product	reachable	VERIFY and BF3 probe	supported	Preview	BS1	FC-TS-001	Established product only.
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge.md:31:  `docs/arch/refactor/rev11/charters/BV1.md:22`,
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge.md:32:  `docs/arch/refactor/rev11/charters/BV1.md:26`,
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge.md:37:  BV1→C3 boundary may carry only closed project-aware imported Vue demands; C3 must
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge.md:47:  parser/BV1-owned. A defaults object is source-local JavaScript syntax, not imported
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge.md:51:  path; the second violates BV1 semantic/codegen ownership. As written, BV1 cannot
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge.md:86:- **Framework separation:** The B2, B3, BV1, BS1, boundary, and SSR contracts otherwise
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge.md:91:- **B2–B6 and C3–C4 boundaries:** B2 remains syntax-only; B3 owns request/default/
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge.md:92:  prerequisite planning; B4 owns units/maps/publication; B5 and B6 own direct and
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge.md:102:  and 56 candidate blocks. Exactly `BF1`, `BF2`, `BF3`, `BV1`, and `BS1` were added.
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge.md:103:  Among original blocks, only B2, B3, B5, and C4 predecessor lists changed; every other
docs/arch/refactor/rev11/evidence/framework-conformance/reviews/architecture-challenge.md:105:  retains `B3`, `B5`, and `C1`; all unaffected D–L edges are preserved.
docs/arch/refactor/rev11/evidence/B4/debt-FC-B4-003-scaffold-text-import-fact-drift.md:63:BV1 (Vue chunk producers), BS1, or B5 semantic work.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:7:EM-006	crates/verter_compiler/src/framework_common/registered_carrier_projection.rs and registry.rs	registered framework dispatch/projection	Converge	B2+B5	Keep neutral dispatch only; parsing and semantic/codegen owners remain framework-local.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:8:EM-007	crates/verter_compiler/src/framework_common/vue_bridge.rs	Vue parse carrier, options conversion, compile/assembly bridge	Converge	B2+B3+BV1+B5	Split parse, canonical request conversion, Vue semantics, and direct route into their owning blocks.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:9:EM-008	crates/verter_compiler/src/compile/mod.rs	current Vue orchestration and multi-product assembly	Replace	B5	Final sole direct core consumes accepted BV1 plans and B4 publication; no semantic repair in B5. Interim B4 increment landed: this file's `VerterScriptBlock` producers (Options API, Composition API, minimal/self-closing setup, template-only synthetic script) now declare an `assembly::fragment::SfcExportPlacement` fact for every `__sfc__` rename/export-removal target at write time, consumed by `verter_session`'s `rewrite_script` instead of a generated-text scan — the export-IDENTITY fact-declaration half of what this row's eventual replacement needs; the orchestration/multi-product assembly this row actually tracks is unchanged and still B5's to replace.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:11:EM-010	crates/verter_compiler/src/script/*	Vue script preparation/macro/code emission	Converge	BV1	Converge into Vue semantic model/product plans with local macro and typed project-demand boundaries.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:12:EM-011	crates/verter_compiler/src/template/code_gen/mod.rs,types.rs,walker.rs	Vue template dispatcher and shared Vue codegen mechanics	Converge	BV1	Allowed as Vue-local staged IR/mechanics; no Svelte consumer and no cross-framework IR.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:13:EM-012	crates/verter_compiler/src/template/code_gen/vdom/*	Vue VDOM emitter	Converge	BV1	Correct against RC.3 VDOM pack and remove semantic divergences.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:14:EM-013	crates/verter_compiler/src/template/code_gen/vapor/*	Vue Vapor emitter	Converge	BV1	Separate Vapor capability family and runtime-vapor topology.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:15:EM-014	crates/verter_compiler/src/template/code_gen/ssr/*	Vue SSR emitter	Converge	BV1	Official compiler-ssr/server-renderer topology; no invented Vapor server backend.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:16:EM-015	crates/verter_compiler/src/template/code_gen/binding.rs,expression.rs,shared/*	Vue binding/expression/helper emission	Converge	BV1	Remain Vue-owned and validated per VDOM/Vapor/SSR family.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:17:EM-016	crates/verter_compiler/src/ide/*	current Vue IDE/TSX emitter	Converge	BV1	Converge into established Vue TypeScript-visible product plans and exact TS observations.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:18:EM-017	crates/verter_compiler/src/tsc/*	current Vue TSC/declaration emission and maps	Converge	BV1+B4	BV1 owns product semantics; B4 owns mapping composition; language-tools excluded.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:19:EM-018	crates/verter_compiler/src/css/* and style_planner.rs	current Vue style rewrite/output owners	Defer	J1+J2	CSS artifact ownership remains J1/J2; BV1 owns only framework semantic metadata and CSS effects on generated code.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:20:EM-019	crates/verter_compiler/src/strip_types/*	JavaScript/TypeScript syntax stripping utility	Preserve	BV1+BS1	Preserve only as syntax utility with framework-owned invocation and independent output/map proof.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:21:EM-020	crates/verter_compiler/src/svelte/carrier.rs	Svelte parse/eval/IDE/runtime carrier bridge	Converge	B2+B3+BS1+B5	Split responsibilities into parser, canonical request, Svelte plans, and sole direct route.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:23:EM-022	crates/verter_compiler/src/svelte/ide/*	Svelte IDE/TSX emission	Converge	BS1	Svelte-owned TypeScript-visible plan; no Vue semantic/helper assumptions.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:24:EM-023	crates/verter_session/src/framework/api_projectors/svelte.rs	Svelte public/declaration projector and source-map builder	Converge	BS1+B4	BS1 owns product semantics; B4 owns source-space mapping; blanking cannot reconstruct semantic output.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:25:EM-024	crates/verter_session/src/framework/api_projectors/vue.rs	Vue public/declaration projector	Converge	BV1+C3+B4	BV1 local semantics, C3 typed project projection, B4 maps.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:26:EM-025	crates/verter_compiler/src/svelte/runtime/{ir.rs,ops.rs,lower_*.rs,client_plan*.rs,component_scope_facts.rs}	Svelte semantic/lowering/client plan owners	Converge	BS1	May remain narrow Svelte-local IRs; must prove no Vue dependency or assumption.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:27:EM-026	crates/verter_compiler/src/svelte/runtime/{client_emit.rs,client_*_emit.rs,expr_emit.rs,template_serialize.rs}	Svelte client JavaScript emitters	Converge	BS1	Accepted only against exact helper/effect/block/event/component/DOM topology.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:28:EM-027	crates/verter_compiler/src/svelte/runtime/{client_imports.rs,helpers.rs,render_callees.rs,naming.rs}	Svelte helper/import/callee/name emission	Converge	BS1	Official import sources/helper families/call order protected from normalization.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:29:EM-028	crates/verter_compiler/src/svelte/runtime/css/render.rs	Svelte CSS renderer	Converge	BS1+J1+J2	BS1 proves generated-code effects; CSS owner blocks retain artifact policy.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:30:EM-029	crates/verter_compiler/src/svelte/runtime/output.rs and client_output.rs	Svelte runtime output assembly	Replace	B4+BS1	Framework plan emits fragments; B4 performs final atomic assembly/publication.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:31:EM-030	crates/verter_compiler/src/svelte/runtime/diff_oracle_divergences.rs	semantic known-divergence allowance owner	Delete	BS1	A successful supported cell retains no semantic known-divergence allowlist.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:32:EM-031	crates/verter_compiler/src/svelte/runtime/{official_reject.rs,official_rule.rs,unsupported.rs,client_surface_refuse.rs}	current refusal/official-rule owners	Converge	BF3+B3+BS1	Interim guards are typed BF3 records; canonical capability rejection moves to B3; correction guards delete at BS1 acceptance.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:33:EM-032	crates/verter_compiler/src/svelte/runtime server emitter (currently absent)	no current RuntimeServer producer	Defer	BS1	BS1 introduces a Svelte-owned server plan/emitter only after B4; no Vue SSR reuse.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:34:EM-033	crates/verter_compiler/src/standalone.rs	legacy standalone compile route	Replace	B5	Converge callers onto sole direct compiler core or delete route.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:39:EM-038	crates/verter_session/src/compile.rs::assemble_vue_main_module	shipped host-owned Vue main-module import/render/HMR assembly	Replace	BV1+B4+B5	Replace host text rewriting and final-module assembly with BV1-owned Vue fragments assembled and atomically published by B4 through the sole B5 direct core; no second session assembly path survives. Interim B4 increments landed: (1) the `template.code.contains("function ssrRender(")`/`"function render("` generated-product reparse this row's own "no second session assembly path" language forbids is deleted — the export binding now comes from `RuntimeTemplateBlock::render_export: TemplateRenderExport`, a fact declared by the codegen backend (`crates/verter_compiler/src/compile/mod.rs`) and threaded through unchanged; (2) the `__sfc__`→`_sfc_main` rename and the terminal `export default` removal are likewise no longer a generated-text scan — every runtime-emission site (`script/process.rs`, plus the template-only synthetic script and `empty_sfc_script_block` in `verter_compiler`'s `compile` module) declares an `assembly::fragment::SfcExportPlacement` fact at write time, threaded through `VerterScriptBlock`/`RuntimeScriptBlock` unchanged, and `assemble_vue_main_module` now builds its fragments through `assembly::compose::assemble_sequence` and publishes through `assembly::publish` (atomicity + final-parse checks) rather than the session-private `MapComposer`/`ModuleWriter`/`FragmentWrite` — see `docs/arch/refactor/rev11/evidence/B4/debt-FC-B4-002-vue-main-module-composer-cutover.md` (RESOLVED). Full BV1-fragment/B5-publish migration (moving fragment OWNERSHIP itself to BV1, not just the export-identity facts) is still future work.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:40:EM-039	crates/verter_compiler/src/svelte/runtime/client.rs and client_module_frame.rs	primary Svelte client module, import-prelude, root-factory, body, epilogue, and source-map emitters	Converge	BS1+B4	Keep Svelte-owned plan-to-fragment emission and protected module topology under BS1; B4 owns final fragment assembly, source-space map composition, and atomic publication.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:41:EM-040	crates/verter_session/src/host_resolve/virtual_file_pipeline.rs::compile_entry	current session Main/Script/Template/Style/Custom and IDE artifact assembly, Template import injection, and map selection/attachment	Replace	B4+B5+C4	B4's atomic artifact set replaces host reconstruction and map reattachment; the session becomes a thin serving route over B5 results, and C4 proves project-aware route identity without a second assembly owner. Interim B4 increment landed: the Template-slot import-inject (`~3376-3390`) prepended `format!("import {{ {} }} from ...")` text ahead of `template.code` WITHOUT adjusting the already-produced map (a `CodeTransform`-single-source-of-truth violation — every mapped position desynced by one line whenever the template declared runtime imports). Now routed through `assembly::prepend_preamble`, which decodes/shifts/re-encodes the map. Script/Style/Custom/IDE slot construction and the map-selection/attachment plumbing elsewhere in `compile_entry` are unchanged — those slots are NOT wired onto B4's `ArtifactSet` in any form, wired or interim. Main-slot construction: `compile_entry` still calls `assemble_vue_main_module` directly (unchanged call site — the stable entry point external callers like `verter_vue_conformance` already depend on), but that function's OWN internals now build an `ArtifactSet` via `assembly::publish` and unwrap it, so the Main slot's bytes flow through B4's atomicity/final-parse checks one call away rather than not at all. `AssembledVueModule`'s return shape is NOT unchanged, though: a hardening round added a `lang: String` field (the module's derived dialect, reused by both call sites in this file instead of each independently re-deriving `main_lang`) alongside the existing `code`/`source_map` fields. That same hardening round (see `debt-FC-B4-002`'s addendum) closed the Main slot's remaining gaps: it now composes real, individually-validated `Fragment`s (not a raw code/map pair), returns a typed `VueMainAssemblyFailure` instead of panicking on a malformed producer-supplied byte (including the one remaining `.expect()` a later review round found and closed — same addendum), and validates/final-parses under the module's own derived dialect instead of a fixed permissive default. None of this extends past the Main slot — Script/Template/Style/Custom/IDE slot construction and the request-wide multi-slot `ArtifactSet` cutover this row's disposition actually names are still fully future work (BV1/BS1 own those slots' emission topology; touching them is out of this charter's "no Vue/Svelte semantics" bound), owned jointly with B5 (direct core) and C4 (project-aware route identity) per this row's disposition.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:42:EM-041	crates/verter_compiler/src/svelte/runtime/expr_rewrite/*	Svelte expression/script rewrite emitter and expression-local source-range authority	Converge	BS1+B4	BS1 retains Svelte scope-aware reactive rewrite semantics and typed refusal; B4 integrates the emitted ranges into canonical source-space composition so no expression-local map authority escapes fragment assembly.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:43:EM-042	crates/verter_compiler/src/svelte/runtime/client_event.rs	Svelte event registration, wrapper, and delegate-epilogue emitter	Converge	BS1+B4	BS1 owns official event/helper/call topology; emitted mapped handler fragments enter B4's canonical fragment and map assembly path.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:44:EM-043	crates/verter_compiler/src/svelte/runtime/client_effect.rs	Svelte template-effect memoization and mapped effect-body emitter	Converge	BS1+B4	BS1 owns effect grouping, dependency, and call topology; B4 owns final placement, source-space map composition, and atomic publication of its mapped fragments.
docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:45:EM-044	crates/verter_compiler/src/compile/helpers.rs::empty_sfc_script_block	Vue empty-SFC synthetic runtime-module emitter	Converge	BV1+B4	BV1 retains the valid empty-component semantics and exact Vue shell topology as a Vue-owned plan; B4 assembles and atomically publishes its synthetic fragments without a hidden final-module owner.
docs/arch/refactor/rev11/evidence/B4/debt-FC-B4-002-vue-main-module-composer-cutover.md:123:  packages stays explicitly out of scope (BV1's conformance-harness proof, not B4's).
docs/arch/refactor/rev11/evidence/framework-conformance/validate-package.mjs:233:  BV1: ["B4"],
docs/arch/refactor/rev11/evidence/framework-conformance/validate-package.mjs:234:  BS1: ["B4"],
docs/arch/refactor/rev11/evidence/framework-conformance/validate-package.mjs:235:  B5: ["BV1", "BS1"],
docs/arch/refactor/rev11/evidence/framework-conformance/validate-package.mjs:236:  B6: ["B5"],
docs/arch/refactor/rev11/evidence/framework-conformance/validate-package.mjs:238:  C2: ["B3", "B5", "C1"],
docs/arch/refactor/rev11/evidence/framework-conformance/validate-package.mjs:240:  C4: ["B6", "C3"],
docs/arch/refactor/rev11/evidence/framework-conformance/validate-package.mjs:257:  "docs/arch/architecture-lock/ledger/program-state.toml",
docs/arch/refactor/rev11/evidence/framework-conformance/package-checklist.md:7:| BF1/BF2/BF3/BV1/BS1 charters | `charters/{BF1,BF2,BF3,BV1,BS1}.md` |
docs/arch/refactor/rev11/evidence/framework-conformance/package-checklist.md:8:| B2/B3/B4/B5/B6/C3/C4 amendments | `charters/{B2,B3,B4,B5,B6,C3,C4}.md` |
docs/arch/refactor/rev11/evidence/B4/finding-frozen-w13-superseded-by-render-export-fact.md:29:the backend already used to choose VDOM/Vapor vs SSR codegen) and consumed exhaustively:
docs/arch/refactor/rev11/evidence/B4/context-packet.md:33:scan required BV1-owned script-producer changes — ruled B4-in-scope,
docs/arch/refactor/rev11/evidence/B4/context-packet.md:39:correctly assigned jointly to not-yet-run B5/C4 in the ratified ledger;
docs/arch/refactor/rev11/evidence/framework-conformance/current-state.md:14:- local `program/architecture-lock`: the same base commit/tree
docs/arch/refactor/rev11/evidence/framework-conformance/current-state.md:15:- `origin/program/architecture-lock`: `ff3728e1768d5ad09123c2221e3847150c6d9723`
docs/arch/refactor/rev11/evidence/framework-conformance/current-state.md:24:read-only Git inspection. It was on `program/architecture-lock` at
docs/arch/refactor/rev11/evidence/framework-conformance/current-state.md:68:  --state docs/arch/architecture-lock/ledger/program-state.toml --mode live
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
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:30:VUE-3E85501FF1EB10D442AE	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:699:5	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	88feb034633040898ef38024e72900f802cbec19633b025a1a0c21e603a4aa47	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:31:VUE-3B3D9EB3BDF3489D4499	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:70:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	87fd679018661daac95ebbda804247c472bd17c5a9ceaca86e28d344649db5dd	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:32:VUE-D54A53F9F673495225CD	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:720:5	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	791844d1fd97f77c6079743267c1973de9b96048f309c7e93eac67b4f2218ed2	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:33:VUE-3E6AA5E6C23252DB737A	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:741:5	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	2a5cd14f338ac374d504316aa0fabd65cbe1e953abb508be9070260188c0912a	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:34:VUE-59E98ACEA1253436A30F	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:84:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	4c487cf335e1c51345a77db6ed2f7ec60fc09ecd1cfdedd3149845953ce083a5	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:35:VUE-80CBA5BBD5B03ADA6B1D	compiler-core	packages/compiler-core/__tests__/codegen.spec.ts:96:3	34386ce6930b81eeae3d9aec3a967a10b838ca44	single-declaration	StringLiteral	25c72e3eb6c0e2601712db0af0793142a9050bab54753195e9576f528ed88ec0	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:36:VUE-FCCE8C307E7188C2D43E	compiler-core	packages/compiler-core/__tests__/compile.spec.ts:112:3	995741091df6082d50abe479b8a9e9d6348beb9c	single-declaration	StringLiteral	28094b23a5dfd404a9a9f1f1cddff2632fac65b97a07c86b65234374d93ea8d3	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:37:VUE-A6FD2F7B55A1BCD6781B	compiler-core	packages/compiler-core/__tests__/compile.spec.ts:189:3	995741091df6082d50abe479b8a9e9d6348beb9c	single-declaration	StringLiteral	63105277ce697ff1687428acb01d7c2b5354e92aa55406ba26d70f430a454998	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:38:VUE-420AF4306508A80F9DD4	compiler-core	packages/compiler-core/__tests__/compile.spec.ts:47:3	995741091df6082d50abe479b8a9e9d6348beb9c	single-declaration	StringLiteral	0ea66cd62ac8337d86adc58bf6e59e463707a78caa547bc8c934533bcf272a53	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:39:VUE-64DAA671AE411031C55B	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1068:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	00e6452ccf938b6e490fd1cc0d1820e247011d59773c3f10f02812b9de11db62	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:40:VUE-8C91E22592640936C5D2	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:114:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	992718373962799eb1802dfdfcdcc061c70fe11f97d0ab7351f37a143705e16c	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:41:VUE-BB5C1AA778A769D15224	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1175:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	b9da631c908887d3202019991ac346583afb484f2b714daa2d38a0e966699517	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:42:VUE-90792B3954322AC821D7	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1219:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	0ebbe27625fa3f9e1d6e1251fdb1fdf004ed55c90a8f98b39c3ac9f3c9fb4e02	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:43:VUE-0EECEB90152907740D2F	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1238:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	51263a1025effd593a086ddab7f7b59a30d32063c6778b5fc1028af644b35f35	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:44:VUE-0159759984C7F9596150	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1267:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	11aa3c6b8ecb614af4b42280db704cbabf11aba194635db45134a71aa7a20f65	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:45:VUE-1EECC026C18B0AD43AC8	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1297:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	d90c09d7f6dbb6d43e95cf9035b9c563c7f1e63ad27db6b41baf99b23aeadb6a	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:46:VUE-716F5689269C02EB3854	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1310:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	f2a84d47467e585a20180f4deff06bd38d70d3d42fe2ac7459979c75895bd6a8	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:47:VUE-6A6028052B112E7DEF88	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1323:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	a6dab01d1697d63d06fed775d5faffdccfc8aa6853be9a996ea4ae5710ea401d	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:48:VUE-158DA5085B931AFAD30C	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1352:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	f7206d98517cabe07e9b12225c1e8741a722742a97e3bd3567aa96877e51f748	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:49:VUE-34E1ADCE2A002B11AAC6	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:139:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	7b1e1231e11b786b057b82a183dd4982f71fef12316547f848a335380eeab2e9	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:50:VUE-ABDCFF3314A46C289EC9	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1391:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	85562cfd65a7dc6b58958162832528dc746e9877fae6748f139b4ff4ec4b441b	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:51:VUE-EA22377ED9CED6D8B36C	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1449:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	574b771e0756ee3aa7fdfa3907ccbb88a0f6ec6551249107fd28d5da58d1b1cb	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:52:VUE-2695434430FCDC97DE68	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1517:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	2724de98e0cdc273d106c8fb56021c69d8ab00bbe552b859a39da87d178002f4	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:53:VUE-A8120426A4C7F4C92B0A	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1566:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	0979be1d4e629b02475d1ebe80d606a85f24cac1965a9d9a1aaa0d5c8760169d	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:54:VUE-A45A73B828BB93A0EF7F	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1593:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	2dbbdbdfd161787c96f0106e7dce3f74152435bbf7ef698aacdaa15ccc7d46ec	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:55:VUE-89B14B07CDDA13A4E186	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:160:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	c24728c49ce91dc890c59df956ce1e0ddcf72ccf8f1b47118960052a90d31406	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:56:VUE-B80FAB9FB0796221EF9B	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1632:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	d777414acf83a43799d9410ee4364b7c9c53250d23234e74c45ba63f9ac0a0cb	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:57:VUE-F7E5CB66CA5DD42C5A5B	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1691:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	f8c61b5293e05942dc0b585090aa0de6584b24faabc1cccd35e3017e9db8b6a2	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:58:VUE-E72DE250BDE82831EAA0	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1751:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	3b84d9b9133a9717cdbd89491da74e80f6aaf2ae93b1bcb0327ff14c2f4f07d3	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:59:VUE-4B5B8B4C262C50E10A97	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1791:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	f08dd696faedbba5c5bf3441f06f13935e794ad23c6288031e05840b4367805f	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:60:VUE-8EBF3C2C6F42F5FEE2F1	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:183:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	11d766508c84b130e0039ff039be2c5adbd52fa1c69fc3beb67c48c23c9bc367	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:61:VUE-FBF81D4C00C4C63161D9	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1851:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	af35220fbe6160780be7ee47215134bfced9299faccefbf1017c9d10c02df09d	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:62:VUE-310B2555E35165779715	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1892:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	9b607b8118f751ce9d2ad98dd0259658c99c2dad994ec2ad12c403e4b0b6f4eb	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:63:VUE-E073731FF21AEEDE05E8	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1921:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	c3b1115ddf466cfb0b5394244786f5b28f97cfaf043fe53d45e220b107c22128	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:64:VUE-F3DEF4F1247C66905AA6	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:1996:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	92a2f84e1cdef8d5d5c86ddb9fef7e7e5065596fa41e6da157d70c8b3ebcab0a	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:65:VUE-00C3759975C506B3883B	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2039:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	263b5bc1c719e3a84d9119c7bd65511c63f0648020b902396f15dca05bbd7e3f	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:66:VUE-4F80A5E10591DEB1CD2C	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:208:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	c67e2a44ca95a54f9ef5d166ecff9d9754dbb333862e8b0a2bad09d829375ab7	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:67:VUE-9E1D2392C9A6CAA3AE26	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2086:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	fb9061b86191e8c5dbee016f6853257f5e7c0c4138285920647c45554229a2ff	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:68:VUE-AB729A6ED6866D03EE70	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:21:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	1d868628e1f4d79603ee32114a60ef168077e49fb37ad492032ff6cc42fb4ad3	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:69:VUE-A138896A539955587CA5	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2104:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	82918c9cd3b9f80050f581b5b89184a85a40a454be3c63cb8b84c21ce54e3f4f	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:70:VUE-A28E2EE2E3699643A562	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2111:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	b706774779188cba6f84be0ae91f1f676a0147f8ee720fb07a10ca42393fd9e0	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:71:VUE-611453737B986F339F19	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2124:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	aa36459f164a3ac6f107d8bf61edb9e8d082b14ee33b9bcc004e0e2333008f72	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:72:VUE-34F52422718E7D8B365F	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2148:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	b5d4a0f94e37b47937d0a94b13e7fdf2d468b0ca0394f7f7756289f98738c709	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:73:VUE-477886344F9A435F4277	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2188:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	74c0ee13011c1147bcf324fb334368e1c9ce055ce74504bc791d0d35c6cf60b6	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:74:VUE-CA4F567610399F5CBE0A	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2232:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	b11d593b1f7889b9793105329b3ab626375519ac82b820f9c35b9974cf4bbdf5	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:75:VUE-444CBA6667E05C7734AB	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2248:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	3ccae8a479dbf4d0fe2b152355def97246f424160a29b7931119a67434c3d314	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:76:VUE-5AB0E8B760A02FC8874E	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2260:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	2f46be5a61d8e3fa5eda37e01828c6ba53bc1a536424737f1e28654eda4d9b98	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:77:VUE-B7B0EBE10BEA680EE083	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2265:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	52085b8d48ccd56cfe2af967178baf7aeb35827027e3ae1ebe7778edc328b76a	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:78:VUE-ECB3FD1E5057586C327C	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2275:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	7de3a9f7467f6d784552fb480e58831ca8b31452346af226950d37660fe7e524	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:79:VUE-0DAA4F9001C74D49B89A	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2282:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	d57ea8b6ed4b370c6da3bfda623d30aa1d23daa778735de61f5248bf1a93972f	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:80:VUE-21FEEC679F67E19262AC	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2290:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	b59f4202b0908a56690f1c2ee04a1942d03a3b995faca667aaa629ac1364b37f	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:81:VUE-AA5D76662613712ADB8D	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2308:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	72d26fac487dbdda9ca81ff8d35681fae294aadbc506ddbb75b13c2afaf78936	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:82:VUE-5D576D64CDB091D0EB27	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2313:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	b698d785cf03443be8367d803df62bb99cd2206b756a6f2b03a2e1302cc17c50	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:83:VUE-FE0E86621BD7E73AE6DB	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2319:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	d3834a6c4f62c1ec2ce503ad2d1f625c33670109f854b2e46737996823d3a798	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:84:VUE-400E8A8349A600DF5026	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2327:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	e5c5e4f6cdea862e525bd0f685c39b75af4b1557114dd13921e5348d232de9c8	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:85:VUE-6B0AB6B873D5E642152C	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:233:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	6b38c603573d555c970e7adee639ab6de891a235e3d67ce82d0ce55009cd61d1	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:86:VUE-9EE19B599615957F867F	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2335:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	918668d9d22f501a2ce3ee75901a6df6f469bcd4a69b919bac0dcaa2317f5edf	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:87:VUE-A0AE897EBF6E730A1E0D	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2346:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	21de60cdf49786bb6d53a755fcf329d9ce01314a9ea8c023cd7ed35c0c5c436a	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:88:VUE-ED5000D680F99F396CEC	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2357:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	4e7b538e395c81b09f75219168ddf989b091155700a615050521d82cd97b0f65	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:89:VUE-2A9D7057E9157D16C4C1	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2369:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	fcd93ed520a3dcc441949e315bde71de6b688ca3a78415604dbdaf664bc79160	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:90:VUE-A65F613D01677F3EB4F1	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2374:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	7824f2a46d87f0ac97bf86d106184a78ab81d99088fe8367f5742696fdefe59e	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:91:VUE-F73356EFE68AC5E4EFF0	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2385:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	68f59d3892e4e251db032a5c7d2fad2e629d2785e792288f5f43b5969dfc8ac7	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:92:VUE-B945E7DE33B5B306AAC6	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2396:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	9cda83976c4610593945745014e31038e12a962e8ee2866e097c3851ec3836e8	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:93:VUE-35BB9391C1D4AC5C82B0	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2405:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	29f38fa89a41e76967f28a96d1046f865fbe88bc380e45ad7793201a635ee5ab	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:94:VUE-F169541E2914FBC82FA7	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2422:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	641d09ffd8ac162f9797712de7156bebd06295547422e2b2d7e7bec2dae0035f	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:95:VUE-39F7B13D198E5882289C	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2427:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	67091f7cfd3aec7f8e52f714527ed4de38fe005a33f59a2a79c0c89369696b63	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:96:VUE-865ECC1C737E0C969739	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2439:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	82b30b39783f5abbfcaa442cb9d7cedc81484dc20225399e47743d52672e9146	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:97:VUE-0996D4A86436539E37D3	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2451:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	0ecca3f6b8e7959eb8ebb2cc5be5e0947225c13d0b7289855be36be8ed47a82e	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:98:VUE-85A745A15630ED14D89C	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2463:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	2c4378ae594b881d28fe329d577d26ac3e0d7a9b36e866baae148186842f6ee8	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:99:VUE-35E810C5B2B5A70108BD	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2474:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	450d6be9896dd8f00bfe88b7ddd3da4021cb3b1d0638ff22b4a744e76302088b	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:100:VUE-E66DC910FC1D46686D4B	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2486:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	c0b50e009aed254c15d7f126de2359cb83dcfcb56b4a74aa163e046b3b00d7e0	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:101:VUE-3E456A10046BBB6F4CAB	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2494:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	ed707c99b8a6418c6e2c9c4a5ba8d62957ea84c1910377c49400f732edeb8db2	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:102:VUE-3D6C64AE27940F326F72	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2502:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	fc39587469036679ca37f7622fbc98982dd393812c8ff22957eaeb34bc949bfb	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:103:VUE-86B9446C9AFE045405EA	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2513:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	6fc8d33c82b238c24579df5e0d11618309735e2e88509b10268ee67161c3a324	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:104:VUE-5E616C94EA73C494B3AC	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2526:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	cf4d799cbb58b5987e831adca2828d101019f76c2f6b510bcd9991d8aa0b9191	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:105:VUE-FA96C443D7688C26487D	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:2535:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	088f7345378d462963926dd1668a2dc7f4ea40f8f66abe1088a2a26cf29c4da1	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:106:VUE-73A1281B39EE5CE2B279	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:279:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	4c42626a36ff384a9a39298e8d1ac3a5f26780053cea304fd13749e2b058b97c	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:107:VUE-87EFE9CC06644DF60612	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:306:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	a44ca9a3cc1616fc1e7ce40e311e0da1d4f325e10f448f9a407f8ddbcd5a8e57	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:108:VUE-7C1DC0A74E4717D5A7C8	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:336:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	e3b1a8bcd26f04bf1f03b5980e08c9dffc843b774047220ba10cf68a2d2eda84	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:109:VUE-5AF3E3C6BDE6F6E31401	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:3443:11	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	CallExpression	d8b4015253577447a7ada54a3853ff34a8599e3295eda3ac8cd4c0b75bec249e	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:110:VUE-4457EA1218D697DFDBF4	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:351:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	9a51ebacd28fcb80170226ca7dd77d5301bf09f703c496dca00741004ac49a9d	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:111:VUE-D08435901A9C625773D3	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:36:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	05c3e695c2d358aa01e5807817d64f941fd7e6903d2f2833a3b325a3b3ce0fcf	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:112:VUE-DFF5FCDEF7AB4D39B08E	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:366:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	462839243b8c33d522ed75897cb1fe04da97d41dff4046e29512c14064eab8b0	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:113:VUE-4B6D0491B49B79DCBEAA	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:391:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	a585235da28248d7da4800815af9fb1a732755fd0c75dc7c7daba0eee3a22f91	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:114:VUE-032E1E9A0B5CAA661FE2	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:400:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	e80f0c3d08a69c8a7685bfb2c0cb71cf9b997ec86241a7b8b451c2602cc8d99f	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-
docs/arch/refactor/rev11/evidence/framework-conformance/vue-official-cases.tsv:115:VUE-8170618A3393F37AE531	compiler-core	packages/compiler-core/__tests__/parse.spec.ts:441:5	f70c7aa5c74e9ec282cf460a14170d09b31bd0cc	single-declaration	StringLiteral	e6abf073f0a13fec0dd15c2242ea124e742caa5e5ccae29954db58d57eeab2de	blocked	BV1	BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.	-

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n \"\\bBV2\\b|post-BV1|bounded post|Vue correction block|BV1 \\+ BS1|BV1 \\+ BS1 ->\" . --glob '"'!target/**'"' --glob '"'!docs/arch/refactor/rev11/evidence/framework-conformance/*.tsv'"'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 811ms:[0m
./docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:1600:b64358705 chore(arch): record ratification and register the immediate Vue correction block
./docs/arch/refactor/rev11/evidence/BF3/architecture-mandate-review.md:616:- `BF3-R3-3` — P2 — The landing record ends with a current-looking proposed transition saying both maintainer rulings are still needed, contradicting the resolution added earlier in the same file; its verification section also describes the wrong delta boundary. Location: `landing-record.md:1046-1057,1077-1100`. Fix: mark the old proposal/verification section superseded and add a correctly bounded post-clarification transition after the independent review result exists.
./crates/verter_session/src/resolution_signature_growth_tests.rs:185:        "the owner witness must grow by the bounded post-DAG unit; observed {observed}"
./packages/dx-harness/src/corpus-gate/config.ts:20: *  - VERTER_CORPUS_GATE_SETTLE_CAP_MS        bounded post-ready settle (120000)

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "git log -8 --oneline --decorate && sed -n '1,240p' docs/arch/refactor/rev11/ORCHESTRATOR.md && nl -ba docs/arch/refactor/rev11/program-dag.toml | sed -n '120,170p' && nl -ba docs/arch/refactor/rev11/program.md | sed -n '145,205p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 249ms:[0m
c038105ca (HEAD -> program/architecture-lock, feat/affected-tests-selector) feat(ci): add affected-tests selector for fast Rust inner-loop test runs
6ab1000bd (origin/program/architecture-lock) fix(ci): gate the Svelte/Vue oracle-cache prerequisite and land bf2-authoritative in the canonical archive
f46de1b6a feat(ci): add stack-window validator and supersede the checkpoint fail-closed refusal
71fb82dec fix(ci): bind review verdicts to the exact candidate they were issued against
5b899200b feat(ci): gate block dispatch on enabling-amendment ratification
6c3939734 chore(arch): accept the Vue conformance train under a narrow authority ruling and open its Svelte twin
98184f153 docs(arch): record landing evidence for the Vue conformance train
71b7211b1 feat(core): close the Vue Vapor/VDOM compiler-output conformance backlog
# Verter Revision 11 — Orchestrator Entry Point

**Use this file as the first package instruction for the orchestrator.**  
**Canonical input:** the extracted Revision 11 split package plus an actual local checkout of `pikax/verter`.  
**Initial authorization:** execute `A0` only.

# 1. Your role

You are the implementation orchestrator, not the maintainer and not an architecture authority.

Your job is to:

- validate the release package and actual checkout;
- maintain exact program, branch, stack, candidate, evidence, and review state;
- execute only the next legal bounded block;
- delegate substantial independent work through immutable context packets;
- stop and report when source or evidence contradicts the plan;
- recommend acceptance only from reproducible evidence.

You must not silently change architecture, weaken a gate, widen a charter, self-approve, or treat your own summary as independent review.

# 2. Opus adapter identity

The supplied Opus bootstrap requests fixed model ID `claude-opus-5`. Record the actual model, provider, and any fallback/substitution in `program-state.toml` before repository mutation.

If the actual runtime differs from the requested runtime, you may complete non-mutating package/repository inspection, but return `A0 BLOCKED` before representing the result as an Opus-governed Foundational outcome unless the designated maintainer records an explicit exception. Model brand does not replace role independence or evidence.

# 3. Read before acting

Read, in this order:

1. `README.md`
2. `contracts/agent-orchestration.md`
3. `contracts/stacked-prs.md`
4. `contracts/package-publication.md`
5. `contracts/architecture-falsification.md`
6. `program-dag.toml`
7. `governance.md`
8. `charters/A0.md`
9. `contracts/baseline-lock.md`
10. `contracts/current-tree-reconciliation.md`
11. the A0-relevant sections of `verification.md`

Do not load the entire package into every worker context. Give each worker only its bounded context packet and directly relevant authority/evidence.

# 4. First commands

From the extracted package:

```bash
python3 tools/validate_package.py
python3 tools/selftest_orchestration.py
claude --version
python3 tools/validate_program_state.py \
  --dag program-dag.toml \
  --state templates/program-state.template.toml \
  --mode template
python3 tools/validate_stack_window.py \
  --dag program-dag.toml \
  --stack templates/stack-window.template.toml \
  --mode template
python3 tools/validate_landing_equivalence.py \
  --proof templates/landing-equivalence.template.toml \
  --mode template
```

From the actual Verter checkout, collect without mutation:

```bash
git rev-parse --show-toplevel
git remote -v
git branch --show-current
git rev-parse HEAD
git rev-parse HEAD^{tree}
git status --porcelain=v1 --untracked-files=all
git worktree list --porcelain
git submodule status --recursive || true
git log -1 --decorate=full --stat
```

Enumerate local/remote architecture-affecting branches and open/queued pull requests using the repository's available GitHub tooling. Record unavailable permissions or APIs rather than assuming a clean state.

Initialize a repository-local ignored or external evidence directory and copy `templates/program-state.template.toml` to `program-state.toml`. Set top-level `status = "ACTIVE"`, resolve every A0-required field, then run:

```bash
python3 <REV11_PACKAGE>/tools/validate_program_state.py \
  --dag <REV11_PACKAGE>/program-dag.toml \
  --state <REPO_EVIDENCE>/program-state.toml \
  --mode live
```

# 5. Execute only A0

A0 produces:

- exact entry checkout SHA/tree and dirty/untracked/submodule/worktree state;
- remote, branch, target-lineage, and toolchain identity;
- Revision 11 package, release, and DAG digests;
- open PR/branch/queued-change disposition, including changes touching compiler, TypeInfo, flow, source ownership, CSS, framework, LSP/provider, protocol, cache, persistence, or lifetime boundaries;
- available command, CI, branch protection, merge queue, stack tooling, signing, and permission facts;
- designated maintainer and orchestrator identities;
- requested/actual model, orchestrator runtime/version, provider, and fallback state;
- initial validated program state with only legal next work exposed;
- A0 context/evidence packet and exact review state.

Do not start `A1` or any production cutover until A0 has been accepted under `governance.md`.

# 6. Delegation

Use no subagent when A0 can be completed directly with a small number of repository/tool calls.

When delegation is genuinely useful:

- default to no more than three active worker contexts;
- assign one role and one immutable context packet per worker;
- use separate writable worktrees/branches;
- do not ask workers to “review the entire architecture”;
- do not use a subagent merely to echo or generically double-check your own conclusion;
- keep conformance, architecture, and adversarial mandates distinct where required.

# 7. Stacked PR policy

Stacked PRs are allowed only under `contracts/stacked-prs.md`. Every `ACTIVE` or later stack snapshot is validated both against its pre-stack program-state basis and the current program-state ledger.

Do not create a program-wide stack. During A0, record whether GitHub native stacks, ordinary dependent PRs, merge queues, and signed rebases are available. A6 selects the operational tooling and locks a bounded stack-window policy. The default maximum is four open review layers; the permitted A6 range is two through six.

Private atomic layers, especially D1, are never independently mergeable to trunk.

# 8. Stop instead of improvising

Stop and report `BLOCKED` or `RESCOPE_REQUIRED` when:

- release checksum or package validation fails;
- the checkout is dirty or contains unowned work;
- target trunk differs from the expected line and the difference is not dispositioned;
- an open PR/branch competes for an authority;
- required repository/GitHub/CI facts cannot be obtained;
- source disproves an architectural assumption;
- A0 would require production ownership changes;
- the maintainer identity or acceptance path is absent;
- actual model/runtime differs from the Opus adapter without an accepted exception.

Do not fix these by hiding changes, deleting someone else's work, weakening checks, or assuming intent.

# 9. Return format

Lead with exactly one:

```text
A0 ACCEPTANCE RECOMMENDED
A0 BLOCKED
A0 RESCOPE REQUIRED
```

Then return the bounded record required by `contracts/agent-orchestration.md`, including raw evidence paths/digests, requested/actual model, exact SHA/tree, unresolved decisions, and next legal blocks derived from validated state.

Do not claim the architecture is implemented or performance-proven. A0 establishes a trustworthy entry state only.
   120	class = "foundational"
   121	predecessors = ["BV0", "BF3", "BA0", "BS0", "BRT0"]
   122	
   123	[[block]]
   124	id = "B3"
   125	name = "Canonical typed compiler request and prerequisite planner"
   126	class = "foundational"
   127	predecessors = ["BV0", "BF3", "BA0", "BS0", "BRT0"]
   128	
   129	[[block]]
   130	id = "B4"
   131	name = "Logical source units mapping composition and atomic publication"
   132	class = "foundational-atomic"
   133	predecessors = ["B2", "B3"]
   134	
   135	[[block]]
   136	id = "BV1"
   137	name = "Vue compiler-output conformance train"
   138	class = "subsystem"
   139	predecessors = ["B4"]
   140	
   141	[[block]]
   142	id = "BS1"
   143	name = "Svelte compiler-output conformance train"
   144	class = "subsystem"
   145	predecessors = ["B4"]
   146	
   147	[[block]]
   148	id = "B5"
   149	name = "Accepted framework algorithms through sole direct compiler core"
   150	class = "foundational-atomic"
   151	predecessors = ["BV1", "BS1"]
   152	
   153	[[block]]
   154	id = "B6"
   155	name = "PreparedCarrier direct batch and direct-core closure"
   156	class = "foundational"
   157	predecessors = ["B5"]
   158	
   159	[[block]]
   160	id = "C1"
   161	name = "ModuleResolverCore and non-flow TypeInfoCore"
   162	class = "foundational"
   163	predecessors = ["A6", "B1", "B2"]
   164	
   165	[[block]]
   166	id = "C2"
   167	name = "Staged compile transaction and sealed facade"
   168	class = "foundational"
   169	predecessors = ["B3", "B5", "C1"]
   170	
   145	
   146	**Exit:** every artifact/query can state construction and compatibility identity without global revision, request identity, or ad hoc strings.
   147	
   148	## B2 — Shared syntax frontends and parse owner domains
   149	
   150	**Predecessors:** `B1`.
   151	
   152	Implement `ParseKey`, `ParseOwnerDomainId`, one live pair, owner-affine access, exact locators, pressure reparse, and direct/prepared/managed lifetime rules. Delete consumer-role parser duplication within each completed cutover.
   153	
   154	**Exit:** same syntax construction in one owner domain parses once; direct calls remain free of hidden managed/global cache state.
   155	
   156	## B3 — Compositional product planner
   157	
   158	**Predecessors:** `B1`.
   159	
   160	Implement canonical typed per-product requests, product-local output/terminal profiles, framework requests, minimal prerequisite closure, independently keyed reusable subplans, one-plan-per-request default, unsupported/duplicate-combination rejection, and zero-work counters.
   161	
   162	**Exit:** requested and forbidden incidental work is mechanically observable.
   163	
   164	## B4 — Compact source units, mapping taxonomy, and blanking deletion
   165	
   166	**Predecessors:** `B2`, `B3`.
   167	**Atomicity:** this block does not land while any production compiler route still requires full-carrier whitespace blanking for the migrated source-unit family.
   168	
   169	Introduce logical units, `PlacementMap`, required `SourceProjectionMap`, optional `RuntimeSourceMapData`, terminal `EncodedSourceMap`, and immutable generated-code-plus-required-map artifacts. Migrate every current compiler consumer in the bounded family and delete source-sized `eval_source`/blank-buffer ownership in the same accepted cutover.
   170	
   171	**Exit:** offset preservation uses exact units/maps rather than a source-sized duplicate, and required IDE maps cannot be skipped by a runtime-map flag.
   172	
   173	## B5 — Borrowed one-shot compiler atomic cutover
   174	
   175	**Predecessors:** `B4`.
   176	
   177	Introduce the borrowed direct boundary over the surviving shared frontend, planner, units, mappings, and framework algorithms. Keep arenas/local state owner-affine, construct no managed/session/provider/audit infrastructure, materialize only requested terminal artifacts, separate pure Rust and FFI paths, migrate every current direct/standalone caller, and delete the displaced public/direct route in the same accepted cutover.
   178	
   179	**Exit:** source-only one-shot compile is one genuine low-overhead library product with no shadow direct implementation.
   180	
   181	## B6 — `PreparedCarrier`, direct batch, and direct-core closure
   182	
   183	**Predecessors:** `B5`.
   184	
   185	Add explicit borrowed/owned preparation, prepare-once/emit-many, inspectable retained weight, safe drop, direct batch ownership, deterministic aggregation, and no implicit project/provider/global state. Prove all three direct regimes use the same accepted parser/planner/unit/mapping/codegen core and delete any transition-only adapters.
   186	
   187	**Exit:** reuse is explicit and lock-free/single-owner by default; the direct core is final enough for semantic projection and managed orchestration to depend on it.
   188	
   189	# 6. Track C — One non-flow semantic core and closed compile projections
   190	
   191	## C1 — Converge `ModuleResolverCore` and non-flow `TypeInfoCore`
   192	
   193	**Predecessors:** `A6`, `B1`, `B2`.
   194	
   195	Preserve correct current resolution/index/fact foundations, remove alternate resolver semantics, use immutable observation views, and return batched `NeedInputs`. Flow semantics are excluded.
   196	
   197	**Exit:** lifecycle differs; module/name/type/relation meaning does not.
   198	
   199	## C2 — Staged compile transaction and concrete sealed facade
   200	
   201	**Predecessors:** `B3`, `B5`, `C1`.
   202	
   203	Implement prepare/plan/project/emit, anti-replay plan/fact tokens, concrete `CompileTypeInfo`, first-party lifecycle modes, no public semantic trait object, no blanket `Send + Sync`, and bounded load/retry behavior.
   204	
   205	**Exit:** project-aware compilation cannot accept another semantic engine or stale/mismatched facts.

[35m[3mcodex[0m[0m
The confirmed trace supports an ownership collision, but I’m checking the program’s authoritative charters before assigning it. The key distinction is whether this is Vue emission semantics (a post-conformance correction) or a shared transform contract change (which would widen both ownership and regression obligations).
[35m[3mexec[0m[0m
[1m/bin/zsh -lc "wc -l docs/arch/refactor/rev11/charters/BV1.md docs/arch/refactor/rev11/charters/BS1.md docs/arch/refactor/rev11/charters/B5.md docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md docs/arch/refactor/rev11/governance.md && nl -ba docs/arch/refactor/rev11/charters/BV1.md | sed -n '1,260p' && nl -ba docs/arch/refactor/rev11/charters/BS1.md | sed -n '1,220p' && nl -ba docs/arch/refactor/rev11/charters/B5.md | sed -n '1,260p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
      60 docs/arch/refactor/rev11/charters/BV1.md
      39 docs/arch/refactor/rev11/charters/BS1.md
      12 docs/arch/refactor/rev11/charters/B5.md
     176 docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md
     378 docs/arch/refactor/rev11/governance.md
     665 total
     1	# BV1 — Vue compiler-output conformance train
     2	
     3	**Status:** PROPOSED / LOCKED. **Class:** Framework subsystem.
     4	**Predecessor:** B4.
     5	
     6	## Objective
     7	
     8	Deliver Vue compiler correctness on the final B2–B4 substrate for the exact
     9	`vuejs/core v3.6.0-rc.3` domain.
    10	
    11	## Owned scope
    12	
    13	- Vue-owned semantic model and requested product plans;
    14	- client VDOM and client Vapor generated JavaScript as separate families;
    15	- server/SSR JavaScript and official RC.3 topology;
    16	- script/template assembly; development/production; JavaScript/TypeScript;
    17	- helper/import/call topology, macros, components, directives, events, props,
    18	  attributes, class/style, slots, fragments, Teleport, Suspense, and async setup;
    19	- scoped/slotted metadata, CSS-variable code effects, custom elements, delimiters,
    20	  whitespace/comments, hoisting, handler caching, and binding metadata;
    21	- diagnostics, maps, PublicApi/TSC/declaration/tooling products;
    22	- source-local macro behavior and closed typed demands for imported project data; and
    23	- the complete accepted official-core Vue pack.
    24	
    25	BV1 does not implement Vue runtime code, invent `SSR x Vapor`, use an official
    26	compiler in production, or fulfil project-aware imported macro demands. C3 fulfils
    27	those demands without replacing codegen.
    28	
    29	`FC-TS-001-LOCAL` is BV1's independently closable partition of `FC-TS-001`. It
    30	proves the source-local PublicApi, TSC/TSX, and declaration cells and proves the
    31	BV1 side of the closed BV1→C3 protocol in `C3.md` with typed deterministic
    32	stubs: every demand kind is planned with the specified identity and order, every
    33	`Success` payload is consumed, and every omitted, `NotFound`, `Stale`, or `Error`
    34	result produces the specified typed non-success without partial publication. It
    35	requires no C3 implementation or live project resolver. `FC-TS-001-PROJECT` is
    36	C3's later end-to-end partition; it combines accepted BV1 codegen with the real
    37	project/type substrate and closes `FC-TS-001` for the jointly owned Vue cells.
    38	
    39	## BV0 preservation
    40	
    41	BV1's required exits additionally prove that every BV0 correction survives the final
    42	B2–B4 substrate and that the exact BV0 seed pack remains green. BV1 may replace a
    43	BV0 implementation only with an accepted equivalent correction; it may not
    44	reintroduce a corrected defect or convert one into a refusal or tracked divergence.
    45	
    46	## Required exits
    47	
    48	`FC-VUE-001`, `FC-HYDRATION-001`, `FC-TS-001-LOCAL`, `FC-ATOMIC-001`,
    49	`FC-ZERO-WORK-001`, and applicable `FC-PERF-001` cells pass. Every BV1-owned or
    50	source-local Vue cell has no blocked official case or semantic known-divergence;
    51	jointly owned project-aware cells remain projection-required until
    52	`FC-TS-001-PROJECT`. Output parses, links to the exact packages, matches protected
    53	structure/topology, executes deterministically, maps accurately, and removes every
    54	corresponding BF3 guard. Vue RC maturity is not Stable.
    55	
    56	## Abort/rescope
    57	
    58	Stop for a demanded project fact not expressible by the closed typed protocol, an
    59	unlocked compatibility change, an official topology the product model cannot express,
    60	or pressure to share a universal semantic/runtime IR with Svelte.
     1	# BS1 — Svelte compiler-output conformance train
     2	
     3	**Status:** PROPOSED / LOCKED. **Class:** Framework subsystem.
     4	**Predecessor:** B4.
     5	
     6	## Objective
     7	
     8	Deliver Svelte compiler correctness on the final B2–B4 substrate for exact
     9	`svelte@5.56.8`.
    10	
    11	## Owned scope
    12	
    13	- Svelte-owned semantic model and product plans;
    14	- client and server JavaScript; development and production;
    15	- runes and legacy behavior only where the capability lock claims them;
    16	- Svelte-native helpers, effects, blocks, events, bindings, actions, transitions,
    17	  animations, components, dynamic elements/components, DOM, and server topology;
    18	- stores, context, slots/children, snippets, boundaries, async behavior, namespaces,
    19	  hydration-compatible output, custom elements, whitespace/comments;
    20	- diagnostics, maps, PublicApi/TSC/declaration/tooling products; and
    21	- the complete accepted official-core Svelte pack.
    22	
    23	BS1 does not implement a Svelte runtime, use an official compiler in production, or
    24	automatically widen Verter's product API because the official compiler returns an
    25	extra artifact.
    26	
    27	## Required exits
    28	
    29	`FC-SVELTE-001`, `FC-HYDRATION-001`, `FC-TS-001`, `FC-ATOMIC-001`,
    30	`FC-ZERO-WORK-001`, and applicable `FC-PERF-001` cells pass. Every supported cell has
    31	no blocked official case or semantic known-divergence. Structural and dependency
    32	proofs demonstrate that no Vue semantic, IR, lowering, helper, hydration, event,
    33	component, or SSR assumption remains anywhere in Svelte implementation paths. Every
    34	corresponding BF3 guard is removed.
    35	
    36	## Abort/rescope
    37	
    38	Stop for an unlocked compatibility change, an unsupported official mode presented as
    39	success, an output requiring runtime patching, or any design that bases Svelte on Vue.
     1	# B5 — Accepted framework algorithms through the sole direct compiler core
     2	
     3	**Status:** PROPOSED amendment / LOCKED. **Predecessors:** BV1 and BS1.
     4	
     5	B5 exposes the already accepted BV1 and BS1 algorithms through one borrowed one-shot
     6	direct compiler core with B4 atomic publication. It performs no framework semantic
     7	repair, option reinterpretation, helper choice, or conformance exception.
     8	
     9	The direct core consumes B3's canonical request and framework-owned plans. It returns
    10	exactly one typed atomic result and performs zero unrequested product work. Exit
    11	proves Vue and Svelte accepted packs remain unchanged through the direct route and no
    12	legacy alternate core can publish the same products.

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md | sed -n '1,220p' && rg -n \"ADR|accepted ADR|decision record|DAG edge|program outcome|Outcome\" docs/arch/refactor/rev11 | head -n 300"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 463ms:[0m
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
docs/arch/refactor/rev11/README.md:29:- [`decisions/`](decisions/) — ADR-001 … ADR-020.
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:28:The manifest and execution documents treated ADRs as architecture authority while the ADRs themselves were marked `Proposed`. An implementor could not know whether a conflicting implementation detail required an ADR amendment.
docs/arch/refactor/rev11/implementation-readiness-review-v9.md:30:**Revision 11 correction:** every durable ADR is `Accepted`; the manifest defines exact precedence and amendment rules.
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:44:| `decisions/ADR-001-semantic-authority-and-derived-projections.md` | 28 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:45:| `decisions/ADR-002-compatibility-domains.md` | 28 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:46:| `decisions/ADR-003-sealed-compile-semantic-facade.md` | 23 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:47:| `decisions/ADR-004-typescript-semantic-profiles.md` | 42 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:48:| `decisions/ADR-005-operation-dtos-and-optional-graph-export.md` | 23 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:49:| `decisions/ADR-006-demand-selected-flow-domains.md` | 23 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:50:| `decisions/ADR-007-direct-core-before-managed-runtime.md` | 23 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:51:| `decisions/ADR-008-deterministic-artifacts-and-persistence.md` | 19 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:52:| `decisions/ADR-009-shared-frontends-and-parse-owner-domains.md` | 36 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:53:| `decisions/ADR-010-compositional-products-and-mapping-taxonomy.md` | 41 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:54:| `decisions/ADR-011-staged-compile-attempt-and-input-loading.md` | 37 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:55:| `decisions/ADR-012-stable-identifiers-and-canonical-ordering.md` | 29 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:56:| `decisions/ADR-013-result-contracts-and-flight-owned-production.md` | 38 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:57:| `decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md` | 30 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:58:| `decisions/ADR-015-binding-dependency-direction.md` | 45 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:59:| `decisions/ADR-016-implementation-lock-and-performance-gates.md` | 36 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:60:| `decisions/ADR-017-stack-aware-review-and-landing.md` | 33 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:61:| `decisions/ADR-018-opus-adapter-and-orchestrator-state.md` | 31 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:62:| `decisions/ADR-019-reproducible-authority-package.md` | 28 |
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md:63:| `decisions/ADR-020-constitutional-invariants-and-falsifiable-tactics.md` | 29 |
docs/arch/refactor/rev11/decisions/ADR-006-demand-selected-flow-domains.md:1:# ADR-006 — Flow Uses Demand-Selected Abstract Domains
docs/arch/refactor/rev11/decisions/ADR-009-shared-frontends-and-parse-owner-domains.md:1:# ADR-009 — IDE and Build Share One Error-Tolerant Frontend per Language Domain
docs/arch/refactor/rev11/decisions/ADR-009-shared-frontends-and-parse-owner-domains.md:5:**Reopen only if:** an accepted ADR proves two genuinely incompatible syntax products are both necessary and cannot share one frontend without material product loss.
docs/arch/refactor/rev11/decisions/ADR-009-shared-frontends-and-parse-owner-domains.md:23:Consumer role is not a key dimension. `ParseProductKind` is used only for a genuinely incompatible syntax product and requires a separate accepted ADR.
docs/arch/refactor/rev11/program.md:4:**Durable authority:** `architecture.md`, contracts, and accepted ADRs.  
docs/arch/refactor/rev11/program.md:437:Run every locked absolute SLO, self no-regression, work/copy/allocation, boundary, and competitor/Pareto cell. The primary direct suite must meet its locked best-in-class target. A miss is blocking; it cannot be waived by a post-result ADR. If the product/equivalent-work premise was materially wrong, amend the architecture and Implementation Lock Record under the blind recalibration rule, invalidate the affected candidate evidence, and restart the cell/block.
docs/arch/refactor/rev11/program.md:450:Make code, architecture, ADRs, capability truth, protocols, and tests agree; remove obsolete plans/charters/shims/guards/campaign comments; pass final exact-SHA conformance, architecture, adversarial performance/memory, and maintainer acceptance.
docs/arch/refactor/rev11/program.md:470:The remedy is an amended contract/ADR/charter and renewed review—not a hidden flag, shim, broad trait, duplicate cache, parallel implementation, or outcome-driven threshold.
docs/arch/refactor/rev11/verification.md:841:| Authority | Revision 11 digest, accepted ADRs, no contradiction or unresolved public/identity/lifetime gate |
docs/arch/refactor/rev11/templates/architecture-premise-ledger.template.md:25:- [ ] constitutional invariants cite architecture/ADR authority
docs/arch/refactor/rev11/decisions/ADR-010-compositional-products-and-mapping-taxonomy.md:1:# ADR-010 — Compiler Products Are Compositional and Mapping Kinds Are Distinct
docs/arch/refactor/rev11/templates/context-packet.md:46:- architecture/ADR/gate weakening:
docs/arch/refactor/rev11/decisions/ADR-003-sealed-compile-semantic-facade.md:1:# ADR-003 — Compile Semantics Use a Concrete Sealed Facade
docs/arch/refactor/rev11/decisions/ADR-012-stable-identifiers-and-canonical-ordering.md:1:# ADR-012 — Stable Entity IDs, Session Handles, and Canonical Ordering Are Distinct
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:150:| Durable decisions | `decisions/ADR-*.md` |
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:156:1. `architecture.md` is the consolidated current durable product architecture; accepted ADRs constrain scoped decisions.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:218:During convergence this split package is normative. The consolidated master is generated from `consolidation-order.txt` and is never edited independently. After `L4`, durable content is reduced to concise architecture documentation and retained ADRs; program files, baseline evidence, charters, agent adapters, and campaign-only material are archived or deleted after unique evidence is preserved.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:617:A6 accepts an architecture premise ledger. Falsified premises stop affected work, preserve independently valid work, and trigger refresh, rescope, or ADR amendment according to their class.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:976:- any future domain accepted by ADR.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1002:Consumer role (`IDE`, `build`, `TypeInfo`, `lint`) and framework name are not identity dimensions. A genuinely incompatible syntax construction uses a different typed frontend compatibility domain or normalized `SyntaxProfileId` and requires an accepted ADR proving it cannot be unified without material correctness or performance loss.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1097:        ProjectionOutcome::Complete(facts) => {
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1101:        ProjectionOutcome::NeedInputs(load_set) => {
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1146:Current Svelte runtime compilation requests no native compile projection unless a future capability ADR proves an official semantic need.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1338:Cross-snapshot semantic-flight joining is disabled by default. A later ADR may enable it for one query family only after proving it reduces total work and cannot cause stale use or retry pathologies. Cross-snapshot warm value reuse remains available through normal value-side validation.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1391:5. within-unit mutable/incremental parsing only if profiling proves full-unit parse remains a leading cost and a separate ADR accepts the complexity.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1449:Built-in static registrations are the default. Host-supplied in-process semantic registrations require a real consumer and an ADR proving lifetime, safety, compatibility, and API value. A stable dynamic plugin ABI is outside this architecture.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1518:Before implementation, a persistence ADR must name:
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1533:Without a ratified consumer ADR, the implementation remains request/prepared/managed-memory only.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1713:Production comments explain current invariants, safety/memory rationale, concurrency/ordering contracts, observable compatibility, or why a simpler-looking implementation is incorrect. Campaign history belongs in ADRs, changelogs, or git history.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1724:- public/precomputed semantic constructors without a current consumer and persistence ADR;
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1924:- accepted ADR or architecture amendment;
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:2009:A6 locks a value from two through six based on reviewer capacity, CI latency, restack frequency, and repository tooling. More than six requires an ADR amendment. A program-wide or fifty-block stack is prohibited.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:2147:Changing one requires an accepted ADR that explicitly supersedes the affected rule, updates `architecture.md`, identifies downstream blocks/evidence invalidated, and re-ratifies affected charters. A local benchmark win is insufficient.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:2198:Action when falsified: refresh | rescope | ADR amendment | abort
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:2222:4. refresh a repository fact, change an allowed tactic, or open an ADR amendment as required;
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:2597:**Binding ADRs:** ADR-002, ADR-004, ADR-012, ADR-016.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:2663:**Binding ADR:** ADR-010.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:2923:# 2. Outcome
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:2926:enum AttemptOutcome<T> {
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3064:A query family may enable broader in-flight compatibility only through an accepted ADR, a closed proof of compatibility before join, and adversarial retry/cancellation evidence.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3209:enum FlowSolveOutcome {
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3339:# Consolidated source: `decisions/ADR-001-semantic-authority-and-derived-projections.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3341:# ADR-001 — One Semantic Authority, Justified Derived Projections
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3373:# Consolidated source: `decisions/ADR-002-compatibility-domains.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3375:# ADR-002 — Compatibility Domains Are Explicit and Monotonic
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3407:# Consolidated source: `decisions/ADR-003-sealed-compile-semantic-facade.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3409:# ADR-003 — Compile Semantics Use a Concrete Sealed Facade
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3436:# Consolidated source: `decisions/ADR-004-typescript-semantic-profiles.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3438:# ADR-004 — TypeScript Compatibility Uses Semantic Profiles, Not Build Versions
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3484:# Consolidated source: `decisions/ADR-005-operation-dtos-and-optional-graph-export.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3486:# ADR-005 — Operation DTOs Are Primary; Semantic Graph Export Is Optional
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3513:# Consolidated source: `decisions/ADR-006-demand-selected-flow-domains.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3515:# ADR-006 — Flow Uses Demand-Selected Abstract Domains
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3542:# Consolidated source: `decisions/ADR-007-direct-core-before-managed-runtime.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3544:# ADR-007 — Prove the Direct Core Before Managed Runtime Convergence
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3562:CSS and bounded framework-contract work may proceed only through explicit DAG edges.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3571:# Consolidated source: `decisions/ADR-008-deterministic-artifacts-and-persistence.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3573:# ADR-008 — Deterministic Artifacts and Narrow Persistence Eligibility
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3596:# Consolidated source: `decisions/ADR-009-shared-frontends-and-parse-owner-domains.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3598:# ADR-009 — IDE and Build Share One Error-Tolerant Frontend per Language Domain
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3602:**Reopen only if:** an accepted ADR proves two genuinely incompatible syntax products are both necessary and cannot share one frontend without material product loss.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3620:Consumer role is not a key dimension. `ParseProductKind` is used only for a genuinely incompatible syntax product and requires a separate accepted ADR.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3638:# Consolidated source: `decisions/ADR-010-compositional-products-and-mapping-taxonomy.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3640:# ADR-010 — Compiler Products Are Compositional and Mapping Kinds Are Distinct
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3685:# Consolidated source: `decisions/ADR-011-staged-compile-attempt-and-input-loading.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3687:# ADR-011 — Project-Aware Compile Uses a Resumable I/O-Free Transaction
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3728:# Consolidated source: `decisions/ADR-012-stable-identifiers-and-canonical-ordering.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3730:# ADR-012 — Stable Entity IDs, Session Handles, and Canonical Ordering Are Distinct
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3763:# Consolidated source: `decisions/ADR-013-result-contracts-and-flight-owned-production.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3765:# ADR-013 — Result Contracts Are Separate from Execution Policy; Flights Own Producers
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3807:# Consolidated source: `decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3809:# ADR-014 — Flow Replaces the Legacy Evaluator in One Atomic Public Cutover
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3843:# Consolidated source: `decisions/ADR-015-binding-dependency-direction.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3845:# ADR-015 — Core Dependency Direction Is Inward and Cycle-Free
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3894:# Consolidated source: `decisions/ADR-016-implementation-lock-and-performance-gates.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3896:# ADR-016 — Foundational Work Requires a Pre-Candidate Implementation Lock
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3936:# Consolidated source: `decisions/ADR-017-stack-aware-review-and-landing.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3938:# ADR-017 — Stacked Review Must Preserve Block Atomicity and Exact-Candidate Proof
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3975:# Consolidated source: `decisions/ADR-018-opus-adapter-and-orchestrator-state.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3977:# ADR-018 — Agent Orchestration Uses One Model-Independent Program Ledger and a Pinned Opus Adapter
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4012:# Consolidated source: `decisions/ADR-019-reproducible-authority-package.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4014:# ADR-019 — Authority Publication Is Reproducible and Single-Source
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4046:# Consolidated source: `decisions/ADR-020-constitutional-invariants-and-falsifiable-tactics.md`
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4048:# ADR-020 — Constitutional Invariants Are Separated from Evidence-Selected Tactics
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4065:- require ADR/architecture amendment for constitutional changes;
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4086:**Durable authority:** `architecture.md`, contracts, and accepted ADRs.  
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4511:Run every locked absolute SLO, self no-regression, work/copy/allocation, boundary, and competitor/Pareto cell. The primary direct suite must meet its locked best-in-class target. A miss is blocking; it cannot be waived by a post-result ADR. If the product/equivalent-work premise was materially wrong, amend the architecture and Implementation Lock Record under the blind recalibration rule, invalidate the affected candidate evidence, and restart the cell/block.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4524:Make code, architecture, ADRs, capability truth, protocols, and tests agree; remove obsolete plans/charters/shims/guards/campaign comments; pass final exact-SHA conformance, architecture, adversarial performance/memory, and maintainer acceptance.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4544:The remedy is an amended contract/ADR/charter and renewed review—not a hidden flag, shim, broad trait, duplicate cache, parallel implementation, or outcome-driven threshold.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4884:- accept `A6`, architecture/ADR amendments, formal rescopes, and gate recalibrations;
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4896:- authority/contract/accepted-ADR consistency checks;
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:5017:The charter states what must become true and what may change. It does not dictate every private implementation detail. It may not remove a predecessor edge, loosen an A6 gate, create a second production authority, or reinterpret an accepted ADR.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:5166:The maintainer decides whether to amend the ADR/architecture/charter, split the block, or abort. A deviation cannot be hidden behind a flag, shim, parallel path, new cache, broader trait, or weaker test.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:5237:- architecture/ADR consistency;
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:6092:| Authority | Revision 11 digest, accepted ADRs, no contradiction or unresolved public/identity/lifetime gate |
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:7574:- [ ] constitutional invariants cite architecture/ADR authority
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:7807:- architecture/ADR/gate weakening:
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:8549:The manifest and execution documents treated ADRs as architecture authority while the ADRs themselves were marked `Proposed`. An implementor could not know whether a conflicting implementation detail required an ADR amendment.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:8551:**Revision 11 correction:** every durable ADR is `Accepted`; the manifest defines exact precedence and amendment rules.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:8891:- accepted authority referred to ADRs still marked Proposed;
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:8904:Revision 11 addresses these in accepted ADRs, the Revision 11 contracts, and an explicit predecessor DAG.
docs/arch/refactor/rev11/architecture.md:114:A6 accepts an architecture premise ledger. Falsified premises stop affected work, preserve independently valid work, and trigger refresh, rescope, or ADR amendment according to their class.
docs/arch/refactor/rev11/architecture.md:473:- any future domain accepted by ADR.
docs/arch/refactor/rev11/architecture.md:499:Consumer role (`IDE`, `build`, `TypeInfo`, `lint`) and framework name are not identity dimensions. A genuinely incompatible syntax construction uses a different typed frontend compatibility domain or normalized `SyntaxProfileId` and requires an accepted ADR proving it cannot be unified without material correctness or performance loss.
docs/arch/refactor/rev11/architecture.md:594:        ProjectionOutcome::Complete(facts) => {
docs/arch/refactor/rev11/architecture.md:598:        ProjectionOutcome::NeedInputs(load_set) => {
docs/arch/refactor/rev11/architecture.md:643:Current Svelte runtime compilation requests no native compile projection unless a future capability ADR proves an official semantic need.
docs/arch/refactor/rev11/architecture.md:835:Cross-snapshot semantic-flight joining is disabled by default. A later ADR may enable it for one query family only after proving it reduces total work and cannot cause stale use or retry pathologies. Cross-snapshot warm value reuse remains available through normal value-side validation.
docs/arch/refactor/rev11/architecture.md:888:5. within-unit mutable/incremental parsing only if profiling proves full-unit parse remains a leading cost and a separate ADR accepts the complexity.
docs/arch/refactor/rev11/architecture.md:946:Built-in static registrations are the default. Host-supplied in-process semantic registrations require a real consumer and an ADR proving lifetime, safety, compatibility, and API value. A stable dynamic plugin ABI is outside this architecture.
docs/arch/refactor/rev11/architecture.md:1015:Before implementation, a persistence ADR must name:
docs/arch/refactor/rev11/architecture.md:1030:Without a ratified consumer ADR, the implementation remains request/prepared/managed-memory only.
docs/arch/refactor/rev11/architecture.md:1210:Production comments explain current invariants, safety/memory rationale, concurrency/ordering contracts, observable compatibility, or why a simpler-looking implementation is incorrect. Campaign history belongs in ADRs, changelogs, or git history.
docs/arch/refactor/rev11/architecture.md:1221:- public/precomputed semantic constructors without a current consumer and persistence ADR;
docs/arch/refactor/rev11/package-README.md:137:| Durable decisions | `decisions/ADR-*.md` |
docs/arch/refactor/rev11/package-README.md:143:1. `architecture.md` is the consolidated current durable product architecture; accepted ADRs constrain scoped decisions.
docs/arch/refactor/rev11/package-README.md:205:During convergence this split package is normative. The consolidated master is generated from `consolidation-order.txt` and is never edited independently. After `L4`, durable content is reduced to concise architecture documentation and retained ADRs; program files, baseline evidence, charters, agent adapters, and campaign-only material are archived or deleted after unique evidence is preserved.
docs/arch/refactor/rev11/decisions/ADR-018-opus-adapter-and-orchestrator-state.md:1:# ADR-018 — Agent Orchestration Uses One Model-Independent Program Ledger and a Pinned Opus Adapter
docs/arch/refactor/rev11/decisions/ADR-015-binding-dependency-direction.md:1:# ADR-015 — Core Dependency Direction Is Inward and Cycle-Free
docs/arch/refactor/rev11/governance.md:17:- accept `A6`, architecture/ADR amendments, formal rescopes, and gate recalibrations;
docs/arch/refactor/rev11/governance.md:29:- authority/contract/accepted-ADR consistency checks;
docs/arch/refactor/rev11/governance.md:150:The charter states what must become true and what may change. It does not dictate every private implementation detail. It may not remove a predecessor edge, loosen an A6 gate, create a second production authority, or reinterpret an accepted ADR.
docs/arch/refactor/rev11/governance.md:299:The maintainer decides whether to amend the ADR/architecture/charter, split the block, or abort. A deviation cannot be hidden behind a flag, shim, parallel path, new cache, broader trait, or weaker test.
docs/arch/refactor/rev11/governance.md:370:- architecture/ADR consistency;
docs/arch/refactor/rev11/charters/C1.md:33:2. **The `NeedInputs`/`AttemptOutcome` cutover is full-coverage, not a first cut.** Every non-flow
docs/arch/refactor/rev11/charters/C1.md:61:`AttemptOutcome`. It owns convergence of context/lifecycle plumbing, dependency direction, physical crate
docs/arch/refactor/rev11/charters/C1.md:91:  returns `AttemptOutcome::NeedInputs(LoadSet)` (per `contracts/input-loading.md` §2, §4) instead of
docs/arch/refactor/rev11/charters/C1.md:150:| C1-AC-5 | `AttemptOutcome::{Complete, NeedInputs(LoadSet), Terminal}` covers **every** non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt — full coverage, not one load point | Discharged structurally, not by sampling. Per C1-AC-7 and the Authority/fallback order clause, the observation interface is the *only* surface an I/O-free (C2-shaped) caller can reach a non-flow `ModuleResolverCore`/`TypeInfoCore` operation through — `ResolverContext` itself can call `ensure_loaded`/`wait_or_drive`, so it is not usable I/O-free. "Every reachable operation" therefore reduces to "every method on this one finite, closed trait." The trait is defined so every method returns `AttemptOutcome<T>` at the type level (never a bare `T`, `Result<T, _>`, or a call that can block) — a non-conforming method is a compile error at authoring time, not a gap a test could miss. Gate: one exhaustive test double (`impl <ObservationInterface> for TestDouble`) that must implement every trait method to compile; a method added later that does not return `AttemptOutcome<T>` fails to compile at the double, not at a sampled runtime assertion. If a future operation cannot be expressed this way, that is itself a Fork-4-reopening discovery (see Abort/rescope), not a reason to fall back to sampling. |
docs/arch/refactor/rev11/charters/C1.md:154:| C1-AC-9 | `ModuleResolverCore` (the relocated `ProjectResolver`) has no direct scheduler/tsgo I/O left uncoverted | Any synchronous I/O call inside the relocated 2122-line resolver either does not exist in the moved code (pure computation) or is converted to the same `AttemptOutcome`/`LoadSet` pattern as `resolver_core`'s load points — audited as part of C1-AC-5's coverage sweep, not a separate carve-out |
docs/arch/refactor/rev11/charters/C1.md:173:`AttemptOutcome` threading (C1-AC-5) must not add allocation to the existing blocking lifecycles' hot
docs/arch/refactor/rev11/charters/C1.md:175:materialization when nothing is missing. C1 may add a dedicated micro-cell only via the ADR-016
docs/arch/refactor/rev11/charters/C1.md:188:| Blocking cross-file load-on-demand | `host_lifecycle.rs:953` (`ensure_loaded`), `:1012` (`wait_or_drive`), `resolver_context.rs:913-914`, `host_resolver_context.rs:281-288`, `bare_name_resolve.rs:188-190` | Stays in `verter_session` (it needs `VerterHost`/the scheduler); the kernel-side call sites it's invoked from gain the full-coverage `AttemptOutcome` alternative instead | `verter_session` (blocking path) + `verter_semantic` (typed outcome) |
docs/arch/refactor/rev11/charters/C1.md:210:returns `AttemptOutcome::{Complete(T), NeedInputs(LoadSet), Terminal(AttemptFailure)}` per
docs/arch/refactor/rev11/charters/C1.md:223:is unqualified across compiler/resolver/TypeInfo kernels, and ADR-011:19-28 requires each attempt to
docs/arch/refactor/rev11/charters/C1.md:235:load/commit/retry orchestration across *multiple* kernel attempts — is `C2` (`program.md:242-247`, ADR-011).
docs/arch/refactor/rev11/charters/C1.md:267:  deleted; any I/O the resolver performs directly is converted to the `AttemptOutcome`/`LoadSet` pattern
docs/arch/refactor/rev11/charters/C1.md:307:  `AttemptOutcome::NeedInputs` caller would otherwise still be forced through — audit
docs/arch/refactor/rev11/charters/C1.md:349:  introduces (the deleted bare-host rail, the collapsed duplication, the new `AttemptOutcome`, the new
docs/arch/refactor/rev11/charters/C1.md:362:authority (`program.md:253`). The full-coverage `AttemptOutcome` threading touches the *calling
docs/arch/refactor/rev11/charters/C1.md:367:`CompileTypeInfo`, and the outer load/commit/retry loop over C1's `NeedInputs` outcome (ADR-011,
docs/arch/refactor/rev11/charters/C1.md:381:algorithms, dependency-neutral value types, `AttemptOutcome`/`LoadSet`) lives in `verter_semantic`; the
docs/arch/refactor/rev11/charters/C1.md:386:deleted, not recorded as still-permitted; `AttemptOutcome::NeedInputs(LoadSet)` exists and covers every
docs/arch/refactor/rev11/charters/C1.md:400:| architecture | diff plus the crate-boundary outcome — specifically whether `verter_semantic`'s new contents satisfy ADR-015's inward dependency direction with zero exception, whether the observation interface genuinely cannot reach `VerterHost`/the scheduler (not just "does not currently"), and whether any duplication-collapse accidentally created a new second authority instead of removing one |
docs/arch/refactor/rev11/charters/C1.md:401:| adversarial performance/memory | diff plus `A6_META_COMPILE_40_COLD_RUST` raw output — specifically whether `session.semantic_dispatch.calls`/`session.semantic_cold_build.calls` regressed, whether the crate-seam move added call/serialization overhead, and whether the full-coverage `AttemptOutcome` path adds allocation on the existing blocking lifecycles' hot path |
docs/arch/refactor/rev11/charters/C1.md:407:scheduler-integrated file-loading responsibilities in a way full-coverage `AttemptOutcome` conversion
docs/arch/refactor/rev11/charters/C1.md:437:   and ADR-011 requires the maximal sound missing-observation set per attempt. Ruling:
docs/arch/refactor/rev11/charters/B1.md:119:ADR-002's forbidden "duplicate counter that must stay equal" — is **NOT PROVEN**. Owner `H2`. B1 must
docs/arch/refactor/rev11/charters/B1.md:170:review class (ADR-016). It may not replace, reweight, subset, or reinterpret the locked cell, and it
docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md:1:# ADR-016 — Foundational Work Requires a Pre-Candidate Implementation Lock
docs/arch/refactor/rev11/baseline/9af553dd.md:238:- accepted authority referred to ADRs still marked Proposed;
docs/arch/refactor/rev11/baseline/9af553dd.md:251:Revision 11 addresses these in accepted ADRs, the Revision 11 contracts, and an explicit predecessor DAG.
docs/arch/refactor/rev11/contracts/architecture-falsification.md:28:Changing one requires an accepted ADR that explicitly supersedes the affected rule, updates `architecture.md`, identifies downstream blocks/evidence invalidated, and re-ratifies affected charters. A local benchmark win is insufficient.
docs/arch/refactor/rev11/contracts/architecture-falsification.md:79:Action when falsified: refresh | rescope | ADR amendment | abort
docs/arch/refactor/rev11/contracts/architecture-falsification.md:103:4. refresh a repository fact, change an allowed tactic, or open an ADR amendment as required;
docs/arch/refactor/rev11/decisions/ADR-019-reproducible-authority-package.md:1:# ADR-019 — Authority Publication Is Reproducible and Single-Source
docs/arch/refactor/rev11/decisions/ADR-007-direct-core-before-managed-runtime.md:1:# ADR-007 — Prove the Direct Core Before Managed Runtime Convergence
docs/arch/refactor/rev11/decisions/ADR-007-direct-core-before-managed-runtime.md:19:CSS and bounded framework-contract work may proceed only through explicit DAG edges.
docs/arch/refactor/rev11/decisions/ADR-011-staged-compile-attempt-and-input-loading.md:1:# ADR-011 — Project-Aware Compile Uses a Resumable I/O-Free Transaction
docs/arch/refactor/rev11/decisions/ADR-004-typescript-semantic-profiles.md:1:# ADR-004 — TypeScript Compatibility Uses Semantic Profiles, Not Build Versions
docs/arch/refactor/rev11/decisions/ADR-005-operation-dtos-and-optional-graph-export.md:1:# ADR-005 — Operation DTOs Are Primary; Semantic Graph Export Is Optional
docs/arch/refactor/rev11/decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md:1:# ADR-014 — Flow Replaces the Legacy Evaluator in One Atomic Public Cutover
docs/arch/refactor/rev11/decisions/ADR-002-compatibility-domains.md:1:# ADR-002 — Compatibility Domains Are Explicit and Monotonic
docs/arch/refactor/rev11/decisions/ADR-020-constitutional-invariants-and-falsifiable-tactics.md:1:# ADR-020 — Constitutional Invariants Are Separated from Evidence-Selected Tactics
docs/arch/refactor/rev11/decisions/ADR-020-constitutional-invariants-and-falsifiable-tactics.md:18:- require ADR/architecture amendment for constitutional changes;
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:30:Full enumeration in §5. **It changes no DAG edge, adds no block, retires no block,
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:708:- change any DAG edge, add or retire any block, or edit `program-dag.toml`;
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:788:**This is execution serialization between siblings, NOT a new semantic DAG edge.**
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:977:> DAG edge, capability cell, option classification, conformance disposition value, or
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:981:> as a scheduling directive that creates no semantic DAG edge and leaves B4 gated on
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:1015:(§1.1 Maintainer): the maintainer "alone may … accept `A6`, architecture/ADR
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md:1019:whether to amend the ADR/architecture/charter, split the block, or abort." Both
docs/arch/refactor/rev11/decisions/ADR-008-deterministic-artifacts-and-persistence.md:1:# ADR-008 — Deterministic Artifacts and Narrow Persistence Eligibility
docs/arch/refactor/rev11/decisions/ADR-013-result-contracts-and-flight-owned-production.md:1:# ADR-013 — Result Contracts Are Separate from Execution Policy; Flights Own Producers
docs/arch/refactor/rev11/contracts/flow-completeness.md:59:enum FlowSolveOutcome {
docs/arch/refactor/rev11/decisions/ADR-017-stack-aware-review-and-landing.md:1:# ADR-017 — Stacked Review Must Preserve Block Atomicity and Exact-Candidate Proof
docs/arch/refactor/rev11/decisions/ADR-001-semantic-authority-and-derived-projections.md:1:# ADR-001 — One Semantic Authority, Justified Derived Projections
docs/arch/refactor/rev11/contracts/identity-encoding.md:4:**Binding ADRs:** ADR-002, ADR-004, ADR-012, ADR-016.
docs/arch/refactor/rev11/evidence/A5-summary.md:20:| [`A5/compatibility-domains.md`](A5/compatibility-domains.md) | every version-like value and the 5 cache-identity dimensions, against ADR-002 |
docs/arch/refactor/rev11/evidence/A5-summary.md:59:ADR-015's stated consequence — "semantic kernel remains reusable across lifecycles" — does not hold:
docs/arch/refactor/rev11/evidence/A5-summary.md:189:  ADR-002's forbidden "duplicate counter that must stay equal" — and why it is hand-maintained
docs/arch/refactor/rev11/contracts/input-loading.md:9:# 2. Outcome
docs/arch/refactor/rev11/contracts/input-loading.md:12:enum AttemptOutcome<T> {
docs/arch/refactor/rev11/evidence/BS0/context-packet.md:71:    return ResolvedOutcome::Missing;
docs/arch/refactor/rev11/evidence/BS0/context-packet.md:125:`matches!(&runes, ResolvedOutcome::Missing)` to decide "this is a LEGACY
docs/arch/refactor/rev11/evidence/BS0/context-packet.md:187:**C1.** The gate's `CharacterizedOutcome` enum has exactly two variants,
docs/arch/refactor/rev11/evidence/BS0/context-packet.md:406:**Q8 — The gate scaffold.** `CharacterizedOutcome` gained an `EmitsAndPasses`
docs/arch/refactor/rev11/contracts/result-contract-and-flight.md:52:A query family may enable broader in-flight compatibility only through an accepted ADR, a closed proof of compatibility before join, and adversarial retry/cancellation evidence.
docs/arch/refactor/rev11/contracts/mapping-products.md:4:**Binding ADR:** ADR-010.
docs/arch/refactor/rev11/amendments/AMD-009-bf3-audit-and-immediate-correction-blocks.md:98:11. **The DAG edges.** The former `{BV0, BF3} -> {B2, B3}` region is superseded by
docs/arch/refactor/rev11/contracts/stacked-prs.md:68:A6 locks a value from two through six based on reviewer capacity, CI latency, restack frequency, and repository tooling. More than six requires an ADR amendment. A program-wide or fifty-block stack is prohibited.
docs/arch/refactor/rev11/evidence/B2/context-packet.md:6:READY; B3 serialized behind it (not a DAG edge — both write
docs/arch/refactor/rev11/evidence/BA0/context-packet.md:117:pub enum CarrierCompileOutcome {
docs/arch/refactor/rev11/evidence/BA0/context-packet.md:144:- When runtime is requested and the compile fail-closes, return `CarrierCompileOutcome::RuntimeSurfaceRefused { .. }` **before** the `if opts.want_ide` block — the IDE projection is not constructed at all for that request.
docs/arch/refactor/rev11/evidence/BA0/context-packet.md:175:pub enum CompileBatchOutcome {
docs/arch/refactor/rev11/evidence/BA0/context-packet.md:198:1. The combined request's runtime node is the typed refusal: `read_node(host, canonical, VirtualNodeKind::Main, &combined)` is `NodeOutcome::Refused { diagnostic_code }`, and the code equals the compiler's own `UnsupportedSvelteRuntimeSurface::…diagnostic_code()` for that cell (take it from `refusal_cells()`-style construction, never a transcribed literal).
docs/arch/refactor/rev11/evidence/BA0/context-packet.md:199:2. **No product at all is published under that combined token**: every other `VirtualNodeKind` is `NodeOutcome::Missing`, AND `host.get_ide(canonical, &combined)` is `None`, AND `host.ensure_ide_compiled(canonical, &combined)` is `Err(HostError::RuntimeSurfaceRefused { .. })`.
docs/arch/refactor/rev11/evidence/BA0/context-packet.md:214:- **Add a new, NOT-ignored structural test** proving the sum cannot express a product beside an error: an exhaustive `match` over `CompileBatchOutcome` (no wildcard arm — a new variant must fail to COMPILE) asserting the produced arm exposes a product and no errors and the failed arm exposes non-empty errors and no product, plus the conversion direction (a successful `VirtualFileResponse` maps to `Produced`; a `RuntimeSurfaceRefused` maps to `Failed`). This test must be discriminating, not a tautology.
docs/arch/refactor/rev11/contracts/agent-orchestration.md:153:- accepted ADR or architecture amendment;
docs/arch/refactor/rev11/evidence/BF3/scope-consult-ruling.md:74:- the BF3 ledger note and any DAG edges needed for immediate correction blocks;
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/conformance-challenge.md:23:   still requires 56 blocks and the old DAG edges/digest, rejecting the 57-row DAG.
docs/arch/refactor/rev11/evidence/BF3/reviews/exhaustion-confirm-2.md:13:  exportOutcome=error
docs/arch/refactor/rev11/evidence/BF3/reviews/exhaustion-confirm-2.md:22:  exportOutcome=error
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md:67:Lock Record amendment under governance sections 1.1 and 10 and ADR-016. The
docs/arch/refactor/rev11/evidence/A4/context-packet.md:143:## Outcome
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md:4:[`ADR-002 — Compatibility Domains Are Explicit and Monotonic`](../../decisions/ADR-002-compatibility-domains.md)
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md:7:ADR-002's test for each value: *is there a real compatibility domain?* If yes it has exactly one
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md:25:The last row is the only value in the tree that follows ADR-002's *namespace* form (an
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md:27:consistent with the ADR; noted because a later block converging "all versions to integers" would
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md:32:ADR-002: "disposable private caches may be invalidated by a new namespace/build fingerprint".
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md:52:builds its key inline), so the values are correct and the **names** are the defect. ADR-002's
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md:61:| workspace `version = "0.0.1-beta.3"` (`Cargo.toml`), all 16 published npm package versions | ADR-002: "package semver … not compatibility epochs" |
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md:62:| `BUNDLED_TSGO_VERSION` (`TsgoVersion::new(7, 0, 2)`) | ADR-002: "external tool versions are not compatibility epochs" |
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md:65:## 4. The consumer-compatibility manifest, and a contradiction with ADR-002
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md:83:### Finding D-2 — `nonzero_version!` forbids epoch zero, contradicting ADR-002
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md:86:`nonzero_version!` macro whose `new` returns `None` for `0`. ADR-002 states the opposite as a
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md:90:So the current type system makes the ADR's prescribed clean-replacement move unrepresentable in
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md:133:   provider surface's own epoch, ADR-002's "duplicate counter that must stay equal" prohibition
docs/arch/refactor/rev11/evidence/maintainer-rulings.md:19:Revision 11 program: package adoption/supersession, A6 acceptance, ADR amendments,
docs/arch/refactor/rev11/evidence/BF3/scope-memo.md:99:and any DAG edge for an immediate Svelte correction owner — is maintainer-ratified
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md:300:   not silently dropped scope — the doc names the exact program-DAG edge
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:653:docs/arch/refactor/rev11/decisions/ADR-009-shared-frontends-and-parse-owner-domains.md:36:- **Process-global direct cache:** violates direct ownership and can add contention/retention.
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:662:docs/arch/refactor/rev11/decisions/ADR-012-stable-identifiers-and-canonical-ordering.md:13:- `StableEntityId` is deterministic from a documented canonical/content-relative basis and may be compared across declared portable regimes;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:671:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md:20:- concrete machine-readable `performance-gates.toml` with no placeholders;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:681:docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-reopen-scoping-ruling.md:2696:docs/arch/refactor/rev11/decisions/ADR-017-stack-aware-review-and-landing.md:19:- lower-layer changes trigger restack, cumulative-tree CI, candidate invalidation, and impact-bounded reattestation; reviewed candidate identity and accepted landing identity remain separate; SHA and full tree may differ after a base advance, but a validated proof must establish exact canonical candidate-delta and generated-output equivalence on recorded bases;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:713:docs/arch/refactor/rev11/decisions/ADR-017-stack-aware-review-and-landing.md:19:- lower-layer changes trigger restack, cumulative-tree CI, candidate invalidation, and impact-bounded reattestation; reviewed candidate identity and accepted landing identity remain separate; SHA and full tree may differ after a base advance, but a validated proof must establish exact canonical candidate-delta and generated-output equivalence on recorded bases;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:876:/bin/zsh -lc "rg -n -S \"Existing thresholds remain locked|genuinely cannot pass|Rerun affected BF2|rerun/re-attestation, not|post-result recalibration|STOP and report|do not loosen it\" . --glob '"'!target/**'"' --glob '"'!node_modules/**'"' && sed -n '450,540p' docs/arch/refactor/rev11/verification.md && sed -n '1,220p' docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md && sed -n '1,230p' docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md" in <repo-root>
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:969:# ADR-016 — Foundational Work Requires a Pre-Candidate Implementation Lock
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:1071:Lock Record amendment under governance sections 1.1 and 10 and ADR-016. The
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:1752:414-# charter's exit criterion; it is not itself a claim that the file's ADR-016
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2421:performance-gates.toml-9-# same independent review class (ADR-016). Weakening a gate after seeing candidate
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2766:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-13-A0 captures an exact entry checkout. A1–A5 form one ordered Gate 0 lineage: command/harness fixes and the fail-closed safety retraction precede measurement and final inventories. A6 then produces one immutable Implementation Lock Record bound to:
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2767:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-14-
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2768:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-15-- exact entry checkout, exact post-Gate-0 implementation baseline/tree, and repository state;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2769:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-16-- Revision 11 package manifest digest;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2770:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-17-- non-vacuous command/capability evidence;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2771:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-18-- identity/profile/compatibility/protocol/dependency decisions;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2772:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-19-- instrumentation/work baseline;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2773:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-20-- concrete machine-readable `performance-gates.toml` with no placeholders;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2774:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-21-- first unlocked foundational charters and review state.
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2775:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-22-
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2776:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-23-Before A6, only adoption, command/capability proof, harness strengthening, wrong-complete safety retraction, measurement-only attribution, inventory, and gate/capability record work is allowed. Any Gate 0 source change invalidates and refreshes affected downstream evidence before A6. Non-safety foundational cutovers remain locked.
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2777:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-24-
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2778:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md:25:Gate recalibration is allowed only before the affected candidate is measured, through an exact new record digest and the same independent review class. Weakening a gate after seeing candidate results is prohibited.
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2779:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-26-
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2780:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-27-## Consequences
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2781:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-28-
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2782:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-29-- implementation does not invent public contracts or success criteria;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2783:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-30-- performance decisions are reproducible and auditable;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2784:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-31-- baseline changes trigger explicit reconciliation rather than silent drift.
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2785:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-32-
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2786:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-33-## Rejected alternatives
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2787:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-34-
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2788:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-35-- **Fill gates during each implementation PR:** enables outcome-driven thresholds.
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2789:docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md-36-- **Use prose-only “fast enough” goals:** not executable or reproducible.
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2853:docs/arch/refactor/rev11/contracts/agent-orchestration.md-153-- accepted ADR or architecture amendment;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2996:docs/arch/refactor/rev11/governance.md:17:- accept `A6`, architecture/ADR amendments, formal rescopes, and gate recalibrations;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:3008:docs/arch/refactor/rev11/governance.md-29-- authority/contract/accepted-ADR consistency checks;
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:3047:docs/arch/refactor/rev11/evidence/maintainer-rulings.md-19-Revision 11 program: package adoption/supersession, A6 acceptance, ADR amendments,
docs/arch/refactor/rev11/evidence/A5/instrumentation-reconciliation.md:304:Its deliverable is this decision record.
docs/arch/refactor/rev11/evidence/A5/context-packet.md:33:ABORT/RESCOPE: if the exact checkout, command target, product capability, current owner, compatibility obligation, or proof boundary differs materially from charter assumptions — STOP, do not improvise a substitute design, and write status RESCOPE_REQUIRED with the exact contradiction to .agent-run/a5-report.yaml. Do not silently widen scope. In particular: if "reconcile ... surviving instrumentation owners" turns out to require nontrivial production code changes beyond a decision record (e.g. actually deleting/merging loop5_instrumentation.rs), STOP and report exactly what's required rather than executing a large unscoped migration.
docs/arch/refactor/rev11/evidence/A5/context-packet.md:37:- Every decision record must be evidence-backed (cite actual file/line/owner from the current tree — an inventory of "everything currently true", not aspiration).
docs/arch/refactor/rev11/evidence/A5/context-packet.md:81:const duplicate a compatibility domain owned elsewhere (per ADR-002), and why is it hand-maintained while
docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:8:Binding authority: [`ADR-015 — Core Dependency Direction Is Inward and Cycle-Free`](../../decisions/ADR-015-binding-dependency-direction.md).
docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:79:ADR-015's chain, resolved to this workspace's crates. **`B1` ratifies this; A5 proposes it and
docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:82:| layer | ADR-015 name | crates |
docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:90:| 7 | harnesses (outside ADR-015; no crate may depend on these) | `verter_bench`, `verter_dx_baseline`, `verter_vue_conformance`, `verter_svelte_conformance`, `verter_session_oracle_macro` |
docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:94:- **`verter_protocol` at layer 5, not layer 2.** ADR-015's layer 2 is *dependency-neutral* DTOs.
docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:140:external tsgo API on native targets.** ADR-015's stated consequence — "semantic kernel remains
docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:164:                    contradicting ADR-015's reusability consequence
docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:185:  by volume, and the module-level direction inside it is not testable this way. ADR-015 is
docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:191:  inverting the dependency. ADR-015 rejects "mutual compiler/semantic callbacks" for this reason;
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:1195:plugin-vue citation inline), and regenerated the corpus. Outcome — the ruling's requirement is met
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:3843:docs/arch/refactor/rev11/contracts/architecture-falsification.md:79:Action when falsified: refresh | rescope | ADR amendment | abort
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:3884:docs/arch/refactor/rev11/governance.md:17:- accept `A6`, architecture/ADR amendments, formal rescopes, and gate recalibrations;
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:3895:docs/arch/refactor/rev11/governance.md-29-- authority/contract/accepted-ADR consistency checks;
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:3967:docs/arch/refactor/rev11/governance.md:150:The charter states what must become true and what may change. It does not dictate every private implementation detail. It may not remove a predecessor edge, loosen an A6 gate, create a second production authority, or reinterpret an accepted ADR.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:4058:docs/arch/refactor/rev11/governance.md-299-The maintainer decides whether to amend the ADR/architecture/charter, split the block, or abort. A deviation cannot be hidden behind a flag, shim, parallel path, new cache, broader trait, or weaker test.

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n '"'^'"\\[\\[block\\]\\]|"'^id = "(BV1|BS1|B5|B6)"|''^status =|''^accepted_|''^outcome|''^predecessors'"' docs/arch/architecture-lock/ledger/program-state.toml | tail -n 120 && rg -n 'id = \"(BV1|BS1|B5|B6)\"' docs/arch/architecture-lock/ledger/program-state.toml"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 89ms:[0m
877:[[block]]
879:status = "LOCKED"
885:accepted_sha = ""
886:accepted_tree = ""
902:[[block]]
904:status = "LOCKED"
910:accepted_sha = ""
911:accepted_tree = ""
927:[[block]]
929:status = "LOCKED"
935:accepted_sha = ""
936:accepted_tree = ""
952:[[block]]
954:status = "LOCKED"
960:accepted_sha = ""
961:accepted_tree = ""
977:[[block]]
979:status = "LOCKED"
985:accepted_sha = ""
986:accepted_tree = ""
1002:[[block]]
1004:status = "LOCKED"
1010:accepted_sha = ""
1011:accepted_tree = ""
1027:[[block]]
1029:status = "LOCKED"
1035:accepted_sha = ""
1036:accepted_tree = ""
1052:[[block]]
1054:status = "LOCKED"
1060:accepted_sha = ""
1061:accepted_tree = ""
1077:[[block]]
1079:status = "LOCKED"
1085:accepted_sha = ""
1086:accepted_tree = ""
1102:[[block]]
1104:status = "LOCKED"
1110:accepted_sha = ""
1111:accepted_tree = ""
1127:[[block]]
1129:status = "LOCKED"
1135:accepted_sha = ""
1136:accepted_tree = ""
1152:[[block]]
1154:status = "LOCKED"
1160:accepted_sha = ""
1161:accepted_tree = ""
1177:[[block]]
1179:status = "LOCKED"
1185:accepted_sha = ""
1186:accepted_tree = ""
1202:[[block]]
1204:status = "LOCKED"
1210:accepted_sha = ""
1211:accepted_tree = ""
1227:[[block]]
1229:status = "LOCKED"
1235:accepted_sha = ""
1236:accepted_tree = ""
1252:[[block]]
1254:status = "LOCKED"
1260:accepted_sha = ""
1261:accepted_tree = ""
1277:[[block]]
1279:status = "LOCKED"
1285:accepted_sha = ""
1286:accepted_tree = ""
1302:[[block]]
1304:status = "LOCKED"
1310:accepted_sha = ""
1311:accepted_tree = ""
1327:[[block]]
1329:status = "LOCKED"
1335:accepted_sha = ""
1336:accepted_tree = ""
1352:[[block]]
1354:status = "LOCKED"
1360:accepted_sha = ""
1361:accepted_tree = ""
1377:[[block]]
1379:status = "LOCKED"
1385:accepted_sha = ""
1386:accepted_tree = ""
1402:[[block]]
1404:status = "LOCKED"
1410:accepted_sha = ""
1411:accepted_tree = ""
1427:[[block]]
1429:status = "LOCKED"
1435:accepted_sha = ""
1436:accepted_tree = ""
1452:[[block]]
1454:status = "LOCKED"
1460:accepted_sha = ""
1461:accepted_tree = ""
1477:[[block]]
1479:status = "LOCKED"
1485:accepted_sha = ""
1486:accepted_tree = ""
1502:[[block]]
1504:status = "LOCKED"
1510:accepted_sha = ""
1511:accepted_tree = ""
1527:[[block]]
1529:status = "LOCKED"
1535:accepted_sha = ""
1536:accepted_tree = ""
1552:[[block]]
1554:status = "LOCKED"
1560:accepted_sha = ""
1561:accepted_tree = ""
1577:[[block]]
1579:status = "LOCKED"
1585:accepted_sha = ""
1586:accepted_tree = ""
1602:[[block]]
1604:status = "LOCKED"
1610:accepted_sha = ""
1611:accepted_tree = ""
603:id = "BV1"
628:id = "BS1"
653:id = "B5"
678:id = "B6"

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba docs/arch/architecture-lock/ledger/program-state.toml | sed -n '560,715p' && nl -ba docs/arch/refactor/rev11/evidence/maintainer-rulings.md | sed -n '1,120p' && nl -ba docs/arch/refactor/rev11/governance.md | sed -n '135,180p;275,315p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 114ms:[0m
     1	# Maintainer Rulings — Verter Revision 11, A0
     2	
     3	This file records the maintainer decisions that shape the tree under
     4	`docs/arch/refactor/rev11/`. It is the authoritative explanation of why the tree looks
     5	the way it does. A future agent continuing the program reads this file together with
     6	`README.md`, `evidence/A0-preflight-blocked.md` (historical entry inspection), and
     7	`evidence/A0-summary.md` (the committed, identity-free description of the A0
     8	landing; live A0 state lives in the external ledger).
     9	
    10	**Maintainer:** Carlos Rodrigues (GitHub `pikax`). Rulings R-1 through R-8 were made
    11	and recorded on **2026-08-09**; R-9 was made and recorded on **2026-08-10**; R-10 and
    12	R-11 were made and recorded on **2026-08-11**. R-12 (rescoping AMD-001 §1's timing) was
    13	made and recorded on **2026-08-12**. R-13 was issued in `A6`'s dispatch packet and
    14	entered in this register on **2026-08-12** (see that section for its source).
    15	
    16	## R-1 — Maintainer designation
    17	
    18	Carlos Rodrigues (`pikax`) is the designated maintainer and repository authority for the
    19	Revision 11 program: package adoption/supersession, A6 acceptance, ADR amendments,
    20	formal rescopes, gate recalibrations, irreversible compatibility/protocol decisions,
    21	and accept/land/merge where repository policy requires maintainer authority
    22	(`governance.md` §1.1), plus final block acceptance (`governance.md` §9 "maintainer
    23	acceptance/land" and §15 "Final maintainer decision" — block acceptance is not part of
    24	the §1.1 grant itself). Resolves preflight blocker B2 ("no maintainer designated").
    25	
    26	## R-2 — Package-validation waiver; the tree stays as-is
    27	
    28	Package validation is **waived**. The release ZIP `verter-architecture-v11.zip` and its
    29	`.sha256` were never available on the machine, and the `tools/*.py` validators could not
    30	run. The 67-file reconstructed tree in this directory **stays as-is**; it is **not**
    31	ratified as a renamed derivative of the canonical 85-file package. `origin/main` is
    32	frozen until this landing completes. Resolves preflight blocker B1.
    33	
    34	## R-3 — The Revision 11 plan supersedes `CLAUDE.md` and existing repo rules
    35	
    36	Where the Revision 11 plan and existing repository rules conflict, the plan governs.
    37	Two concrete conflicts this ruling resolves:
    38	
    39	1. **Python validators vs the no-Python dependency policy.**
    40	   - The plan (`contracts/agent-orchestration.md` §4): "run
    41	     `python3 tools/validate_package.py` against the extracted package" — and the
    42	     mandated first commands further name `tools/selftest_orchestration.py`,
    43	     `tools/validate_program_state.py`, `tools/validate_stack_window.py`, and
    44	     `tools/validate_landing_equivalence.py`.
    45	   - `CLAUDE.md` (Dependencies Policy): "Repo-owned toolchain is Rust + JS/Node only —
    46	     no committed Python. Repo-owned gate, build, CI, test, code-generation, packaging,
    47	     and release tooling is implemented as Rust bins or JS/Node scripts; Python is not a
    48	     committed implementation language for those paths."
    49	   - Resolution: the plan supersedes. See also R-4 — the validators are to be
    50	     reimplemented in Node, so no Python is committed.
    51	
    52	2. **`architecture.md` §8.2 vs `CLAUDE.md`'s Typed-IR-Only rule.**
    53	   - The plan (`architecture.md` §8.2): "The final architecture contains no general
    54	     recursive owned `TypeExpr` or `PortableTypeExpr` as a generic semantic transit IR,
    55	     final cache value, compile projection contract, or public result."
    56	   - `CLAUDE.md` (Typed-IR-Only Resolver Rule (CRITICAL)): "The native component-meta /
    57	     typeinfo type resolver — analyzer → projector → registry → policy → materialiser —
    58	     drives semantic decisions exclusively from the typed IR
    59	     (`verter_semantic::analysis::type_expr::TypeExpr` on Rust, `TypeDescriptor` from
    60	     `@verter/type-ir` on TS)."
    61	   - Resolution: the plan supersedes; §8.2's ordered cutover away from a general owned
    62	     `TypeExpr` governs the end state.
    63	
    64	## R-4 — Python-to-JS validator reimplementation
    65	
    66	The six validators named in `_EXTRACTION_INDEX.md` (`validate_package.py`,
    67	`selftest_orchestration.py`, `validate_program_state.py`,
    68	`validate_performance_gates.py`, `validate_stack_window.py`,
    69	`validate_landing_equivalence.py`) are to be **reimplemented in Node, not Python**.
    70	Status: `validate_program_state.py` is reimplemented as
    71	`scripts/validate-program-state.mjs` (with its `node --test` suite) and lands with
    72	this commit as a new ratified implementation — the Python original was never
    73	available, so no behavior was ported; every check is derived from the Revision 11
    74	tree's own text. The other five remain future work.
    75	
    76	## R-5 — PR #98 disposition: abandon
    77	
    78	PR #98 (`main <- agent/rsvelte-runtime-engine`, DRAFT, "feat(svelte): delegate runtime
    79	compilation to rsvelte") is dispositioned as **abandon** — the nearest value in
    80	`contracts/baseline-lock.md` §3's closed set (include before freeze / exclude and
    81	rebase-reconcile later / abandon / coordinate as predecessor-dependent block). This
    82	records the **program's relationship** to the PR only. **No GitHub action was taken or
    83	is to be taken**: the PR is an external contributor's draft and is left untouched on
    84	GitHub.
    85	
    86	## R-6 — The program ledger stays external
    87	
    88	The live program ledger (`program-state.toml`) **stays external** to the repository, in
    89	an operator-local evidence directory (referred to in the evidence records as
    90	`<EXTERNAL_EVIDENCE_ROOT>`); it is not committed. Consequence: this directory alone is
    91	not sufficient to resume the live program — the external ledger is also required (see
    92	`README.md`).
    93	
    94	## R-7 — `update-docs` workflow left as-is; one narrow CI-wiring edit authorized
    95	
    96	The repository's existing `update-docs` GitHub workflow is left alone. **Amendment
    97	(A0 fix round):** the maintainer-directed review mandates required the program-state
    98	validator test suite to be wired to a real gate, and editing `.github/workflows/` was
    99	explicitly authorized **for that one purpose only**. The sole `.github/` edit of this
   100	landing is therefore adding `scripts/validate-program-state.mjs` and
   101	`scripts/validate-program-state.test.mjs` to the `js` change-detection path filter in
   102	`.github/workflows/ci.yml` (the convention already used there for
   103	`scripts/sccache-env.test.mjs` and its siblings), so a validator-only change triggers
   104	the `js-build-test` job, whose `pnpm run test:scripts` step is the authoritative
   105	runner of the suite (`test:scripts` in `package.json` runs
   106	`node --test scripts/validate-program-state.test.mjs`). No other workflow content is
   107	touched.
   108	
   109	## R-8 — Nothing is ever pushed to `origin`
   110	
   111	All Revision 11 program work stays local: work happens on a local worktree branch,
   112	and landing is a local fast-forward of `main` — nothing is pushed to `origin`, and
   113	`origin/main` is frozen until this work lands. No GitHub action of any kind is taken
   114	as part of this program work.
   115	
   116	## R-9 — Ratify A2C completion-model predecessor for A3
   117	
   118	The maintainer ratifies **AMD-002 — A2C completion-model predecessor for A3**. `A2C`
   119	is inserted directly after `A2` and directly before `A3`; `A3` now depends on `A2C`,
   120	and no other predecessor list changes. `A2C` owns only content-free,
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
   275	-> Candidate B
   276	-> impact-bounded recheck by all three mandates
   277	-> all three PASS on Candidate B
   278	-> maintainer acceptance/land
   279	```
   280	
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

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_compiler/src/template/code_gen/types.rs | sed -n '1,310p;640,730p' && nl -ba crates/verter_compiler/src/code_transform/segmented.rs | sed -n '1,150p' && nl -ba crates/verter_compiler/src/template/code_gen/vdom/mod.rs | sed -n '75,145p;620,730p;810,880p;1030,1095p;1710,1780p' && nl -ba crates/verter_compiler/src/template/code_gen/vdom/comment.rs | sed -n '1,55p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
     1	//! Code generation output accumulator and internal types.
     2	//!
     3	//! All codegen operations are deferred into [`CodeGenOutput`] vecs.
     4	//! Nothing is applied to the source until [`CodeGenOutput::apply_to()`] is called.
     5	
     6	use oxc_allocator::Allocator;
     7	use smallvec::SmallVec;
     8	
     9	use crate::code_transform::{CodeTransform, GeneratedContentMarker, SegmentAnchor};
    10	
    11	use super::vapor::PendingNavQueue;
    12	
    13	use super::shared::helpers::{
    14	    BuiltinComponentFlags, SsrHelper, SsrHelperFlags, VaporHelper, VaporHelperFlags, VdomHelper,
    15	    VdomHelperFlags,
    16	};
    17	
    18	/// Capability token for [`CodeTransform::try_overwrite_segmented`].
    19	/// Constructor is `pub(in crate::template::code_gen)` — only Vue
    20	/// VDOM/Vapor/SSR emitters can mint one. `ide`/`svelte` get `E0603`.
    21	/// `apply_to` mints its own only to drain already-authorized entries.
    22	///
    23	/// External-crate unreachability:
    24	/// `tests/cases/segmented_overwrite_compile_fail.rs`. Intra-crate
    25	/// exclusion of `ide`/`svelte` is the privacy checker, not a scanner.
    26	#[derive(Clone, Copy)]
    27	pub struct SegmentedOverwriteAuthority(());
    28	
    29	impl SegmentedOverwriteAuthority {
    30	    pub(in crate::template::code_gen) fn new() -> Self {
    31	        Self(())
    32	    }
    33	
    34	    /// Test-only hatch for `code_transform/segmented_tests.rs` (outside
    35	    /// `template::code_gen`). Present under `cfg(test)` only; production
    36	    /// builds keep the call-site restriction.
    37	    #[cfg(test)]
    38	    pub(crate) fn new_for_test() -> Self {
    39	        Self(())
    40	    }
    41	}
    42	
    43	// ======================== CodeGenOutput ========================
    44	
    45	/// Accumulated code generation operations.
    46	///
    47	/// All operations are deferred — nothing is applied until [`apply_to()`](Self::apply_to).
    48	/// This avoids passing `CodeTransform` through every trait method and enables
    49	/// a single O(n+m) batch application at the end.
    50	#[derive(Clone)]
    51	pub struct CodeGenOutput<'alloc> {
    52	    /// Replace source ranges: (start, end, replacement).
    53	    /// Applied via `ct.batch_overwrite()` in sorted order.
    54	    pub overwrites: Vec<(u32, u32, &'alloc str)>,
    55	
    56	    /// Replace source ranges with wholly synthetic content that has no
    57	    /// character-level correspondence to the replaced span: (start, end,
    58	    /// replacement). Applied via `ct.overwrite_unmapped()` per entry — see
    59	    /// [`overwrite_or_root_prefix`](Self::overwrite_or_root_prefix) for why
    60	    /// this is a separate channel from `overwrites` rather than a flag on
    61	    /// the same one.
    62	    unmapped_overwrites: Vec<(u32, u32, &'alloc str)>,
    63	
    64	    /// Segmented overwrite: (start, end, replacement, anchors). Applied via
    65	    /// `ct.try_overwrite_segmented()`. Vue VDOM/Vapor/SSR only — interpolation
    66	    /// identifiers and static attribute names inside otherwise synthetic code.
    67	    segmented_overwrites: Vec<(u32, u32, &'alloc str, &'alloc [SegmentAnchor])>,
    68	
    69	    /// Insert content before a position: (position, content).
    70	    /// Applied via `ct.batch_prepend_left_static()` in sorted order.
    71	    /// Used for binding prefixes (`_ctx.`, `$setup.`), suffixes (`.value`), separators.
    72	    pub prepends: Vec<(u32, &'alloc str)>,
    73	
    74	    /// Insert content before a position with source map mapping:
    75	    /// (insertion_pos, source_pos, content_offset, content).
    76	    /// Creates `InsertedMapped` chunks that emit source map tokens at `source_pos`.
    77	    /// `content_offset` shifts the token within the content (characters before it
    78	    /// are unmapped). Used for relocated directive expressions (v-if conditions,
    79	    /// v-for iterables) where binding prefixes precede the original identifier.
    80	    pub mapped_prepends: Vec<(u32, u32, u32, &'alloc str)>,
    81	
    82	    /// VDOM runtime helper imports (bitflags, O(1) dedup).
    83	    vdom_imports: VdomHelperFlags,
    84	
    85	    /// Vapor runtime helper imports (bitflags, O(1) dedup).
    86	    vapor_imports: VaporHelperFlags,
    87	
    88	    /// SSR runtime helper imports from `vue/server-renderer` (bitflags, O(1) dedup).
    89	    ssr_imports: SsrHelperFlags,
    90	
    91	    /// Vue built-in component imports (Suspense, Teleport, etc.).
    92	    builtin_imports: BuiltinComponentFlags,
    93	
    94	    /// Deferred move operations: (start, end, target).
    95	    /// Applied via `ct.move_slice()` after overwrites and prepends.
    96	    moves: Vec<(u32, u32, u32)>,
    97	
    98	    /// Deferred wrapped move operations: (start, end, target, prefix, suffix).
    99	    /// Applied via `ct.move_wrapped()` after overwrites and prepends.
   100	    /// Preserves sourcemap for the moved content while wrapping it.
   101	    wrapped_moves: Vec<(u32, u32, u32, &'alloc str, &'alloc str)>,
   102	
   103	    /// The bump-allocated content of the leading helper-import preamble insertion, when codegen
   104	    /// emitted one via [`prepend_helper_preamble`](Self::prepend_helper_preamble). Transferred to
   105	    /// the [`CodeTransform`] in [`apply_to`](Self::apply_to) so source-map generation can report
   106	    /// the generated-TSX position immediately after it (the typed preamble-end boundary). `None`
   107	    /// when no helper-import preamble was emitted.
   108	    helper_preamble: Option<&'alloc str>,
   109	
   110	    /// Inline-mode hoisted-constant module preamble, recorded via
   111	    /// [`set_module_preamble`](Self::set_module_preamble). Transferred to
   112	    /// [`TemplateImports::module_preamble`] in [`apply_to`](Self::apply_to)
   113	    /// for the orchestrator to apply with `ct.prepend(...)`.
   114	    module_preamble: Option<&'alloc str>,
   115	
   116	    /// Explicit source-map boundary offsets, transferred in
   117	    /// [`apply_to`](Self::apply_to) via `try_add_sourcemap_location`.
   118	    /// `Original`/`Moved` chunks otherwise token only start and newlines;
   119	    /// a registered offset gets a mid-line token (e.g. a top-level
   120	    /// identifier the `verbatim-carry` oracle must land on).
   121	    sourcemap_locations: Vec<u32>,
   122	
   123	    /// Declared script-export-placement facts (see
   124	    /// [`record_sfc_export_fact`](Self::record_sfc_export_fact)), resolved
   125	    /// into markers in [`apply_to`](Self::apply_to).
   126	    sfc_export_facts: Vec<SfcExportFact<'alloc>>,
   127	
   128	    /// Allocator reference for bump-allocating generated strings.
   129	    alloc: &'alloc Allocator,
   130	
   131	    /// One reusable scratch buffer for the `write!`-style format sinks
   132	    /// (`overwrite_fmt`, `prepend_fmt`, and the mapped variants). Each sink
   133	    /// clears and reuses it, so a formatted emission costs the retained heap
   134	    /// capacity plus one bump copy — never a fresh `String` per call. It is
   135	    /// an operation-construction helper only; it never holds built output.
   136	    scratch: String,
   137	}
   138	
   139	impl<'alloc> CodeGenOutput<'alloc> {
   140	    /// Create a new empty output accumulator.
   141	    pub fn new(alloc: &'alloc Allocator) -> Self {
   142	        Self {
   143	            overwrites: Vec::with_capacity(16),
   144	            unmapped_overwrites: Vec::new(),
   145	            segmented_overwrites: Vec::new(),
   146	            prepends: Vec::with_capacity(16),
   147	            mapped_prepends: Vec::new(),
   148	            vdom_imports: VdomHelperFlags::empty(),
   149	            vapor_imports: VaporHelperFlags::empty(),
   150	            ssr_imports: SsrHelperFlags::empty(),
   151	            builtin_imports: BuiltinComponentFlags::empty(),
   152	            moves: Vec::new(),
   153	            wrapped_moves: Vec::new(),
   154	            helper_preamble: None,
   155	            module_preamble: None,
   156	            sourcemap_locations: Vec::new(),
   157	            sfc_export_facts: Vec::new(),
   158	            alloc,
   159	            scratch: String::new(),
   160	        }
   161	    }
   162	
   163	    /// Push an overwrite operation. The content is bump-allocated.
   164	    #[inline]
   165	    pub fn overwrite(&mut self, start: u32, end: u32, content: &str) {
   166	        let allocated = self.alloc.alloc_str(content);
   167	        self.overwrites.push((start, end, allocated));
   168	    }
   169	
   170	    /// Push an overwrite with pre-allocated content (avoids double allocation).
   171	    #[inline]
   172	    pub fn overwrite_alloc(&mut self, start: u32, end: u32, content: &'alloc str) {
   173	        self.overwrites.push((start, end, content));
   174	    }
   175	
   176	    /// Push an unmapped overwrite operation (see `unmapped_overwrites` field
   177	    /// doc). The content is bump-allocated.
   178	    #[inline]
   179	    pub fn overwrite_unmapped(&mut self, start: u32, end: u32, content: &str) {
   180	        let allocated = self.alloc.alloc_str(content);
   181	        self.unmapped_overwrites.push((start, end, allocated));
   182	    }
   183	
   184	    /// Same as [`overwrite_unmapped`](Self::overwrite_unmapped), but
   185	    /// `content` is already bump-allocated — avoids a second allocation,
   186	    /// and lets the caller later mint a
   187	    /// [`CodeTransform::mark_generated_sub_range`] fact against the EXACT
   188	    /// pointer this call inserts (see [`record_sfc_export_fact`]).
   189	    #[inline]
   190	    pub fn overwrite_unmapped_alloc(&mut self, start: u32, end: u32, content: &'alloc str) {
   191	        self.unmapped_overwrites.push((start, end, content));
   192	    }
   193	
   194	    /// Segmented overwrite: `content` replaces `[start, end)`; each `anchors`
   195	    /// entry is an authored lexeme inside `content`. Token is required here
   196	    /// (the emitter entry), not only on the primitive `apply_to` drains into —
   197	    /// otherwise any `CodeGenOutput` caller could reach this with no token.
   198	    /// See [`SegmentedOverwriteAuthority`].
   199	    #[inline]
   200	    pub fn overwrite_segmented(
   201	        &mut self,
   202	        start: u32,
   203	        end: u32,
   204	        content: &str,
   205	        anchors: &[SegmentAnchor],
   206	        _authority: SegmentedOverwriteAuthority,
   207	    ) {
   208	        let content_ref = self.alloc.alloc_str(content);
   209	        let anchors_ref: &'alloc [SegmentAnchor] = if anchors.is_empty() {
   210	            &[]
   211	        } else {
   212	            self.alloc.alloc_slice_copy(anchors)
   213	        };
   214	        self.segmented_overwrites
   215	            .push((start, end, content_ref, anchors_ref));
   216	    }
   217	
   218	    /// Push a prepend-left (insert before position) with `&'static str` content.
   219	    /// Zero allocation — the string lives in the binary.
   220	    #[inline]
   221	    pub fn prepend_static(&mut self, pos: u32, content: &'static str) {
   222	        self.prepends.push((pos, content));
   223	    }
   224	
   225	    /// Push a prepend-left with bump-allocated content.
   226	    #[inline]
   227	    pub fn prepend_alloc(&mut self, pos: u32, content: &str) {
   228	        let allocated = self.alloc.alloc_str(content);
   229	        self.prepends.push((pos, allocated));
   230	    }
   231	
   232	    /// Root wrapper insertion for a raw template unit whose synthetic root has
   233	    /// a zero-width opening tag. It must precede child operations at the same
   234	    /// byte boundary.
   235	    ///
   236	    /// Nonzero-width (a real tag, e.g. `<script setup>`, replaced by synthetic
   237	    /// wrapper) uses an unmapped overwrite: still `Overwritten` (ordering vs
   238	    /// later `move_slice` stays intact) but no source-map token. A mapped
   239	    /// `Overwritten` would claim the tag's start for the entire replacement —
   240	    /// truthful only for 1:1 substitutions. Official rc.3 `prependLeft` wrapper
   241	    /// insertion never touches the tag and carries no mapping either.
   242	    pub fn overwrite_or_root_prefix(&mut self, start: u32, end: u32, content: &str) {
   243	        if start == end {
   244	            let allocated = self.alloc.alloc_str(content);
   245	            self.prepends.insert(0, (start, allocated));
   246	        } else {
   247	            self.overwrite_unmapped(start, end, content);
   248	        }
   249	    }
   250	
   251	    /// Same dispatch as
   252	    /// [`overwrite_or_root_prefix`](Self::overwrite_or_root_prefix), but
   253	    /// `content` is already bump-allocated.
   254	    pub fn overwrite_or_root_prefix_alloc(&mut self, start: u32, end: u32, content: &'alloc str) {
   255	        if start == end {
   256	            self.prepends.insert(0, (start, content));
   257	        } else {
   258	            self.overwrite_unmapped_alloc(start, end, content);
   259	        }
   260	    }
   261	
   262	    /// Segmented [`overwrite_or_root_prefix`](Self::overwrite_or_root_prefix):
   263	    /// authored anchors (e.g. a hoisted static-props `class` key) survive.
   264	    /// Zero-width stays an unmapped prepend; nonzero-width goes through
   265	    /// [`overwrite_segmented`](Self::overwrite_segmented).
   266	    pub fn overwrite_or_root_prefix_segmented(
   267	        &mut self,
   268	        start: u32,
   269	        end: u32,
   270	        content: &str,
   271	        anchors: &[SegmentAnchor],
   272	        authority: SegmentedOverwriteAuthority,
   273	    ) {
   274	        if start == end {
   275	            let allocated = self.alloc.alloc_str(content);
   276	            self.prepends.insert(0, (start, allocated));
   277	        } else {
   278	            self.overwrite_segmented(start, end, content, anchors, authority);
   279	        }
   280	    }
   281	
   282	    /// Root wrapper close for a raw template unit whose synthetic closing tag
   283	    /// is a zero-width boundary after all authored bytes.
   284	    ///
   285	    /// See [`overwrite_or_root_prefix`](Self::overwrite_or_root_prefix) for
   286	    /// why the nonzero-width case uses an unmapped overwrite rather than a
   287	    /// regular (falsely position-claiming) one.
   288	    pub fn overwrite_or_root_suffix(&mut self, start: u32, end: u32, content: &str) {
   289	        if start == end {
   290	            self.prepend_alloc(start, content);
   291	        } else {
   292	            self.overwrite_unmapped(start, end, content);
   293	        }
   294	    }
   295	
   296	    /// Same dispatch as
   297	    /// [`overwrite_or_root_suffix`](Self::overwrite_or_root_suffix), but
   298	    /// `content` is already bump-allocated.
   299	    pub fn overwrite_or_root_suffix_alloc(&mut self, start: u32, end: u32, content: &'alloc str) {
   300	        if start == end {
   301	            self.prepends.push((start, content));
   302	        } else {
   303	            self.overwrite_unmapped_alloc(start, end, content);
   304	        }
   305	    }
   306	
   307	    /// Record a declared script-export-placement fact for `content` — a
   308	    /// chunk ALREADY inserted via one of this call's siblings above (e.g.
   309	    /// [`overwrite_or_root_suffix_alloc`](Self::overwrite_or_root_suffix_alloc)).
   310	    /// `binding_local_ranges` are every `__sfc__`→`_sfc_main` rename target
   640	        // Carry the recorded helper-import preamble identity into the transform. The same `&'alloc
   641	        // str` becomes an `Inserted` chunk below, so source-map generation can locate it by pointer
   642	        // and report the typed preamble-end boundary. No-op when no preamble was emitted.
   643	        if let Some(preamble) = self.helper_preamble {
   644	            ct.set_helper_preamble_content(preamble);
   645	        }
   646	
   647	        // Apply wrapped moves FIRST — they operate on Original chunks and must
   648	        // run before overwrites replace those chunks. This preserves sourcemap
   649	        // for moved content (e.g., defineProps type params).
   650	        for &(start, end, target, prefix, suffix) in &self.wrapped_moves {
   651	            ct.move_wrapped(start, end, target, prefix, suffix);
   652	        }
   653	
   654	        // Sort by start ascending, then by end descending (so that for equal
   655	        // starts, the wider range comes first and the narrower is filtered out).
   656	        self.overwrites
   657	            .sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
   658	
   659	        // Filter out fully-contained ranges. After sorting, any range whose
   660	        // end <= the running max_end is fully inside a preceding range and
   661	        // would produce a redundant (overlapping) overwrite.
   662	        let mut max_end: u32 = 0;
   663	        self.overwrites.retain(|&(start, end, _)| {
   664	            if start >= max_end {
   665	                // Non-overlapping — accept and update max_end
   666	                max_end = end;
   667	                true
   668	            } else if end > max_end {
   669	                // Partial overlap extending beyond max_end — accept the extension
   670	                max_end = end;
   671	                true
   672	            } else {
   673	                // Fully contained (start >= prev_start && end <= max_end) — drop
   674	                false
   675	            }
   676	        });
   677	
   678	        ct.batch_overwrite(&self.overwrites);
   679	
   680	        // Unmapped overwrites are few and never overlap `overwrites` (both
   681	        // target root-level tag spans) — a sorted loop is enough.
   682	        //
   683	        // `overwrite_unmapped_alloc`, not `overwrite_unmapped`: every entry
   684	        // here is ALREADY bump-allocated against this same `'alloc`
   685	        // allocator (`overwrite_unmapped_alloc`/`overwrite_or_root_prefix_alloc`/
   686	        // `overwrite_or_root_suffix_alloc`'s nonzero-width branch all push
   687	        // here) — `overwrite_unmapped`'s unconditional re-allocation would
   688	        // mint a NEW pointer, breaking any `record_sfc_export_fact` marker
   689	        // minted against the original.
   690	        self.unmapped_overwrites
   691	            .sort_unstable_by_key(|&(start, ..)| start);
   692	        for &(start, end, content) in &self.unmapped_overwrites {
   693	            ct.overwrite_unmapped_alloc(start, end, content);
   694	        }
   695	
   696	        // Segmented overwrites: disjoint from the other overwrite channels
   697	        // (one mechanism per AST node), so ordering vs those channels is
   698	        // irrelevant. Failure is a caller precondition violation, not
   699	        // recoverable. Fail closed in every build: a silent `ct.overwrite()`
   700	        // fallback would re-fabricate whole-block provenance (every byte of
   701	        // `content` mapping to `start`) — a silently wrong source map.
   702	        self.segmented_overwrites
   703	            .sort_unstable_by_key(|&(start, ..)| start);
   704	        for &(start, end, content, anchors) in &self.segmented_overwrites {
   705	            if let Err(err) = ct.try_overwrite_segmented(
   706	                start,
   707	                end,
   708	                content,
   709	                anchors,
   710	                SegmentedOverwriteAuthority::new(),
   711	            ) {
   712	                panic!("overwrite_segmented precondition violated at [{start},{end}): {err:?}");
   713	            }
   714	        }
   715	
   716	        if self.mapped_prepends.is_empty() {
   717	            // Fast path: no mapped prepends, use the simpler batch method.
   718	            // Must use stable sort to preserve insertion order for same-position
   719	            // prepends. Scope-close suffixes (e.g., ` : _createCommentVNode(...)`)
   720	            // are pushed before sibling comma separators during tree walking, and
   721	            // both land at the element's end position. Unstable sort can reorder
   722	            // them, producing invalid JS like `, : _createCommentVNode(...)`.
   723	            self.prepends.sort_by_key(|(pos, _)| *pos);
   724	            ct.batch_prepend_left_static(&self.prepends);
   725	        } else {
   726	            // Merge the unmapped (`prepends`) and source-mapped (`mapped_prepends`)
   727	            // channels DIRECTLY during one chunk rebuild — no third temporary Vec.
   728	            // Each channel is stably sorted by position; the merge emits every
   729	            // unmapped prepend before any mapped prepend at an equal position, so
   730	            // the two channels interleave at a shared anchor in unmapped-first order.
     1	//! Additive segmented overwrite.
     2	//!
     3	//! `Chunk::Overwritten` emits at most one source-map token for the whole
     4	//! replacement (MagicString: no character-level correspondence). Wrong
     5	//! when generated scaffolding embeds authored lexemes at known offsets
     6	//! (interpolation inside `_toDisplayString(...)`).
     7	//!
     8	//! `try_overwrite_segmented` is a separate crate-private entry; it does
     9	//! not change `overwrite`/`update`. Target range must lie in one live
    10	//! `Original` chunk with no affinity-anchored insertion — otherwise a
    11	//! typed refusal, not a best-effort splice.
    12	
    13	use super::chunk::Chunk;
    14	use super::code_transform::CodeTransform;
    15	use super::fallible::CodeTransformError;
    16	use crate::template::code_gen::types::SegmentedOverwriteAuthority;
    17	
    18	/// Authored lexeme inside a segmented overwrite:
    19	/// `content[content_offset..content_offset + length]` maps to
    20	/// `[source_pos, source_pos + length)`. Bytes outside anchors are synthetic
    21	/// (no source-map token).
    22	///
    23	/// `pub` to match the `pub` carriers that hold it (`VaporTextPart::Dynamic`,
    24	/// …). Operations that produce a `SegmentAnchor` chunk stay restricted to
    25	/// Vue runtime emitters ([`SegmentedOverwriteAuthority`]).
    26	#[derive(Debug, Clone, Copy, PartialEq, Eq)]
    27	pub struct SegmentAnchor {
    28	    pub content_offset: u32,
    29	    pub length: u32,
    30	    pub source_pos: u32,
    31	}
    32	
    33	impl SegmentAnchor {
    34	    /// Test-only; production builds a struct literal from AST/segment-plan data.
    35	    #[cfg(test)]
    36	    pub(crate) fn new(content_offset: u32, length: u32, source_pos: u32) -> Self {
    37	        Self {
    38	            content_offset,
    39	            length,
    40	            source_pos,
    41	        }
    42	    }
    43	
    44	    fn content_end(&self) -> u32 {
    45	        self.content_offset + self.length
    46	    }
    47	}
    48	
    49	impl<'a> CodeTransform<'a> {
    50	    /// Checked segmented overwrite. Fails atomically (no mutation on `Err`)
    51	    /// for a malformed/empty range, out-of-order or overlapping anchors,
    52	    /// out-of-range/`source_pos` mid-character, a target not entirely in one
    53	    /// live `Original` chunk, or any affinity-anchored insertion.
    54	    ///
    55	    /// `_authority` is the static call-site guard ([`SegmentedOverwriteAuthority`]).
    56	    pub(crate) fn try_overwrite_segmented(
    57	        &mut self,
    58	        start: u32,
    59	        end: u32,
    60	        content: &str,
    61	        anchors: &[SegmentAnchor],
    62	        _authority: SegmentedOverwriteAuthority,
    63	    ) -> Result<&mut Self, CodeTransformError> {
    64	        self.record_audit_op();
    65	        self.check_range(start, end)?;
    66	        if start == end {
    67	            return Err(CodeTransformError::ZeroLengthRange { offset: start });
    68	        }
    69	        if self.anchored_present {
    70	            return Err(CodeTransformError::ReplacedContentSplit { offset: start });
    71	        }
    72	
    73	        let content_len = content.len() as u32;
    74	        let mut prev_end = 0u32;
    75	        for anchor in anchors {
    76	            if anchor.content_offset < prev_end {
    77	                return Err(CodeTransformError::ReversedRange {
    78	                    start: anchor.content_offset,
    79	                    end: prev_end,
    80	                });
    81	            }
    82	            let anchor_end = anchor.content_end();
    83	            if anchor_end > content_len {
    84	                return Err(CodeTransformError::OutOfRange {
    85	                    offset: anchor_end,
    86	                    len: content_len,
    87	                });
    88	            }
    89	            if !content.is_char_boundary(anchor.content_offset as usize)
    90	                || !content.is_char_boundary(anchor_end as usize)
    91	            {
    92	                return Err(CodeTransformError::MidChar {
    93	                    offset: anchor.content_offset,
    94	                });
    95	            }
    96	            self.check_offset(anchor.source_pos)?;
    97	            let source_end = anchor.source_pos + anchor.length;
    98	            self.check_offset(source_end)?;
    99	            prev_end = anchor_end;
   100	        }
   101	
   102	        // Narrow precondition: the range must fall entirely inside ONE
   103	        // live `Original` chunk (the exact shape `try_fast_overwrite`
   104	        // already isolates for the unchecked splice — reused here as a
   105	        // read-only classification, never mutating on a `false` result).
   106	        let Some(chunk_index) = self.find_sole_containing_original(start, end) else {
   107	            return Err(CodeTransformError::ReplacedContentSplit { offset: start });
   108	        };
   109	
   110	        let content_ref = if content.is_empty() {
   111	            ""
   112	        } else {
   113	            self.allocator.alloc_str(content)
   114	        };
   115	        let anchors_ref: &'a [SegmentAnchor] = if anchors.is_empty() {
   116	            &[]
   117	        } else {
   118	            self.allocator.alloc_slice_copy(anchors)
   119	        };
   120	
   121	        self.splice_segmented_fast(chunk_index, start, end, content_ref, anchors_ref);
   122	        self.output_delta += content_ref.len() as i64 - (end - start) as i64;
   123	        Ok(self)
   124	    }
   125	
   126	    /// Read-only classification mirroring `try_fast_overwrite`'s own
   127	    /// precondition: `[start, end)` falls entirely inside exactly one live
   128	    /// `Original` chunk. Returns that chunk's index, never mutating.
   129	    fn find_sole_containing_original(&self, start: u32, end: u32) -> Option<usize> {
   130	        let search_start = self.search_start_for(start);
   131	        for i in search_start..self.chunks.len() {
   132	            match self.chunks[i] {
   133	                Chunk::Original { start: cs, end: ce } => {
   134	                    if ce <= start {
   135	                        continue;
   136	                    }
   137	                    if cs > start {
   138	                        return None;
   139	                    }
   140	                    if ce < end {
   141	                        return None;
   142	                    }
   143	                    return Some(i);
   144	                }
   145	                Chunk::Overwritten { .. }
   146	                | Chunk::OverwrittenSegmented { .. }
   147	                | Chunk::Moved { .. }
   148	                | Chunk::Inserted { .. }
   149	                | Chunk::InsertedMapped { .. }
   150	                | Chunk::InsertedAnchored { .. } => {
    75	use crate::types::NodeId;
    76	
    77	use super::binding::BindingResolver;
    78	use super::expression::{build_prefixed_expr_segments, resolve_simple_expr_segments};
    79	use super::shared::helpers::{self, VdomHelper};
    80	use super::types::{
    81	    ChildKind, ChildRecord, CodeGenOutput, ConditionChainRole, MappedGeneratedText, ScopeClose,
    82	    SegmentedOverwriteAuthority,
    83	};
    84	use super::{TemplateCodeGen, TemplateCodeGenOptions};
    85	
    86	/// VDOM code generation backend.
    87	///
    88	/// Produces `_createElementVNode()` / `_createElementBlock()` calls with
    89	/// patch flags, dynamic props arrays, and proper children wrapping.
    90	///
    91	/// Child records are built on-demand from the AST in `leave_element` /
    92	/// `leave_template`, eliminating the need for a per-element state stack.
    93	pub struct VdomCodeGen<'ast, 'alloc> {
    94	    /// Reference to the template AST arena for O(1) node lookups.
    95	    ast: &'ast TemplateAst,
    96	    /// NodeId-aligned OXC parse data — used for the official-parity
    97	    /// `hasScopeRef` slot-flag decision (scanning a component's slot
    98	    /// subtree for references to outer template-scope variables).
    99	    oxc_ast: &'ast crate::template::oxc::types::OxcParsedAst<'alloc>,
   100	    resolver: BindingResolver<'alloc>,
   101	    options: TemplateCodeGenOptions,
   102	    /// Reusable buffer for building open/close tag strings.
   103	    /// Uses `std::mem::take` pattern to avoid per-element allocation.
   104	    buf: String,
   105	    /// Scope close stack for structural directives (v-if, v-for).
   106	    /// Pushed in `enter_element`, popped in `leave_element`.
   107	    scope_closes: Vec<Option<ScopeClose>>,
   108	    /// v-for prefix stack. Stored during `enter_element` and consumed by
   109	    /// `process_element_leave` to include in the open tag overwrite. This
   110	    /// ensures correct ordering when a sibling text node ends at the same
   111	    /// position as the v-for element starts.
   112	    /// Tuple: (prefix_string, iterable_source_start) for source map mapping.
   113	    v_for_prefixes: Vec<Option<(String, Option<u32>)>>,
   114	    /// Pre-computed condition expressions with binding resolution, carried as
   115	    /// segment plans so the ternary head maps authored identifiers to source
   116	    /// while leaving synthetic scaffolding unmapped.
   117	    /// Populated during `enter_element` (where OXC data is available) and
   118	    /// consumed by `build_child_records` (which only sees AST data).
   119	    /// Keyed by AST node index. Holds the bare resolved expression (no `(` …
   120	    /// `) ? ` wrapper); `build_child_records` wraps it per element.
   121	    resolved_condition_prefixes: FxHashMap<usize, MappedGeneratedText>,
   122	    /// Whether the template has a single effective root element (not multi-root).
   123	    /// Set in `enter_template`, used by `leave_element` to determine if a root
   124	    /// element should be a block root (`_createElementBlock` / `_createBlock`).
   125	    single_root: bool,
   126	    /// Hoisted constant strings (e.g., `["id"]`) collected during codegen,
   127	    /// each paired with its OWN authored anchors (relative to the string's
   128	    /// own start) — a hoisted static-props object can embed a static
   129	    /// attribute's own key (e.g. `class`), and that anchor must survive
   130	    /// into the hoisted-preamble insertion (see
   131	    /// `code_transform::segmented`'s module doc). Empty when the string
   132	    /// carries no mappable key (the common case — most hoisted constants
   133	    /// are `["id"]`-style arrays or `{ key: N }` branch markers). Emitted as
   134	    /// `const _hoisted_N = ...` before the render function. Deduplicated:
   135	    /// identical strings share the same `_hoisted_N` reference (first
   136	    /// occurrence's anchors win — see `reserve_hoist`).
   137	    hoisted_constants: Vec<(String, Vec<SegmentAnchor>)>,
   138	    /// Cache index counter for `_cache[N]` static element wrapping.
   139	    /// Incremented each time a fully-static element is cached.
   140	    cache_index: usize,
   141	    /// Whether we are currently inside a slot function body.
   142	    /// When true, `leave_element` skips individual `_cache[N]` wrapping
   143	    /// because slot-level cache grouping handles it instead.
   144	    /// Stored as a stack to handle nested slot contexts.
   145	    in_slot_context_stack: Vec<bool>,
   620	    /// `hoisted_constants`'s field doc.
   621	    fn reserve_hoist(&mut self, id: NodeId, content: String, anchors: Vec<SegmentAnchor>) {
   622	        let idx = if let Some(existing) = self
   623	            .hoisted_constants
   624	            .iter()
   625	            .position(|(c, _)| *c == content)
   626	        {
   627	            existing + 1
   628	        } else {
   629	            self.hoisted_constants.push((content, anchors));
   630	            self.hoisted_constants.len()
   631	        };
   632	        self.hoist_reservations.insert(id.0, idx);
   633	    }
   634	
   635	    /// Build child records from AST children (O(n) scan).
   636	    ///
   637	    /// Replaces the old per-element `ElementState.children` accumulator.
   638	    /// Children are classified on-demand from the AST when the parent's
   639	    /// leave phase needs them.
   640	    pub(super) fn build_child_records(
   641	        &self,
   642	        children: &[NodeId],
   643	        source: &str,
   644	    ) -> Vec<ChildRecord> {
   645	        let mut records = Vec::with_capacity(children.len());
   646	        for &child_id in children {
   647	            let node = &self.ast.nodes[child_id.0];
   648	            match &node.kind {
   649	                AstNodeKind::Text(text_node) => {
   650	                    let content = &source[text_node.start as usize..text_node.end as usize];
   651	                    if let Some(kind) = text::classify_text_kind(content) {
   652	                        records.push(ChildRecord {
   653	                            start: text_node.start,
   654	                            end: text_node.end,
   655	                            kind,
   656	                            condition: None,
   657	                            condition_prefix: None,
   658	                        });
   659	                    }
   660	                }
   661	                AstNodeKind::Interpolation(interp) => {
   662	                    records.push(ChildRecord {
   663	                        start: interp.start,
   664	                        end: interp.end,
   665	                        kind: ChildKind::Interpolation,
   666	                        condition: None,
   667	                        condition_prefix: None,
   668	                    });
   669	                }
   670	                AstNodeKind::Element(el) => {
   671	                    let end = el
   672	                        .tag_close
   673	                        .as_ref()
   674	                        .map(|tc| tc.end)
   675	                        .unwrap_or(el.tag_open.end);
   676	
   677	                    let (condition, condition_prefix) = match el.v_condition.as_ref() {
   678	                        Some(c) => {
   679	                            let role = match c.kind {
   680	                                ElementNodeConditionKind::If => ConditionChainRole::Start,
   681	                                ElementNodeConditionKind::ElseIf
   682	                                | ElementNodeConditionKind::Else => {
   683	                                    ConditionChainRole::Continuation
   684	                                }
   685	                            };
   686	                            // Build the ternary head for v-if/v-else-if (not v-else).
   687	                            // Wrap the pre-resolved expression plan from
   688	                            // `enter_element` (the only place with OXC binding data
   689	                            // for correct $setup./$props. prefixes) in the synthetic
   690	                            // `(` … `) ? ` so only authored identifiers map to source.
   691	                            let prefix = match c.kind {
   692	                                ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
   693	                                    Some(self.condition_prefix_segments(child_id.0, c, source))
   694	                                }
   695	                                ElementNodeConditionKind::Else => None,
   696	                            };
   697	                            (Some(role), prefix)
   698	                        }
   699	                        None => (None, None),
   700	                    };
   701	                    records.push(ChildRecord {
   702	                        start: el.tag_open.start,
   703	                        end,
   704	                        kind: ChildKind::Element,
   705	                        condition,
   706	                        condition_prefix,
   707	                    });
   708	                }
   709	                AstNodeKind::Comment(comment) => {
   710	                    if self.options.comments {
   711	                        records.push(ChildRecord {
   712	                            start: comment.start,
   713	                            end: comment.end,
   714	                            kind: ChildKind::Comment,
   715	                            condition: None,
   716	                            condition_prefix: None,
   717	                        });
   718	                    }
   719	                }
   720	            }
   721	        }
   722	
   723	        records
   724	    }
   725	
   726	    /// Build the `(` … `) ? ` ternary head plan for a v-if/v-else-if element.
   727	    ///
   728	    /// Wraps the pre-resolved expression plan stored by `enter_element` (keyed
   729	    /// by AST node index) in the synthetic ternary head, so only authored
   730	    /// identifiers carry source-map tokens. `enter_element` always populates the
   810	        // Static-array group caching (official `cacheStatic`, a separate
   811	        // pass) numbers after handler caching (the main-transform pass) —
   812	        // see both reservation maps' doc comments. `v-memo` caching still
   813	        // runs interleaved with individual static caching during the main
   814	        // walk, unchanged; no current corpus case combines it with either
   815	        // pre-pass, so their exact relative order there is not modeled.
   816	        self.reserve_array_group_caches(root_children, true);
   817	        // Open tag overwrite is deferred to leave_template where we have
   818	        // full context (children count, v-if status) to emit the correct
   819	        // combined prefix (function signature + return + openBlock).
   820	    }
   821	
   822	    fn leave_template(
   823	        &mut self,
   824	        root: &RootNodeTemplate,
   825	        source: &'alloc str,
   826	        out: &mut CodeGenOutput<'alloc>,
   827	    ) {
   828	        let root_children = root
   829	            .content
   830	            .as_ref()
   831	            .map(|c| c.children.as_slice())
   832	            .unwrap_or(&[]);
   833	        let mut children = self.build_child_records(root_children, source);
   834	
   835	        // Resolve whitespace at root level. Leading and trailing whitespace
   836	        // are dropped from the children vec WITHOUT overwrites — the combined
   837	        // open/close tag overwrites below cover those source regions.
   838	        // Interior whitespace is resolved with overwrites as usual.
   839	        {
   840	            // Drop leading whitespace (no overwrite)
   841	            let leading = children
   842	                .iter()
   843	                .take_while(|c| element::is_whitespace_kind(c.kind))
   844	                .count();
   845	            children.drain(..leading);
   846	
   847	            // Drop trailing whitespace (no overwrite)
   848	            while children
   849	                .last()
   850	                .is_some_and(|c| element::is_whitespace_kind(c.kind))
   851	            {
   852	                children.pop();
   853	            }
   854	
   855	            // Resolve interior whitespace (with overwrites)
   856	            let mut i = 0;
   857	            while i < children.len() {
   858	                match children[i].kind {
   859	                    ChildKind::WhitespaceNewline => {
   860	                        let removed = children.remove(i);
   861	                        out.overwrite(removed.start, removed.end, "");
   862	                    }
   863	                    ChildKind::WhitespaceSpace => {
   864	                        out.overwrite(children[i].start, children[i].end, " ");
   865	                        children[i].kind = ChildKind::Text;
   866	                        i += 1;
   867	                    }
   868	                    _ => {
   869	                        i += 1;
   870	                    }
   871	                }
   872	            }
   873	        }
   874	
   875	        // Strip comments/text between v-if chain members (at root level too)
   876	        element::strip_interstitial_condition_nodes(&mut children, out, true);
   877	
   878	        // Build hoisted constant preamble (e.g., `const _hoisted_1 = ["id"]\n`),
   879	        // tracking each constant's own embedded anchors (shifted to their
   880	        // absolute position within `hoisted_preamble`) — the opt-in
  1030	                    for cont in children.iter().skip(1) {
  1031	                        if let Some(ref cond) = cont.condition_prefix {
  1032	                            children::emit_condition_prefix_mapped(out, cont.start, cond);
  1033	                        }
  1034	                    }
  1035	
  1036	                    out.overwrite_or_root_suffix(close_start, close_end, "\n}");
  1037	                } else if self.root_element_has_v_memo(root_children, source) {
  1038	                    // v-memo root: `_withMemo(..., () => (_openBlock(), …))`
  1039	                    // owns its openBlock inside the memo factory (emitted by
  1040	                    // leave_element), so leave_template must NOT add an outer
  1041	                    // `(_openBlock(), …)` wrapper — just `return`.
  1042	                    let mut prefix = String::with_capacity(full_prefix.len() + 8);
  1043	                    prefix.push_str(&full_prefix);
  1044	                    prefix.push_str("return ");
  1045	                    out.overwrite_or_root_prefix(tag_open.start, child.start, &prefix);
  1046	                    out.overwrite_or_root_suffix(close_start, close_end, "\n}");
  1047	                } else if self.root_element_has_directives_wrap(root_children, source) {
  1048	                    // Directives-wrapped root (v-show, native v-model, a
  1049	                    // custom directive) with no v-if/v-for of its own:
  1050	                    // `process_element_leave` already nests its own
  1051	                    // `(_openBlock(), …)` inside `_withDirectives(…)` — see
  1052	                    // `root_element_has_directives_wrap`'s doc comment.
  1053	                    // leave_template must NOT add a second outer wrapper.
  1054	                    let mut prefix = String::with_capacity(full_prefix.len() + 8);
  1055	                    prefix.push_str(&full_prefix);
  1056	                    prefix.push_str("return ");
  1057	                    out.overwrite_or_root_prefix(tag_open.start, child.start, &prefix);
  1058	                    out.overwrite_or_root_suffix(close_start, close_end, "\n}");
  1059	                } else {
  1060	                    // Single root — block root with _openBlock + _createElementBlock
  1061	                    out.add_vdom_import(VdomHelper::OpenBlock);
  1062	                    let mut prefix = String::with_capacity(full_prefix.len() + 24);
  1063	                    prefix.push_str(&full_prefix);
  1064	                    prefix.push_str("return (_openBlock(), ");
  1065	                    // `hoisted_preamble_anchors` apply at THEIR OWN recorded
  1066	                    // offsets only when `hoisted_preamble` was folded
  1067	                    // unshifted into `full_prefix`'s own start — exactly the
  1068	                    // `!is_inline` case (see `full_prefix`'s own
  1069	                    // construction above); the inline case emits
  1070	                    // `hoisted_preamble` through a SEPARATE module-scope
  1071	                    // prepend instead, so its anchors do not apply here.
  1072	                    if !self.options.is_inline && !hoisted_preamble_anchors.is_empty() {
  1073	                        out.overwrite_or_root_prefix_segmented(
  1074	                            tag_open.start,
  1075	                            child.start,
  1076	                            &prefix,
  1077	                            &hoisted_preamble_anchors,
  1078	                            SegmentedOverwriteAuthority::new(),
  1079	                        );
  1080	                    } else {
  1081	                        out.overwrite_or_root_prefix(tag_open.start, child.start, &prefix);
  1082	                    }
  1083	                    out.overwrite_or_root_suffix(close_start, close_end, ")\n}");
  1084	                }
  1085	            }
  1086	            _ => {
  1087	                // Multi-root — wrap in Fragment
  1088	                out.add_vdom_import(VdomHelper::OpenBlock);
  1089	                out.add_vdom_import(VdomHelper::CreateElementBlock);
  1090	                out.add_vdom_import(VdomHelper::Fragment);
  1091	
  1092	                // Prefix: function sig + return + openBlock + Fragment + array open.
  1093	                let mut prefix = String::with_capacity(full_prefix.len() + 80);
  1094	                prefix.push_str(&full_prefix);
  1095	                prefix.push_str("return (_openBlock(), _createElementBlock(_Fragment, null, [");
  1710	        &mut self,
  1711	        id: NodeId,
  1712	        text_node: &TextNode,
  1713	        source: &'alloc str,
  1714	        out: &mut CodeGenOutput<'alloc>,
  1715	    ) {
  1716	        helpers::debug_assert_slice_bounds(source, text_node.start, text_node.end, "visit_text");
  1717	        // Skip text between v-if chain members (Vue discards these).
  1718	        // Don't emit an overwrite here — the parent's leave phase handles removal
  1719	        // (via strip_interstitial_condition_nodes or gap-filling).
  1720	        if self.is_interstitial_condition_node(id) {
  1721	            return;
  1722	        }
  1723	        // Apply text overwrites (condensation, escaping).
  1724	        // Child classification is handled by build_child_records from the AST.
  1725	        let _ = text::process_text(text_node, source, out);
  1726	    }
  1727	
  1728	    fn visit_interpolation(
  1729	        &mut self,
  1730	        _id: NodeId,
  1731	        interp: &InterpolationNode,
  1732	        oxc: &OxcParsedExpression<'alloc>,
  1733	        _source: &'alloc str,
  1734	        out: &mut CodeGenOutput<'alloc>,
  1735	    ) {
  1736	        // Apply delimiter overwrites and binding patches.
  1737	        // Child classification is handled by build_child_records from the AST.
  1738	        let _ = interpolation::process_interpolation(interp, oxc, &self.resolver, out);
  1739	    }
  1740	
  1741	    fn visit_comment(
  1742	        &mut self,
  1743	        id: NodeId,
  1744	        comment_node: &CommentNode,
  1745	        source: &'alloc str,
  1746	        out: &mut CodeGenOutput<'alloc>,
  1747	    ) {
  1748	        helpers::debug_assert_slice_bounds(
  1749	            source,
  1750	            comment_node.start,
  1751	            comment_node.end,
  1752	            "visit_comment",
  1753	        );
  1754	        // Skip comments between v-if chain members (Vue discards these).
  1755	        // Emit removal overwrite directly — the parent's leave phase may not
  1756	        // include this comment in its child records (when options.comments=false,
  1757	        // build_child_records excludes comments, so strip_interstitial_condition_nodes
  1758	        // can't find them). At root level, gap-filling also doesn't cover these.
  1759	        if self.is_interstitial_condition_node(id) {
  1760	            out.overwrite(comment_node.start, comment_node.end, "");
  1761	            return;
  1762	        }
  1763	        // Apply comment overwrites (or removal if disabled).
  1764	        // Child classification is handled by build_child_records from the AST.
  1765	        let _ = comment::process_comment(comment_node, source, self.options.comments, out);
  1766	    }
  1767	}
  1768	
  1769	#[cfg(test)]
  1770	mod tests;
     1	//! VDOM comment node code generation.
     2	//!
     3	//! Transforms `<!-- content -->` into `_createCommentVNode("content")`.
     4	//! When comments are disabled, the comment is removed entirely.
     5	//!
     6	//! Two processing modes:
     7	//! - **Simple**: content has no special chars → two overwrites for prefix and suffix.
     8	//! - **Complex**: content has `"`, `\`, newlines → same two overwrites + `prepend_alloc`
     9	//!   for the escaped content and `overwrite_alloc` to delete the original.
    10	
    11	use crate::ast::types::CommentNode;
    12	
    13	use super::super::shared::helpers::{escape_js_string_into, needs_js_escaping, VdomHelper};
    14	use super::super::types::{ChildKind, ChildRecord, CodeGenOutput};
    15	
    16	/// Process a comment node for VDOM codegen.
    17	///
    18	/// If `comments_enabled` is `true`, transforms `<!-- content -->` into
    19	/// `_createCommentVNode("content")`. If `false`, returns `None` (comment is dropped).
    20	///
    21	/// Returns a [`ChildRecord`] with `ChildKind::Comment` when the comment is kept.
    22	pub fn process_comment<'alloc>(
    23	    comment: &CommentNode,
    24	    source: &str,
    25	    comments_enabled: bool,
    26	    out: &mut CodeGenOutput<'alloc>,
    27	) -> Option<ChildRecord> {
    28	    if !comments_enabled {
    29	        // Drop the comment: overwrite entire span with empty string
    30	        out.overwrite(comment.start, comment.end, "");
    31	        return None;
    32	    }
    33	
    34	    let content = &source[comment.content_start as usize..comment.content_end as usize];
    35	
    36	    // Simple: no escaping needed — two overwrites for prefix and suffix
    37	    out.overwrite(
    38	        comment.start,
    39	        comment.content_start,
    40	        "_createCommentVNode(\"",
    41	    );
    42	    out.overwrite(comment.content_end, comment.end, "\")");
    43	
    44	    // Register the runtime helper import
    45	    out.add_vdom_import(VdomHelper::CreateCommentVNode);
    46	
    47	    // Complex: content needs escaping — prepend escaped content, delete original
    48	    if needs_js_escaping(content) {
    49	        let mut buf = String::with_capacity(content.len() + content.len() / 4);
    50	        escape_js_string_into(&mut buf, content);
    51	        out.prepend_alloc(comment.content_start, &buf);
    52	        out.overwrite_alloc(comment.content_start, comment.content_end, "");
    53	    }
    54	
    55	    Some(ChildRecord {

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_compiler/src/template/code_gen/vdom/mod.rs | sed -n '85,185p;300,410p;878,1035p;1085,1150p' && nl -ba crates/verter_compiler/src/code_transform/segmented.rs | sed -n '1,160p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
    85	
    86	/// VDOM code generation backend.
    87	///
    88	/// Produces `_createElementVNode()` / `_createElementBlock()` calls with
    89	/// patch flags, dynamic props arrays, and proper children wrapping.
    90	///
    91	/// Child records are built on-demand from the AST in `leave_element` /
    92	/// `leave_template`, eliminating the need for a per-element state stack.
    93	pub struct VdomCodeGen<'ast, 'alloc> {
    94	    /// Reference to the template AST arena for O(1) node lookups.
    95	    ast: &'ast TemplateAst,
    96	    /// NodeId-aligned OXC parse data — used for the official-parity
    97	    /// `hasScopeRef` slot-flag decision (scanning a component's slot
    98	    /// subtree for references to outer template-scope variables).
    99	    oxc_ast: &'ast crate::template::oxc::types::OxcParsedAst<'alloc>,
   100	    resolver: BindingResolver<'alloc>,
   101	    options: TemplateCodeGenOptions,
   102	    /// Reusable buffer for building open/close tag strings.
   103	    /// Uses `std::mem::take` pattern to avoid per-element allocation.
   104	    buf: String,
   105	    /// Scope close stack for structural directives (v-if, v-for).
   106	    /// Pushed in `enter_element`, popped in `leave_element`.
   107	    scope_closes: Vec<Option<ScopeClose>>,
   108	    /// v-for prefix stack. Stored during `enter_element` and consumed by
   109	    /// `process_element_leave` to include in the open tag overwrite. This
   110	    /// ensures correct ordering when a sibling text node ends at the same
   111	    /// position as the v-for element starts.
   112	    /// Tuple: (prefix_string, iterable_source_start) for source map mapping.
   113	    v_for_prefixes: Vec<Option<(String, Option<u32>)>>,
   114	    /// Pre-computed condition expressions with binding resolution, carried as
   115	    /// segment plans so the ternary head maps authored identifiers to source
   116	    /// while leaving synthetic scaffolding unmapped.
   117	    /// Populated during `enter_element` (where OXC data is available) and
   118	    /// consumed by `build_child_records` (which only sees AST data).
   119	    /// Keyed by AST node index. Holds the bare resolved expression (no `(` …
   120	    /// `) ? ` wrapper); `build_child_records` wraps it per element.
   121	    resolved_condition_prefixes: FxHashMap<usize, MappedGeneratedText>,
   122	    /// Whether the template has a single effective root element (not multi-root).
   123	    /// Set in `enter_template`, used by `leave_element` to determine if a root
   124	    /// element should be a block root (`_createElementBlock` / `_createBlock`).
   125	    single_root: bool,
   126	    /// Hoisted constant strings (e.g., `["id"]`) collected during codegen,
   127	    /// each paired with its OWN authored anchors (relative to the string's
   128	    /// own start) — a hoisted static-props object can embed a static
   129	    /// attribute's own key (e.g. `class`), and that anchor must survive
   130	    /// into the hoisted-preamble insertion (see
   131	    /// `code_transform::segmented`'s module doc). Empty when the string
   132	    /// carries no mappable key (the common case — most hoisted constants
   133	    /// are `["id"]`-style arrays or `{ key: N }` branch markers). Emitted as
   134	    /// `const _hoisted_N = ...` before the render function. Deduplicated:
   135	    /// identical strings share the same `_hoisted_N` reference (first
   136	    /// occurrence's anchors win — see `reserve_hoist`).
   137	    hoisted_constants: Vec<(String, Vec<SegmentAnchor>)>,
   138	    /// Cache index counter for `_cache[N]` static element wrapping.
   139	    /// Incremented each time a fully-static element is cached.
   140	    cache_index: usize,
   141	    /// Whether we are currently inside a slot function body.
   142	    /// When true, `leave_element` skips individual `_cache[N]` wrapping
   143	    /// because slot-level cache grouping handles it instead.
   144	    /// Stored as a stack to handle nested slot contexts.
   145	    in_slot_context_stack: Vec<bool>,
   146	    /// Hoisted _resolveComponent() calls: Vec of (tag_name, variable_name).
   147	    /// Emitted as `const _component_x = _resolveComponent("x")` at the top
   148	    /// of the render function body. Insertion-ordered.
   149	    resolved_components: Vec<(String, String)>,
   150	    /// Per-item v-memo close suffix for a `v-for` + `v-memo` element, keyed by
   151	    /// AST node index. Built in `enter_element` (where the v-for prefix and memo
   152	    /// index are computed) and applied in `leave_element` in place of the normal
   153	    /// `_renderList` fragment close.
   154	    memo_for_suffixes: FxHashMap<usize, String>,
   155	    /// `_hoisted_N` index reserved during `enter_element` for an element's
   156	    /// own props/injected-key hoist, keyed by AST node index.
   157	    ///
   158	    /// Official `_hoisted_N` numbering comes from `cacheStatic`, a SEPARATE
   159	    /// pass over the transformed tree that walks in document PRE-order — a
   160	    /// node's own hoistable props register before recursing into its
   161	    /// children (`@vue/compiler-core`: the parent's `walk()` call hoists
   162	    /// each child's props before descending into that child). Verter's
   163	    /// codegen is necessarily bottom-up (`leave_element` fires child before
   164	    /// parent, since parent content depends on built children), so without
   165	    /// this, any element that is both an ancestor of a hoistable descendant
   166	    /// AND itself hoistable gets the wrong (too-late) index — confirmed on
   167	    /// `basic-interpolation.vue`: golden hoists `{ class: "root" }` as
   168	    /// `_hoisted_1` (the root, an ancestor) then the `v-if`/`v-else` branch
   169	    /// keys as `_hoisted_2`/`_hoisted_3` (descendants) — reserving the
   170	    /// ancestor's slot before descending is what keeps that order right.
   171	    ///
   172	    /// `try_reserve_element_hoist` (called from `enter_element`, BEFORE
   173	    /// children are visited) computes and reserves the slot for the two
   174	    /// cases `process_element_leave` already hoists — a fully-literal props
   175	    /// object, and a `v-if`/`v-else` branch's synthetic `{ key: N }` — using
   176	    /// only facts available without visiting children (`element.props`,
   177	    /// `element.is_fully_static`, structural AST lookups). It deliberately
   178	    /// does NOT replicate the `_cache[N]`/`slot_cached` parent-lookup
   179	    /// eligibility logic: restricting to `!element.is_fully_static`
   180	    /// structurally guarantees `has_cached_patchflag` is false for the
   181	    /// plain-props case (an element with a dynamic descendant can never be
   182	    /// `is_fully_static`), and a `v-if`/`v-else` branch can never be
   183	    /// `cache_idx`-eligible at all (that mechanism requires
   184	    /// `v_condition.is_none()`). Every case this fast path does NOT
   185	    /// recognize (dynamic props, components, fully-static subtrees) falls
   300	                _ => None,
   301	            };
   302	            let is_component = el.tag_type.is_component();
   303	            let mut reservations: FxHashMap<usize, usize> = FxHashMap::default();
   304	            for (prop_idx, prop) in el.props.iter().enumerate() {
   305	                if !prop.is_directive {
   306	                    continue;
   307	                }
   308	                let dname = &source[prop.start as usize..prop.name_end as usize];
   309	                let is_on = is_v_on(dname);
   310	                if is_on && prop.arg_start.is_some() {
   311	                    let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) else {
   312	                        continue;
   313	                    };
   314	                    let value = &source[vs as usize..ve as usize];
   315	                    let oxc_exp =
   316	                        crate::template::code_gen::vapor::find_prop_oxc_exp(oxc_el, prop_idx);
   317	                    // Modifier wrapping (`_withModifiers`/`_withKeys`) composes
   318	                    // around the handler span separately — no current case
   319	                    // combines it with handler caching, so skip reserving
   320	                    // when modifiers are present (mirrors the emission-site
   321	                    // guard in `build_props_object_into`).
   322	                    if prop.modifiers.is_empty()
   323	                        && element::is_cacheable_inline_event_handler(value, oxc_exp)
   324	                    {
   325	                        let idx = self.cache_index;
   326	                        self.cache_index += 1;
   327	                        reservations.insert(prop_idx, idx);
   328	                    }
   329	                } else if dname == "v-model" && !is_component {
   330	                    // Native `v-model` always produces a `$event => (… = $event)`
   331	                    // update handler — always cacheable.
   332	                    let idx = self.cache_index;
   333	                    self.cache_index += 1;
   334	                    reservations.insert(prop_idx, idx);
   335	                }
   336	            }
   337	            if !reservations.is_empty() {
   338	                self.handler_cache_reservations.insert(id.0, reservations);
   339	            }
   340	        }
   341	    }
   342	
   343	    /// A direct element child that (per the leave-time `cache_idx` logic in
   344	    /// `leave_element`) would be individually `_cache[N]`-eligible: fully
   345	    /// static, not a block root, no structural directive, not a component
   346	    /// (components use slot-level caching instead). A child found via
   347	    /// `el.content.children` is never itself the single template root, so
   348	    /// that check does not apply here.
   349	    fn child_is_group_cacheable(&self, id: NodeId) -> bool {
   350	        let AstNodeKind::Element(el) = &self.ast.nodes[id.0].kind else {
   351	            return false;
   352	        };
   353	        el.is_fully_static
   354	            && el.v_condition.is_none()
   355	            && el.v_for.is_none()
   356	            && !el.tag_type.is_component()
   357	    }
   358	
   359	    /// Walk `ids` bottom-up (children before self) and, for every element
   360	    /// whose direct children are ALL individually `_cache[N]`-eligible
   361	    /// (`child_is_group_cacheable`), reserve ONE `_cache[N]` slot to hold
   362	    /// the whole children array — official Vue's `cacheStatic`: when
   363	    /// `toCache.length === children.length`, the node's children are
   364	    /// grouped into one cached array and spread back
   365	    /// (`[...(_cache[N] || (_cache[N] = [...]))]`) instead of caching each
   366	    /// child separately. A mix of static and dynamic/directive children
   367	    /// disqualifies the whole parent — each cacheable child then falls back
   368	    /// to its own individual `cache_idx`, unchanged. The parent's OWN
   369	    /// eligibility (block root, directives, …) does not gate this —
   370	    /// `static-element.vue`'s root `<div id=… title=…>` IS the block root
   371	    /// and still groups its one static `<p>` child.
   372	    fn reserve_array_group_caches(&mut self, ids: &[NodeId], is_root_level: bool) {
   373	        for &id in ids {
   374	            let AstNodeKind::Element(el) = &self.ast.nodes[id.0].kind else {
   375	                continue;
   376	            };
   377	            // Slot outlets (`<slot>` fallback content), `<template>`
   378	            // (`v-slot`/`v-if`/`v-for`), and components all route their
   379	            // children through entirely separate leave-time functions
   380	            // (`process_slot_outlet`, `process_template_slot`,
   381	            // `leave_template_fragment`, `leave_component_with_slots`,
   382	            // `leave_component_with_default_slot`) that never consult
   383	            // `array_group_reservations` — each already has its own
   384	            // pre-existing slot-level cache-grouping mechanism. Reserving
   385	            // (and thus advancing `self.cache_index`) for their children
   386	            // here would both dead-reserve an unused slot and desync the
   387	            // numbering those mechanisms compute independently at leave
   388	            // time. Do not recurse into them at all.
   389	            if el.tag_type.is_slot_outlet()
   390	                || el.tag_type == TagType::Template
   391	                || el.tag_type.is_component()
   392	            {
   393	                continue;
   394	            }
   395	            let children = el
   396	                .content
   397	                .as_ref()
   398	                .map(|c| c.children.as_slice())
   399	                .unwrap_or(&[]);
   400	            self.reserve_array_group_caches(children, false);
   401	
   402	            if children.is_empty() {
   403	                continue;
   404	            }
   405	
   406	            // If `id` itself would be swallowed into an ancestor's single
   407	            // cache (fully static, not a block root, no structural
   408	            // directive, not a component — mirrors the leave-time
   409	            // `parent_is_cached` check in `leave_element`), its children
   410	            // never reach individual OR grouped caching at all: the
   878	        // Build hoisted constant preamble (e.g., `const _hoisted_1 = ["id"]\n`),
   879	        // tracking each constant's own embedded anchors (shifted to their
   880	        // absolute position within `hoisted_preamble`) — the opt-in
   881	        // segmented-overwrite primitive's anchor shape (see
   882	        // `code_transform::segmented`'s module doc). Consumed ONLY by the
   883	        // single-root, non-v-if/v-memo `leave_template` branch below (the
   884	        // shape the current test corpus requires); every other branch
   885	        // keeps splicing `hoisted_preamble` through the pre-existing
   886	        // unmapped `overwrite_or_root_prefix` path.
   887	        let mut hoisted_preamble_anchors: Vec<SegmentAnchor> = Vec::new();
   888	        let hoisted_preamble = if self.hoisted_constants.is_empty() {
   889	            String::new()
   890	        } else {
   891	            let mut preamble = String::with_capacity(self.hoisted_constants.len() * 30);
   892	            for (i, (constant, constant_anchors)) in self.hoisted_constants.iter().enumerate() {
   893	                preamble.push_str("const _hoisted_");
   894	                preamble.push_str(&(i + 1).to_string());
   895	                preamble.push_str(" = ");
   896	                let base = preamble.len() as u32;
   897	                preamble.push_str(constant);
   898	                preamble.push('\n');
   899	                hoisted_preamble_anchors.extend(constant_anchors.iter().map(|a| SegmentAnchor {
   900	                    content_offset: base + a.content_offset,
   901	                    length: a.length,
   902	                    source_pos: a.source_pos,
   903	                }));
   904	            }
   905	            preamble.push('\n');
   906	            preamble
   907	        };
   908	
   909	        // Inline mode: hoisted constants are MODULE-scope (official emits them
   910	        // in the module preamble, prepended by compileScript) — not inside the
   911	        // setup closure. Recorded via `set_module_preamble` (NOT a
   912	        // position-anchored `prepend_alloc(0, ...)` — see that method's doc
   913	        // comment: a position-0 prepend here loses the ordering race
   914	        // against the script codegen's OWN position-0 user-import hoist,
   915	        // which already ran and baked an opaque chunk by the time this
   916	        // template codegen pass runs). The orchestrator applies it with
   917	        // `ct.prepend(...)` before its own import-line prepend, so the
   918	        // final order is: helper import, hoisted consts, user code
   919	        // (matching official).
   920	        if self.options.is_inline && !hoisted_preamble.is_empty() {
   921	            out.set_module_preamble(&hoisted_preamble);
   922	        }
   923	
   924	        // Function signature prefix. Official `@vue/compiler-core` emits the
   925	        // full `(_ctx, _cache, $props, $setup, $data, $options)` form only when
   926	        // binding metadata exists (a script block) and the template is not
   927	        // inlined; template-only SFCs get the 2-param `(_ctx, _cache)` form —
   928	        // their bodies reference only `_ctx`/`_cache` (no bindings to route
   929	        // through `$props`/`$setup`/`$data`/`$options`).
   930	        let fn_sig = if self.options.is_inline {
   931	            "return (_ctx,_cache) => {\n"
   932	        } else if self.options.has_script {
   933	            "function render(_ctx, _cache, $props, $setup, $data, $options) {\n"
   934	        } else {
   935	            "function render(_ctx, _cache) {\n"
   936	        };
   937	
   938	        // Build resolved component declarations (inside the function body)
   939	        // e.g., `const _component_el_button = _resolveComponent("el-button")\n`
   940	        let resolved_comp_preamble = if self.resolved_components.is_empty() {
   941	            String::new()
   942	        } else {
   943	            let mut s = String::with_capacity(self.resolved_components.len() * 60);
   944	            for (tag, var) in &self.resolved_components {
   945	                // Check if this is a self-reference
   946	                let is_self_ref = !self.options.self_name.is_empty() && {
   947	                    let pascal = component::to_pascal_case(tag);
   948	                    pascal == self.options.self_name
   949	                };
   950	                s.push_str("const ");
   951	                s.push_str(var);
   952	                s.push_str(" = _resolveComponent(\"");
   953	                s.push_str(tag);
   954	                if is_self_ref {
   955	                    s.push_str("\", true)\n");
   956	                } else {
   957	                    s.push_str("\")\n");
   958	                }
   959	            }
   960	            s
   961	        };
   962	
   963	        // Combined preamble: hoisted constants + function signature + resolved components
   964	        let full_prefix = {
   965	            // Inline keeps hoists OUT of the render chunk (they were emitted
   966	            // at module scope above) — the chunk starts at the arrow.
   967	            let mut s = if self.options.is_inline || hoisted_preamble.is_empty() {
   968	                fn_sig.to_string()
   969	            } else {
   970	                let mut p = hoisted_preamble;
   971	                p.push_str(fn_sig);
   972	                p
   973	            };
   974	            if !resolved_comp_preamble.is_empty() {
   975	                s.push_str(&resolved_comp_preamble);
   976	                s.push('\n');
   977	            }
   978	            s
   979	        };
   980	
   981	        // Determine close tag region
   982	        let (close_start, close_end) = match root.tag_close.as_ref() {
   983	            Some(tc) => (tc.start, tc.end),
   984	            None => {
   985	                let pos = root
   986	                    .content
   987	                    .as_ref()
   988	                    .map(|c| c.end)
   989	                    .unwrap_or(root.tag_open.end);
   990	                (pos, pos)
   991	            }
   992	        };
   993	
   994	        // Count effective roots: v-if chains collapse into a single root.
   995	        let effective_count = children
   996	            .iter()
   997	            .filter(|c| c.condition != Some(ConditionChainRole::Continuation))
   998	            .count();
   999	
  1000	        let tag_open = &root.tag_open;
  1001	
  1002	        match effective_count {
  1003	            0 => {
  1004	                // Empty template — overwrite everything
  1005	                let mut buf = String::with_capacity(full_prefix.len() + 16);
  1006	                buf.push_str(&full_prefix);
  1007	                buf.push_str("return null\n}");
  1008	                out.overwrite_or_root_prefix(tag_open.start, close_end, &buf);
  1009	            }
  1010	            1 => {
  1011	                let child = &children[0];
  1012	                let is_v_if = child.condition == Some(ConditionChainRole::Start);
  1013	
  1014	                if is_v_if {
  1015	                    // Root-level v-if chain — overwrite up to child.start with
  1016	                    // the function signature + "return ", then emit the condition
  1017	                    // prefix as a separate source-mapped prepend.
  1018	                    let mut prefix = String::with_capacity(full_prefix.len() + 32);
  1019	                    prefix.push_str(&full_prefix);
  1020	                    prefix.push_str("return ");
  1021	                    out.overwrite_or_root_prefix(tag_open.start, child.start, &prefix);
  1022	
  1023	                    // Emit the v-if condition prefix with per-segment source mapping.
  1024	                    if let Some(ref cond) = child.condition_prefix {
  1025	                        children::emit_condition_prefix_mapped(out, child.start, cond);
  1026	                    }
  1027	
  1028	                    // Emit condition prefixes for continuation children
  1029	                    // (v-else-if elements in the chain) with source mapping.
  1030	                    for cont in children.iter().skip(1) {
  1031	                        if let Some(ref cond) = cont.condition_prefix {
  1032	                            children::emit_condition_prefix_mapped(out, cont.start, cond);
  1033	                        }
  1034	                    }
  1035	
  1085	            }
  1086	            _ => {
  1087	                // Multi-root — wrap in Fragment
  1088	                out.add_vdom_import(VdomHelper::OpenBlock);
  1089	                out.add_vdom_import(VdomHelper::CreateElementBlock);
  1090	                out.add_vdom_import(VdomHelper::Fragment);
  1091	
  1092	                // Prefix: function sig + return + openBlock + Fragment + array open.
  1093	                let mut prefix = String::with_capacity(full_prefix.len() + 80);
  1094	                prefix.push_str(&full_prefix);
  1095	                prefix.push_str("return (_openBlock(), _createElementBlock(_Fragment, null, [");
  1096	                out.overwrite_or_root_prefix(tag_open.start, children[0].start, &prefix);
  1097	
  1098	                // Delegate to wrap_array_text_runs for separators AND text
  1099	                // wrapping. This handles:
  1100	                // - Comma separators between array items
  1101	                // - _createTextVNode() wrapping for text/interpolation runs
  1102	                // - Condition prefix emission (v-if/v-else-if)
  1103	                // - v-for prefix ordering (comma at prev_item_end)
  1104	                children::add_children_separators_array(
  1105	                    &children,
  1106	                    out,
  1107	                    &self.options,
  1108	                    source,
  1109	                    self.ast,
  1110	                    root_children,
  1111	                );
  1112	
  1113	                // Close fragment + render function.
  1114	                //
  1115	                // Official Vue flags a root Fragment `STABLE_FRAGMENT |
  1116	                // DEV_ROOT_FRAGMENT` (2112) when it exists ONLY because comments
  1117	                // sit beside a SINGLE logical non-comment root — so the runtime
  1118	                // filters to the real root for fallthrough / HMR. A v-if/v-else
  1119	                // chain counts as ONE logical root (its continuation arms do not
  1120	                // add). Two or more real roots stay a plain STABLE_FRAGMENT (64).
  1121	                let has_comment = children.iter().any(|c| c.kind == ChildKind::Comment);
  1122	                let logical_root_count = children
  1123	                    .iter()
  1124	                    .filter(|c| {
  1125	                        !matches!(
  1126	                            c.kind,
  1127	                            ChildKind::Comment
  1128	                                | ChildKind::WhitespaceNewline
  1129	                                | ChildKind::WhitespaceSpace
  1130	                        ) && c.condition != Some(ConditionChainRole::Continuation)
  1131	                    })
  1132	                    .count();
  1133	                let frag_flag = if has_comment && logical_root_count == 1 {
  1134	                    helpers::PATCH_STABLE_FRAGMENT | helpers::PATCH_DEV_ROOT_FRAGMENT
  1135	                } else {
  1136	                    helpers::PATCH_STABLE_FRAGMENT
  1137	                };
  1138	                let flag_str =
  1139	                    helpers::format_patch_flag(frag_flag, self.options.is_production, |s| {
  1140	                        out.alloc_str(s)
  1141	                    });
  1142	                let mut close_buf = String::with_capacity(32);
  1143	                close_buf.push_str("\n], ");
  1144	                close_buf.push_str(flag_str);
  1145	                close_buf.push_str("))\n}");
  1146	                out.overwrite_or_root_suffix(close_start, close_end, &close_buf);
  1147	            }
  1148	        }
  1149	    }
  1150	
     1	//! Additive segmented overwrite.
     2	//!
     3	//! `Chunk::Overwritten` emits at most one source-map token for the whole
     4	//! replacement (MagicString: no character-level correspondence). Wrong
     5	//! when generated scaffolding embeds authored lexemes at known offsets
     6	//! (interpolation inside `_toDisplayString(...)`).
     7	//!
     8	//! `try_overwrite_segmented` is a separate crate-private entry; it does
     9	//! not change `overwrite`/`update`. Target range must lie in one live
    10	//! `Original` chunk with no affinity-anchored insertion — otherwise a
    11	//! typed refusal, not a best-effort splice.
    12	
    13	use super::chunk::Chunk;
    14	use super::code_transform::CodeTransform;
    15	use super::fallible::CodeTransformError;
    16	use crate::template::code_gen::types::SegmentedOverwriteAuthority;
    17	
    18	/// Authored lexeme inside a segmented overwrite:
    19	/// `content[content_offset..content_offset + length]` maps to
    20	/// `[source_pos, source_pos + length)`. Bytes outside anchors are synthetic
    21	/// (no source-map token).
    22	///
    23	/// `pub` to match the `pub` carriers that hold it (`VaporTextPart::Dynamic`,
    24	/// …). Operations that produce a `SegmentAnchor` chunk stay restricted to
    25	/// Vue runtime emitters ([`SegmentedOverwriteAuthority`]).
    26	#[derive(Debug, Clone, Copy, PartialEq, Eq)]
    27	pub struct SegmentAnchor {
    28	    pub content_offset: u32,
    29	    pub length: u32,
    30	    pub source_pos: u32,
    31	}
    32	
    33	impl SegmentAnchor {
    34	    /// Test-only; production builds a struct literal from AST/segment-plan data.
    35	    #[cfg(test)]
    36	    pub(crate) fn new(content_offset: u32, length: u32, source_pos: u32) -> Self {
    37	        Self {
    38	            content_offset,
    39	            length,
    40	            source_pos,
    41	        }
    42	    }
    43	
    44	    fn content_end(&self) -> u32 {
    45	        self.content_offset + self.length
    46	    }
    47	}
    48	
    49	impl<'a> CodeTransform<'a> {
    50	    /// Checked segmented overwrite. Fails atomically (no mutation on `Err`)
    51	    /// for a malformed/empty range, out-of-order or overlapping anchors,
    52	    /// out-of-range/`source_pos` mid-character, a target not entirely in one
    53	    /// live `Original` chunk, or any affinity-anchored insertion.
    54	    ///
    55	    /// `_authority` is the static call-site guard ([`SegmentedOverwriteAuthority`]).
    56	    pub(crate) fn try_overwrite_segmented(
    57	        &mut self,
    58	        start: u32,
    59	        end: u32,
    60	        content: &str,
    61	        anchors: &[SegmentAnchor],
    62	        _authority: SegmentedOverwriteAuthority,
    63	    ) -> Result<&mut Self, CodeTransformError> {
    64	        self.record_audit_op();
    65	        self.check_range(start, end)?;
    66	        if start == end {
    67	            return Err(CodeTransformError::ZeroLengthRange { offset: start });
    68	        }
    69	        if self.anchored_present {
    70	            return Err(CodeTransformError::ReplacedContentSplit { offset: start });
    71	        }
    72	
    73	        let content_len = content.len() as u32;
    74	        let mut prev_end = 0u32;
    75	        for anchor in anchors {
    76	            if anchor.content_offset < prev_end {
    77	                return Err(CodeTransformError::ReversedRange {
    78	                    start: anchor.content_offset,
    79	                    end: prev_end,
    80	                });
    81	            }
    82	            let anchor_end = anchor.content_end();
    83	            if anchor_end > content_len {
    84	                return Err(CodeTransformError::OutOfRange {
    85	                    offset: anchor_end,
    86	                    len: content_len,
    87	                });
    88	            }
    89	            if !content.is_char_boundary(anchor.content_offset as usize)
    90	                || !content.is_char_boundary(anchor_end as usize)
    91	            {
    92	                return Err(CodeTransformError::MidChar {
    93	                    offset: anchor.content_offset,
    94	                });
    95	            }
    96	            self.check_offset(anchor.source_pos)?;
    97	            let source_end = anchor.source_pos + anchor.length;
    98	            self.check_offset(source_end)?;
    99	            prev_end = anchor_end;
   100	        }
   101	
   102	        // Narrow precondition: the range must fall entirely inside ONE
   103	        // live `Original` chunk (the exact shape `try_fast_overwrite`
   104	        // already isolates for the unchecked splice — reused here as a
   105	        // read-only classification, never mutating on a `false` result).
   106	        let Some(chunk_index) = self.find_sole_containing_original(start, end) else {
   107	            return Err(CodeTransformError::ReplacedContentSplit { offset: start });
   108	        };
   109	
   110	        let content_ref = if content.is_empty() {
   111	            ""
   112	        } else {
   113	            self.allocator.alloc_str(content)
   114	        };
   115	        let anchors_ref: &'a [SegmentAnchor] = if anchors.is_empty() {
   116	            &[]
   117	        } else {
   118	            self.allocator.alloc_slice_copy(anchors)
   119	        };
   120	
   121	        self.splice_segmented_fast(chunk_index, start, end, content_ref, anchors_ref);
   122	        self.output_delta += content_ref.len() as i64 - (end - start) as i64;
   123	        Ok(self)
   124	    }
   125	
   126	    /// Read-only classification mirroring `try_fast_overwrite`'s own
   127	    /// precondition: `[start, end)` falls entirely inside exactly one live
   128	    /// `Original` chunk. Returns that chunk's index, never mutating.
   129	    fn find_sole_containing_original(&self, start: u32, end: u32) -> Option<usize> {
   130	        let search_start = self.search_start_for(start);
   131	        for i in search_start..self.chunks.len() {
   132	            match self.chunks[i] {
   133	                Chunk::Original { start: cs, end: ce } => {
   134	                    if ce <= start {
   135	                        continue;
   136	                    }
   137	                    if cs > start {
   138	                        return None;
   139	                    }
   140	                    if ce < end {
   141	                        return None;
   142	                    }
   143	                    return Some(i);
   144	                }
   145	                Chunk::Overwritten { .. }
   146	                | Chunk::OverwrittenSegmented { .. }
   147	                | Chunk::Moved { .. }
   148	                | Chunk::Inserted { .. }
   149	                | Chunk::InsertedMapped { .. }
   150	                | Chunk::InsertedAnchored { .. } => {
   151	                    if let Some(cs) = chunk_start(&self.chunks[i]) {
   152	                        if cs > start {
   153	                            return None;
   154	                        }
   155	                    }
   156	                }
   157	            }
   158	        }
   159	        None
   160	    }

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n \"VdomCodeGen \\{|impl<'ast, 'alloc> VdomCodeGen|fn new\\(\" crates/verter_compiler/src/template/code_gen/vdom/mod.rs | head -n 30 && nl -ba crates/verter_compiler/src/template/code_gen/vdom/mod.rs | sed -n '215,300p' && nl -ba crates/verter_compiler/src/template/code_gen/walker.rs | sed -n '1,260p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 1ms:[0m
[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_compiler/src/template/code_gen/vdom/mod.rs | sed -n '500,625p' && sed -n '185,220p' crates/verter_compiler/src/template/code_gen/vdom/mod.rs"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
   500	                i += 1;
   501	            }
   502	        }
   503	        reservations
   504	    }
   505	
   506	    pub fn new(
   507	        ast: &'ast TemplateAst,
   508	        oxc_ast: &'ast crate::template::oxc::types::OxcParsedAst<'alloc>,
   509	        resolver: BindingResolver<'alloc>,
   510	        options: &TemplateCodeGenOptions,
   511	    ) -> Self {
   512	        Self {
   513	            ast,
   514	            oxc_ast,
   515	            resolver,
   516	            options: options.clone(),
   517	            buf: String::with_capacity(128),
   518	            scope_closes: Vec::new(),
   519	            v_for_prefixes: Vec::new(),
   520	            resolved_condition_prefixes: FxHashMap::default(),
   521	            single_root: false,
   522	            hoisted_constants: Vec::new(),
   523	            cache_index: 0,
   524	            in_slot_context_stack: Vec::new(),
   525	            resolved_components: Vec::new(),
   526	            memo_for_suffixes: FxHashMap::default(),
   527	            hoist_reservations: FxHashMap::default(),
   528	            handler_cache_reservations: FxHashMap::default(),
   529	            array_group_reservations: FxHashMap::default(),
   530	            array_grouped_children: FxHashSet::default(),
   531	        }
   532	    }
   533	
   534	    /// Reserve a document-pre-order `_hoisted_N` slot for `id`'s own props
   535	    /// or injected branch key, BEFORE any of its children are visited. See
   536	    /// [`Self::hoist_reservations`] for the full rationale and scope.
   537	    fn try_reserve_element_hoist(
   538	        &mut self,
   539	        id: NodeId,
   540	        element: &ElementNode,
   541	        oxc: Option<&OxcParsedElement<'alloc>>,
   542	        source: &'alloc str,
   543	    ) {
   544	        // Whole-subtree-static elements route through `_createStaticVNode`
   545	        // / `_cache[N]` instead — restrict to elements with at least one
   546	        // dynamic descendant, where `has_cached_patchflag` is structurally
   547	        // guaranteed false (see the field doc comment).
   548	        if element.is_fully_static || element.tag_type.is_component() {
   549	            return;
   550	        }
   551	        // A slot outlet (`<slot>`) never reaches `process_element_leave` —
   552	        // `leave_element` routes it to `process_slot_outlet`, which builds
   553	        // its own `_renderSlot(...)` props string and never consults
   554	        // `pre_reserved_hoist`. Same for ANY `<template>` element — both
   555	        // `v-slot` (`process_template_slot`) and `v-if`/`v-for`
   556	        // (`leave_template_fragment`) route to their own separate
   557	        // functions. Reserving for either here would push an orphaned,
   558	        // unreferenced `_hoisted_N` (confirmed as a regression on
   559	        // `slots.vue`'s `<slot name="header">` during development).
   560	        if element.tag_type.is_slot_outlet() || element.tag_type == TagType::Template {
   561	            return;
   562	        }
   563	
   564	        let has_props = !element.props.is_empty() || element.v_ref.is_some();
   565	        if has_props {
   566	            // Any directive makes the props object non-constant; bail to
   567	            // the unchanged leave-time path, which resolves the dynamic
   568	            // shape correctly.
   569	            if element.props.iter().any(|p| p.is_directive) {
   570	                return;
   571	            }
   572	            let mut buf = String::new();
   573	            let mut anchors: Vec<SegmentAnchor> = Vec::new();
   574	            let props_result = element::build_props_object_into(
   575	                &mut buf,
   576	                element,
   577	                source,
   578	                &self.resolver,
   579	                oxc,
   580	                None,
   581	                self.options.force_js,
   582	                &mut anchors,
   583	                // No directive props reach here (early return above) — a
   584	                // handler/`v-model` reservation is structurally impossible.
   585	                None,
   586	            );
   587	            // Mirrors `can_hoist_props`'s guard set in
   588	            // `element::process_element_leave`, minus
   589	            // `injected_key.is_none()` (a `has_props` element never carries
   590	            // one — injected keys only apply on the props-less branch) and
   591	            // `!has_cached_patchflag` (structurally false here already).
   592	            let can_hoist = props_result.dynamic_props.is_empty()
   593	                && !props_result.has_vnode_key
   594	                && !props_result.has_dynamic_ref
   595	                && !props_result.uses_merge
   596	                && !props_result.uses_normalize_class
   597	                && !props_result.uses_normalize_style
   598	                && !props_result.uses_normalize_props
   599	                && !props_result.uses_guard_reactive_props
   600	                && !props_result.uses_to_handlers
   601	                && props_result.native_vmodel.is_none()
   602	                && props_result.directive_entries.is_empty();
   603	            if can_hoist {
   604	                self.reserve_hoist(id, buf, anchors);
   605	            }
   606	        } else if element.v_condition.is_some()
   607	            && element.v_for.is_none()
   608	            && !directives::element_has_vnode_key(element, source)
   609	        {
   610	            if let Some(k) = directives::condition_branch_index(self.ast, id) {
   611	                self.reserve_hoist(id, format!("{{ key: {k} }}"), Vec::new());
   612	            }
   613	        }
   614	    }
   615	
   616	    /// Push (or dedup-reuse) a hoisted constant now and remember its index
   617	    /// for `id`, so `process_element_leave` can reference it directly
   618	    /// instead of pushing again at leave time. `anchors` are `content`'s own
   619	    /// embedded authored anchors (relative to `content`'s own start) — see
   620	    /// `hoisted_constants`'s field doc.
   621	    fn reserve_hoist(&mut self, id: NodeId, content: String, anchors: Vec<SegmentAnchor>) {
   622	        let idx = if let Some(existing) = self
   623	            .hoisted_constants
   624	            .iter()
   625	            .position(|(c, _)| *c == content)
    /// recognize (dynamic props, components, fully-static subtrees) falls
    /// through untouched to `process_element_leave`'s existing bottom-up
    /// logic, which remains the sole authority for them — this map is
    /// purely additive, never a second hoist-decision engine.
    hoist_reservations: FxHashMap<usize, usize>,
    /// `_cache[N]` indices reserved for cacheable inline event handlers and
    /// the native `v-model` update handler, keyed by element `NodeId` then
    /// `prop_idx`. Populated once by [`Self::reserve_handler_caches`] in
    /// `enter_template`, BEFORE `self.cache_index` starts being consumed by
    /// static-vnode/`v-memo` caching during the main bottom-up walk —
    /// mirroring official Vue's two-pass model (the main AST transform
    /// assigns every handler/`v-memo` `context.cache()` slot; the SEPARATE
    /// `cacheStatic` pass that runs after it assigns static-vnode slots).
    /// Reservation itself walks in the SAME bottom-up (children-then-self)
    /// order `leave_element` visits nodes, so relative ordering among
    /// handlers matches; it does not attempt to interleave with `v-memo`.
    handler_cache_reservations: FxHashMap<usize, FxHashMap<usize, usize>>,
    /// `_cache[N]` reserved to hold an element's ENTIRE static children
    /// array as one group, keyed by the element's own `NodeId`. Populated
    /// by [`Self::reserve_array_group_caches`] in `enter_template`: an
    /// element qualifies when EVERY direct child would otherwise be
    /// individually `_cache[N]`-eligible (official Vue's `cacheStatic`:
    /// `toCache.length === children.length` groups into one cached array
    /// spread instead of caching each child separately). A child whose
    /// parent has a reservation here is looked up via
    /// `array_grouped_children` and skips its own individual `cache_idx`.
    array_group_reservations: FxHashMap<usize, usize>,
    /// The set of child `NodeId`s covered by an `array_group_reservations`
    /// entry on their parent — these render un-individually-cached (their
    /// parent's array wrapper covers them) but keep their own `-1 CACHED`
    /// patch flag, same as the existing slot-context `slot_cached` path.
    array_grouped_children: FxHashSet<usize>,
}

impl<'ast, 'alloc> VdomCodeGen<'ast, 'alloc> {
    /// True when the single logical root element carries a `v-memo` directive.

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_compiler/src/template/code_gen/vapor/mod.rs | sed -n '3310,3395p;4125,4190p' && nl -ba crates/verter_compiler/src/template/code_gen/vapor/comment.rs | sed -n '1,90p' && rg -n \"out\\.overwrite\\(|out\\.overwrite_unmapped|out\\.overwrite_segmented|out\\.overwrite_or\" crates/verter_compiler/src/template/code_gen/vapor | head -n 250"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 11ms:[0m
crates/verter_compiler/src/template/code_gen/vapor/mod.rs:3366:        out.overwrite_segmented(

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_compiler/src/template/code_gen/ssr/mod.rs | sed -n '4170,4335p;6500,6570p' && rg -n \"fn enter_template|fn leave_template|fn visit_comment|comments\" crates/verter_compiler/src/template/code_gen/ssr/mod.rs | tail -n 80 && rg -n \"out\\.overwrite\\(\" crates/verter_compiler/src/template/code_gen/ssr/mod.rs | head -n 200"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
  4170	
  4171	impl<'ast, 'alloc> TemplateCodeGen<'alloc> for SsrCodeGen<'ast, 'alloc> {
  4172	    fn enter_template(
  4173	        &mut self,
  4174	        root: &RootNodeTemplate,
  4175	        source: &'alloc str,
  4176	        out: &mut CodeGenOutput<'alloc>,
  4177	    ) {
  4178	        let root_children = root
  4179	            .content
  4180	            .as_ref()
  4181	            .map(|c| c.children.as_slice())
  4182	            .unwrap_or(&[]);
  4183	        let effective_count = self.count_effective_roots(root_children, source);
  4184	        // Count element-level roots only (excluding text/interpolation) for fragment
  4185	        // marker decisions. Vue SSR only adds fragment markers when there are
  4186	        // multiple element roots, not for text/interpolation content.
  4187	        let element_root_count = self.count_element_roots(root_children);
  4188	        self.is_multi_root = effective_count > 1;
  4189	
  4190	        // Vue's SSR compiler treats templates with root-level comments as
  4191	        // needing fragment markers for hydration, even if there's only one
  4192	        // effective element root. The single element still receives _attrs.
  4193	        let has_root_comments = root_children
  4194	            .iter()
  4195	            .any(|&id| matches!(self.ast.nodes[id.0].kind, AstNodeKind::Comment(_)));
  4196	        self.needs_fragment = element_root_count > 1 || has_root_comments;
  4197	
  4198	        if self.needs_fragment {
  4199	            // Open a push with the fragment open marker.
  4200	            out.prepend_alloc(root.tag_open.end, "_push(`<!--[-->");
  4201	            self.in_push = true;
  4202	        }
  4203	    }
  4204	
  4205	    fn leave_template(
  4206	        &mut self,
  4207	        root: &RootNodeTemplate,
  4208	        source: &'alloc str,
  4209	        out: &mut CodeGenOutput<'alloc>,
  4210	    ) {
  4211	        let root_children = root
  4212	            .content
  4213	            .as_ref()
  4214	            .map(|c| c.children.as_slice())
  4215	            .unwrap_or(&[]);
  4216	
  4217	        let effective_count = self.count_effective_roots(root_children, source);
  4218	
  4219	        // Build function signature with hoisted component resolves.
  4220	        // Official `@vue/compiler-core` (shared by SSR and VDOM codegen)
  4221	        // appends `$props, $setup, $data, $options` whenever binding
  4222	        // metadata exists (a script block) and the template is not inlined
  4223	        // (`options.bindingMetadata && !options.inline` —
  4224	        // compiler-core.cjs.js:3500); a template-only SFC's `ssrRender`
  4225	        // stays the bare 4-param form since nothing in its body references
  4226	        // those parameters. This mirrors the VDOM `render` signature rule
  4227	        // in `vdom/mod.rs`.
  4228	        self.buf.clear();
  4229	        if !self.options.is_inline && self.options.has_script {
  4230	            self.buf.push_str(
  4231	                "function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {\n",
  4232	            );
  4233	        } else {
  4234	            self.buf
  4235	                .push_str("function ssrRender(_ctx, _push, _parent, _attrs) {\n");
  4236	        }
  4237	        for resolve in &self.component_resolves {
  4238	            self.buf.push_str(resolve);
  4239	            self.buf.push('\n');
  4240	        }
  4241	        for resolve in &self.directive_resolves {
  4242	            self.buf.push_str(resolve);
  4243	            self.buf.push('\n');
  4244	        }
  4245	        if self.temp_var_needed {
  4246	            self.buf.push_str("let _temp0\n");
  4247	        }
  4248	        // SSR CSS v-bind(): inject `_cssVars` so root attrs carry the CSS custom
  4249	        // properties. Client uses `_useCssVars`; SSR must put values in HTML.
  4250	        if !self.options.ssr_css_vars.is_empty() {
  4251	            self.buf.push_str("const _cssVars = { style: {\n");
  4252	            for (i, (var_name, expr)) in self.options.ssr_css_vars.iter().enumerate() {
  4253	                if i > 0 {
  4254	                    self.buf.push_str(",\n");
  4255	                }
  4256	                let resolved = self.resolver.resolve_simple_expr(expr);
  4257	                let _ = write!(self.buf, "  \"{}\": ({})", var_name, resolved);
  4258	            }
  4259	            self.buf.push_str("\n} }\n");
  4260	        }
  4261	        let fn_sig = self.buf.clone();
  4262	
  4263	        let (close_start, close_end) = match root.tag_close.as_ref() {
  4264	            Some(tc) => (tc.start, tc.end),
  4265	            None => {
  4266	                let pos = root
  4267	                    .content
  4268	                    .as_ref()
  4269	                    .map(|c| c.end)
  4270	                    .unwrap_or(root.tag_open.end);
  4271	                (pos, pos)
  4272	            }
  4273	        };
  4274	
  4275	        // Close any open push before the function close.
  4276	        // For templates needing fragment markers (multi-root or root comments),
  4277	        // append the fragment close marker before closing.
  4278	        if self.needs_fragment {
  4279	            if self.in_push {
  4280	                out.prepend_alloc(close_start, "<!--]-->`)\n");
  4281	                self.in_push = false;
  4282	            } else {
  4283	                out.prepend_alloc(close_start, "_push(`<!--]-->`)\n");
  4284	            }
  4285	        } else if self.in_push {
  4286	            self.close_push(close_start, out);
  4287	        }
  4288	
  4289	        if effective_count == 0 {
  4290	            // Empty template
  4291	            out.overwrite_segmented(
  4292	                root.tag_open.start,
  4293	                close_end,
  4294	                &fn_sig,
  4295	                &[],
  4296	                SegmentedOverwriteAuthority::new(),
  4297	            );
  4298	            out.prepend_static(close_end, "}");
  4299	        } else {
  4300	            out.overwrite_segmented(
  4301	                root.tag_open.start,
  4302	                root.tag_open.end,
  4303	                &fn_sig,
  4304	                &[],
  4305	                SegmentedOverwriteAuthority::new(),
  4306	            );
  4307	            out.overwrite_segmented(
  4308	                close_start,
  4309	                close_end,
  4310	                "}",
  4311	                &[],
  4312	                SegmentedOverwriteAuthority::new(),
  4313	            );
  4314	        }
  4315	    }
  4316	
  4317	    fn enter_element(
  4318	        &mut self,
  4319	        _id: NodeId,
  4320	        el: &ElementNode,
  4321	        oxc: Option<&OxcParsedElement<'alloc>>,
  4322	        source: &'alloc str,
  4323	        out: &mut CodeGenOutput<'alloc>,
  4324	    ) -> super::WalkAction {
  4325	        let tag_name = self.tag_name(el, source);
  4326	        let is_root = self.is_root();
  4327	
  4328	        // ── 0. Implicit default slot wrapping ───────────────────
  4329	        // When inside a ComponentWithSlots with named slots, non-template
  4330	        // children need to be wrapped in `default: _withCtx(...)`.
  4331	        if matches!(
  4332	            self.elem_ctx.last(),
  4333	            Some(&ElemCtx::ComponentWithSlots) | Some(&ElemCtx::DynamicComponentWithSlots)
  4334	        ) && !(el.tag_type == TagType::Template && el.v_slot.is_some())
  4335	            && !self.default_slot_open
  6500	        self.buf.push_str(")}");
  6501	
  6502	        let anchors: Vec<SegmentAnchor> = segments
  6503	            .segments
  6504	            .iter()
  6505	            .filter_map(|seg| {
  6506	                seg.source_start.map(|source_pos| SegmentAnchor {
  6507	                    content_offset: wrap_offset + seg.generated_start,
  6508	                    length: seg.generated_end - seg.generated_start,
  6509	                    source_pos,
  6510	                })
  6511	            })
  6512	            .collect();
  6513	
  6514	        out.overwrite_segmented(
  6515	            interp.start,
  6516	            interp.end,
  6517	            &self.buf,
  6518	            &anchors,
  6519	            SegmentedOverwriteAuthority::new(),
  6520	        );
  6521	    }
  6522	
  6523	    fn visit_comment(
  6524	        &mut self,
  6525	        _id: NodeId,
  6526	        comment: &CommentNode,
  6527	        source: &'alloc str,
  6528	        out: &mut CodeGenOutput<'alloc>,
  6529	    ) {
  6530	        if !self.options.comments {
  6531	            out.overwrite_segmented(
  6532	                comment.start,
  6533	                comment.end,
  6534	                "",
  6535	                &[],
  6536	                SegmentedOverwriteAuthority::new(),
  6537	            );
  6538	            return;
  6539	        }
  6540	
  6541	        let content = &source[comment.start as usize..comment.end as usize];
  6542	        // Use ensure_push to coalesce with adjacent nodes
  6543	        self.ensure_push(comment.start, out);
  6544	        let escaped = escape_template_literal(content);
  6545	        if escaped != content {
  6546	            out.overwrite_segmented(
  6547	                comment.start,
  6548	                comment.end,
  6549	                &escaped,
  6550	                &[],
  6551	                SegmentedOverwriteAuthority::new(),
  6552	            );
  6553	        }
  6554	    }
  6555	}
  6556	
  6557	// ======================== OXC lookup helper ========================
  6558	
  6559	/// Find the OXC parsed prop data for a given element prop index.
  6560	///
  6561	/// O(1) lookup through the element's dense `prop_lookup` table — no linear scan
  6562	/// over the sparse `props` vec.
  6563	fn find_oxc_prop<'a, 'alloc>(
  6564	    oxc: &'a OxcParsedElement<'alloc>,
  6565	    prop_index: usize,
  6566	) -> Option<&'a crate::template::oxc::types::OxcParsedProp<'alloc>> {
  6567	    oxc.prop(prop_index)
  6568	}
  6569	
  6570	// ======================== Utility functions ========================
126:    /// that have root-level comments (needed for hydration correctness).
448:    /// Count element-level roots, excluding text, interpolation, and comments.
899:                        *i += 1; // Skip comments between v-if branches
3927:    /// 2. Between two comments
3990:        // Rule 2: between two comments
4156:                // Skip whitespace text and comments between v-if branches
4172:    fn enter_template(
4190:        // Vue's SSR compiler treats templates with root-level comments as
4193:        let has_root_comments = root_children
4196:        self.needs_fragment = element_root_count > 1 || has_root_comments;
4205:    fn leave_template(
4276:        // For templates needing fragment markers (multi-root or root comments),
6405:            //   2. Between two comments
6523:    fn visit_comment(
6530:        if !self.options.comments {

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_compiler/src/template/code_gen/ssr/mod.rs | sed -n '390,475p;4120,4170p' && rg -n \"empty.*comment|comment.*empty|SSR.*comment|ssr.*comment|comments.*false|is_production\" crates/verter_compiler/src/template/code_gen/ssr crates/verter_compiler/src/compile_tests.rs crates/verter_compiler/tests/cases | head -n 300"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 112ms:[0m
   390	            self.in_push = false;
   391	        }
   392	    }
   393	
   394	    /// Ensure a `_push(\`...\`)` literal is open. If not, open one.
   395	    fn ensure_push(&mut self, pos: u32, out: &mut CodeGenOutput<'alloc>) {
   396	        if !self.in_push {
   397	            out.prepend_alloc(pos, "_push(`");
   398	            self.in_push = true;
   399	        }
   400	    }
   401	
   402	    // ── Tag and position helpers ───────────────────────────────
   403	
   404	    /// Extract the tag name from the element's open tag.
   405	    fn tag_name(&self, el: &ElementNode, source: &str) -> String {
   406	        source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize].to_string()
   407	    }
   408	
   409	    /// Get the end position of the element (end of close tag, or end of self-closing tag).
   410	    fn el_end(&self, el: &ElementNode) -> u32 {
   411	        el.tag_close
   412	            .as_ref()
   413	            .map(|tc| tc.end)
   414	            .unwrap_or(el.tag_open.end)
   415	    }
   416	
   417	    /// Look up OXC parsed data for a given node ID.
   418	    fn oxc_element(&self, id: NodeId) -> Option<&OxcParsedElement<'alloc>> {
   419	        match &self.oxc_ast.data[id.0] {
   420	            OxcNodeData::Element(e) => Some(e.as_ref()),
   421	            _ => None,
   422	        }
   423	    }
   424	
   425	    /// Look up OXC parsed interpolation for a given node ID.
   426	    fn oxc_interpolation(&self, id: NodeId) -> Option<&OxcParsedExpression<'alloc>> {
   427	        match &self.oxc_ast.data[id.0] {
   428	            OxcNodeData::Interpolation(expr) => Some(expr),
   429	            _ => None,
   430	        }
   431	    }
   432	
   433	    /// Resolve an expression using the binding resolver.
   434	    /// Note: SSR does NOT strip TypeScript syntax — Vue's SSR compiler preserves
   435	    /// `as` casts, `!` assertions, etc. Bundler-level TS stripping handles these.
   436	    fn resolve_expr(
   437	        &self,
   438	        expr: &str,
   439	        offset: u32,
   440	        oxc_expr: Option<&OxcParsedExpression<'alloc>>,
   441	    ) -> String {
   442	        resolve_expr(expr, offset, oxc_expr, &self.resolver, false)
   443	    }
   444	
   445	    /// Count effective root children (non-whitespace text, elements, interpolations).
   446	    /// v-else-if and v-else branches don't count as separate roots since only one
   447	    /// branch renders at a time.
   448	    /// Count element-level roots, excluding text, interpolation, and comments.
   449	    ///
   450	    /// Used for fragment marker decisions: Vue SSR only emits `<!--[-->...<!--]-->`
   451	    /// when there are 2+ element roots. Text/interpolation at root level is just
   452	    /// inline content and doesn't trigger fragment wrapping.
   453	    fn count_element_roots(&self, root_children: &[NodeId]) -> usize {
   454	        let mut count = 0;
   455	        for &child_id in root_children {
   456	            let child = &self.ast.nodes[child_id.0];
   457	            if let AstNodeKind::Element(ref el) = child.kind {
   458	                // v-else-if and v-else don't count as separate roots
   459	                if let Some(ref cond) = el.v_condition {
   460	                    if matches!(
   461	                        cond.kind,
   462	                        ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
   463	                    ) {
   464	                        continue;
   465	                    }
   466	                }
   467	                count += 1;
   468	            }
   469	        }
   470	        count
   471	    }
   472	
   473	    fn count_effective_roots(&self, root_children: &[NodeId], source: &str) -> usize {
   474	        let mut count = 0;
   475	        for &child_id in root_children {
  4120	            if let Some(result) = self.find_continuation_in(&root_content.children, el) {
  4121	                return result;
  4122	            }
  4123	        }
  4124	        for node in &self.ast.nodes {
  4125	            if let AstNodeKind::Element(ref parent_el) = node.kind {
  4126	                if let Some(ref content) = parent_el.content {
  4127	                    if let Some(result) = self.find_continuation_in(&content.children, el) {
  4128	                        return result;
  4129	                    }
  4130	                }
  4131	            }
  4132	        }
  4133	        false
  4134	    }
  4135	
  4136	    /// Search a children list for `el` (by tag_open.start) and check if
  4137	    /// the next element sibling has v-else-if or v-else.
  4138	    fn find_continuation_in(&self, children: &[NodeId], el: &ElementNode) -> Option<bool> {
  4139	        let mut found_self = false;
  4140	        for &child_id in children {
  4141	            let child = &self.ast.nodes[child_id.0];
  4142	            if let AstNodeKind::Element(ref child_el) = child.kind {
  4143	                if child_el.tag_open.start == el.tag_open.start {
  4144	                    found_self = true;
  4145	                    continue;
  4146	                }
  4147	                if found_self {
  4148	                    return Some(child_el.v_condition.as_ref().is_some_and(|c| {
  4149	                        matches!(
  4150	                            c.kind,
  4151	                            ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
  4152	                        )
  4153	                    }));
  4154	                }
  4155	            } else if found_self {
  4156	                // Skip whitespace text and comments between v-if branches
  4157	                if matches!(child.kind, AstNodeKind::Text(_) | AstNodeKind::Comment(_)) {
  4158	                    continue;
  4159	                }
  4160	                return Some(false);
  4161	            }
  4162	        }
  4163	        if found_self {
  4164	            Some(false)
  4165	        } else {
  4166	            None
  4167	        }
  4168	    }
  4169	}
  4170	
crates/verter_compiler/tests/cases/svelte_parser_strictness_fails_closed.rs:586:fn empty_comment_at_eof_is_unexpected_eof() {
crates/verter_compiler/tests/cases/svelte_parser_strictness_fails_closed.rs:589:    assert_fails_closed("empty_comment_eof", "<!--", "unexpected_eof");
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4190:        // Vue's SSR compiler treats templates with root-level comments as
crates/verter_compiler/tests/cases/style_planner.rs:519:            is_production: true,
crates/verter_compiler/tests/cases/svelte_client_fail_matrix.rs:555:        // An empty template (no rendered root) — the comment-anchor root shape. A
crates/verter_compiler/tests/cases/svelte_goldens_in_sync.rs:204:fn official_positive_semantic_comment_golden_is_non_empty() {
crates/verter_compiler/tests/cases/svelte_goldens_in_sync.rs:222:        "the official-positive `/*! general-license */` fixture must produce a non-empty semantic-comment oracle"
crates/verter_compiler/tests/cases/svelte_goldens_in_sync.rs:308:    if golden.backend == "server" && !golden.semantic_comment_signature.is_empty() {
crates/verter_compiler/tests/cases/svelte_carrier_runtime_compile_options_channel.rs:4://! `preserve_comments` / `disclose_version` to `None`/`false` regardless of what
crates/verter_compiler/src/template/code_gen/ssr/tests.rs:288:        "v-if without else should emit empty comment fallback, got:\n{}",
crates/verter_compiler/src/template/code_gen/ssr/tests.rs:532:fn ssr_dollar_escaped_in_comment() {
crates/verter_compiler/src/template/code_gen/ssr/tests.rs:710:fn ssr_comment_preserved_in_dev() {
crates/verter_compiler/src/template/code_gen/ssr/tests.rs:711:    let code = gen_ssr_template("<template><!-- comment --><div>hello</div></template>");
crates/verter_compiler/src/template/code_gen/ssr/tests.rs:4645:fn ssr_root_comment_before_element() {
crates/verter_compiler/src/template/code_gen/ssr/tests.rs:4900:fn ssr_v_else_if_chain_with_comments() {
crates/verter_compiler/src/template/code_gen/ssr/tests.rs:6236:fn ssr_vdom_fallback_comment_preserves_whitespace() {
crates/verter_compiler/src/compile_tests.rs:434:            is_production: true,
crates/verter_compiler/src/compile_tests.rs:737:            is_production: true,
crates/verter_compiler/src/compile_tests.rs:867:    let compile_profile = |is_production, custom_element| {
crates/verter_compiler/src/compile_tests.rs:871:                is_production,
crates/verter_compiler/src/compile_tests.rs:6633:/// when options.comments=false, so strip_interstitial_condition_nodes couldn't find them.
crates/verter_compiler/src/compile_tests.rs:6639:        is_production: true, // comments=false (default is !is_production)
crates/verter_compiler/src/compile_tests.rs:16603:        is_production: true,
crates/verter_compiler/src/compile_tests.rs:16724:        is_production: true,
crates/verter_compiler/src/compile_tests.rs:19096:// default (`resolve_inline` = `inline.unwrap_or(is_production)`); VDOM-only
crates/verter_compiler/src/compile_tests.rs:19282:    // resolve_inline: None → is_production (dev = non-inline, unchanged).
crates/verter_compiler/src/compile_tests.rs:19307:    // resolve_inline: None + is_production → inline (official prod default).
crates/verter_compiler/src/compile_tests.rs:19311:        is_production: true,
crates/verter_compiler/tests/cases/svelte_client_authored_value_routing_guard.rs:1019:        synthesized_construction_sites(&commented).is_empty(),
crates/verter_compiler/tests/cases/svelte_client_authored_value_routing_guard.rs:1626:        serialization_sites(&commented, &family).is_empty(),
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:2584:// `statements_sig`, node-types the segments, and gives comments attached to normalized-away empty
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:2641:// comment-anchor index mirrors that filter (a filtered empty gets a synthetic
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:2690:// segments, and gives comments attached to normalized-away empty statements an explicit synthetic
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:5457:    // The empty-statement filter ↔ comment-anchor index MISMATCH (false-PASS). `statements_sig`
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:5460:    // semantic comment attached to a FILTERED empty gets a SYNTHETIC
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:5472:        "a JSDoc semantic comment on a leading FILTERED empty statement vs on the leading real \
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:5503:         (the anchor index normalizes the empty away so the comment anchors the same real statement; \
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:5543:    // semantic comment on a real nested statement must compare EQUAL (the nested empty is normalized
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:5560:    // moved from BEFORE the first empty to BETWEEN the two empties must compare UNEQUAL — the comment
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:5567:        "a semantic (PURE) comment moved between two CONSECUTIVE filtered empty statements must FAIL \
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:5577:        "a PURE comment leading the FIRST of two consecutive filtered empties anchors empty_gap[0.0]"
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:5582:        "a PURE comment leading the SECOND of two consecutive filtered empties anchors empty_gap[0.1]"
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:5588:        "the same PURE comment at the same empty position differing only by whitespace must compare EQUAL"

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_compiler/src/template/code_gen/ssr/mod.rs | sed -n '473,530p' && nl -ba crates/verter_compiler/src/template/code_gen/ssr/tests.rs | sed -n '680,735p;4620,4675p' && rg -n \"TemplateCodeGenOptions|comments:\" crates/verter_compiler/src/template/code_gen/ssr/tests.rs | head -n 80"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
   473	    fn count_effective_roots(&self, root_children: &[NodeId], source: &str) -> usize {
   474	        let mut count = 0;
   475	        for &child_id in root_children {
   476	            let child = &self.ast.nodes[child_id.0];
   477	            match &child.kind {
   478	                AstNodeKind::Element(el) => {
   479	                    // v-else-if and v-else don't count as separate roots
   480	                    if let Some(ref cond) = el.v_condition {
   481	                        if matches!(
   482	                            cond.kind,
   483	                            ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
   484	                        ) {
   485	                            continue;
   486	                        }
   487	                    }
   488	                    // v-for produces 0..N elements — treat as multi-root so
   489	                    // _attrs is NOT applied to each iteration element.
   490	                    if el.v_for.is_some() {
   491	                        count += 2;
   492	                    } else {
   493	                        count += 1;
   494	                    }
   495	                }
   496	                AstNodeKind::Interpolation(_) => count += 1,
   497	                // Comments don't count as effective roots — a template with
   498	                // `<!-- comment --><div>...</div>` is single-root in Vue SSR.
   499	                AstNodeKind::Comment(_) => {}
   500	                AstNodeKind::Text(t) => {
   501	                    let text = &source[t.start as usize..t.end as usize];
   502	                    if !text.trim().is_empty() {
   503	                        count += 1;
   504	                    }
   505	                }
   506	            }
   507	        }
   508	        count
   509	    }
   510	
   511	    // ── VDOM fallback for slot else branches ─────────────────────
   512	
   513	    /// Generate VDOM VNode array for a slot's else branch.
   514	    ///
   515	    /// In Vue's SSR output, slot callbacks have both an SSR `if (_push)` branch
   516	    /// and a VDOM `else { return [...] }` fallback. This method generates the
   517	    /// VNode array contents for the else branch.
   518	    fn generate_vdom_fallback(
   519	        &self,
   520	        el: &ElementNode,
   521	        source: &str,
   522	        out: &mut CodeGenOutput<'alloc>,
   523	    ) -> String {
   524	        let children = match el.content {
   525	            Some(ref content) => &content.children[..],
   526	            None => return "[]".to_string(),
   527	        };
   528	        self.generate_vdom_children(children, source, out)
   529	    }
   530	
   680	    assert!(
   681	        tpl.ssr_imports.contains(&"_ssrInterpolate"),
   682	        "should import _ssrInterpolate, got: {:?}",
   683	        tpl.ssr_imports
   684	    );
   685	}
   686	
   687	#[test]
   688	fn ssr_vue_imports_separate_from_ssr_imports() {
   689	    let result = compile_sfc_ssr(r#"<template><div class="hello">{{ msg }}</div></template>"#);
   690	    let tpl = result.template.as_ref().expect("should have template");
   691	    // _mergeProps comes from "vue", not "vue/server-renderer"
   692	    assert!(
   693	        tpl.imports.contains(&"_mergeProps"),
   694	        "vue imports should include _mergeProps, got: {:?}",
   695	        tpl.imports
   696	    );
   697	    // SSR imports are separate
   698	    assert!(
   699	        !tpl.imports.contains(&"_ssrRenderAttrs"),
   700	        "_ssrRenderAttrs should be in ssr_imports, not vue imports, got: {:?}",
   701	        tpl.imports
   702	    );
   703	}
   704	
   705	// ══════════════════════════════════════════════════════════════════
   706	// Comments
   707	// ══════════════════════════════════════════════════════════════════
   708	
   709	#[test]
   710	fn ssr_comment_preserved_in_dev() {
   711	    let code = gen_ssr_template("<template><!-- comment --><div>hello</div></template>");
   712	    assert!(
   713	        code.contains("<!-- comment -->"),
   714	        "comments should be preserved in dev mode, got:\n{}",
   715	        code
   716	    );
   717	}
   718	
   719	// ══════════════════════════════════════════════════════════════════
   720	// Push buffering — component resolveComponent ordering
   721	// ══════════════════════════════════════════════════════════════════
   722	
   723	/// Vue hoists `_resolveComponent()` calls before any `_push()`. When a
   724	/// component appears as a child of a normal element, the resolve must
   725	/// appear BEFORE the parent's `_push()`, not after.
   726	///
   727	/// Vue output pattern:
   728	/// ```js
   729	/// const _component_MyComp = _resolveComponent("MyComp")
   730	/// _push(`<div${_ssrRenderAttrs(_attrs)}>`)
   731	/// _push(_ssrRenderComponent(_component_MyComp, ...))
   732	/// _push(`<p>after</p></div>`)
   733	/// ```
   734	#[test]
   735	fn ssr_component_resolve_before_push() {
  4620	/// @ai-generated — Dynamic :ref should be skipped in SSR output.
  4621	#[test]
  4622	fn ssr_dynamic_ref_skipped() {
  4623	    let code = gen_ssr_template(
  4624	        r#"<template><ul><li v-for="(item, i) in items" :ref="el => setRef(el, i)">{{ item }}</li></ul></template>
  4625	<script setup>
  4626	const items = ['a', 'b']
  4627	function setRef(el, i) {}
  4628	</script>"#,
  4629	    );
  4630	    // :ref should NOT produce _ssrRenderAttr
  4631	    assert!(
  4632	        !code.contains("_ssrRenderAttr(\"ref\""),
  4633	        ":ref should not produce _ssrRenderAttr in SSR, got:\n{}",
  4634	        code
  4635	    );
  4636	    assert!(
  4637	        !code.contains("setRef"),
  4638	        "ref callback should not appear in SSR output, got:\n{}",
  4639	        code
  4640	    );
  4641	}
  4642	
  4643	/// @ai-generated — Root-level comment before element should produce valid push and fragment markers.
  4644	#[test]
  4645	fn ssr_root_comment_before_element() {
  4646	    let code = gen_ssr_template(
  4647	        r#"<template>
  4648	  <!--before div-->
  4649	  <div>
  4650	    <!--after div-->
  4651	    foo
  4652	  </div>
  4653	</template>"#,
  4654	    );
  4655	    // Should have fragment markers for multi-root (comment counts for hydration)
  4656	    assert!(
  4657	        code.contains("<!--[-->"),
  4658	        "should have fragment open marker, got:\n{}",
  4659	        code
  4660	    );
  4661	    assert!(
  4662	        code.contains("<!--]-->"),
  4663	        "should have fragment close marker, got:\n{}",
  4664	        code
  4665	    );
  4666	    // Should have the comment inside the push
  4667	    assert!(
  4668	        code.contains("<!--before div-->"),
  4669	        "should include the comment, got:\n{}",
  4670	        code
  4671	    );
  4672	    // Should still apply _attrs to the root div
  4673	    assert!(
  4674	        code.contains("_ssrRenderAttrs(_attrs)"),
  4675	        "should apply _attrs to root div, got:\n{}",

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_compiler/src/code_transform/segmented_tests.rs | sed -n '220,355p' && nl -ba crates/verter_compiler/src/code_transform/source_map_tests.rs | sed -n '1,125p' && nl -ba CLAUDE.md | sed -n '140,255p;509,565p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 55ms:[0m
     1	use super::*;
     2	use crate::code_transform::CodeTransformError;
     3	use crate::template::code_gen::types::SegmentedOverwriteAuthority;
     4	use oxc_allocator::Allocator;
     5	
     6	#[test]
     7	fn explicit_sourcemap_location_emits_an_exact_utf16_boundary_token() {
     8	    let allocator = Allocator::default();
     9	    let source = "\u{3b1} color: red";
    10	    let mut transform = CodeTransform::new(source, &allocator);
    11	    let color_offset = source.find("color").expect("fixture contains property") as u32;
    12	
    13	    transform
    14	        .try_add_sourcemap_location(color_offset)
    15	        .expect("authored boundary is valid");
    16	    let map = transform.generate_map(SourceMapOptions::new().with_source("App.svelte"));
    17	
    18	    let token = map
    19	        .get_tokens()
    20	        .find(|token| token.get_dst_line() == 0 && token.get_dst_col() == 2)
    21	        .expect("explicit property boundary must produce a generated token");
    22	    assert_eq!((token.get_src_line(), token.get_src_col()), (0, 2));
    23	    assert!(token.get_source_id().is_some());
    24	}
    25	
    26	#[test]
    27	fn explicit_sourcemap_location_rejects_mid_codepoint_offsets_without_mutation() {
    28	    let allocator = Allocator::default();
    29	    let mut transform = CodeTransform::new("a\u{e9}", &allocator);
    30	
    31	    let Err(error) = transform.try_add_sourcemap_location(2) else {
    32	        panic!("byte 2 must be rejected inside the two-byte e-acute codepoint");
    33	    };
    34	
    35	    assert_eq!(error, CodeTransformError::MidChar { offset: 2 });
    36	    assert!(transform.sourcemap_locations().is_empty());
    37	}
    38	
    39	/// Byte-identical regression guard: the resolver-reuse and reserved-token-vector
    40	/// changes are allocation-only and must not alter emitted source-map bytes.
    41	/// The expected string is the JSON produced for this representative case
    42	/// (overwrite + a mapped insertion across a multi-line source).
    43	/// Legacy byte-equivalence proof for `SegmentedOverwriteAuthority`'s
    44	/// introduction: this fixture exercises ONLY the pre-existing, non-opt-in
    45	/// operations (`overwrite`, `batch_prepend_left_with_source_map`) — no
    46	/// `try_overwrite_segmented` call anywhere — and pins both `content` and the
    47	/// serialized map to an exact byte string. This TEST FUNCTION's own body
    48	/// (the fixture and the pinned strings below) is unchanged, byte for byte,
    49	/// from before the segmented-overwrite primitive existed to today — only
    50	/// this doc comment and this file's OTHER contents (imports, sibling
    51	/// tests) have moved. The pinned strings themselves have therefore been
    52	/// asserted unchanged both BEFORE and AFTER that primitive's introduction
    53	/// — durable evidence, not a "the suite stays green" claim. See
    54	/// [`segmented_overwrite_channel_does_not_perturb_sibling_overwrite_bytes`]
    55	/// for the complementary mixed-channel case (a document using BOTH the
    56	/// legacy and the new opt-in channel together).
    57	#[test]
    58	fn source_map_json_is_byte_identical_for_representative_case() {
    59	    let allocator = Allocator::default();
    60	    let source = "const x = 1;\nconst y = 2;\nconst z = 3;";
    61	    let mut ct = CodeTransform::new(source, &allocator);
    62	    ct.overwrite(6, 7, "foo");
    63	    ct.batch_prepend_left_with_source_map(&[(13, Some((6, 0)), "(mapped) ")]);
    64	    let json = ct.generate_map_json(
    65	        SourceMapOptions::new()
    66	            .with_source("golden.ts")
    67	            .with_file("golden.ts.map"),
    68	    );
    69	    assert_eq!(
    70	        json,
    71	        "{\"version\":3,\"file\":\"golden.ts.map\",\"names\":[],\"sources\":[\"golden.ts\"],\"sourcesContent\":[\"const x = 1;\\nconst y = 2;\\nconst z = 3;\"],\"mappings\":\"AAAA,MAAM,GAAC;AAAD,SACN;AACA\"}"
    72	    );
    73	}
    74	
    75	/// Companion to the proof above: a document using the legacy `overwrite`
    76	/// channel for one range and the new opt-in `try_overwrite_segmented`
    77	/// channel for a DISJOINT range must produce byte-identical `content` and
    78	/// map output for the legacy range as an equivalent document that never
    79	/// touches the segmented channel at all — i.e. adding the new channel does
    80	/// not perturb the pre-existing one it sits alongside. Both variants below
    81	/// are asserted against the SAME exact pinned string.
    82	#[test]
    83	fn segmented_overwrite_channel_does_not_perturb_sibling_overwrite_bytes() {
    84	    let source = "const x = 1;\nconst y = 2;\nconst z = 3;";
    85	    const EXPECTED: &str = "const XXX = 1;\nconst y = 2;\nconst z = 3;";
    86	
    87	    // Variant A: only the legacy channel touches the source at all.
    88	    let allocator_a = Allocator::default();
    89	    let mut ct_a = CodeTransform::new(source, &allocator_a);
    90	    ct_a.overwrite(6, 7, "XXX");
    91	    assert_eq!(ct_a.build_string(), EXPECTED);
    92	    let json_a = ct_a.generate_map_json(SourceMapOptions::new().with_source("a.ts"));
    93	
    94	    // Variant B: the SAME legacy overwrite, plus an unrelated, disjoint
    95	    // segmented-overwrite entry elsewhere in the same document.
    96	    let allocator_b = Allocator::default();
    97	    let mut ct_b = CodeTransform::new(source, &allocator_b);
    98	    ct_b.overwrite(6, 7, "XXX");
    99	    ct_b.try_overwrite_segmented(
   100	        32,
   101	        33,
   102	        "9",
   103	        &[],
   104	        SegmentedOverwriteAuthority::new_for_test(),
   105	    )
   106	    .expect("disjoint segmented overwrite with no anchors must succeed");
   107	    let content_b = ct_b.build_string();
   108	    assert_eq!(
   109	        content_b, "const XXX = 1;\nconst y = 2;\nconst 9 = 3;",
   110	        "the legacy overwrite's own bytes must be unaffected by an unrelated segmented entry"
   111	    );
   112	    // Re-derive variant A's expected bytes as a substring check: everything
   113	    // outside the segmented entry's range is byte-identical to variant A.
   114	    assert_eq!(&content_b[..14], &EXPECTED[..14]);
   115	    let json_b = ct_b.generate_map_json(SourceMapOptions::new().with_source("a.ts"));
   116	
   117	    // The legacy overwrite's own mapping token set is unchanged by the
   118	    // sibling segmented entry: both maps carry the same token at the
   119	    // legacy overwrite's generated position.
   120	    let token_a = json_a.split("\"mappings\":\"").nth(1).unwrap();
   121	    let token_b = json_b.split("\"mappings\":\"").nth(1).unwrap();
   122	    assert_eq!(
   123	        token_a.split(';').next(),
   124	        token_b.split(';').next(),
   125	        "the first mapping segment (the legacy overwrite's own line) must be identical \
   140	### Declaration Augmentation (CRITICAL)
   141	
   142	Ambient declaration augmentation (`declare module "X" { ... }` / `declare global { ... }`) is a RETAINED, addressable scoped inventory — never fingerprint-only facts, never file-scope pollution. `EvalEnv.augmentation_scopes` / `EvalEnv.augmentation_value_scopes` key `(AugmentationScopeKind {Global, Module(specifier)}, name)` → ordered `TypeDeclGroup`/`ValueDeclGroup`, mirrored on `ShallowFileState`; inner decls NEVER enter file-scope `type_symbols`/`value_symbols`. Parse-domain `ModuleAugmentationFact`s are DERIVED from this typed inventory (`fact_emission::collect_augmentations`) — NO raw-source byte-scan.
   143	
   144	Cross-file augmentation merge is the SAME `MergedDecl` peer-merge path as same-file merging — NOT a second merge engine: `stitch_module_augmentations` finds every augmenter via `FileArtifactStore::ensure_augmentation_index_populated`, lowers each augmenter's RETAINED inner body in its own file context, and folds base ∪ augmenter contributions into ONE `SemanticNodeData::MergedDecl` carrier; augmenter order is the stable `(canonical, parse_stable_hash)` key — discovery-order-independent.
   145	
   146	Facts rail: the cold stitch observes one `FactKey::ModuleAugmentationIndexShape` fingerprint plus one `FileWholeHash` per contributing file and records `self_root_canonicals = {base} ∪ {augmenters}` — a content edit to ANY contributor misses the warm read; torn/partial routes through `ReturnOnly`. Query keys stay content-free (R6). The index is OVERLAY-AWARE (`AugmentationPopulation {Base, Session(overlay-set fingerprint)}`): overlay augmenters NEVER poison the base index and NEVER cross sessions, and there is NO base-only session assert on the augmentation-index surface — a session view is accepted under `Session` scope.
   147	
   148	Guards: `session_overlay_augmenter_isolated_from_base_index`, `session_overlay_augmentation_isolated_from_base_meta`.
   149	
   150	See `/type-resolution` skill for the stitch chain and the overlay-aware index, and `/type-cache-architecture` for the content-addressed vs query-identity augmentation key split.
   151	
   152	### Two Template Codegen Paths (CRITICAL)
   153	
   154	The Rust compiler has two separate template codegen paths; modifying one does NOT affect the other: **VDOM/Vapor** (`template/code_gen/vdom/`) for runtime render functions, and **IDE** (`ide/template/`) for valid JSX/TSX used by LSP/TSGO type checking. The LSP uses the IDE path via `CompileTarget::IDE`.
   155	
   156	Guards: `compile_audit_sourcemap`.
   157	
   158	See `/compiler-codegen` skill for full codegen pipeline, backends, and CompileTarget details.
   159	
   160	### Carrier IDE TS Surface Principle
   161	
   162	North star for the IDE/LSP experience: for every carrier with an IDE projection (`.vue`, `.svelte`), the script block (`<script>`, `<script setup>`, Svelte module/instance scripts) AND the supported template/markup expressions are **ONE** generated TypeScript/JavaScript/JSX surface — interpolations (`{{ }}`, Svelte `{expr}`), directive/attribute expression values (`v-if`/`v-for`/`v-bind`/`:`/`v-on`/`@`/`v-model`/`v-slot` and dynamic args; Svelte `bind:`/`on:`/`class:`/`style:`/`use:`, `{#if}`/`{#each}`/`{#await}`/snippets, `{@render}`/`{@html}`/`{@const}`, rune calls) all lower into it. That surface is obtained through the IDE path (`CompileTarget::IDE`/`TSX`), synced to the active TypeProvider, with provider positions/ranges/edits mapped back through the document's `ProviderPositionMapper`.
   163	
   164	**The bar:** for any supported mapped TS/JS expression position, every provider-backed IDE feature — diagnostics, hover, definition/type-definition, references, rename, completion/resolve, signature help, document highlights, semantic tokens, inlay hints, and generic code actions whose edits map exactly — should behave like the equivalent standalone `.ts`/`.js`/`.jsx` program, with results mapped back to the carrier source. A binding represented in BOTH script and template is discoverable and renamable from either side (rename spans script + template; find-all-references finds both). This holds for **both Vue and Svelte** over the shared LSP path.
   165	
   166	Fail-closed boundary: unmapped synthetic helper code, framework tokens with no TS correlate, unsupported/experimental projection regions, and provider edits whose full ranges cannot be mapped must fail closed or return framework-native results — never mis-mapped. Source actions (organize-imports, fix-all, formatting) require explicit per-action support and tests; they are NOT implied by this principle. This is a **principle, not yet a `(CRITICAL)` guarded rule** — it is promoted to CRITICAL once real-provider cross-region Vue/Svelte regression tests guard it.
   167	
   168	See `/compiler-codegen` → "Carrier IDE TS Surface Principle" for the full normative text (every covered expression form), and `/host-session` / `/position-encoding` for provider sync and position/range/edit mapping.
   169	
   170	### Compiled-Output Conformance (CRITICAL)
   171	
   172	Official-framework compiler conformance is behavioral plus structural/helper-topology parity, not raw-byte identity. For Vue VDOM/Vapor, Svelte `svelte/internal/*`, SSR/client, and future runtime backends, compare emitted output by observable behavior plus parsed/token-normalized structure: imports, helper families, helper call sequence where order is semantic, memoization/reactivity/effect topology, DOM/hydration template topology, class/style/attribute normalization, prop/property routing, event delegation, and diagnostic/reject ordering.
   173	
   174	Cosmetic JS carrier formatting is not a finding: indentation, line breaks, non-semantic comments, intra-expression whitespace outside literals, and behavior-preserving redundant parentheses may differ from the official compiler. Directive, pragma, license/preserve, source-map/sourceURL, TS-directive, JSDoc, and other tool-consumed or framework-significant comments remain in contract. Generated local identifier spellings are waived only when the backend oracle implements scope-aware alpha-equivalence for private, non-observable bindings; otherwise identifiers are structural. Literal payload bytes, static HTML/CSS/SSR strings, public/exported or source-authored names, sourcemap mappings, diagnostic text/codes/order, and any framework-defined observable format remain in contract.
   175	
   176	Do not build or route production compiled-output emission through JS printers, re-printers, redundant-paren canonicalizers, or any machinery whose role includes mimicking the official compiler's cosmetic JS carrier formatting. Direct-emission helpers may emit syntax-required tokens, including required parentheses for valid JavaScript expression/statement shape, but they must be scoped to semantic/syntactic correctness and covered by behavioral/structural tests rather than official cosmetic byte parity. Emit correct code directly and make conformance oracles structural for cosmetic categories: a cosmetic-only diff passes; a behavioral or structural divergence fails.
   177	
   178	The positive structural-discriminator guard currently covers Svelte client only (Vue VDOM/Vapor and SSR/client positive oracles are tracked follow-ups); the re-printer guard is cross-backend negative coverage. See `/compiler-codegen` for the tracked guard gap.
   179	
   180	Guards: `svelte_structural_conformance_discriminates_cosmetic_from_behavioral_diffs`, `no_compiled_output_cosmetic_reprinter_path`.
   181	
   182	### Fallthrough / Root Inheritance (CRITICAL)
   183	
   184	The shared Rust pipeline owns all fallthrough and root inheritance semantics. `verter_semantic::analysis` extracts root reachability facts only. `verter_session` owns the single inheritance resolver, recursion, conditional branch composition, generic propagation, caching, and final metadata projection.
   185	
   186	Key rules: `inheritAttrs: false` → no inherited surface. Single native root → intrinsic attrs minus declared props/events. Single component root → recursive propagation. Conditional branches → exact union. Cycles → unresolved branches. `class`/`style` are never consumed.
   187	
   188	Guards: `fallthrough_recomputes_from_runtime_subnodes_after_top_level_node_clear`, `fallthrough_runtime_reuse_survives_host_cache_clear`, `fallthrough_reuses_root_follow_after_branch_union_node_clear`.
   189	
   190	See `/component-meta` skill for the full semantic rules, public contract, authority chain, and key files.
   191	
   192	### Component-Meta Shallow-By-Default Rule (CRITICAL)
   193	
   194	Types and properties are ALWAYS published shallow at the projector surface UNLESS the consumer explicitly walks the path. This is the single architectural invariant the projector pipeline (`meta_resolve::projectors::reduce_published_field_types` + sink-private `reduce_field_value_node`) enforces.
   195	
   196	Concrete contract:
   197	
   198	- Plain alias references (`type Foo = ...`) — published prop type stays `TypeExpr::Ref { name: "Foo" }`. Consumers re-resolve `Foo` through the registry on demand. The projector does NOT eagerly inline the alias body.
   199	- `Pick<Foo, "bar">` — materialises ONLY the `bar` member of Foo. Other Foo properties stay shallow (path-precise). Built-in utility types (`Pick`, `Omit`, `Required`, `Partial`) behave identically to a userland implementation referencing the same keys.
   200	- **Carrier-preserving decl-body lowering.** Under `Shallow` (as under `Navigate` / `Skeleton`), decl-body lowering interns `DeclRef` / `InstantiationRef` carriers for member-value type references — including ALL builtin utilities — and never executes `ResolveDecl` / `Instantiate` eagerly; eager lowering-time execution is `Expanded` / `Identity` only; materialisation enters exclusively through the demand points (PathWalker hops, the shallow-surface synthesiser's carrier unwrap, closed object-filter surface reads, the relation/conditional oracle). Eager Shallow member-value lowering was the `Table.vue` storm: 94.3% of all budget charges were `Instantiate(StructuralTransit:Shallow)` recursion across the transitive TanStack decl graph.
   201	- **Open key domain ⇒ shallow carrier (L1) — route/mode-independent.** TWO families stay shallow carriers at EVERY entrance, in every mode, and open-OR-UNKNOWN (including traversal-budget exhaustion) preserves the carrier instead of falling through into Expanded materialisation: (1) an object-filter utility (`Pick`/`Omit`) whose enumeration domain is OPEN or undecidable (`Pick<PropsBase<T>, …>` over the SFC's open `generic="T"` stays `Pick<…>`); (2) a mapped type `{ [K in S]: V }` whose produced surface still depends on an unbound OUTER generic (a CLOSED-key/open-VALUE mapped enumerates its keys path-precisely with shallow values). Closed sources still materialise the requested keys path-precisely. A carrier-stopped `Pick` at a SURFACE-enumeration demand (heritage arm / macro props-slots surface) still publishes its CLOSED output-key selection from the source's enumerable arms via the shallow walker's `Pick`-carrier enumeration — the source is never whole-materialised and `Omit` (source-dependent-open output keys) stays a carrier; zero-member surface collapse was the nuxt-ui ContentSearch/DropdownMenuContent bug. Typed-IR only, no string matching. The carrier-stop is the PRIMARY defense for the open-generic class; the per-request projection budget (`request_budget.rs`) is an ARMED-by-default runaway fuse (`projection_op_budget == 0` ⇒ effective cap 2000) whose trip returns `BudgetExceeded` as a genuine partial — refused warm admission, the no-poison invariant. Publication demand is `Navigate`-only on the projector/registry macro surfaces: a full `get_component_meta` records ZERO `Published(Expanded)` projection contexts; `Table.vue` and `ChatMessages.vue` are COMPLETE corpus members with un-ignored green trackers (`table_resolves_complete_and_warm`, `chat_messages_resolves_complete_without_false_partial`, `chat_messages_resolves_without_timeout`). The FULL authoritative spec — entrances, owner predicates, the per-argument position-sensitive key-domain rule, the tri-state conditional oracle, per-utility output-key semantics, mapped family composition, OPEN/CLOSED definitions, memoization, invalidation, the `TypeOf` demand rails, and the four named current scoped exceptions — lives in `/type-resolution` → Open-Key-Domain Carrier-Stop (L1).
   202	- `Omit<Foo, "bar">` — keeps `bar` shallow (excluded from the surface) and materialises the others.
   203	- `Foo['a']['b']` — path-precise: only the `a` and `b` hops load; other Foo keys never enter the published surface.
   204	- True recursive types (`type Self = Pick<Self>`) — NOT supported. The published surface stays the bare `Ref { name: "Self" }`.
   205	- Imported alias names (workspace-owned OR package-backed) — stay shallow regardless of where they live.
   206	
   207	The projector pipeline is the sole post-projection authority — no eager per-field materialisation runs at publication time.
   208	
   209	Guards: `decl_body_lowering_keeps_member_value_refs_as_carriers`, `publication_routes_never_demand_expanded`, `chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier`, `closed_pick_sources_still_materialize_path_precisely`, `projection_budget_counts_instantiate_and_conditional`, `cycle_guard_roots_at_utility_source_type_argument` — full list in `CRITICAL_RULE_GUARDS`.
   210	
   211	See `/component-meta` skill for the publication-surface rules and the locked-down negative tests in `crates/verter_session/src/meta_tests.rs`, and `/type-resolution` for the authoritative L1 spec.
   212	
   213	### Component-Meta Native Vs Compat (CRITICAL)
   214	
   215	The native component-meta payload is the semantic authority. `@verter/component-meta/compat` is a projection layer for `vue-component-meta` interoperability, not a second semantic pipeline.
   216	
   217	Core rules: Fix metadata in the native layer first. Rust owns resolution, declaration routing, graph construction. One async native request per query. JS may transform structure but must not recover meaning. JS must not become a second resolver or expander. Cache-owned type recovery only — no AST/source fallbacks.
   218	
   219	Guards: `no_napi_direct_verter_compiler_emitters`, `compat_one_napi_call_audit`.
   220	
   221	See `/component-meta` skill for the full policy, resolver rules, and cache contracts.
   222	
   223	### Typed-IR-Only Resolver Rule (CRITICAL)
   224	
   225	The native component-meta / typeinfo type resolver — analyzer → projector → registry → policy → materialiser — drives semantic decisions exclusively from the typed IR (`verter_semantic::analysis::type_expr::TypeExpr` on Rust, `TypeDescriptor` from `@verter/type-ir` on TS). Forbidden inside that pipeline:
   226	
   227	- Source slicing, regex against type text, hand-rolled type-text splitters (`split_top_level_*`, `find_top_level_char`, `extract_pick_slot_bindings`, `extract_string_literal_name`, `splitTopLevelTypeOperator`), `starts_with("Pick<")` shape sniffing, and the synthesise-then-reparse pattern (`format!(...).parse_type_annotation(...)`). Walk the typed IR instead.
   228	- `parse_type_annotation` anywhere except JSDoc tag-type payloads — the single explicit text exception: `{Type}` payloads inside JSDoc tags are inherently text, parsed via the dedicated JSDoc path only.
   229	- Parsing back raw / display strings (`Analyzed*Field.type_annotation`, `ExpandedField.raw_type`, `ResolvedLocalType.expanded`, `PropMeta.rawType`) — display-only passthroughs. The JS compat layer (`@verter/component-meta/compat`) reads `prop.type` (`TypeDescriptor`) for every semantic decision; `prop.rawType` must not feed any `looksLike*`, `extract*`, `normalize*`, `split*`, `strip*`, `prefer*`, `shouldPrefer*`, or `repairOpaque*` branch.
   230	- Substring path classification (`"/node_modules/"`, `"\\node_modules\\"`) — use `ResolverContext::workspace_is_package_backed(canonical_id)`. That predicate is the single structural authority for workspace-ownership classification, and it is what the live decision sites call directly (`component_meta_materialize.rs`, `framework/script_facts.rs`, `host_manage/jsdoc_resolve.rs`, `meta_resolve/graph_predicates.rs`, `meta_resolve/materialize/field_types.rs`, `meta_resolve/projectors/output_sink.rs`, `project_semantic_dispatch/raise.rs`/`walk.rs`, and others). Workspace-owned is its complement — there is no separate `workspace_is_workspace_owned` predicate.
   231	- Name-suffix role inference (`name.ends_with("Props")` / `"Emits"` / `"Events"` / `"Model"` / `"Slots"`). Type-role classification is structural, not nominal: a type is a prop/emit/model/slot type because a Vue SFC macro (`defineProps`, `defineEmits`, `defineModel`, `defineSlots`, `withDefaults`) consumes it — read from `AnalyzedMacro.kind` / `parsed_type_argument` / `type_references` on the analyzer snapshot.
   232	
   233	OXC is a syntax/lowering front-end only and never resolves types at query time. Macro/JSDoc producer fields still lower at their producer boundary via `lower_ts_type(ts_type, source)` (the producers emit content-free locators — `Analyzed*Field` carries `payload: Option<MacroPayloadLocator>`, `ResolvedLocalType` carries `shape: ResolvedLocalShape`; only display-only text rides alongside, surviving all caches); top-level declaration bodies lower LAZILY through the scheduler-retained parse snapshot (`DeclBodyMemo` → `DeclLoweringService`) and return owned typed IR before dispatch/reducers ever see them — no raw-string reparsing, no OXC resolver path. For the hot read surface the `decl_body_hot_ref` accessor mints a `HotTypeRef` handle over the `Instantiate` query result (`build_instantiate`'s post-processed node, produced via the resolving-lowerer body-shape helper `lower_decl_body_with_provenance`); the handle is NOT a re-lowering — bodies still lower to typed IR, and `DeclBodyMemo` records are fact+locator content-free end to end (the former memoized `LoweredTypeDecl.type_parameters: Vec<TypeParam>` pocket is CLOSED — the narrow mirror is the stored authority; bound content re-borrows lease-only). If a new requirement appears to need text manipulation inside the resolver, fix the producer (lower the right OXC node, store the right typed field, extend `@verter/type-ir` with a missing variant) rather than reparsing or pattern-matching on text.
   234	
   235	Guards: `no_macro_string_heuristics_in_resolver_core`, `no_format_then_reparse`, `no_role_inference_from_name_suffix`, `no_node_modules_substring_outside_workspace_api`, `no_pick_or_omit_string_prefix_check`, `lazy_decl_lowering_uses_scheduler_snapshot_not_reparse`, plus the rest of the typed-IR guard cluster — full list in `CRITICAL_RULE_GUARDS`.
   236	
   237	See `/component-meta` and `/type-resolution` skills for the typed schema contract, the producer-side lowering points, and the architecture-guard list.
   238	
   239	### CodeTransform Is the Single Source of Truth (CRITICAL)
   240	
   241	**All modifications to generated code MUST go through `CodeTransform` operations** (`overwrite`, `prepend_left`, `append_left`, `move_with_suffix`, etc.) — never string replacements, regex transforms, or manual splicing on the output of `build_string()` or content produced by a `CodeTransform`. `CodeTransform` generates source maps by tracking chunks (Original, Inserted, Moved, Overwritten); modifying the string after the transform desyncs byte offsets → LSP position mismatches (hover landing on the wrong token, go-to-definition jumping to wrong locations).
   242	
   243	**Correct:** `ct.prepend_left(pos, ".ts")` — chunk list and source map stay consistent. **Wrong:** `content.replace(".vue'", ".vue.ts'")` on the built string — the source map still reflects pre-replace byte offsets.
   244	
   245	Guards: `compile_audit_sourcemap`.
   246	
   247	### Typeinfo Wire Contract (CRITICAL)
   248	
   249	The typeinfo graph wire surface (`crates/verter_protocol/proto/verter/v1/typeinfo.proto`, its generated Rust and TS bindings, and the audit envelope on top) is a closed contract. Four invariants:
   250	
   251	1. **Closed-enum discipline.** `GraphTypeNode.kind`, `StructuredTypeExpression.kind`, `TypeInfoGraphRequest.payload`, `TypeInfoRequestError.kind` are closed `oneof` taxonomies. Adding a variant bumps `SemanticTypeGraph.schema_version`; removing one requires `reserved` directives at the enclosing message scope (proto3 forbids `reserved` inside an `oneof` block).
   252	2. **Wire-compat: field numbers never reused.** A retired variant's tag goes into the message's `reserved` list with its name (off-tree clients keep round-tripping the slot as an unknown field); new variants take the next free tag, never a recycled one.
   253	3. **Audit envelope additions are purely additive.** Every new typeinfo audit field (`structured_event`, `kind_payload`, `RequestKind::TypeInfoGraph`) lands as a new arm or a default-zero field, never a replacement.
   254	4. **Request validation runs before semantic execution.** `validate_type_info_graph_request` rejects malformed envelopes through a typed `TypeInfoRequestError`; the schema-version gate is closed-set (`SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS`); per-variant structured-expression validation is exhaustive over the `oneof` taxonomy.
   255	
   509	## Agent Implementation Rules
   510	
   511	### Codebase Navigation
   512	
   513	Use semantic code-navigation tools (Serena or equivalent MCP: symbol overviews, symbol/reference lookup, rename/refactor ops) before broad source reads. Read full source files only when symbolic context is insufficient or the file is small enough that a full read is clearly the most direct path.
   514	
   515	### Planning
   516	
   517	Prefer architecturally correct, long-term solutions; evaluate by correctness and durability, not implementation speed. Time constraints, implementation size, migration breadth, anticipated breaking changes, or "a lot of work" are not valid reasons to weaken the design, preserve a compromised path, or diverge from the approved plan — if the correct implementation is larger or breaking, plan for it explicitly or raise it before execution; never silently ship an architectural deviation. Do not provide time estimates unless explicitly asked, and never use estimated effort/duration/perceived time cost as a factor for doing, not doing, or partially doing planned work.
   518	
   519	Plans must include these sections:
   520	1. **Context** — why this change is being made
   521	2. **Intent Contract** — the ratified statement of intent, before any mechanism design
   522	3. **Changes** — specific files to modify with concrete modifications
   523	4. **Legacy Deletions** — explicit list of files, functions, code paths, feature flags to remove
   524	5. **Verification** — full workspace test commands and expected outcomes
   525	
   526	Without explicit legacy deletion lists, agents skip deletions and leave dual paths alive.
   527	
   528	**Intent before mechanism.** Before mechanism design for a block that changes observable behavior, authority, or fallback, record a ratified intent contract: the actor/problem and why the capability should exist; required and forbidden observable outcomes; authority/fallback order; a planned test or gate for each stable acceptance ID; and material cold, warm, allocation, fan-out, and latency bounds. An internal substrate block may reference its parent contract but must state the invariant and performance contribution it owns. Ratification comes from the approved plan or product authority; no implementation brief is dispatched without it. Enforcement is judgment — exercised at decomposition and again immediately before implementation dispatch.
   529	
   530	### Execution
   531	
   532	Execute approved plans fully in one pass, end-to-end, without intermediate checkpoints or mid-plan confirmation on already-approved steps. Do not pause, defer scope, leave planned work unfinished, or rewrite the plan into a smaller/safer variant because the correct path is breaking, broad, or labor-intensive. Approved plans land as written unless the user explicitly re-scopes them.
   533	
   534	**One-pass execution applies only while the approved design remains valid.** The second-REOPEN circuit breaker lapses approval for the affected design: pause implementation, obtain and record the required architecture/product ruling, and resume only once the design is ratified again. This is not a checkpoint — one-pass governs *executing an approved design*, and the breaker fires when *approval itself has lapsed*, which is a different event and precisely why execution must stop rather than grind on. STOP, failed verification, rule conflict, and verified plan-invalidating discoveries pause at their prescribed evidence gate without creating a discretionary user checkpoint. Breadth, breakage, effort, or migration size never lapses approval; approved scope changes only through the recorded ruling or explicit user re-scope. See `/mom-cto-orchestration` → Decision Admission.
   535	
   536	### Orchestrating Large Plans
   537	
   538	For a large multi-block plan, refactor, migration, or staged cutover executed autonomously, drive it via the `/multi-agent-orchestration` skill rather than improvising: a pure orchestrator delegates blocks to implementer/reviewer/fix sub-agents, gates each on dual review (independent reviewer + `codex`), runs fix cycles until clean, and verifies sub-agent reports against git state (trust but verify).
   539	
   540	When a block runs in a dedicated `git worktree`, run `pnpm install --frozen-lockfile` in the worktree root once at creation time, before any JS/TS test or workspace-importing Node script — fresh worktrees do not get the gitignored `node_modules/`, and a missing install makes JS/TS tests fail spuriously and read as a false regression. See the skill's "Worktree hygiene & environmental discipline" section.
   541	
   542	### Self-Review
   543	
   544	After completing a plan, review the full implementation before declaring done:
   545	- Verify all plan steps were executed
   546	- Check for missed edge cases or incomplete migrations
   547	- Run the full workspace test suite (see End-of-change Checks above)
   548	
   549	### Legacy Code Deletion
   550	
   551	When replacing a feature or refactoring a system, delete the superseded code in the same change. Do not add shims, double branches, compatibility wrappers, or feature flags to preserve old behavior alongside new. If unsure whether specific files or code paths should be preserved, ask the user explicitly rather than silently keeping them.
   552	
   553	### Fix Quality
   554	
   555	When encountering issues during implementation:
   556	- If the correct fix aligns with the architecture → implement it properly
   557	- Never apply a dirty fix that contradicts architectural rules just to make tests pass
   558	- If the proper fix is outside approved scope, do not apply a workaround and do not use a `TODO` as its disposition. Route the finding through the applicable scope authority and record `ADOPT-NOW`, `DEFER`, or `REJECT` before related work continues. A `TODO` may reference an approved debt row but never replaces it.
   559	
   560	**Explicit finding disposition.** Every scope-deviating correctness finding is dispositioned before related work continues as `ADOPT-NOW`, `DEFER`, or `REJECT`. `ADOPT-NOW` records the scope and acceptance-contract change. `DEFER` requires a codex-DEFER ruling and a debt row naming the durable owner block, the resolution gate no later than plan close, the acceptance ID/test, and the ruling reference. `REJECT` records evidence and rationale. A TODO, a feedback entry, or an ephemeral agent identity is not a disposition; plan close requires zero open deferrals. Enforcement is judgment — codex at the scope consult, and the plan-close zero-open-deferral check.
   561	
   562	### Stub Prevention (CRITICAL)
   563	
   564	Do not use empty test bodies, trivially-passing stubs, or "deferred to follow-up commit" placeholders to satisfy a named contract — a gate check, a characterization test, a plan invariant, a review obligation, a declared completion criterion. A stub that happens to pass is a gate-bypass, not a pass.
   565	

[35m[3mexec[0m[0m
[1m/bin/zsh -lc 'git blame -L 50,72 crates/verter_compiler/src/template/code_gen/types.rs && git blame -L 1060,1084 crates/verter_compiler/src/template/code_gen/vdom/mod.rs && git blame -L 1741,1766 crates/verter_compiler/src/template/code_gen/vdom/mod.rs && git log --oneline -- crates/verter_compiler/src/code_transform/segmented.rs crates/verter_compiler/src/template/code_gen/types.rs | head -n 30'[0m in <MACHINE_ROOT>/verter
[32m succeeded in 513ms:[0m
e6191e2801 crates/verter_compiler/src/template/code_gen/types.rs (Carlos Rodrigues 2026-08-07 08:21:13 +0100 50) #[derive(Clone)]
bf1ad04cf0 crates/verter_core/src/template/code_gen/types.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 51) pub struct CodeGenOutput<'alloc> {
bf1ad04cf0 crates/verter_core/src/template/code_gen/types.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 52)     /// Replace source ranges: (start, end, replacement).
bf1ad04cf0 crates/verter_core/src/template/code_gen/types.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 53)     /// Applied via `ct.batch_overwrite()` in sorted order.
bf1ad04cf0 crates/verter_core/src/template/code_gen/types.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 54)     pub overwrites: Vec<(u32, u32, &'alloc str)>,
bf1ad04cf0 crates/verter_core/src/template/code_gen/types.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 55) 
1c5e8e53fa crates/verter_compiler/src/template/code_gen/types.rs (pikax            2026-08-16 05:13:32 +0100 56)     /// Replace source ranges with wholly synthetic content that has no
1c5e8e53fa crates/verter_compiler/src/template/code_gen/types.rs (pikax            2026-08-16 05:13:32 +0100 57)     /// character-level correspondence to the replaced span: (start, end,
1c5e8e53fa crates/verter_compiler/src/template/code_gen/types.rs (pikax            2026-08-16 05:13:32 +0100 58)     /// replacement). Applied via `ct.overwrite_unmapped()` per entry — see
1c5e8e53fa crates/verter_compiler/src/template/code_gen/types.rs (pikax            2026-08-16 05:13:32 +0100 59)     /// [`overwrite_or_root_prefix`](Self::overwrite_or_root_prefix) for why
1c5e8e53fa crates/verter_compiler/src/template/code_gen/types.rs (pikax            2026-08-16 05:13:32 +0100 60)     /// this is a separate channel from `overwrites` rather than a flag on
1c5e8e53fa crates/verter_compiler/src/template/code_gen/types.rs (pikax            2026-08-16 05:13:32 +0100 61)     /// the same one.
1c5e8e53fa crates/verter_compiler/src/template/code_gen/types.rs (pikax            2026-08-16 05:13:32 +0100 62)     unmapped_overwrites: Vec<(u32, u32, &'alloc str)>,
1c5e8e53fa crates/verter_compiler/src/template/code_gen/types.rs (pikax            2026-08-16 05:13:32 +0100 63) 
2d7339ef4d crates/verter_compiler/src/template/code_gen/types.rs (pikax            2026-08-20 10:34:35 +0100 64)     /// Segmented overwrite: (start, end, replacement, anchors). Applied via
2d7339ef4d crates/verter_compiler/src/template/code_gen/types.rs (pikax            2026-08-20 10:34:35 +0100 65)     /// `ct.try_overwrite_segmented()`. Vue VDOM/Vapor/SSR only — interpolation
2d7339ef4d crates/verter_compiler/src/template/code_gen/types.rs (pikax            2026-08-20 10:34:35 +0100 66)     /// identifiers and static attribute names inside otherwise synthetic code.
1c5e8e53fa crates/verter_compiler/src/template/code_gen/types.rs (pikax            2026-08-16 05:13:32 +0100 67)     segmented_overwrites: Vec<(u32, u32, &'alloc str, &'alloc [SegmentAnchor])>,
1c5e8e53fa crates/verter_compiler/src/template/code_gen/types.rs (pikax            2026-08-16 05:13:32 +0100 68) 
bf1ad04cf0 crates/verter_core/src/template/code_gen/types.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 69)     /// Insert content before a position: (position, content).
bf1ad04cf0 crates/verter_core/src/template/code_gen/types.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 70)     /// Applied via `ct.batch_prepend_left_static()` in sorted order.
bf1ad04cf0 crates/verter_core/src/template/code_gen/types.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 71)     /// Used for binding prefixes (`_ctx.`, `$setup.`), suffixes (`.value`), separators.
bf1ad04cf0 crates/verter_core/src/template/code_gen/types.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 72)     pub prepends: Vec<(u32, &'alloc str)>,
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1060)                     // Single root — block root with _openBlock + _createElementBlock
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1061)                     out.add_vdom_import(VdomHelper::OpenBlock);
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1062)                     let mut prefix = String::with_capacity(full_prefix.len() + 24);
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1063)                     prefix.push_str(&full_prefix);
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1064)                     prefix.push_str("return (_openBlock(), ");
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1065)                     // `hoisted_preamble_anchors` apply at THEIR OWN recorded
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1066)                     // offsets only when `hoisted_preamble` was folded
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1067)                     // unshifted into `full_prefix`'s own start — exactly the
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1068)                     // `!is_inline` case (see `full_prefix`'s own
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1069)                     // construction above); the inline case emits
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1070)                     // `hoisted_preamble` through a SEPARATE module-scope
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1071)                     // prepend instead, so its anchors do not apply here.
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1072)                     if !self.options.is_inline && !hoisted_preamble_anchors.is_empty() {
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1073)                         out.overwrite_or_root_prefix_segmented(
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1074)                             tag_open.start,
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1075)                             child.start,
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1076)                             &prefix,
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1077)                             &hoisted_preamble_anchors,
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1078)                             SegmentedOverwriteAuthority::new(),
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1079)                         );
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1080)                     } else {
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1081)                         out.overwrite_or_root_prefix(tag_open.start, child.start, &prefix);
1c5e8e53fa crates/verter_compiler/src/template/code_gen/vdom/mod.rs (pikax            2026-08-16 05:13:32 +0100 1082)                     }
e6191e2801 crates/verter_compiler/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-08-07 08:21:13 +0100 1083)                     out.overwrite_or_root_suffix(close_start, close_end, ")\n}");
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs     (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1084)                 }
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1741)     fn visit_comment(
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1742)         &mut self,
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1743)         id: NodeId,
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1744)         comment_node: &CommentNode,
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1745)         source: &'alloc str,
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1746)         out: &mut CodeGenOutput<'alloc>,
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1747)     ) {
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1748)         helpers::debug_assert_slice_bounds(
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1749)             source,
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1750)             comment_node.start,
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1751)             comment_node.end,
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1752)             "visit_comment",
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1753)         );
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1754)         // Skip comments between v-if chain members (Vue discards these).
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1755)         // Emit removal overwrite directly — the parent's leave phase may not
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1756)         // include this comment in its child records (when options.comments=false,
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1757)         // build_child_records excludes comments, so strip_interstitial_condition_nodes
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1758)         // can't find them). At root level, gap-filling also doesn't cover these.
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1759)         if self.is_interstitial_condition_node(id) {
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1760)             out.overwrite(comment_node.start, comment_node.end, "");
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1761)             return;
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1762)         }
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1763)         // Apply comment overwrites (or removal if disabled).
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1764)         // Child classification is handled by build_child_records from the AST.
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1765)         let _ = comment::process_comment(comment_node, source, self.options.comments, out);
bf1ad04cf0 crates/verter_core/src/template/code_gen/vdom/mod.rs (Carlos Rodrigues 2026-03-16 10:35:29 +0100 1766)     }
71b7211b1 feat(core): close the Vue Vapor/VDOM compiler-output conformance backlog
ffcfd4a50 feat(core): fragment-based assembly, atomic publication, and typed all-or-nothing refusal for Vue main-module compilation
2d7339ef4 docs(*): tighten comments across the workspace
1c5e8e53f fix(core): correct Vue script, VDOM, Vapor, SSR, and source-map defects against the rc.3 oracle
e6191e280 refactor(css): replace lightning css with custom parser (#100)
80d54ce30 chore(release): integrate the tsc-performance work line
1a0f47ad1 refactor(core): consolidate the native semantic pipeline

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_compiler/src/compile_tests.rs | sed -n '6580,6675p;19060,19340p' && rg -n \"hoisted.*anchor|static class|root.*comment|comment.*root|source.?map.*class|segmented\" crates/verter_compiler/src/template/code_gen/vdom/tests.rs crates/verter_compiler/src/compile_tests.rs crates/verter_compiler/tests/cases | head -n 300"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 118ms:[0m
crates/verter_compiler/src/compile_tests.rs:1932:        "root v-if should have comment fallback\n{}",
crates/verter_compiler/src/compile_tests.rs:5428:        "Should include static class.\nOutput:\n{}",
crates/verter_compiler/src/compile_tests.rs:6681:    // Test 2: comment between v-if branches at template root level
crates/verter_compiler/src/compile_tests.rs:6711:        "HTML comment at root level:\n{}",
crates/verter_compiler/src/compile_tests.rs:13836:/// `_hoisted_N` constant, exactly like a static class/attrs object. Official
crates/verter_compiler/src/compile_tests.rs:14084:        "element with static class should use _cache wrapping\n--- code ---\n{}",
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:308:fn leave_template_multi_root_production_no_comment() {
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:561:/// F19 regression guard: two genuine element roots with a leading comment are a
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:563:/// over-triggered — it applies only when comments surround a SINGLE logical root.
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:565:fn multi_root_fragment_with_comment_stays_stable_not_dev_root() {
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:571:        "multi-root fragment with a comment must be plain STABLE_FRAGMENT (64).\n{code}"
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:581:/// a comment beside a SINGLE logical root (here a v-if/v-else chain, which is
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:583:/// (2112) so fallthrough / single-root filtering ignore the comment vnode.
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:585:fn dev_root_fragment_comment_plus_single_conditional_root() {
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:596:        "comment + single conditional root must be DEV_ROOT_FRAGMENT (2112).\n{code}"
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:600:/// F19 (ported from grok spec): a comment beside a SINGLE component root flags
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:604:fn dev_root_fragment_comment_plus_single_component_root() {
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:614:        "comment + single component root must be DEV_ROOT_FRAGMENT (2112).\n{code}"
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:618:        "the component beside a root comment must be a _createVNode fragment child.\n{code}"
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:623:        "component next to a root comment must not be a bare _createBlock.\n{code}"
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:659:/// a root comment stays a `_createVNode` fragment child (official Vue emits
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:665:fn root_comment_plus_component_uses_create_vnode_not_bare_create_block() {
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:677:        "comment + component root must use DEV_ROOT_FRAGMENT, got:\n{code}"
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:681:        "component next to root comment must use _createVNode (multi-root child), got:\n{code}"
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:688:        "component next to root comment must NOT use _createBlock (would lack openBlock), got:\n{code}"
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:890:  <!-- comment forces multi-root fragment -->
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:2536:    // Negative: the static class/style must not also be emitted as bare props.
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:2539:        "static class must be merged, not emitted bare, got:\n{code}"
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:2648:// ── condition prefix segmented mapping ──────────────────────────────
crates/verter_compiler/tests/cases/mod.rs:13:mod segmented_overwrite_compile_fail;
crates/verter_compiler/tests/cases/svelte_client_fail_matrix.rs:555:        // An empty template (no rendered root) — the comment-anchor root shape. A
crates/verter_compiler/tests/cases/compile-fail/segmented_overwrite_authority_unreachable.stderr:2:  --> tests/cases/compile-fail/segmented_overwrite_authority_unreachable.rs:29:26
crates/verter_compiler/tests/cases/compile-fail/segmented_overwrite_authority_unreachable.stderr:14:  --> tests/cases/compile-fail/segmented_overwrite_authority_unreachable.rs:29:82
crates/verter_compiler/tests/cases/pending_nav_request_compile_fail.rs:11://! Same structural limitation as `segmented_overwrite_compile_fail.rs`
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:768:    // then `import Beta`, both after `* as $`) + the multi-root `<!><!> ` comment-anchor
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:1120:    // static class + `class:` directive — the official lone-class fast path WITH the
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:1129:    // static class + `style:` directive — the directive synthesizes the empty `style`
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:1141:    // failed snippet — the hoisted `const failed = ($$anchor, error = $.noop, reset = $.noop)
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:1144:    // pending snippet — the hoisted `const pending = ($$anchor) => {…}` + `{ pending }`.
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:1557:    // A sole-root slot with fallback: the `$.comment()` anchor frame (no
crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs:7623:    // ESCAPING — newline in a single-quoted static class base: the pre-fix
crates/verter_compiler/tests/cases/segmented_overwrite_compile_fail.rs:7://! overwrite_segmented` now REQUIRES a `SegmentedOverwriteAuthority` value
crates/verter_compiler/tests/cases/segmented_overwrite_compile_fail.rs:38://! --test main segmented_overwrite_compile_fail`, that walk succeeds:
crates/verter_compiler/tests/cases/segmented_overwrite_compile_fail.rs:59:fn segmented_overwrite_authority_is_unreachable_outside_the_crate() {
crates/verter_compiler/tests/cases/segmented_overwrite_compile_fail.rs:61:    t.compile_fail("tests/cases/compile-fail/segmented_overwrite_authority_unreachable.rs");
crates/verter_compiler/tests/cases/compile-fail/segmented_overwrite_authority_unreachable.rs:1:// Negative control for the segmented-overwrite call-site guard.
crates/verter_compiler/tests/cases/compile-fail/segmented_overwrite_authority_unreachable.rs:15:// segmented_overwrite_compile_fail`, trybuild's probe DOES see `bench`
crates/verter_compiler/tests/cases/compile-fail/segmented_overwrite_authority_unreachable.rs:24:// `overwrite_segmented` directly, which is exactly the false-provenance

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n \"CodeTransform|try_update|try_remove|try_append_left|try_prepend_right\" crates/verter_compiler/src/svelte/runtime/css/render.rs crates/verter_compiler/src/svelte | head -n 220 && nl -ba crates/verter_compiler/src/svelte/runtime/css/render.rs | sed -n '1,100p;300,390p' && nl -ba crates/verter_compiler/src/template/code_gen/types.rs | sed -n '647,716p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 234ms:[0m
crates/verter_compiler/src/svelte/runtime/css/render.rs:10://!   [`CodeTransform`]'s checked (`try_*`) operations, whose
crates/verter_compiler/src/svelte/runtime/css/render.rs:13://!   meaning: `try_append_left` content precedes `try_prepend_right` content
crates/verter_compiler/src/svelte/runtime/css/render.rs:14://!   at the same position, the content-only `try_update` preserves boundary
crates/verter_compiler/src/svelte/runtime/css/render.rs:16://!   unused comment" case), and `try_remove` clears interior insertions. No
crates/verter_compiler/src/svelte/runtime/css/render.rs:20://!   (CodeTransform-SSOT).
crates/verter_compiler/src/svelte/runtime/css/render.rs:46:use crate::code_transform::{CodeTransform, CodeTransformError, SourceMapOptions};
crates/verter_compiler/src/svelte/runtime/css/render.rs:63:/// desync (CodeTransform-SSOT).
crates/verter_compiler/src/svelte/runtime/css/render.rs:115:        code: CodeTransform::new(source, &allocator),
crates/verter_compiler/src/svelte/runtime/css/render.rs:189:fn edit_failure_offset(error: CodeTransformError) -> u32 {
crates/verter_compiler/src/svelte/runtime/css/render.rs:191:        CodeTransformError::OutOfRange { offset, .. }
crates/verter_compiler/src/svelte/runtime/css/render.rs:192:        | CodeTransformError::MidChar { offset }
crates/verter_compiler/src/svelte/runtime/css/render.rs:193:        | CodeTransformError::ZeroLengthRange { offset }
crates/verter_compiler/src/svelte/runtime/css/render.rs:194:        | CodeTransformError::ReplacedContentSplit { offset } => offset,
crates/verter_compiler/src/svelte/runtime/css/render.rs:195:        CodeTransformError::ReversedRange { start, .. } => start,
crates/verter_compiler/src/svelte/runtime/css/render.rs:211:    /// The SHARED span-edit transform (CodeTransform-SSOT) over the ORIGINAL
crates/verter_compiler/src/svelte/runtime/css/render.rs:214:    code: CodeTransform<'a>,
crates/verter_compiler/src/svelte/runtime/css/render.rs:236:    edit_failure: Option<CodeTransformError>,
crates/verter_compiler/src/svelte/runtime/css/render.rs:340:        if let Err(error) = self.code.try_append_left(index, content) {
crates/verter_compiler/src/svelte/runtime/css/render.rs:351:        if let Err(error) = self.code.try_prepend_right(index, content) {
crates/verter_compiler/src/svelte/runtime/css/render.rs:373:        if let Err(error) = self.code.try_update(start, end, content) {
crates/verter_compiler/src/svelte/runtime/css/render.rs:396:        if let Err(error) = self.code.try_remove(start, end) {
crates/verter_compiler/src/svelte/carrier.rs:288:    /// The Svelte IDE codegen owns its OWN `CodeTransform` (the single source
crates/verter_compiler/src/svelte/carrier.rs:1445:        // checker injection went through CodeTransform, so the map stays
crates/verter_compiler/src/svelte/mod.rs:11://! projection ([`ide`]) is a pure syntactic transform via `CodeTransform` —
crates/verter_compiler/src/svelte/ide/mod.rs:5://! contract). Every edit goes through one [`CodeTransform`] (the single source of
crates/verter_compiler/src/svelte/ide/mod.rs:26://! pure SYNTACTIC transform via `CodeTransform` — NO type lowering runs here
crates/verter_compiler/src/svelte/ide/projector/event_inference.rs:16:use crate::code_transform::CodeTransform;
crates/verter_compiler/src/svelte/ide/projector/event_inference.rs:29:    out: &mut CodeTransform<'_>,
crates/verter_compiler/src/svelte/ide/projector/event_inference.rs:158:    out: &mut CodeTransform<'_>,
crates/verter_compiler/src/svelte/ide/projector/facade.rs:25:use crate::code_transform::CodeTransform;
crates/verter_compiler/src/svelte/ide/projector/facade.rs:29:    ct: &mut CodeTransform<'a>,
crates/verter_compiler/src/svelte/ide/projector_tests.rs:2081:    // The CodeTransform map is chunk-granular: the bound-value Original chunk
crates/verter_compiler/src/svelte/ide/projector_tests.rs:3092:    // F11 (P1-2): the text-path @const rewrite reuses the CodeTransform ops (not
crates/verter_compiler/src/svelte/ide/projector/store.rs:8://! identifier). The whole rewrite goes through `CodeTransform` ops — never a
crates/verter_compiler/src/svelte/ide/projector/store.rs:13:use crate::code_transform::CodeTransform;
crates/verter_compiler/src/svelte/ide/projector/store.rs:84:        let mut ct = CodeTransform::new(pattern_text, &allocator);
crates/verter_compiler/src/svelte/ide/projector/store.rs:122:    /// applied to the text and returned for the caller's single CodeTransform
crates/verter_compiler/src/svelte/ide/projector/store.rs:149:/// BOTH rewrites run the SAME CodeTransform ops the span path uses
crates/verter_compiler/src/svelte/ide/projector/store.rs:165:    let mut ct = CodeTransform::new(text, &allocator);
crates/verter_compiler/src/svelte/ide/projector/store.rs:195:pub(super) fn rewrite_store_sub(ct: &mut CodeTransform, base: u32, sub: &StoreSub) {
crates/verter_compiler/src/svelte/ide/await_scan.rs:25:use crate::code_transform::CodeTransform;
crates/verter_compiler/src/svelte/ide/await_scan.rs:29:/// `__verter_await_expr(ARG)` through `CodeTransform` ops on `ct`.
crates/verter_compiler/src/svelte/ide/await_scan.rs:47:pub(super) fn rewrite_await_exprs_on(ct: &mut CodeTransform, base: u32, text: &str) {
crates/verter_compiler/src/svelte/ide/await_scan.rs:68:fn apply_await_rewrite(ct: &mut CodeTransform, positions: impl Iterator<Item = AwaitPosition>) {
crates/verter_compiler/src/svelte/ide/await_scan.rs:84:/// `($item = await thing())`) through `CodeTransform` ops on `ct`, returning
crates/verter_compiler/src/svelte/ide/await_scan.rs:99:    ct: &mut CodeTransform,
crates/verter_compiler/src/svelte/ide/projector/mod.rs:3://! Drives ONE [`CodeTransform`] over the original `.svelte` source. Structural
crates/verter_compiler/src/svelte/ide/projector/mod.rs:11://! scope in source order via CodeTransform MOVE operations.
crates/verter_compiler/src/svelte/ide/projector/mod.rs:19:use crate::code_transform::{CodeTransform, SourceMapOptions};
crates/verter_compiler/src/svelte/ide/projector/mod.rs:120:    let mut ct = CodeTransform::new(source, &allocator);
crates/verter_compiler/src/svelte/ide/projector/mod.rs:135:    // `$`-byte / `=`-operator CodeTransform overwrites only (the identifier / RHS
crates/verter_compiler/src/svelte/ide/projector/mod.rs:192:    // parameter tuple before any script body can be moved; CodeTransform moves
crates/verter_compiler/src/svelte/ide/projector/mod.rs:260:    // CodeTransform still emits it before every authored/moved chunk and
crates/verter_compiler/src/svelte/ide/projector/mod.rs:268:    // CodeTransform insertions keep facade scaffolding unmapped while each line
crates/verter_compiler/src/svelte/ide/projector/mod.rs:270:    // leaving CodeTransform as the single text and mapping authority.
crates/verter_compiler/src/svelte/ide/projector/mod.rs:401:fn remove_span(ct: &mut CodeTransform, span: Span) {
crates/verter_compiler/src/svelte/ide/projector/mod.rs:417:    ct: &mut CodeTransform,
crates/verter_compiler/src/svelte/ide/projector/mod.rs:452:    ct: &'ct mut CodeTransform<'a>,
crates/verter_compiler/src/svelte/ide/projector/mod.rs:713:    /// run on the TEXT before it is emitted, NOT via CodeTransform ops on the
crates/verter_compiler/src/svelte/ide/projector/mod.rs:785:                // entry (`$store` / `await e` interior CodeTransform overwrites,
crates/verter_compiler/src/svelte/ide/projector/mod.rs:1263:    /// only the synthetic wrapper bytes are inserted via CodeTransform ops (no
crates/verter_compiler/src/svelte/runtime/instance_items.rs:64:    /// `CodeTransform`. Svelte-owned declarations/runes keep their dedicated
crates/verter_compiler/src/svelte/runtime/css/render.rs:10://!   [`CodeTransform`]'s checked (`try_*`) operations, whose
crates/verter_compiler/src/svelte/runtime/css/render.rs:13://!   meaning: `try_append_left` content precedes `try_prepend_right` content
crates/verter_compiler/src/svelte/runtime/css/render.rs:14://!   at the same position, the content-only `try_update` preserves boundary
crates/verter_compiler/src/svelte/runtime/css/render.rs:16://!   unused comment" case), and `try_remove` clears interior insertions. No
crates/verter_compiler/src/svelte/runtime/css/render.rs:20://!   (CodeTransform-SSOT).
crates/verter_compiler/src/svelte/runtime/css/render.rs:46:use crate::code_transform::{CodeTransform, CodeTransformError, SourceMapOptions};
crates/verter_compiler/src/svelte/runtime/css/render.rs:63:/// desync (CodeTransform-SSOT).
crates/verter_compiler/src/svelte/runtime/css/render.rs:115:        code: CodeTransform::new(source, &allocator),
crates/verter_compiler/src/svelte/runtime/css/render.rs:189:fn edit_failure_offset(error: CodeTransformError) -> u32 {
crates/verter_compiler/src/svelte/runtime/css/render.rs:191:        CodeTransformError::OutOfRange { offset, .. }
crates/verter_compiler/src/svelte/runtime/css/render.rs:192:        | CodeTransformError::MidChar { offset }
crates/verter_compiler/src/svelte/runtime/css/render.rs:193:        | CodeTransformError::ZeroLengthRange { offset }
crates/verter_compiler/src/svelte/runtime/css/render.rs:194:        | CodeTransformError::ReplacedContentSplit { offset } => offset,
crates/verter_compiler/src/svelte/runtime/css/render.rs:195:        CodeTransformError::ReversedRange { start, .. } => start,
crates/verter_compiler/src/svelte/runtime/css/render.rs:211:    /// The SHARED span-edit transform (CodeTransform-SSOT) over the ORIGINAL
crates/verter_compiler/src/svelte/runtime/css/render.rs:214:    code: CodeTransform<'a>,
crates/verter_compiler/src/svelte/runtime/css/render.rs:236:    edit_failure: Option<CodeTransformError>,
crates/verter_compiler/src/svelte/runtime/css/render.rs:340:        if let Err(error) = self.code.try_append_left(index, content) {
crates/verter_compiler/src/svelte/runtime/css/render.rs:351:        if let Err(error) = self.code.try_prepend_right(index, content) {
crates/verter_compiler/src/svelte/runtime/css/render.rs:373:        if let Err(error) = self.code.try_update(start, end, content) {
crates/verter_compiler/src/svelte/runtime/css/render.rs:396:        if let Err(error) = self.code.try_remove(start, end) {
crates/verter_compiler/src/svelte/runtime/output.rs:4://! emission. The final module is lowered once through [`CodeTransform`], which is
crates/verter_compiler/src/svelte/runtime/output.rs:12:use crate::code_transform::{CodeTransform, SourceMapOptions};
crates/verter_compiler/src/svelte/runtime/output.rs:149:        let mut transform = CodeTransform::new(source, allocator);
crates/verter_compiler/src/svelte/runtime/expr_rewrite/plan_planner.rs:6://! applies to its `CodeTransform`.
crates/verter_compiler/src/svelte/runtime/expr_rewrite/plan_planner.rs:12:/// and emits the CodeTransform edits, OR records a refusal. A `MustRewrite`
crates/verter_compiler/src/svelte/runtime/expr_emit_tests.rs:835:// erasure with reactive edits through `CodeTransform`. Component-level coverage for
crates/verter_compiler/src/svelte/runtime/expr_rewrite/mod.rs:15://! 2. [`RewritePlanner`](plan_planner::RewritePlanner) — turns the occurrences into [`CodeTransform`] edits, or a
crates/verter_compiler/src/svelte/runtime/expr_rewrite/mod.rs:36:use crate::code_transform::CodeTransform;
crates/verter_compiler/src/svelte/runtime/expr_rewrite/mod.rs:48:/// CodeTransform that produced it owned the source-map authority, which is a
crates/verter_compiler/src/svelte/runtime/expr_rewrite/mod.rs:69:/// edits over the original script bytes with one `CodeTransform`.
crates/verter_compiler/src/svelte/runtime/expr_rewrite/mod.rs:109:    let mut transform = CodeTransform::new(source, &alloc);
crates/verter_compiler/src/svelte/runtime/expr_rewrite/mod.rs:138:fn apply_edits(transform: &mut CodeTransform<'_>, edits: &[Edit]) {
crates/verter_compiler/src/svelte/runtime/expr_rewrite/mod.rs:497:/// the CodeTransform composition, the inner-expression slice) is dialect- and
crates/verter_compiler/src/svelte/runtime/expr_rewrite/mod.rs:579:    let mut ct = CodeTransform::new(&wrapped, &ct_alloc);
crates/verter_compiler/src/svelte/runtime/client.rs:19://! rewritten through its own [`CodeTransform`] over the expression's source
crates/verter_compiler/src/svelte/runtime/client.rs:21://! spans), so the load-bearing source-derived edits stay on the CodeTransform
crates/verter_compiler/src/svelte/runtime/expr_rewrite/plan.rs:7://! typed [`Edit`]s the caller applies to its `CodeTransform`.
crates/verter_compiler/src/svelte/runtime/expr_rewrite/plan.rs:75:    // dropped. Pass 2 turns the occurrences into CodeTransform edits or a refusal.
crates/verter_compiler/src/svelte/runtime/expr_rewrite/plan.rs:134:/// erasure on one `CodeTransform`.
crates/verter_compiler/src/svelte/runtime/expr_rewrite/plan.rs:1443:        // removes the type syntax on the same `CodeTransform`. Do NOT descend (a
crates/verter_compiler/src/svelte/runtime/mod.rs:26://! rewrites through [`CodeTransform`](crate::code_transform::CodeTransform); the
crates/verter_compiler/src/svelte/runtime/client_tests.rs:12207:// keeps the carrier-codegen CodeTransform text-scan guard scoped to
     1	//! The scoped-CSS renderer — a faithful port of the official `svelte@5.56.3`
     2	//! `phases/3-transform/css/index.js` (`render_stylesheet` + its zimmerframe
     3	//! visitors), producing the scoped stylesheet text (the official `css.code`)
     4	//! by SOURCE-POSITION edits over the ORIGINAL component source.
     5	//!
     6	//! Two fidelity anchors:
     7	//!
     8	//! - Every mutation is a span-addressed edit (insert / update / remove at
     9	//!   byte offsets carried by the parsed CSS AST) applied through the SHARED
    10	//!   [`CodeTransform`]'s checked (`try_*`) operations, whose
    11	//!   insertion-affinity chunk model carries the `magic-string` semantics the
    12	//!   official renderer edits through — the semantics that carry output
    13	//!   meaning: `try_append_left` content precedes `try_prepend_right` content
    14	//!   at the same position, the content-only `try_update` preserves boundary
    15	//!   insertions on the replaced range's first chunk (the official "closing
    16	//!   unused comment" case), and `try_remove` clears interior insertions. No
    17	//!   string surgery on rendered output, no reserialization — and because the
    18	//!   render edits the one shared transform, the on-demand css SOURCE MAP is
    19	//!   generated from the SAME chunk list that built the code
    20	//!   (CodeTransform-SSOT).
    21	//! - The visitor walk mirrors the official visitor set, order, and early
    22	//!   returns: `Atrule` (keyframes rename, then stop), `Declaration`
    23	//!   (`animation`/`animation-name` token rewrite), `Rule` (empty / unused
    24	//!   comment-wrap, `:global { … }` block wrap), `SelectorList` (per-selector
    25	//!   prune toggle + the per-rule specificity reset), `ComplexSelector`
    26	//!   (`:global` removal + scope-class application), and the
    27	//!   `PseudoClassSelector` recursion rule (argument lists are visited for
    28	//!   `:is`/`:where`/`:has`/`:not` only).
    29	//!
    30	//! Verter's runtime codegen refuses dev output, so the renderer implements
    31	//! the official non-dev branches in BOTH minify families: the EXTERNAL
    32	//! `css.code` artifact (comment-wrapped prunes, whitespace preserved) and the
    33	//! MINIFIED injected `$$css` payload (`state.minify = inject_styles && !dev`
    34	//! — outright removals, per-declaration whitespace collapse, custom-property
    35	//! values preserved). Both outputs are byte-parity with the official
    36	//! compiler for the same input.
    37	
    38	use oxc_allocator::Allocator;
    39	use verter_span::Span;
    40	
    41	use super::analyze::{is_keyframes_node, keyframes_name_token_span, remove_css_prefix};
    42	use super::types::{
    43	    Atrule, Block, BlockChild, Combinator, ComplexSelector, Declaration, KeyframeName, Rule,
    44	    SelectorList, SimpleSelector, StyleChild, StyleSheet,
    45	};
    46	use crate::code_transform::{CodeTransform, CodeTransformError, SourceMapOptions};
    47	
    48	/// A fail-closed render refusal: the AST or spans handed to the renderer were
    49	/// malformed — an out-of-range or mid-character span, an inconsistent
    50	/// metadata/node shape, an edit the chunk model cannot express. The caller
    51	/// treats it exactly like an analysis failure (the style stays refused); a
    52	/// partial or unscoped stylesheet is never produced, and the renderer never
    53	/// panics the host.
    54	#[derive(Debug, Clone, Copy, PartialEq, Eq)]
    55	pub(crate) struct RenderError {
    56	    /// The span (or point offset) of the offending construct.
    57	    pub span: Span,
    58	}
    59	
    60	/// The scoped render's output pair — the official `css.code` bytes plus the
    61	/// on-demand css source map (the official `css.map`), generated from the
    62	/// SAME shared transform whose edits produced the code, so the two can never
    63	/// desync (CodeTransform-SSOT).
    64	#[derive(Debug, Clone, PartialEq, Eq)]
    65	pub(crate) struct ScopedCssRender {
    66	    /// The rendered scoped stylesheet (the official `css.code`).
    67	    pub(crate) code: String,
    68	    /// The css source-map JSON — `Some` ONLY when the render was asked for
    69	    /// it (`want_source_map`). Its mappings point rendered css positions
    70	    /// back to the ORIGINAL component source; `file` and `sources[0]` carry
    71	    /// the component filename's BASENAME (the official magic-string
    72	    /// `generateMap` naming) or `"(unknown)"` when no filename was given
    73	    /// (svelte's validated-options default), and `sourcesContent` embeds the
    74	    /// component source either way.
    75	    pub(crate) source_map: Option<String>,
    76	}
    77	
    78	/// Render the scoped stylesheet (the official `css.code`) from the analyzed +
    79	/// matcher-verdict-bearing AST: scope classes applied per the `scoped`
    80	/// compound verdicts, `:global(...)` unwrapped, unused/empty rules
    81	/// comment-pruned per the `used` selector verdicts, and local `@keyframes`
    82	/// renamed to `<hash>-<name>` (with `animation`/`animation-name` references
    83	/// rewritten). `keyframes` is the LOCAL rename list (the official
    84	/// `analysis.css.keyframes`).
    85	///
    86	/// `minify` selects the official `state.minify = inject_styles && !dev`
    87	/// branch family (Verter's runtime codegen refuses dev output, so the flag is
    88	/// exactly the css output mode): the INJECTED `$$css` payload strips
    89	/// inter-rule/declaration whitespace and REMOVES unused/empty rules and the
    90	/// `:global {}` wrapper tokens outright, where the external artifact
    91	/// comment-wraps them.
    92	///
    93	/// A malformed input fails closed with [`RenderError`] instead of panicking;
    94	/// CSS that parsed and analyzed against the same source never takes that
    95	/// path, so the faithful output is unchanged for every valid stylesheet.
    96	///
    97	/// `filename` names the component source in the on-demand css source map
    98	/// (the official `generateMap({ source, file })` inputs — emitted as its
    99	/// BASENAME, or `"(unknown)"` when absent); `want_source_map` is the map
   100	/// demand — `source_map` stays `None` without it, and the rendered `code`
   300	
   301	    fn register_selector_list_locations(&mut self, list: &SelectorList) {
   302	        self.register_span(list.span);
   303	        for complex in &list.children {
   304	            self.register_span(complex.span);
   305	            for relative in &complex.children {
   306	                self.register_span(relative.span);
   307	                if let Some(combinator) = &relative.combinator {
   308	                    self.register_span(combinator.span);
   309	                }
   310	                for selector in &relative.selectors {
   311	                    self.register_span(selector.span());
   312	                    if let SimpleSelector::PseudoClass {
   313	                        args: Some(arguments),
   314	                        ..
   315	                    } = selector
   316	                    {
   317	                        self.register_selector_list_locations(arguments);
   318	                    }
   319	                }
   320	            }
   321	        }
   322	    }
   323	
   324	    fn register_span(&mut self, span: Span) {
   325	        if self.edit_failure.is_some() {
   326	            return;
   327	        }
   328	        for offset in [span.start, span.end] {
   329	            if let Err(error) = self.code.try_add_sourcemap_location(offset) {
   330	                self.edit_failure = Some(error);
   331	                return;
   332	            }
   333	        }
   334	    }
   335	
   336	    fn append_left(&mut self, index: u32, content: &str) {
   337	        if self.edit_failure.is_some() {
   338	            return;
   339	        }
   340	        if let Err(error) = self.code.try_append_left(index, content) {
   341	            self.edit_failure = Some(error);
   342	        }
   343	    }
   344	
   345	    /// `prependRight(index, content)` — RIGHT affinity, stacking in reverse
   346	    /// call order.
   347	    fn prepend_right(&mut self, index: u32, content: &str) {
   348	        if self.edit_failure.is_some() {
   349	            return;
   350	        }
   351	        if let Err(error) = self.code.try_prepend_right(index, content) {
   352	            self.edit_failure = Some(error);
   353	        }
   354	    }
   355	
   356	    /// `appendRight(index, content)` — RIGHT affinity, stacking in call
   357	    /// order.
   358	    fn append_right(&mut self, index: u32, content: &str) {
   359	        if self.edit_failure.is_some() {
   360	            return;
   361	        }
   362	        if let Err(error) = self.code.try_append_right(index, content) {
   363	            self.edit_failure = Some(error);
   364	        }
   365	    }
   366	
   367	    /// `update(start, end, content)` — content-only: the range's first chunk
   368	    /// keeps its boundary insertions.
   369	    fn update(&mut self, start: u32, end: u32, content: &str) {
   370	        if self.edit_failure.is_some() {
   371	            return;
   372	        }
   373	        if let Err(error) = self.code.try_update(start, end, content) {
   374	            self.edit_failure = Some(error);
   375	        }
   376	    }
   377	
   378	    /// `overwrite(start, end, content)` — clears the range's first-chunk
   379	    /// boundary insertions.
   380	    fn overwrite(&mut self, start: u32, end: u32, content: &str) {
   381	        if self.edit_failure.is_some() {
   382	            return;
   383	        }
   384	        if let Err(error) = self.code.try_overwrite(start, end, content) {
   385	            self.edit_failure = Some(error);
   386	        }
   387	    }
   388	
   389	    /// `remove(start, end)` — clears content AND boundary insertions of
   390	    /// every chunk starting within the range (a zero-length range is a
   647	        // Apply wrapped moves FIRST — they operate on Original chunks and must
   648	        // run before overwrites replace those chunks. This preserves sourcemap
   649	        // for moved content (e.g., defineProps type params).
   650	        for &(start, end, target, prefix, suffix) in &self.wrapped_moves {
   651	            ct.move_wrapped(start, end, target, prefix, suffix);
   652	        }
   653	
   654	        // Sort by start ascending, then by end descending (so that for equal
   655	        // starts, the wider range comes first and the narrower is filtered out).
   656	        self.overwrites
   657	            .sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
   658	
   659	        // Filter out fully-contained ranges. After sorting, any range whose
   660	        // end <= the running max_end is fully inside a preceding range and
   661	        // would produce a redundant (overlapping) overwrite.
   662	        let mut max_end: u32 = 0;
   663	        self.overwrites.retain(|&(start, end, _)| {
   664	            if start >= max_end {
   665	                // Non-overlapping — accept and update max_end
   666	                max_end = end;
   667	                true
   668	            } else if end > max_end {
   669	                // Partial overlap extending beyond max_end — accept the extension
   670	                max_end = end;
   671	                true
   672	            } else {
   673	                // Fully contained (start >= prev_start && end <= max_end) — drop
   674	                false
   675	            }
   676	        });
   677	
   678	        ct.batch_overwrite(&self.overwrites);
   679	
   680	        // Unmapped overwrites are few and never overlap `overwrites` (both
   681	        // target root-level tag spans) — a sorted loop is enough.
   682	        //
   683	        // `overwrite_unmapped_alloc`, not `overwrite_unmapped`: every entry
   684	        // here is ALREADY bump-allocated against this same `'alloc`
   685	        // allocator (`overwrite_unmapped_alloc`/`overwrite_or_root_prefix_alloc`/
   686	        // `overwrite_or_root_suffix_alloc`'s nonzero-width branch all push
   687	        // here) — `overwrite_unmapped`'s unconditional re-allocation would
   688	        // mint a NEW pointer, breaking any `record_sfc_export_fact` marker
   689	        // minted against the original.
   690	        self.unmapped_overwrites
   691	            .sort_unstable_by_key(|&(start, ..)| start);
   692	        for &(start, end, content) in &self.unmapped_overwrites {
   693	            ct.overwrite_unmapped_alloc(start, end, content);
   694	        }
   695	
   696	        // Segmented overwrites: disjoint from the other overwrite channels
   697	        // (one mechanism per AST node), so ordering vs those channels is
   698	        // irrelevant. Failure is a caller precondition violation, not
   699	        // recoverable. Fail closed in every build: a silent `ct.overwrite()`
   700	        // fallback would re-fabricate whole-block provenance (every byte of
   701	        // `content` mapping to `start`) — a silently wrong source map.
   702	        self.segmented_overwrites
   703	            .sort_unstable_by_key(|&(start, ..)| start);
   704	        for &(start, end, content, anchors) in &self.segmented_overwrites {
   705	            if let Err(err) = ct.try_overwrite_segmented(
   706	                start,
   707	                end,
   708	                content,
   709	                anchors,
   710	                SegmentedOverwriteAuthority::new(),
   711	            ) {
   712	                panic!("overwrite_segmented precondition violated at [{start},{end}): {err:?}");
   713	            }
   714	        }
   715	
   716	        if self.mapped_prepends.is_empty() {

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/decisions/ADR-010-compositional-products-and-mapping-taxonomy.md | sed -n '1,90p' && nl -ba docs/arch/refactor/rev11/architecture.md | sed -n '250,350p' | head -n 120"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
     1	# ADR-010 — Compiler Products Are Compositional and Mapping Kinds Are Distinct
     2	
     3	**Status:** Accepted  
     4	**Decision owner:** compiler request and generated-artifact contract  
     5	**Reopen only if:** a product can prove mutual exclusivity or a new mapping class with distinct semantics.
     6	
     7	## Context
     8	
     9	Real requests can require several products and independent materializations. A single artifact-level enum encourages hidden “full analysis.” Treating all maps as one product conflicts with IDE companions that require projection mappings and runtime outputs whose source maps are optional.
    10	
    11	## Decision
    12	
    13	A compile request contains:
    14	
    15	- a canonical non-empty collection of typed product requests;
    16	- per-product output and terminal materialization requests rather than one global output/materialization bag;
    17	- a typed Vue or Svelte payload before planning;
    18	- one shared semantic profile only when the requested work observes TypeScript-compatible semantics.
    19	
    20	Each product request carries only the output, presentation, mapping, provenance, and serialization profiles that can affect that product. Duplicate product kinds and irrelevant profile fields are rejected before expensive work. Equal normalized subrequests may share one private stage/subplan.
    21	
    22	Mapping classes are separate:
    23	
    24	1. `PlacementMap` — source/unit placement composition used internally where required;
    25	2. `SourceProjectionMap` — required by an IDE/provider companion and published atomically with it;
    26	3. `RuntimeSourceMapData` — optional runtime/build map segments created only when requested;
    27	4. `EncodedSourceMap` — terminal serialization of requested map data.
    28	
    29	An operation with no mapping requirement performs zero map construction/encoding. No universal artifact bag is required; typed product results may share one private execution plan.
    30	
    31	## Consequences
    32	
    33	- runtime plus declarations or IDE plus public API can be requested coherently even when their output/terminal profiles differ;
    34	- required mappings cannot be omitted or mixed with another code generation;
    35	- presentation/serialization changes do not invalidate unrelated semantic/code artifacts;
    36	- map-disabled runtime work remains truly map-free.
    37	
    38	## Rejected alternatives
    39	
    40	- **Single mutually exclusive artifact enum:** cannot express real product composition.
    41	- **Always build one map type:** wastes work and conflates different validity contracts.
   250	- an incompatible explicit selection returns an actionable capability error;
   251	- `auto` is evidence-based and project-scoped;
   252	- provider failure does not silently switch semantic engines inside a request;
   253	- native enrichment may add separately labeled Verter facts but never overwrite provider TypeScript semantics;
   254	- native TypeInfo is not an automatic fallback;
   255	- a future native-primary mode requires a separate conformance/product program.
   256	
   257	`verter_tsc` uses a narrow native-TypeScript batch checker client and does not import `ProviderHub`, editor attachment, hover/completion state, or LSP provider lifecycle.
   258	
   259	# 3. Identity and compatibility
   260	
   261	## 3.1 Distinct identity types
   262	
   263	Identity types are non-interchangeable:
   264	
   265	```text
   266	ContentId                    exact byte content
   267	SourceId                     logical source identity
   268	SourceRevision               exact source version
   269	SourceUnitId                 stable logical carrier unit identity
   270	DocumentVersion              LSP client version
   271	DocumentIncarnation          open/close lifecycle identity
   272	EngineRevision               committed input ordering aid
   273	ProjectRevision              project topology identity
   274	ConfigurationRevision        configuration identity
   275	SyntaxProfileId              grammar/source-type/recovery/options identity
   276	ParseKey                     exact syntax construction identity
   277	ParseOwnerDomainId           direct invocation/batch, PreparedCarrier, or managed owner/shard
   278	ParseInstanceId              (ParseOwnerDomainId, ParseKey, instance generation)
   279	TypeScriptSemanticProfileId  TypeScript-compatible interpretation
   280	OutputProfileId              generated program/runtime contract
   281	PresentationProfileId        rendered text/diagnostic/path-ordering contract
   282	SerializationProfileId       wire/container layout contract
   283	ArtifactKey<T>               typed artifact construction identity
   284	QueryIdentity<Q>             semantic arguments + profiles + result contract; snapshot-independent candidate lookup
   285	InputBasisId                 exact captured semantic observation basis
   286	SemanticFlightKey<Q>         (QueryIdentity<Q>, InputBasisId)
   287	ResultContractId             observable semantics/exactness/capability/approximation contract
   288	ProviderEpoch                selected provider lifecycle identity
   289	ProviderContractId           provider route/version/capability interpretation
   290	RequestGeneration            supersession order for one request stream
   291	CompatibilityDomainId        serialized/persistent interpretation namespace
   292	CompatibilityEpoch           monotonic epoch inside a domain
   293	StableEntityId               deterministic public/content-relative identity
   294	SessionHandle                opaque owner/cohort-bound continuation handle
   295	```
   296	
   297	A digest encodes identity; it is not identity authority by itself. Collision-sensitive uses retain equality material or verify equality on collision.
   298	
   299	Deadlines, cancellation tokens, trace IDs, priorities, queue classes, and ordinary work/time/memory budgets are not reusable query identity. Semantic/output/presentation/serialization profile values remain in their named profile IDs; `ResultContractId` does not duplicate them. It represents operation/product shape, required exactness/completeness, capability and unsupported/degradation policy, required mapping/diagnostic/serialization outcome at the typed boundary, and explicitly requested approximation mode. Waiter-local limits remain in `ExecutionPolicy`; budget exhaustion yields partial/failure rather than a weaker complete result.
   300	
   301	`EngineRevision` orders commits and captures snapshots. It is not a universal cache key.
   302	
   303	## 3.2 Semantic, output, presentation, serialization, and execution profiles
   304	
   305	`TypeScriptSemanticProfileId` identifies observable TypeScript-compatible interpretation. Its closed normalized descriptor includes every supported dimension that can change meaning, such as:
   306	
   307	- supported TypeScript compatibility family/version;
   308	- semantics-affecting compiler options;
   309	- module/module-resolution mode and conditions;
   310	- JSX semantics;
   311	- target/lib basis, including exact custom/versioned library fingerprints where relevant;
   312	- package exports/imports, paths, type roots, package-boundary, case, symlink, and workspace policy;
   313	- declared supported behavior of the Verter semantic kernel.
   314	
   315	It does **not** contain diagnostic wording, path display, serialization layout, worker count, cache policy, build timestamp, or an internal progress counter. Unknown behavior-affecting options fail closed.
   316	
   317	`OutputProfileId` owns generated-program semantics: framework target, client/server/runtime mode, helper/runtime compatibility, and other code-generation dimensions.
   318	
   319	`PresentationProfileId` owns deterministic rendered forms: diagnostic text version, rendered type text, path display, locale policy, and stable presentation ordering. It is absent when presentation is not requested.
   320	
   321	`SerializationProfileId` owns wire/container encoding and field layout. It is absent when serialization is not requested.
   322	
   323	Execution profile—native threaded, native single-thread, or WASM single-thread—is not semantic identity for operations declared portable. Capability differences are declared before execution; they do not silently change the result contract.
   324	
   325	An internal refactor leaves the semantic profile unchanged unless observable semantics change. Verter-specific stricter analysis is separately labeled enrichment.
   326	
   327	## 3.3 Compatibility domains
   328	
   329	Version numbers represent interpretation compatibility, not progress.
   330	
   331	Classify every version-like value:
   332	
   333	1. **No compatibility boundary:** internal in-memory implementation detail. Delete it.
   334	2. **Persistent/precomputed boundary:** retain one typed compatibility domain and monotonic epoch only if older bytes could otherwise be misinterpreted.
   335	3. **External wire/protocol boundary:** retain a named domain, schema/epoch policy, and compatibility tests.
   336	
   337	Rules:
   338	
   339	- `0` is a valid first epoch and never means uninitialized;
   340	- epochs are monotonic within a published domain;
   341	- a clean incompatible replacement may create a new domain/namespace whose first epoch is `0`;
   342	- do not reset an existing published domain merely because the project is pre-1.0;
   343	- one domain has one owner;
   344	- no duplicate counters must remain equal by convention;
   345	- ordinary in-memory DTOs are versionless;
   346	- a build/toolchain fingerprint may invalidate disposable caches without per-subsystem progress counters;
   347	- disposable beta caches may be dropped without converters;
   348	- package semver, external tool versions, source revisions, and provider epochs are separate concepts.
   349	
   350	# 4. Execution regimes and platform profiles

[35m[3mcodex[0m[0m
The backend audit is now bounded: Vapor has one whole-template replacement and never emits comment-strip edits into `CodeGenOutput`; SSR uses segmented edits for comment stripping and generally disjoint tag/node ranges. I found one separate SSR edge worth distinguishing from this VDOM blocker: a comment-only template can pair the SSR empty-template whole-range replacement with a nested comment replacement. I’m checking whether that is reachable under current options before stating its disposition.
[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n \"comments\\.unwrap_or|comments:.*is_production|TemplateCodeGenOptions.*comments|comments,\" crates/verter_compiler/src | head -n 160 && rg -n \"VueBackendRequest::Ssr|backend: VueBackendRequest|Ssr\" crates/verter_compiler/tests/cases crates/verter_compiler/src/standalone crates/verter_compiler/src | head -n 220"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 33ms:[0m
crates/verter_compiler/src/tsc/module_specifiers.rs:8://! literals, template literals, comments, and regular expressions, and the
crates/verter_compiler/src/css/walk.rs:13:/// (`from`, `to`, `0%`, etc.), comments, and strings.
crates/verter_compiler/src/css/mod.rs:63:/// This normalizes comments, strings, at-rules, and nesting so downstream
crates/verter_compiler/src/css/mod.rs:163:/// Skips class-like patterns inside strings, comments, and `v-bind()`.
crates/verter_compiler/src/css/mod.rs:164:/// May produce false positives from comments, but that's acceptable for IDE completions.
crates/verter_compiler/src/framework_common/vue_bridge.rs:354:                comments: opts.comments,
crates/verter_compiler/src/framework_common/vue_bridge.rs:889:            comments: opts.comments,
crates/verter_compiler/src/framework_common/vue_bridge.rs:994:                comments: opts.comments,
crates/verter_compiler/src/compile_request/svelte.rs:343:            preserve_comments: self.preserve_comments,
crates/verter_compiler/src/compile_request/svelte.rs:426:        assert_eq!(request.preserve_comments, Some(true));
crates/verter_compiler/src/svelte/parser/template_ast.rs:244:/// `acorn.parse(source, comments, parser.ts, …)`: when the parser-wide TS flag is OFF every
crates/verter_compiler/src/svelte/parser/tokenizer.rs:865:    /// Scan the top-level component body: text, comments, `<script>` /
crates/verter_compiler/src/svelte/parser/tokenizer.rs:1503:        // reject nested elements, comments, and blocks as `title_invalid_content`; an ordinary
crates/verter_compiler/src/template/code_gen/vdom/block.rs:18:    /// skipping non-element nodes (text, comments, interpolations) that
crates/verter_compiler/src/template/code_gen/vdom/block.rs:48:        // Scan forward, skipping non-element siblings (text, comments, etc.)
crates/verter_compiler/src/compile_request/vue.rs:694:            comments: self.comments,
crates/verter_compiler/src/compile_request/vue.rs:819:        assert_eq!(request.comments, Some(true));
crates/verter_compiler/src/compile_request/vue.rs:877:        assert_eq!(request.comments, Some(true));
crates/verter_compiler/src/compile/mod.rs:150:    // malformed comments, arbitrary top-level text) is still diagnosed.
crates/verter_compiler/src/compile/mod.rs:235:/// styles, custom blocks, malformed comments, and arbitrary top-level text are
crates/verter_compiler/src/compile/mod.rs:706:        comments: vue.comments,
crates/verter_compiler/src/compile/mod.rs:1224:                comments: options.comments.unwrap_or(!options.is_production),
crates/verter_compiler/src/compile/mod.rs:1617:                    comments: options.comments.unwrap_or(!options.is_production),
crates/verter_compiler/src/compile/mod.rs:1963:                        comments: options.comments.unwrap_or(!options.is_production),
crates/verter_compiler/src/compile/mod.rs:2396:            comments: options.comments,
crates/verter_compiler/src/compile/helpers.rs:66:/// SFC source may contain content between root-level blocks (e.g., HTML comments,
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1757:        // build_child_records excludes comments, so strip_interstitial_condition_nodes
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1765:        let _ = comment::process_comment(comment_node, source, self.options.comments, out);
crates/verter_compiler/src/template/code_gen/vapor/mod.rs:2113:            AstNodeKind::Comment(_) => self.options.comments,
crates/verter_compiler/src/template/code_gen/vapor/mod.rs:2166:            AstNodeKind::Comment(_) => self.options.comments,
crates/verter_compiler/src/template/code_gen/vapor/mod.rs:4173:            comment::process_comment(comment_node, source, self.options.comments, &mut self.html);
crates/verter_compiler/src/compile/types.rs:155:/// These mirror the Vue compiler's public options (comments, whitespace, hoisting,
crates/verter_compiler/src/template/code_gen/vdom/element.rs:124:/// Remove non-element children (comments, text) between v-if chain members.
crates/verter_compiler/src/svelte/runtime/runtime_tests.rs:2191:// comments, so the whole expression is captured and the trailing literal survives.
crates/verter_compiler/src/svelte/runtime/runtime_tests.rs:4494:            preserve_comments: resolved.preserve_comments,
crates/verter_compiler/src/svelte/runtime/client_compile.rs:166:        preserve_comments: resolved.preserve_comments,
crates/verter_compiler/src/svelte/carrier.rs:557:            preserve_comments: opts.svelte_preserve_comments,
crates/verter_compiler/src/svelte/runtime/compile_options.rs:233:    let preserve_comments = opts.preserve_comments.unwrap_or(false);
crates/verter_compiler/src/svelte/runtime/compile_options.rs:239:        preserve_comments,
crates/verter_compiler/src/svelte/runtime/html.rs:533:        ir.root_options.preserve_comments,
crates/verter_compiler/src/ide/script/setup.rs:165:        &effective_program.comments,
crates/verter_compiler/src/svelte/runtime/instance_items.rs:400:            &program.comments,
crates/verter_compiler/src/svelte/runtime/instance_items.rs:468:                comments,
crates/verter_compiler/src/svelte/runtime/instance_items.rs:548:                comments,
crates/verter_compiler/src/svelte/runtime/instance_items.rs:807:        comments,
crates/verter_compiler/src/svelte/runtime/instance_items.rs:812:        carrier_tail_comment_trivia(instance_source, comments, fact.call_span.end, stmt_end);
crates/verter_compiler/src/svelte/runtime/client_surface_refuse.rs:105:///   body (`animation_invalid_placement`) — "significant" excludes comments,
crates/verter_compiler/src/svelte/runtime/client_surface_refuse.rs:123:    // rule: comments, whitespace-only text, and `{@const}` / `{const …}` /
crates/verter_compiler/src/svelte/runtime/instance_item_shapes.rs:114:        comments,
crates/verter_compiler/src/svelte/runtime/instance_item_shapes.rs:120:        comments,
crates/verter_compiler/src/ide/script/mod.rs:44://!   // Block scope: destructure from temp with offset comments, then template JSX
crates/verter_compiler/src/svelte/runtime/expr_rewrite/mod.rs:89:        &program.comments,
crates/verter_compiler/src/svelte/runtime/expr_rewrite/mod.rs:574:        &program.comments,
crates/verter_compiler/src/svelte/runtime/whitespace.rs:90:            preserve_comments,
crates/verter_compiler/src/svelte/runtime/whitespace.rs:108:            preserve_comments: self.preserve_comments,
crates/verter_compiler/src/svelte/runtime/whitespace.rs:275:    // index). The dropped set (comments, hoisted non-rendering constructs, non-body
crates/verter_compiler/src/svelte/runtime/compile_options_tests.rs:1220:        preserve_comments: resolved.preserve_comments,
crates/verter_compiler/src/svelte/runtime/expr_rewrite/plan.rs:83:        comments,
crates/verter_compiler/src/svelte/runtime/expr_rewrite/plan.rs:148:        comments,
crates/verter_compiler/src/svelte/runtime/expr_rewrite/plan.rs:1194:                            self.comments,
crates/verter_compiler/src/svelte/runtime/expr_rewrite/plan.rs:1238:                                comments,
crates/verter_compiler/src/svelte/runtime/expr_rewrite/plan.rs:1247:                        self.comments,
crates/verter_compiler/src/svelte/runtime/client_walk.rs:97:/// region to walk: `region_walked` is computed without counting comments, and
crates/verter_compiler/src/svelte/runtime/client_tests.rs:299:    // NOT break the run: `clean_nodes` DROPS comments, so `a {c}<!--x--> b` is one
crates/verter_compiler/src/svelte/runtime/client_tests.rs:21261:    // Two same-line block comments, ASI at EOF: both collect, in source order,
crates/verter_compiler/src/svelte/ide/projector/special.rs:408:        comments: &parsed.program.comments,
crates/verter_compiler/src/svelte/ide/projector/special.rs:462:                    leading_jsdoc_type_payload(self.comments, decl.span.start, self.source)
crates/verter_compiler/src/ide/script/type_constructs.rs:67:                        let jsdoc = find_leading_jsdoc(comments, decl_start, content_str);
crates/verter_compiler/src/ide/script/type_constructs.rs:81:                    let jsdoc = find_leading_jsdoc(comments, func.span.start, content_str);
crates/verter_compiler/src/ide/script_recover.rs:8://! parsed. The scanner handles comments, strings, template literals, and bracket
crates/verter_compiler/src/ide/script_recover.rs:219:/// Single-pass, handles comments, strings, template literals, bracket matching.
crates/verter_compiler/src/ide/script_recover.rs:677:    /// Skip whitespace and comments, returning true if any were skipped.
crates/verter_compiler/src/ide/script_recover.rs:701:    /// Skip whitespace and comments, returning true if a newline was crossed.
crates/verter_compiler/src/ide/template/mod.rs:1470:    /// Comment indices to reposition inside v-if IIFEs (ALL comments, existing behavior).
crates/verter_compiler/src/ide/template/mod.rs:1975:    // TS directive comments, then ` return `. Both wrappers are synthetic, so only
crates/verter_compiler/src/ide/template/tests.rs:1500:        "should not have any comments, got:\n{}",
rg: crates/verter_compiler/src/standalone: No such file or directory (os error 2)
crates/verter_compiler/src/framework_common/vue_bridge.rs:783:                CompileRequestError::SsrVaporBackendUnsupported,
crates/verter_compiler/src/framework_common/vue_bridge.rs:788:                CompileRequestError::InlineSsrUnsupported,
crates/verter_compiler/src/framework_common/vue_bridge.rs:1993:                filename: Some("SsrVapor.vue".to_string()),
crates/verter_compiler/src/framework_common/vue_bridge.rs:2005:                    CompileRequestError::SsrVaporBackendUnsupported
crates/verter_compiler/src/framework_common/vue_bridge.rs:2008:            "expected a typed SsrVaporBackendUnsupported refusal, got {result:?}"
crates/verter_compiler/src/framework_common/vue_bridge.rs:2026:                filename: Some("ImplicitVaporSsr.vue".to_string()),
crates/verter_compiler/src/framework_common/vue_bridge.rs:2037:                    CompileRequestError::SsrVaporBackendUnsupported
crates/verter_compiler/src/framework_common/vue_bridge.rs:2040:            "expected a typed SsrVaporBackendUnsupported refusal for the implicit marker, got {result:?}"
crates/verter_compiler/src/framework_common/vue_bridge.rs:2059:                filename: Some("InlineSsr.vue".to_string()),
crates/verter_compiler/src/framework_common/vue_bridge.rs:2071:                    CompileRequestError::InlineSsrUnsupported
crates/verter_compiler/src/framework_common/vue_bridge.rs:2074:            "expected a typed InlineSsrUnsupported refusal, got {result:?}"
crates/verter_compiler/src/framework_common/vue_bridge.rs:2093:                    filename: Some("SsrOnly.vue".to_string()),
crates/verter_compiler/src/framework_common/carrier_compiler.rs:608:    SsrRender,
crates/verter_compiler/src/compile/mod.rs:806:    // `SsrVaporBackendUnsupported` before `compile()` is invoked at all,
crates/verter_compiler/src/compile/mod.rs:1609:                        CodeGenMode::Ssr
crates/verter_compiler/src/compile/mod.rs:1704:                        crate::framework_common::TemplateRenderExport::SsrRender
crates/verter_compiler/src/template/code_gen/mod.rs:13://! - **[`ssr::SsrCodeGen`]** — SSR mode. Emits `_push()` + `_ssrRenderAttrs()` for
crates/verter_compiler/src/template/code_gen/mod.rs:51:    Ssr,
crates/verter_compiler/src/template/code_gen/mod.rs:240:        CodeGenMode::Ssr => {
crates/verter_compiler/src/template/code_gen/mod.rs:269:        CodeGenMode::Ssr => {
crates/verter_compiler/src/template/code_gen/mod.rs:270:            let mut gen = ssr::SsrCodeGen::new(ast, oxc_ast, resolver, options);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:42:use super::shared::helpers::{self, is_builtin_component, to_pascal_case, SsrHelper, VdomHelper};
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:99:pub struct SsrCodeGen<'ast, 'alloc> {
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:182:impl<'ast, 'alloc> SsrCodeGen<'ast, 'alloc> {
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:365:        out.add_ssr_import(SsrHelper::GetDirectiveProps);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:2586:                                    out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:2595:                                    out.add_ssr_import(SsrHelper::RenderAttr);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:2671:                                    out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:2680:                                    out.add_ssr_import(SsrHelper::RenderAttr);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:2803:                    out.add_ssr_import(SsrHelper::RenderClass);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:2836:                    out.add_ssr_import(SsrHelper::RenderClass);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3001:                                out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3002:                                out.add_ssr_import(SsrHelper::LooseContain);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3018:                                out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3019:                                out.add_ssr_import(SsrHelper::LooseEqual);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3032:                                out.add_ssr_import(SsrHelper::RenderAttr);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3048:                                out.add_ssr_import(SsrHelper::LooseContain);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3062:                                out.add_ssr_import(SsrHelper::LooseEqual);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3098:                out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3099:                out.add_ssr_import(SsrHelper::LooseContain);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3100:                out.add_ssr_import(SsrHelper::LooseEqual);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3195:                out.add_ssr_import(SsrHelper::RenderStyle);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3320:                out.add_ssr_import(SsrHelper::RenderAttrs);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3321:                out.add_ssr_import(SsrHelper::GetDynamicModelProps);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3334:            out.add_ssr_import(SsrHelper::RenderAttrs);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3360:            out.add_ssr_import(SsrHelper::RenderAttrs);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3363:            out.add_ssr_import(SsrHelper::RenderAttrs);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:3397:                    out.add_ssr_import(SsrHelper::RenderStyle);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4171:impl<'ast, 'alloc> TemplateCodeGen<'alloc> for SsrCodeGen<'ast, 'alloc> {
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4395:            out.add_ssr_import(SsrHelper::RenderList);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4440:                    out.add_ssr_import(SsrHelper::RenderVNode);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4793:                    out.add_ssr_import(SsrHelper::RenderSuspense);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4948:                    out.add_ssr_import(SsrHelper::RenderTeleport);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5029:            out.add_ssr_import(SsrHelper::RenderComponent);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5618:            out.add_ssr_import(SsrHelper::RenderSlot);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5733:            out.add_ssr_import(SsrHelper::Interpolate);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5771:                    out.add_ssr_import(SsrHelper::Interpolate);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:5792:                        out.add_ssr_import(SsrHelper::Interpolate);
crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6464:        out.add_ssr_import(SsrHelper::Interpolate);
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:551:pub enum SsrHelper {
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:570:impl SsrHelper {
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:595:/// Ordered lookup table for `SsrHelperFlags::to_imports()`.
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:596:const ALL_SSR: [SsrHelper; 16] = [
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:597:    SsrHelper::RenderAttrs,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:598:    SsrHelper::RenderList,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:599:    SsrHelper::RenderComponent,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:600:    SsrHelper::RenderSlot,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:601:    SsrHelper::Interpolate,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:602:    SsrHelper::RenderAttr,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:603:    SsrHelper::GetDynamicModelProps,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:604:    SsrHelper::RenderTeleport,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:605:    SsrHelper::RenderVNode,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:606:    SsrHelper::RenderClass,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:607:    SsrHelper::RenderStyle,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:608:    SsrHelper::IncludeBooleanAttr,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:609:    SsrHelper::RenderSuspense,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:610:    SsrHelper::GetDirectiveProps,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:611:    SsrHelper::LooseContain,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:612:    SsrHelper::LooseEqual,
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:617:pub struct SsrHelperFlags(pub u32);
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:619:impl SsrHelperFlags {
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:637:    pub const fn has(self, h: SsrHelper) -> bool {
crates/verter_compiler/src/template/code_gen/shared/helpers.rs:643:    pub const fn add(self, h: SsrHelper) -> Self {
crates/verter_compiler/src/compile_tests.rs:19369:            backend: VueBackendRequest::Vapor,
crates/verter_compiler/src/template/code_gen/types.rs:14:    BuiltinComponentFlags, SsrHelper, SsrHelperFlags, VaporHelper, VaporHelperFlags, VdomHelper,
crates/verter_compiler/src/template/code_gen/types.rs:89:    ssr_imports: SsrHelperFlags,
crates/verter_compiler/src/template/code_gen/types.rs:150:            ssr_imports: SsrHelperFlags::empty(),
crates/verter_compiler/src/template/code_gen/types.rs:606:    pub fn add_ssr_import(&mut self, h: SsrHelper) {
crates/verter_compiler/src/compile_request/capability.rs:19:    VueSsr,
crates/verter_compiler/src/compile_request/capability.rs:20:    VueSsrVaporBackend,
crates/verter_compiler/src/compile_request/capability.rs:93:            VueSsr => Supported,
crates/verter_compiler/src/compile_request/capability.rs:94:            VueSsrVaporBackend => UnsupportedFailClosed,
crates/verter_compiler/src/compile_request/capability.rs:149:        CapabilityCell::VueSsr,
crates/verter_compiler/src/compile_request/capability.rs:150:        CapabilityCell::VueSsrVaporBackend,
crates/verter_compiler/src/compile_request/vue.rs:62:    TransformOptionsSsrCssVars,
crates/verter_compiler/src/compile_request/vue.rs:68:    SharedTransformCodegenOptionsSsr,
crates/verter_compiler/src/compile_request/vue.rs:69:    SharedTransformCodegenOptionsInSsr,
crates/verter_compiler/src/compile_request/vue.rs:81:    CodegenOptionsSsrRuntimeModuleName,
crates/verter_compiler/src/compile_request/vue.rs:117:    CompileTemplateSsr,
crates/verter_compiler/src/compile_request/vue.rs:118:    CompileTemplateSsrCssVars,
crates/verter_compiler/src/compile_request/vue.rs:232:            TransformOptionsSsrCssVars => Derived,
crates/verter_compiler/src/compile_request/vue.rs:237:            SharedTransformCodegenOptionsSsr => Derived,
crates/verter_compiler/src/compile_request/vue.rs:238:            SharedTransformCodegenOptionsInSsr => Derived,
crates/verter_compiler/src/compile_request/vue.rs:249:            CodegenOptionsSsrRuntimeModuleName => SupportedCanonical,
crates/verter_compiler/src/compile_request/vue.rs:282:            CompileTemplateSsr => Derived,
crates/verter_compiler/src/compile_request/vue.rs:283:            CompileTemplateSsrCssVars => Derived,
crates/verter_compiler/src/compile_request/vue.rs:369:        TransformOptionsSsrCssVars,
crates/verter_compiler/src/compile_request/vue.rs:373:        SharedTransformCodegenOptionsSsr,
crates/verter_compiler/src/compile_request/vue.rs:374:        SharedTransformCodegenOptionsInSsr,
crates/verter_compiler/src/compile_request/vue.rs:384:        CodegenOptionsSsrRuntimeModuleName,
crates/verter_compiler/src/compile_request/vue.rs:414:        CompileTemplateSsr,
crates/verter_compiler/src/compile_request/vue.rs:415:        CompileTemplateSsrCssVars,
crates/verter_compiler/src/compile_request/vue.rs:530:    pub backend: VueBackendRequest,
crates/verter_compiler/src/compile_request/vue.rs:574:    pub backend: VueBackendRequest,
crates/verter_compiler/src/compile_request/vue.rs:778:            backend: VueBackendRequest::Vapor,
crates/verter_compiler/src/compile_request/mod.rs:70:    SsrVaporBackendUnsupported,
crates/verter_compiler/src/compile_request/mod.rs:74:    InlineSsrUnsupported,
crates/verter_compiler/src/compile_request/mod.rs:178:                return Err(CompileRequestError::SsrVaporBackendUnsupported);
crates/verter_compiler/src/compile_request/mod.rs:181:                return Err(CompileRequestError::InlineSsrUnsupported);
crates/verter_compiler/src/compile_request/mod.rs:189:            return Err(CompileRequestError::InlineSsrUnsupported);
crates/verter_compiler/src/compile_request/mod.rs:298:            return Err(CompileRequestError::SsrVaporBackendUnsupported);
crates/verter_compiler/src/compile_request/mod.rs:444:    fn vue_req(backend: VueBackendRequest) -> FrameworkCompileRequest {
crates/verter_compiler/src/compile_request/mod.rs:462:        assert_eq!(err, CompileRequestError::SsrVaporBackendUnsupported);
crates/verter_compiler/src/compile_request/mod.rs:487:        assert_eq!(err, CompileRequestError::SsrVaporBackendUnsupported);
crates/verter_compiler/src/compile_request/mod.rs:516:        assert_eq!(err, CompileRequestError::InlineSsrUnsupported);
crates/verter_compiler/src/svelte/carrier.rs:508:                crate::compile_request::CompileRequestError::InlineSsrUnsupported,
crates/verter_compiler/src/svelte/carrier.rs:964:        // non-Vue `InlineSsrUnsupported` refusal.
crates/verter_compiler/src/svelte/carrier.rs:982:                    crate::compile_request::CompileRequestError::InlineSsrUnsupported
crates/verter_compiler/src/svelte/carrier.rs:985:            "expected a typed InlineSsrUnsupported refusal, got {result:?}"
crates/verter_compiler/tests/cases/capability_matrix_compile_request_coverage.rs:137:                    backend: VueBackendRequest::Vdom,
crates/verter_compiler/tests/cases/capability_matrix_compile_request_coverage.rs:153:                    backend: VueBackendRequest::Vapor,
crates/verter_compiler/tests/cases/capability_matrix_compile_request_coverage.rs:171:                    backend: VueBackendRequest::Vdom,
crates/verter_compiler/tests/cases/capability_matrix_compile_request_coverage.rs:190:                    backend: VueBackendRequest::Vapor,
crates/verter_compiler/tests/cases/capability_matrix_compile_request_coverage.rs:202:                CompileRequestError::SsrVaporBackendUnsupported,
crates/verter_compiler/tests/cases/capability_matrix_compile_request_coverage.rs:213:                    backend: VueBackendRequest::Inferred,
crates/verter_compiler/tests/cases/capability_matrix_compile_request_coverage.rs:225:                Err(CompileRequestError::SsrVaporBackendUnsupported),

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "ls target/debug/deps/main-* 2>/dev/null | head -n 10 && ls target/debug/deps/verter_compiler-* 2>/dev/null | head -n 20 && rg -n \"fn compile_sfc_ssr|fn gen_ssr_template|TemplateCodeGenOptions\" crates/verter_compiler/src/template/code_gen/ssr/tests.rs | head -n 40 && nl -ba crates/verter_compiler/src/template/code_gen/ssr/tests.rs | sed -n '1,90p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 743ms:[0m
target/debug/deps/main-05dc8dcfcfc5855f.d
target/debug/deps/main-0686d4e98cd37c35.d
target/debug/deps/main-0b39c5eaf79128a0
target/debug/deps/main-0b39c5eaf79128a0.05ume7qfte1dqz3y3xyxyj3zf.0leby8j.rcgu.o
target/debug/deps/main-0b39c5eaf79128a0.06h6kzb8jod18xyp4803dlr2m.0leby8j.rcgu.o
target/debug/deps/main-0b39c5eaf79128a0.0ajqd76ehexvtdg7wwe3ook5h.0leby8j.rcgu.o
target/debug/deps/main-0b39c5eaf79128a0.0aqttqhrd60nyn8ctxhhtawa4.0leby8j.rcgu.o
target/debug/deps/main-0b39c5eaf79128a0.0e3fxx7fv4n1xm1to8kypkjzy.0leby8j.rcgu.o
target/debug/deps/main-0b39c5eaf79128a0.0epq8pjtqyq5rjobpbcouexk7.0leby8j.rcgu.o
target/debug/deps/main-0b39c5eaf79128a0.0f4ig5b2y8pzjzb6xe369x86d.0leby8j.rcgu.o
target/debug/deps/verter_compiler-037bdb6b37229510.d
target/debug/deps/verter_compiler-0550a7c477b20c3e
target/debug/deps/verter_compiler-0550a7c477b20c3e.006cj5ujratl3y57siflxpw1f.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.00a0owgudpy03idb8594tae8u.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.07yjswsfr8go3k9m85a35o4nv.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0a3d1zcz7q48q88cq096c3mcu.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0auaea0c2ht5cehpvi3wykn2i.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0budsksh7lkqz6zi4pj2ezvvq.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0c9crdwuy4hlzoj6g27zjjqg0.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0eexcsmc85txdj5i8jp8rs2x6.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0ljdqfe96csuldfr4dje9zwhg.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0riqfontmedk9qkxifmp0ahhu.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0rs64vxcgbqajdcqnxjz3rf6f.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0t1y4fi03104z6xp2fl8db1r7.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0t71mr1ikcxejgnq5x5olnbog.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0tbuw2fqlukbp9gc6ywdp67ro.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0u3c7pmnzenw0ub5fity222ff.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0uidqwamqgghu5p91s5eepy9o.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0urb0mlljzk2phkfb0hjtrz9h.0j3ha66.rcgu.o
target/debug/deps/verter_compiler-0550a7c477b20c3e.0usw44cssibhwz8pampa91mr4.0j3ha66.rcgu.o
11:fn compile_sfc_ssr(source: &str) -> VerterCompileResult {
15:fn compile_sfc_ssr_with_semantics(
33:fn gen_ssr_template(source: &str) -> String {
38:fn gen_ssr_template_with_runtime(
     1	//! SSR codegen tests.
     2	//!
     3	//! Each test validates the SSR string-concatenation output against
     4	//! the patterns produced by Vue's `@vue/compiler-ssr`.
     5	
     6	use oxc_allocator::Allocator;
     7	
     8	use crate::compile::legacy_test_support::{compile, CodegenOptions, VerterCompileOptions};
     9	use crate::compile::{VerterCompileResult, VueMacroSemanticInput};
    10	
    11	fn compile_sfc_ssr(source: &str) -> VerterCompileResult {
    12	    compile_sfc_ssr_with_semantics(source, &VueMacroSemanticInput::Unavailable)
    13	}
    14	
    15	fn compile_sfc_ssr_with_semantics(
    16	    source: &str,
    17	    macro_semantics: &VueMacroSemanticInput,
    18	) -> VerterCompileResult {
    19	    let alloc = Allocator::new();
    20	    let options = CodegenOptions {
    21	        filename: Some("App.vue".to_string()),
    22	        ..Default::default()
    23	    };
    24	    let verter_opts = VerterCompileOptions {
    25	        force_js: true,
    26	        ssr: true,
    27	        ..Default::default()
    28	    };
    29	    compile(source, &options, &verter_opts, macro_semantics, &alloc)
    30	}
    31	
    32	/// Helper: compile and return the template code, asserting no errors.
    33	fn gen_ssr_template(source: &str) -> String {
    34	    let result = compile_sfc_ssr(source);
    35	    ssr_template_code(result)
    36	}
    37	
    38	fn gen_ssr_template_with_runtime(
    39	    source: &str,
    40	    runtime: std::sync::Arc<verter_macro_dto::MacroRuntimeBundle>,
    41	) -> String {
    42	    let result = compile_sfc_ssr_with_semantics(source, &VueMacroSemanticInput::Runtime(runtime));
    43	    ssr_template_code(result)
    44	}
    45	
    46	fn ssr_template_code(result: VerterCompileResult) -> String {
    47	    assert!(
    48	        result.errors.is_empty(),
    49	        "compile errors: {:?}",
    50	        result.errors
    51	    );
    52	    let tpl = result
    53	        .template
    54	        .as_ref()
    55	        .expect("should have template block");
    56	    tpl.code.clone()
    57	}
    58	
    59	/// Helper: compile and return the script code, asserting no errors.
    60	fn gen_ssr_script(source: &str) -> String {
    61	    let result = compile_sfc_ssr(source);
    62	    ssr_script_code(result)
    63	}
    64	
    65	fn ssr_script_code(result: VerterCompileResult) -> String {
    66	    assert!(
    67	        result.errors.is_empty(),
    68	        "compile errors: {:?}",
    69	        result.errors
    70	    );
    71	    let script = result.script.as_ref().expect("should have script block");
    72	    script.code.clone()
    73	}
    74	
    75	// ══════════════════════════════════════════════════════════════════
    76	// Basic element rendering
    77	// ══════════════════════════════════════════════════════════════════
    78	
    79	#[test]
    80	fn ssr_single_element() {
    81	    let code = gen_ssr_template("<template><div>hello</div></template>");
    82	    assert!(
    83	        code.contains("function ssrRender("),
    84	        "should have ssrRender function signature, got:\n{}",
    85	        code
    86	    );
    87	    assert!(
    88	        code.contains("_push("),
    89	        "should use _push(), got:\n{}",
    90	        code

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "node -e \"for (const p of ['@vue/compiler-sfc/package.json','@vue/compiler-ssr/package.json']) { try { const x=require(p); process.stdout.write(p+' '+x.version+'\\n') } catch(e){ process.stdout.write(p+' MISSING\\n') } }\" && rg -n 'vuejs/core|compiler-ssr|vue@' crates/verter_vue_conformance package.json pnpm-lock.yaml | head -n 120"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 425ms:[0m
@vue/compiler-sfc/package.json 3.5.34
@vue/compiler-ssr/package.json MISSING
pnpm-lock.yaml:377:        version: 6.0.7(vite@8.0.14(@types/node@25.9.1)(esbuild@0.28.0)(jiti@2.7.0)(less@4.6.4)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(vue@3.5.34(typescript@6.0.3))
pnpm-lock.yaml:4993:  '@unhead/vue@2.1.9':
pnpm-lock.yaml:5013:  '@vitejs/plugin-vue@5.2.4':
pnpm-lock.yaml:5020:  '@vitejs/plugin-vue@6.0.5':
pnpm-lock.yaml:5027:  '@vitejs/plugin-vue@6.0.7':
pnpm-lock.yaml:5207:  '@vue/compiler-ssr@3.5.34':
pnpm-lock.yaml:5210:  '@vue/compiler-ssr@3.6.0-beta.6':
pnpm-lock.yaml:5213:  '@vue/compiler-ssr@3.6.0-rc.3':
pnpm-lock.yaml:11308:  vue@3.5.34:
pnpm-lock.yaml:11316:  vue@3.6.0-beta.6:
pnpm-lock.yaml:11324:  vue@3.6.0-rc.3:
pnpm-lock.yaml:13527:  '@nuxt/devtools@3.2.2(vite@8.0.14(@types/node@25.9.1)(esbuild@0.28.0)(jiti@2.6.1)(less@4.6.4)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(vue@3.5.34(typescript@6.0.3))':
pnpm-lock.yaml:13532:      '@vue/devtools-core': 8.0.6(vite@8.0.14(@types/node@25.9.1)(esbuild@0.28.0)(jiti@2.6.1)(less@4.6.4)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(vue@3.5.34(typescript@6.0.3))
pnpm-lock.yaml:13559:      vite-plugin-vue-tracer: 1.2.0(vite@8.0.14(@types/node@25.9.1)(esbuild@0.28.0)(jiti@2.6.1)(less@4.6.4)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(vue@3.5.34(typescript@6.0.3))
pnpm-lock.yaml:13622:      '@unhead/vue': 2.1.9(vue@3.5.34(typescript@6.0.3))
pnpm-lock.yaml:13700:  '@nuxt/vite-builder@4.3.1(@types/node@25.9.1)(less@4.6.4)(lightningcss@1.32.0)(magicast@0.5.2)(nuxt@4.3.1(@azure/identity@4.13.1)(@parcel/watcher@2.5.6)(@types/node@25.9.1)(@vue/compiler-sfc@3.6.0-rc.3)(cac@6.7.14)(db0@0.3.4)(ioredis@5.9.3)(less@4.6.4)(lightningcss@1.32.0)(magicast@0.5.2)(rolldown@1.0.2)(rollup@4.57.1)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(typescript@6.0.3)(vite@8.0.14(@types/node@25.9.1)(esbuild@0.28.0)(jiti@2.6.1)(less@4.6.4)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(yaml@2.8.2))(rolldown@1.0.2)(rollup@4.57.1)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(typescript@6.0.3)(vue@3.5.34(typescript@6.0.3))(yaml@2.8.2)':
pnpm-lock.yaml:13704:      '@vitejs/plugin-vue': 6.0.5(vite@7.3.1(@types/node@25.9.1)(jiti@2.6.1)(less@4.6.4)(lightningcss@1.32.0)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(vue@3.5.34(typescript@6.0.3))
pnpm-lock.yaml:13705:      '@vitejs/plugin-vue-jsx': 5.1.4(vite@7.3.1(@types/node@25.9.1)(jiti@2.6.1)(less@4.6.4)(lightningcss@1.32.0)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(vue@3.5.34(typescript@6.0.3))
pnpm-lock.yaml:15507:  '@unhead/vue@2.1.9(vue@3.5.34(typescript@6.0.3))':
pnpm-lock.yaml:15537:  '@vitejs/plugin-vue-jsx@5.1.4(vite@7.3.1(@types/node@25.9.1)(jiti@2.6.1)(less@4.6.4)(lightningcss@1.32.0)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(vue@3.5.34(typescript@6.0.3))':
pnpm-lock.yaml:15549:  '@vitejs/plugin-vue@5.2.4(vite@5.4.21(@types/node@25.9.1)(less@4.6.4)(lightningcss@1.32.0)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0))(vue@3.5.34(typescript@7.0.2))':
pnpm-lock.yaml:15554:  '@vitejs/plugin-vue@6.0.5(vite@7.3.1(@types/node@25.9.1)(jiti@2.6.1)(less@4.6.4)(lightningcss@1.32.0)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(vue@3.5.34(typescript@6.0.3))':
pnpm-lock.yaml:15560:  '@vitejs/plugin-vue@6.0.7(vite@8.0.14(@types/node@25.9.1)(esbuild@0.28.0)(jiti@2.7.0)(less@4.6.4)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(vue@3.5.34(typescript@6.0.3))':
pnpm-lock.yaml:15762:  '@vue-macros/common@3.1.2(vue@3.5.34(typescript@6.0.3))':
pnpm-lock.yaml:15845:      '@vue/compiler-ssr': 3.5.34
pnpm-lock.yaml:15857:      '@vue/compiler-ssr': 3.6.0-beta.6
pnpm-lock.yaml:15870:      '@vue/compiler-ssr': 3.6.0-rc.3
pnpm-lock.yaml:15878:  '@vue/compiler-ssr@3.5.34':
pnpm-lock.yaml:15883:  '@vue/compiler-ssr@3.6.0-beta.6':
pnpm-lock.yaml:15888:  '@vue/compiler-ssr@3.6.0-rc.3':
pnpm-lock.yaml:15915:  '@vue/devtools-core@8.0.6(vite@8.0.14(@types/node@25.9.1)(esbuild@0.28.0)(jiti@2.6.1)(less@4.6.4)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(vue@3.5.34(typescript@6.0.3))':
pnpm-lock.yaml:16068:  '@vue/server-renderer@3.5.34(vue@3.5.34(typescript@6.0.3))':
pnpm-lock.yaml:16070:      '@vue/compiler-ssr': 3.5.34
pnpm-lock.yaml:16074:  '@vue/server-renderer@3.5.34(vue@3.5.34(typescript@7.0.2))':
pnpm-lock.yaml:16076:      '@vue/compiler-ssr': 3.5.34
pnpm-lock.yaml:16080:  '@vue/server-renderer@3.6.0-beta.6(vue@3.6.0-beta.6(typescript@6.0.3))':
pnpm-lock.yaml:16082:      '@vue/compiler-ssr': 3.6.0-beta.6
pnpm-lock.yaml:16088:      '@vue/compiler-ssr': 3.6.0-rc.3
pnpm-lock.yaml:19681:      '@nuxt/devtools': 3.2.2(vite@8.0.14(@types/node@25.9.1)(esbuild@0.28.0)(jiti@2.6.1)(less@4.6.4)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(vue@3.5.34(typescript@6.0.3))
pnpm-lock.yaml:19686:      '@nuxt/vite-builder': 4.3.1(@types/node@25.9.1)(less@4.6.4)(lightningcss@1.32.0)(magicast@0.5.2)(nuxt@4.3.1(@azure/identity@4.13.1)(@parcel/watcher@2.5.6)(@types/node@25.9.1)(@vue/compiler-sfc@3.6.0-rc.3)(cac@6.7.14)(db0@0.3.4)(ioredis@5.9.3)(less@4.6.4)(lightningcss@1.32.0)(magicast@0.5.2)(rolldown@1.0.2)(rollup@4.57.1)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(typescript@6.0.3)(vite@8.0.14(@types/node@25.9.1)(esbuild@0.28.0)(jiti@2.6.1)(less@4.6.4)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(yaml@2.8.2))(rolldown@1.0.2)(rollup@4.57.1)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(typescript@6.0.3)(vue@3.5.34(typescript@6.0.3))(yaml@2.8.2)
pnpm-lock.yaml:19687:      '@unhead/vue': 2.1.9(vue@3.5.34(typescript@6.0.3))
pnpm-lock.yaml:19732:      unplugin-vue-router: 0.19.2(@vue/compiler-sfc@3.6.0-rc.3)(vue-router@4.6.4(vue@3.5.34(typescript@6.0.3)))(vue@3.5.34(typescript@6.0.3))
pnpm-lock.yaml:19735:      vue-router: 4.6.4(vue@3.5.34(typescript@6.0.3))
pnpm-lock.yaml:22173:  unplugin-vue-router@0.19.2(@vue/compiler-sfc@3.6.0-rc.3)(vue-router@4.6.4(vue@3.5.34(typescript@6.0.3)))(vue@3.5.34(typescript@6.0.3)):
pnpm-lock.yaml:22176:      '@vue-macros/common': 3.1.2(vue@3.5.34(typescript@6.0.3))
pnpm-lock.yaml:22194:      vue-router: 4.6.4(vue@3.5.34(typescript@6.0.3))
pnpm-lock.yaml:22389:  vite-plugin-vue-tracer@1.2.0(vite@8.0.14(@types/node@25.9.1)(esbuild@0.28.0)(jiti@2.6.1)(less@4.6.4)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0)(tsx@4.22.3)(yaml@2.8.2))(vue@3.5.34(typescript@6.0.3)):
pnpm-lock.yaml:22487:      '@vitejs/plugin-vue': 5.2.4(vite@5.4.21(@types/node@25.9.1)(less@4.6.4)(lightningcss@1.32.0)(sass-embedded@1.100.0)(sass@1.100.0)(terser@5.48.0))(vue@3.5.34(typescript@7.0.2))
pnpm-lock.yaml:22681:  vue-router@4.6.4(vue@3.5.34(typescript@6.0.3)):
pnpm-lock.yaml:22686:  vue@3.5.34(typescript@6.0.3):
pnpm-lock.yaml:22691:      '@vue/server-renderer': 3.5.34(vue@3.5.34(typescript@6.0.3))
pnpm-lock.yaml:22696:  vue@3.5.34(typescript@7.0.2):
pnpm-lock.yaml:22701:      '@vue/server-renderer': 3.5.34(vue@3.5.34(typescript@7.0.2))
pnpm-lock.yaml:22706:  vue@3.6.0-beta.6(typescript@6.0.3):
pnpm-lock.yaml:22712:      '@vue/server-renderer': 3.6.0-beta.6(vue@3.6.0-beta.6(typescript@6.0.3))
pnpm-lock.yaml:22717:  vue@3.6.0-rc.3(typescript@7.0.2):
crates/verter_vue_conformance/tests/cases/generator_smoke.rs:329:    // Bare `vue@<pin>:` resolved package key (no peer suffix, no path entry).
crates/verter_vue_conformance/tests/cases/generator_smoke.rs:332:            .strip_prefix("vue@")
crates/verter_vue_conformance/tests/cases/generator_smoke.rs:336:    assert!(vue_resolved, "lockfile must resolve vue@{vue_pin}");

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg --files node_modules/.pnpm | rg '@vue\\+compiler-ssr@3\\.6\\.0-rc\\.3/.*/package.json"'$|@vue'"\\+compiler-sfc@3\\.6\\.0-rc\\.3/.*/package.json"'$'"' | head -n 20"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 319ms:[0m
[35m[3mexec[0m[0m
[1m/bin/zsh -lc "ls node_modules/.pnpm | rg '"'^@vue'"\\+compiler-(ssr|sfc)@3\\.6\\.0-rc\\.3' | head -n 20"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
@vue+compiler-sfc@3.6.0-rc.3
@vue+compiler-ssr@3.6.0-rc.3

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "node -e 'const c=require(\"./node_modules/.pnpm/@vue+compiler-ssr@3.6.0-rc.3/node_modules/@vue/compiler-ssr\"); for (const comments of [true,false]) for (const src of [\"<"'!--c-->","<!--c--><div>x</div>"]) { try { const r=c.compile(src,{mode:"module",comments}); console.log(JSON.stringify({comments,src,code:r.code})) } catch(e) { console.log(JSON.stringify({comments,src,error:String(e)})) } }'"'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 455ms:[0m
{"comments":true,"src":"<!--c-->","code":"\nexport function ssrRender(_ctx, _push, _parent, _attrs) {\n  _push(`<!--c-->`)\n}"}
{"comments":true,"src":"<!--c--><div>x</div>","code":"import { ssrRenderAttrs as _ssrRenderAttrs } from \"vue/server-renderer\"\n\nexport function ssrRender(_ctx, _push, _parent, _attrs) {\n  _push(`<!--[--><!--c--><div${_ssrRenderAttrs(_attrs)}>x</div><!--]-->`)\n}"}
{"comments":false,"src":"<!--c-->","code":"\nexport function ssrRender(_ctx, _push, _parent, _attrs) {\n  \n}"}
{"comments":false,"src":"<!--c--><div>x</div>","code":"import { ssrRenderAttrs as _ssrRenderAttrs } from \"vue/server-renderer\"\n\nexport function ssrRender(_ctx, _push, _parent, _attrs) {\n  _push(`<div${_ssrRenderAttrs(_attrs)}>x</div>`)\n}"}
(node:7561) Warning: The 'NO_COLOR' env is ignored due to the 'FORCE_COLOR' env being set.
(Use `node --trace-warnings ...` to show where the warning was created)

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "find target/debug -maxdepth 1 -type f -perm -111 -print | sort | head -n 80 && rg -n \"name = \\\"verter_compiler\\\"|\\[\\[bin\\]\\]|\\[\\[example\\]\\]\" crates/verter_compiler/Cargo.toml Cargo.toml | head -n 80"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 45ms:[0m
crates/verter_compiler/Cargo.toml:2:name = "verter_compiler"
crates/verter_compiler/Cargo.toml:111:[[bin]]

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_compiler/Cargo.toml | sed -n '100,130p' && nl -ba crates/verter_compiler/src/compile/mod.rs | sed -n '1200,1235p;1595,1630p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 0ms:[0m
   100	
   101	[dev-dependencies]
   102	dhat = "0.3.3"
   103	proc-macro2 = { workspace = true }
   104	quote = { workspace = true }
   105	syn = { workspace = true }
   106	tempfile = "3.27"
   107	serde = { workspace = true }
   108	serde_json = { workspace = true }
   109	trybuild = "1.0"
   110	
   111	[[bin]]
   112	name = "parse_corpus_probe"
   113	path = "src/bin/parse_corpus_probe.rs"
   114	required-features = ["external-corpus"]
  1200	        // codegen itself. All template-expression source mappings stay on the
  1201	        // single shared CT, so the merged module keeps full map fidelity.
  1202	        let mut inline_tpl_imports: Vec<&'static str> = Vec::new();
  1203	        if inline_active && parsed.template_ast().is_some() {
  1204	            let template_ast = parsed
  1205	                .template_ast()
  1206	                .expect("inline_active requires a template");
  1207	            // Reuse the runtime overlay entry (same parse facts as the
  1208	            // used-vars lane above; completion-prefix matching off).
  1209	            let oxc_ast = expr_store.get_or_build(
  1210	                template_ast,
  1211	                input,
  1212	                allocator,
  1213	                template_region_span(template_ast),
  1214	                &parse_options,
  1215	                source_type,
  1216	                false,
  1217	            );
  1218	            collect_expression_errors(oxc_ast, input, &mut all_diagnostics, &mut compile_failures);
  1219	
  1220	            let tpl_options = TemplateCodeGenOptions {
  1221	                mode: CodeGenMode::Vdom,
  1222	                is_inline: true,
  1223	                is_production: options.is_production,
  1224	                comments: options.comments.unwrap_or(!options.is_production),
  1225	                force_js: verter_options.force_js,
  1226	                self_name: to_pascal_case(&component_name),
  1227	                const_props: verter_options.prop_constness_overrides.clone(),
  1228	                has_script: true,
  1229	                ref_bindable_imports: ref_bindable_imports.clone(),
  1230	                has_scoped_style,
  1231	                hoist_static: options.resolve_hoist_static(),
  1232	                scope_id: if has_scoped_style {
  1233	                    scope_id_full.clone()
  1234	                } else {
  1235	                    String::new()
  1595	
  1596	                let ssr_css_vars = if verter_options.ssr {
  1597	                    // Dedup by var_name (same v-bind may appear in multiple style blocks)
  1598	                    let mut seen = rustc_hash::FxHashSet::default();
  1599	                    all_v_bind_vars
  1600	                        .iter()
  1601	                        .filter(|v| seen.insert(v.var_name.clone()))
  1602	                        .map(|v| (v.var_name.clone(), v.expression.clone()))
  1603	                        .collect()
  1604	                } else {
  1605	                    Vec::new()
  1606	                };
  1607	                let tpl_options = TemplateCodeGenOptions {
  1608	                    mode: if verter_options.ssr {
  1609	                        CodeGenMode::Ssr
  1610	                    } else if use_vapor {
  1611	                        CodeGenMode::Vapor
  1612	                    } else {
  1613	                        CodeGenMode::Vdom
  1614	                    },
  1615	                    is_inline: verter_options.runtime_inline_template_chunk,
  1616	                    is_production: options.is_production,
  1617	                    comments: options.comments.unwrap_or(!options.is_production),
  1618	                    force_js: verter_options.force_js,
  1619	                    self_name: to_pascal_case(&component_name),
  1620	                    const_props: verter_options
  1621	                        .prop_constness_overrides
  1622	                        .clone()
  1623	                        .or_else(|| transferred_bindings.and_then(|m| m.const_props.clone())),
  1624	                    // Full 6-param render signature only when the SFC has a
  1625	                    // script block (official: `bindingMetadata && !inline`).
  1626	                    has_script: parsed.script().is_some()
  1627	                        || parsed.script_setup().is_some()
  1628	                        || transferred_bindings.is_some_and(|metadata| metadata.has_script),
  1629	                    ref_bindable_imports: ref_bindable_imports.clone(),
  1630	                    has_scoped_style,

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n \"CodeGenOutput::|\\.apply_to\\(|CodeGenOutput\" crates/verter_compiler/src/template crates/verter_compiler/src/svelte crates/verter_compiler/src/ide | head -n 220 && rg -n \"apply_to\\(\" crates/verter_compiler/src/template/code_gen -g '*.rs'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 21ms:[0m
crates/verter_compiler/src/template/code_gen/binding_tests.rs:210:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/binding_tests.rs:239:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/binding_tests.rs:267:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/binding_tests.rs:293:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/binding_tests.rs:327:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/directives.rs:17:use super::super::types::{CodeGenOutput, ConditionBranchClose, ScopeClose};
crates/verter_compiler/src/template/code_gen/vdom/directives.rs:397:pub fn collect_scope_imports(close: &ScopeClose, out: &mut CodeGenOutput<'_>) {
crates/verter_compiler/src/template/code_gen/vdom/directives.rs:737:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/directives.rs:745:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/directives.rs:771:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/directives.rs:782:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:81:    ChildKind, ChildRecord, CodeGenOutput, ConditionChainRole, MappedGeneratedText, ScopeClose,
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:759:        _out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:826:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1157:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1397:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1714:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1734:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1746:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/ide/script/setup.rs:23:use crate::template::code_gen::types::CodeGenOutput;
crates/verter_compiler/src/ide/script/setup.rs:47:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/text.rs:4://! All mutations go through `CodeGenOutput` overwrites.
crates/verter_compiler/src/template/code_gen/vdom/text.rs:21:use super::super::types::{ChildKind, ChildRecord, CodeGenOutput};
crates/verter_compiler/src/template/code_gen/vdom/text.rs:26:/// modify `CodeGenOutput`. Used by `build_child_records` to construct child
crates/verter_compiler/src/template/code_gen/vdom/text.rs:66:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/text.rs:132:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/text.rs:144:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/text.rs:156:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/text.rs:167:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/text.rs:180:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/text.rs:192:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/text.rs:209:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/text.rs:224:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/text.rs:237:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/text.rs:251:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/text.rs:264:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/text.rs:277:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/text.rs:292:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/interpolation.rs:7://! resolver and pushed into `CodeGenOutput.prepends`.
crates/verter_compiler/src/template/code_gen/vdom/interpolation.rs:13:use super::super::types::{ChildKind, ChildRecord, CodeGenOutput};
crates/verter_compiler/src/template/code_gen/vdom/interpolation.rs:27:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/interpolation.rs:83:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/interpolation.rs:115:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/interpolation.rs:141:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/interpolation.rs:177:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/interpolation.rs:213:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/interpolation.rs:239:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/ide/script/options_api.rs:23:use crate::template::code_gen::types::CodeGenOutput;
crates/verter_compiler/src/ide/script/options_api.rs:50:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/ide/script/options_api.rs:162:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/binding.rs:8:use super::types::CodeGenOutput;
crates/verter_compiler/src/template/code_gen/binding.rs:412:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/walker.rs:10:use super::types::CodeGenOutput;
crates/verter_compiler/src/template/code_gen/walker.rs:32:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/walker.rs:121:    use super::super::types::CodeGenOutput;
crates/verter_compiler/src/template/code_gen/walker.rs:140:            _out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/walker.rs:149:            _out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/walker.rs:160:            _out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/walker.rs:172:            _out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/walker.rs:182:            _out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/walker.rs:193:            _out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/walker.rs:203:            _out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/walker.rs:237:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/walker.rs:254:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/walker.rs:279:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/walker.rs:313:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/walker.rs:348:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/walker.rs:381:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/walker.rs:409:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/walker.rs:437:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:15:use super::super::types::{ChildKind, ChildRecord, CodeGenOutput, ConditionChainRole};
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:133:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:392:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:509:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:915:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:964:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:1092:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:1557:        out: &mut CodeGenOutput<'_>,
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:1610:        out: &mut CodeGenOutput<'_>,
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:1720:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/slots.rs:1934:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/mod.rs:4://! transform operations into a [`CodeGenOutput`]. The caller applies these
crates/verter_compiler/src/template/code_gen/mod.rs:20://! - [`types`] — [`CodeGenOutput`] accumulator and internal data structures.
crates/verter_compiler/src/template/code_gen/mod.rs:41:use self::types::{CodeGenOutput, TemplateImports};
crates/verter_compiler/src/template/code_gen/mod.rs:138:/// deferred operations into `CodeGenOutput` — no `CodeTransform` is passed.
crates/verter_compiler/src/template/code_gen/mod.rs:145:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/mod.rs:153:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/mod.rs:167:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/mod.rs:177:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/mod.rs:186:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/mod.rs:196:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/mod.rs:205:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/mod.rs:211:/// Accumulates all operations into `CodeGenOutput`, then batch-applies
crates/verter_compiler/src/template/code_gen/mod.rs:258:    let mut out = CodeGenOutput::new(alloc);
crates/verter_compiler/src/template/code_gen/mod.rs:275:    out.apply_to(ct)
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:94:fn apply_output<'a>(source: &str, out: CodeGenOutput<'a>, alloc: &'a Allocator) -> String {
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:96:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:105:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:130:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:157:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:198:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:254:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:310:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:2701:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:2704:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:2778:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:2781:    out.apply_to(&mut ct);
crates/verter_compiler/src/ide/script/macros.rs:10:use crate::template::code_gen::types::CodeGenOutput;
crates/verter_compiler/src/ide/script/macros.rs:63:    pub(super) out: &'a mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vapor/text.rs:9:use crate::template::code_gen::types::{CodeGenOutput, VaporElementState, VaporTextPart};
crates/verter_compiler/src/template/code_gen/vapor/text.rs:31:    out: &CodeGenOutput<'a>,
crates/verter_compiler/src/template/code_gen/vapor/text.rs:81:        let out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/text.rs:99:        let out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/text.rs:125:        let out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/text.rs:144:        let out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/text.rs:165:        let out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/text.rs:189:        let out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/text.rs:208:        let out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/children.rs:13:    ChildKind, ChildRecord, CodeGenOutput, ConditionChainRole, MappedGeneratedText,
crates/verter_compiler/src/template/code_gen/vdom/children.rs:28:    out: &mut CodeGenOutput<'_>,
crates/verter_compiler/src/template/code_gen/vdom/children.rs:45:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/children.rs:146:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/children.rs:193:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/children.rs:232:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/component.rs:12:use super::super::types::CodeGenOutput;
crates/verter_compiler/src/template/code_gen/vdom/component.rs:50:    out: &mut CodeGenOutput<'_>,
crates/verter_compiler/src/template/code_gen/vdom/component.rs:174:    out: &mut CodeGenOutput<'a>,
crates/verter_compiler/src/ide/script/mod.rs:60:use crate::template::code_gen::types::CodeGenOutput;
crates/verter_compiler/src/ide/script/mod.rs:152:    let mut out = CodeGenOutput::new(alloc);
crates/verter_compiler/src/ide/script/mod.rs:229:    out.apply_to(ct);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:14:use super::super::types::{ChildKind, ChildRecord, CodeGenOutput};
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:26:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:80:    fn apply_to_string<'a>(source: &str, out: CodeGenOutput<'a>, alloc: &'a Allocator) -> String {
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:82:        out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:91:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:113:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:127:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:143:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:160:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:180:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:193:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:208:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:221:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:236:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:251:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:268:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:285:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/ide/script/event_inference.rs:15:use crate::template::code_gen::types::CodeGenOutput;
crates/verter_compiler/src/ide/script/event_inference.rs:30:    out: &mut CodeGenOutput<'_>,
crates/verter_compiler/src/ide/script/event_inference.rs:81:    out: &mut CodeGenOutput<'_>,
crates/verter_compiler/src/template/code_gen/vapor/tests.rs:56:fn apply_output<'a>(source: &str, out: CodeGenOutput<'a>, alloc: &'a Allocator) -> String {
crates/verter_compiler/src/template/code_gen/vapor/tests.rs:58:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/vapor/tests.rs:839:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/tests.rs:879:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/tests.rs:972:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/tests.rs:1043:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:73:/// Apply CodeGenOutput to source and return the result string.
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:74:fn apply_output<'a>(source: &str, out: CodeGenOutput<'a>, alloc: &'a Allocator) -> String {
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:76:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:85:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:114:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:142:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:169:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:205:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:240:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:267:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:296:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:332:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:369:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:413:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:463:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:519:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:579:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:652:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:724:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:776:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:843:    let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/ide/script/wrapper.rs:17:use crate::template::code_gen::types::CodeGenOutput;
crates/verter_compiler/src/ide/script/wrapper.rs:337:    out: &mut CodeGenOutput<'_>,
crates/verter_compiler/src/template/code_gen/vdom/block.rs:12:use super::super::types::CodeGenOutput;
crates/verter_compiler/src/template/code_gen/vdom/block.rs:138:        out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/element.rs:24:use super::super::types::{ChildKind, ChildRecord, CodeGenOutput};
crates/verter_compiler/src/template/code_gen/vdom/element.rs:44:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/element.rs:134:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/element.rs:2246:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vdom/element.rs:2365:    out: &mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vapor/element.rs:14:    CodeGenOutput, VaporCounters, VaporEffect, VaporElementState, VaporRootElement,
crates/verter_compiler/src/template/code_gen/vapor/element.rs:153:    out: &mut CodeGenOutput<'_>,
crates/verter_compiler/src/template/code_gen/vapor/element.rs:241:    out: &mut CodeGenOutput<'a>,
crates/verter_compiler/src/template/code_gen/vapor/element.rs:519:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/element.rs:538:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/element.rs:566:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/element.rs:584:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/element.rs:606:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/element.rs:640:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:15:    CodeGenOutput, VaporCounters, VaporEffect, VaporElementState,
crates/verter_compiler/src/template/code_gen/vapor/props.rs:30:    pub out: &'a mut CodeGenOutput<'alloc>,
crates/verter_compiler/src/template/code_gen/vapor/props.rs:750:    out: &mut CodeGenOutput<'a>,
crates/verter_compiler/src/template/code_gen/vapor/props.rs:922:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:977:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1030:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1105:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1158:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1214:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1258:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1311:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1369:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1428:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1487:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1547:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1611:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1672:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1723:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1772:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1825:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1879:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1933:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/template/code_gen/vapor/props.rs:1990:        let mut out = CodeGenOutput::new(&alloc);
crates/verter_compiler/src/ide/script/type_constructs.rs:10:use crate::template::code_gen::types::CodeGenOutput;
crates/verter_compiler/src/ide/script/type_constructs.rs:279:    out: &mut CodeGenOutput<'_>,
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:96:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:2704:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/vdom/tests.rs:2781:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/vdom/element_tests.rs:76:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/mod.rs:275:    out.apply_to(ct)
crates/verter_compiler/src/template/code_gen/vdom/comment.rs:82:        out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types.rs:4://! Nothing is applied to the source until [`CodeGenOutput::apply_to()`] is called.
crates/verter_compiler/src/template/code_gen/types.rs:47:/// All operations are deferred — nothing is applied until [`apply_to()`](Self::apply_to).
crates/verter_compiler/src/template/code_gen/types.rs:639:    pub fn apply_to(mut self, ct: &mut CodeTransform<'alloc>) -> TemplateImports<'alloc> {
crates/verter_compiler/src/template/code_gen/vapor/tests.rs:58:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:44:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:58:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:96:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:132:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:160:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:188:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:210:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:251:    let imports = out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:266:    let imports = out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:279:    let imports = out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:537:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:553:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:592:    via_format.apply_to(&mut ct_a);
crates/verter_compiler/src/template/code_gen/types_tests.rs:594:    via_fmt.apply_to(&mut ct_b);
crates/verter_compiler/src/template/code_gen/types_tests.rs:615:    via_format.apply_to(&mut ct_a);
crates/verter_compiler/src/template/code_gen/types_tests.rs:617:    via_fmt.apply_to(&mut ct_b);
crates/verter_compiler/src/template/code_gen/types_tests.rs:642:    via_alloc.apply_to(&mut ct_alloc);
crates/verter_compiler/src/template/code_gen/types_tests.rs:644:    via_fmt.apply_to(&mut ct_fmt);
crates/verter_compiler/src/template/code_gen/types_tests.rs:670:    via_alloc.apply_to(&mut ct_alloc);
crates/verter_compiler/src/template/code_gen/types_tests.rs:672:    via_fmt.apply_to(&mut ct_fmt);
crates/verter_compiler/src/template/code_gen/types_tests.rs:750:    out.apply_to(&mut ct);
crates/verter_compiler/src/template/code_gen/types_tests.rs:864:    out.apply_to(&mut ct);

[35m[3mcodex[0m[0m
The investigator’s repair class is correct, with one sharpening: eliminate duplicate region ownership for every VDOM root-prefix replacement, not only the static-class/segmented branch.

1. Repair ownership

`visit_comment` currently emits the deletion immediately at [comment.rs:28](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vdom/comment.rs:28), before root finalization because of walker order at [walker.rs:98](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/walker.rs:98) and [walker.rs:106](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/walker.rs:106). Meanwhile disabled comments disappear from child bookkeeping at [vdom/mod.rs:709](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vdom/mod.rs:709), and the final root-prefix producer claims through `child.start` at [vdom/mod.rs:1072](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1072).

The structural implementation should therefore be VDOM-private:

- When comments are disabled, [visit_comment](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1741) records a pending removal intent instead of emitting an overwrite.
- [leave_template](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vdom/mod.rs:822), after determining the final effective root and exact prefix range, absorbs pending removals wholly contained by that replacement and emits ordinary deletions only for unclaimed interior/trailing comments.
- Remove the disabled-comment mutation branch from `process_comment`, making it preservation-only. That prevents the duplicate producer from reappearing.

This must not be conditional on static classes or segmented output. The root-prefix owner also subsumes comments under ordinary unmapped prefix replacements, restoring the stated cross-channel disjointness contract.

Reordering, narrowing, insertion conversion, and carrying original header chunks are inferior. After a superset replacement, the comment’s source coordinates no longer identify generated bytes; splitting around arbitrary comments fragments an atomic prefix and its anchor plan; carrying original bytes preserves content that should be absent. Those approaches encode the conflict instead of removing it.

2. `apply_to` is not the defect

The flush order should remain unchanged. [types.rs:696](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/types.rs:696) explicitly requires segmented overwrites to be disjoint from other channels. The VDOM producers violate that contract. The strict single-`Original` requirement at [segmented.rs:102](<MACHINE_ROOT>/verter/crates/verter_compiler/src/code_transform/segmented.rs:102) correctly detects the violation and must remain; the existing rejection test at [segmented_tests.rs:291](<MACHINE_ROOT>/verter/crates/verter_compiler/src/code_transform/segmented_tests.rs:291) should continue passing.

Thus neither the channel model nor `CodeTransform` needs semantic widening. A diagnostic preflight could be added separately, but it is not the repair.

3. Backend blast radius

- Vapor is unaffected. It omits disabled comments from its private HTML assembly at [vapor/comment.rs:14](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vapor/comment.rs:14) and emits one whole-template segmented replacement at [vapor/mod.rs:3352](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vapor/mod.rs:3352). There is no independent comment overwrite.
- SSR does not have the reported plain-overwrite/segmented-prefix collision for a nonempty root. However, the audit exposes a separate comment-only collision: comments do not count toward `effective_count` at [ssr/mod.rs:473](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/ssr/mod.rs:473), disabled comments enqueue their own segmented deletion at [ssr/mod.rs:6523](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6523), and the zero-effective-root branch enqueues a whole-template segmented replacement at [ssr/mod.rs:4289](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4289). Assign that exact sibling correction and its production/development controls to BV2 as well, using backend-local ownership—not global reordering.
- The selected repair touches neither `code_transform/` nor `CodeGenOutput::apply_to`. No special Svelte regression suite is therefore required beyond normal gates. A shared transform change would require Svelte coverage because Svelte directly depends on its affinity/edit semantics at [svelte/runtime/css/render.rs:8](<MACHINE_ROOT>/verter/crates/verter_compiler/src/svelte/runtime/css/render.rs:8), even though segmented authority itself excludes Svelte at [types.rs:18](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/types.rs:18).

4. Placement and program effect

BV2 is the correct owner. Vue emission semantics, comments, hoisting, and maps are Vue subsystem scope under [BV1.md:11](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/BV1.md:11). B5 explicitly forbids framework semantic repair at [B5.md:5](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/B5.md:5); B6 is likewise a lifecycle/reuse cutover, not a codegen repair owner.

No accepted ADR changes: the existing mapping taxonomy remains intact. No final program outcome changes: Vue compilation remains successful with truthful mappings. The DAG does change from the current direct `{BV1, BS1} -> B5` edge at [program-dag.toml:136](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/program-dag.toml:136) to `{BV1, BS1} -> BV2 -> B5`; B6 remains after B5. BV1’s accepted record remains historical and is not rewritten.

REPAIR: Eliminate duplicate ownership locally—defer disabled-comment removals, let the final VDOM root-prefix owner absorb contained removals, retain all strict segmented-overwrite preconditions and flush order; close the discovered SSR comment-only sibling locally in the same correction block.

PLACEMENT: BV2 is the sole owner, with predecessors BV1 and BS1 and B5 changed to depend on BV2; no accepted ADR or final program outcome changes.
[2mtokens used[0m
230 376
The investigator’s repair class is correct, with one sharpening: eliminate duplicate region ownership for every VDOM root-prefix replacement, not only the static-class/segmented branch.

1. Repair ownership

`visit_comment` currently emits the deletion immediately at [comment.rs:28](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vdom/comment.rs:28), before root finalization because of walker order at [walker.rs:98](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/walker.rs:98) and [walker.rs:106](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/walker.rs:106). Meanwhile disabled comments disappear from child bookkeeping at [vdom/mod.rs:709](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vdom/mod.rs:709), and the final root-prefix producer claims through `child.start` at [vdom/mod.rs:1072](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1072).

The structural implementation should therefore be VDOM-private:

- When comments are disabled, [visit_comment](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vdom/mod.rs:1741) records a pending removal intent instead of emitting an overwrite.
- [leave_template](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vdom/mod.rs:822), after determining the final effective root and exact prefix range, absorbs pending removals wholly contained by that replacement and emits ordinary deletions only for unclaimed interior/trailing comments.
- Remove the disabled-comment mutation branch from `process_comment`, making it preservation-only. That prevents the duplicate producer from reappearing.

This must not be conditional on static classes or segmented output. The root-prefix owner also subsumes comments under ordinary unmapped prefix replacements, restoring the stated cross-channel disjointness contract.

Reordering, narrowing, insertion conversion, and carrying original header chunks are inferior. After a superset replacement, the comment’s source coordinates no longer identify generated bytes; splitting around arbitrary comments fragments an atomic prefix and its anchor plan; carrying original bytes preserves content that should be absent. Those approaches encode the conflict instead of removing it.

2. `apply_to` is not the defect

The flush order should remain unchanged. [types.rs:696](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/types.rs:696) explicitly requires segmented overwrites to be disjoint from other channels. The VDOM producers violate that contract. The strict single-`Original` requirement at [segmented.rs:102](<MACHINE_ROOT>/verter/crates/verter_compiler/src/code_transform/segmented.rs:102) correctly detects the violation and must remain; the existing rejection test at [segmented_tests.rs:291](<MACHINE_ROOT>/verter/crates/verter_compiler/src/code_transform/segmented_tests.rs:291) should continue passing.

Thus neither the channel model nor `CodeTransform` needs semantic widening. A diagnostic preflight could be added separately, but it is not the repair.

3. Backend blast radius

- Vapor is unaffected. It omits disabled comments from its private HTML assembly at [vapor/comment.rs:14](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vapor/comment.rs:14) and emits one whole-template segmented replacement at [vapor/mod.rs:3352](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/vapor/mod.rs:3352). There is no independent comment overwrite.
- SSR does not have the reported plain-overwrite/segmented-prefix collision for a nonempty root. However, the audit exposes a separate comment-only collision: comments do not count toward `effective_count` at [ssr/mod.rs:473](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/ssr/mod.rs:473), disabled comments enqueue their own segmented deletion at [ssr/mod.rs:6523](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/ssr/mod.rs:6523), and the zero-effective-root branch enqueues a whole-template segmented replacement at [ssr/mod.rs:4289](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/ssr/mod.rs:4289). Assign that exact sibling correction and its production/development controls to BV2 as well, using backend-local ownership—not global reordering.
- The selected repair touches neither `code_transform/` nor `CodeGenOutput::apply_to`. No special Svelte regression suite is therefore required beyond normal gates. A shared transform change would require Svelte coverage because Svelte directly depends on its affinity/edit semantics at [svelte/runtime/css/render.rs:8](<MACHINE_ROOT>/verter/crates/verter_compiler/src/svelte/runtime/css/render.rs:8), even though segmented authority itself excludes Svelte at [types.rs:18](<MACHINE_ROOT>/verter/crates/verter_compiler/src/template/code_gen/types.rs:18).

4. Placement and program effect

BV2 is the correct owner. Vue emission semantics, comments, hoisting, and maps are Vue subsystem scope under [BV1.md:11](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/BV1.md:11). B5 explicitly forbids framework semantic repair at [B5.md:5](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/B5.md:5); B6 is likewise a lifecycle/reuse cutover, not a codegen repair owner.

No accepted ADR changes: the existing mapping taxonomy remains intact. No final program outcome changes: Vue compilation remains successful with truthful mappings. The DAG does change from the current direct `{BV1, BS1} -> B5` edge at [program-dag.toml:136](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/program-dag.toml:136) to `{BV1, BS1} -> BV2 -> B5`; B6 remains after B5. BV1’s accepted record remains historical and is not rewritten.

REPAIR: Eliminate duplicate ownership locally—defer disabled-comment removals, let the final VDOM root-prefix owner absorb contained removals, retain all strict segmented-overwrite preconditions and flush order; close the discovered SSR comment-only sibling locally in the same correction block.

PLACEMENT: BV2 is the sole owner, with predecessors BV1 and BS1 and B5 changed to depend on BV2; no accepted ADR or final program outcome changes.
