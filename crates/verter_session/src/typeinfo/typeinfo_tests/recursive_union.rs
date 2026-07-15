//! @ai-generated - Synthetic recursive and object-contribution tests.

use super::support::*;

#[test]
fn recursive_tree_surface_uses_cycle_sentinel_for_children() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/recursive-union.ts", RECURSIVE_UNION);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/recursive-union.ts",
        "RecursiveTreeSurface",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["children", "id", "meta"]);
    assert!(!props["id"].optional);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert!(props["children"].optional);
    assert_array_of_recursive_ref(&props["children"].ty, "TreeNode");
    assert!(props["meta"].optional);
    assert_ref(&props["meta"].ty, "RecursiveMeta");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn cyclic_alias_surface_stops_at_recursive_back_edge() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/recursive-union.ts", RECURSIVE_UNION);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/recursive-union.ts",
        "CyclicAliasSurface",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_string_literal(&props["kind"].ty, "a");
    assert_ref(&props["next"].ty, "AliasB");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn intersection_surface_collects_object_contribution_arms() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/recursive-union.ts", RECURSIVE_UNION);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/recursive-union.ts",
        "IntersectionContribution",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["count", "id", "label", "ready"]);
    assert!(!props["id"].optional);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert!(!props["label"].optional);
    assert_primitive(&props["label"].ty, PrimitiveName::String);
    assert!(!props["count"].optional);
    assert_primitive(&props["count"].ty, PrimitiveName::Number);
    assert!(props["ready"].optional);
    assert_primitive(&props["ready"].ty, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn union_surface_preserves_each_object_contribution_arm() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/recursive-union.ts", RECURSIVE_UNION);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/recursive-union.ts",
        "ObjectUnionContribution",
        &[],
        ProjectionMode::Expanded,
    );

    assert_union_has_object_arm(&expr, &["shared", "text", "variant"]);
    assert_union_has_object_arm(&expr, &["count", "shared", "variant"]);

    let TypeExpr::Union(arms) = &expr else {
        panic!("expected discriminated union, got {expr:?}");
    };
    let text_arm = arms
        .iter()
        .find(|ty| {
            let props = object_props(ty);
            matches!(
                props.get("variant"),
                Some(prop)
                    if matches!(
                        &prop.ty,
                        TypeExpr::Literal(verter_type_expr::LiteralValue::String(value))
                            if value == "text"
                    )
            )
        })
        .expect("text discriminated arm");
    let text_props = object_props(text_arm);
    assert_primitive(&text_props["text"].ty, PrimitiveName::String);
    assert_primitive(&text_props["shared"].ty, PrimitiveName::Boolean);

    let count_arm = arms
        .iter()
        .find(|ty| {
            let props = object_props(ty);
            matches!(
                props.get("variant"),
                Some(prop)
                    if matches!(
                        &prop.ty,
                        TypeExpr::Literal(verter_type_expr::LiteralValue::String(value))
                            if value == "count"
                    )
            )
        })
        .expect("count discriminated arm");
    let count_props = object_props(count_arm);
    assert_primitive(&count_props["count"].ty, PrimitiveName::Number);
    assert_primitive(&count_props["shared"].ty, PrimitiveName::Boolean);

    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
