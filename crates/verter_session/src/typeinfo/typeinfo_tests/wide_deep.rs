//! @ai-generated - Synthetic wide/deep surface typeinfo tests.

use super::oracle;
use super::support::*;
use verter_session_oracle_macro::oracle_row;

#[test]
fn wide_deep_surface_expands_broad_component_like_shape() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/wide-deep.ts", WIDE_DEEP);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/wide-deep.ts",
        "WideDeepSurface",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(
        prop_names(&props),
        vec![
            "header", "nested", "row00", "row01", "row02", "row03", "row04", "row05", "row06",
            "row07", "row08", "row09", "row10", "row11", "row12", "row13", "row14", "row15"
        ]
    );
    let header = object_props(&props["header"].ty);
    assert_primitive(&header["title"].ty, PrimitiveName::String);
    assert_array_of_ref(&header["actions"].ty, "Action");
    assert_ref(&props["row00"].ty, "Leaf");
    assert_ref(&props["row15"].ty, "Leaf");
    let nested = object_props(&props["nested"].ty);
    let level1 = object_props(&nested["level1"].ty);
    let level2 = object_props(&level1["level2"].ty);
    let target = object_props(&level2["target"].ty);
    assert_eq!(prop_names(&target), vec!["id", "score", "token"]);
    assert_primitive(&target["id"].ty, PrimitiveName::String);
    assert_primitive(&target["score"].ty, PrimitiveName::Number);
    assert_ref(&target["token"].ty, "Token");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "the U2 IndexedAccess-reduction bridge has landed (operator-bodied alias reduction in resolve_named_symbol) and reduces a multi-hop chain to its scalar/literal terminal (see the lifted `wide_deep_projected_token`); the remaining blocker is terminal Pick-intersection materialization — surfacing the full `Pick<TLeaf,'id'|'score'> & { token }` object at the indexed-access terminal"]
fn wide_deep_projected_target_resolves_terminal_pick_intersection() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/wide-deep.ts", WIDE_DEEP);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/wide-deep.ts",
        "WideDeepProjectedTarget",
        &[],
        ProjectionMode::Expanded,
    );

    let target = object_props(&expr);
    assert_eq!(prop_names(&target), vec!["id", "score", "token"]);
    assert_primitive(&target["id"].ty, PrimitiveName::String);
    assert_primitive(&target["score"].ty, PrimitiveName::Number);
    assert_literal_union(&target["token"].ty, &["alpha", "beta", "gamma"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// LIFTED: `WideDeepProjectedToken =
// WidePanel["nested"]["level1"]["level2"]["target"]["token"]` reduces the
// multi-hop indexed-access chain THROUGH the terminal `Pick<TLeaf,…> & { token }`
// intersection to the literal union `"alpha" | "beta" | "gamma"`. The lifted body
// is the registry-keyed `oracle::run_row` shared-driver call the `#[oracle_row]`
// macro synthesizes: it resolves Verter's `Expanded` projection and compares it
// against the checked-in tsgo snapshot. The DAG-terminal producer is
// `MappedTemplateRemap` (block `U2.MAPPED_TEMPLATE` — the terminal `Pick` mapped
// remap dominates the trace); the measured dispatch trace is
// `[IndexedAccess, Instantiate, KeyOf, MappedType, ResolveDecl]`, proven live by
// `lifted_row_mechanism_trace_matches_manifest`.
#[oracle_row]
#[test]
fn wide_deep_projected_token_resolves_literal_union() {}

#[test]
#[ignore = "the U2 IndexedAccess-reduction bridge has landed (operator-bodied alias reduction in resolve_named_symbol); the remaining blocker is NonNullable-through-IndexedAccess — reducing `NonNullable<WidePanel['row00']>['flags']` through optional generic-member projection to the Partial<Record<...>> surface"]
fn wide_deep_row_flags_resolve_partial_record_surface() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/wide-deep.ts", WIDE_DEEP);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/wide-deep.ts",
        "WideDeepRowFlags",
        &[],
        ProjectionMode::Expanded,
    );

    let flags = object_props(&expr);
    assert_eq!(prop_names(&flags), vec!["active", "pinned"]);
    assert_primitive(&flags["active"].ty, PrimitiveName::Boolean);
    assert!(flags["active"].optional);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "the U2 IndexedAccess-reduction bridge has landed (operator-bodied alias reduction in resolve_named_symbol); the remaining blocker is NonNullable-through-IndexedAccess — projecting a terminal flag through nested `NonNullable<...>` and indexed-access operators"]
fn wide_deep_flag_active_resolves_boolean_terminal() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/wide-deep.ts", WIDE_DEEP);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/wide-deep.ts",
        "WideDeepFlagActive",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
