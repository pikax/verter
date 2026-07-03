# Stage 10 — Terminal `TypeExpr` Removal (BINDING DESIGN)

Status: **PROPOSED-BINDING — pending CTO + owner ratification** (not yet ratified; becomes binding on ratification). Codex-reviewed (`.feedback/stage10-scope/OUT-REVIEW.txt`, verdict REVISE → findings applied in this revision). Authored on the settled post-Stage-9 tree `refactor/semantic-db-overhaul` @ `4ca7692cd`. This document is the DESIGN AUTHORITY for Stage 10; block contracts cite it instead of re-demanding the design gate. It supersedes the shelved draft `mom/stage10-typeexpr-compat-removal-design` @ `9062d4baf` (imported in part per §15; the shelved draft is provenance-only and is NOT binding).

Governance basis: the codex-2/2 Stage-10 scope panel returned **TERMINAL-SCOPE REBASE / REBASE-AND-EXPAND** (`.feedback/stage10-scope/OUT-LEG1.txt`, `OUT-LEG2.txt`): import the shelved A/B/C mechanism kernel, REPLACE its stale scope/guards/sequencing, ABSORB the orphaned carriers its §4.5 wrongly disclaimed, and close the whole semantic-`TypeExpr` terminal surface with structural-only landed enforcement. This design encodes that verdict plus the owner's terminal-completeness mandate.

---

## 0. Terminal-completeness mandate (the binding gate criterion)

Stage 10 is the **TERMINAL** `TypeExpr` block. After it lands:

- **ZERO** remaining semantic-`TypeExpr` work; **ZERO** new `TypeExpr` deferral carried to Stage 11 or Stage 12.
- The interim dual representation (a decl body reachable BOTH as cache-owned `TypeExpr` and as graph node / `HotTypeRef`) is **deleted**.
- The residual inventory (`residual_type_expr_body_reader_inventory.rs`) and both deferral docs (`authored-shape-graph-native-migration-deferral.md`, `hot-materialize-tripwire-residual-deferral.md`, and the now-moot `q2-structural-body-cache-deferral.md`) are **deleted at landing**.

`TypeExpr` may survive Stage 10 **ONLY** as the already-designated syntactic / output / diagnostic class — the sealed `OutputProjector` output seam, protocol / JSON / display payloads, and JSDoc `{Type}` text parsing (the Stage-11 quarantine census owns those). Stage 10 **may not grow that class** by relabeling semantic work as "output." Any plan that leaves a semantic-`TypeExpr` residual, or defers one out of Stage 10, is rejected by this mandate.

---

## 1. Context

`TypeExpr` is the pre-graph typed-IR body carrier. PARSELOWER Stages 1–9 removed it from the hot parse / shallow / macro / lazy-body caches and installed the graph-native substrate: the single `ProjectSemanticDispatch` query engine, the `SemanticGraphStore` multi-candidate read-set-validated caches, the `verter_no_typeexpr::NoTypeExpr` compiler witness, the sealed `OutputProjector` output seam, and `RaisedShapeFacts` / `RaisedShapeKey`. The hot-materialize fence `hot_path_never_calls_materialize_type_expr` is enabled and green at zero offenders.

Stage 6 landed as **Option B**: the declaration-body hot READ path is a thin `decl_body_hot_ref` accessor over the `SemanticGraphStore` `Instantiate` memo, with exactly ONE migrated graph-backed reader (`lower_decl_body_to_node`). The full flip of the remaining reader population was deferred. That deferral is what Stage 10 terminates.

The residual surface is enumerated in §3. The A/B/C dispositions (`A=REMOVE` authored-shape readers; `B=NARROW` graph-free frontier/shallow/eval-env DTOs; `C=NARROW` lower-crate `Prepared*`/`Analyzed*`) stand on their topology + single-engine merits, re-confirmed by both scope legs. Stage 10 executes them across the WHOLE semantic-`TypeExpr` surface — including the orphaned carriers the shelved §4.5 wrongly assigned to an earlier storage flip that never happened.

---

## 2. Dispositions (imported kernel — §§0–1 of the shelved draft, still binding on the merits)

| Surface | Disposition | Meaning |
|---|---|---|
| **A — authored-shape readers** | **REMOVE** | Query-time `TypeExpr` walking of authored heritage / closedness / key-domain shape is replaced by prep-time graph-native **authored-shape / closedness / key-domain FACTS** consumed by dispatch. No query-time `TypeExpr` decision logic survives. |
| **B — graph-free frontier / shallow / eval-env DTOs** | **NARROW (boundary KEPT)** | The broad `TypeExpr` body is replaced by **graph-free content-free locators + finite closed symbolic facts**. The graph-free boundary is preserved — these layers exist BEFORE the graph and must never carry a generation-local `HotTypeRef`. |
| **C — lower-crate `verter_semantic` `Prepared*`/`Analyzed*` DTOs** | **NARROW IN PLACE (crate boundary KEPT)** | The persistent / cache-owned DTO CONTRACT narrows to **facts + locators** inside `verter_semantic`. The crate boundary `verter_semantic ⊥ verter_session` is kept: these never carry `HotTypeRef` and never move up. |

The dispositions stand because they remain correct on topology and single-engine grounds, NOT because the shelved draft recorded them "do not re-litigate." Terminal completeness EXTENDS them: it also RESOLVES the open "permanent split-carrier compat vs full graph-native migration" question the `authored-shape-graph-native-migration-deferral.md` recorded — Stage 10 chooses **full graph-native (A=REMOVE)**; the permanent-split-carrier option is dropped.

The lower-neutral crate `verter_type_expr` (below both `verter_semantic` and `verter_session`) is the home for `TypeExpr` itself AND for the new locator + closed-fact families. `verter_no_typeexpr` / `verter_no_typeexpr_derive` provide the compiler-enforced witness. `DeclBodyMemo` + its `SnapshotLease` remain the retained-parse source — **NOT a persistent `TypeExpr` body store** post-Stage-10; its `TypeExpr`-bearing memo products are deleted and the provider re-routes through the locator (§6).

---

## 3. Live terminal census (verified first-hand @ `4ca7692cd`)

### 3.1 Residual body-reader inventory — 39 semantic + 5 output-compat

`crates/verter_session/tests/cases/residual_type_expr_body_reader_inventory.rs` pins (assertions at ~:3067–3131):

| Class (`enum ReaderClass`) | Count | Stage-10 fate |
|---|---|---|
| `GraphBackedMigrated` | 1 | Already migrated (`lower_decl_body_to_node`); the model for the rest. |
| `ProducerLowering` | 3 | Survive as the **sanctioned private transient authored-IR → graph producer bridge** (`lower_decl_body_with_provenance`, `prepare_type_decl_from_lowered`, `prepare_local_value_decl`) — not a residual semantic reader. Re-pointed to lower from the locator. |
| `AuthoredShape` | 17 | Surface A → REMOVE (facts). |
| `GraphFreeDto` | 12 | Surface B → NARROW (locators + facts). |
| `GraphBackedPending` | 6 | Absorbed (registry / member-surface / annotation-handle refactors). |
| **Semantic total** | **39** | → **0 migratable semantic readers** (35 = 17+12+6 disappear; 3 ProducerLowering become the producer bridge; 1 already migrated). Inventory file DELETED at landing. |
| `OutputCompat` (`COMPAT_BODY_READERS`) | 5 | **SPLIT — NOT all permitted survivors** (see below). |

**OutputCompat split (codex REVISE finding 1 — the 5 rows are NOT uniformly output).** Three are production cache/fact **fingerprint inputs** — semantic infrastructure, NOT sealed output/protocol/display/JSDoc — and MUST convert in Stage 10:

| Row | Location | Nature | Stage-10 fate |
|---|---|---|---|
| `compat_type_body_hash_input` | `decl_body_memo.rs:394` | cache fingerprint input | **CONVERT** → fact/locator/stable-hash input, no `TypeExpr` |
| `compat_value_body_hash_input` | `fact_emission.rs:270` | fact fingerprint input | **CONVERT** → stable-hash/fact input, no `TypeExpr` |
| `LazyBodyFactSource::compute` | `fact_emission.rs:176` | fact emission | **CONVERT** → fact input, no `TypeExpr` |
| `compat_type_contributors_for_typeinfo` | `shallow_file_state.rs:1015` | typeinfo contributor computation | reduce to **WIP/test-oracle only** — NOT a production survivor |
| `walk` | `typeinfo/oracle_core/source_walk.rs:145` | oracle walk | **WIP/test-oracle only** — NOT a production survivor |

None of the 5 is a legitimate Stage-11 production `TypeExpr` survivor. The permitted-survivor class (§4.3) is the sealed `OutputProjector` seam + protocol/JSON/display + JSDoc-text — NOT these inventory rows. The hash-input trio converts in Stage 10 (B5); the two oracle readers reduce to test-only surfaces (the WIP parity oracle / source-walk test harness).

Non-growth constants pinned today (`GRAPH_BACKED_PENDING_CAP = 6`, `GRAPH_BACKED_PENDING_TARGET = 0`) confirm the intended terminal target is zero.

### 3.2 Orphaned semantic-`TypeExpr` carriers (no remaining stage owns them — ABSORBED into Stage 10)

The shelved §4.5 declared these OUT of scope on the false premise that a Stage-5A/6/7 `TypeExpr → HotTypeRef` STORAGE FLIP owned them and would "flip before Stage 10 runs." Stage 6 landed Option B (flip deferred); Stages 7–9 did not flip them. They are live and unowned:

| Carrier | Location | Shape today | Stage-10 target |
|---|---|---|---|
| `ResolvedImportedRegistrySymbol.body` | `resolver_core/component_meta_query_engine/mod.rs:450` | `pub body: TypeExpr` (engine memo `imported_registry_symbols`) | Session-side graph-backed **identity / hot handle** + authored locator/facts (this is a genuinely graph-backed session surface). |
| `OwnerCollectionDb` value + `owner_collection_exprs` memo | `component_meta_caches.rs:920-921`; `mod.rs:530` | key `(Arc<str>, Arc<str>)`; **value** `Option<Arc<TypeExpr>>` (TypeExpr-**valued**, not keyed) | De-`TypeExpr` the value: handle/locator/fact on the store; query-local memo likewise. |
| `ProjectedMember.ty`, `ProjectedIndexSignature.key_type/value_type`, `ProjectedSurface.call/construct_signatures` | `verter_semantic/.../query_engine.rs:18-120` | `TypeExpr` fields (lower crate) | **Facts + locators** (graph-free; never `HotTypeRef` — crate boundary). Narrows under Surface C. |
| `FastShallowFieldExpr.expr` | `resolver_core/component_meta_query_engine/mod.rs:462` | `pub expr: TypeExpr` (session) | Session handle / fact. |
| `NamedTypeMember.type_expr` | `typeinfo/framework_surface/results.rs:175` | `Option<TypeExpr>` (`OptionsSurface`/`ExposeSurface` members; session output DTO) | **Split**: internal semantic surface → hot/fact; sealed output DTO → permitted survivor only AFTER sealed output projection. |
| Svelte facts | `verter_semantic/.../framework_facts/svelte.rs:64/124/173/189` | `SveltePropsCandidate.props_type`, `SvelteScriptCandidates.dispatcher_events`, `SvelteScriptFacts.props_type/dispatcher_events`: `Option<TypeExpr>` | Persisted/cache-owned Svelte carriers → **facts + locators**; only genuinely transient producer-local IR (immediately converted) is allowed. |
| `MacroSurfaceDtos` containers | `typeinfo/framework_surface/results.rs:184` | TypeExpr-bearing transitively via `Analyzed*Field.type_expr`, `NamedTypeMember.type_expr` | The embedded `verter_semantic::Analyzed*` payloads narrow TRANSITIVELY under Surface C; the session container split is Surface-A/B7 output boundary. |

### 3.3 `HotPrepared*` scaffolding + empty `structural_body_cache`

- `resolver_core/hot_prepared.rs` — a **complete** 15-carrier handle-native mirror family (`HotPreparedTypeDecl`, `HotPreparedValueDecl`, wrapper/classifier/value analogues), every carrier `#[derive(NoTypeExpr)]` + `assert_impl_all!(_: NoTypeExpr)` (16 assertions, :76–91), every one `#[allow(dead_code)]`, **ZERO production callers** (grep hits only `hot_prepared.rs`, gated tests, and two `tests/cases/` guards). Populate/read wiring was deferred at Stage 6.
- `resolver_core/prepared_decl.rs:679` — `structural_body_cache: Arc<PreparedStructuralBodyCache>` constructed EMPTY (:760), accessor `#[allow(dead_code)]` (:688) with ZERO call sites, never populated. `q2-structural-body-cache-deferral.md` records the codex-DEFER ruling: its key lacks the resolving lowerer's args/env/substitution/mode dimensions, so population would be **unsound**; warm reuse is already sound via the `Instantiate` memo.

### 3.4 FN5.2 — Unknown-fence typed-degradation end-state

`hot-materialize-tripwire-residual-deferral.md` (:45–92): the fence `no_new_semantic_unknown_control_flow_outside_owner` recognizes the `TypeExpr::Unknown { raw: <sentinel> }` control-flow shape; FN5.2 (explicitly assigned **Stage 10**) replaces that control sentinel with a typed degradation carrier, at which point the control-flow shape itself disappears. Guard: `output_projector_residual_guards.rs:17400` with `UNKNOWN_SENTINEL_OWNER_FILES` = **6 owner entries** (5 files + 1 subtree); the single owner mapping is `semantic_query_error_raw` (`resolver_core/component_meta_query_engine/surface.rs:723`).

### 3.5 Stale prose the census supersedes (corrected in-tree at landing, per §13)

- `CLAUDE.md:63` — "three deferred SEMANTIC reader classes" understates the taxonomy (5 semantic classes + OutputCompat). Update to the census.
- `docs/arch/parselower-design.md:10/155/162`, `semantic-db-overhaul-unified-remaining-plan.md:80`, `.claude/skills/type-resolution/SKILL.md:756/759`, `.claude/skills/component-meta/SKILL.md:259` — say "~40 total / 7 GraphBackedPending"; the live pin is **39 total / 6 GraphBackedPending**. No literal "41" exists in-tree (the shelved draft's "41" is superseded; the live-tree drift is ~40/7). These are deleted or corrected as their owning docs are updated at landing (§13).
- Inventory row wording "TypeExpr-keyed `OwnerCollectionDb`" is loose — it is TypeExpr-**valued**. The inventory file deletes at landing, mooting the wording.

---

## 4. Scope (REPLACES the shelved §4.5)

### 4.1 In scope

Every semantic-`TypeExpr` carrier reachable in `verter_session` / `verter_semantic`:

1. The three A/B/C surfaces (§2).
2. All 35 migratable residual readers (17 AuthoredShape + 12 GraphFreeDto + 6 GraphBackedPending) → 0.
3. The ProducerLowering trio → re-pointed to lower from the locator (producer bridge).
4. All §3.2 orphaned carriers (imported registry, `OwnerCollectionDb`, `Projected*`, `FastShallowFieldExpr`, `NamedTypeMember`, Svelte facts, `MacroSurfaceDtos` embedded payloads).
5. `DeclBodyMemo` end-state: retained parse/locator source only; its `TypeExpr`-bearing memo products (`LoweredTypeDecl.body: TypeDeclBody`, `LoweredValueDecl.type_annotation: Option<TypeExpr>`, `object_shape`, signatures, enum bodies, nested `TypeParam` payloads) deleted as persistent memo fields.
6. `HotPrepared*` **production-wired** as the live session hot-prepared surface.
7. `structural_body_cache` (+ `PreparedStructuralBodyCache` machinery) **deleted**.
8. FN5.2 typed-degradation carrier replacing `TypeExpr::Unknown { raw: sentinel }` control flow.
9. The type-param `constraint`/`default` carriers and the four prepared-wrapper payloads (recorded DEFERRED/OPEN by earlier stages — Stage 10 is their owner).
10. **The production hash/fingerprint-input trio** (`compat_type_body_hash_input`, `compat_value_body_hash_input`, `LazyBodyFactSource::compute`) — converted to fact/locator/stable-hash inputs with no `TypeExpr` (these are cache/fact identity, i.e. semantic infrastructure, not output). The two oracle readers reduce to WIP/test-oracle surfaces.

### 4.2 Out of scope (Stage 11 / Stage 12 / roadmap)

- **Stage 11** — the TypeSyntax / output quarantine CENSUS and rename only: the sealed `OutputProjector` output seam, protocol / JSON / display payloads, diagnostics, and JSDoc `{Type}` text parsing. Stage 10 leaves these as the permitted survivor class; it does not rename or quarantine them.
- **Stage 12** — profile-gated perf compaction only, AFTER the architecture is final. Any future structural body cache belongs here (Stage 10 wires only the correctness-required `LocatorLoweringKey` memo on the existing validated substrate).
- The grandfathered Stage-9 hot-materialize syntactic tripwire is NOT removed.
- No TypeScript-checker parity, language-service, general-emit, whole-env, or cache-compaction debt is touched.
- `HotTypeRef` is NOT forced into lower graph-free crates or protocol / output DTOs.

### 4.3 Permitted survivors (Stage-11 quarantine class — Stage 10 may NOT grow it)

Only the sealed output / protocol / display / JSDoc-text class: the `OutputProjector` materializer, protocol/JSON DTOs, display strings, and JSDoc `{Type}` tag payloads. A carrier is permitted ONLY if it survives BEHIND the sealed output projection — never as a semantic-decision authority.

**The 5 `OutputCompat` inventory rows are NOT blanket permitted survivors** (codex REVISE finding 1). "Output" means sealed output/protocol/display/JSDoc ONLY. **Hash inputs, fact emission, cache identity, typeinfo contributor computation, and query control flow are SEMANTIC INFRASTRUCTURE** and are finished IN Stage 10, not deferred as "output": the fingerprint-input trio converts (§3.1) and the two oracle readers reduce to test-only. Relabeling any semantic work "output" to keep a `TypeExpr` is a rejected deviation.

---

## 5. Mechanisms (imported kernel §§2–4, extended to the terminal surface)

### 5.1 Surface A — authored-shape → prep-time graph-native facts (REMOVE)

- **`authored_heritage_bases: Arc<[HeritageBaseFact]>`** on `PreparedTypeDecl`, produced where `member_index` / `wrapper_shape` / `projection_class` are already produced. `HeritageBaseFact` carries ONLY authored data — the authored `name` (as written), a member-origin locator for its authored span (§7), `Arc<[TypeArgLocator]>` for authored type arguments, and a reference into the decl's own local `name_resolution` mapping. **No resolved identity** in the fact (target `(canonical_id, symbol_name)` routing is computed at dispatch time — a prep-stored resolved identity is a stale-identity R21 hazard). `class_heritage_bases` becomes a thin read of the fact.
- **Closedness / key-domain = locator-through-dispatch, NOT a global scalar and NOT a new walker.** The prep producer emits a local **`ClosednessRecipe`** capturing only cheap decidable-from-syntax shapes: closed-named-members `Object ⇒ Closed`; `Intersection(Arc<[ClosednessRecipe]>) ⇒ ClosedIfAllArms`; `Parenthesized(inner)`; mapped-type-with-open-key ⇒ `Open` (`MappedOpenParam`). Two escape arms: **`FollowSlot(ref-slot locator)`** (reference case only) and **`FollowLocator(AuthoredBodyLocator + role/position + a SYMBOLIC binding/substitution locator)`** (the general escape for every complex/undecided subject). A stored `FollowLocator` fact **MUST NOT persist** a live `KeyDomainBindings`, a borrowed `TypeExpr`, or a `SemanticNodeId` (codex REVISE finding 6) — it stores ONLY the symbolic binding/substitution locator; the live `KeyDomainBinding` / canonical substitution environment is RE-MINTED inside the single dispatch engine at evaluation time. An index-signature whose `key_type` is not cheaply closed falls to `FollowLocator` — the key-DOMAIN closedness axis is distinct from the declared `key_type` SHAPE (which is `ObjectShapeFact` payload).
- **`KeyDomainFact`** (the owned prep fact) arms are RECIPE-ONLY: `Open` / `ClosedAbstract` / `FollowSlot` / `FollowLocator`. The live borrowed `KeyDomainBinding<'e>` arms `ClosedExpr(&'e TypeExpr)` / `ClosedNode(SemanticNodeId)` are NEVER prep-fact arms — they are re-minted during dispatch evaluation.
- **Evaluation** re-points the retained dispatch helpers (`prepared_decl_body_is_closed`-family, `key_domain_type_expr_is_closed`, `userland_instantiation_body_is_closed_object`) to read the recipe/fact and lower locators into the graph under the existing budget/visited recursion. FORBIDDEN: any caller-side / prep-time / `verter_semantic`-side transitive closedness/key-domain resolver — that is a second engine. Engine count is unchanged (cross-decl recursion already lives dispatch-side).

### 5.2 Surface B — graph-free frontier/shallow/eval-env → locators + finite facts (NARROW, boundary kept)

- `external_type_frontier::ResolvedSymbol.body: Option<TypeExpr>` → `body_locator: Option<SymbolBodyLocator>` and/or `frontier_body: Option<NarrowFrontierBody>` — a CLOSED finite enum with ONLY unresolved-symbolic arms (export routes, unresolved external refs, type-param shells); deliberately NO object-members arm and NO general body arm (the `lookup_object()` `Cow<TypeExpr>` population becomes the locator escape).
- `ShallowFileState`: `LoweredTypeDecl.body` → body slot (locator) + type-decl facts; `LoweredValueDecl.type_annotation` → annotation locator + value facts. The internal route-demand walks (`route_closure`, `member_path_route_closure`, `member_route_closure`, `whole_route_closure` + `collect_whole_route_refs`, `follow_local_symbol_precise`, `follow_routed_expr`, `extract_string_literal_keys_from_type_expr`, `collect_member_path_seed_names`) narrow to closed **`ShallowRouteFacts`**: object-member-names route fact (or `OpenKeyDomain` for the carrier-stop class), member-path seed-ref fact, per-member dependency-edge facts, whole-route ref-closure fact. NAME/REF enumerations, not type-shape evaluations. `collect_whole_route_refs` recursion deletes with the closure it serves.
- `eval_env`: the `TypeExpr::TypeOf` peel (`peel_value_decl_alias_graph_native`, `dependency_value_symbol_graph_native`) → a precomputed graph-free **`typeof_alias_target: Option<ValueDeclIdentityPart>`** stored at shallow analysis.
- `ResolvedSymbol.type_parameters: Vec<TypeParam>` (nesting `constraint`/`default: Option<Arc<TypeExpr>>`) → **`NarrowTypeParam` / `TypeParamDeclFact`** (Stage-10-owned; the earlier stages' recorded "flip to `HotTypeRef`" is ILLEGAL for a graph-free field).
- **Boundary rule**: never `HotTypeRef` below the graph — it is generation-local; shallow state precedes the graph; the locator is the deliberate keyable INVERSE of `HotTypeRef`.

### 5.3 Surface C — lower-crate `Prepared*`/`Analyzed*` → facts + locators IN PLACE (NARROW, crate boundary kept)

Field-by-field narrowing (crate topology binding: `Prepared*`, `Analyzed*`, `Projected*`, and the three prep classifiers `build_member_index`/`classify_wrapper_shape`/`classify_projection` STAY in `verter_semantic`; NEVER `HotTypeRef`):

- `PreparedTypeDecl.body: TypeExpr` → `body_slot: TypeBodySlot` (locator) + `PreparedTypeBodyFacts`.
- `PreparedTypeDecl.merged_contributors: Vec<TypeExpr>` → ordered contributor slots/facts.
- `PreparedValueDecl.type_annotation` → `Option<ValueTypeAnnotationFact>` + annotation locator.
- `PreparedValueDecl.signatures` → **`FunctionSignatureFact`** (ordered overload-group; per-signature param facts, return locator-or-fact, `has_implementation_body`, `NarrowTypeParam`).
- `PreparedValueDecl.object_shape: Option<ObjectExpr>` → **`ObjectShapeFact`** closed over ALL FIVE `ObjectMember` variants (`Property`/`IndexSignature`/`CallSignature`/`ConstructSignature`/`Method`), reusing `FunctionSignatureFact` for function-like members. `Property`/`Method` carry REQUIRED `visibility: MemberVisibility` (identity-participating, publication-filtered) + `optional`; the index-signature fact carries declared `key_type` SHAPE (fact-or-locator, so `[k: string] ≠ [k: number]`) + `value_type` + `key_name` + `readonly`.
- `PreparedValueDecl.enum_members` → **`EnumMemberFact`** (ordered name → closed scalar; folded literal / sound primitive domain).
- `PreparedMember.ty` → **`PreparedMemberFact`** (REQUIRED `optional`/`readonly`/`is_method`/`visibility`/`declaration_origin` + `ty` locator); `PreparedValueMember.ty` → **`PreparedValueMemberFact`** (+ `is_method`).
- `PreparedTypeDecl.type_parameters` → **`NarrowTypeParam` / `TypeParamDeclFact`** (in `verter_type_expr`).
- Wrapper/forward: `PreparedKeyFilterShape::Opaque(TypeExpr)` / `PreparedKeyRemapShape::Opaque(TypeExpr)` / `PreparedValueRuleShape::Transform(TypeExpr)` → LOCATOR payload; `PreparedForwardPayload.target_args: Vec<TypeExpr>` → `Arc<[TypeArgLocator]>` (keeping `target_name` + `forwarding_kind`).
- `Analyzed*` / `Projected*` narrow BY SOURCE via the four-source model (§6.2): authored (b) → closed fact + `AuthoredBodyLocator`; session/adapter-raised (c) → lower-neutral closed fact + session-side `SessionDemandIdentity`; synthesized (d) → `ResolvedLocalTypeFact`-style closed fact. `ProjectedMember.ty` / `ProjectedIndexSignature` / `ProjectedSurface.call/construct_signatures` narrow the same way.

**§5.3 contract allowlist (`P2-CONTRACT-ALLOWLIST`)**: `verter_semantic` MAY use an internal `TypeExpr`-shaped value while PRODUCING a fact. `TypeExpr` may exist ONLY in (a) parser/prep-LOCAL internals (including a prep-local constructor parameter like `PreparedTypeDecl::new(.., body: TypeExpr)` not retained as a contract field) and (b) the private locator backing store. FORBIDDEN: public/persistent/cache-owned FIELDS; accessors/impl return types (`-> TypeExpr` / `-> &TypeExpr` / `-> Option<TypeExpr>` / `-> Arc<TypeExpr>`); any semantic-decision API parameter or return; hand-written `Serialize`/`Deserialize` impls and `Wire` helper structs exposing broad `TypeExpr` (live instance: `AnalyzedMacro`'s manual serde of `parsedTypeArgument`).

### 5.4 Orphaned-carrier migrations (§3.2)

- **`ResolvedImportedRegistrySymbol.body`** is a genuinely graph-backed session surface → identity + **session hot handle** (via `SessionDemandIdentity`, §6.2c) + authored locator/facts; the `imported_registry_alias_should_stay_symbolic` consumer reads the fact/handle, not a `TypeExpr`.
- **`OwnerCollectionDb`** value + `owner_collection_exprs` → de-`TypeExpr` to handle/locator/fact; anti-tear preserved (atomic publication).
- **`Projected*`** (lower crate) → facts + locators (Surface C; never `HotTypeRef`).
- **`FastShallowFieldExpr.expr`** (session) → session handle / fact.
- **`NamedTypeMember.type_expr`** → split: internal semantic surface (hot/fact) vs sealed OUTPUT DTO (permitted survivor only after sealed projection).
- **Svelte facts** → the persisted/cache-owned carriers (`SvelteScriptFacts`, and the content-addressed capture candidates) narrow to facts + locators; the graph-raised member surface uses `SessionDemandIdentity`; genuinely synthesized shapes (`svelte_default_synth`) use synthesized-(d) facts. Only transient producer-local IR (immediately converted) keeps a `TypeExpr` shape.

### 5.5 `HotPrepared*` production wiring + `structural_body_cache` deletion

- **Wire `HotPrepared*`** as the live **session hot-prepared surface**. The relationship is topology-clean: `verter_semantic` `Prepared*`/`Analyzed*` = facts + locators (graph-free); `verter_session` `HotPrepared*` = the handle-native session surface that consumes those facts and lowers those locators through the ONE dispatch into `HotTypeRef`. `HotPrepared*` is **RECONCILED to the new narrowed fact schema** (§5.3 / §8), NOT blindly mirrored from the obsolete lower `Prepared*` `TypeExpr` DTOs (codex REVISE finding 7): each `HotPrepared*` field maps to a narrowed fact or a minted handle, so the two families are the SAME end-state at two crate levels, not a fact-carrier and a stale-DTO-mirror. Remove the `#[allow(dead_code)]` posture; `assert_impl_all!(_: NoTypeExpr)` stays. The `HotPreparedTypeDecl` `semantic_body` / `lookup_body` handle split is RENAMED at wiring so no accessor reads as a surviving legacy `TypeExpr` compat path (`lookup_body` → a handle/fact-typed accessor name, e.g. `lookup_body_handle`). `PreparedDeclBundle` STOPS storing the lower-crate `Arc<PreparedTypeDecl>`/`Arc<PreparedValueDecl>` `TypeExpr` DTOs as the query-time semantic body source, and instead publishes an **atomic facts + hot-prepared bundle** under a **completion fence** as ONE unit with read-set validation (no torn facts/hot handles; cancelled/superseded/budget-exceeded results never publish warm).
- **Delete** `structural_body_cache`, `PreparedStructuralBodyCache`, `StructuralBodyRegistry`, `StructuralBodyMemo`, and their instrumentation — never populated, key unsound for resolving-lowered bodies. The correctness-required lowered-body memo is the `LocatorLoweringKey` memo on the existing `SemanticGraphStore` multi-candidate substrate. Any future perf cache is Stage 12. This deletion is the ONE incrementally-landable slice (dead code, no dual path).

### 5.6 FN5.2 typed degradation

Replace the `TypeExpr::Unknown { raw: <sentinel> }` control-flow shape with a **typed degradation carrier** (`QueryError` / a typed degradation state) at the 6 owner sites. `semantic_query_error_raw` and the raise/shape-engine sentinel producers return the typed carrier; downstream consumers branch on the typed state. `TypeExpr::Unknown { raw }` survives ONLY in final output/display/JSDoc/true-unknown positions (behind the sealed output seam). The fence's recognized control-flow shape disappears; the guard's owner-file allowlist empties and the fence + its doc delete at landing.

---

## 6. Locator / fact substrate (imported kernel §§5.1–5.6, with [P1]/[P2] closed)

### 6.1 `AuthoredBodyLocator`

The cross-boundary CONTENT-FREE slot identity — a CLOSED sum over the AUTHORED parse-backed source kinds only: (a) decl-body + (b) authored macro/field payloads. Leaf variants `SymbolBodyLocator` / `TypeBodySlot` / `TypeArgLocator` + macro/field-payload leaves. Fields:

- **anchor** = `canonical_id` + `symbol` + `space` (for (b): owning declaration + macro index/kind; the anchoring canonical is the PRODUCING canonical `type_expr_scope`, which may be a cross-file resolver's canonical, NOT the component owner);
- **intra-decl / payload path** = an enum of named positions / small indices, **PRODUCER-EMITTED** (never span-reconstructed — name spans cannot address annotation `TSType`s), never an embedded `TypeExpr`, never a byte span as identity.

NO env-hash dims, NO content/whole hash, NO `SemanticNodeId`, NO `HotTypeRef`, NO versioned `DeclIdentity`. Derives `Hash + Eq` (the keyable inverse of `HotTypeRef`), modeled on the anchor+path portion of `ResolvedDeclSlotIdentity`. Must deterministically reach (via deref) the lexical scope + type-parameter environment, macro-expansion provenance, and authored spans (§7).

### 6.2 Four-source model

Disjoint source kinds:

- **(a) decl-body** → `AuthoredBodyLocator` (decl-body leaf).
- **(b) authored macro/field payloads** → `AuthoredBodyLocator` (payload leaf); REUSES the PRODUCING canonical's `DeclBodyMemo` snapshot (`P1-AUTHORED-REUSES-DECLBODY-SNAPSHOT`) — NOT a new `AnalyzerPayloadBodyMemo` (that would re-parse identical source under the identical `SnapshotKey`, violating "parse each live file version once").
- **(c) session/framework-adapter-raised** → **`SessionDemandIdentity`** (NOT a locator arm; owned by `verter_session`). The demand identity for payloads raised from the graph by a registered adapter's session-side normalizer (Vue `vue_exec/normalize.rs` `raise_member_value`; the Svelte graph-raised surface members; `ResolvedImportedRegistrySymbol`; any future adapter — classified by PRODUCER CLASS, not framework name; covers only the "single replayable graph route" class). Fields: the OWNER (component/surface canonical + macro/surface anchor) + the MEMBER/ROLE PATH + the ROUTE DISCRIMINANT (macro-hot-mirror / `ProjectPath` selector). Content-free / env-free / `Hash + Eq`. It does NOT lower via `LocatorLoweringKey`; its deref REPLAYS the existing session graph route, memoized in the existing session graph memo. **`SessionDemandIdentity` is NEVER stored in `verter_type_expr` or `verter_semantic`** (codex REVISE finding 3): the lower-crate (c) field carries ONLY a lower-neutral closed source fact/locator with a `TypeExprScope`-form producing-canonical; the session-typed replay identity is held session-side, and any session map KEYED by `SessionDemandIdentity` stays inside the existing session graph/cache regime under env + read-set validation (so the crate boundary is not tripped and no new off-store cache is created).
- **(d) synthesized** → a closed fact (NOT a locator). E.g. `ResolvedLocalType` (the live carrier; the shelved draft's `ResolvedLocalTypeFact` name is the PROPOSED fact) synthesized by `build_expanded_type_expr`.

### 6.3 Closed fact families (the [P2] enumeration — see §8 for the required-field tables)

Full family list (every family is a CLOSED FINITE enum/struct; NO `TypeExpr` / `Box<Self>` / open recursive arm; unsupported structure is a LOCATOR — the single graph-engine-routed escape; adding an arm is a reviewed schema event):

Surface A: `HeritageBaseFact`, `ClosednessRecipe`, `KeyDomainFact`.
Surface B: `NarrowFrontierBody`, `ShallowRouteFacts`, `ValueTypeAnnotationFact` (incl. `typeof_alias_target`), `SymbolBodyLocator`/`TypeBodySlot`, `NarrowTypeParam`/`TypeParamDeclFact`.
Surface C: `PreparedTypeBodyFacts`, `FunctionSignatureFact`, `ObjectShapeFact`, `EnumMemberFact`, `PreparedMemberFact`, `PreparedValueMemberFact`, the narrowed `PreparedKeyFilterShape`/`PreparedKeyRemapShape`/`PreparedValueRuleShape`/`PreparedForwardPayload`, and the macro/field/payload facts (`AnalyzedMacro`/`AnalyzedPropField`/`AnalyzedEmitField`/`AnalyzedSlotField`+`AnalyzedSlotFieldBinding`/`AnalyzedOptionsProp`/`AnalyzedExposeField`/`ResolvedLocalType`+`parsed_type_argument`), plus `ResolvedLocalTypeFact` with `TuplePayloadFact { readonly, elements }` / `TupleElementFact { label, optional, rest, ty }` / the indexed-access fact, and the `Projected*` facts.

### 6.4 `LocatorLoweringKey`

The SESSION-SIDE warm-memo key = `AuthoredBodyLocator` + the FULL live graph-lowering dims: `parse_env_hash` + `project_identity` + `type_env_hash` + `lib_env_hash` + `resolve_env_hash` + the FULL `ProjectionReductionContext(mode, demand, provenance, merge_role)` axis + the substitution axis when lowering under an instantiated body (equivalently: route through the existing `Instantiate` query key whose key already carries `args`). A `{locator + resolve_env_hash}`-only key would alias distinct lowered nodes. Content-free (no content/whole hash — R6); never appears in a graph-free / lower-crate DTO.

### 6.5 Body-source provider

ONE provider `fn lower_locator(locator: &AuthoredBodyLocator, ctx: &LoweringContext) -> SemanticNodeId`, in `verter_session` beside `lower_decl_body_with_provenance` (which today reads `&prepared.body` — re-routing that read is THE Stage-10 work). Source-kind-aware:

- (a) decl-body → the retained `DeclBodyMemo` `SnapshotLease`, keyed `SnapshotKey(canonical, whole_hash, parse_env_hash)`.
- (b) authored payloads → REUSE the producing canonical's `DeclBodyMemo` snapshot (whole-program `Rc<ParsedEvalProgram>`, spans SFC-absolute).
- Flow — **two-phase worker-purity split**: the PURE WORKER re-borrows the retained snapshot sub-position named by the locator's producer-emitted origin path (in the authored position's own lexical scope + type-param env) and returns transient OWNED typed-IR (no host / dispatch / `DeclLoweringService::run` re-entry, per the `decl_lowering.rs` purity contract); the SESSION phase graph-lowers that IR into a `SemanticNodeId` through the ONE shared dispatch and memoizes it under `LocatorLoweringKey`. Exception: the `parsed_type_argument` (b) sub-case session-lowers via `lower_type_expr_structural` (guard-locked `session_graph_lowerer_makes_no_query`), re-pointing `macro_hot_mirror::macro_type_arg_hot_ref` BEFORE/AS the field is deleted (no source-less window).
- Memo = the EXISTING `SemanticGraphStore` MULTI-CANDIDATE read-set-validated substrate (every warm hit validates `cached_satisfies` + `read_set_signature.validate_with_self_roots` against the caller's live view).
- `P1-LOCATOR-WHOLEHASH`: the deref-time `whole_hash` is sourced LIVE from the workspace generation / `StoreView` and RECORDED in the caller's read-set — never carried in the locator nor `LocatorLoweringKey` (R6). The `SnapshotLease` stays retained through the graph-lowering call site (dropping it before deref forces a reparse — a defect).

### 6.6 R6 / R21 / performance contracts

- Prep-time facts key on `parse_env_hash` only, invalidate on the owning file's content hash; `HeritageBaseFact` has NO `resolve_env_hash` in its identity (target resolution is dispatch-time). Frontier facts take `parse_env_hash + resolve_env_hash`, NO `lib_env_hash`; `ShallowRouteFacts` / `typeof_alias_target` `parse_env_hash` only. Lowered values memoize under `LocatorLoweringKey` and version-root via the node's read-set.
- R6 forbids — in cross-boundary identities AND any content-free query-identity key — generation ids, `SemanticNodeId`, `HotTypeRef`, content/whole hash, versioned `DeclIdentity`; explicit carve-out: the graph-internal read-set-validated value-rooted `SemanticQueryKey` keys (`Instantiate.args`, `ProjectMember.base`) legitimately carry a `SemanticNodeId` value root.
- R21 five split env-hash dimensions untouched; facts/locators key only on dims they depend on.
- Performance: per-generation memoization on the existing multi-candidate substrate (warm demand = memo hit, no re-borrow/re-lower); prep facts EAGER + cheap (like existing classifiers); body lowering LAZY on demand (preserving IndexedReady's zero-body-publish invariant); facts/locators are small owned data (net allocation reduction). Perf gates: no NEW reparse on the body path; warm hot-path traces show the memoized node; prep facts add no query-time cost; `hot_path_never_calls_materialize_type_expr` stays green.

---

## 7. [P1] — span-recovery-before-identity gate (BLOCKING for B1)

### 7.1 The soundness problem (verified)

`ObjectProperty` (`verter_type_expr/src/lib.rs:651`) derives `PartialEq`/`Eq`/`Hash` over ALL fields **including `spans: MemberSpans`** (`#[serde(skip)]` excludes it from the wire but NOT from identity; `MemberSpans` is itself `Eq + Hash`). The same holds for `MethodSignature`, `FunctionExpr`, `IndexSignature` (derived), and `FunctionParam` (hand-written `PartialEq`/`Hash` include `.span`). Therefore **member spans participate in node identity**: two structurally-identical members with different declaration sites compare unequal and hash differently.

The synthesized-(d) `ResolvedLocalType` path builds members via `ObjectProperty::with_spans_public(name, ty, field.is_optional, false, MemberSpans::name_only(field.span))` (`verter_semantic/.../macros.rs:1423`), where `field.span` is the REAL authored prop-name span. If a narrowed fact reconstructs/interns an `ObjectProperty` WITHOUT recovering `MemberSpans::name_only(field.span)` — e.g. with `Span::default()` / byte-0 — the reconstructed node is `Eq`/`Hash`-UNEQUAL to the authored node. That is a soundness failure (identity divergence → cache-key divergence). This is a **pre-implementation gate**, not carry-forward debt.

### 7.2 The sound mechanism

**Recover-via-locator-before-identity.** Spans are NEVER stored as fact fields (facts stay content-free; over-storing a `Span` as a fact-identity field is ITSELF a violation, enforced by the `NoStoredSpan` witness of §9). Instead, every fact family that reconstructs/hash-interns an IR struct whose derived identity includes span fields MUST carry a **producer-emitted origin locator** sufficient to recover the exact spans from the producing-canonical retained `DeclBodyMemo` snapshot, BEFORE any `Eq`/`Hash`/interning/projection/reconstruction.

**Explicit per-span-class contract (codex REVISE finding 4 — "member-origin path" alone is insufficient).** The design REQUIRES an explicit origin locator for EVERY identity-participating span class, not one blanket "member-origin path":

| Span class | Bearing struct(s) (`verter_type_expr/src/lib.rs`) | Required origin locator |
|---|---|---|
| `MemberSpans` | `ObjectProperty` (:651/:665), `MethodSignature` | a member-origin locator recovering `MemberSpans` (declaration / name / type_annotation spans) |
| `IndexSignatureSpans` | `IndexSignature` | an index-signature-origin locator recovering `IndexSignatureSpans` |
| `FunctionSpans` | `FunctionExpr`, `MethodSignature` | a function-origin locator recovering `FunctionSpans` |
| `FunctionParam.span` | `FunctionParam` (hand-written `PartialEq`/`Hash`) | a per-parameter origin locator recovering each `FunctionParam.span` |

A single **explicit `SourceSynthetic` variant** is permitted ONLY for a TRULY synthetic node with no authored origin (documented per site); it is never a fallback for an authored node whose locator was omitted.

Concretely for the synthesized-(d) object-member schema (the [P1] amendment target): the `ResolvedLocalTypeFact` object-member schema carries, per member, `name` + REQUIRED `optional: bool` + `ty` (fact-or-locator) + a **producer-emitted `MemberSpans` origin locator** recovering `MemberSpans::name_only(field.span)`. `readonly = false` / `visibility = Public` are producer-constants (documented as such). The origin locator is emitted by the producer, NEVER derived from `*.span` at reconstruction and NEVER an embedded `TypeExpr`.

### 7.3 Discriminating fixture spec (`fixture_p1_span_recovery`)

Coverage matrix — one fixture per identity-participating span class: `ObjectProperty`/`MemberSpans` (property member), `MethodSignature`/`FunctionSpans` (method member, also exercising `optional`), `IndexSignature`/`IndexSignatureSpans` (`[k: string]` vs `[k: number]`), `FunctionParam.span` (param span in hand-written identity), `FunctionExpr`/`FunctionSpans`. Each fixture drives the REAL reconstruction path (the body-source provider deref, not a hand-built struct) and asserts:

1. **Positive**: the node reconstructed via `fact + origin locator` through the real provider path produces an IR struct whose spans are byte-identical to the pre-change producer output (`MemberSpans::name_only(field.span)` for the synthesized case), so it is `Eq`/`Hash`-EQUAL to the authored node.
2. **Negative (MUST FAIL pre-change; PASS post-change)**: reconstructing the same node with `Span::default()`/zero (or lacking the origin locator) produces a struct that is `Eq`/`Hash`-UNEQUAL to the authored node. The fixture asserts BOTH `reconstructed_with_locator == authored` AND `reconstructed_with_default_span != authored` — proving discrimination. It fails against the pre-change tree precisely because default/reconstructed spans produce different `Eq`/`Hash`.
3. **Over-storage negative (structural)**: no fact family carries a `Span`-typed field — enforced by the `NoStoredSpan` marker witness (§9), NOT by `NoTypeExpr` (which admits `Span`). A fact family that added a `Span` field must fail to compile.

This closes [P1]: the synthesized object-member arm now specifies span recovery, and the fixture discriminates default-span reconstruction.

---

## 8. [P2] — explicit closed fact-schema enumeration (BLOCKING for B1)

The closed schema is an EXPLICIT enumeration, not "examples." The governing completeness rule: EVERY field that is identity-participating (`Eq`/`Hash`), `#[serde]`-persisted (NOT `#[serde(skip)]`), read by a publication/projection FILTER, or required by a reconstruction/lowering EMIT MUST become a REQUIRED stored fact field. Two carve-outs only: spans (recovered-via-locator per §7, never stored) and display-only passthroughs.

**B1 deliverable + structural witness (codex REVISE finding 5 — §8 must not read like examples).** For EACH narrowed struct, B1 produces an **exhaustive field-to-fact table** (every source field → fact field OR named carve-out) as a committed artifact, enforced by a **structural witness**: the producer that builds the fact from the source struct uses **exhaustive destructuring without `..`** (or a generated field-mapping), so ADDING a source field FAILS compilation until the fact schema is updated. This replaces the removed name-keyed `fact_families_preserve_live_non_typeexpr_member_metadata` scanner with a compiler-level obligation. The tables below name the required fields per family AND the specific live fields the review found missing from an examples-style draft — these are REQUIRED, not illustrative.

### 8.1 Surface A facts

| Fact family | REQUIRED stored fields | Locator/escape | Carve-outs |
|---|---|---|---|
| `HeritageBaseFact` | authored `name: String`; `type_args: Arc<[TypeArgLocator]>`; local `name_resolution` ref | member-origin locator for the base name span (§7) | resolved target identity NEVER stored (dispatch-time) |
| `ClosednessRecipe` | closed arms `ObjectClosed` / `IntersectionAllArms(Arc<[ClosednessRecipe]>)` / `Parenthesized(Box<ClosednessRecipe>)` / `MappedOpenParam` | `FollowSlot(ref-slot locator)` / `FollowLocator(AuthoredBodyLocator + role/position + a symbolic binding/substitution locator)` — never a live `KeyDomainBindings`/borrowed `TypeExpr`/`SemanticNodeId` (§5.1); live bindings re-minted at dispatch | — |
| `KeyDomainFact` | `Open` / `ClosedAbstract` / `FollowSlot` / `FollowLocator` | recipe-only arms; borrowed `ClosedExpr`/`ClosedNode` re-minted at dispatch, never stored | — |

### 8.2 Surface B facts

| Fact family | REQUIRED stored fields | Locator/escape | Carve-outs |
|---|---|---|---|
| `NarrowFrontierBody` | closed arms only: `ExportRoute` / `UnresolvedExternalRef` / `TypeParamShell` | `SymbolBodyLocator` for any resolvable body | NO object-members arm, NO general body arm |
| `ShallowRouteFacts` | object-member-names route (or `OpenKeyDomain`); member-path seed-ref; per-member dependency-edge; whole-route ref-closure | member-path locator where a body is needed | — |
| `ValueTypeAnnotationFact` | `typeof_alias_target: Option<ValueDeclIdentityPart>`; annotation classification | annotation locator | — |
| `NarrowTypeParam`/`TypeParamDeclFact` | `name`; ordinal/scope facts; `constraint_locator`/`default_locator` OR closed bound fact | constraint/default locators | — |

### 8.3 Surface C facts

| Fact family | REQUIRED stored fields | Locator/escape |
|---|---|---|
| `PreparedTypeBodyFacts` | body classification | `body_slot: TypeBodySlot` |
| `FunctionSignatureFact` | ordered param facts; `has_implementation_body: bool`; `NarrowTypeParam` for own type-params | return locator-or-fact; param `ty` locators |
| `ObjectShapeFact` | over all 5 `ObjectMember` variants; `Property`/`Method`: REQUIRED `visibility: MemberVisibility` (identity-participating, publication-filtered) + `optional: bool` + `readonly`; index-sig: declared `key_type` SHAPE (fact-or-locator, `[k:string]≠[k:number]`) + `value_type` + `key_name` + `readonly` | member `ty` locators; member-origin locators for spans (§7) |
| `EnumMemberFact` | ordered `name` → closed scalar (folded literal / sound primitive domain) | — |
| `PreparedMemberFact` | REQUIRED `optional`/`readonly`/`is_method`/`visibility`/`declaration_origin` | `ty` locator |
| `PreparedValueMemberFact` | as above + `is_method` | `ty` locator |
| `PreparedForwardPayload` | `target_name: String`; `forwarding_kind: PreparedForwardingKind` (`IdentityParams`/`AppliedAlias`) | `target_args: Arc<[TypeArgLocator]>` |
| `PreparedKeyFilterShape`/`PreparedKeyRemapShape`/`PreparedValueRuleShape` | shape discriminant | Opaque/Transform payload → LOCATOR |

### 8.4 `Analyzed*` / `Projected*` / synthesized facts (the [P2] named-instance surface)

**Span discipline in these facts (resolves the §7/§9 vs §8.4 contradiction).** A semantic fact NEVER stores a raw `Span`/`MemberSpans`/etc. field (`NoStoredSpan` is absolute, §9). Wherever the review flagged a "missing `span`," the requirement is met by a **declaration-span ORIGIN LOCATOR** (recovered per §7) or, for a truly synthetic node, the explicit `SourceSynthetic` variant — the span INFORMATION is preserved and recoverable, just not stored as a `Span` field. If a public/output surface genuinely needs a MATERIALIZED span VALUE (a publication/diagnostic wire span), that is a separate **output DTO OUTSIDE the `FactPayload` class** (a span value, not a `TypeExpr`, on the sealed output surface) — split from the semantic fact, never a stored field on it. So "span" below always means "declaration-span origin locator," never a stored `Span`.

The B1 exhaustive field-to-fact tables cover each struct in full and name each EXACT field or carve-out; the columns below name the REQUIRED fields and, in bold, the fields the review found MISSING from an examples-style draft (`crates/verter_semantic/src/analysis/types.rs`, `.../type_solver/query_engine.rs`):

| Fact family | REQUIRED fields (as facts + origin locators — no stored `Span`) | Display-only carve-outs |
|---|---|---|
| `AnalyzedPropField` fact (`types.rs:988`) | `name`; `is_optional`; **`declared_in_macro_type_arg: bool`** ([P2] NAMED instance — persisted + policy-consumed at `types.rs:1027-1037` + `verter_audit/src/published_surface.rs:227-232`, filter-read, NOT display); **declaration-span origin locator** (recovers the prop-name span; never a stored `Span`); **the `type_expr_scope` pairing** (the producing-canonical scope that pairs with the narrowed body locator); `type_constructor`/`has_default` where present | `type_annotation: Option<String>`, `description`, `tags`, `resolution_source`/`resolution_error` |
| `AnalyzedEmitField` fact (`types.rs:1051`) | `name`; **declaration-span origin locator**; **the `type_expr_scope` pairing**; emit-signature facts | display strings |
| `AnalyzedSlotField` + `AnalyzedSlotFieldBinding` (`types.rs:1084`) | `name`; `is_required`; **declaration-span origin locator**; **binding `has_default` + the `type_expr_scope` pairing**; binding facts | display strings |
| `AnalyzedOptionsProp` fact (`types.rs:1125`/`:1198`) | `name`; `is_required`; `has_default`; `type_constructor`; **declaration-span origin locator**; **the `type_expr_scope` pairing** | display strings |
| `AnalyzedExposeField` fact (`types.rs:1263`) | `name`; **declaration-span origin locator**; **the `type_expr_scope` pairing**; expose facts | display strings |
| `ResolvedLocalType` → `ResolvedLocalTypeFact` | synthesized object-member schema (`name` + `optional` + `MemberSpans` origin locator per §7); `TuplePayloadFact { readonly, elements: Arc<[TupleElementFact]> }`; `TupleElementFact { label: Option<String>, optional, rest, ty }`; indexed-access fact | `resolution_source`/`resolution_error` |
| `ProjectedMember` (`query_engine.rs:18`) | member `visibility`/`optional`/`readonly`; **`name`**; **`is_method`**; **`declared_in_macro_type_arg`**; **`declaration_origin`**; **member-span origin locator** | — |
| `ProjectedIndexSignature` (`query_engine.rs:86`) | `key_type` SHAPE + `value_type`; **`key_name`**; **`readonly`** | — |
| `ProjectedSurface` (`query_engine.rs:115`) | ordered call/construct signatures as `FunctionSignatureFact`; **`has_index_signature`** | — |
| `parsed_type_argument` (authored (b)) | authored payload facts | — (re-points `macro_type_arg_hot_ref`) |
| Svelte facts (`SvelteScriptFacts` + content-addressed candidates) | props/dispatcher-events facts + locators; **the `type_expr_scope` pairing + declaration-span origin locators** | — |

The exact per-field enumeration for every struct (including the precise `type_expr_scope`-pairing field name and any remaining persisted metadata field) is the B1 committed table; no field is left as a vague "scope field."

The structural witness (exhaustive destructuring without `..`, §8 intro + §9) keeps the metadata-loss class closed against future-added struct fields: a struct that grows a live identity-participating/persisted/filter-read field WITHOUT a corresponding fact field or explicit carve-out FAILS compilation.

This closes [P2]: `declared_in_macro_type_arg` (and the specific declaration-span origin-locator / `type_expr_scope` / `is_method` / `key_name` / `readonly` / `declaration_origin` / `has_index_signature` requirements the review flagged) are named as required fact fields or origin locators, every family enumerates its required non-`TypeExpr` siblings rather than listing examples, no fact stores a raw `Span` (so §8.4 is consistent with §7/§9), and the destructuring witness makes the enumeration compiler-complete.

---

## 9. Guards — STRUCTURAL-ONLY (REPLACES the shelved §6)

Per the forward-only landed-scanner bar (`4ca7692cd`), LANDED enforcement is compiler / type-system / tool-based; a NEW heuristic file-scanner keyed to a spelled source name/path/token is WIP-only. The shelved §6 was almost entirely name-keyed AST/call-graph scanners — **those are REMOVED, not ported**. A scanner that would need a change is removed, not edited. The landed enforcement is:

1. **`NoTypeExpr` derive + `assert_impl_all!` witnesses (PRIMARY enforcement)** on every internal semantic carrier (`Prepared*`, `Analyzed*`, `Projected*`, all closed fact families, `HotPrepared*`). A carrier that transitively owns a `TypeExpr` fails the `assert_impl_all!` at compile time (alias/nesting-laundering included — the marker trait is transitive). This — not field deletion — is the durable enforcement.
2. **Field deletion is migration PRESSURE, not durable enforcement** (codex REVISE finding 2): re-adding an UNUSED `TypeExpr` field can compile unless a witness rejects it, so deletion drives the migration but the `NoTypeExpr` witness (item 1) is what forbids re-introduction.
3. **`NoStoredSpan` (a.k.a. `FactPayload`) marker witness for span-free facts** (codex REVISE finding 2 + finding 4): `Span` IMPLEMENTS `NoTypeExpr`, so `NoTypeExpr` does NOT forbid a stored span. A SEPARATE marker (`NoStoredSpan`, or a private-constructor `FactPayload` witness) is derived on every fact family, asserting it carries no `Span`-typed field. This is what enforces the §7 "spans recovered-via-locator, never stored" contract.
4. **Module privacy / E0603** for the output-only materializers; the sealed `OutputProjector` output seam stays the only path a `TypeExpr` reaches from a semantic surface.
5. **Sealed traits** for the FN5.2 typed-degradation capability and the output-projection capability (no external impl can re-introduce a semantic `TypeExpr` return).
6. **Crate-dependency boundary** `no_verter_semantic_to_verter_session_dep` (STRUCTURAL; already green, stays) — without the dep edge `verter_semantic` cannot even NAME `HotTypeRef`/`SemanticNodeId`, so the "no `HotTypeRef` in the lower crate" property is compiler-guaranteed by topology.
7. **Real R6 type-level key witness** (codex REVISE finding 2): the locator and query-identity keys are built from **sealed allowed-dimension types** (or a derive that REJECTS `FileWholeHash`, content hashes, `SemanticNodeId`, `HotTypeRef`, and versioned `DeclIdentity`) — a compile-time proof that the forbidden dimensions cannot appear in the key, NOT a compile-fail fixture alone. `Hash + Eq` is compile-asserted. This supersedes any name-scan of key fields.
8. **Compile-fail fixtures** (trybuild-style, SUPPLEMENTARY to the witnesses above): a fact family declared with a `TypeExpr` arm fails; a lower-crate DTO naming `HotTypeRef` fails; a locator struct declared with a forbidden-dimension field fails; a fact family declared with a `Span` field fails (the `NoStoredSpan` negative).
9. **Exhaustive destructuring-without-`..` field-mapping witness** for [P2] completeness (§8): each fact producer destructures its source struct exhaustively, so a new source field fails compilation until mapped.
10. **Behavioral / oracle discrimination fixtures** (evidence tier, at every review tier): the §7 `fixture_p1_span_recovery` matrix; the §8 `declared_in_macro_type_arg` + method-optionality + member-visibility + index-key-shape + tuple label/optional/rest discrimination fixtures; the FN5.2 typed-degradation-vs-raw-`Unknown` fixture; the hash-input-conversion fingerprint-parity fixture (§3.1 trio produces the same fingerprint from facts as from the pre-change `TypeExpr`); `OwnerCollectionDb` anti-tear; hot-prepared read; per-surface A/B/C parity oracle (WIP-only, §10).
11. **Deletions at landing**: the residual inventory guard + file, `authored-shape-graph-native-migration-deferral.md`, `hot-materialize-tripwire-residual-deferral.md`, `q2-structural-body-cache-deferral.md`. The FN5.2 owner-file allowlist EMPTIES (the control-flow shape is gone) — the grandfathered fence + its doc delete; no new scanner is added.

Grandfathered (retained as-is, not modified): `hot_path_never_calls_materialize_type_expr`, `no_new_semantic_unknown_control_flow_outside_owner` (until FN5.2 empties it and it deletes).

Any guard mechanism change routes through governance + the codex rail (no implementer self-certification that "no structural mechanism fits").

---

## 10. Sequencing — B1–B8 as WIP slices of ONE atomic squashed landing

`P1-SEQ`: the CONSUMER-FLIP + FIELD-REMOVAL slices are WIP/review slices of ONE atomic landing group, NOT independently landable — narrowing a surface first removes fields other surfaces still read, producing a non-compiling / dual-pathed intermediate tree. Interim dual paths are WIP-only on a staging branch that squashes.

**Independently-landable scaffolding vs non-landable cutover** (codex REVISE finding 8). The following create NO dual production path and MAY land incrementally ahead of the atomic squash, de-risking it: (i) the `structural_body_cache` dead-code deletion (B2); (ii) pure NEW type definitions (the B1 locator + closed-fact families + `LocatorLoweringKey` + `SessionDemandIdentity`, so long as no consumer reads them yet); (iii) marker derives (`NoStoredSpan`) and their trybuild compile-fail fixtures; (iv) the [P1]/[P2] discriminating fixtures against retained pre-change behavior. **Non-landable (atomic-only)**: every consumer flip and every `TypeExpr` FIELD REMOVAL — these land together in the squash. The production hash-input conversion (§3.1 trio) and the P1/P2 schema fixes land BEFORE the consumer cutover within the atomic group.

| Block | Scope | Review tier |
|---|---|---|
| **B0** | This design gate — live census, binding design, [P1]/[P2] closure, ratification. | S |
| **B1** | Locator + closed-fact SUBSTRATE in `verter_type_expr` (`AuthoredBodyLocator` + all §6.3 fact families, closed, `NoTypeExpr`-witnessed); session-side `LocatorLoweringKey` + `SessionDemandIdentity` definitions. No consumer flip yet. [P1]/[P2] must be closed here — the fact schemas + member-origin span-recovery paths are DEFINED in B1. | S |
| **B2** | Body-source provider `lower_locator` + re-route `lower_decl_body_with_provenance` to lower from `DeclBodyMemo` via the locator; validated multi-candidate memo under `LocatorLoweringKey`. DELETE `structural_body_cache` + machinery (incremental-landable). | S |
| **B3** | Surface C narrowing IN PLACE — `Prepared*`/`Analyzed*`/`Projected*`/type-params/serde-`Wire` helpers → facts + locators (`verter_semantic`, crate boundary kept). | S |
| **B4** | Session hot-prepared cutover — wire `HotPrepared*` as the live graph-backed session prepared surface; `PreparedDeclBundle` publishes the atomic facts + hot-prepared bundle under the completion fence. | S |
| **B5** | Surface B narrowing — `ShallowFileState` route closures, `external_type_frontier`, `eval_env` typeof peel → locators + `ShallowRouteFacts` + `typeof_alias_target` + `NarrowFrontierBody`. **Plus the production hash/fingerprint-input conversion** (`compat_type_body_hash_input`, `compat_value_body_hash_input`, `LazyBodyFactSource::compute`) → fact/locator/stable-hash inputs, with the fingerprint-parity fixture; reduce the two oracle readers to test-only. | S |
| **B6** | Surface A + orphan-carrier migrations — heritage facts, graph-native closedness/key-domain, `OwnerCollectionDb`, imported-registry body identity, component-meta registry helpers, `ProjectedMember`/`ProjectedSurface`, `FastShallowFieldExpr`, `NamedTypeMember` semantic arm. | S |
| **B7** | Framework/output boundary + FN5.2 — Svelte persisted facts, `framework_surface` DTO split (internal semantic vs sealed output), `NamedTypeMember` output arm, FN5.2 typed-degradation carrier at the 6 owner sites. | S |
| **B8** | Landing — delete residual inventory + guard + all three deferral docs; correct §3.5 stale prose; remove WIP parity rail; land the §9 structural guards; squash B1–B8 into one atomic clean cutover; full gate. | S (A/C for mechanical import/serde/doc cleanup inside, only after the owning S review passes) |

**Parity rail (WIP-ONLY, TEST-ONLY)**: a `TypeExpr` oracle proving each narrowed/removed surface produces byte-identical answers as the pre-change `TypeExpr` path — Surface-A comparison over published component-meta + fallthrough/root-inheritance metadata + the open-key-domain L1 carrier-stop disposition (NOT resolved-type equality). NEVER linked into landed production code; REMOVED at B8 before the §9 guards pass (the guards fail if any `TypeExpr` carrier/walk, including the oracle, survives).

---

## 11. Non-negotiable invariants (imported kernel §8, extended)

1. **EXACTLY ONE query-time engine** — prep-time fact producers are syntax-only classification; no second query-time resolver, no per-surface walker; all cross-decl recursion stays in dispatch over the graph.
2. **No-bridge rule** — no `HotTypeRef → TypeExpr → semantic decision` on any hot path.
3. **R6** — locators + cross-boundary/query-identity keys are content-free; no `HotTypeRef`/`SemanticNodeId`/content-hash/versioned `DeclIdentity`; the graph-internal node-rooted keys (`Instantiate.args`, `ProjectMember.base`) are the only carve-out.
4. **R21** — five split env-hash dimensions untouched; each key scopes only the dims it depends on.
5. **Shallow-by-default / carrier-preserving lowering** — shallow/frontier surfaces stay graph-free and available before the graph; the L1 open-key-domain carrier-stop is preserved.
6. **Crate boundary** — `verter_semantic` never depends on `verter_session`; Surface C narrows in place; locators/facts live lower-neutral in `verter_type_expr`; `HotTypeRef` lives ONLY in `verter_session`.
7. **Completion fence / no torn warm publication** — the atomic facts + hot-prepared bundle publishes only complete, revalidated results; cancelled/superseded/budget-exceeded/interrupted results never warm caches.
8. **Single clean cutover** — interim dual paths WIP-only; the end-state deletes the legacy `TypeExpr` carrier/walk with no legacy path, no shim, no compat flag.
9. **No stubs; discriminating tests only** — every removal/narrowing lands with a test that FAILS pre-change and PASSES post-change.
10. **Structural landed enforcement** — no new landed name-keyed scanner (§9); the Stage-9 tripwire stays grandfathered.

---

## 12. Deferrals — ZERO semantic `TypeExpr` deferral

No semantic-`TypeExpr` deferral is acceptable out of Stage 10. The only legitimate future homes:

- **Stage 11** — quarantine + rename of the surviving syntactic/output/diagnostic/JSDoc/protocol `TypeExpr` class: the sealed `OutputProjector` seam, protocol/JSON DTOs, display strings, and JSDoc `{Type}` payloads ONLY. This is a census/rename, not a semantic-`TypeExpr` deferral. It explicitly does NOT include the §3.1 hash-input trio (converted in Stage 10) or the two oracle readers (reduced to test-only in Stage 10) — hash inputs, fact emission, cache identity, typeinfo contributor computation, and query control flow are semantic infrastructure finished IN Stage 10.
- **Stage 12** — measured perf compaction, including any future SOUND structural-template cache (the deleted `structural_body_cache` is NOT resurrected here — a Stage-12 cache is a fresh, correctly-keyed design).
- Broader TypeScript-parity work in the U-block roadmap (unrelated to `TypeExpr` removal).

Each, if invoked, records the six-field debt row (item / why-not-now / owning block / temporary behavior / fail-closed guard / closure condition). Stage 10 itself carries none.

---

## 13. Verification & documentation upkeep

- **Gate**: `node scripts/gate.mjs` (both surfaces — `cargo nextest run --workspace` process-isolation + `cargo test -p verter_session --tests` in-process), `cargo clippy --workspace -- -D warnings`, `cargo fmt --all --check`, `pnpm install --frozen-lockfile`. Full green on the final squashed tree.
- **Evidence tier** (unchanged by the review tier): the §7 [P1] span-recovery matrix; the §8 [P2] discrimination fixtures; the FN5.2 typed-degradation fixture; per-surface A/B/C behavioral parity; warm-state/perf checks (no reparse, no duplicate lowering, validated memo hits); `hot_path_never_calls_materialize_type_expr` green.
- **Documentation upkeep at landing (B8)**: update `CLAUDE.md` (§3.5 "three deferred SEMANTIC reader classes" bullet → final-state prose; the "Shallow File Processing Core Invariant" hot-read-path bullet's dual-representation language); `docs/arch/parselower-design.md` (the ~40/7 counts + the Stage-6 dual-representation stanza → terminal-state); `docs/arch/semantic-db-overhaul-unified-remaining-plan.md` (the ~40 count + append the locked-designs row, §15); `.claude/skills/type-resolution/SKILL.md` + `.claude/skills/component-meta/SKILL.md` (the ~40/7 counts + reader-class prose). Delete the three deferral docs + the residual inventory.

---

## 14. Risk & mitigation — the atomic landing

**Risk**: B1–B8 squash into ONE atomic clean-cutover commit. A single large squashed landing is hard to bisect, and a post-landing regression reverts the whole terminal block.

**Mitigation**:

1. **Per-slice TIER-REQUIRED review on the staging branch** — every slice is S-tier (§10), so each gets the FULL 3/3 review set (adversarial claude + claims-aware codex + unprimed codex) + discrimination verification BEFORE the squash, not merely "dual review" (codex REVISE finding 8). The squash itself is mechanical (no new code introduced at squash time).
2. **The WIP-only `TypeExpr` parity oracle** runs across EVERY slice, proving byte-identical published surfaces (component-meta, fallthrough/root-inheritance, L1 carrier-stop) against the pre-change `TypeExpr` path — per-slice confidence before the atomic squash. It is removed at B8, and the §9 structural guards then prove no `TypeExpr` carrier/walk (including the oracle) survives.
3. **Full-workspace gate on the final squashed tree** — both surfaces green, clippy/fmt clean.
4. **Independently-landable scaffolding lands ahead of the squash** (§10) — `structural_body_cache` deletion, the new locator/fact type definitions, the `NoStoredSpan` marker + trybuild fixtures, and the P1/P2 discriminating fixtures create no dual production path and each shrink the atomic group by one risk unit.
5. **The CONSUMER FLIP + FIELD REMOVAL are atomic-only** — because the interim tree is non-compiling/dual-pathed between those slices, incremental landing of the representation CHANGE is architecturally impossible (a partial landing would be the forbidden landed dual path). The mitigation there is review/oracle rigor on the staging branch, not slicing the landing.

---

## 15. Provenance (shelved-design import map — NOT binding)

The shelved draft `9062d4baf` is provenance-only. Import map:

- **Imported in substance** (mechanism kernel, still correct on the merits): §§0–1 A/B/C dispositions; §2 Surface-A mechanism; §3 Surface-B graph-free boundary; §4.1–§4.4 lower-crate topology; §5.1–§5.6 locator/fact/provider concepts (after closing [P1]/[P2] and adding the terminal `DeclBodyMemo`/`HotPrepared*` reconciliation); §8 invariants.
- **REPLACED**: §4.5 scope boundary (the false "flip before Stage 10" premise → §4 here, absorbing the orphaned carriers); §6 guard plan (name-keyed scanners → §9 structural-only); §7 sequencing (three-surface cutover → §10 whole-terminal-surface B1–B8).
- **ADDED (absent from the shelved draft)**: the `HotPrepared*` reconciliation (§5.5); the `structural_body_cache` deletion (§5.5); the whole-tree orphaned-carrier census + absorption (§3.2, §4, §5.4); FN5.2 typed degradation (§5.6); the structural landed-guard policy (§9); the resolution of the "permanent split-carrier vs full graph-native" open question toward full graph-native (§2).
- The shelved draft's "do not re-litigate" language is NOT carried as authority — the dispositions stand because they remain correct, re-confirmed by the codex-2/2 scope panel, not because the draft asserted finality.

---

## Appendix A — census figures (verified @ `4ca7692cd`, for ratification)

- Residual inventory: **39 semantic** (1 `GraphBackedMigrated` + 3 `ProducerLowering` + 17 `AuthoredShape` + 12 `GraphFreeDto` + 6 `GraphBackedPending`) + **5 `OutputCompat`**. Terminal target: 35 migratable semantic readers → 0; 3 ProducerLowering → producer bridge; 1 already migrated; inventory file deleted. The 5 `OutputCompat` rows SPLIT (codex REVISE finding 1): the hash-input trio (`compat_type_body_hash_input` / `compat_value_body_hash_input` / `LazyBodyFactSource::compute`) CONVERTS in Stage 10; the two oracle readers reduce to test-only — none is a Stage-11 production survivor.
- `HotPrepared*`: 15 carriers, `NoTypeExpr` + `assert_impl_all!` enforced, zero production callers → production-wired.
- `structural_body_cache`: empty, dead accessor, never populated → deleted.
- FN5.2: 6 owner files, single owner mapping `semantic_query_error_raw` → typed degradation; owner allowlist empties.
- [P1]: `ObjectProperty` span IS in derived `Eq`/`Hash`; synthesized members use real authored `field.span` via `MemberSpans::name_only` → recover-via-locator-before-identity.
- `ResolvedLocalTypeFact` is the PROPOSED fact name; the live carrier is `ResolvedLocalType`. `OwnerCollectionDb` is TypeExpr-**valued** (key `(Arc<str>, Arc<str>)`), not keyed.
