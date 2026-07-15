//! @ai-generated - Synthetic table-like typeinfo fixture tests.

use super::support::*;

#[test]
fn table_like_options_extract_wide_feature_surface() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/table-like.ts", TABLE_LIKE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/table-like.ts",
        "ConcreteGridProps",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    for name in [
        "data",
        "columns",
        "virtualize",
        "onSelect",
        "feature00Options",
        "feature17Options",
    ] {
        assert!(
            props.contains_key(name),
            "table-like surface missing {name}; got {:?}",
            prop_names(&props)
        );
    }
    assert_array_of_ref(&props["data"].ty, "GridRow");
    let column = object_props(array_element(&props["columns"].ty));
    for name in ["accessorKey", "cell", "columns", "header", "meta"] {
        assert!(
            column.contains_key(name),
            "expanded ColumnDef element missing {name}; got {:?}",
            prop_names(&column)
        );
    }
    let feature = object_props(&props["feature07Options"].ty);
    assert!(
        feature.contains_key("state07"),
        "Omit<Feature07Options, 'onFeature07Change'> must keep state07"
    );
    assert!(
        !feature.contains_key("onFeature07Change"),
        "Omit<Feature07Options, 'onFeature07Change'> must remove the callback"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo does not yet materialize template-literal Record keys for dynamic slot lookup; keep as the future dynamic-slot contract"]
fn table_like_dynamic_slot_projection_uses_template_literal_keys() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/table-like.ts", TABLE_LIKE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/table-like.ts",
        "NameCellSlot",
        &[],
        ProjectionMode::Expanded,
    );

    let slot = function_type(&expr);
    assert_eq!(slot.parameters.len(), 1);
    let payload = object_props(&slot.parameters[0].ty);
    assert_ref(&payload["row"].ty, "Row");
    assert_ref(&payload["cell"].ty, "Cell");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
