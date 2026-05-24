//! @ai-generated - Synthetic wide/deep surface typeinfo tests.

use super::support::*;

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
#[ignore = "typeinfo currently preserves multi-hop indexed-access chains instead of reducing WidePanel['nested']['level1']['level2']['target']; keep as the future deep-projection contract"]
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

#[test]
#[ignore = "typeinfo currently cannot reduce terminal properties beyond a multi-hop indexed-access chain; keep as the future wide/deep terminal projection contract"]
fn wide_deep_projected_token_resolves_literal_union() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/wide-deep.ts", WIDE_DEEP);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/wide-deep.ts",
        "WideDeepProjectedToken",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["alpha", "beta", "gamma"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce NonNullable<WidePanel['row00']>['flags'] through optional generic-member projection; keep as the future wide/deep optional member contract"]
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
#[ignore = "typeinfo currently cannot project a terminal flag through nested NonNullable and indexed-access operators; keep as the future wide/deep flag terminal contract"]
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
