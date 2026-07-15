//! @ai-generated - Synthetic menu-like typeinfo fixture tests.

use super::support::*;

#[test]
fn menu_like_props_surface_keeps_expected_controls() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/menu-like.ts", MENU_LIKE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/menu-like.ts",
        "ConcreteMenuPropsSurface",
        &[],
        ProjectionMode::Expanded,
    );

    // Fixture: MenuProps<A, VK, M, Mod, C> declares every member with `?`. TS7 keeps the
    // optional marker on every instantiated member.
    let props = object_props(&expr);
    assert_eq!(
        prop_names(&props),
        vec![
            "clear",
            "defaultValue",
            "items",
            "modelValue",
            "multiple",
            "ui",
            "valueKey",
        ]
    );
    for name in [
        "clear",
        "defaultValue",
        "items",
        "modelValue",
        "multiple",
        "ui",
        "valueKey",
    ] {
        assert!(
            props[name].optional,
            "MenuProps.{name} declared with `?` must publish optional=true"
        );
    }
    assert_ref(&props["items"].ty, "ConcreteMenuItems");
    assert_ref(&props["ui"].ty, "MenuUi");
    assert_expr_contains_primitive(&props["multiple"].ty, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently resolves the nested Exclude/infer model-value chain to semanticMiss; keep as the future conditional-utility contract"]
fn menu_like_model_value_resolves_nested_conditional_utilities() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/menu-like.ts", MENU_LIKE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/menu-like.ts",
        "ConcreteMenuModelValue",
        &[],
        ProjectionMode::Expanded,
    );

    assert_array_of_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently resolves slot payload extraction through nested conditional utilities to semanticMiss; keep as the future menu-slot contract"]
fn menu_like_slot_payload_extracts_item_and_model_value() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/menu-like.ts", MENU_LIKE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/menu-like.ts",
        "ConcreteMenuLeadingSlotPayload",
        &[],
        ProjectionMode::Expanded,
    );

    let payload = object_props(&expr);
    assert_array_of_primitive(&payload["modelValue"].ty, PrimitiveName::String);
    assert_primitive(&payload["open"].ty, PrimitiveName::Boolean);
    assert_ref(&payload["ui"].ty, "MenuUi");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
