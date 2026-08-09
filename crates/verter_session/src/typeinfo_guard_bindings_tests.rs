//! Lib-target half of the typed guard-registry executable bindings.
//!
//! The generated `GuardId` registry marks some live guards
//! `Live { target: SessionLib }`: their `#[test]` fns are compiled into
//! THIS lib unit-test binary (run by `cargo test -p verter_session --lib`
//! and the workspace nextest gate), not the consolidated integration
//! binary. This module includes the generated lib mirror of the registry
//! (same generator data map as the integration-side registry, so the two
//! cannot diverge), binds each lib-live `GuardId` to its `#[test]` fn
//! through `lib_guard!` — one path token sequence yields BOTH the fn
//! pointer (compile-time existence) and the recorded identity — and
//! proves execution/default-gate membership against this binary's OWN
//! libtest inventory.

use std::collections::BTreeSet;

include!("../tests/cases/manifest_data/typeinfo_guard_registry_lib.rs");

/// One live executable binding: `GuardId` → the bound `#[test]` fn.
struct LibGuardBinding {
    id: GuardId,
    run: fn(),
    test_path: &'static str,
}

macro_rules! lib_guard {
    ($id:ident => $p:path) => {
        LibGuardBinding {
            id: GuardId::$id,
            run: { $p } as fn(),
            test_path: stringify!($p),
        }
    };
}

static LIB_LIVE_GUARD_BINDINGS: &[LibGuardBinding] = &[
    lib_guard!(SlotFinalizationEntersEnvOnlyInQueryKey => crate::binder_identity_facts::tests::slot_finalization_enters_env_only_in_query_key),
    lib_guard!(FreshnessTracksPerPropertySpreadTaint => crate::project_semantic_dispatch_invariants_tests::fresh_excess_property_checking::freshness_tracks_per_property_spread_taint),
    lib_guard!(ReverseMappedInferenceIsRelationOwnedInSession => crate::project_semantic_dispatch::relation::reverse_ownership_tests::reverse_mapped_inference_is_relation_owned_in_session),
    lib_guard!(KeyspaceBudgetExceededAdmitsNothing => crate::typeinfo::typeinfo_tests::mapped_template::keyspace_budget_exceeded_admits_nothing),
    lib_guard!(MappedMinusOptionalStripsOnlyOptionalOriginUndefined => crate::typeinfo::typeinfo_tests::mapped_modifiers::mapped_minus_optional_strips_only_optional_origin_undefined),
    lib_guard!(MappedMinusOptionalPreservesExplicitUndefinedOnRequiredProperty => crate::typeinfo::typeinfo_tests::mapped_modifiers::mapped_minus_optional_preserves_explicit_undefined_on_required_property),
    lib_guard!(TemplateLiteralReduceModelsTsNumericBigintLexing => crate::typeinfo::typeinfo_tests::mapped_template::template_literal_reduce_models_ts_numeric_bigint_lexing),
    lib_guard!(FunctionFlowGraphBuiltOncePerFunctionSkeleton => crate::cache_runtime::flow_slice_node::tests::two_demands_one_function_flow_graph_build),
    lib_guard!(FlowSliceIsGraphReachabilityNotProceduralWalk => crate::cache_runtime::flow_slice_node::tests::flow_slice_is_graph_reachability_not_procedural_walk),
    lib_guard!(FlowGraphEffectEdgesStayLivePastValueWrites => crate::cache_runtime::flow_slice_node::tests::flow_graph_effect_edges_stay_live_past_value_writes),
    lib_guard!(FlowGraphBuildIsShallowInternedNoLoweringLazyRegions => crate::cache_runtime::flow_slice_node::tests::flow_graph_build_is_shallow_interned_no_lowering_lazy_regions),
    lib_guard!(FlowReturnRoutesThroughProjectSemanticDispatch => crate::project_semantic_dispatch::flow_return_tests::flow_return_routes_through_project_semantic_dispatch),
    lib_guard!(FlowSliceLoweredBodyDoesNotComputeSliceHash => crate::cache_runtime::flow_slice_node::tests::hash_then_lower_round_trip_serves_lowered_slice_ir),
    lib_guard!(FlowSliceKeysOnBodySensitiveHashNotParseStableHash => crate::cache_runtime::flow_slice_node::tests::distinct_content_versions_key_distinct_artifacts),
    lib_guard!(FlowReturnKeyCoversEnvDimensions => crate::project_semantic_dispatch::flow_return_tests::flow_return_keys_do_not_warm_hit_across_env_axes),
    lib_guard!(FlowReturnKeyCoversInputContextAndProjectionDemand => crate::project_semantic_dispatch::flow_return_tests::flow_return_key_covers_input_context_and_projection_demand),
    lib_guard!(NoFlowSlotInPublishedTypeSurface => crate::component_meta_flow_return_admission_tests::no_flow_slot_in_published_type_surface),
    lib_guard!(FlowSliceBudgetExceededAdmitsNothing => crate::project_semantic_dispatch::flow_return_tests::flow_slice_budget_exceeded_is_return_only_at_the_memo),
    lib_guard!(FlowSliceIrDetachesFromOxcArena => crate::cache_runtime::flow_slice_node::tests::flow_slice_ir_detaches_from_oxc_arena),
    lib_guard!(CacheSatisfactionIsMaterializedPointNotNominalDemand => crate::semantic_query_memo::tests::cache_satisfaction_is_materialized_point_not_nominal_demand),
    lib_guard!(BackfillWritesOnlyRecordedMaterializedPoints => crate::semantic_query_memo::tests::backfill_writes_only_recorded_materialized_points),
    lib_guard!(GuardRegistryLibBindingsAreComplete => crate::typeinfo_guard_bindings_tests::guard_registry_lib_bindings_are_complete),
    lib_guard!(LiveLibGuardsAreHarnessRegisteredAndNotIgnored => crate::typeinfo_guard_bindings_tests::live_lib_guards_are_harness_registered_and_not_ignored),
];

/// The lib binding table binds EXACTLY the generated `LIB_LIVE_GUARD_IDS`
/// set — no duplicates, no foreign id, no lib-live id unbound — and no
/// two ids alias one fn.
#[test]
pub(crate) fn guard_registry_lib_bindings_are_complete() {
    let expected: BTreeSet<GuardId> = LIB_LIVE_GUARD_IDS.iter().copied().collect();
    assert_eq!(
        expected.len(),
        LIB_LIVE_GUARD_IDS.len(),
        "duplicate id in the generated LIB_LIVE_GUARD_IDS",
    );
    let bound: Vec<GuardId> = LIB_LIVE_GUARD_BINDINGS.iter().map(|b| b.id).collect();
    let bound_set: BTreeSet<GuardId> = bound.iter().copied().collect();
    assert_eq!(
        bound.len(),
        bound_set.len(),
        "duplicate GuardId in the lib binding table",
    );
    assert_eq!(
        bound_set,
        expected,
        "lib bindings must equal the Live/SessionLib registry set exactly; \
         unbound={:?} extra={:?}",
        expected.difference(&bound_set).collect::<Vec<_>>(),
        bound_set.difference(&expected).collect::<Vec<_>>(),
    );
    let mut fn_addrs: Vec<usize> = LIB_LIVE_GUARD_BINDINGS
        .iter()
        .map(|b| b.run as usize)
        .collect();
    fn_addrs.sort_unstable();
    fn_addrs.dedup();
    assert_eq!(
        fn_addrs.len(),
        LIB_LIVE_GUARD_BINDINGS.len(),
        "two GuardIds bind the SAME test fn — each live guard is a distinct \
         executable",
    );
}

/// Normalize a `lib_guard!`-recorded path to the harness's test-name
/// form: strip whitespace and the `crate::` prefix.
fn harness_test_name(recorded: &str) -> String {
    let compact: String = recorded.chars().filter(|c| !c.is_whitespace()).collect();
    compact
        .strip_prefix("crate::")
        .unwrap_or(&compact)
        .to_string()
}

/// Ask THIS lib test binary's own libtest harness for its inventory.
fn harness_inventory(extra_args: &[&str]) -> BTreeSet<String> {
    let exe = std::env::current_exe().expect("current_exe");
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("--list")
        .args(extra_args)
        .args(["--format", "terse"]);
    let out = cmd.output().expect("self --list");
    assert!(
        out.status.success(),
        "self harness --list failed: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_suffix(": test"))
        .map(|s| s.to_string())
        .collect()
}

/// Every bound live lib guard is REGISTERED in this binary's own harness
/// inventory and NOT ignored — execution/default-gate-membership proof
/// from the execution universe itself. The negative control proves the
/// oracle discriminates: a real non-test fn is absent from the inventory.
#[test]
pub(crate) fn live_lib_guards_are_harness_registered_and_not_ignored() {
    let all = harness_inventory(&[]);
    let ignored = harness_inventory(&["--ignored"]);

    // Negative control (the oracle discriminates): a plain fn in this very
    // module is not in the inventory.
    assert!(
        !all.contains("typeinfo_guard_bindings_tests::harness_test_name"),
        "negative control: a plain fn must NOT appear in the harness inventory",
    );

    let mut failures: Vec<String> = Vec::new();
    for binding in LIB_LIVE_GUARD_BINDINGS {
        let name = harness_test_name(binding.test_path);
        if !all.contains(&name) {
            failures.push(format!(
                "{:?}: bound path `{name}` is NOT a registered test in this \
                 binary's harness inventory",
                binding.id,
            ));
        }
        if ignored.contains(&name) {
            failures.push(format!(
                "{:?}: bound test `{name}` is #[ignore]d — a live guard must \
                 run in the default gate",
                binding.id,
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "live lib guards not proven executable in the default gate ({}):\n  {}",
        failures.len(),
        failures.join("\n  "),
    );
}
