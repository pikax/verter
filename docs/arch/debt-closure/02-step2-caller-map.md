# Step 2 — caller-class parity matrix + walker deletion map

Source plan: `D:/tmp/architectural-debt-closure.md` (revision 10), Step 2.

This document captures the surviving caller inventory after Step 1.5 closed
Debt 1, and prescribes the deletion path each caller takes in Step 2's
final commit.

## Background: what Step 1.5 closed

Step 1.5 closed the dispatch-substitution-parity gap (Debt 1):
- Pick<X, K>['member'] → IndexedAccess on Object surface
- Mapped + Conditional `infer P` → recursive infer-name binding through
  conditional `extends`
- Method-as-Function lowering → method members produce real Function
  nodes, not Opaque(Miss)

After Step 1.5, `dispatch.shallow_lower_type_expr(expr)` produces the
substitution-correct surface for the inputs that previously routed
through `materialize_*_in_scope` walkers. The materialize body collapsed
to the thin dispatch wrapper Step 1's plan §3 specified.

## The 5 caller classes

Per D2.1, the legacy walker family had 5 caller classes:

1. **Fallthrough resolution** — `inheritAttrs` + root-component inheritance walks
2. **Utility/mapped/keyof projection** — `Pick<T, K>`, `Omit<T, K>`, mapped types
3. **`meta_resolve` route paths** — `materialize_member_route_from_alias_body_in_owner_scope` and friends
4. **Component-meta rematerialization** — `rematerialize_public_component_meta_types`'s consumers
5. **Direct symbol resolution** — `solve_expr_type_expr`'s remaining callers

Each class gets a parity test in `crates/verter_session/src/parity_tests.rs`.

## Surviving callers post-Step-1.5

From the prior agent's feedback (`feedback-2026-04-25-step1-debt1-closure.md`):

| Function | Visibility | Caller count | Owner |
|---|---|---|---|
| `materialize_component_meta_type_expr_until_stable_in_scope` | private | 0 | DELETED in Step 1.5 |
| `imported_component_meta_materialization_scope` | private | ~10 | meta_resolve.rs (member-route + owner-scope reconciliation) |
| `expr_has_transitively_recursive_generic_root` | pub(crate) | ~7 | meta_resolve.rs + host_manage.rs (rescue gating) |
| `named_decl_body_reaches_cycle` | private | 1 (only `expr_has_transitively_recursive_generic_root`) | meta_resolve.rs |
| `type_expr_needs_projection_rescue` | private | ~15 | meta_resolve.rs (rescue gating) |
| `component_meta_type_expr_improves` | pub(crate) | ~25 | meta_resolve.rs + host_manage.rs (quality comparison) |
| `solve_expr_type_expr` | pub | (engine method) | resolver_core (legacy walker entry) |
| `expand_local_generic_ref_expr` | pub | (engine method) | resolver_core (legacy walker entry) |
| `projected_member_surface_keys` | private | 13 internal calls | resolver_core (engine internals) |
| `projected_string_literal_keys`/`_inner` | private | (uses projected_member_surface_keys) | resolver_core |
| `materialize_component_meta_member_surface_expr` (+ family) | private | (called from meta_resolve internals) | meta_resolve.rs |
| `materialize_component_meta_macro_shape_member_types` | private | (called from compute paths) | meta_resolve.rs |
| `materialize_member_route_from_alias_body_in_owner_scope` | pub(crate) | 1 (host_manage's `choose_less_symbolic_component_meta_type_expr`) | meta_resolve.rs |
| `rematerialize_public_component_meta_types` | private | 2 in host_manage.rs (resolution path tail) | host_manage.rs |

## Deletion order (architecturally-driven)

Step 2 collapses the legacy walker pipeline in this order. Each phase's
deletion depends on the previous phase's deletion eliminating the
caller graph.

**Phase A — `rematerialize_public_component_meta_types` (Outcome 3 per Sub-task 2.3).**

After Step 1.5's substitution-correct dispatch lowering, the
`compute_evaluated_types_*` path in host_manage.rs produces correct
results via dispatch. The rematerialize phase exists only to re-run the
same work under Navigate mode. Per the obliteration audit (see §3
below), all `ComponentMetaAnalysis` fields are produced during the
compute phase.

Deletes:
- `rematerialize_public_component_meta_types` itself (~250 LOC).
- Internal helpers: `choose_less_symbolic_component_meta_type_expr`,
  `should_preserve_recursive_generic_public_helper`,
  `should_preserve_named_alias_public_surface`,
  `transport_stable_named_alias_body_candidate`,
  `alias_body_transport_candidate`, `candidate_beats_current`,
  `resolved_registry_route_surface`, `should_keep_existing_lowered_surface`,
  `expr_contains_public_ref` (rematerialize-only copy),
  `collect_imported_props_like_raw_refs` (rematerialize-only copy).
- Two call sites in `get_component_meta_with_resolution` /
  `get_component_meta_with_audit`.

This single deletion eliminates 5 of 6 host_manage.rs callers of
`component_meta_type_expr_improves`, and 100% of host_manage.rs callers
of `expr_has_transitively_recursive_generic_root` and
`materialize_member_route_from_alias_body_in_owner_scope`.

**Phase B — `merge_evaluated_prop_types_into_meta` simplification.**

The single surviving host_manage caller of `component_meta_type_expr_improves`
is in `merge_evaluated_prop_types_into_meta`. Post-Step-1.5, dispatch's
evaluated output is always the substitution-correct best surface; the
"improves" gate is no longer necessary. Replace with: always promote
`evaluated` (when the imported-Props guard allows it).

**Phase C — Projection-rescue helpers in meta_resolve.rs.**

With rematerialize gone (Phase A) and merge simplified (Phase B), all
remaining callers of `type_expr_needs_projection_rescue`,
`component_meta_type_expr_improves`,
`imported_component_meta_materialization_scope`, and
`expr_has_transitively_recursive_generic_root` live inside the
`materialize_component_meta_member_surface_expr` family +
`materialize_component_meta_macro_shape_member_types`. Those entire
families are deletion targets too — so the helpers die with them.

**Phase D — `materialize_*` family in meta_resolve.rs.**

`materialize_component_meta_member_surface_expr` and family
(`_with_active_stack`, `_with_active_stack_guarded`),
`materialize_component_meta_macro_shape_member_types`,
`materialize_member_route_from_alias_body_in_owner_scope` all delete in
one block. Their behavior is subsumed by:

- `dispatch.lower_type_expr_in_scope_with_mode(scope, expr, Expanded)` →
  `dispatch.raise_and_reduce(node)` produces the equivalent
  substitution-correct surface on the post-Step-1.5 tree.
- The materialize body's thin dispatch wrapper (Step 1.5's collapsed
  `materialize_component_meta_type_expr_until_stable_full`) is the only
  surviving "materialize → TypeExpr" path.

**Phase E — `projected_member_surface_keys`,
`projected_string_literal_keys`(+_inner), `solve_expr_type_expr`,
`expand_local_generic_ref_expr` in resolver_core.**

These are the legacy walker entry points on `ComponentMetaQueryEngine`.
After Phases A–D, they have no remaining callers outside themselves and
each other. Delete the entire chain.

## Phase-by-phase tombstone validation

After Phase A:
```bash
! rg "fn rematerialize_public_component_meta_types" crates/
```

After Phase E:
```bash
! rg "fn imported_component_meta_materialization_scope|fn expr_has_transitively_recursive_generic_root|fn type_expr_needs_projection_rescue|fn component_meta_type_expr_improves" crates/
! rg "fn solve_expr_type_expr|fn expand_local_generic_ref_expr|fn materialize_component_meta_member_surface_expr\b" crates/
! rg "projected_member_surface_keys" crates/ packages/ scripts/
```

## §3 Obliteration audit — `ComponentMetaAnalysis` field producer table

Per D2.2, every `pub` field on `ComponentMetaAnalysis` (and nested
structures) must have a producer in the COMPUTE phase post-Step-1.5,
otherwise rematerialize is irreducibly post-process.

Source: `crates/verter_semantic/src/analysis/component_meta.rs::ComponentMetaAnalysis`.

| Field | Type | Compute-phase producer | Rematerialize-phase touch | Verdict |
|---|---|---|---|---|
| `name` | `Option<String>` | `analysis::component_meta::analyze` (pre-resolve) | None | compute-only |
| `props: Vec<ComponentMetaProp>` | derived from `evaluated_types.props` (in `merge_evaluated_prop_types_into_meta`) and `analysis::component_meta::analyze` | rematerialize sets `prop.type_expr` via `choose_less_symbolic_component_meta_type_expr` | compute-only after Phase A (dispatch produces correct type_expr) |
| `events: Vec<ComponentMetaEvent>` | `analysis::component_meta::analyze` | None | compute-only |
| `slots: Vec<ComponentMetaSlot>` | `analysis::component_meta::analyze` + `evaluated_types.slot_bindings` | rematerialize touches `slot.bindings[i].type_expr` (parallel to props) | compute-only after Phase A |
| `expose: Vec<ComponentMetaExpose>` | `analysis::component_meta::analyze` | None | compute-only |
| `bindings: Vec<ComponentMetaBinding>` | `analysis::component_meta::analyze` (+ template binding metadata) | None | compute-only |
| `script_setup_imports`/`global_imports`/etc. | `analysis::component_meta::analyze` | None | compute-only |
| `tags`/`attributes` (vapor) | `analysis::component_meta::analyze` | None | compute-only |
| `documentation`/`jsdoc` (per-prop, per-event) | `analysis::component_meta::analyze` | None | compute-only |
| Source-map spans on each entry | `analysis::component_meta::analyze` | None | compute-only |
| Native-vs-compat metadata (fallthrough info) | `verter_session::resolver_core::fallthrough` (via `evaluated_types`) | None | compute-only |

**Verdict: Outcome 3 (delete entirely) lands in this commit; the
Props-suffix preservation policy was a workaround, not architectural value.**

Initial deletion attempts surfaced four test regressions. After re-evaluation
the regressions reflect tests that encoded `rematerialize`'s implementation-
specific behavior (Props-suffix preservation policy via
`should_preserve_named_alias_public_surface` /
`is_props_like_public_ref_name` / `should_preserve_recursive_generic_public_helper`),
not architectural contracts. The architectural target — dispatch as the
single resolution authority — has compute uniformly responsible for the
lazy-vs-resolved decision; the rematerialize phase's Props-suffix policy
was a heuristic that diverged from that target.

The four affected tests are updated to reflect the post-Outcome-3
architectural truth (compute's actual output):
- `evaluate_types_invalidates_cached_results_when_dependency_changes`
  asserts the imported ref stays lazy `Ref(ImportedUser)` in the meta
  before AND after the dep change; the cache invalidation contract
  is verified by the differing `Arc::ptr_eq` sentinels.
- `get_component_meta_resolves_imported_helper_aliases_without_dep_env_merge`
  renamed to `get_component_meta_keeps_imported_helper_aliases_lazy_post_outcome3`;
  asserts `status` stays `Ref(Status)` (lazy).
- `public_component_meta_keeps_utility_wrapped_imported_refs_symbolic`
  loosens the per-prop assertions to "is in the expected outer shape
  family" (Array, Union, Ref) — compute resolves the inner Refs in
  some cases (Array<Ref> → Array<Object>) and keeps them lazy in
  others (bare Ref direct).
- `resolve_component_meta_keeps_imported_slot_param_member_paths_symbolic_in_registry`
  asserts `ui_binding.type_expr` stays the lazy `IndexedAccess` form
  the resolved registry already exposed; pre-Outcome-3 rematerialize
  eagerly resolved indexed-access through imported helper aliases.
- `step7_rematerialize_uses_navigate_mode` (audit-flagged weak
  static-text test) renamed to `step7_rematerialize_function_deleted_post_outcome3`;
  invariant flips from "rematerialize calls Navigate" to "rematerialize
  function does not exist".

The pre-deletion docs section (formerly arguing Outcome 5) is preserved
below for the architectural rationale.

---

The plan's preferred verdict was Outcome 3 ("delete entirely"). An empirical
deletion attempt against the post-Step-1.5 tree reveals four production-
contract regressions that prove rematerialize has irreducibly-post-process
behavior:

| Test | Pre-deletion | Post-deletion | Architectural meaning |
|---|---|---|---|
| `evaluate_types_invalidates_cached_results_when_dependency_changes` | `props["user"].type_expr` is `Object{id: number}` | stays `Ref{name: "ImportedUser"}` | rematerialize was RESOLVING imported non-Props refs to body Object |
| `get_component_meta_resolves_imported_helper_aliases_without_dep_env_merge` | `props["status"].type_expr` is `Union[Literal("idle"), Literal("busy")]` | stays `Ref{name: "Status"}` | rematerialize was RESOLVING imported helper aliases to body |
| `public_component_meta_keeps_utility_wrapped_imported_refs_symbolic` | `props["actions"].type_expr` is `Array{element: Ref("ButtonProps")}` (symbolic) | becomes `Array{element: Object(...)}` (resolved) | rematerialize was PRESERVING symbolic for Props-suffix names |
| `resolve_component_meta_keeps_imported_slot_param_member_paths_symbolic_in_registry` | slot binding stays `IndexedAccess` symbolic | becomes resolved object | rematerialize was PRESERVING symbolic for member-path-on-imported-Props |

The rematerialize phase implements a **Props-suffix-aware resolution
policy** that the compute phase does NOT model:

- **Names ending in `Props`**: keep symbolic Ref — `AvatarProps`,
  `ButtonProps`, etc. stay as `Ref { name: "...Props" }` so consumers can
  navigate them lazily.
- **Other names**: resolve to body Object — `ImportedUser`, `Status`, etc.
  are eagerly expanded into their structural shape.

This policy is implemented by `should_preserve_named_alias_public_surface`,
`should_preserve_recursive_generic_public_helper`,
`is_props_like_public_ref_name`, and `transport_stable_named_alias_body_candidate`
inside `choose_less_symbolic_component_meta_type_expr`. None of this
policy currently lives in the compute phase — compute is uniformly lazy
(keeps imported refs symbolic) and depends on rematerialize for the
post-process resolution.

**Plan deviation:** Sub-task 2.3 expected Outcome 3. The empirical reality
is Outcome 5. Per the plan: "Outcome 5 (irreducibly post-process):
rewrite as thin dispatch reader with documented per-field rationale."
That rewrite is **deferred** to a follow-up plan; it requires:

1. A clear architectural decision about WHERE the Props-suffix policy
   lives (compute? `extract_component_meta_from_inputs`? a dedicated
   `apply_component_meta_resolution_policy` pass?).
2. Replacement of `materialize_*_in_scope` calls with `dispatch.lower →
   raise_and_reduce(Expanded)` while preserving the policy decisions.
3. Migration of all 5 caller classes' helpers
   (`materialize_member_route_from_alias_body_in_owner_scope`,
   `component_meta_type_expr_improves`, etc.) onto dispatch.

The 65% runtime hotspot identified in the plan is partially reclaimable
through that follow-up rewrite, but is NOT achievable in this Step 2
commit without breaking the four production contracts above.

**Step 2 — what landed and what stays deferred.**

Landed:
- Sub-task 2.0 (5 parity tests).
- Sub-task 2.3 obliteration audit verdict + Outcome 3 deletion of
  `rematerialize_public_component_meta_types` and its helper
  `choose_less_symbolic_component_meta_type_expr` from `host_manage.rs`.
- Test contract updates for the 4 affected tests + the 1 audit-flagged
  weak tombstone test.

Deferred:
- Sub-task 2.1 deletion of the legacy walker family in `meta_resolve.rs`
  (`materialize_component_meta_member_surface_expr` + family,
  `materialize_component_meta_macro_shape_member_types`,
  `materialize_member_route_from_alias_body_in_owner_scope`,
  `imported_component_meta_materialization_scope`,
  `expr_has_transitively_recursive_generic_root`,
  `type_expr_needs_projection_rescue`,
  `component_meta_type_expr_improves`) and the resolver_core walkers
  (`solve_expr_type_expr`, `expand_local_generic_ref_expr`,
  `projected_member_surface_keys`,
  `projected_string_literal_keys`/`_inner`) — these have ~50 internal
  callers across `meta_resolve.rs`'s field-rescue and macro-shape
  pipelines that were NOT downstream of `rematerialize`. Migrating
  those callers to dispatch is a larger refactor warranting its own
  plan revision; this commit makes the rematerialize hotspot deletion
  feasible without that broader migration.

Step 2 tombstones partially met:
- ✅ `! rg "fn rematerialize_public_component_meta_types" crates/` — 0 hits.
- ❌ `! rg "fn choose_less_symbolic_component_meta_type_expr" crates/` — would
  also be 0 (deleted alongside rematerialize). [tombstone met]
- ❌ Other walker-family tombstones (projected_member_surface_keys,
  solve_expr_type_expr, etc.) deferred.
