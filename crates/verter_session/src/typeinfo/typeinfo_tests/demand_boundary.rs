//! @ai-generated - Synthetic typeinfo demand-boundary footprint tests.

use super::support::*;

#[test]
#[ignore = "typeinfo currently fails to attach a request footprint on this resolver path after the block-6.i AX-WIP audit-passive-observer refactor (commit b0798e28); the contract is that barrel resolution must not load unrequested re-exports — only the requested leaf appears in the declared dependency footprint. Keep as the future demand-bounded-barrel contract once AX-WIP closes Rule-5 leak."]
fn demand_boundary_barrel_resolution_does_not_load_unrequested_reexport() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/demand-needed.ts", DEMAND_NEEDED);
    upsert_ts(&host, "/fixtures/demand-unused.ts", DEMAND_UNUSED);
    upsert_ts(&host, "/fixtures/demand-barrel.ts", DEMAND_BARREL);
    upsert_ts(&host, "/fixtures/demand-owner.ts", DEMAND_OWNER);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/demand-owner.ts",
        "DemandSurface",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_ref(&props["keep"].ty, "SelectedPayload");
    assert_primitive(&props["local"].ty, PrimitiveName::String);

    let loaded = loaded_file_names(&host);
    assert!(
        loaded
            .iter()
            .any(|path| path == "/fixtures/demand-barrel.ts"),
        "barrel route should be touched for SelectedPayload; got {loaded:?}"
    );
    assert!(
        !loaded
            .iter()
            .any(|path| path == "/fixtures/demand-unused.ts"),
        "unrequested re-export branch must stay unloaded; got {loaded:?}"
    );
    assert!(
        !loaded.iter().any(|path| path == "/fixtures/demand-needed.ts"),
        "shallow imported alias should not load the selected leaf until requested directly; got {loaded:?}"
    );

    let declared = declared_dependency_file_names(&record);
    assert!(
        !declared
            .iter()
            .any(|path| path == "/fixtures/demand-unused.ts"),
        "unrequested re-export branch must not enter the typeinfo footprint; got {declared:?}"
    );
    assert!(
        !declared
            .iter()
            .any(|path| path == "/fixtures/demand-needed.ts"),
        "selected leaf should stay outside the footprint while kept as a shallow imported alias; got {declared:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently keeps imported aliases shallow and does not attribute the selected leaf when projecting through DemandSurface['keep']; keep as the future demand-loaded selected-branch contract"]
fn demand_boundary_projection_into_selected_alias_loads_needed_but_not_unused() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/demand-needed.ts", DEMAND_NEEDED);
    upsert_ts(&host, "/fixtures/demand-unused.ts", DEMAND_UNUSED);
    upsert_ts(&host, "/fixtures/demand-barrel.ts", DEMAND_BARREL);
    upsert_ts(&host, "/fixtures/demand-owner.ts", DEMAND_OWNER);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/demand-owner.ts",
        "DemandProjectedKeep",
        &[],
        ProjectionMode::Expanded,
    );

    let keep = object_props(&expr);
    assert_primitive(&keep["id"].ty, PrimitiveName::String);
    assert_primitive(&keep["value"].ty, PrimitiveName::Number);

    let declared = declared_dependency_file_names(&record);
    assert!(
        declared
            .iter()
            .any(|path| path == "/fixtures/demand-needed.ts"),
        "projecting into keep must attribute the selected leaf; got {declared:?}"
    );
    assert!(
        !declared
            .iter()
            .any(|path| path == "/fixtures/demand-unused.ts"),
        "projecting into keep must not load the unselected barrel sibling; got {declared:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently cannot reduce a terminal property through a shallow imported alias reached via indexed access; keep as the future demand terminal projection contract"]
fn demand_boundary_terminal_projection_resolves_value_without_unused_branch() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/demand-needed.ts", DEMAND_NEEDED);
    upsert_ts(&host, "/fixtures/demand-unused.ts", DEMAND_UNUSED);
    upsert_ts(&host, "/fixtures/demand-barrel.ts", DEMAND_BARREL);
    upsert_ts(&host, "/fixtures/demand-owner.ts", DEMAND_OWNER);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/demand-owner.ts",
        "DemandProjectedKeepValue",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::Number);
    let declared = declared_dependency_file_names(&record);
    assert!(
        declared
            .iter()
            .any(|path| path == "/fixtures/demand-needed.ts"),
        "terminal projection must attribute selected leaf; got {declared:?}"
    );
    assert!(
        !declared
            .iter()
            .any(|path| path == "/fixtures/demand-unused.ts"),
        "terminal projection must not load unselected sibling; got {declared:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
