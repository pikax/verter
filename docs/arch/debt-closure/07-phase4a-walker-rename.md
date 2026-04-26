# Phase 4A — Walker-family closure (architectural-debt-closure rev 11.3)

**Status:** Sub-tasks 4A.1 / 4A.2 / 4A.3 / 4A.5 closed in two commits
beyond the systematic rename. The legacy walker function names are
removed from the active codebase AND the architectural debt patterns
the rename hid (`MATERIALIZE_DEPTH` thread-local, `FxHashSet<TypeExpr>`
active set, inline manual scope iteration) are gone. All four Phase 4A
discriminator fixtures continue to pass.

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

## Tombstone closure (sub-task 4A.5 — rename pass)

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

## Architectural completion (sub-tasks 4A.1 / 4A.2 / 4A.3 / 4A.5)

Two follow-up commits close the architectural intent the rename alone
did not satisfy.

### `12d5f717` — `crates/verter_session/src/component_meta_dispatch_iteration.rs`

New module with three caller-side helpers + the
`SemanticNodeId`-keyed visited set:

| Helper | Plan reference | Purpose |
|---|---|---|
| `lower_in_first_responsive_scope` | §4A D4A.2 Gap 1 | Iterates `[owner, imported_scope, …]` through `dispatch.lower_type_expr_in_scope_with_mode`; first non-opaque-miss wins. |
| `rewrite_omitted_generic_args_with_defaults` | §4A D4A.2 Gap 2 | Rewrites `Ref { name, type_arguments: [] }` to `Ref { name, type_arguments: [defaults] }` when every prepared type parameter carries a default. |
| `iterate_ref_chain_until_non_ref` | §4A D4A.2 Gap 3 | Caller-side `dispatch.lower → ProjectPath{[], mode} → raise` per hop, guarded by `WalkerVisitedNodes` cycle detector + defensive fuse. |
| `WalkerVisitedNodes` (struct) | §4A D4A.2 Gap 3 | `FxHashSet<SemanticNodeId>` + `fuse_hops` counter (`VISITED_NODES_DEFENSIVE_FUSE = 4096`). Replaces the legacy `FxHashSet<TypeExpr>` active set. |

The module ships with 11 helper-level FAIL-FIRST tests covering visited-set
contracts (insert/cycle/pop/fuse) and the three Gap helpers' end-to-end
behaviour against fixture projects.

### `65f46a8c` — Walker bodies use the new helpers

In `crates/verter_session/src/meta_resolve.rs`:

- `MAX_MATERIALIZE_DEPTH = 48` constant + `MATERIALIZE_DEPTH`
  thread-local **deleted**. The walker no longer carries a hard depth
  cap as ordinary termination — cycle detection on resolved
  `SemanticNodeId`s drives termination, with `WalkerVisitedNodes`'s
  `VISITED_NODES_DEFENSIVE_FUSE = 4096` as the safety rail.
- `walk_component_meta_member_surface_expr_with_active_stack` and
  `_with_active_stack_guarded` collapsed into one function
  `walk_component_meta_member_surface_expr_with_visited`. The two-stage
  split existed solely to wrap depth-fuse manipulation around the body;
  with `MATERIALIZE_DEPTH` retired the wrapper is no longer needed.
- `active: &mut FxHashSet<TypeExpr>` parameter replaced with
  `visited: &mut crate::component_meta_dispatch_iteration::WalkerVisitedNodes`
  — `SemanticNodeId`-keyed instead of `TypeExpr`-keyed.
- New file-level helper `walker_cycle_key_node` lowers an input
  expression through dispatch and returns its `SemanticNodeId` for
  visited-set keying (or `None` when the expression cannot be lowered
  or returns `Opaque(Miss)`).
- New file-level helper `expand_generic_ref_via_scope_iteration`
  encapsulates the `[owner_scope, imported_scope]` retry pattern. The
  per-scope expander remains
  `engine.instantiate_local_generic_ref` (preserved by D-Cutover §5.8
  row 5 characterization at
  `d_cutover_characterization_tests.rs:2493-2508`); the architectural
  cleanup is removing the *manual iteration* from the walker body.
- 30+ recursive call sites updated mechanically to pass
  `&mut visited` and call
  `walk_component_meta_member_surface_expr_with_visited`.
- Each early-return path's `active.remove(expr)` replaced with
  `if let Some(node) = pushed_node { visited.pop(node); }` so cycle
  detection is scoped per call frame (Object members both referencing
  the same `Ref` each see a cleanly popped visited set).

The remaining `FxHashSet<TypeExpr>` use in
`materialize_component_meta_registry_structural_expr::inner`
(`meta_resolve.rs:8830`) is a *separate* helper outside the renamed
walker family and is unchanged. It is not in scope for plan §4A.

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

## Architectural-debt-pattern verification

```bash
# Architectural debt patterns named by handoff "Definition of done":
$ rg "MATERIALIZE_DEPTH|MAX_MATERIALIZE_DEPTH" crates/verter_session/src/meta_resolve.rs
# Only doc-comment references remain (retired patterns are documented).

$ rg "fn .*active.*FxHashSet.*TypeExpr" crates/verter_session/src/meta_resolve.rs
# Only `materialize_component_meta_registry_structural_expr::inner`
# (a separate helper, not in the renamed walker family).

$ rg "fn expand_generic_ref_for_materialization" crates/
# (no output — superseded by `expand_generic_ref_via_scope_iteration`)
```

## Verification

- **1709/1709** verter_session lib tests pass (pre-existing 1698 + 11
  helper tests from `12d5f717`).
- All four Phase 4A discriminator fixtures pass.
- Full workspace `cargo test --workspace --tests` green.
- `cargo clippy --workspace -- -D warnings` clean.
- `cargo fmt --all -- --check` clean.
- `pnpm --filter @verter/component-meta test` 231/231 pass.

## Final commit count beyond plan base `4b146ff4`

1. `cf56873d` — Step 0 spike.
2. `281769ba` — Step 1 (Debt 1 closure rewire).
3. `2b9d2fe8` — Step 1.5 (dispatch-substitution-parity).
4. `624b14d2` — Step 2 partial (rematerialize deletion + parity tests).
5. `950399e7` — Step 3 partial (cooperative_admission primitive).
6. `af35f069` — Step 4 (audit warm-cache short-circuit).
7. `fa073650` — Step 3 closure (10-DB migrations).
8. `20d85e15` — Phase 4B (publication policy).
9. `5aea90b0` — Phase 4A walker-family rename (sub-task 4A.5 names).
10. `e277cf25` — Phase 4A scope assessment + deferral note (superseded).
11. `30200483` — Phase 1-4 audit (closes 5 quality issues).
12. `12d5f717` — Phase 4A.1/4A.2/4A.3 dispatch-iteration helpers.
13. `<this commit's parent>` — Phase 4A architectural completion: walker
    bodies use Gap 1/2/3 helpers; `MATERIALIZE_DEPTH` +
    `FxHashSet<TypeExpr>` + inline scope iteration retired.
