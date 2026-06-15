# PARSELOWER — Staged Migration Design v3 (synthesized from the 3/3 design-review consensus)

GOAL: delete `TypeExpr` from the HOT parse/shallow/macro/lazy-body/prepared caches; lower demanded OXC bodies + macro type-args to interned handles in the `SemanticNodeData` arena; keep OXC AST worker-local (`!Send`); materialize `TypeExpr` ONLY at compat/JSON/test/diagnostic/output boundaries. PERFPARSE non-SFC fast-path is separate.

This v3 keeps the endorsed 9-stage skeleton and bakes in the 7 review refinements (the v1→v2→v3 review chain endorsed the direction; v3 finalizes the stage-1/2/3 foundation). Stages 1-4 are NON-BREAKING (additive / dormant dual-read); 5-9 breaking-but-each-gate-green.

## Foundational definitions (Stage 1 deliverables)
- **Internal handle name = `HotTypeRef`** (NOT `TypeHandle` — `component_meta_payload::TypeHandle` already exists as a public content-hash BFS DTO and stays output-only). `HotTypeRef` wraps a `SemanticNodeId` in the session arena; it is NEVER a cache KEY (R6: keys stay content-free slot/fact identity).
- **`CarrierResolverContext`** (RUNTIME / VALUE-SIDE — NEVER a query key): the demand-time context a `BareRef`/`ImportType` carrier needs to resolve, recovering what the current `lower.rs:642-769` `Ref` path uses — `name_resolution`, `DeclarationScopePayload`, `ScopeShadowing` (builtin shadowing), substitutions, augmentation scope, type-param/binder env, reduction-demand axes (mode). It is PASSED at resolution time as runtime state (or split into a small content-free slot key + value-side rehydration of the heavy fields). DO NOT hash `name_resolution`/`DeclarationScopePayload`/`ScopeShadowing`/substitutions/active-stacks/content-versions into `SemanticQueryKey` — the query key stays content-free (R6); version rides the cached VALUE's `NodeScopeId`/read-set.
- **New graph carriers** (in `SemanticNodeData`, scoped by `NodeScopeId`): `BareRef{name, scope}` (unresolved name), `ImportType{specifier, qualifier, type_args, typeof}` carrier (unresolved), a raw-fallback carrier preserving `Unknown{raw}` text, and a content-free **`SyntheticBindingId`** replacing `SyntheticCarrierKey.value_node: u64` as the synthetic-binding identity. `RecursiveRef` is DEMAND-TIME-minted (solver-produced, Stage 3), not Stage-2-emitted. Preserve `Rest`/`ConstructorType` fidelity.
- **`materialize_type_expr(handle) -> TypeExpr`** = the SINGLE reverse boundary, output/compat only.
- **Typed replacements for the ~99 SEMANTIC `Unknown{raw}` control-flow uses**: extend `QueryError` / a typed carrier; the `BUDGET_EXCEEDED_SENTINEL` fuse is named must-not-regress. Display-only `rawType` passthrough is SEPARATE (round-trips raw text through materialize). The Stage-1 "no Unknown-as-control-flow" guard applies ONLY to NEW carrier constructors; the GLOBAL fence is Stage 8/9 (raise.rs:263/739 legitimately use Unknown as control output today).

## Stages

**Stage 1 — Ownership + Carrier Contracts (NON-BREAKING, additive).**
scope: the crate decision (session-owns hot macro/prepared/body cache structs carrying `HotTypeRef`; `verter_semantic` keeps `Analyzed*`/prepared structs as compat DTOs with `TypeExpr`/locators — verter_semantic NEVER depends on verter_session). Introduce `HotTypeRef`, `CarrierResolverContext`, the new graph carriers (`BareRef`/`ImportType`/raw-fallback/`SyntheticBindingId`/`Rest`/`ConstructorType`), and the `materialize_type_expr` boundary. Define the typed `QueryError`/carrier replacements for semantic `Unknown{raw}`.
owners: `verter_session` (arena, semantic_query SemanticNodeData, hot cache structs), `verter_semantic` (compat DTOs only).
acceptance: structural — carrier constructors preserve raw text, locator identity, scope roots, ctor/rest/recursive fidelity, the synthetic-binding identity, WITHOUT host queries; the new structs/handle compile + round-trip through `materialize_type_expr` for compat. NO producer/prepared field flips yet.
breaking: none (additive).
guards: new-carrier-constructor Unknown-as-control-flow ban (scoped, NOT global); `SyntheticBindingId` content-free (no bare SemanticNodeId in its identity); `HotTypeRef`/public-`TypeHandle` name distinctness; cache keys stay R6/R21 content-free.
doc-edits: type-resolution, type-cache-architecture, component-meta SKILLs (additive notes).

**Stage 2 — Session-Owned Query-Free Structural Graph Lowering (NON-BREAKING).**
scope: a SESSION-OWNED query-free structural lowerer consumes OXC/semantic syntax output (the OXC worker stays lease-pinned producing syntax/`TypeExpr`/DTO — it does NOT emit session graph nodes) and emits the structural graph + unresolved `BareRef`/`ImportType` carriers + raw/synthetic/rest/ctor carriers, NodeScopeId-rooted, performing NO name/import/type reduction.
owners: a session graph-lowerer module (consuming OXC output), arena.
acceptance: structural — hermetic structural-equivalence fixtures compare the structural graph + carriers (raw text, NodeScopeId roots, synthetic keys, heritage/type-arg metadata, value-decl carriers) vs the old TypeExpr→graph for the no-resolution-needed shapes. Preserve `indexed_ready_publish_lowers_zero_decl_bodies` (emission stays demand-time, never pulled to publish).
breaking: none (old TypeExpr outputs authoritative until consumers handle-capable / atomic slices land).
guards: session-lowerer query ban (no host/type-provider/dispatch calls); unresolved carriers not materialized during emission; OXC worker emits no session graph node.
doc-edits: type-resolution, compiler-codegen.

**Stage 3 — Demand-Time Carrier Resolution + Reduction (NON-BREAKING, additive dispatch).**
scope: the ONE query-time dispatch resolves carriers + reduces using `CarrierResolverContext`: bare-name resolution, import/augmentation stitch, enum projection, mode-dependent carrier-vs-execute, and the `instantiate_active` recursion guard MOVES here. CRITICAL: carrier resolution for instantiate / Vue-default bodies runs WHILE the active identity is still pushed (the query context carries the active identity) so the frame hasn't popped — preserving termination. `RecursiveRef` is minted here.
owners: session resolver / type query dispatch.
acceptance: host — resolution-equivalence fixtures cover imports, augmentations, enums, recursive refs, ctor/rest fidelity, mode differences, AND the 3 documented termination cases (incl. context-different eager re-entry only the active stack catches). The termination tests MUST be written to FAIL if Stage-3 carrier resolution for instantiate/Vue-default bodies ever leaks PAST the `build_instantiate` push→pop window (build.rs:2210-2285) — i.e. resolution firing after `pop_instantiate_active` is a test failure (this is the top residual risk).
breaking: none (additive; feeds legacy DTO materialization only at output).
guards: carrier resolution requires explicit `CarrierResolverContext`; recursion guard in dispatch; no worker fallback resolution; resolution cache keys slot/fact-identity + env-split, anon subjects uncached.
doc-edits: type-resolution, type-cache-architecture.

**Stage 4 — Handle-Capable Consumers First (NON-BREAKING, dual-read).**
scope: make consumers accept handles BEFORE producer flips (dual-read through the SAME dispatch — legitimate read-compat, not a 2nd resolver): `ShapeSubject::TypeExpr`, `PreparedWrapperShape::Opaque/Transform(TypeExpr)`, type-param `constraint/default` + `Vec<TypeParam>`, class heritage/type-arg metadata, value decls, `ResolvedImportedRegistrySymbol.body`, AND `OwnerCollectionDb`/`owner_collection_exprs` (component_meta_caches.rs:911, registry_decl.rs:633/681).
owners: shape resolver, component-meta compat layer, registry-symbol consumers, OwnerCollectionDb.
acceptance: host — old TypeExpr and new handle paths produce equivalent public shapes/fallthrough/wrapper-transforms/imported-registry-bodies/owner-collection.
breaking: none (dual-read).
guards: no hot-path `materialize_type_expr` bridge; materialization only at public DTO/output seams; per-inventory guard: each listed hot carrier has a handle-native consumer before producer conversion.
doc-edits: component-meta, type-resolution.

**Stage 5 — Atomic Macro-Producer + Native-Consumer Slice (BREAKING).**
scope: atomically flip the SESSION-OWNED hot macro payloads/caches (the session-side ingestion + macro-surface caches DERIVED from the semantic DTOs) to `HotTypeRef` TOGETHER with native projector/shape consumers; synthetic slot carriers, macro type-params, wrapper shapes, heritage/type-args, value-decl carriers move as ONE gate-green slice. CRATE-BOUNDARY (per Stage 1): the `verter_semantic` `Analyzed*` DTO fields (`AnalyzedPropField.type_expr`, `AnalyzedMacro.parsed_type_argument`, the `*_expr` fields) STAY `TypeExpr`/locator compat DTOs — they are NOT flipped (verter_semantic has no session dep). Only the session-owned hot mirror/cache that ingests those DTOs carries `HotTypeRef`; if a hot struct currently lives in `verter_semantic` and must hold a handle, MOVE it into `verter_session` first (do not add a session dep to verter_semantic).
owners: session macro ingestion/cache, native projector, shape/component-meta consumers (NOT verter_semantic::analysis DTOs).
acceptance: host — macro/component-meta fixtures prove native projection == compat output, incl. XP.5 synthetic slots + fallthrough/root inheritance.
breaking: internal cache payloads → handles; public DTOs compat-materialized at output.
guards: atomic-slice guard (producer flip can't merge unless native projector + all shape consumers handle-native); macro hot paths not in the transitional materialize allowlist.
doc-edits: component-meta, type-resolution.

**Stage 6 — Atomic DeclBodyMemo + Prepared-Decl Slice (BREAKING, TOP REGRESSION RISK).**
scope: atomically replace `DeclBodyMemo`/prepared-decl payloads with handles + graph-native deps. EXHAUSTIVELY enumerate + convert every `whole_env()` consumer (local type-decl lookup, fallthrough, runtime values, value-alias peeling — decl_body_memo.rs:449); remove stored `EvalEnv<TypeExpr>` ONLY after those are native.
owners: DeclBodyMemo, prepared-decl consumers, dependency extractor.
acceptance: host — prepared-decl fixtures prove lookup/fallthrough/runtime/value-alias unchanged; graph-native dep extraction matches legacy env-derived deps; single-flight + zero-body-publish guards green.
breaking: internal prepared/body cache layout; compat DTOs materialized at public seams.
guards: atomic-slice guard (no DeclBodyMemo flip before all whole_env() consumers native); graph-native dep extraction before deleting EvalEnv<TypeExpr>; no hot materialization bridge; strengthen `no_indexed_ready_eval_env_or_type_decl_body_storage`.
doc-edits: type-cache-architecture, type-resolution.

**Stage 7 — Cache-Key + Artifact Cutover (BREAKING).**
scope: migrate hot cache artifacts to handle-native payloads with R6/R21 content-free slot/fact identities (NOT bare SemanticNodeId); migrate the synthetic-deepening path to `SyntheticBindingId`; preserve multi-candidate storage, env-split, parse-stable hashing, uncached anon subjects. Includes OwnerCollectionDb + ShapeSubject artifact families.
owners: FileArtifactStore, type cache layer, session cache owners.
acceptance: host — cache-hit equivalence + invalidation tests over augmentation, env splits, prepared bodies, shape/materialize caches, OwnerCollectionDb, synthetic deepening, anon subjects.
breaking: persisted artifact schema may rev; all-or-nothing per artifact family.
guards: cache-key lint forbids raw-text content + content/version hashes in shape/materialize keys; `ShapeSubject::SemanticNode` (component_meta_caches.rs:1153-1156, a keyed `ShapeCacheKey` field used for REGULAR member dedup, not just synthetic) is audited MIGRATE-OR-CARVE-OUT — it is a DEFENSIBLE carve-out IF it stays a within-generation interning subject that is fact-rooted + generation-validated (R6 bans content/version hashes + versioned `DeclIdentity`; a bare `SemanticNodeId` ordinal is neither, so it may legitimately remain as a value-rooted interning subject) — the Stage-7 decision + the guard's exact scope are made EXPLICIT here, not pre-concluded; anon subjects bypass cache; producer+consumer schema versions advance atomically.
doc-edits: type-cache-architecture.

**Stage 8 — Compat DTO Materialization Fence (BREAKING).**
scope: collapse materialization to the single `materialize_type_expr` boundary for public/output DTOs, diagnostics, protocol, explicit compat APIs; all hot semantic paths on handles/carriers.
owners: output adapters, diagnostics/protocol, compat exporters.
acceptance: host — public snapshot/API tests prove legacy TypeExpr/rawType compat while hot-path traces stay handle-native.
breaking: internal callers lose direct raise/reduce outside allowlisted output paths.
guards: path-based allowlist over ALL ~113 `raise_node_to_type_expr` + 12 `raise_and_reduce_with_context` sites, each classified hot=forbidden / output=allowed; transitional allowlists monotonically shrink.
doc-edits: type-resolution, compiler-codegen.

**Stage 9 — Remove Transitional Bridges (BREAKING, final).**
scope: delete legacy hot TypeExpr bridges, stale raise/reduce entry points, non-output compat shims; enforce handle-native dispatch everywhere hot. Enable the GLOBAL Unknown-as-control-flow fence here.
owners: session/type-resolution maintainers, test owners.
acceptance: host — full canonical gate (`cargo nextest run --workspace` + `cargo test -p verter_session --tests`) + targeted host resolution suites green with ZERO transitional materialize allowlist entries.
breaking: hot-TypeExpr-dependent internal APIs removed.
guards: `hot_path_never_calls_materialize_type_expr` enabled LAST (after allowlists → 0); CI rejects new hot raise/reduce + query-time resolver use from worker lowering; global Unknown-as-control-flow fence.

## New guards (registered across the stages above)
`no_type_expr_fields_in_decl_body_memo`, `no_type_expr_fields_in_prepared_decl_hot_cache`, `no_type_expr_fields_in_macro_hot_cache`, `no_type_expr_shape_subject_in_production`, `no_lower_ts_type_call_in_hot_producers`, `hot_path_never_calls_materialize_type_expr` (last), `session_graph_lowerer_makes_no_query`, `no_bare_semantic_node_id_in_shape_or_materialize_key`, `no_verter_semantic_to_verter_session_dep`, `synthetic_binding_identity_is_content_free`.

## CRITICAL doc edits (update, not weaken)
CLAUDE.md (Shallow-File-Processing lazy bodies → handles; Typed-IR-Only producer = handles; component-meta shallow-by-default internal carriers = handles, public TypeExpr::Ref = materialized output; Declaration Merging MergedDecl handle mandatory); type-resolution / type-cache-architecture / component-meta / host-session / testing SKILLs per the stages.

## Invariants preserved
lazy-body single-flight + zero-body-publish (IndexedReady body-free); Send+Sync host caches (only handles/Arc/facts escape workers; OXC lease-pinned worker-local); Declaration Merging MergedDecl carrier (handles, not intersection); 5-mode dispatch (lowering mode-neutral, mode applied at demand); fact-based content-free query keys (handles rooted by NodeScopeId + file/version facts; R6/R21).

## Transitional dual-path note (CLAUDE.md "one clean cutover" compliance)
The Stage-4 dual-read consumers and the Stage-8 monotonically-shrinking materialize allowlist are TRANSITIONAL, faithful to the SPIRIT of CLAUDE.md's "one clean cutover / no double branches" (there is ONE resolver — Stage-4 dual-read routes both arms through the SAME dispatch; it is read-compat, not a second resolution engine). Stage 9 (`hot_path_never_calls_materialize_type_expr` enabled last, allowlist → 0, global Unknown fence) leaves ZERO permanent dual path. Stated explicitly so a reviewer does not read the transitional dual-read/allowlist as a rule violation.

## Highest regression risk
Stage 6 (DeclBodyMemo+prepared atomic) is the top risk — exhaustive whole_env() enumeration is mandatory. Then the macro public surfaces (Stage 5) + the compat bridge (Stage 8). De-risked by: non-breaking Stages 1-4 (carriers + demand resolution + handle-capable consumers all land before any producer flip), then atomic producer+consumer slices, then delete.
