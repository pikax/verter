//! @ai-generated - Path-precise cross-file flow-return contracts.

use super::support::*;
use crate::VerterHost;
use verter_type_expr::LiteralValue;

const OWNER: &str = "/fixtures/flow_return_path_owner.ts";
const BARREL: &str = "/fixtures/flow_return_path_barrel.ts";
const SELECTED: &str = "/fixtures/flow_return_path_selected.ts";
const ALTERNATE: &str = "/fixtures/flow_return_path_alternate.ts";
const UNUSED: &str = "/fixtures/flow_return_path_unused.ts";

fn upsert_path_fixture(host: &VerterHost) {
    upsert_ts(host, SELECTED, FLOW_RETURN_PATH_SELECTED);
    upsert_ts(host, ALTERNATE, FLOW_RETURN_PATH_ALTERNATE);
    upsert_ts(host, UNUSED, FLOW_RETURN_PATH_UNUSED);
    upsert_ts(host, BARREL, FLOW_RETURN_PATH_BARREL);
    upsert_ts(host, OWNER, FLOW_RETURN_PATH_OWNER);
}

fn assert_path_alias_warm<F>(alias: &str, selected: &[&str], unselected: &[&str], check: F)
where
    F: Fn(&TypeExpr),
{
    let host = make_host_with_footprint();
    upsert_path_fixture(&host);

    let (cold_expr, cold_record) = resolve_expr(&host, OWNER, alias, &[], ProjectionMode::Expanded);
    for canonical_id in selected {
        assert_loaded_files_include(&host, canonical_id);
        assert_declared_dependency_includes(&cold_record, canonical_id);
    }
    for canonical_id in unselected {
        assert_loaded_files_exclude(&host, canonical_id);
        assert_declared_dependency_excludes(&cold_record, canonical_id);
    }
    assert_query_mode(&cold_record, ProjectionModeTag::Expanded);
    check(&cold_expr);

    let (warm_expr, warm_record) = resolve_expr(&host, OWNER, alias, &[], ProjectionMode::Expanded);
    assert_eq!(
        warm_expr, cold_expr,
        "warm rerun must preserve the cold typeinfo result for {alias}"
    );
    assert_no_fresh_source_loading(&warm_record);
    assert_no_route_misses(&warm_record);
    for canonical_id in selected {
        assert_declared_dependency_includes(&warm_record, canonical_id);
    }
    for canonical_id in unselected {
        assert_request_loaded_files_exclude(&warm_record, canonical_id);
        assert_declared_dependency_excludes(&warm_record, canonical_id);
    }
    assert_query_mode(&warm_record, ProjectionModeTag::Expanded);
    check(&warm_expr);
}

fn assert_object_has_props(
    expr: &TypeExpr,
    expected: &[&str],
) -> std::collections::BTreeMap<String, verter_type_expr::ObjectProperty> {
    let props = object_props(expr);
    assert_eq!(prop_names(&props), expected.to_vec());
    props
}

fn assert_union_contains_string_literal(expr: &TypeExpr, expected: &str) {
    let TypeExpr::Union(types) = expr else {
        panic!("expected union containing string literal {expected:?}, got {expr:?}");
    };
    assert!(
        types.iter().any(|ty| {
            matches!(
                ty,
                TypeExpr::Literal(LiteralValue::String(value)) if value == expected
            )
        }),
        "expected union {expr:?} to contain string literal {expected:?}"
    );
}

fn object_arm_with_kind<'a>(expr: &'a TypeExpr, expected_kind: &str) -> &'a TypeExpr {
    let TypeExpr::Union(types) = expr else {
        panic!("expected object union, got {expr:?}");
    };
    types
        .iter()
        .find(|ty| {
            let props = object_props(ty);
            props
                .get("kind")
                .map(|prop| {
                    matches!(
                        &prop.ty,
                        TypeExpr::Literal(LiteralValue::String(value)) if value == expected_kind
                    )
                })
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("expected union {expr:?} to contain kind {expected_kind:?}"))
}

macro_rules! future_path_contract {
    ($name:ident, $alias:literal, $selected:expr, $unselected:expr, $reason:literal, $check:expr) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            assert_path_alias_warm($alias, $selected, $unselected, $check);
        }
    };
}

#[test]
fn flow_return_fp_fixture_routes_are_hermetic_and_resolvable() {
    let host = make_host_with_footprint();
    upsert_path_fixture(&host);

    let owner_barrel = host.resolve_loaded_dependency_canonical(
        OWNER,
        "./flow_return_path_barrel",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(owner_barrel.as_deref(), Some(BARREL));

    let selected = host.resolve_loaded_dependency_canonical(
        BARREL,
        "./flow_return_path_selected",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(selected.as_deref(), Some(SELECTED));

    let alternate = host.resolve_loaded_dependency_canonical(
        BARREL,
        "./flow_return_path_alternate",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(alternate.as_deref(), Some(ALTERNATE));

    let unused = host.resolve_loaded_dependency_canonical(
        BARREL,
        "./flow_return_path_unused",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(unused.as_deref(), Some(UNUSED));
}

future_path_contract!(
    flow_return_fp01_parameter_member_projection_loads_only_selected_type_branch,
    "FP01",
    &[BARREL, SELECTED],
    &[ALTERNATE, UNUSED],
    "typeinfo currently does not project function parameter member returns through a selected barrel-imported type while keeping sibling branches shallow; keep as the future FP01 path-load contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_path_contract!(
    flow_return_fp02_conditional_value_calls_load_exact_two_branch_factories,
    "FP02",
    &[BARREL, SELECTED, ALTERNATE],
    &[UNUSED],
    "typeinfo currently does not route both value-call branches precisely through a barrel while excluding unused reexports; keep as the future FP02 branch-load contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_path_contract!(
    flow_return_fp03_imported_predicate_loads_selected_guard_only,
    "FP03",
    &[BARREL, SELECTED],
    &[ALTERNATE, UNUSED],
    "typeinfo currently does not apply selected-branch imported predicates while keeping alternate and unused branches cold; keep as the future FP03 predicate path contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_path_contract!(
    flow_return_fp04_assertion_and_callback_load_selected_without_siblings,
    "FP04",
    &[BARREL, SELECTED],
    &[ALTERNATE, UNUSED],
    "typeinfo currently does not combine imported assertion effects, generic callback inference, and path-precise selected loading; keep as the future FP04 assertion-callback path contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_path_contract!(
    flow_return_fp05_union_parameter_loads_contributing_type_branches_only,
    "FP05",
    &[BARREL, SELECTED, ALTERNATE],
    &[UNUSED],
    "typeinfo currently does not load exactly the contributing imported type branches for union parameter member flow; keep as the future FP05 union-branch path contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_path_contract!(
    flow_return_fp06_pick_surface_projection_loads_picked_branch_only,
    "FP06",
    &[BARREL, SELECTED],
    &[ALTERNATE, UNUSED],
    "typeinfo currently does not project through Pick<T, K> into only the selected imported branch for function-body member returns; keep as the future FP06 Pick path contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_path_contract!(
    flow_return_fp07_switch_union_loads_all_reachable_branch_types_not_unused,
    "FP07",
    &[BARREL, SELECTED, ALTERNATE],
    &[UNUSED],
    "typeinfo currently does not combine switch discriminant flow with exact cross-file branch loading; keep as the future FP07 switch path contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_path_contract!(
    flow_return_fp08_selected_kitchen_sink_keeps_alternate_and_unused_shallow,
    "FP08",
    &[BARREL, SELECTED],
    &[ALTERNATE, UNUSED],
    "typeinfo currently does not solve the selected-only kitchen path with assertion, Array.find predicate, generic callback return, and warm-cache route constraints; keep as the future FP08 selected kitchen contract",
    |expr| {
        let props = assert_object_has_props(expr, &["id", "label"]);
        assert_primitive(&props["id"].ty, PrimitiveName::String);
        assert_primitive(&props["label"].ty, PrimitiveName::String);
    }
);

future_path_contract!(
    flow_return_fp09_pick_alternate_projection_loads_alternate_only,
    "FP09",
    &[BARREL, ALTERNATE],
    &[SELECTED, UNUSED],
    "typeinfo currently does not project through Pick<T, K> into the alternate imported branch while excluding selected and unused siblings; keep as the future FP09 alternate Pick path contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_path_contract!(
    flow_return_fp10_local_only_flow_does_not_touch_import_routes,
    "FP10",
    &[],
    &[BARREL, SELECTED, ALTERNATE, UNUSED],
    "typeinfo currently does not solve local-only flow returns without touching unrelated owner imports; keep as the future FP10 local-only boundary contract",
    |expr| {
        assert_union_contains_string_literal(expr, "local");
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_path_contract!(
    flow_return_fp11_namespace_import_value_call_loads_selected_only,
    "FP11",
    &[BARREL, SELECTED],
    &[ALTERNATE, UNUSED],
    "typeinfo currently does not route namespace imported value calls path-precisely through barrels while excluding unused siblings; keep as the future FP11 namespace path contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_path_contract!(
    flow_return_fp_kitchen_cross_file_flow_loads_reachable_branches_only,
    "FPKitchen",
    &[BARREL, SELECTED, ALTERNATE],
    &[UNUSED],
    "typeinfo currently does not solve the cross-file kitchen sink with discriminant narrowing, nullish coalescing, imported assertion, callback inference, optional find, object-return unioning, and exact branch loading; keep as the future FPKitchen contract",
    |expr| {
        let selected = object_props(object_arm_with_kind(expr, "selected"));
        assert_primitive(&selected["name"].ty, PrimitiveName::String);
        assert_union_contains_primitive(&selected["found"].ty, PrimitiveName::String);
        assert_union_contains_primitive(&selected["found"].ty, PrimitiveName::Undefined);

        let alternate = object_props(object_arm_with_kind(expr, "alternate"));
        assert_primitive(&alternate["count"].ty, PrimitiveName::Number);
    }
);
