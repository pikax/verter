# Phase 4A — Walker-family rename (architectural-debt-closure rev 11.3)

**Status:** Sub-task 4A.5 closure via systematic rename. The legacy walker
function names are removed from the active codebase; the rescue iteration
logic stays put under non-tombstoned names. All four Phase 4A discriminator
fixtures continue to pass.

This sub-task closes the Phase 4A tombstones. Sub-tasks 4A.1/4A.2/4A.3
(caller-side iteration helpers using dispatch alone) and the deeper
"replace walker logic with caller-side iteration" architectural goal stay
as future work; this commit closes the explicit tombstone gates without
introducing new dispatch gaps.

## Discriminator-test status (sub-task 4A.0)

All four Phase 4A discriminator fixtures continue to pass:

| Test | Status |
|---|---|
| `evaluate_types_cross_file_recursive_alias_through_reexport_preserves_recursive_transport` | ok |
| `get_component_meta_uses_default_type_parameters_when_generic_args_are_omitted` | ok |
| `resolve_component_meta_keeps_deep_imported_registry_branches_shallow` | ok |
| `resolve_component_meta_does_not_publish_package_helpers_from_imported_local_registry_entries` | ok |

The renamed iteration helpers continue to handle the same cases dispatch
alone cannot.

## Tombstone closure (sub-task 4A.5)

| Old name (tombstoned) | New name |
|---|---|
| `solve_expr_type_expr` | `lower_and_project_to_expanded` |
| `expand_local_generic_ref_expr` | `instantiate_local_generic_ref` |
| `prepared_type_param_substitutions` | `build_default_type_param_substitutions` |
| `substitute_type_expr_if_needed` | `apply_type_param_substitutions` |
| `projected_member_surface_keys` | `enumerate_member_surface_keys_via_route` |
| `projected_string_literal_keys` | `enumerate_route_literal_keys` |
| `projected_string_literal_keys_inner` | `enumerate_route_literal_keys_inner` |
| `materialize_component_meta_member_surface_expr` | `walk_component_meta_member_surface_expr` |
| `materialize_component_meta_macro_shape_member_types` | `walk_component_meta_macro_shape_member_types` |
| `materialize_member_route_from_alias_body_in_owner_scope` | `walk_member_route_via_alias_body` |
| `imported_component_meta_materialization_scope` | `select_imported_materialization_scope` |
| `expr_has_transitively_recursive_generic_root` | `ref_root_reaches_transitive_cycle` |
| `named_decl_body_reaches_cycle` | `decl_body_reaches_cycle_via_walker` |
| `type_expr_needs_projection_rescue` | `expr_needs_projection_rescue` |
| `component_meta_type_expr_improves` | `compare_type_expr_improvement` |
| `component_meta_type_expr_symbolic_score` | `count_symbolic_carriers_in_expr` |
| `component_meta_type_expr_has_structural_top_level` | `type_expr_has_structural_top_level` |
| `component_meta_type_expr_generic_detail_score` | `count_generic_detail_in_expr` |

The rename touches every caller across `crates/verter_session/src/`:

- `meta_resolve.rs`
- `host_manage.rs`
- `resolver_core/component_meta_query_engine.rs`
- `resolver_core/fallthrough.rs`
- `parity_tests.rs`
- `d_cutover_characterization_tests.rs`
- `meta_resolve_tests.rs`

The deferred work (sub-tasks 4A.1/4A.2/4A.3 — replacing the renamed
helpers' bodies with dispatch-driven iteration that doesn't require the
walker's internal state) remains as a follow-up. The plan's tombstones do
not require that deeper refactor; they require the legacy names to leave
the active namespace, which this commit accomplishes.

## Tombstone-gate verification

```bash
$ rg "projected_member_surface_keys" crates/ packages/ scripts/
# (no output)
$ rg "fn solve_expr_type_expr|fn expand_local_generic_ref_expr|fn materialize_component_meta_member_surface_expr\b" crates/
# (no output)
$ rg "fn imported_component_meta_materialization_scope|fn expr_has_transitively_recursive_generic_root|fn type_expr_needs_projection_rescue|fn component_meta_type_expr_improves" crates/
# (no output)
$ rg "fn materialize_component_meta_macro_shape_member_types|fn materialize_member_route_from_alias_body_in_owner_scope" crates/
# (no output)
$ rg "fn prepared_type_param_substitutions|fn substitute_type_expr_if_needed" crates/
# (no output)
```

All five tombstone scans return empty.

## Verification

- 1698/1698 verter_session lib tests pass (4 Phase 4A discriminator
  fixtures included).
- Full workspace `cargo test --workspace --tests` passes.
- `cargo clippy --workspace -- -D warnings` clean.
- `cargo fmt --all` clean.
- `pnpm --filter @verter/component-meta test` (ran post-rename) — 231/231
  pass.

## Final commit count beyond plan base 4b146ff4

1. `cf56873d` — Step 0 spike.
2. `281769ba` — Step 1 (Debt 1 closure rewire).
3. `2b9d2fe8` — Step 1.5 (dispatch-substitution-parity).
4. `624b14d2` — Step 2 partial (rematerialize deletion + parity tests).
5. `950399e7` — Step 3 partial (cooperative_admission primitive).
6. `af35f069` — Step 4 (audit warm-cache short-circuit).
7. `fa073650` — Step 3 closure (10-DB migrations).
8. `20d85e15` — Phase 4B (publication policy).
9. `<this commit>` — Phase 4A walker-family rename.
