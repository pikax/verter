//! @ai-generated - Synthetic constrained generic default typeinfo tests.

use super::support::*;

#[test]
fn generic_default_arguments_are_applied_when_omitted() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/generic-defaults.ts", GENERIC_DEFAULTS);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/generic-defaults.ts",
        "DefaultGenericBox",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(
        prop_names(&props),
        vec!["describe", "item", "list", "value"]
    );
    assert_ref(&props["item"].ty, "DefaultItem");
    assert_array_of_ref(&props["list"].ty, "DefaultItem");
    assert_string_literal(&props["value"].ty, "default");
    let describe = function_type(&props["describe"].ty);
    assert_ref(&describe.parameters[0].ty, "DefaultItem");
    assert_string_literal(&describe.parameters[1].ty, "default");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn constrained_generic_arguments_substitute_custom_types() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/generic-defaults.ts", GENERIC_DEFAULTS);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/generic-defaults.ts",
        "CustomGenericBox",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_ref(&props["item"].ty, "CustomItem");
    assert_array_of_ref(&props["list"].ty, "CustomItem");
    assert_string_literal(&props["value"].ty, "custom");
    let describe = function_type(&props["describe"].ty);
    assert_ref(&describe.parameters[0].ty, "CustomItem");
    assert_string_literal(&describe.parameters[1].ty, "custom");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn generic_default_can_reference_prior_constrained_parameter() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/generic-defaults.ts", GENERIC_DEFAULTS);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/generic-defaults.ts",
        "DefaultConstrainedPair",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_string_literal(&props["value"].ty, "left");
    assert_string_literal(&props["mirror"].ty, "left");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
