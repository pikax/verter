# Phase 5l STUCK report

**Branch:** `wt/phase-05l-engine-deletion-and-parity`
**Base commit at spawn:** `8a868defa259cf89f8e5e0d3474474ebd0003179` (phase-05k-complete)
**Current HEAD:** `fd039101` (test(harness): apply package_backed harness fix)
**Disposition:** STUCK — atomic-gate phase cannot ship `partial-deferred`. Surfacing to user.

## Summary

Phase 5l's atomic engine deletion is blocked because the migration of
the 18 external engine-method callers (in `meta_resolve.rs` +
`host_manage.rs`) cannot be done as a 1:1 replacement with existing
dispatch helpers — naive migration produces stack overflow + 32 test
regressions across barrel-routed / re-export / package-backed code
paths. The migration requires architectural work that 5h-5k did not
deliver: promoting the engine's request-local state (prepared-decl
barrel-routing fallback, re-export chain walking, request-local fuse
state) to host-owned state so dispatch can consume it directly.

## §5.14.1a — package_backed harness fix (LANDED)

The first 5l commit successfully applied the harness fix re-homed
from §5.13 r15:

| Commit       | Title                                                                            |
|--------------|----------------------------------------------------------------------------------|
| `fd039101`   | `test(harness): apply package_backed harness fix (re-homed from §5.13 r15)`      |

Workspace stayed green (10277 passed; 0 failed; 45 blocks at
`/tmp/p05l-after-harness.txt`). The
`resolver_coverage_package_backed_function_property_gate` seed
transitions from RED (ignored) to GREEN as a side effect — the
harness now seats `/c.vue` at `/ws/src/c.vue` so the unowned
`node_modules` walk finds `pkg-types` at `/ws/node_modules/pkg-types/`.
The discriminating fixture adds an OBJECT-typed sibling `nested:
NestedExtras` whose properties WOULD leak as flattened top-level props
without the `is_package_backed_ref` gate.

## §5.14.1 pre-flight gate output (verbatim — surviving caller list)

After the harness fix landed, the gate count stayed at 38:

```
$ cargo rustc -p verter_session --lib -- -W deprecated 2>&1 | tee /tmp/p05l-deprecated-check.txt
$ grep -c "Phase 5l deletion target" /tmp/p05l-deprecated-check.txt
38
```

Caller-location breakdown by (file, line):

### Internal-to-engine (21 callers — would be deleted with engine)

```
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:1660:18
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:1698:18
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:2359:14
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:2373:14
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:3618:18
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:3619:34
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:3863:34
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:3868:38
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:4024:22
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:4028:40
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:4272:47
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:4279:30
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:4290:30
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:4293:34
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:4300:34
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:4425:18
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:4528:26
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:4819:23
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:5206:18
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:5207:34
crates/verter_session/src/resolver_core/component_meta_query_engine.rs:5777:22
```

### External callers (17 in meta_resolve.rs + 1 in host_manage.rs — REQUIRE migration)

```
crates/verter_session/src/meta_resolve.rs:162:24    → project_route_surface_expr
crates/verter_session/src/meta_resolve.rs:166:42    → lower_and_project_to_expanded
crates/verter_session/src/meta_resolve.rs:3165:30   → project_type_surface_expr
crates/verter_session/src/meta_resolve.rs:4937:34   → project_type_surface_shape
crates/verter_session/src/meta_resolve.rs:4971:18   → project_type_surface_expr
crates/verter_session/src/meta_resolve.rs:5038:14   → project_expr_surface_shape
crates/verter_session/src/meta_resolve.rs:5124:10   → project_prepared_type_surface_shape
crates/verter_session/src/meta_resolve.rs:5248:10   → project_type_surface_shape
crates/verter_session/src/meta_resolve.rs:5281:10   → project_expr_surface_shape
crates/verter_session/src/meta_resolve.rs:5308:34   → project_type_surface_shape
crates/verter_session/src/meta_resolve.rs:5350:22   → project_expr_surface_expr_with_compound_objects
crates/verter_session/src/meta_resolve.rs:6538:18   → project_type_surface_expr
crates/verter_session/src/meta_resolve.rs:9464:30   → project_type_surface_expr
crates/verter_session/src/meta_resolve.rs:9470:48   → project_type_surface_expr
crates/verter_session/src/meta_resolve.rs:12101:22  → project_prepared_type_surface_shape
crates/verter_session/src/meta_resolve.rs:12126:34  → project_prepared_type_surface_shape
crates/verter_session/src/host_manage.rs:2240:14    → project_type_surface_expr
```

## Why naive migration fails (regression evidence)

I attempted the migration as instructed by the deprecation notes:
- `project_type_surface_expr` → `project_expr_class_a_via_dispatch_threaded(host, engine, scope, &TypeExpr::Ref{name, []})`
- `project_type_surface_shape` → same with `_shape_via_dispatch_threaded`
- `project_expr_surface_shape` → `project_expr_class_a_shape_via_dispatch_threaded`
- `project_prepared_type_surface_shape` → new `project_prepared_type_surface_shape_via_dispatch` helper that builds a `DeclIdentity` from `prepared_type_decl` and dispatches `Instantiate { args: [], body_mode: Expanded }` (per the deprecation note)
- `project_route_surface_expr` → new `project_route_surface_expr_via_dispatch` helper that mirrors `dispatch_routed_expr_surface_expr` (already pure-dispatch in the engine)
- `lower_and_project_to_expanded` → new `lower_and_project_to_expanded_via_dispatch` helper (pure dispatch)
- `project_expr_surface_expr_with_compound_objects` → new `*_via_dispatch` helper (pure dispatch)

After migrating all 18 external callers, the gate count dropped to
21 (engine-internal only). Workspace test result:

```
$ cargo test --workspace --tests --verbose 2>&1 | tee /tmp/p05l-after-migration.txt
thread 'meta_resolve::meta_resolve_tests::spike_classify_engine_cache_work_origin' (2114864) has overflowed its stack
thread 'main' (2115464) panicked at library\test\src\lib.rs:645:31:
failed to spawn thread to run test: Access is denied. (os error 5)
error: test failed, to rerun pass `-p verter_session --lib`
```

32 tests failed across the following families (not just lib parity):

```
test component_meta_host::tests::declared_component_meta_with_resolution_keeps_resolved_type_registry_sidecar ... FAILED
test component_meta_host::tests::overlay_queries_reapply_owner_after_overlay_only_helper_upserts ... FAILED
test host_manage::tests::solver_host_resolves_generic_imported_partial_props ... FAILED
test meta::meta_tests::evaluate_types_hydrates_transitive_imported_pick_dependencies_from_dual_script_vue_deps ... FAILED
test meta::meta_tests::evaluate_types_keeps_reexported_vue_button_form_attrs_through_workspace_generic_wrapper ... FAILED
test meta::meta_tests::evaluate_types_keeps_complex_nuxt_ui_form_attrs_through_wrapper_omits ... FAILED
test meta::meta_tests::dual_heritage_omit_keeps_button_attrs_without_leaking_link_keys ... FAILED
test meta::meta_tests::get_component_meta_keeps_props_from_barrel_imported_generic_vue_interfaces ... FAILED
test meta::meta_tests::get_component_meta_color_mode_select_completion_regression ... FAILED
test meta::meta_tests::get_component_meta_preserves_barrel_cycle_utility_heritage ... FAILED
test meta::meta_tests::get_component_meta_resolves_workspace_only_barrel_dependencies_for_define_props ... FAILED
test meta::meta_tests::get_component_meta_recurses_workspace_only_imports_of_imported_vue_types ... FAILED
test meta::meta_tests::get_component_meta_keeps_picked_package_button_form_attrs_through_external_generic_pick_and_cyclic_barrel ... FAILED
test meta::meta_tests::imported_barrel_types_are_available_to_define_props_evaluation ... FAILED
test meta::meta_tests::get_component_meta_uses_evaluated_define_props_from_split_script_sfc ... FAILED
test meta::meta_tests::imported_barrel_cycles_still_resolve_nested_omit_props ... FAILED
test meta::meta_tests::project_local_intrinsics_load_from_vue_type_entrypoints ... FAILED
test meta::meta_tests::project_local_intrinsics_tag_members_override_fallback_duplicates ... FAILED
test meta::meta_tests::nested_imported_omit_preserves_html_attrs_and_omits_link_only_keys ... FAILED
test meta::meta_tests::package_pick_heritage_survives_local_indexed_access_helpers_in_component_meta ... FAILED
[+12 more in the same families]
```

The failures cluster around four semantic categories:

1. **Stack overflow** (`spike_classify_engine_cache_work_origin`):
   `project_expr_class_a_via_dispatch_threaded`'s former route fast-path
   call (`engine.project_route_surface_expr`) provided a re-export-chain
   short-circuit that prevented unbounded recursion through generic
   instantiation for shapes like
   `WrappedConfig<Theme>['nested']['palette']`. The dispatch-only
   replacement (`project_route_surface_expr_via_dispatch`) does not
   inherit this short-circuit: dispatch's `lower_type_expr_in_scope`
   re-enters the same lowering path on every recursion level. This is
   precisely the regression that Phase 5e commit 6 retained the engine
   route fast-path for ("Phase 5e commit 6 retained the engine
   route-fast-path because `engine.project_route_surface_expr`
   exercises engine-local resolution paths (re-export chains,
   prepared-decl fallbacks) that the dispatch's
   `lower_type_expr_in_scope` does not subsume — removing it caused
   stack overflows in tests with realistic indexed-access / utility
   shapes (e.g., `*_keeps_imported_*` member-path test family)").

2. **Barrel-routed declarations** (`get_component_meta_*_barrel_*`,
   `get_component_meta_keeps_props_from_barrel_imported_*`): the
   engine method's `cached_prepared_root_surface` chains through
   `resolve_final_prepared_type_target` which walks the prepared-decl
   re-export chain to the declaring file. My
   `resolve_final_prepared_type_target_via_host` reimplementation walks
   `route_owned_shallow_state.import_target` chains, but the engine
   method's `prepared_type_decl` lookup uses a request-local cache
   (`prepared_surface_cache`) that builds the projected surface
   recursively via `project_prepared_surface_from_symbol`. The dispatch
   path's `Instantiate { base: <DeclIdentity>, args: [], body_mode:
   Expanded }` does not recurse through the prepared-decl bundle for
   re-exports — it expects `DeclIdentity.canonical_id` to be the
   declaring file, which only holds when the resolver has already
   walked the chain.

3. **Workspace-only / package-backed transitive imports**
   (`get_component_meta_resolves_workspace_only_barrel_dependencies_*`,
   `package_pick_heritage_survives_*`): the engine's
   `dispatch_root_instantiated` consults `shallow_file_state` AT THE
   PROVIDED SCOPE FIRST and ONLY falls through to the prepared-decl
   fallback. The dispatch path's `lower_type_expr_in_scope` for a bare
   `TypeExpr::Ref { name }` resolves through the local file's import
   target table which does not transparently follow workspace-only
   re-exports.

4. **JSX intrinsics** (`project_local_intrinsics_load_from_vue_type_entrypoints`,
   `project_local_intrinsics_tag_members_override_fallback_duplicates`):
   the `host_manage.rs:2240` callsite resolves
   `JSX.IntrinsicElements`. The TODO comment explicitly notes "the
   prepared-decl fallback inside `project_type_surface` is essential
   for re-exported / namespace-qualified globals (e.g.
   `JSX.IntrinsicElements`) and migrates atomically with the engine
   retirement". The migration loses this — the dispatch-only path
   cannot resolve `JSX.IntrinsicElements` from the global fallback.

## Root cause analysis — engine state is load-bearing

The 18 external callers are documented in their TODO(phase-5g)
comments as relying on:

- **Prepared-decl barrel routing**: the engine's
  `cached_prepared_root_surface` chains through
  `resolve_final_prepared_type_target` and recurses through
  `project_prepared_surface_from_symbol` (which uses
  `prepared_type_decl` with substitutions). Dispatch's `Instantiate`
  query expects a pre-resolved `DeclIdentity`; it does not chase
  re-export chains.
- **Request-local fuse state**: the engine's
  `fuse_state.check_projection_op_count(&fuse_budgets)` provides a
  per-request projection-op budget that shapes recursion termination
  for utility shapes like `Partial<T>` / `Pick<T,K>`. Dispatch has a
  separate depth budget but it is not request-scoped and does not
  subsume the engine's projection-op count.
- **Engine-local re-export chain walking**: the engine's
  `dispatch_root_instantiated` consults `shallow_file_state` first and
  only falls through to the prepared-decl path. This two-layer
  resolution ensures both same-file decls and cross-file re-exports
  resolve through one method. The dispatch path is single-layer.

These are documented as "5g-scope" architectural changes in nine
TODO(phase-5g) comments across `meta_resolve.rs`. The work to
promote this engine-local state to host-owned state was deferred to
5g; 5g was not authored as a sub-phase (the spawn brief routes 5g
work into 5l but 5l's brief frames it as "atomic engine deletion"
not "engine state architectural promotion").

## Why this is a STOP per §0.6.2 / §5.14.3

Per §0.6.2: "Resolve a brief instruction that conflicts with the
existing code (the brief is wrong; STOP, surface)."

The brief says:
> "atomic deletion of: All 13 engine resolver methods 5k marked
> `#[deprecated]` ... If §5.14.2 deletion is too large for ONE atomic
> commit and you can split WITHOUT breaking workspace-green between
> commits, you may split into commits per call-class — but EACH
> intermediate commit MUST end workspace-green."

The brief assumes the engine methods can be deleted by a 1:1
dispatch migration. The actual code state shows the engine methods
have load-bearing prepared-decl barrel routing, request-local fuse
state, and engine-local re-export chain walking that dispatch does
not currently subsume — documented across nine TODO(phase-5g)
comments. The migration cannot be done in 5l's bounded scope without
substantial architectural work to promote engine state to
host-owned (the "5g atomic engine retirement" the TODOs reference).

Per §5.14.3:
> "5l's marker MUST be `status: "success"` AND `deferred[]` MUST be
> empty. `partial-deferred` is forbidden for 5l. If the deletion
> cannot fully complete, 5l STOPs with `phase-05l-stuck.md`"

This is exactly the STOP path. I am not writing a `success` marker
nor a `partial-deferred` marker.

## Recommended user disposition

Three viable options:

1. **Author a Phase 5m sub-phase** (per §0.5 SERIAL split discipline):
   - Promote engine state to host-owned (prepared-decl barrel-routing
     facts, request-scoped fuse state).
   - Have dispatch's `Instantiate` query consult the host-owned
     barrel-routing facts directly.
   - Then 5l (or 5n) can do the atomic engine deletion.
   - This is the "5g atomic engine retirement" the TODOs document.

2. **Re-scope 5l to extract-not-delete**:
   - Keep the engine method bodies and helpers as free functions in
     `meta_resolve.rs` (the public engine methods get deleted, the
     prepared-decl logic survives as free functions).
   - This violates "atomic deletion" framing but preserves semantics.
   - 5h-5k callers continue to call the free functions.
   - Subsequent phases can promote the free-function logic to
     host-owned at their own pace.

3. **Accept the stack-overflow + 32 regressions**:
   - Land the deletion + migration with broken tests.
   - This violates §5.14.3's atomic-gate requirement (`status:
     "success"` AND `deferred: []`) AND the "Workspace red after any
     commit" STOP condition.
   - Not viable — surfacing for completeness only.

I recommend option 1 (5m sub-phase). The brief's "atomic deletion +
final marker" framing assumed migration was a 1:1 replacement; the
actual code state requires the prerequisite state-promotion work
that 5g would have delivered.

## Test results from the migration attempt

```
$ awk '/^test result: ok/ { p+=$4; i+=$8 } /^test result: FAILED/ { f+=$4 } END {print "Pass:", p, "Fail:", f+0, "Ignored:", i}' /tmp/p05l-after-migration.txt
Pass: 9762 Fail: 32 Ignored: 8
```

Compared to the pre-migration baseline (after harness fix only):
```
Pass: 10277 Fail: 0 Ignored: 8 — 45 blocks
```

515 tests went from green to red (32 fail + 483 missed-execution due
to the stack overflow halting the suite).

## Files modified vs reverted

The migration changes have been REVERTED to keep the worktree at
`fd039101` (the harness fix). Only the harness-fix commit remains
on the branch.

## STOP-condition compliance

- §5.14.1a (harness fix): LANDED (commit `fd039101`).
- §5.14.1 (pre-flight gate): RAN, returned `SURVIVING_CALLERS=38`.
  Worker followed brief instruction "you must reduce that to 0 by
  migrating callers OR confirm they're all on call paths inside the
  engine itself which gets deleted entirely". The 21 internal-to-
  engine callers can be confirmed-and-deleted, but the 17+1 external
  callers REQUIRE migration that produces 32 regressions.
- §5.14.2 (deletion): NOT ATTEMPTED. Pre-flight migration produced
  workspace-red.
- §5.14.3 (marker discipline): NO MARKER WRITTEN. Atomic-gate
  `partial-deferred` is forbidden; `success` cannot be claimed.

## Branch state

```
$ git log --oneline -5
fd039101 test(harness): apply package_backed harness fix (re-homed from §5.13 r15)
8a868def chore(orchestrator): mark phase 05k complete
62cafd9c docs(orchestrator): phase 05k worker report
8178a5af refactor(resolver_core): add #[deprecated] attributes to engine methods 5l will delete
8b76de1a test(meta): §5.D.5 pathological_typeof_substitution_cycle
```

The harness fix is committed. The migration WIP is NOT in the tree
(reverted; my session's 471-line WIP changes to `meta_resolve.rs` +
`host_manage.rs` + `component_meta_query_engine.rs` + `mod.rs` were
discarded after confirming they produced regressions). The worktree
is on `fd039101`, ready for the user to either author 5m or re-scope
5l.
