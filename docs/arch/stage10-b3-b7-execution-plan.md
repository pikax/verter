# Stage 10 — B3–B7 execution plan + parallelization DAG (SEQUENCING AUTHORITY)

**STATUS: SEMANTIC-`TypeExpr` GAP CLOSED + §9.3 SINGLE-ENGINE FIREWALL-FORM DECIDED — the SEMANTIC-`TypeExpr` DAG + forks DECIDED and triple-review-validated; the §7 design re-census is COMPLETE and
the `type_expand` sub-design is DECIDED. The §9.3 SINGLE-ENGINE-axis firewall FORM (the one previously-open fork) is now DECIDED: **form (c) — full DTO-mediated consolidation, enforced by a Cargo dependency-graph query-package firewall — which CLOSES the §9.3 deferral and RE-SCOPES B6** (see the AMENDMENT note; CTO firewall-form decision, two independent unprimed code-verifying codex legs converged). All blocks B3–B7 are dispatchable on the SEMANTIC-`TypeExpr` axis (subject to their DAG edges); B6's SINGLE-ENGINE-axis landing is now UNBLOCKED under the re-scoped form-(c) in-block consolidation (the B1–B8 atomic squash is no longer gated on an open fork).**
The scoping pass VALIDATED the parallelization DAG, the FN5.2→B6 redraw, the boundary forks, and the four
carrier/mechanism forks (all survived adversarial 3/3 review + a fix-cycle re-review). It ALSO discovered — via
review + a code-verifying codex consult — that the binding design's residual census (§3, "39 semantic readers")
was materially INCOMPLETE (an entire class of query-time resolved/generated `TypeExpr` surfaces was unscoped).
**That gap is now CLOSED (§7):** a systematic re-census (design §3.6) enumerated the COMPLETE ≈96-surface
semantic-`TypeExpr` set (≈24 previously unscoped — dominated by the 15 `component_meta.rs *Analysis` carriers +
the newly-found `html_intrinsics` catalog); the `type_expand`/`ExpandedField` THREE-surface sub-design is DECIDED
(two unprimed codex legs + a code-verifying decider — design §5.7); the `svelte_default_synth` reassignment is
ratified (semantic cut → B6; §7.2); the newly-found surfaces are assigned (design §3.6); the dead
`resolver_core::type_expansion` is confirmed DELETE-in-B6; and the terminal bar (all semantic `TypeExpr` → 0 by
B8) is **ACHIEVABLE with NO semantic-`TypeExpr` blocker** (no Stage-11/12 deferral). The DAG topology and all fork
decisions are UNCHANGED — the re-census GREW several block scopes but PRESERVED Wave-1 `B3∥B5` and Wave-2 `B4∥B7`
file-disjointness (§2.2, re-verified). **(SCOPE CORRECTION — read with the AMENDMENT note below: this "NO blocker /
UNCHANGED" is the SEMANTIC-`TypeExpr` axis; the AMENDMENT's ONE SINGLE-ENGINE-axis fork — the §9.3 firewall
FORM — is now DECIDED (form (c)). It grows the B6 touch-set (a new `verter_session_query` Cargo crate/package + a dependency-neutral macro-DTO crate + a `verter_compiler`
public-API change + the parser-expander reduction) — all Wave-3-local, disjointness-preserving — and CLOSES the deferral.)**

**P1/P2 sequencing (supersedes the "dependency-neutral macro-DTO crate" as an initial deliverable throughout this doc).** The neutral macro-DTO crate (`verter_macro_dto`) + `SemanticMacroSurfaceShell` + the shell's PUBLIC constructor and its P2 crate-graph-valid construction authority land in P2 (macro-surface consolidation), together with the `ResolvedElements` split. P1 establishes the query-boundary `verter_session_query` Cargo package, the Cargo dependency-graph firewall guard, and the query host port only. Wherever this doc lists the macro-DTO crate / a "dispatch-mint" shell constructor in the B6 touch-set, read it as the P2 deliverable (design §9.3, P1/P2 sequencing note). The constructor is public because the shell (neutral crate) and its `ProjectSemanticDispatch` minter (query crate) are cross-crate, and Rust cannot express a private cross-crate "dispatch-mint" constructor; its construction authority is made crate-graph-valid at P2 (a co-located constructor+minter, or an unforgeable capability/witness) — the precise mechanism settles at P2 scoping.

**AMENDMENT (independent-confirm, below-`verter_semantic` `VueMacroElements` hole).** A post-land confirm found ONE
semantic-`TypeExpr` surface the earlier `verter_session`/`verter_semantic`-only scope structurally omitted: the
`verter_parser` `ResolvedProp`/`ResolvedNamedCallSignature.type_expr` carriers (embedded in `ResolvedElements`,
persisted whole as `SemanticNodeData::VueMacroElements` AND in the SECOND `ProjectTypeStore` cache `ResolvedTypeCacheDb`,
warm-cloned on the imported-`.vue` hot path). It is now OWNED by **B6** (design §3.6 rule-1 class (vi) / §5.4 / §9.3;
execution-plan §2.1 B6 GAINS + §2.2) — ALL query-time holders (`VueMacroElements`, `ResolvedTypeCacheDb` [DELETED, not converted],
and the session `ResolvedElements` resolver holders) convert to NoTypeExpr fact+locator DTOs that PROJECT through the ONE dispatch
(the confirmed second engine — the parser `type_surface` structural expander + the session `frontier_engine` — is severed as
their SEMANTIC AUTHORITY). Under the DECIDED firewall form (c) the codegen/SFC path is CONSOLIDATED onto the dispatch-produced
macro DTO and the parser expander is deleted or reduced to syntax-only extraction = the **§9.3 obligation CLOSED IN B6 (the CTO
firewall-form decision — no carried deferral)** — invariant-1 (EXACTLY ONE query-time engine) complete for BOTH the query-time
surface AND the codegen/SFC producer. Terminal-`TypeExpr` bar ACHIEVABLE on the SEMANTIC-`TypeExpr` axis (zero semantic
`TypeExpr` persisted/warm-cloned on the query-time carriers). **SCOPE CORRECTION to the top-of-doc headline.** The headline's
"ACHIEVABLE with NO blocker" and "all fork decisions are UNCHANGED" are scoped to the SEMANTIC-`TypeExpr` axis; this amendment
RESOLVED the ONE previously-open NON-semantic-`TypeExpr` fork on the SINGLE-ENGINE axis: the §9.3 structural callability-firewall FORM.
The three established forms were (a) query-time Cargo crate/package extraction (`verter_session_query`, no parser/compiler/OXC dependency) + Cargo dep-graph guard + drop the `verter_compiler` re-export; (b) de-publicize+cap-gate every
expander read AND write entrypoint; (c) full DTO-mediated consolidation. **DECIDED (CTO firewall-form decision — two independent unprimed
code-verifying codex legs converged, no decider needed): form (c), which CLOSES the §9.3 deferral and RE-SCOPES B6**, its durable structural
enforcement a Cargo dependency-graph query-package firewall + a P2 crate-graph-valid shell-construction authority (design §9.3). So **B6's SINGLE-ENGINE-axis
landing is UNBLOCKED** under the re-scoped in-block consolidation (the SEMANTIC-`TypeExpr` terminal bar was achievable regardless of form).
§4.1 scope + §3.6 ownership-by-rule are WIDENED to cover below-`verter_semantic`
graph-persisted payload DTOs (tightened to "a crate `verter_semantic` transitively depends on", excluding above-semantic output
crates like `verter_protocol` → B7). **DAG re-verified under form (c):** B6 runs ALONE in Wave 3, so its
touch-set additions — now including a new `verter_session_query` Cargo crate/package + a dependency-neutral macro-DTO crate + a `verter_compiler` public-API change + the `verter_parser` expander reduction —
are same-wave-disjoint BY CONSTRUCTION (none is claimed by a same-wave concurrent block: `host_manage.rs` is a serial cross-wave B5(W1)/B6(W3) share;
the frontier surface is B5 Surface-B territory, cross-wave serial; `verter_parser` + `verter_compiler` are NEW crates that NO Wave-1/Wave-2
block claims). The robust invariant is same-wave
DISJOINTNESS — preserved under form (c) because B6 is Wave-3-alone (any crate/file it touches is disjoint from concurrent
blocks); form (c) grows B6's touch-set SIZE (the codegen/session-boundary consolidation) but NOT the Wave-1 `B3∥B5` / Wave-2 `B4∥B7`
file-disjointness (B6 is neither in Wave 1 nor Wave 2). B6's growth is a WAVE-3-LOCAL re-scope, absorbed before dispatch/landing. The B7 `ComponentMetaResultDb` value split is clarified to name
BOTH `ResolutionTemplate` and `CachedComponentMetaResult.analysis: ComponentMetaAnalysis`.

This document sits UNDER the binding design
[`stage10-typeexpr-terminal-removal-design.md`](./stage10-typeexpr-terminal-removal-design.md) (mechanism/
schema authority) and the field maps [`stage10-fact-schema-field-maps.md`](./stage10-fact-schema-field-maps.md);
it decides sequencing/parallelization + the open carrier forks the design left as menus, and records the
census gap it found. Authored on `refactor/semantic-db-overhaul` @ `84b2d59a8` (B0/B1/B2 landed+confirmed).

**Provenance (record, not architectural evidence).** The DAG + fork decisions were made on the codex
architecture rail (unprimed, effort-neutral, best-on-merits); divergences resolved by a code-verifying
codex decider; the plan passed a design-bearing 3/3 review whose findings are folded in here. The
correctness of each decision rests on the source facts cited, not on the process labels.

**Numbering.** Canonical = the design's B3–B8 (used here + in the field-maps doc). A brief saying "B3–B6"
is the compressed paraphrase for the five remaining implementation blocks B3–B7 (B8 = atomic squash).

---

## 0. Fixed constraints (not decided here)

- **Landing is atomic.** B1–B8 squash into ONE clean cutover (design §10, §14). Every consumer-flip and
  `TypeExpr` field REMOVAL lands together; interim dual paths are WIP-only on the staging branch.
  **Parallelism here is concurrent IMPLEMENTATION on the staging branch, never landing** — landing always
  serializes into the one linear-ff squash. Because the intermediate tree is deliberately non-compiling
  between narrowing slices, "assemble on staging" between waves means cherry-pick/merge WITHOUT a green
  compile gate; per-slice proof is the WIP parity oracle (design §10), not a green staging tree.
- **B0/B1/B2 landed.** The locator/fact substrate (`verter_type_expr::{facts,locators,span_origins,
  fact_witnesses}`), the `lower_locator` provider, the `LowerLocator` query, the sealed
  `LocatorLoweringKey`/`InstantiateKey`, and the `InstantiateBodySource` axis all exist.
- **Fact schema fixed** by design §8 + field-maps. **Crate boundary** `verter_semantic ⊥ verter_session`
  compiler-enforced; `HotTypeRef` ONLY in `verter_session`; locators/facts lower-neutral in `verter_type_expr`.

---

## 1. The decided execution DAG

Two independent unprimed codex legs converged on the core; a third code-verifying decider resolved the one
divergence (FN5.2 placement + second parallel wave, §3). All of this survived a subsequent adversarial 3/3
review of this plan.

```
Prelude  TP0 : shared NarrowTypeParam / TypeParamDeclFact LOCATOR SUBSTRATE in verter_type_expr
                (facts.rs + locators.rs + session locator-deref/identity + witnesses — a real
                 prelude, NOT just a fact type; B3/B5 are CONSUMERS, never competing producers)
                │
        ┌───────┴────────┐
Wave 1  │  B3  ∥   B5    │   VALIDATED FF-safe (per-file disjoint; see §2)
        └───────┬────────┘
                │  (assemble slices on staging — no green-compile gate, atomic model)
        ┌───────┴────────┐
Wave 2  │  B4  ∥   B7    │   VALIDATED FF-safe — §7 re-census RESOLVED: svelte-synth cut → B6, so
        └───────┬────────┘   B4∩B7 = ∅ (B7 = output boundary + ComponentMetaResultDb split only).
                │
Wave 3        B6*             consumes B3 schema + B4 hot surface; owns FN5.2 + type_expand internal (§7)
                │
        B8   serial atomic squash landing
```

**DAG edges (verified dependencies):**

| Edge | Reason |
|---|---|
| `TP0 → B3`, `TP0 → B5` | Both consume `NarrowTypeParam`/`TypeParamDeclFact`; one lower-neutral producer prevents two worktrees forking it. TP0 must also extend `locators.rs` + session deref so type-param constraint/default positions are ADDRESSABLE (they are not today). |
| `B3 → B4` | B4's `HotPrepared*` consumes B3's narrowed `Prepared*`/`Analyzed*` fact schema. |
| `B3 → B6` | B6 reads B3-owned narrowed `Projected*` facts + lower-crate fact/locator schema. |
| `B5 → B4` | Shared `crates/verter_session/src/host_manage/eval_env.rs` (B5 typeof-peel; B4 `component_meta_binding_type_entries`). |
| `B4 → B6` | Shared `crates/verter_session/src/project_semantic_dispatch/build.rs` (B4 `build_typeof`; B6 `class_heritage_bases`); B6 migrates orphan consumers after the hot-prepared bundle exists. |
| `B6 → B8`, `B7 → B8` | fold into the squash. |

**Parallel-wave FF-safety (evidence in §2):**

- **Wave 1 `B3 ∥ B5` — VALIDATED FF-safe.** B3's files are all `verter_semantic`; B5's are `verter_session`
  PLUS `verter_semantic/src/facts/hashing.rs` (the no-`TypeExpr` fingerprint-encoder API — fork c, §4). The
  file INTERSECTION with B3 is still ∅ because B3 does not touch `hashing.rs`. The disjointness rests on the
  per-file lists in §2.1, not on a "B5 = verter_session only" crate claim (which is false). Residual risk:
  neither worktree edits the shared `verter_type_expr` type-param substrate — that is TP0, done first.
- **Wave 2 `B4 ∥ B7` — VALIDATED FF-safe (§7 re-census RESOLVED).** The §7 re-census risk — that the
  `svelte_default_synth` cut at `project_semantic_dispatch/build.rs:940` (a B4 file) would grow B7 into a
  same-wave `build.rs` collision with B4 — is DISSOLVED: the svelte-synth SEMANTIC-carrier cut was reassigned to
  **B6 (Wave 3)**, which already owns `build.rs`. B7's grown scope is now output-boundary-only (Svelte persisted
  facts + `SvelteScriptProvider::VERSION`/`stable_candidate_hash` + framework/output DTOs + the
  `component_meta_result_db.rs` semantic/output snapshot split + vue `runtime_ctor`) — none a B4 file — so
  B4∩B7 = ∅ holds. (§2.1 addendum + §2.2 build.rs row.)
- **Wave 3 `B6`** runs alone; coupled to B3 (schema), B4 (`build.rs`), owns FN5.2 + the type_expand internal
  carrier. Running last removes remaining conflicts.

**Why not more parallelism.** The only other file-disjoint pairs are `B3∥B7` / `B5∥B7`; full B7 output-projects
over the final internal representation, so it is best not scheduled ahead of B6. `B4/B6` and (pre-redraw) `B6/B7`
share files and MUST serialize. Two parallel waves at ≤2 wide + a serial tail is the clean plan; forcing a third
parallel pair re-conflicts on a shared file.

---

## 2. File-touch-set evidence (verified @ `84b2d59a8`)

Residual readers extracted from `crates/verter_session/tests/cases/residual_type_expr_body_reader_inventory.rs`
(`RESIDUAL_BODY_READERS`, `COMPAT_BODY_READERS`) and cross-checked first-hand. Paths repo-relative. **This set
is the KNOWN scope; §7 adds surfaces the inventory itself omits.**

### 2.1 Per-block file-touch set (known scope)

**TP0 (`verter_type_expr` + session deref)**
- `crates/verter_type_expr/src/facts.rs` — `NarrowTypeParam`/`TypeParamDeclFact` (already models
  constraint/default as `TypeBodySlot`).
- `crates/verter_type_expr/src/locators.rs` + `crates/verter_session/src/decl_body_memo/locator_deref.rs` —
  extend so a type-parameter constraint/default POSITION is addressable + deref-able (not possible today);
  update `fact_witnesses.rs`.

**B3 — Surface C (entirely `verter_semantic`)**
- `crates/verter_semantic/src/analysis/type_solver/prepared.rs`, `.../analysis/types.rs` (incl. the manual
  `Serialize`/`Deserialize` `Wire` for `AnalyzedMacro.parsedTypeArgument`), `.../analysis/type_solver/query_engine.rs`
  (`Projected*` DEFINITION), `.../analysis/type_eval.rs` (`FunctionSignature`), `.../analysis/fact_projection.rs`
  (demo/witness producer). B3 does NOT touch `facts/hashing.rs` (keeps Wave-1 disjoint from B5).

**B4 — Session hot-prepared cutover (`verter_session`)**
- `resolver_core/hot_prepared.rs`, `resolver_core/prepared_decl.rs`, `host_manage/prepared_decl.rs`,
  `host_manage/eval_program.rs`; GraphBackedPending annotation readers `project_semantic_dispatch/build.rs::build_typeof`,
  `resolver_core/runtime_values.rs::prepared_value_decl_to_value_decl_info`,
  `host_manage/eval_env.rs::component_meta_binding_type_entries`.

**B5R — Surface-B route facts (landed; Model B+ producer/downstream split)**
- The route half of B5 landed as B5R under the DECIDER-binding Model B+: the PRODUCER emits per-decl
  DIRECT route facts graph-free at the lazy lowering hook (`decl_body_memo.rs::lowered_type_decl_from_group`
  → `verter_semantic::facts::route_facts::produce_shallow_route_facts` over the transient contributor bodies +
  the hash-free `RouteFactLens`, a SECOND view of the shallow import/header tables carrying the FULL
  three-field import target — the fingerprint `ShallowLens`/`CrossDeclLens` and the R12 grammar untouched);
  the same-file TRANSITIVE closure + cycle/budget live DOWNSTREAM in the shared fact-closure core
  (`verter_semantic::facts::route_closure::route_closure_over_facts`), driven by `ShallowFileState` through
  `SfsRouteFactProvider` (stored sibling facts + `type_deps` + header/import reads — never a body re-walk,
  never a second engine).
- Final schema (V1/V2/V3): `ShallowRouteFacts.whole_route_edges: Arc<[WholeRouteEdgeFact]>` —
  `Local{name, route, context}` | `External{external_ref, context}` | `DeferredKeyUtility(edge)` with the
  per-EDGE `WholeRouteContextFact{Root|CallableParam|LeafProperty}` (context on BOTH the Local AND External
  arms — byte-parity-load-bearing for the LeafProperty-follow composition; oracle-proven);
  `member_path_seed_edges: Arc<[MemberPathSeedEdge]>` with the terminal-vs-boundary discriminator
  (`TerminalDeps` exact-match / `ForwardBoundary` bare-ref tail-append; complex terminals fail closed);
  the V3 `DeferredKeyUtilityEdge` records the cross-decl Pick/Omit/indexed key-source RECIPE only
  (`KeyDomainFact::FollowSlot` of the local alias) — enumerated downstream, fail-closed;
  `RouteDependencyRefFact::Local{name, route}`; canonical `verter_type_expr::RouteDemand` with the
  normalized private `RouteKeySet` (sub-fork A collapse — Eq/Hash agree; `resolver_core` re-exports).
- The 8 legacy `lookup_object` route-walk readers swapped atomically onto the fact closure; the deep
  `&TypeExpr` walkers were deleted (`collect_whole_route_refs`, `follow_local_symbol_precise`,
  `follow_routed_expr`, `utility_route_for_ref`, `extract_string_literal_keys_from_type_expr`,
  `extract_indexed_access_route/base`, `collect_member_path_seed_names`, `direct_object_*`,
  `extract_member_deps`, `WholeRouteContext`, `ExternalRefAccumulator`). Byte-parity oracle:
  `verter_semantic/src/facts/route_facts_tests.rs` (independent golden port vs the new pipeline;
  `unresolved_external` + `status` byte-identical; discrimination-verified).
- **V3 status — precise.** The V3 DESIGN (recipe + downstream tri-state enumeration in
  `route_closure.rs`) and the byte-parity of the FOLLOWABLE V3 subclass (bare local key alias, incl.
  chained aliases) are proven in the core + oracle. The SESSION hand-off
  (`SfsRouteFactProvider::key_source_bodies`) is NOT wired today — it returns `None` (B8-deferred),
  which the closure treats as the tri-state `DeferredKeyOutcome::Unavailable`: fail closed, the
  deferred edge contributes nothing and the empty-keys fallback does NOT fire (under-produce, never
  a wrong route). B8 hookup: implement `key_source_bodies` by dereferencing the
  `KeyDomainFact::FollowSlot` symbol locator to the alias's contributor bodies through the B6
  locator-deref / key-domain-dispatch machinery (it joins the other read-side `lookup_object`
  consumers deferred to B4/B6 — `verter_session` does not compile on this branch until those land).
- **Persists PAST B8 (structural recipe limit, safe direction):** a composite key expression
  containing a local alias (`Pick<Q, K | 'c'>`, `Pick<Q, K1 | K2>`) poisons produce-time key
  extraction — no recipe, nothing emitted — and a non-outermost deferred index (`Imported[K]['x']`)
  likewise mints no edge; the single-symbol `FollowSlot` recipe cannot carry a composite key
  expression or an inner-level deferral, so B8's hand-off wiring does NOT close these two classes
  (a richer recipe would). Both are fail-closed under-production, never a wrong route
  (characterized: `characterize_composite_key_alias_under_production_vs_legacy`, plus the
  fail-closed cases in `parity_cross_decl_pick_follow_slot` /
  `deferred_key_source_unavailable_fails_closed_not_wrong_route`).

**B5 — Surface B + hash-input trio (`verter_session` + one `verter_semantic` file)**
- `resolver_core/shallow_file_state.rs` (8 route closures; reduce `compat_type_contributors_for_typeinfo` to
  test-only), `resolver_core/external_type_frontier.rs`, `host_manage/eval_env.rs` (typeof peel),
  `decl_body_memo.rs` (`compat_type_body_hash_input`), `fact_emission.rs`
  (`compat_value_body_hash_input`, `LazyBodyFactSource::compute`, `value_body_for_hash`),
  the `typeinfo/oracle_core/**` subtree (`admission`/`normalize`/`source_walk`/`snapshot`/`hover_extract`/`identity`/`probe` — not just `source_walk::walk`) is ALREADY `#[cfg(any(test, feature = "oracle-gen"))]`-gated at `typeinfo/mod.rs:139` (never in the default build) → B5 only confirms/keeps the gating, no production conversion; and the imported-symbol dependency-closure walkers — `collect_type_expr_symbol_refs` defined in `host_manage.rs`, closure walkers in `host_manage/prepared_decl.rs` (§3.6 completeness model).
- **`crates/verter_semantic/src/facts/hashing.rs`** — B5 ADDS a no-`TypeExpr` fingerprint-encoder entry point
  reusing the legacy byte grammar. The byte grammar to preserve is `compute_semantic_hash(body: &TypeExpr)`
  (`hashing.rs:186`) ONLY. `compute_display_hash` is NOT in this file and NOT part of the TypeExpr byte grammar:
  it lives at `crates/verter_session/src/fact_emission.rs:656` and its `_body: &TypeExpr` param is already UNUSED
  (a trivial dead-param cleanup, not a parity encoder). B5 owns the `compute_semantic_hash` encoder refactor +
  the byte-parity fixture (fork c, §4). Disjoint from B3 (B3 stays off `hashing.rs`).

**B6 — Surface A + orphan carriers + FN5.2 (`verter_session`, several `verter_semantic` touches)**
- `project_semantic_dispatch/build.rs` (`class_heritage_bases`), `project_semantic_dispatch/raise.rs`
  (closedness/key-domain), `resolver_core/component_meta_registry.rs`,
  `resolver_core/component_meta_query_engine/{shallow_preserve,registry_decl,node_materialize,helpers,mod}.rs`
  (incl. carrier defs `ResolvedImportedRegistrySymbol.body`, `FastShallowFieldExpr.expr`, `owner_collection_exprs`),
  **`crates/verter_session/src/component_meta_caches.rs`** (`OwnerCollectionDb` — corrected path, NOT under
  `resolver_core/`), `host_manage/component_meta_methods.rs`, `component_meta_resolution_policy/core.rs`,
  `meta_resolve/{registry_materialize.rs,materialize/field_types.rs}`.
- FN5.2 (moved from B7, §3): `resolver_core/component_meta_query_engine/{surface,mod}.rs`,
  `project_semantic_dispatch/{raise,raise_sentinel,raise/shape_engine/*}.rs`, `host_manage/jsdoc_resolve.rs`, plus
  `Unknown{raw}` control-check consumers `meta_resolve/projectors/output_sink.rs`, `meta_resolve/materialize/field_types.rs`.
- Session consumers of B3-owned narrowed `Projected*` facts.
- **`type_expand/request.rs` (`verter_semantic`) — B3 owns the lower field narrowing** (§3.6 RESOLUTION / design §5.7):
  the `Expanded*` `TypeExpr` field deletion → the `Expanded*Fact` NoTypeExpr family (incl. `ExpandedNormalizedExpr`)
  lands in **B3**, alongside the rest of the lower fact family. **B6 CONSUMES** the narrowed facts through the session
  `HotExpanded*` handle surface + the ~40 consumer rewrites; it does NOT own the lower-crate DTO edit. (Supersedes the
  earlier "fork-b carried file → B6" framing — the two-legs-plus-decider ruling placed the lower `verter_semantic`
  narrowing in B3; fork b's `FastShallowFieldExpr → HotTypeRef` session carrier remains a B6 session concern.)

**B7 — framework/output boundary (FN5.2 REMOVED)**
- `crates/verter_semantic/src/analysis/framework_facts/svelte.rs` (Svelte persisted facts + `SvelteScriptProvider::VERSION`
  + `stable_candidate_hash`, §7 Surface 3), `typeinfo/framework_surface/{results.rs,graph_export.rs,vue_exec/*,svelte_exec.rs}`,
  **plus the `NamedTypeMember`/`MacroSurfaceDtos` aggregation/adapter consumers**
  `typeinfo/framework_surface/{executor.rs,mod.rs}`, `typeinfo/adapters/{vue,svelte}/adapter.rs`, and the framework
  stores (verify each compiles under the split DTO shape or is edited).

**§3.6 re-census scope-growth addendum (verified file-disjoint — see §7 RESOLUTION):**
- **B3 GAINS** (all `verter_semantic`, Wave 1): `analysis/type_expand/request.rs` (the `Expanded*` lower fact family),
  `analysis/type_eval_build.rs` (the producer + `CollectedMacroTypeParams`), `analysis/component_meta.rs` (the 15
  `*Analysis` lower fact carriers), `analysis/html_intrinsics.rs` (the lower `IntrinsicMemberFact` catalog). NONE is
  B5's only `verter_semantic` file (`facts/hashing.rs`) → Wave-1 `B3∥B5` STILL disjoint.
- **B5 GAINS** (Wave 1): `crates/verter_session/src/mapper_binder_registry.rs` (`hash_type_expr_structurally` → a
  no-`TypeExpr` fingerprint encoder, fork-c sibling); plus the imported-symbol dependency-closure walkers
  `host_manage/prepared_decl.rs` + `host_manage.rs` (`collect_type_expr_symbol_refs`, the dependency/fact-closure surface).
  `verter_session` files, disjoint from B3's `verter_semantic` set.
- **B6 GAINS** (Wave 3, runs alone): the session `HotExpanded*`/`HotComponentMetaAnalysis` surface + the ~40 `type_expand`
  consumers (`host_manage/component_meta_extract.rs`, `meta_resolve/macro_member_walk.rs`, `meta_resolve/projectors/*`,
  fallthrough); the framework-NEUTRAL synthesized-default semantic cut (`build.rs:940` + `insert_synthesised_value_default`
  + BOTH `svelte_default_synth.rs` AND `vue_default_synth.rs`); the `ShapeSubject::TypeExpr`/`NonSyntheticTypeExpr`
  cache-KEY conversion (`component_meta_caches.rs` → the sibling node-based `MemberValueNode` subject) + the
  `meta_resolve/materialize/{utility_types,macro_shapes}.rs` L1 Pick/Omit + ref-name walkers;
  `semantic_query_memo/synthetic_carrier_guard.rs`; `component_meta_registry.rs` resolved-body routing; and the DELETION
  of `resolver_core/type_expansion{,_verter}.rs`.
- **B6 GAINS — the independent-confirm below-`verter_semantic` `VueMacroElements`/`ResolvedElements` carrier** (Wave 3, runs alone;
  design §3.6 third-carrier row / rule-1 class (vi) / §5.4). Touch-set additions: **`crates/verter_parser/src/utils/oxc/script/type_surface/mod.rs`**
  (`ResolvedProp.type_expr`/`ResolvedNamedCallSignature.type_expr` → NoTypeExpr fact+`AuthoredBodyLocator` DTO; `ResolvedElements` + the
  transitive embeddings `ShallowResolvedElements`/`TypeResolutionContext.companion_types`, and `verter_compiler`'s `PreparedSetup.companion_types`/`external_types` construction sites),
  **`crates/verter_session/src/semantic_query.rs`** (the `VueMacroElements` node re-cut as a NoTypeExpr projection + doc), **`crates/verter_session/src/semantic_query_memo/mod.rs`**
  (`insert_/get_resolved_named_type` — intern NoTypeExpr, refcount-only warm read, no raw `TypeExpr`), and **`crates/verter_session/src/host_manage.rs`**
  (the `HostNamedTypeCacheAdapter` `NamedTypeCache` `get`/`insert` at `:385`–`:427`), plus the already-listed `semantic_query_memo/synthetic_carrier_guard.rs`
  (delete the `TypeExpr` walk → structural `NoTypeExpr` invariant) and `resolver_core/surface_projector.rs` (display via sealed output/dispatch handle, no `&TypeExpr` semantic read).
  **Additional `ResolvedElements` holders (3/3-review addition — `VueMacroElements` is NOT the only persisted holder):** the SECOND persisted `ProjectTypeStore` cache **`ResolvedTypeCacheDb`** (`ResolvedTypeCacheEntry.resolved: Option<ResolvedElements>` — `crates/verter_session/src/types.rs:2531` + `crates/verter_session/src/project_type_store.rs:831`, **DELETED** not converted — query-time reuse rides the dispatch memo), plus session resolver holders/readers `resolver_core/symbol_resolver.rs` (`TypeShape`/`elements`), `resolver_core/external_type_body.rs`, `resolver_core/external_macro_types.rs`, `resolver_core/component_meta/mod.rs` (`elements: ResolvedElements`, `:82`), `host_resolve/frontier_helpers.rs`, `host_resolve/external_macro_collector.rs` (`:36`), `host_manage/jsdoc_resolve.rs` (`:450`), the write/read site `host_resolve/external_type_resolution.rs`, and the CROSS-WAVE `host_manage/prepared_decl.rs` (`ResolvedElements` returns `:2957`/`:2980` — already a B4(W2)/B5(W1) file, so the B6(W3) touch is cross-wave serial, no same-wave collision) — all hold/return the SAME `ResolvedElements` and convert with the single `NoTypeExpr`+`NoStoredSpan` type change, each an OWNED B6 site. **Audit/display consumers** `component_meta_audit/footprint_miner.rs` (`:1133` hashes `VueMacroElements` via `Debug`) + `semantic_query/display.rs` (`:429` reads the payload for display) get EXPLICIT stable-identity/display rules over the new NoTypeExpr shell (not incidental `Debug` formatting). Under the DECIDED form (c), the parser expander's SFC-extraction (`crates/verter_parser/.../vue/script/{bindings,setup}.rs`) + compiler-codegen consumers (`crates/verter_compiler/src/framework_common/vue_bridge.rs`, `host_resolve/virtual_file_pipeline.rs`) RE-POINT onto the dispatch-produced macro DTO and the parser structural expander is deleted/reduced to syntax-only extraction = the §9.3 obligation CLOSED IN B6 (owned + consolidated here, not a carried deferral, not dead code). The `host_resolve/{external_type_resolution,route_surface,frontier_engine}.rs` frontier surface is B5 Surface-B territory — routed by ownership-by-rule, cross-wave serial (B5 W1 / B6 W3), not a same-wave collision.
  **Enforcement (design §9.3):** `ResolvedElements` SPLITS into a NEW query-owned NoTypeExpr+`NoStoredSpan` semantic shell (`SemanticMacroSurfaceShell`, admitted only through its P2 crate-graph-valid construction authority, the only valid `VueMacroElements` payload) + a codegen-only `ResolvedElements` survivor (`type_expr`/`type_expr_scope` removed, spans retained); a `SemanticDispatchAuthorityCap` / `CodegenStructuralSurfaceCap` capability pair (the P2 admission model — its precise construction-authority mechanism settles at P2, see the line-22 P1/P2 note) + compile-fail/trait guards + a behavioral disagreement fixture are NECESSARY barriers — but the code-verifying decider ruled them NOT SUFFICIENT (they close only the STORAGE + known-path axes; a query module can still NAME/CALL the `pub` expander and BRANCH on the result in-flight, and the parser `NamedTypeCache` WRITE adapter `HostNamedTypeCacheAdapter::insert` [`host_manage.rs:385`-`:427`] still WRITES expander-authored `ResolvedElements` into the graph). **A structural CALLABILITY FIREWALL is a HARD B6 landing precondition** (query-time semantic code must be structurally UNABLE to name/call/feed the `pub` expander — READ and WRITE entrypoints both). The three established forms were (a) crate extraction (`verter_session_query` with no parser/compiler/OXC dep, a Cargo dep-graph guard, + removing `verter_compiler`'s `pub use verter_parser::utils`; PRESERVES the deferral); (b) de-publicize/cap-gate EVERY expander read AND write entrypoint + re-own the write path by dispatch (PRESERVES the deferral); (c) full DTO-mediated consolidation (CLOSES the deferral by re-scoping B6). **DECIDED (CTO firewall-form decision — two independent unprimed code-verifying codex legs converged, no decider): form (c).** Its durable enforcement is a Cargo dependency-graph PACKAGE firewall (a real crate boundary, NOT a module slice): query-time semantic execution moves into a NEW `verter_session_query` Cargo crate/package whose closure excludes `verter_parser`/`verter_compiler`/`verter_type_expr_oxc`/`oxc_*` (checked under all features), plus a NEW dependency-neutral macro-DTO crate (introduced by B6) that both `verter_session_query` and `verter_compiler` depend on, `verter_compiler` keeping its no-`verter_session` direction + a P2 crate-graph-valid shell-construction authority + compile-fail fixtures — NOT a name-scanner (design §9.3). **The §9.3 deferral is CLOSED; B6's SINGLE-ENGINE-axis landing is UNBLOCKED** under the re-scoped in-block consolidation — MAJOR B6 scope growth (a NEW `verter_session_query` Cargo crate/package + a NEW dependency-neutral macro-DTO crate + a `verter_compiler` public-API change + the parser-expander reduction), all Wave-3-local, absorbed here. (The SEMANTIC-`TypeExpr` axis was never form-gated; both axes now close at Stage-10 landing.) Form (c)'s codegen/SFC consolidation also owns the direct `ResolvedElements` users on the codegen rail — `verter_compiler/src/tsc/script.rs:97`, `verter_parser/.../vue/script/macros.rs:23`, `bindings.rs`/`setup.rs`, `PreparedSetup.companion_types`/`external_types` — under a mechanical "all compiler/SFC `ResolvedElements` users migrate through the dispatch-produced macro DTO rail" rule (they read zero `.type_expr`, so dropping it from the codegen DTO is clean). **Write-path + guard touch-set (single-engine WRITE severance — the firewall must close the WRITE bypass, not only READ/CALL):** the full write chain is the injection site `host_manage/eval_program.rs:466`/`:486` (`ctx.set_named_type_cache(Some(HostNamedTypeCacheAdapter { .. }))`), the parser cache trait `verter_parser/.../vue/script/named_type_keys.rs:56` (`NamedTypeCache::get`/`insert` traffic `Arc<ResolvedElements>` — a semantic payload across the parser boundary), the parser-ctx read/write `type_surface/mod.rs:973`-`:1023`, and the adapter `HostNamedTypeCacheAdapter::insert` (`host_manage.rs:385` impl / `insert` `:409`-`:427`) → `insert_resolved_named_type`. B6 removes or re-owns the `NamedTypeCache<ResolvedElements>` API as dispatch-owned syntax/locator ingestion (dispatch PRODUCES the shell). **Existing guards that currently ASSERT the old parser-write bypass MUST FLIP/DELETE in B6/B8** — `project_semantic_dispatch_invariants_tests.rs:1831`-`:1848` + `:2907`-`:2941` (assert `HostNamedTypeCacheAdapter` writes the graph via `insert_resolved_named_type` directly) and `architecture_guards.rs:17917`-`:17922` (asserts the ONLY production call site of `insert_resolved_named_type` is the adapter forwarding the parser `Arc<ResolvedElements>` UNCHANGED); leaving them green would let the parser-write path survive under a renamed adapter. Under the DECIDED form (c) these guards FLIP/DELETE in B6/B8 (the parser-write bypass is removed, dispatch owns the write). **Full old-shape guard/test disposition (B6/B8 flip or delete EVERY assertion of the retired shape — the named sites are KNOWN HIGH-SIGNAL sites, NOT exhaustive; any old-shape assertion surfaced at impl flips by the SAME rule, else the retired `TypeExpr`-carrying model survives under renamed plumbing):** (i) the `VueMacroElements(Arc<ResolvedElements>)` payload / single-construction / `Debug` / direct `get`/`insert` / member-`type_expr` old-shape tests — `crates/verter_session/src/semantic_query_memo/tests.rs` (the `VueMacroElements`/`get_`/`insert_resolved_named_type` payload-shape cases ~`:128`/`:3491`/`:3568`/`:4738`/`:4883`/`:8641`), `crates/verter_session/tests/cases/g_block/u2_display_projection_guards.rs:1047`, the WHOLE `crates/verter_session/tests/cases/architecture_guards.rs:17917`-`:18224` block (it pins the old single-construction/`Debug`/`Arc<ResolvedElements>` model, not only `:17917`-`:17922`), the parser typed-form test class `crates/verter_parser/src/utils/oxc/script/type_surface_typed_form_tests.rs` (pins the `type_expr`/`type_expr_scope` member fields), and `crates/verter_session/src/project_global_cache_tests.rs` (pins `insert_resolved_named_type` synthetic-carrier rejection over member `type_expr`); (ii) the `ResolvedTypeCacheDb` DB-existence / reuse / boundedness / empty-cache / eviction / generation / cache-len fixtures for the DELETED cache — `crates/verter_session/src/project_type_store_tests.rs:47`/`:122`/`:941`/`:1054`, `crates/verter_session/src/meta_tests.rs:13243`/`:13284`, `crates/verter_session/src/host_manage_tests.rs:10355`, `crates/verter_session/tests/cases/g_cache/cache_reuse_invariants.rs:190`, `crates/verter_session/tests/cases/g_misc0/byte_identical_upsert_no_op.rs:77` — DELETED or rewritten to assert the dispatch-`Instantiate`/macro-payload-memo behavior that replaces the cache.
  **DAG disjointness re-verified: B6 runs ALONE in Wave 3, so intra-wave file-disjointness is AUTOMATIC for all additions** — none is claimed by a same-wave concurrent block. This enumeration is the KNOWN scope, not an exhaustiveness claim — any further `ResolvedElements` holder surfaced at implementation is routed by §3.6 rule-1 class (vi) → B6 (session) or the frontier → B5, never an unowned gap or a same-wave collision (B6 is Wave 3 alone). (The DECIDED form (c) additionally touches `verter_compiler` — a NEW crate to the plan (macro-DTO consumption + re-export removal) — plus a NEW `verter_session_query` Cargo crate/package + a NEW dependency-neutral macro-DTO crate and the `verter_parser` expander reduction; this blast radius is the routed form-(c) enforcement, all Wave-3-local.) Cross-wave overlaps (serial-safe under the one atomic squash): `host_manage.rs` is already a **B5 (Wave 1)** file (`collect_type_expr_symbol_refs`) — the B6 (Wave 3) touch is cross-wave serial, exactly like `host_manage/eval_env.rs` (§2.2), never two same-wave owners; `crates/verter_parser/.../type_surface/mod.rs` is a NEW crate to the plan — NO block touches `verter_parser` (B3 is `verter_semantic`-only; B4/B5/B7 are `verter_session`/output) so there is no collision; `semantic_query.rs` and `semantic_query_memo/mod.rs` are claimed by no Wave-1/Wave-2 concurrent block. Wave-1 `B3∥B5` and Wave-2 `B4∥B7` file-disjointness is UNCHANGED (B6 is Wave 3).
- **B7 GAINS** (Wave 2): the sealed `MaterializedExpanded*` output DTOs + the FULL `component_meta_result_db.rs` value
  split (semantic snapshot + output snapshot — single-owner B7, NOT split with B6), covering BOTH persisted value carriers —
  `ResolutionTemplate` (`:146`) AND `CachedComponentMetaResult.analysis: ComponentMetaAnalysis` (`:173`/`:174`), not only `ResolutionTemplate`;
  `typeinfo/adapters/vue/runtime_ctor.rs` (or delete). `component_meta_result_db.rs` does not intersect B4's `verter_session` files → Wave-2 `B4∥B7` STILL disjoint.

### 2.2 Cross-block shared-file map (known scope)

| Shared file | Blocks (function) | Resolution |
|---|---|---|
| `project_semantic_dispatch/build.rs` | B4 `build_typeof` (W2) · B6 `class_heritage_bases` + §3.6 svelte-synth `build_vue_default_instance` `:940` (W3) [· B2 done] | serial across waves; RESOLVED — the svelte-synth `:940` cut lands in B6 (Wave 3), NOT B7 (Wave 2), so `build.rs` is B4(W2) then B6(W3), never two same-wave owners. |
| `host_manage/eval_env.rs` | B5 typeof-peel (W1) · B4 `component_meta_binding_type_entries` (W2) · **B6 fork-b `fast_to_expansion`/`FastShallowFieldExpr` (W3)** | 3-block, serial across waves — no conflict, but all three touch it. |
| `project_semantic_dispatch/raise.rs` | B6 closedness · FN5.2 (now B6) | single-owner (B6). |
| `resolver_core/component_meta_query_engine/mod.rs` | B6 carriers · FN5.2 (now B6) | single-owner (B6). |
| `verter_semantic/.../type_solver/query_engine.rs` | B3 `Projected*` def · B6 consumers (NOT this file) | def→B3 only. |
| `resolver_core/prepared_decl.rs` | B2 done · B4 bundle | B4 owns forward. |
| `host_manage.rs` | B5 `collect_type_expr_symbol_refs` (W1) · **B6 `HostNamedTypeCacheAdapter` `NamedTypeCache` get/insert `:385`-`:427` (W3)** | serial across waves — like `host_manage/eval_env.rs`, both touch it but never same-wave. |
| `semantic_query.rs` | **B6 `VueMacroElements` node re-cut (W3)** | single-owner (B6); no Wave-1/2 concurrent claim. |
| `semantic_query_memo/mod.rs` | **B6 `insert_/get_resolved_named_type` (W3)** | single-owner (B6); sibling of the already-B6 `synthetic_carrier_guard.rs`; no Wave-1/2 concurrent claim. |
| `verter_parser/.../type_surface/mod.rs` | **B6 `ResolvedProp`/`ResolvedNamedCallSignature.type_expr` + the `ResolvedElements` type-split (W3)** | single-owner (B6); NEW crate — no block touches `verter_parser`, so no collision. |
| `host_resolve/external_type_resolution.rs` | B5 frontier (W1) · **B6 `ResolvedTypeCacheDb` deletion + read-site rewrite (W3)** | frontier surface = B5 Surface-B territory; cross-wave serial (B5 W1 / B6 W3), no same-wave collision. |
| `host_resolve/frontier_engine.rs` | B5 frontier (W1) · **B6 `ResolvedElements` return/memo severance (W3)** | cross-wave serial (B5 W1 / B6 W3); frontier ownership routed by §3.6 rule → B5 for the frontier walk, B6 for the `ResolvedElements` payload cut. |
| **form-(c) firewall additions** — `verter_compiler` (`lib.rs:17` re-export removal, `compile/macro_dto.rs` consumption, `compile/types.rs`, `framework_common/vue_bridge.rs`, `tsc/script.rs`) + a NEW `verter_session_query` Cargo crate/package + a NEW dependency-neutral macro-DTO crate | **B6 (W3)** — drop the `pub use verter_parser::utils` re-export, introduce the neutral macro-DTO crate + the dispatch-result→neutral-DTO bridge, house the query-time semantic path in `verter_session_query` with no parser/compiler/OXC dep | single-owner (B6); `verter_compiler` + the new `verter_session_query`/macro-DTO packages are NEW crates NO Wave-1/2 block touches — Wave-3-local, no same-wave collision (enforced by the Cargo dep-graph package firewall). |

After the FN5.2 redraw, no two CONCURRENT (same-wave) blocks share a file in the KNOWN scope. The §3.6 re-census
additions PRESERVE this (verified — §7 RESOLUTION + the addendum in §2.1): the svelte-synth cut landed in B6 (Wave 3,
not B7/Wave 2), and B3's new `verter_semantic` files do not touch B5's `facts/hashing.rs`. So Wave-1 `B3∥B5` and
Wave-2 `B4∥B7` remain same-wave file-disjoint.

---

## 3. FN5.2 redraw — B7 → B6 (codex-ratified design amendment)

Both DAG legs diverged on the second parallel wave; the code-verifying decider ruled to MOVE FN5.2 into B6,
verified first-hand: (1) the 6 `UNKNOWN_SENTINEL_OWNER_FILES` (per
`crates/verter_session/tests/cases/output_projector_residual_guards.rs`) are the sentinel/control substrate
co-located with B6's `raise.rs` closedness readers + `component_meta_query_engine/mod.rs` carriers — semantic
control-flow, not framework output; typed authority already in `project_semantic_dispatch/raise_sentinel.rs`;
(2) B7's framework output resolves through the PRE-EXISTING macro hot mirror route + a thin `raise_member_value`
projection and does NOT read B6's orphan carriers; `graph_export.rs` is output-only zero-dispatch. The review
independently verified the 6 owner files match and that none is a framework file.

**Consequence:** FN5.2 → B6 removes the only B6∩B7 overlap; the ORIGINAL B7 becomes file-disjoint from B4.
The MECHANISM (design §5.6) is unchanged. **REQUIRED before dispatch (not B8):** amend the binding design §10
B6/B7 rows + §5.6 owner reference to "FN5.2 → B6", so the two authorities do not disagree during execution.

---

## 4. Decided per-block carrier/mechanism forks

All four converged across two independent unprimed codex legs and survived the plan review. Each is
`NoTypeExpr` by construction, respecting R6 / crate-boundary / anti-tear.

**Fork a — `OwnerCollectionDb` value + query-local memo (B6). DECIDED: `Option<AuthoredBodyLocator>`.**
The persistent store value (`crates/verter_session/src/component_meta_caches.rs`, `OwnerCollectionDb`, currently
`Option<Arc<TypeExpr>>`) becomes `Option<AuthoredBodyLocator>` (content-free, keyable, lowers lazily through the
one engine); the query-local `owner_collection_exprs` memo likewise. Producer `owner_collection_expr` computes the
locator from the observed prepared/indexed declaration (NOT `prepared.body.clone()`); consumers lower the locator +
read node-domain predicates, never inspect a stored body; consumers that need `LocatorLoweringKey` obtain the
matching resolved declaration slot from the same observed owner artifact that mints the locator. Anti-tear: publish
locator + fact signature from that one artifact. Rejected: `HotTypeRef` (generation-local; not a persistent store
value), `SessionDemandIdentity` (authored body, not an adapter route), closed fact (open type body). Codex noted a
FURTHER simplification — deleting the body-bearing DB and routing purely through the slot/locator substrate — as a
PREFERRED end-state; that is a routine B6 dedup/perf decision (both routes carry the identical locator
representation), NOT an open architecture fork: the DECIDED value type is `Option<AuthoredBodyLocator>` whether or
not the DB layer survives.

**Fork b — `FastShallowFieldExpr.expr` (B6). DECIDED: session `HotTypeRef` handle + exactness discriminant**
(a `NoTypeExpr` session carrier). It is a live PRODUCTION hot carrier (read at `host_manage/eval_env.rs:899`
`fast_to_expansion`). Rejected: locator-alone (producer emits alias-rewritten SYNTHESIZED bodies), closed fact,
removal. Authored-exact paths lower via locator/hot mirror; synthesized alias-rewrites intern nodes through one
sanctioned session builder. **This directly feeds the type_expand family (§7 Surface 1)** — the downstream
`ExpandedNormalizedExpr.expr`/`ExpandedField.r#type` carrier must become handle/node-native too (that is the §7
scope-expansion, owned with fork b in B6; `TypeExpr` materialization stays at the sealed output sink).

**Fork c — hash/fingerprint-input trio (B5), parity REQUIRED. DECIDED: a dedicated no-`TypeExpr`
`body_fingerprint` producer** at the lazy-body/prep boundary that REUSES the `compute_semantic_hash` byte grammar
(`crates/verter_semantic/src/facts/hashing.rs:186`, currently `&TypeExpr`) to emit the SAME legacy semantic-hash
bytes WITHOUT constructing a `TypeExpr` (type-decl encodes the legacy `lookup_object()` view; value-decl → a closed
`ValueBodyFingerprintInput` encoder). The `display_hash` side is NOT a TypeExpr byte grammar — `compute_display_hash`
(`crates/verter_session/src/fact_emission.rs:656`) already ignores its `_body` param, so it needs only a trivial
dead-param cleanup, not a parity encoder. Rejected: hashing the new facts directly (changes the input domain —
breaks byte-parity), hashing locator identity. If exact byte-parity proves non-isomorphic AFTER the encoder exists,
take a DELIBERATE documented cache-generation bump — never reconstruct a `TypeExpr` to preserve warm hits, never
silently redefine while claiming parity. The fingerprint-parity fixture (design §10) is the impl-time proof; the
architecture is decided. Requires a `verter_semantic/src/facts/hashing.rs` edit (a no-`TypeExpr` semantic-hash
encoder API) owned by B5.

**Fork d — FN5.2 typed-degradation carrier (B6, per §3). DECIDED: typed outcomes carrying the EXISTING
`QueryError`** (structurally `Value(T) | Degraded(QueryError)`, reusing the `raise_sentinel.rs` authority) — NOT a
new taxonomy. Downstream branches that pattern-match `TypeExpr::Unknown{raw}`
(`component_meta_query_engine/surface.rs`, `meta_resolve/projectors/output_sink.rs`,
`meta_resolve/materialize/field_types.rs`) branch on the typed `QueryError`/`RaisedShapeFacts` predicates. The
sentinel-spelling authority moves into `raise_sentinel.rs`. `raise/shape_engine/materialize.rs` stays the sealed
output seam emitting `Unknown{raw}` for display/JSDoc/raw-fallback only.

**Boundary forks (DAG consult, all three legs agree):**
- **Projected\*** DEFINITION narrowing (`verter_semantic/.../query_engine.rs`) → **B3**; B6 owns only session
  consumers (no lower-crate DTO edit).
- **GraphBackedPending 6 readers:** 3 annotation-handle → **B4** (`build_typeof`,
  `prepared_value_decl_to_value_decl_info`, `component_meta_binding_type_entries`); 3 registry/member → **B6**
  (`resolve_imported_registry_symbol_with_budget`, `append_component_meta_registry_entries`,
  `locate_declaration`/`named_decl_body`).
- **Type-params** → one shared TP0 producer (§2.1).
- **`NamedTypeMember`** OUTPUT DTO/projection → **B7** (output-boundary only; the only reads are the
  `graph_export` encoder + producer/raise arms — no semantic-decision reader). B7 must supply a producer/consumer
  table for `NamedTypeMember`/`MacroSurfaceDtos` separating semantic decisions from output/cache storage, and cover
  the aggregation/adapter consumers (§2.1).

---

## 5. Known-census assignment (complete for the 39-reader inventory; NOW terminal — the §7 re-census closed the gap)

Within the design's KNOWN census, every residual is owned by exactly one block, none double- or unassigned:
17 AuthoredShape → **B6**; 12 GraphFreeDto → **B5**; 6 GraphBackedPending → **B4**(3)+**B6**(3); the lower-crate
`Prepared*`/`Analyzed*`/`Projected*` field removals → **B3**; `HotPrepared*`+`PreparedDeclBundle` → **B4**; orphan
carriers → **B6** (+ `NamedTypeMember` output/Svelte facts → **B7**); hash trio → **B5** CONVERT; the 2 inventory oracle readers
(and the whole `typeinfo/oracle_core/**` subtree the §3.6/§7 re-census expands them to) are already `#[cfg(test/oracle-gen)]`-gated (`typeinfo/mod.rs:139`, outside the default build) → **B5** confirms/keeps the gating, no production conversion; FN5.2 → **B6**.

**No deferrals INSIDE the old 39-reader census** — every known-census fork is decided (fork c's byte-parity is an
impl-time fixture proof against a decided architecture; fork b's downstream type_expand dependency is owned, not
deferred). The §7 re-census gate is now CLOSED: the surfaces OUTSIDE the 39-reader inventory (the `type_expand`
three-surface family + the 15 `*Analysis` carriers, `svelte_default_synth`, the newly-found `html_intrinsics` /
`mapper_binder` / `synthetic_carrier_guard` / `CollectedMacroTypeParams` / vue `runtime_ctor` /
`component_meta_registry` resolved-body, and the dead `resolver_core::type_expansion`) are ALL censused and assigned
(design §3.6 / §5.7). **The plan now ESTABLISHES zero semantic-`TypeExpr` residual by B8 under the §3.6 ownership-by-rule +
exhaustive-conversion proof — terminal-completeness is ACHIEVABLE with no semantic-`TypeExpr` blocker (design §3.6 verdict), no semantic-`TypeExpr`
Stage-11/12 deferral (memo/perf compaction, e.g. the `whole_env` materialization teardown, may remain Stage-12).**

---

## 6. How the CTO dispatches from this plan

**Dispatchable NOW (independent of the §7 gap):**
1. **TP0** — a locator-substrate prelude (facts.rs + locators.rs + session `locator_deref.rs` + witnesses).
2. **B5** — Surface B + the hash-input trio (fork c). Cleanly independent of the §7 surfaces; may run concurrently
   with TP0-consuming B3 ONLY once B3 is unblocked (below). Assembling a WIP slice onto staging means cherry-pick
   WITHOUT a green-compile gate (atomic model; per-slice proof is the WIP parity oracle, not a green tree).

**Now DISPATCHABLE — the §7 re-census gate is CLEARED and the §9.3 SINGLE-ENGINE-axis firewall FORM is DECIDED (form (c) — full DTO-mediated consolidation, deferral CLOSED, B6 RE-SCOPED; AMENDMENT note + §9.3) (subject to their DAG edges + scope growth below; B6's single-engine-axis landing and the B1–B8 atomic squash are UNBLOCKED under the re-scoped form-(c) consolidation):**
3. **B3** — its `Analyzed*`/`Prepared*` narrowing forces the `type_expand` producer (`type_eval_build.rs`) to change;
   the Analyzed* fact schema must be designed to feed `ExpandedField`'s no-`TypeExpr` replacement. Dispatch B3 in
   Wave 1 alongside B5 ONLY after the `type_expand` sub-design fixes that schema (B3∥B5 stays FF-safe per §2).
4. **B4** — gated TRANSITIVELY: it consumes B3's schema (`B3 → B4`), so it cannot precede B3; and its own
   `build_typeof` carries synthesized-default logic (`build.rs:556`, calling `build_synthesized_vue_default_construct_object`
   `build.rs:575`) beside the neutral synthesized-default reader `build_vue_default_instance` (`build.rs:940`) that
   reads the `value_decl("default")` slot the svelte-synth surface populates. When B4 IS dispatched (after B3 + the
   re-census), it must NOT touch ANY synthesized-default logic in `build.rs` — B6 owns that migration.
5. **B6** — Surface A + orphan carriers + FN5.2 + the `type_expand` SESSION cutover (`HotExpanded*`/`HotComponentMetaAnalysis`
   + the ~40 consumers) + the svelte-synth semantic cut + `synthetic_carrier_guard` + `component_meta_registry` resolved-body
   + the `resolver_core::type_expansion` deletion; AND the below-`verter_semantic` `VueMacroElements`/`ResolvedElements` carrier (§2.1 B6 GAINS). Dispatchable (Wave 3, runs alone); its SINGLE-ENGINE-axis landing is UNBLOCKED under the DECIDED §9.3 firewall form (c) — B6 is RE-SCOPED to absorb the full DTO-mediated codegen/session-boundary consolidation (parser expander deleted/reduced to syntax-only; codegen consumes the dispatch-produced macro DTO; Cargo dep-graph query-package firewall) (AMENDMENT note + §9.3). NOTE: B6 owns the type_expand SESSION
   surface + consumers; the lower `verter_semantic` `Expanded*Fact` family is B3 (§2.1 addendum).
6. **B7** — framework/output boundary + persistence; scope grew (§3.6): the sealed `MaterializedExpanded*` output DTOs + the
   FULL `ComponentMetaResultDb` semantic/output snapshot split + the typeinfo `TypeArgList` wire-input narrowing + vue
   `runtime_ctor` (if retained). `B4 ∩ B7 = ∅` RE-VERIFIED disjoint (the svelte-synth semantic cut moved to B6; B7 is
   output-boundary-only). Dispatchable.

**Pre-dispatch prerequisites — ALL DONE:** (a) ✓ FN5.2→B6 formalized in the binding design (§5.6 + §3); (b) ✓ the §7
systematic re-census complete (design §3.6); (c) ✓ the `type_expand` handle-native sub-design decided (design §5.7,
three-surface split); (d) ✓ the svelte-synth reassignment ratified (semantic cut → B6). B3/B4/B6/B7 are
dispatchable on the SEMANTIC-`TypeExpr` axis (B6's single-engine-axis landing + the atomic squash are UNBLOCKED under the
DECIDED §9.3 firewall form (c) — B6 RE-SCOPED, AMENDMENT note); each B3/B6/B7 brief absorbs its §3.6 scope growth (B3: the lower `Expanded*Fact`/`ComponentMeta*Fact`
family + 15 `*Analysis` + `html_intrinsics` lower catalog + `CollectedMacroTypeParams`; B6: session `HotExpanded*`/
`HotComponentMetaAnalysis` + the ~40 consumers + `svelte_default_synth` cut + `synthetic_carrier_guard` +
`component_meta_registry` resolved-body + the `type_expansion` deletion; B7: sealed `MaterializedExpanded*` +
`ComponentMetaResultDb` split + vue `runtime_ctor`; B5: the `mapper_binder` fingerprint).

**B8** — squash B1–B8; land §9 structural guards; delete the residual inventory + deferral docs; correct §3.5 stale
prose. Every block is S-tier → full 3/3 + discrimination verification + independent confirm; the WIP parity oracle
runs across every slice.

---

## 7. RESOLVED — design residual census completed + `type_expand` sub-design decided (was: materially incomplete, gated B3/B4/B6/B7)

**RESOLUTION — this gap is CLOSED.** The re-census below is COMPLETE (design §3.6: ≈96 semantic surfaces, ≈24
previously unscoped). Decisions landed into the binding design:

- **`type_expand`/`ExpandedField` sub-design → DECIDED** (design §5.7): a THREE-surface split — **B3** owns the lower
  `verter_semantic` NoTypeExpr fact family (`Expanded*Fact`/`ComponentMeta*Fact` + the 15 `component_meta.rs *Analysis`
  carriers + the `type_eval_build.rs` producer + field deletion); **B6** owns the session `HotExpanded*` /
  `HotComponentMetaAnalysis` surface + the ~40 consumer rewrites; **B7** owns the sealed `MaterializedExpanded*` output
  DTOs + the full `ComponentMetaResultDb` semantic-snapshot / output-snapshot split. Two unprimed codex legs converged
  on the design (E = three-surface split, rejecting hoist-to-session-`HotTypeRef` and scatter-per-consumer); a
  code-verifying decider resolved the ONE divergence (lower fact family + `*Analysis`: B3, not B6 — the session
  dependency is a sequencing artifact the atomic model dissolves).
- **synthesized-default reassignment → RATIFIED** (§7.2 recommendation adopted): the framework-NEUTRAL semantic-carrier cut
  (`project_semantic_dispatch/build.rs:940` `build_vue_default_instance` + `ShallowFileState::insert_synthesised_value_default`
  + BOTH `svelte_default_synth.rs` AND the live Vue sibling `vue_default_synth.rs` — the shared `ComponentDefaultSynth` seam)
  → **B6** (Wave 3, already owns `build.rs`); B7 retains the persisted `SvelteScriptFacts` +
  `SvelteScriptProvider::VERSION`/`stable_candidate_hash` + framework output. This RESTORES Wave-2 `B4∥B7` disjointness
  (no `build.rs` collision — the collision the §7.2 draft flagged is dissolved).
- **Newly-found surfaces → ASSIGNED** (design §3.6 table): `html_intrinsics` → **B3** (lower `IntrinsicMemberFact`
  static catalog, NOT an `AuthoredBodyLocator`) + **B6** (session consumers); `mapper_binder_registry` fingerprint → **B5**;
  `synthetic_carrier_guard` → **B6**; `CollectedMacroTypeParams` → **B3** (currently callerless — delete/test-confine likely);
  vue `runtime_ctor` → **B7** (or delete — no prod caller); `component_meta_registry` resolved-body → **B6**; the typeinfo
  `resolve_named_symbol`/`TypeArgList` wire-input (`TypeArgList = &[Arc<TypeExpr>]`, lowers to `SemanticNodeId` at the host
  boundary — a wire/producer boundary, not a semantic authority) → **B7** (narrow the session API off bare `Arc<TypeExpr>`);
  `resolver_core::type_expansion{,_verter}` → **DELETE in B6** (verified dead: `.expand_type(` has zero production callers).
  `ResolvedJsdocTag.resolved_type` (JSDoc-text-derived, output-projected to the verter_protocol wire graph + verter_ffi
  only; VERIFIED no semantic reader) — is OUTPUT-class (no semantic conversion), BUT it is transitively PERSISTED on the
  `ComponentMetaResultDb` value (`ResolutionTemplate.resolved_macros[].jsdoc.tags[].resolved_type`), so **B7** converts
  the persisted form to the sealed output-snapshot node-id/display (never a bare persisted `TypeExpr`), as part of the
  full `ComponentMetaResultDb` value split (design §5.7 / §4.1 item 10, pass-6 correction).
- **Review-pass additions → FOLDED IN** (design §3.6 "Two further carriers" + "Completeness model"): the
  `ShapeSubject::TypeExpr`/`NonSyntheticTypeExpr` cache-KEY (`component_meta_caches.rs:1076`) is RECLASSIFIED semantic
  cache-identity (not an output value) → **B6** (narrow to the existing node-based `MemberValueNode` subject; the
  `MaterializedOutputTypeExpr` VALUE stays OUT); `vue_default_synth` joins the framework-neutral B6 synth cut; the named
  `&TypeExpr` param-walker tail (`meta_resolve/materialize/{utility_types,macro_shapes}.rs` → **B6**;
  `host_manage/prepared_decl.rs`+`host_manage.rs` `collect_type_expr_symbol_refs` → **B5**; the `cycle_guard.rs`
  `hash_type_expr`/`NormalizedTypeArgs` cache-identity walker → **B6**; the DEAD `owned_artifacts::OwnedTypeResolutionContext`
  / `OwnedTypeExpr` arena → **B6 DELETE**, no production writer) travels with its owning subtree's block and is OWNED by
  rule + CONVERTED (or deleted) per the design §3.6 Completeness model — the carrier-mechanical core + defense-in-depth
  perimeter + §4.1 universal-scope, NOT a single §9 guard closing the walk surface.
- **Pass-7 surface-class additions → FOLDED IN** (design §3.6 total-class ownership rule + named instances; the ownership
  rule now ranges over FIVE surface classes — carrier field / free walker-reader-producer fn / retained memo-env / persisted
  cache-value position / dead — not only struct fields): the RETAINED legacy eval-env carrier `DeclBodyMemo.whole_env:
  OnceLock<Arc<EvalEnv>>` (`decl_body_memo.rs:202`) + its `EvalEnv` cluster (`type_eval.rs` `TypeDeclInfo.body`/
  `MergedTypeBody.contributors`/`ValueDeclInfo.type_annotation`/`FunctionSignature.return_type`) — VALUE-authoritative on every
  `get_component_meta` via `local_type_declaration_id`/`base_eval_env_arc` — → **B3** (lower-cluster field narrowing → a
  `TypeExpr`-free `EvalEnv`, terminal-zero met regardless of memo survival) + **B6** consumer conversion: (a) re-point the
  id/value consumers off `whole_env()` onto the EXISTING shallow-state-backed graph-native siblings
  (`local_type_declaration_id_graph_native` etc., debug_assert-cross-checked, still `TypeExpr`-bearing until B3 narrows),
  AND (b) convert the REMAINING fallthrough env-substitution surface (`fallthrough.rs:468` `base_eval_env_arc().clone()` →
  the `Option<&EvalEnv>` node-domain evaluators `fallthrough_value_eval.rs:114+`) — MORE than a re-point. `whole_env`
  SURVIVES B8 as a narrowed `TypeExpr`-free structure; its memo teardown is Stage-12 perf debt (retired-as-authority ≠ dead).
  This REVERSES the stale design-§4.2 "whole-env not touched" exclusion (corrected). The lower-crate free-walker readers `verter_semantic/analysis/type_expr_refs.rs`
  (`field_references_type_params` etc., called from `component_meta_query_engine/shallow_preserve.rs:91`) → **B3** schema +
  **B6** callsite; the B6 policy walkers `component_meta_resolution_policy/{raw_restoration,slot_preservation}.rs`
  (`raw_type_expr: Option<&TypeExpr>`) + `registry_materialize.rs` symbolic-preservation → **B6**; the DEAD walkers
  `loop5_instrumentation.rs::{count_operator_nodes,record_outer_call_type_expr}` → **B6/B8 DELETE** (callerless in production).
  Terminal bar stays ACHIEVABLE — all rule-1-owned and convertible/deletable; no un-convertible surface.
- **Terminal bar → ACHIEVABLE by B8, NO semantic-`TypeExpr` blocker.** Every semantic `TypeExpr` carrier has a viable NoTypeExpr target
  (fact / `AuthoredBodyLocator` / `HotTypeRef` / `SessionDemandIdentity` / sealed output). ZERO semantic-`TypeExpr` deferral to Stage 11/12. (The §9.3 single-engine-axis firewall FORM added by the AMENDMENT note is now DECIDED — form (c), full DTO-mediated consolidation — which CLOSES the deferral in B6; it never gated the terminal-`TypeExpr` bar, and B6's single-engine-axis landing + the atomic squash are now UNBLOCKED.)
- **DAG → same-wave DISJOINTNESS holds under the DECIDED §9.3 firewall form (c) (B6 is Wave-3-alone, so any crate/file it touches is disjoint from concurrent blocks by construction). Form (c) grows B6's Wave-3-local touch-set — a NEW `verter_session_query` Cargo crate/package (query-time semantic execution, no parser/compiler/OXC dep) + a NEW dependency-neutral macro-DTO crate + a `verter_compiler` public-API change (macro-DTO consumption + re-export removal) + the `verter_parser` expander reduction; none is claimed by a Wave-1/Wave-2 block (AMENDMENT note + §9.3).** The re-census grew B3 (adds `type_expand/request.rs`,
  `type_eval_build.rs`, `component_meta.rs`, `html_intrinsics.rs` — all `verter_semantic`, none touching B5's
  `facts/hashing.rs`), B6 (session cutover, runs alone Wave 3), and B7 (cache split); Wave-1 `B3∥B5` and Wave-2 `B4∥B7`
  remain file-disjoint (§2.2 updated).

The original gap analysis follows, retained as the record. **(SUPERSEDED — every "gated" / "do NOT dispatch" / "only B5+TP0 dispatchable" statement below is the PRE-resolution framing; the RESOLUTION above is authoritative: all blocks are dispatchable on the SEMANTIC-`TypeExpr` axis, the census is complete, terminal bar ACHIEVABLE — and the ONE post-resolution fork the AMENDMENT note added (the §9.3 firewall FORM) is now DECIDED (form (c)): B6's single-engine-axis landing + the atomic squash are UNBLOCKED under the re-scoped in-block consolidation.)**

---

A code-verifying codex consult (framed on review findings, verified first-hand) established that the binding
design's residual census (§3.1 "39 semantic readers" + §3.2 orphan carriers) and the residual-inventory test
**under-count an entire class of query-time RESOLVED/GENERATED `TypeExpr` outputs** — surfaces produced by the
resolver at query time, distinct from the authored-body readers the census enumerated. The KNOWN-scope decisions
above stand; these ADDITIONS must be censused, scoped, and (for the largest) sub-designed before B6/B7 briefs are
final.

**Newly-found in-scope semantic-`TypeExpr` surfaces (verified):**

1. **`type_expand` DTO family** — `ExpandedNormalizedExpr.expr`, `ExpandedField.r#type`, `ExpandedObjectShape`,
   `ExpandedProperty`, signatures/params, `ExpandedComponentTypes`, defined in
   `crates/verter_semantic/src/analysis/type_expand/request.rs` and PRODUCED in
   `crates/verter_semantic/src/analysis/type_eval_build.rs` (~:2745+, which reads the B3-owned `Analyzed*.type_expr`
   fields). This is the CENTRAL component-meta RESOLVED-field representation, `TypeExpr`-based, read for SEMANTIC
   decisions at `host_manage/component_meta_extract.rs:260` (root-identity + improvement comparison) and
   `meta_resolve/macro_member_walk.rs:102`, and consumed across **~40 production files** (the entire
   `meta_resolve/projectors/*` published-field pipeline). It appears ZERO times in the design + field-maps.
   **NOT sealed output** — only the terminal `output_sink` materialization is a survivor.
   **Owner:** B6 for the internal handle/node carrier (downstream of fork b's `FastShallowFieldExpr → HotTypeRef`);
   B7 only for the final sealed output DTO arm. **This is a substantial sub-design** (handle-native `ExpandedField`
   across the projector pipeline) that must be codex-designed like the other surfaces — it is NOT a file-list tweak.
   **Coupling into B3:** because `type_eval_build.rs` reads `Analyzed*.type_expr`, B3's narrowing of those fields
   forces this producer to change — B3's `Analyzed*` fact schema must be designed to feed `ExpandedField`'s
   no-`TypeExpr` replacement, so B3 is gated on this sub-design (§6).

2. **`svelte_default_synth` / `ComponentDefaultSynth`** — builds a `LoweredValueDecl` with `TypeExpr` bodies
   (`crates/verter_session/src/resolver_core/svelte_default_synth.rs`, `crates/verter_session/src/framework/synth.rs`),
   whose output lands in the `value_decl("default")` slot (via `insert_synthesised_value_default`) that the
   framework-NEUTRAL synthesized-default dispatch reader `build_vue_default_instance`
   (`project_semantic_dispatch/build.rs:940`, gated on `is_synthesised_component_default`) consumes for
   typeof/instantiate; also read for display by `crates/verter_session/src/framework/api_projectors/svelte.rs`.
   **In-scope.** Mechanism: return a synthesized closed-fact product (facts for `$props`/`$events`/`$slots`/exports/
   legacy-props/shapes; locators for props/dispatcher bodies; session handles only after demand). NOTE: the design
   mentions `svelte_default_synth` once (§5.4:178) as a synthesized-(d) target, but does NOT enter it in the
   39-reader census, the field-map obligations, or any block scope — so it is "mentioned but not censused/scoped/
   assigned," not literally absent. **Owner CONFLICT to resolve:** codex assigned it to B7, but its dispatch consumer
   lives in `build.rs` — a B4 (Wave 2) file — so the
   semantic-carrier cut collides with `B4 ∥ B7`. Recommended resolution: the svelte-synth SEMANTIC-carrier cut
   (touching `build.rs` + `ShallowFileState::insert_synthesised_value_default` + the default-branch dispatch) moves
   to **B6 (Wave 3)**, leaving B7 the persisted facts + output + cache identity. This reassignment needs codex
   ratification.

3. **Svelte capture-candidate cache identity** — `SvelteScriptProvider::VERSION` +
   `stable_candidate_hash` hash `Debug` strings of `props_type`/`dispatcher_events`
   (`crates/verter_semantic/src/analysis/framework_facts/svelte.rs`). When those fields become locators/facts, bump
   the provider version + replace the hash grammar with canonical locator/fact hashing + add a cache-miss
   discrimination fixture. **Owner:** B7.

4. **`resolver_core::type_expansion{,_verter,_host}`** — `TypeExpansionResult`/`ExpandedMember` carry `TypeExpr`
   (`crates/verter_session/src/resolver_core/type_expansion.rs`), apparently stale/protocol-shaped, not a hot
   semantic path. Must be DELETED (if unused) or explicitly quarantined as protocol/output — not left an
   unclassified resolver-core `TypeExpr` API.

**Required actions — ALL COMPLETED (see RESOLUTION at the top of §7):**
- ✓ SYSTEMATIC design re-census of query-time resolved/generated `TypeExpr` surfaces — DONE (design §3.6, ≈96
  surfaces; design §3.1 pointer added; §4.1 scope bullet 10 added; the residual-inventory test's module doc records
  the superseding census).
- ✓ Codex sub-design the `type_expand` handle-native migration — DONE (design §5.7: three-surface split; two unprimed
  legs + code-verifying decider).
- ✓ Ratify the `svelte_default_synth` block reassignment (§7.2) and re-verify `B4∥B7` disjointness — DONE (semantic
  cut → B6; `B4∥B7` re-verified disjoint).
- ✓ **Dispatch gating CLEARED on BOTH axes (per §6 + the §9.3 firewall-form decision):** B5 + TP0 were always clean; B3, B4, B6, B7 are
  dispatchable for their semantic-`TypeExpr` conversion. **The one post-resolution single-engine gate is RESOLVED (AMENDMENT note + §9.3):** the §9.3 firewall FORM is DECIDED — **form (c)** — so B6's
  SINGLE-ENGINE-axis landing and the B1–B8 atomic squash are UNBLOCKED under the re-scoped in-block consolidation. The DAG shape (Wave 1 `B3∥B5`, Wave 2 `B4∥B7`, Wave 3 `B6`) and the fork decisions are UNCHANGED;
  form (c) grows B6's Wave-3-local touch-set (a new `verter_session_query` crate/package + a `verter_compiler` public-API change + the `verter_parser` expander reduction), absorbed into the B6 scope. The re-census grew block scopes but preserved the
  topology and the same-wave file-disjointness. When B4 is dispatched it still must NOT
  touch ANY synthesized-default logic in `build.rs` (B6 owns that migration, including the svelte-synth cut).

## 8. Carried obligations

### 8.1 Type-parameter bound lexical env — TS-exact policy (row CLOSED)

The type-parameter bound position (`TypeBodyPathStep::TypeParamBound { ordinal, position }`) is producer-emitted (`narrow_decl_header_type_params` mints a bound slot for every authored decl-header constraint / default; signature-scoped bounds stay `None` — whole-signature recovery) and deref-served. Its lexical policy is TS-exact, implemented by the ONE shared binder-frame constructor (`ProjectSemanticDispatch::build_type_param_binder_frame`, `crates/verter_session/src/project_semantic_dispatch/locator_shape.rs`) that every binder-frame site routes through — the decl-header shape build, function/constructor signature generics, free authored `TypeParameter` occurrences, and the fact-side re-derivation from `NarrowTypeParam` facts (`locator_binder_frame_from_narrow_params`, bounds via the memoized `LowerLocator` query) — so every derivation interns identical binder ids:

- **Two-phase frames.** Every sibling parameter predeclares a bound-free `TypeParam` SHELL before any bound lowers, so every sibling NAME shadows outer same-named declarations.
- **Constraint at ordinal N** resolves against the FULL sibling frame: prior siblings through their final (bound-carrying) binders; self + later siblings through the predeclared shells (TS permits constraint forward references — `type Foo<T extends U, U>` — and F-bounded self references — `T extends Comparable<T>`). Constraints stay graph EDGES, never eagerly evaluated: a mutual `<T extends U, U extends T>` just creates the edges (the shell break keeps node data acyclic by construction) and resolves to the resolution-time machinery's circular-constraint indeterminate — never an outer capture, never a hang.
- **Default at ordinal N**: prior siblings usable; self + later siblings present-as-shadow but FORBIDDEN (`BinderSlot::ShadowOnly` → the fail-closed `Opaque(Miss)`) — an illegal forward / self default reference never falls through to an outer same-named declaration.
- **Deref carriage.** `navigate_type_space_body` (`crates/verter_session/src/decl_body_memo/locator_deref.rs`) returns the FULL sibling list plus the position-exact `TypeParamVisibility` (`Body` / `Constraint{ordinal}` / `Default{ordinal}`, `verter_type_expr::locators`) — a bare parameter list cannot express present-but-forbidden.

Structurally-invalid demands keep their typed fail-closed errors (`TypeParamBoundStepMisplaced` / `TypeParamOrdinalOutOfRange` / `TypeParamBoundAbsent`). Pinned by `locator_deref_constraint_env_is_full_sibling_list`, `aug_module_type_param_constraint_env_is_full_sibling_list`, `locator_deref_default_env_carries_default_visibility`, `lower_locator_constraint_binds_full_sibling_frame`, `lower_locator_constraint_sibling_shadows_outer_same_named_decl`, `lower_locator_default_forward_and_self_refs_are_shadow_forbidden`, `lower_locator_mutual_circular_constraints_terminate_without_outer_capture`, `lower_locator_f_bounded_constraint_binds_self_shell`. This policy is the six-field row's closure; no carried obligation remains on this axis.

### 8.2 B6 carried obligation (codex-DEFER `shared-overlay-explicit-serve-fail-closed`) — shared overlay-serve explicit-mask fail-closed

An OVERLAY-TIER hardening obligation (distinct from the semantic-`TypeExpr` and single-engine axes) surfaced by the B6/P1 query host port, which routes its artifact serve through the shared overlay-aware resolver tier. Dispositioned by a neutral codex scope adjudication (verdict: NOT a P1 port defect — the port correctly delegates and must not duplicate overlay semantics; the concern is a pre-existing shared-tier fail-open shape). Recorded with the six required fields:

- **Item.** `crates/verter_session/src/host_manage/overlay_priority.rs` `ensure_indexed_ready_serve_with_view`: when `view.overlay_content_hash_for(canonical).is_some()` (an explicit overlay masks the canonical) but `materialize_overlay_indexed_ready_serve_with_view(...)` returns `None`, control falls through to the BASE `host.ensure_indexed_ready_serve(canonical)` — a fail-OPEN code shape that could serve a base artifact (`store_published: true` → base-view `Cacheable`) for a canonical the view explicitly overlays. The explicit-overlay branch should encode that an explicit overlay masks base: return the overlay materializer serve directly, or fail closed to `None`; never fall through to base.
- **Why not now.** Pre-existing property of the SHARED overlay resolver tier (`overlay_priority.rs`/`overlay_materialize.rs`, byte-untouched by the B6 port work; base commit `261e92833`), used by EVERY session-bearing query — not a port concern. Per the Shared-Optimized-Codebase rule a fix belongs in the owner tier (affecting all session queries), NOT the port consumer (which cannot observe overlay-ness without duplicating the tier). The B6 port fix (context-binding to the overlay-aware `ResolverContext`) is complete and correct; hardening the tier's fail-open shape is separate owner-layer work.
- **Owning future block.** The block owning the shared overlay resolver tier (`host_manage::overlay_priority` / `overlay_materialize`); exercised end-to-end at the B6 P5 caller-wiring phase (the first production consumer of the port).
- **Temporary behavior.** No reachable wrong-view `Cacheable` today: under current view-coherence invariants a genuinely-overlaid canonical's `materialize_overlay_indexed_ready_serve_with_view` returns `Some` (coherent source/hash/discriminator; stale overlays RE-materialize rather than base-fallback), and the narrow `None` cases (empty / `is_raw_import_specifier` / mid-flight race) are also declined by the base method or handled by the completion fence. The fail-open SHAPE is latent, not a live wrong-view leak. The sibling F-B concern (raw-overlay-owner vs normalized-analysis-canonical keying) is a NON-ISSUE — the tier already splits the two via `OverlayArtifactIdentity` (incl. the `.js`→`.d.ts` companion case, with a discriminating test).
- **Executable fail-closed.** The tier hardening must make the explicit-overlay branch fail closed (overlay serve or `None`, never base fall-through), pinned by owner-tier tests: `.js` raw owner with `.d.ts` analysis canonical; identical overlay/base bytes with overlay-only route divergence; edge-stale overlay re-materialization; and a synthetic materializer-decline case proving NO base `store_published: true` serve escapes for an explicitly-overlaid canonical.
- **Closure condition.** The row closes when `ensure_indexed_ready_serve_with_view`'s explicit-overlay branch fails closed (never base fall-through) with the pinning tests above, AND the B6 P5 caller-wiring's real-adapter overlay regression verifies `SessionQueryHostPort::new(&session_ctx).lower_authored_body(locator)` over a session overlay uses the overlay `DeclBodyMemo`, maps overlay `store_published`→admission, and returns `ReturnOnly`/miss rather than base `Cacheable` when overlay materialization cannot produce a serve.
