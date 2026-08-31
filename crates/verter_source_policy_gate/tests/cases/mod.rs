//! Private sorted manifest of the source-tree guards owned outside
//! `verter_session::tests::cases`. Each entry
//! derives its verdict purely from reading the workspace tree — see the
//! crate-level docs in `Cargo.toml` and `tests/main.rs`.
//!
//! Guards that scan `verter_session` anchor `crate_root()` explicitly at
//! `workspace_root().join("crates/verter_session")`; this crate's own
//! `CARGO_MANIFEST_DIR` is not the subject of those checks. The other guards
//! (`tracked_paths_are_portable`,
//! `tracked_paths_no_machine_roots`, `scanners_replacement`,
//! `framework_known_bug_manifest`) already computed the workspace root
//! generically (git-rooted or two parents up from `CARGO_MANIFEST_DIR`).

mod framework_known_bug_manifest;
mod handle_capable_consumer_guards;
mod output_projector_residual_guards;
mod residual_type_expr_body_reader_inventory;
mod scanners_replacement;
mod source_corpus;
mod tracked_paths_are_portable;
mod tracked_paths_no_machine_roots;
mod whole_env_consumer_graph_native_inventory;

/// Exact durable IDs dispatched by [`repository_source_policy_guards`].
///
/// The critical-rule registry parses this const structurally, so a critical
/// member cannot disappear behind the aggregate's generic libtest name. The
/// aggregate independently compares this manifest with the executable member
/// tables before running any rule.
const REQUIRED_REPOSITORY_SOURCE_POLICY_GUARD_IDS: &[&str] = &[
    "every_enumerated_body_reader_is_present_at_its_anchor",
    "every_anchored_body_reader_is_unique",
    "no_method_chain_body_read_outside_the_inventory",
    "compat_consumer_files_contain_no_direct_method_chain_body_read",
    "graph_backed_migrated_anchors_perform_no_typeexpr_body_read",
    "graph_backed_migrated_no_read_check_discriminates",
    "enumeration_is_the_completeness_rail_for_bare_field_readers",
    "real_tree_satisfies_all_invariants",
    "real_tree_inventory_is_non_vacuous",
    "every_whole_env_consumer_has_a_graph_native_reader_in_production",
    "anchored_definitions_are_unique",
    "graph_native_reader_bodies_do_not_route_through_whole_env",
    "no_unanchored_direct_whole_env_reach_in_production",
    "resolved_anchors_are_present_unique_and_enumerated",
    "bounded_body_guard_discriminates_violation_from_clean",
    "structural_inventory_is_non_vacuous_and_anchors_resolve",
    "g_a_exactly_one_boundary_definition_in_raise",
    "stage4_deferred_carriers_have_no_session_resolution_consumer",
    "no_hand_written_no_type_expr_impls_except_audited_hot_type_ref",
    "retired_kind_b_bridge_symbol_absent_from_production_source",
    "output_cap_mint_scope_is_per_leaf_not_subtree",
    "cross_sink_raw_authority_to_type_expr_boundary",
    "forgeable_input_fence_has_no_dual_bearing_type",
    "authority_scopes_contain_no_unsafe",
    "hot_path_never_calls_materialize_type_expr",
    "hot_materialize_scanner_flags_in_memory_injected_offender",
    "hot_terminal_allowlist_entries_are_pure_one_shot_sinks",
    "hot_detector_spellings_are_live_or_synthetic",
];

fn repository_source_policy_guard_receipt() -> Vec<&'static str> {
    residual_type_expr_body_reader_inventory::PRODUCTION_GUARDS
        .iter()
        .chain(whole_env_consumer_graph_native_inventory::PRODUCTION_GUARDS)
        .chain(handle_capable_consumer_guards::PRODUCTION_GUARDS)
        .chain(output_projector_residual_guards::PRODUCTION_GUARDS)
        .map(|(id, _)| *id)
        .collect()
}

fn assert_repository_source_policy_guard_receipt_is_exact() {
    let dispatched = repository_source_policy_guard_receipt();
    assert_eq!(
        dispatched, REQUIRED_REPOSITORY_SOURCE_POLICY_GUARD_IDS,
        "repository source-policy aggregate membership drifted: every required guard ID must map to \
         exactly one executable member, in the reviewed family order"
    );
    let unique: std::collections::HashSet<_> = dispatched.iter().copied().collect();
    assert_eq!(
        unique.len(),
        dispatched.len(),
        "repository source-policy aggregate contains a duplicate guard ID"
    );
}

#[test]
fn repository_source_policy_guards() {
    assert_repository_source_policy_guard_receipt_is_exact();
    source_corpus::assert_repeated_policy_queries_share_one_source_scan();

    std::thread::scope(|scope| {
        let residual =
            scope.spawn(|| residual_type_expr_body_reader_inventory::run_production_guards());
        let whole_env =
            scope.spawn(|| whole_env_consumer_graph_native_inventory::run_production_guards());
        let handle = scope.spawn(|| handle_capable_consumer_guards::run_production_guards());
        let output = scope.spawn(|| output_projector_residual_guards::run_production_guards());

        residual
            .join()
            .expect("residual body-reader production policy thread must complete");
        whole_env
            .join()
            .expect("whole-env production policy thread must complete");
        handle
            .join()
            .expect("handle-capable production policy thread must complete");
        output
            .join()
            .expect("output-projector production policy thread must complete");
    });

    source_corpus::assert_repeated_policy_queries_share_one_source_scan();
}
