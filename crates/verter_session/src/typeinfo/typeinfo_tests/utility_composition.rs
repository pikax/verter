//! @ai-generated - Synthetic built-in utility composition typeinfo tests.

use super::support::*;

#[test]
fn utility_composition_applies_pick_omit_partial_and_required() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/utility-composition.ts",
        UTILITY_COMPOSITION,
    );

    let (required, required_record) = resolve_expr(
        &host,
        "/fixtures/utility-composition.ts",
        "RequiredIdentity",
        &[],
        ProjectionMode::Expanded,
    );
    let required_props = object_props(&required);
    assert_eq!(prop_names(&required_props), vec!["id", "label"]);
    assert_primitive(&required_props["id"].ty, PrimitiveName::String);
    assert_primitive(&required_props["label"].ty, PrimitiveName::String);
    assert!(!required_props["label"].optional);

    let (partial, partial_record) = resolve_expr(
        &host,
        "/fixtures/utility-composition.ts",
        "PublicPartial",
        &[],
        ProjectionMode::Expanded,
    );
    let partial_props = object_props(&partial);
    assert_eq!(
        prop_names(&partial_props),
        vec!["id", "label", "mode", "payload", "tone"]
    );
    assert!(partial_props["id"].optional);
    assert!(partial_props["mode"].optional);
    assert!(
        !partial_props.contains_key("internal"),
        "Omit<UtilitySource, 'internal'> must not publish internal"
    );
    assert_query_mode(&required_record, ProjectionModeTag::Expanded);
    assert_query_mode(&partial_record, ProjectionModeTag::Expanded);
}

#[test]
fn utility_composition_applies_extract_and_exclude_unions() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/utility-composition.ts",
        UTILITY_COMPOSITION,
    );

    let (visible, visible_record) = resolve_expr(
        &host,
        "/fixtures/utility-composition.ts",
        "VisibleMode",
        &[],
        ProjectionMode::Expanded,
    );
    assert_literal_union(&visible, &["edit", "view"]);

    let (runtime, runtime_record) = resolve_expr(
        &host,
        "/fixtures/utility-composition.ts",
        "RuntimeMode",
        &[],
        ProjectionMode::Expanded,
    );
    assert_literal_union(&runtime, &["edit", "view"]);
    assert_query_mode(&visible_record, ProjectionModeTag::Expanded);
    assert_query_mode(&runtime_record, ProjectionModeTag::Expanded);
}

#[test]
fn utility_composition_intersection_keeps_object_contributions() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/utility-composition.ts",
        UTILITY_COMPOSITION,
    );

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/utility-composition.ts",
        "UtilityCombinationSurface",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Intersection(parts) = &expr else {
        panic!("expected intersection surface, got {expr:?}");
    };
    assert!(
        parts.iter().any(
            |part| matches!(part, TypeExpr::Ref { name, .. } if name.as_ref() == "RequiredIdentity")
        ),
        "intersection should preserve RequiredIdentity alias arm; got {expr:?}"
    );
    assert!(
        parts.iter().any(
            |part| matches!(part, TypeExpr::Ref { name, .. } if name.as_ref() == "PublicPartial")
        ),
        "intersection should preserve PublicPartial alias arm; got {expr:?}"
    );
    let object_arm = parts
        .iter()
        .find(|part| matches!(part, TypeExpr::Object(_)))
        .expect("intersection should include inline object contribution");
    let props = object_props(object_arm);
    assert!(props.contains_key("visibleMode"));
    assert!(props.contains_key("runtimeMode"));
    assert_ref(&props["visibleMode"].ty, "VisibleMode");
    assert_ref(&props["runtimeMode"].ty, "RuntimeMode");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently leaves Required<Pick<NonNullable<T['payload']>, ...>> behind nested utility/indexed-access shells; keep as the future deep utility composition contract"]
fn utility_composition_resolves_required_pick_over_nested_nonnullable_payload() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/utility-composition.ts",
        UTILITY_COMPOSITION,
    );

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/utility-composition.ts",
        "DeepUtilityPayload",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["count", "tags"]);
    assert_primitive(&props["count"].ty, PrimitiveName::Number);
    assert_array_of_primitive(&props["tags"].ty, PrimitiveName::String);
    assert!(!props["count"].optional);
    assert!(!props["tags"].optional);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently preserves alias arms in intersections instead of recursively reducing DeepUtilityConfig's utility-derived members; keep as the future deep intersection utility contract"]
fn utility_composition_resolves_deep_intersection_config() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/utility-composition.ts",
        UTILITY_COMPOSITION,
    );

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/utility-composition.ts",
        "DeepUtilityConfig",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["mode", "payload", "tone"]);
    assert_literal_union(&props["mode"].ty, &["edit", "view"]);
    assert_literal_union(&props["tone"].ty, &["accent", "neutral"]);
    let payload = object_props(&props["payload"].ty);
    assert_primitive(&payload["count"].ty, PrimitiveName::Number);
    assert_array_of_primitive(&payload["tags"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
