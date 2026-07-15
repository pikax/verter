//! @ai-generated - Synthetic barrel and renamed-import typeinfo tests.

use super::support::*;
use crate::VerterHost;

fn upsert_cross_file_fixture(host: &VerterHost) {
    upsert_ts(host, "/fixtures/cross-file-leaf.ts", CROSS_FILE_LEAF);
    upsert_ts(host, "/fixtures/cross-file-unused.ts", CROSS_FILE_UNUSED);
    upsert_ts(host, "/fixtures/cross-file-barrel.ts", CROSS_FILE_BARREL);
    upsert_ts(
        host,
        "/fixtures/cross-file-consumer.ts",
        CROSS_FILE_CONSUMER,
    );
}

#[test]
fn cross_file_surface_resolves_barrel_and_renamed_imports() {
    let host = make_host_with_footprint();
    upsert_cross_file_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/cross-file-consumer.ts",
        "CrossFileSurface",
        &[],
        ProjectionMode::Expanded,
    );

    // Fixture: CrossFileSurface = RenamedSurface<LocalItem> & { ui?: StyleAlias }.
    // TS7: `ui` is optional (declared with `?` in the consumer intersection),
    // `labelFor` is optional in the renamed barrel definition. `item` and `items`
    // are required because RenamedSurface declares them without `?`.
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["item", "items", "labelFor", "ui"]);
    assert!(!props["item"].optional);
    assert_ref(&props["item"].ty, "LocalItem");
    assert!(!props["items"].optional);
    assert_array_of_ref(&props["items"].ty, "LocalItem");
    assert!(props["ui"].optional);
    assert_ref(&props["ui"].ty, "RemoteStyle");
    assert!(props["labelFor"].optional);
    let label_for = function_type(&props["labelFor"].ty);
    assert_eq!(label_for.parameters.len(), 2);
    assert_ref(&label_for.parameters[0].ty, "LocalItem");
    assert_primitive(&label_for.parameters[1].ty, PrimitiveName::Number);
    assert_primitive(
        label_for
            .return_type
            .as_ref()
            .expect("labelFor has a return type"),
        PrimitiveName::String,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce indexed-access projection through a barrel-renamed imported generic alias; keep as the future cross-file path projection contract"]
fn cross_file_projected_item_resolves_local_extension() {
    let host = make_host_with_footprint();
    upsert_cross_file_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/cross-file-consumer.ts",
        "CrossFileProjectedItem",
        &[],
        ProjectionMode::Expanded,
    );

    let item = object_props(&expr);
    assert_primitive(&item["id"].ty, PrimitiveName::String);
    assert_primitive(&item["label"].ty, PrimitiveName::String);
    assert_primitive(&item["extra"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently preserves multi-hop indexed access through cross-file aliases instead of reducing the terminal property; keep as the future cross-file terminal projection contract"]
fn cross_file_projected_extra_resolves_number_terminal() {
    let host = make_host_with_footprint();
    upsert_cross_file_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/cross-file-consumer.ts",
        "CrossFileProjectedExtra",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently cannot combine Parameters<T>[0] with a cross-file indexed-access function property; keep as the future cross-file Parameters projection contract"]
fn cross_file_label_parameter_resolves_local_item() {
    let host = make_host_with_footprint();
    upsert_cross_file_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/cross-file-consumer.ts",
        "CrossFileLabelFirstParam",
        &[],
        ProjectionMode::Expanded,
    );

    let item = object_props(&expr);
    assert_primitive(&item["extra"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
