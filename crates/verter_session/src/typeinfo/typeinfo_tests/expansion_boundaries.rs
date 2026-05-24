//! @ai-generated - Synthetic tests for positive and negative typeinfo
//! expansion boundaries.

use super::support::*;
use crate::VerterHost;

fn upsert_expansion_fixture(host: &VerterHost) {
    upsert_ts(host, "/fixtures/expansion_selected.ts", EXPANSION_SELECTED);
    upsert_ts(
        host,
        "/fixtures/expansion_unselected.ts",
        EXPANSION_UNSELECTED,
    );
    upsert_ts(host, "/fixtures/expansion_owner.ts", EXPANSION_OWNER);
}

#[test]
fn expansion_surface_expands_inline_literals_but_preserves_alias_boundaries() {
    let host = make_host_with_footprint();
    upsert_expansion_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/expansion_owner.ts",
        "ExpansionSurface",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(
        prop_names(&props),
        vec!["inline", "local", "selected", "unused"]
    );
    assert_ref(&props["local"].ty, "LocalPayload");
    assert_ref(&props["selected"].ty, "SelectedBranch");
    assert_ref(&props["unused"].ty, "UnselectedBranch");

    let inline = object_props(&props["inline"].ty);
    assert_eq!(prop_names(&inline), vec!["details", "visible"]);
    assert_primitive(&inline["visible"].ty, PrimitiveName::Boolean);
    let details = object_props(&inline["details"].ty);
    assert_eq!(prop_names(&details), vec!["count", "note"]);
    assert_primitive(&details["note"].ty, PrimitiveName::String);
    assert_primitive(&details["count"].ty, PrimitiveName::Number);

    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn expansion_pick_keeps_only_requested_members() {
    let host = make_host_with_footprint();
    upsert_expansion_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/expansion_owner.ts",
        "PickedExpansion",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["inline", "local"]);
    assert!(!props.contains_key("selected"));
    assert!(!props.contains_key("unused"));
    assert_ref(&props["local"].ty, "LocalPayload");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently loads direct imported member aliases while enumerating Pick<T, K> even when K excludes those branches; keep as the future route-aware Pick filtering contract"]
fn expansion_pick_does_not_load_unpicked_imports() {
    let host = make_host_with_footprint();
    upsert_expansion_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/expansion_owner.ts",
        "PickedExpansion",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["inline", "local"]);
    assert_loaded_files_exclude(&host, "/fixtures/expansion_selected.ts");
    assert_loaded_files_exclude(&host, "/fixtures/expansion_unselected.ts");
    assert_declared_dependency_excludes(&record, "/fixtures/expansion_selected.ts");
    assert_declared_dependency_excludes(&record, "/fixtures/expansion_unselected.ts");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn expansion_omit_removes_excluded_member() {
    let host = make_host_with_footprint();
    upsert_expansion_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/expansion_owner.ts",
        "OmittedExpansion",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["inline", "local", "selected"]);
    assert!(!props.contains_key("unused"));
    assert_ref(&props["selected"].ty, "SelectedBranch");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently loads the direct imported branch for Omit<T, K> before applying the excluded key filter; keep as the future route-aware Omit filtering contract"]
fn expansion_omit_does_not_load_excluded_import() {
    let host = make_host_with_footprint();
    upsert_expansion_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/expansion_owner.ts",
        "OmittedExpansion",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["inline", "local", "selected"]);
    assert_loaded_files_exclude(&host, "/fixtures/expansion_unselected.ts");
    assert_declared_dependency_excludes(&record, "/fixtures/expansion_unselected.ts");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently preserves multi-hop indexed access for same-file object literals instead of expanding only the terminal inline member; keep as the future path-precise inline projection contract"]
fn expansion_inline_details_projection_expands_only_terminal_inline_path() {
    let host = make_host_with_footprint();
    upsert_expansion_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/expansion_owner.ts",
        "InlineDetailsProjection",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["count", "note"]);
    assert_primitive(&props["note"].ty, PrimitiveName::String);
    assert_primitive(&props["count"].ty, PrimitiveName::Number);
    assert_loaded_files_exclude(&host, "/fixtures/expansion_selected.ts");
    assert_loaded_files_exclude(&host, "/fixtures/expansion_unselected.ts");
    assert_declared_dependency_excludes(&record, "/fixtures/expansion_selected.ts");
    assert_declared_dependency_excludes(&record, "/fixtures/expansion_unselected.ts");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not reduce indexed-access projection through a local alias member before expanding the terminal branch object; keep as the future path-precise local alias projection contract"]
fn expansion_local_branch_projection_expands_target_without_sibling_meta() {
    let host = make_host_with_footprint();
    upsert_expansion_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/expansion_owner.ts",
        "LocalBranchProjection",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["deep", "visible"]);
    assert_string_literal(&props["visible"].ty, "local");
    let deep = object_props(&props["deep"].ty);
    assert_string_literal(&deep["token"].ty, "deep");
    assert_loaded_files_exclude(&host, "/fixtures/expansion_selected.ts");
    assert_loaded_files_exclude(&host, "/fixtures/expansion_unselected.ts");
    assert_declared_dependency_excludes(&record, "/fixtures/expansion_selected.ts");
    assert_declared_dependency_excludes(&record, "/fixtures/expansion_unselected.ts");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently keeps imported aliases shallow and cannot project through ExpansionSurface['selected']; keep as the future imported path projection contract"]
fn expansion_imported_projection_loads_selected_but_not_unselected_branch() {
    let host = make_host_with_footprint();
    upsert_expansion_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/expansion_owner.ts",
        "ImportedSelectedProjection",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["nested", "value"]);
    assert_primitive(&props["value"].ty, PrimitiveName::String);
    assert!(
        declared_dependency_file_names(&record)
            .iter()
            .any(|path| path == "/fixtures/expansion_selected.ts"),
        "selected projection must attribute the selected dependency"
    );
    assert_loaded_files_exclude(&host, "/fixtures/expansion_unselected.ts");
    assert_declared_dependency_excludes(&record, "/fixtures/expansion_unselected.ts");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently cannot reduce a terminal boolean through imported alias plus multi-hop indexed access; keep as the future imported terminal projection contract"]
fn expansion_imported_terminal_projection_reduces_flag_without_unselected_branch() {
    let host = make_host_with_footprint();
    upsert_expansion_fixture(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/expansion_owner.ts",
        "ImportedNestedFlagProjection",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Boolean);
    assert!(
        declared_dependency_file_names(&record)
            .iter()
            .any(|path| path == "/fixtures/expansion_selected.ts"),
        "terminal projection must attribute the selected dependency"
    );
    assert_loaded_files_exclude(&host, "/fixtures/expansion_unselected.ts");
    assert_declared_dependency_excludes(&record, "/fixtures/expansion_unselected.ts");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
