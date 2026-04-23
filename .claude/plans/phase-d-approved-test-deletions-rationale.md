# Phase D — Approved test deletions rationale

The canonical list lives in `.claude/plans/phase-d-approved-test-deletions.txt` (pure data, regenerated at gate time from `scripts/phase-d/generate-approved-test-deletions.sh`). This file explains *why* each category is retired.

## Category 1 — Wholesale file deletions (§4.1 `DELETE_FILES`)

The following files are deleted by the cutover per Change W / Change S / Change D5. Every `#[test]` in each file is retired together with the file's subject code.

- `crates/verter_semantic/src/analysis/type_solver/project.rs`
- `crates/verter_semantic/src/analysis/type_solver/relate.rs`
- `crates/verter_session/src/resolver_core/solver_host.rs`
- `crates/verter_session/src/resolver_core/solver_host_tests.rs`
- `crates/verter_session/src/resolver_core/type_surface_db.rs`
- `crates/verter_session/src/resolver_core/type_surface_tests.rs`
- `crates/verter_session/src/dispatch_bridge.rs`

Rationale: §2 Architectural decision — "Dispatch is the canonical lazy semantic layer. Solver arena demotes to per-request scratch for TypeExpr lowering and Vue macro parsing only." These files implement the retired second authority (solver host/relate/project) and the bridge layer that existed solely to paper over the dual authority. `TypeSurfaceDb` is superseded by `SemanticGraphStore`'s memo. Their tests exercise behaviour that the post-cutover `ProjectSemanticDispatch` + `SemanticGraphStore` assumes directly.

## Category 2 — Retired subject-code references (§4.1 `SYMBOL_SCAN_FILES` × `DELETE_SYMBOLS`)

Tests in the following surviving files are retired if they reference any `DELETE_SYMBOLS` identifier (the tests' subject code is deleted):

- `crates/verter_semantic/src/analysis/type_solver/solve.rs` — walker method tests (`resolve_node`, `resolve_indexed_access`, `resolve_conditional`, `collect_structural_property_descriptors_inner`, `resolve_prepared_ref`, `resolve_type_parameters_in_body`, `resolve_keyspace`, `resolve_member`, `project_type_parameter_refs`).
- `crates/verter_semantic/src/analysis/type_solver/query_engine.rs` — `TypeQueryEngine` struct + all methods deleted.
- `crates/verter_semantic/src/analysis/type_solver/arena.rs` — `Node::Error` variant + `SolverCaches` deletions.
- `crates/verter_semantic/src/analysis/type_solver/host.rs` — `TypeSolverHost` trait + `EvalEnvSolverHost` / `SessionSolverHost` deletions.

Rationale: these files survive (lowering helpers, arena core types), but the tests that pin their retired APIs have no post-cutover counterpart. Callers migrate to `dispatch.execute(SemanticQueryKey::...)` per §9 appendix; those call-paths are validated by §6.7 preservation tests instead.

## Category 3 — Explicit full test IDs (§4.1 `EXPLICIT_TEST_IDS`)

Three `project_semantic_dispatch::tests` tests were retired wholesale (physically deleted from `project_semantic_dispatch/tests.rs` in §5.5 WIP-M) because Change M (§2) rewrites `build_mapped_type` around `KeyEnumeration`, which changes the observed behaviour they characterised. They are not listed in `EXPLICIT_TEST_IDS` — their absence from the post-cutover baseline comes from the source edit, not a retirement explicitly resolved through Pass 3. Pointers:

- `project_semantic_dispatch::tests::mapped_type_value_stays_opaque_when_source_is_not_object` — replaced by §6.2 `d_cutover_characterization_tests::mapped_type_value_substitutes_into_keyspace_even_when_source_is_not_object`. New semantic: symbolic-source mapped types substitute into the keyspace rather than short-circuiting to Opaque.
- `project_semantic_dispatch::tests::mapped_type_inside_non_contributing_intersection_arm_ignored` — retired; contributor-rule moves from `walk_internal` Intersection arm to `KeyEnumeration::Intersection` aggregation (§6.2 `d_cutover_characterization_tests::build_mapped_type_produces_canonical_mapped_shell_on_unresolvable_enumeration` exercises the aggregated path).
- `project_semantic_dispatch::tests::mapped_type_with_as_key_remapping_emits_project_member_with_remap_meta` — replaced by §6.2 `d_cutover_characterization_tests::mapped_type_with_as_clause_symbolic_remapping_defers_whole_shape_preserving_name_remap`. New rule: symbolic `name_remap` defers the whole shape; only literal/`never` remaps project immediately.

Two additional entries were added in §5.8 WIP-W when the retired solver's observability surface went away:

- `verter_session::meta::meta_tests::public_component_meta_keeps_simple_imported_alias_union_surface` — the test asserts that an imported alias `Ref<VNode>` stays symbolic at the union-arm level AND that the `Function` branch `(() => VNode)` lowers fully. The characterisation depended on the retired solver's `RecursionTracker`-based symbolic-preservation heuristic (via `TypeQueryEngine::should_preserve_shallow_field_expr`). §5.8 deletes that solver path; dispatch's `project_type_surface_expr` expands via the hot path and no longer emits the pinned symbolic-vs-concrete mix at the same granularity. The surviving dispatch-backed component-meta coverage already exercises the underlying import/alias resolution.
- `verter_session::meta_resolve::meta_resolve_tests::produce_one_macro_object_shape_keeps_projection_rescue_for_indexed_access_aliases` — asserts `solve_count == 2` directly, which observes the retired `owner_engine.solve_scoped` call count from the projection-rescue path. With `TypeQueryEngine::solve_count` retired and dispatch owning the solve path, there is no analogous engine-level observable to pin.

Three more entries were added once the retired solver-telemetry surface was fully cut:

- `verter_session::meta_resolve::meta_resolve_tests::resolve_component_meta_populates_compute_audit_when_enabled` — reads the retired `ComponentMetaComputeAudit` telemetry block (solver-owned step counters / cache-hit counters) and asserts it was populated during a meta query. Plan §5.9 moves the reusable telemetry onto `SemanticGraphStats`; the solver-specific audit block is gone.
- `verter_session::meta_resolve::meta_resolve_tests::produce_one_macro_object_shape_skips_redundant_projection_for_generic_ref_solver_shapes` — asserts `solve_count == 1` after a projection-rescue path deduplicates against an already-warm generic-ref solver surface. Dispatch replaces the projection-rescue pass with a memo hit in `SemanticGraphStore`, so the "skips a second solver call" observable is retired with the solver.
- `verter_session::meta_resolve::meta_resolve_tests::produce_one_macro_object_shape_skips_projection_rescue_for_nested_indexed_property_types` — asserts `solve_count == 0` after a nested-indexed-property shape is satisfied by the prepared projection without triggering a rescue solve. Same rationale: without a solver, `solve_count` is always zero and the predicate no longer discriminates.

(`host_manage::tests::solver_host_resolves_generic_imported_partial_props` was removed from `EXPLICIT_TEST_IDS` in §5.8 WIP-W Session 9: the test body was rewritten to a pure `get_component_meta` integration and now passes via dispatch's Partial mapper — no solver-host construction remains.)

(`resolver_core::type_surface_db::tests::clear_removes_all` and `resolver_core::type_surface_db::tests::miss_is_cached` were removed from `EXPLICIT_TEST_IDS` in §5.8 WIP-W Session 9: `type_surface_db.rs` + `type_surface_tests.rs` were already deleted wholesale via `DELETE_FILES`, so the explicit entries no longer matched anything in the baseline. The wholesale file-deletion category still covers them.)

These entries live in `EXPLICIT_TEST_IDS` so they are resolved exactly via pass 3 even if future file moves collide with their bare function names.

## Gate semantics

`§7.5 Check 3` regenerates this list at gate time from the same script and diff's the committed `phase-d-approved-test-deletions.txt` against the regenerator output (byte-for-byte). Any drift hard-fails the gate. The generator is deterministic (sorted final output) and collision-hard-failing (ambiguous function names force an `EXPLICIT_TEST_IDS` entry).
