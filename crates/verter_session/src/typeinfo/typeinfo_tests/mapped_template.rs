//! @ai-generated - Synthetic mapped-type and template-literal key tests.

use super::support::*;

#[test]
fn mapped_type_without_key_remap_materializes_required_surface() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "PlainSlotMap",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["empty", "item", "root"]);
    assert!(!props["root"].optional);
    let root = object_props(&props["root"].ty);
    assert_primitive(&root["id"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo does not yet materialize mapped types whose keys are remapped with a template-literal name; keep as the future key-remapping contract"]
fn mapped_type_with_template_literal_key_remap_resolves_remapped_slot() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "RootRemappedSlot",
        &[],
        ProjectionMode::Expanded,
    );

    let slot = function_type(&expr);
    assert_eq!(slot.parameters.len(), 1);
    let payload = object_props(&slot.parameters[0].ty);
    assert_primitive(&payload["id"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo does not yet substitute remapped template-literal mapped keys for non-root members; keep as the future remapped member projection contract"]
fn mapped_type_with_template_literal_key_remap_resolves_item_slot() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "ItemRemappedSlot",
        &[],
        ProjectionMode::Expanded,
    );

    let slot = function_type(&expr);
    assert_eq!(slot.parameters.len(), 1);
    let payload = object_props(&slot.parameters[0].ty);
    assert_primitive(&payload["value"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo does not yet reduce template-literal type aliases used as indexed-access keys; keep as the future template-key projection contract"]
fn template_literal_key_alias_projects_static_template_slot() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "NameCellRenderer",
        &[],
        ProjectionMode::Expanded,
    );

    let renderer = function_type(&expr);
    assert_eq!(renderer.parameters.len(), 1);
    let payload = object_props(&renderer.parameters[0].ty);
    assert_primitive(&payload["value"].ty, PrimitiveName::String);
    assert_string_literal(&payload["column"].ty, "name");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo does not yet enumerate Record keys whose keyspace is a template-literal union; keep as the future template-literal Record key contract"]
fn record_with_template_literal_key_union_projects_root_slot() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "RecordTemplateRootSlot",
        &[],
        ProjectionMode::Expanded,
    );

    let slot = function_type(&expr);
    assert_eq!(slot.parameters.len(), 1);
    let payload = object_props(&slot.parameters[0].ty);
    assert_literal_union(&payload["name"].ty, &["item", "root"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo does not yet project indexed access with a template-literal union key into a union of matching members; keep as the future template-union projection contract"]
fn template_literal_union_key_projects_static_slot_union() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "StaticTemplateSlotUnion",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Union(arms) = expr else {
        panic!("expected union of cell renderers, got {expr:?}");
    };
    assert_eq!(arms.len(), 2);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
