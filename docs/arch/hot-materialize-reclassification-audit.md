# Hot-materialize fence reclassification audit — genuineness ledger

Audit of every count-changing reclassification of the hot-path reverse-materialization
fence (`cases::output_projector_residual_guards::hot_path_never_calls_materialize_type_expr`)
from the intentionally-RED 34-offender landing (`5ccc57272`) to GREEN 0 and through the
post-GREEN allowlist maintenance, on branch `mom/stage9-impl`
(merge-base `a665d4851`, audited tip `15fb50232`).

**Scanner-shape reconciliation (final state).** The audit was performed against tip
`15fb50232` and certified all 34 reclassifications GENUINE. All line references below
(source files and the guard file) cite that audited tip unless labelled otherwise.
After the audited tip the scanner was hardened and swept on the SAME branch — none of
which changes any historical reclassification verdict, and the three scanner allowlists
(`HOT_TERMINAL_SINKS` at 35 entries, `HOT_LOWERING_IDENTS`,
`HOT_TERMINAL_PASSTHROUGH_IDENTS`) are byte-identical between the audited tip and the
post-sweep scanner:

- `71d9e3e35` — collection-mutation receiver taint through `push`/`insert`/`extend`
  (closes the §5 owed gap; recorded as the third hardening, `hardening_rounds: 3`).
- `4f8c63ad2` — inert FN6 detector-spelling sweep, deletion of the inert
  extracting-gate rail, and a detector-spelling liveness guard.
- `ceabc7278` — both-rails load-bearing proof in the default gate (in-memory scanner
  revert-probe + always-on scoped structural smoke).
- `4c5ce82a0` — the SC-first record's `scanner_invariant` id renamed from
  `stage9_residual_hot_materialize_syntactic_tripwire` to the phase-neutral
  `hot_materialize_syntactic_tripwire_residual_backstop`.
- `d1eb687d5` — governance-record vocabulary scrub. The in-tree SC-first record cites
  the reopen ruling as `codex-s9-5c-scope-ruling-2026-07-02` — the same ruling the
  close-out artifacts name `codex-s9-5c-scope-consult-2026-07-02`, respelled in-tree
  because the phase-archaeology guard bans that vocabulary pairing on a single
  test-tree line.

**Headline: ALL 34 reclassifications GENUINE. Zero NON-GENUINE, zero SUSPECT, zero
UNVERIFIED. No allowlist offender-hiding beyond the 2 known-closed instances; both
confirmed closed. No hiding windows (no commit range in which an allowlist entry
concealed a live materialize-then-decide body).**

Audit method: adversarial, diff-first (commit messages never trusted); every
"node fact" helper followed to its implementation and required to bottom out in
`SemanticNodeData` / graph folds / interned `RaisedShapeKey` comparisons — never a
freshly materialized `TypeExpr`; every sink addition's body read at tip and required to
be a one-shot terminal publication with no branching on materialized DTO variants;
every removal required to be deletion- or conversion-backed.

---

## 1. Per-entry ledger — the 34 RED sites

Key: offender = qualified fn from `dispatchA-RED-inventory.txt`. Disposition:
CONV = node-domain conversion; DEL = deletion (with replacement identified);
CONV+SINK = converted and registered terminal in the same commit.
All files under `crates/verter_session/src/` unless noted.

| # | Offender (RED key, abbreviated) | Exit commit(s) | Disp. | Before → after | Why node-domain now / deletion evidence | Pinned by | Verdict |
|---|---|---|---|---|---|---|---|
| 1 | `host_manage::component_meta_methods::append_component_meta_registry_entries` | `c42741c22` | CONV | `component_meta_registry_has_explicit_object_surface(&materialized)` → reads precomputed `RegistryCandidate.explicit_object_surface` | fact produced in the query engine off the member NODE (`component_meta_registry_node_has_explicit_object_surface`, node_materialize.rs:486 — visited-set walk over `SemanticNodeData` arms) | `node_materialize_predicate_tests.rs` differential parity (:97/:879/:984); fence GREEN | GENUINE |
| 2 | `…::materialize_component_meta_registry_candidate` (nested) | `c42741c22` (+`4253b0cbf`) | CONV | bridge `project_type_surface_expr_via_host_threaded` + `type_expr_contains_semantic_miss` on stabilized values → all decides on authored INPUT `raw_body` or node facts; materialized values flow only to sinks/restitcher | diff shows `-type_expr_contains_semantic_miss(...)` on stabilized values; bridge deleted (zero tip refs) | same + fence | GENUINE |
| 3 | `…::materialize_component_meta_registry_candidate_for_route` (nested) | `c42741c22` | CONV | `match TypeExpr::…` / `matches!` / callable-surface gate on materialized → per-branch facts from query-engine node producers (`project_member_path_leaf_facts` etc.) | surviving `type_expr_contains_callable_surface(&raw_member_leaf)` reads the RAW authored leaf (input, feeds a debug counter only) | fence + registry tests (`8840064fe`) | GENUINE |
| 4 | `…::owner_local_generic_alias_substituted_body_via_dispatch` (nested) | `c42741c22` conv, `1bf455d24` relocated | CONV+move | `matches!(raised, TypeExpr::Object(_))` after `into_type_expr` → `node_raises_to_object_surface` (shape-engine fold), relocated to `node_materialize::owner_local_generic_alias_candidate` | diff shows the exact `-matches!(raised, TypeExpr::Object(_))` deletion | fence; fn absent at tip | GENUINE |
| 5 | `host_manage::fallthrough::resolve_dynamic_root_candidates` | `4253b0cbf` | CONV | tainted value → `collect_dynamic_root_candidates_from_type` → node walker `collect_dynamic_root_candidates_from_node`; TypeExpr collector now receives only the SYNTACTIC parse (authored input) | evaluated path walks `SemanticNodeData`; bounding follow-ups `3c81e6dc4`/`a66fa3196` | fallthrough tests + `fallthrough_value_eval_recursion_tests.rs` | GENUINE |
| 6 | `host_manage::fallthrough::resolve_root_consumption` | `4253b0cbf` | CONV | `known_spread_keys_from_type_expr` on materialized → `known_spread_keys_from_node` | node walker verified | same | GENUINE |
| 7 | `host_manage::intrinsic_projection::expand_project_intrinsic_shape_for_canonical` | `9111d0a04` | CONV | bridge + `project_expr_class_a_via_dispatch` fallback + `type_expr_to_object_shape` → one call `engine.project_intrinsic_root_shape` | PRIMARY: `dispatch_projected_surface_with_node` + `resolve_typeinfo_surface_view` + `surface_view_to_expanded_shape` (SurfaceView → `surface_view_to_projected_surface` sink + pure map — never `type_expr_to_object_shape`); FALLBACK: `project_expr_class_a_node_via_dispatch_threaded` + `project_admitted_route_node_to_expanded_object_shape` (intrinsic_surface.rs:43-64) | fence; `intrinsic_member_surface_keeps_top_level_ref_shallow` (TRAP 4); meta_tests characterizations | GENUINE |
| 8 | `…::expand_project_intrinsic_tag_members_for_canonical` | `bb71c2815` (deliberately NOT converted at `9111d0a04` — honest TODO; revert `afc2ff072` kept the converted impl) | CONV | Class-A materializing bridge + `type_expr_to_object_shape` (last-arm-wins) → `project_intrinsic_tag_member_shape` = Class-A NODE projection + admitted-node object shape (intrinsic_surface.rs:75-88); deliberate semantic correction to TS-correct value-intersection, test corrected accordingly | tip body (intrinsic_projection.rs:104-139) has no bridge / no `type_expr_to_object_shape` | `project_local_intrinsics_tag_members_value_intersect_conflicting_fallback` (meta_tests) | GENUINE |
| 9 | `…::materialize_project_intrinsic_member_surface_expr` | `9111d0a04` | CONV | `solved != *expr` TypeExpr convergence fixpoint → `stabilize_intrinsic_member_surface`: node fixpoint converging via `route_projection_node_eq_to_expr` / `route_projection_nodes_eq` (interned `RaisedShapeKey` comparisons — surface.rs:181-207, raise.rs:4829-4887, shape_engine-backed), single mint at the registered `materialize_route_projection_node` sink | structural recursion split into `materialize_project_intrinsic_member_structural` preserving `nested_surface` shallow gating | `intrinsic_member_fixpoint_converges_in_node_domain_not_per_iteration_materialize` (meta_tests.rs:23057); TRAP 4 | GENUINE |
| 10 | `host_manage::jsdoc_resolve::owner_local_macro_root_has_surface` | `0e18f538e` | CONV | bridge + `.is_empty()` on materialized `ExpandedObjectShape` → shared `owner_local_macro_root_surface_presence` reading `SurfaceView` cardinality per macro kind | bridge `project_expr_surface_shape_via_host_threaded` deleted (zero tip refs) | fence + jsdoc tests | GENUINE |
| 11 | `…::projectable_owner_local_macro_roots` | `0e18f538e` | CONV | same signals → same shared presence helper (gate + pre-filter agree by construction) | same | same | GENUINE |
| 12 | `meta_resolve::dispatch_helpers::project_expr_class_a_via_dispatch_threaded` | `c42741c22` (node sibling) → `4253b0cbf` (DELETED) | CONV+DEL | bridge → `project_expr_class_a_node_via_dispatch_threaded` returning `Option<AdmittedRouteProjectionNode>` (dispatch_helpers.rs:415-467); every caller re-pointed | tip's engine-less `project_expr_class_a_via_dispatch` wraps `project_class_a_published` = node projection + registered sink | fence; fallthrough_value_eval guard asserts the node name | GENUINE |
| 13 | `…::project_expr_surface_expr_via_host_threaded` | `c837ce8c3` (orphaned) → `294321194` (DELETED) | DEL | production-orphan at deletion (only `#[cfg(test)]` callers + own `*_published` tail, `cfg_attr(not(test), allow(dead_code))`) | deleted with its `HOT_MAT_BRIDGE_IDENTS` + sink entries in one commit | 3 `differential_route_shape_*` tests added same commit | GENUINE |
| 14 | `meta_resolve::materialize::field_types::materialize_component_meta_type_expr_until_stable_full` | `6433ba184` | CONV+SINK | `type_expr_root_is_unmaterialized_sentinel(materialized.type_expr(&cap))` → `materialized.node_id().is_some_and(node_root_is_unmaterialized_sentinel_with_dispatch)` (tip field_types.rs:708-713); helper = pure `SemanticNodeData` root-summary fold (sentinel bit from summary leaves only) | remaining `matches!(expr, TypeOf(_))` is on the INPUT the fn lowers (`shallow_lower_type_expr_with_context` :611) = sanctioned symbolic-input mint boundary | `node_root_sentinel_is_root_only_not_whole_surface_miss` parity test; purity rail | GENUINE |
| 15 | `meta_resolve::projectors::output_sink::materialized_root_is_unmaterialized_sentinel` | `81ecd232b` | DEL | fn deleted outright (`-fn` in diff); replacement = node-domain sentinel read inline in site 18's body (tip output_sink.rs:1018-1022) | old TypeExpr predicate survives only as parity oracle | parity test above | GENUINE |
| 16 | `…::member_shape_peek_or_compute` | `ccdba4c55` (facts: `908484d63`/`5450aefbe`/`066cca7da`) | CONV | raised-TypeExpr gate set (package-backed root, transitive cycle, reducible-operator, TypeExpr-keyed peek, 6× `seal_type_expr`) → node gates on the ADMITTED member node BEFORE any materialize: `node_package_backed_object_like_root_with_fence` (:302), `classify_node_reduction_gates` (:326), `node_root_reaches_transitive_cycle_with_fence` (:333); peek keyed by node | helpers verified to fold `SemanticNodeData` directly (published_reducer.rs:166-240; graph_predicates.rs:1076); only materialized-carrier read is `result_is_partial()` (completeness flag) | syn-AST characterization in meta_tests (old predicates ABSENT + node calls PRESENT); `5b9147ed5` re-pins | GENUINE |
| 17 | `…::project_model` | `ea95a9f2e` | CONV+SINK | `type_expr_contains_reducible_operator(&raised)` → `classify_node_reduction_gates(ctx, payload_node).contains_reducible_operator` decided on the payload NODE before materializing (tip :1123-1124) | the RED note "STAYS RED" described the pre-conversion state; conversion + allowlisting simultaneous — no window | `output_sink_conversions_decide_in_node_domain_not_on_materialized_type_expr` | GENUINE |
| 18 | `…::reduce_field_type_expr_with_mode` | `81ecd232b` | CONV | decide via deleted helper #15 + unwrap → all shape gates read the INPUT `&expr` (authored analyzer DTO); cold mint via registered `materialize_field_value_carrier`; ONLY materialized-carrier decide is the node-domain root-sentinel (:1018-1022); returns sealed carrier | | `reduce_field_bare_carrier_publishes_shallow_without_poisoning_shared_cache_slot` | GENUINE |
| 19 | `…::reduce_published_field_types` | `81ecd232b` (+`be53155c7`, `3b35cb2cf`) | CONV+SINK | `compare_type_expr_improvement` on materialized pair → `compare_node_improvement(ctx, sn, rn) \|\| node_root_is_explicit_selector_operator(ctx, sn)` over NODES, decided BEFORE the single `unwrap_materialized` per field (:1234-1266) | `project_node_publication_score` = `fold_node` with `PublicationScoreAlg` ("WITHOUT materialising a TypeExpr", publication.rs:688); scoring.rs:78-113 matches `SemanticNodeData` arms; `3b35cb2cf` = unraisable ⇒ never an improvement | `compare_node_improvement_matches_type_expr_comparator_per_clause` differential + `…_unraisable_candidate_is_never_an_improvement` | GENUINE |
| 20 | `meta_resolve::registry_materialize::materialize_component_meta_registry_structural_expr::inner` | `4253b0cbf` | DEL+rebuild | bridge-calling structural materialiser (−542 lines) → node-domain rebuild `registry_structural.rs::materialize_registry_structural_candidate` (+619): facts composed per-arm from producing-node leaves, "never recovered by re-lowering" (no `has_explicit_object_surface` in the rebuild) | honest interim: between `c42741c22` and `4253b0cbf` the site stayed RED at its own key — never hidden | fence; registry structural tests | GENUINE |
| 21 | `resolver_core::…::registry_decl::dispatch_routed_expr_surface_expr` | `4cebaaaec` (CONV+SINK) → `4253b0cbf` (DELETED) | CONV→DEL | `.filter(dispatch_route_expr_is_materialized)` on materialized → admit BEFORE materializing via `node_raised_shape_facts_with_dispatch(…).filter(\|f\| f.materialized)` → `AdmittedRouteProjectionNode`; surviving terminal body minted once, NO post-mint filter | **no hiding window** — while allowlisted the body was a genuine terminal; in-window edit `44ce243f2` tightened the node gate; node sibling `dispatch_routed_expr_surface_node` survives (registry_decl.rs:1100) | fence; route tests | GENUINE |
| 22 | `…::dispatch_routed_pick_omit_via_shared_engine` | `4cebaaaec` | CONV (renamed `…_node`) | same decide → tip returns `Option<AdmittedRouteProjectionNode>`; gate = `node_raised_shape_facts_with_dispatch` + `route_admission::admit_materialized` (registry_decl.rs:1205-1206) | bare old name absent at tip (renamed sibling) | `routed_pick_omit_projects_in_node_domain_via_shared_engine` (meta_tests.rs:23158) | GENUINE |
| 23 | `…::registry_decl::project_expr_to_surface_shape` | `c837ce8c3` (CONV) → `d8956bc8d` (DEL, dead cluster) | CONV→DEL | `type_expr_to_object_shape(&projected)` → `dispatch_routed_expr_surface_node` + `project_admitted_route_node_to_expanded_object_shape` (SurfaceView; leaf-only materialization through the registered surface sink) | later deleted as part of the internally-connected orphan cycle (zero external production entries, verified caller greps) | discrimination test (zero old-call assertions) + 5 shape characterizations + `RETIRED_UTILITY_SHAPE_CLUSTER` tombstone (architecture_guards.rs:1800-1823, `11d05f3c9`) | GENUINE |
| 24 | `…::route_keys::enumerate_member_surface_keys_via_route` | `0c6e9193f` (file deleted `d8956bc8d`) | DEL+replace | ~350-line `match TypeExpr` walkers deleted → `enumerate_public_surface_member_names_from_admitted_node` = `resolve_typeinfo_surface_view(node, Shallow).members` | `legacy_*` copies were `#[cfg(test)]` differential oracles (compile-absent) until `d8956bc8d` | `e6f3b5dfb`/`89146d6b4` differential locks (retired with the dead cluster); fence at tip | GENUINE |
| 25 | `…::route_keys::enumerate_route_literal_keys_inner` | `0c6e9193f` (file deleted `d8956bc8d`) | DEL+replace | → `enumerate_keyspace_names_from_admitted_node` → `dispatch.key_names_from_keyspace_node(node)`; distinct keyspace-node token (`dd272e13a`) | same | same | GENUINE |
| 26 | `…::route_keys::project_direct_utility_surface_shape::projected_target_shape` | `c837ce8c3` (CONV) → `d8956bc8d` (DEL) | CONV→DEL | bridges + gate + `shape_has_surface` → engine node method + `project_expr_surface_expr_node_via_host_threaded`; `shape_has_surface` on node-derived DTO; Navigate/Shallow arm routing + budget guard preserved | index-signature asymmetry (registry counts / direct-utility ignores) explicitly preserved + test-pinned; cluster later dead-deleted | `aa087e167` class-utility visibility pin; tombstone | GENUINE |
| 27 | `…::route_keys::solve_or_project_leaf_expr_until_stable` | `4cebaaaec` (CONV) → `d8956bc8d` (DEL) | CONV→DEL | per-iteration `if next == current` TypeExpr compare → `RouteFixpointCursor{Input\|Node}` converging via `route_projection_node_eq_to_expr`/`route_projection_nodes_eq` (interned `RaisedShapeKey`), one post-convergence mint | live successor: `solve_or_project_intrinsic_member_node_until_stable` (intrinsic_surface.rs:139-175) | `intrinsic_member_fixpoint_converges_in_node_domain_not_per_iteration_materialize` | GENUINE |
| 28 | `…::route_keys::solve_or_project_leaf_expr_with_context` | `0c6e9193f` (CONV) → `d8956bc8d` (DEL) | CONV→DEL | `==`/`!=` convergence → `StableRouteLeafNode { node, eq_to_input }` with `eq_to_input` from node-shape equality ("never TypeExpr ==") | | same | GENUINE |
| 29 | `resolver_core::fallthrough::resolve_fallthrough_surface` | `4253b0cbf` | CONV | tainted → `append_component_candidate_branches` with `FxHashMap<String,TypeExpr>` override map → node-backed `FallthroughPropOverrideSet { node: SemanticNodeId }`; `inject_prop_type_overrides` (EvalEnv re-injection) DELETED | | `append_component_candidate_branches_*` tests; fence | GENUINE |
| 30 | `typeinfo::framework_surface::svelte_exec::callback_events_from_props_surface` | `e0f690918` | CONV | `callable_arm_from_raised(&raised)` after mint → `CallableNodeView::new(&dispatch, member.value).signature(context)` → `raw_params()`; single mint at svelte `materialize_payload_tuple` (tip svelte_exec.rs:1017-1028) | `meta_resolve/callable_view.rs` has ZERO non-comment `TypeExpr` uses — decides exclusively on `SemanticNodeData` via `node_data_for` | `callable_view_tests.rs` (+398 @`362c03a84`); fence | GENUINE |
| 31 | `…::svelte_exec::svelte_snippet_slots_from_typeinfo_surface` | `362c03a84` | CONV | `snippet_callable_positional_bindings(&value)` after mint → `validated_snippet_positional_params(context)` backed by exhaustive no-wildcard `SemanticNodeData` classifier `classify_snippet_params_arg` (callable_view.rs:106-151); mint only at `materialize_snippet_slot_bindings` | | svelte_exec_tests (+147); fence | GENUINE |
| 32 | `…::vue_exec::normalize::binding_fields_from_param_ty` | `362c03a84` | DEL+rewrite | the caught-hiding fn (see §7): branched `if let TypeExpr::Object`, navigated, Pick-shape-matched, minted per-member → DELETED (zero tip refs, along with `slot_callable_param_and_return`, `callable_arm_from_raised`, `pick_named_source_root`); replacement `binding_fields_from_param_node(first_param: SemanticNodeId)` gates on `slot_param_root_is_symbolic_only` (pure `node_data_for` walk); Pick root via `pick_source_root_node` (normalize.rs:689-702 — `InstantiationRef` `__builtin__::Pick` + nominal-root set incl. `BareRef`: typed-IR identity, no text sniffing) | `c0b89dc7d`/`38b1bec9e` refinements node-domain only (their `matches!(TypeExpr::…)` additions are TEST-file assertions) | fence self-tests T/U (allowlist self-policing proves it can never return); symbolic-Pick `IndexedAccess` assertions; behavioral first-param slot guard (`e884c98fd`) | GENUINE |
| 33 | `…::vue_exec::normalize::emits_from_typeinfo_surface` | `e0f690918` (+`d5996a94e`) | CONV | `let Some(TypeExpr::Function(func)) = &raised` + `match &first.ty { Literal \| Union }` → `view.event_names(context)` / `view.signature(context)` (CallableNodeView); property fallback gated on node-domain discovery bool `call_signature_emit_found` (replaced the `emits.is_empty()` DTO-cardinality gate — a tightening) | mints confined to `materialize_payload_tuple` / `property_style_emit_fields` sinks | callable_view event_names tests; fence | GENUINE |
| 34 | `…::vue_exec::normalize::slots_from_typeinfo_surface` | `362c03a84` | CONV | cardinality decide on materialized + `slot_callable_param_and_return(&value)` + tainted args → `realized_callable_root(context)`, combiner from `match node_data_for(…) { SemanticNodeData::Union(_) … }`, `slot_param_and_return_by_arm(combine, context)`; return minted ONCE at `materialize_slot_return_node`; `slice_canonical_span` now fed a node-domain SPAN (display-only) | old RED taint chain structurally gone | callable_view slot tests; fence | GENUINE |

---

## 2. `HOT_TERMINAL_SINKS` — 14 additions (chronological), per-entry evidence

Baseline at RED landing `5ccc57272`: 27 entries. Tip `15fb50232`: 35 entries
(unchanged by the post-audit scanner hardening — the list is byte-identical at the
post-sweep scanner). 14 additions − 6 removals = +8. Every addition's body was
verified at tip (and, for the ex-RED entries, at the adding commit): one-shot terminal
publication, no branching on materialized DTO variants. The purity rail
(`hot_terminal_allowlist_entries_are_pure_one_shot_sinks`) seeds each entry's
`TypeExpr` params tainted and treats the body as non-terminal, so a
decide-injected-here STILL fires (discrimination self-test proven); per-entry
accounting (`hot_terminal_allowlist_accounting_failures`) fails duplicates and
zero-located entries by name.

| Added entry | Commit | Evidence |
|---|---|---|
| `surface.rs::materialize_route_projection_node` | `4cebaaaec` | tip: one-shot `materialize_published_node(&dispatch, node.node())` over the sealed `AdmittedRouteProjectionNode`; zero branching. Also classified sink-local raiser (`d96eefbf5`). |
| `registry_decl.rs::dispatch_routed_expr_surface_expr` | `4cebaaaec` (removed `4253b0cbf`) | transient; genuine terminal while listed (admission decided on node facts BEFORE materialize; no post-mint filter); deleted with its entry. |
| `field_types.rs::materialize_component_meta_type_expr_until_stable_full` | `6433ba184` | ex-RED #14; conversion in the SAME commit (node root-sentinel admission fact); `expr` input is lowered (symbolic-input mint boundary). |
| `output_sink.rs::project_model` | `ea95a9f2e` | ex-RED #17; only branch = node-domain reducibility fact (`classify_node_reduction_gates`); takes no `TypeExpr` param. |
| `output_sink.rs::raise_node_to_sealed_carrier` | `ccdba4c55` | tip :108-133 — one `cap.materialize_output_type_expr(node)`; raise-miss seals an Unknown shell (publication default); carrier assembled from parts; zero variant decide; `SemanticNodeId` param. |
| `output_sink.rs::materialize_field_value_carrier` | `81ecd232b` | tip :150-174 — pure delegation to the registered `until_stable_full` sink. |
| `output_sink.rs::reduce_published_field_types` | `81ecd232b` | ex-RED #19; improvement chosen on NODES before the single per-field unwrap; no `TypeExpr` param. |
| `surface.rs::materialize_registry_publication_node` | `633a034e5` | one-shot `materialize_published_node` over the no-admission-claim `RegistryPublicationNode` carrier; object-surface fact read off the node separately. |
| `vue_exec/normalize.rs::materialize_payload_tuple` | `e0f690918` | per-param node mint into labelled `TupleElement`s; `unwrap_or(Unknown)` = mint-success `Option` fallback (position-preserving), not a variant decide; caps constructed internally since `d1c2038d0` (mint authority never crosses from non-terminal callers). |
| `vue_exec/normalize.rs::property_style_emit_fields` | `e0f690918` | iterates PUBLIC members (node-domain visibility fact), mints via registered `raise_member_value`; structurally identical to `props_from_typeinfo_surface`. |
| `svelte_exec.rs::materialize_payload_tuple` | `e0f690918` | Svelte-cap twin; same shape. |
| `svelte_exec.rs::materialize_snippet_slot_bindings` | `362c03a84` | tip :654-691 — per-`PositionalParamNode` one-shot mint; display via by-name `.and_then(render_type_expr_display)`; no `TypeExpr` param. |
| `vue_exec/normalize.rs::materialize_slot_return_node` | `362c03a84` | tip :570-582 — single-node one-shot mint, `unwrap_or(Unknown)` robustness fallback only. |
| `vue_exec/normalize.rs::slot_binding_field` | `362c03a84` | only branch is the node-domain `Option<SemanticNodeId>` Pick source-root (decided in the non-terminal `binding_fields_from_param_node`); Pick member published as SYMBOLIC `NamedRoot['member']`. |

## 3. `HOT_TERMINAL_SINKS` — 6 removals, per-entry evidence

All deletion-backed; each fn deleted in the SAME commit that removed its allowlist
entry (no stale windows), except the one disclosed nuance below.

| Removed entry | Commit | Evidence |
|---|---|---|
| `surface.rs::project_expr_surface_expr_published` | `294321194` | fn deleted same commit. Nuance: for exactly one commit (`c837ce8c3`→`294321194`) the entry pointed at a production-ORPHANED (test-only, `cfg_attr(not(test), allow(dead_code))`) fn — it remained a pure one-shot terminal during that window, so not offender-hiding. |
| `surface.rs::instantiate_local_generic_ref_published` | `0c6e9193f` | fn + `via_dispatch` wrapper + callers deleted same commit (last callers were inside the deleted split-scope arm). |
| `surface.rs::lower_and_project_to_expanded_published` | `4253b0cbf` | fn deleted same commit. |
| `surface.rs::project_class_a_terminal_published` | `4253b0cbf` | fn deleted same commit. |
| `registry_decl.rs::dispatch_routed_expr_surface_expr` | `4253b0cbf` | fn deleted same commit (see §1 #21). |
| `vue_exec/mod.rs::raise_realized_callable_member_value` | `9386ee486` | orphan deletion (42 lines) after its last caller was converted at `362c03a84`; guard authorizations removed same commit; zero tip references. |

## 4. `HOT_LOWERING_IDENTS` — growth 3→6

Baseline: `lower_type_expr_in_scope`, `…_with_mode`, `…_with_context`.

| Ident | Commit | Evidence |
|---|---|---|
| `lower_and_project_to_expanded_node` | +`4cebaaaec` | internally calls `dispatch.lower_type_expr_in_scope_with_mode`, returns `Option<AdmittedRouteProjectionNode>` (never a `TypeExpr`); node-fact gates only — a true pipeline feed, cannot launder a decide. |
| `project_expr_surface_expr_node` | +`4cebaaaec`, −`d8956bc8d` | transient; same shape at add; fn deleted at `d8956bc8d` with its ident removed in the same commit. |
| `shallow_lower_type_expr_with_context` | +`6433ba184` | lower.rs:142 — the workspace's SOLE eager `TypeExpr → SemanticNodeId` lowering path (single-definition-guarded by `type_expr_lowering_has_exactly_two_single_definition_producers`); returns a node. |
| `project_class_a_terminal_node` | +`c837ce8c3` | decomposes the IndexedAccess chain, lowers internally, projects `ProjectPath`, returns the admitted node. |

## 5. `HOT_TERMINAL_PASSTHROUGH_IDENTS` `push`/`insert`/`extend` — re-scoped-vs-owed determination

**Determination: FORMALLY RE-SCOPED with a full governance paper trail — NOT silently
dropped. Still OWED at the audited tip `15fb50232` (the scanner there had no
collection-mutation receiver taint); subsequently LANDED on this branch at
`71d9e3e35` as the record's third hardening (`hardening_rounds: 3`).**

History (verified against briefs, PROGRESS.md, and commit ancestry):
1. `push`/`insert`/`extend` were passthrough-excluded from the unknown-helper rail at
   RED landing (`5ccc57272`) with NO receiver taint — a disclosed syntactic gap.
2. The first §5c implementation brief (`brief-5c-impl.md`, PART B) demanded
   mutation-taint propagation (codex adjudication Q3: local `Vec::new()` +
   `push(tainted)` + `.len()` escapes the fence), alongside PART A (converting two
   codex-ruled NOT-legitimate arm-cardinality sinks).
3. That first implementer produced commit `9d15889c7` (parent `58a1663cd`) containing
   BOTH parts — but that lineage forked at `b937e0536` and was **never landed**
   (`git merge-base --is-ancestor 9d15889c7 15fb50232` ⇒ false). The manager HELD it on
   a governance flag: the scanner's hardening record was FROZEN at
   `hardening_rounds: 2` after a laundering escape, so a third hardening required an
   explicit codex structural-reopen ruling (SC-first).
4. The codex scope ruling (`codex-s9-5c-scope-consult-2026-07-02`) ruled the gap REAL
   ("a real syntactic soundness gap … treat the collection-taint fix as scanner
   hardening, not as a new allowlist broadening") and re-authorized it.
5. The follow-up implementer brief (`s9-5c-r2-implA-brief.md`, COMMIT 1 / Item 2(c))
   formally scheduled it: keep the idents on the passthrough list, ADD receiver-taint
   propagation, discriminating self-tests (positive + two negative controls, with a
   recorded pre-change failure), and the `hardening_rounds: 3` governance record naming
   the ruling. Landed post-audit at `71d9e3e35`.
6. Landed-tree exposure check: the landed lineage NEVER contained the vec-len
   materialization-outcome shapes the gap would have hidden (greps at `579e56fb9`,
   `d9adfe289`, `15fb50232` show zero `Vec<TypeExpr>`+len shapes in the framework
   normalizers; the shapes existed only on the unlanded `58a1663cd` tree). A tip-wide
   `Vec<TypeExpr>` sweep found only input-side copy-on-write rewriters
   (component_meta_resolution_policy/core.rs), the shape-engine's own fold materializer,
   and oracle/lowering-input paths — no minted-collection cardinality decide exploiting
   the blind spot at tip.

## 6. Intrinsic-tag exit (`9111d0a04` / `bb71c2815` / `9337dbd61`+`7fd73dfe4` / `fa7ae1b3f` / `afc2ff072`) — independent verification

Verified from diffs (not commit messages): **genuine node-domain conversion; the
reverted partial-recovery feature is separate behavior debt, not fence laundering** —
codex's preliminary read independently CONFIRMED.

- `9111d0a04` converted sites #7/#9 (see §1) and honestly documented site #8 as NOT
  converted (TODO), keeping it RED — the opposite of hiding.
- `bb71c2815` converted site #8 with a deliberate, test-corrected semantic change
  (last-arm-override → TS-correct value-intersection).
- `7fd73dfe4`/`9337dbd61` added a partial intrinsic-tag surface recovery FALLBACK
  (itself node-domain: `resolvable_intersection_remainder` + lower + SurfaceView — not
  a materialize-then-decide); `fa7ae1b3f` narrowed it; `afc2ff072` REVERTED it as a
  reachable no-poison BLOCKER (scope-added feature, not fence work), restoring the
  plain converted impl and recording 3 codex-DEFER rows
  (`PROJECTPATH_INTERSECTION_OPAQUE_LAUNDERING`, `INTRINSIC_STATIC_FALLBACK_WARM_CACHE`,
  `INTRINSIC_PARTIAL_RECOVERY_NO_POISON`).
- Tip engine rail (intrinsic_surface.rs) verified: convergence via interned
  `RaisedShapeKey` (`node_raised_shape_for_eq_with_dispatch` /
  `raised_shape_eq_nodes` → shape_engine), shape building via SurfaceView, single mint
  at the registered `materialize_route_projection_node` sink. No
  `type_expr_to_object_shape` anywhere on the rail.

## 7. Known-closed hiding instances + stale/hiding sweep

1. **`index_key_to_type_expr` stale on `SINK_LOCAL_RAW_AUTHORITY_ALLOWLIST` — CONFIRMED
   CLOSED at `d9adfe289`.** The entry predates the branch (present at merge-base
   `a665d4851`); the fn definition exists NOWHERE at merge-base or tip (a deletion/
   rename predating this branch — the live similarly-named helper is
   `raise_index_key_to_type_expr`, comment-referenced only). Not an active concealment
   (nothing existed to hide), but a dormant grant a future fn of that name would have
   inherited. `d9adfe289` removed it and added per-entry stale accounting
   (`sink_local_raw_authority_accounting_failures`) with a discrimination self-test
   (`…_is_per_entry_not_aggregate`).
2. **`binding_fields_from_param_ty` caught hiding on `HOT_TERMINAL_SINKS` — CONFIRMED
   CLOSED.** Removed from the allowlist BEFORE the RED landing (the FIX-2 swap recorded
   in the RED inventory; the landed `5ccc57272` allowlist carries the explanatory NOT-here
   NOTE), flagged RED as #32, then genuinely deleted + rewritten node-domain at
   `362c03a84` (§1 #32). The purity rail (self-test U) proves it could not have
   re-entered the allowlist.
3. **Sweep for further stale/hiding entries — NONE FOUND.** Independent per-entry
   existence check at tip: all 35 `HOT_TERMINAL_SINKS` entries locate ≥1 production fn
   (multi-def entries are the documented sealed carrier chain: `into_type_expr` ×3,
   `type_expr` ×2), no duplicate tuples, every file suffix unambiguous (matches exactly
   one file). All 26 `SINK_LOCAL_RAW_AUTHORITY_ALLOWLIST` entries locate; the two
   `*_for_test` raisers and `project_node_to_type_expr_for_test` are cfg-gated
   (test/oracle-gen — compile-absent from production). Terminal-purity of every sink
   entry re-verified by body reads (forks A–E + §2). No duplicate-name shadowing across
   files (the two `materialize_payload_tuple` entries are distinct file-suffix keys by
   design).

## 8. Residual notes (none is a blocker)

- **Close-out items decided by ruling `codex-s9-5c-scope-consult-2026-07-02` — since
  LANDED on this branch (post-audit):** collection-mutation receiver taint
  (`71d9e3e35`, §5), the inert FN6 detector-spelling sweep + inert extracting-gate
  rail deletion + detector-spelling liveness guard (`4f8c63ad2`; residual-deferral doc
  context at `6c2e41bc0`), the both-rails load-bearing proof (`ceabc7278`), and the
  three degenerate-edge behavior pins + the slot raise-miss producer normalization
  (`5eda5440e`, `77c8c0f7c`, `d8eab3628`). This ledger's scanner-shape statements are
  reconciled to that post-sweep shape.
- **Purity-rail enumerated limit (disclosed by design):** the rail seeds only
  `TypeExpr`-typed params; a sink receiving materialized values inside a non-`TypeExpr`
  carrier is covered by the in-body mint-then-decide rails + this audit's manual body
  reads, not by param seeding.
- `navigate_param_to_object_surface` (TypeExpr-navigating helper) still serves svelte
  normalizers that were never in the RED-34 set (svelte_exec.rs:282,478,543,911,952) —
  outside this audit's population; flagged for any future fence-scope widening.
- `lower_and_project_to_expanded_via_host_threaded` (deleted bridge) is fenced against
  CALLS (`HOT_MAT_BRIDGE_IDENTS`) but, at the audited tip, absent from the
  `RETIRED_UTILITY_SHAPE_CLUSTER` tombstone — an uncalled reintroduced DEFINITION under
  that exact name would not be flagged until called. Defense-in-depth nit, flagged for
  the close-out.
- Stale comment at `host_manage/component_meta_methods.rs:1487-1497` (at the audited
  tip) still claimed the structural-materialiser fact "is NOT node-domain here … a
  later block converts" — contradicted by the landed `registry_structural.rs` rebuild.
  Docs-only; flagged for the close-out.
- `type_expr_to_object_shape` still EXISTS in `verter_semantic` (type_expand/mod.rs:107)
  with zero production callers in any crate; reintroduction is fenced
  (`HOT_DECIDE_STANDALONE_IDENTS`) and pinned by meta_tests.rs:23237.

## 9. Re-run commands (audit reproduction)

Run from the root of any checkout with branch `mom/stage9-impl` reachable.

```bash
REPO=.   # repo root
GUARD=crates/verter_session/tests/cases/output_projector_residual_guards.rs

# Guard-file commit spine (23 commits):
git -C $REPO log --oneline a665d4851..15fb50232 -- $GUARD

# Allowlist timeline (extract HOT_TERMINAL_SINKS / HOT_LOWERING_IDENTS /
# HOT_TERMINAL_PASSTHROUGH_IDENTS at consecutive spine commits and diff):
git -C $REPO show 5ccc57272:$GUARD | sed -n '/const HOT_TERMINAL_SINKS/,/^];/p'
git -C $REPO show 15fb50232:$GUARD | sed -n '/const HOT_TERMINAL_SINKS/,/^];/p'

# Per-site exit diffs (production only):
git -C $REPO show <exit-commit> -- crates/verter_session/src   # per §1 table
# e.g. the intrinsic cluster:
git -C $REPO show 9111d0a04 -- crates/verter_session/src/host_manage/intrinsic_projection.rs
git -C $REPO show bb71c2815 afc2ff072 -- crates/verter_session/src/resolver_core/component_meta_query_engine/intrinsic_surface.rs

# Fn-fate queries:
git -C $REPO log --oneline -S '<fn_name>' a665d4851..15fb50232 -- crates/verter_session/src
git -C $REPO grep -n "fn <fn_name>" 15fb50232 -- crates/verter_session/src

# Known-closed instance #1:
git -C $REPO show d9adfe289 -- $GUARD | grep -B4 -A2 index_key_to_type_expr
git -C $REPO grep -rn "fn .*index_key_to_type_expr" a665d4851 15fb50232 -- crates  # both empty

# Unlanded first-pass lineage (passthrough §5):
git -C $REPO merge-base 9d15889c7 15fb50232        # b937e0536
git -C $REPO merge-base --is-ancestor 9d15889c7 15fb50232; echo $?   # 1 (not landed)

# The fence + rails themselves (run in a worktree with the branch checked out):
cargo test -p verter_session --test main -- \
  cases::output_projector_residual_guards::hot_path_never_calls_materialize_type_expr \
  cases::output_projector_residual_guards::hot_terminal_allowlist_entries_are_pure_one_shot_sinks \
  --nocapture
```

Verified at check-in: the spine command returns 23 commits; the `5ccc57272` /
`15fb50232` allowlist extracts and the `9111d0a04` exit-diff show resolve; the
unlanded-lineage checks reproduce (`merge-base` = `b937e0536`, `--is-ancestor` false).
Note the unlanded `9d15889c7` lineage commands require an object store that still
carries that unlanded lineage (true of the originating repo; a fresh shallow clone of
the branch will not have it).
