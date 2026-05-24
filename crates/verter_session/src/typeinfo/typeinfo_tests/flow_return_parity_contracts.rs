//! @ai-generated - Final TypeScript parity gap contracts for flow-return typeinfo.

use super::support::*;
use crate::VerterHost;
use verter_type_expr::LiteralValue;

const PARITY_OWNER: &str = "/fixtures/flow_return_parity_aug_owner.ts";
const PARITY_BARREL: &str = "/fixtures/flow_return_parity_aug_barrel.ts";
const PARITY_BASE: &str = "/fixtures/flow_return_parity_aug_base.ts";
const PARITY_PATCH: &str = "/fixtures/flow_return_parity_aug_patch.ts";
const PARITY_UNUSED: &str = "/fixtures/flow_return_parity_aug_unused.ts";

fn upsert_parity_fixture(host: &VerterHost) {
    upsert_ts(
        host,
        "/fixtures/flow_return_parity_catalog.ts",
        FLOW_RETURN_PARITY_CATALOG,
    );
}

fn upsert_parity_aug_fixture(host: &VerterHost) {
    upsert_ts(host, PARITY_BASE, FLOW_RETURN_PARITY_AUG_BASE);
    upsert_ts(host, PARITY_PATCH, FLOW_RETURN_PARITY_AUG_PATCH);
    upsert_ts(host, PARITY_UNUSED, FLOW_RETURN_PARITY_AUG_UNUSED);
    upsert_ts(host, PARITY_BARREL, FLOW_RETURN_PARITY_AUG_BARREL);
    upsert_ts(host, PARITY_OWNER, FLOW_RETURN_PARITY_AUG_OWNER);
}

fn assert_parity_alias<F>(alias: &str, check: F)
where
    F: FnOnce(&TypeExpr),
{
    let host = make_host_with_footprint();
    upsert_parity_fixture(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/flow_return_parity_catalog.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    check(&expr);
}

fn assert_parity_aug_alias_warm<F>(alias: &str, check: F)
where
    F: Fn(&TypeExpr),
{
    let host = make_host_with_footprint();
    upsert_parity_aug_fixture(&host);

    let (cold_expr, cold_record) =
        resolve_expr(&host, PARITY_OWNER, alias, &[], ProjectionMode::Expanded);
    assert_loaded_files_include(&host, PARITY_BARREL);
    assert_loaded_files_include(&host, PARITY_BASE);
    assert_loaded_files_include(&host, PARITY_PATCH);
    assert_loaded_files_exclude(&host, PARITY_UNUSED);
    assert_declared_dependency_includes(&cold_record, PARITY_BARREL);
    assert_declared_dependency_includes(&cold_record, PARITY_BASE);
    assert_declared_dependency_includes(&cold_record, PARITY_PATCH);
    assert_declared_dependency_excludes(&cold_record, PARITY_UNUSED);
    assert_query_mode(&cold_record, ProjectionModeTag::Expanded);
    check(&cold_expr);

    let (warm_expr, warm_record) =
        resolve_expr(&host, PARITY_OWNER, alias, &[], ProjectionMode::Expanded);
    assert_eq!(
        warm_expr, cold_expr,
        "warm rerun must preserve the cold typeinfo result for {alias}"
    );
    assert_no_fresh_source_loading(&warm_record);
    assert_no_route_misses(&warm_record);
    assert_declared_dependency_includes(&warm_record, PARITY_BARREL);
    assert_declared_dependency_includes(&warm_record, PARITY_BASE);
    assert_declared_dependency_includes(&warm_record, PARITY_PATCH);
    assert_request_loaded_files_exclude(&warm_record, PARITY_UNUSED);
    assert_declared_dependency_excludes(&warm_record, PARITY_UNUSED);
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

fn assert_ref_with_args<'a>(expr: &'a TypeExpr, expected_name: &str, len: usize) -> &'a [TypeExpr] {
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = expr
    else {
        panic!("expected ref {expected_name}, got {expr:?}");
    };
    assert_eq!(name.as_ref(), expected_name);
    assert_eq!(type_arguments.len(), len);
    type_arguments.as_ref()
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

macro_rules! future_parity_contract {
    ($name:ident, $alias:literal, $reason:literal, $check:expr) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            assert_parity_alias($alias, $check);
        }
    };
}

macro_rules! future_parity_aug_contract {
    ($name:ident, $alias:literal, $reason:literal, $check:expr) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            assert_parity_aug_alias_warm($alias, $check);
        }
    };
}

#[test]
fn flow_return_mp_fixture_routes_are_hermetic_and_resolvable() {
    let host = make_host_with_footprint();
    upsert_parity_aug_fixture(&host);

    let barrel = host.resolve_loaded_dependency_canonical(
        PARITY_OWNER,
        "./flow_return_parity_aug_barrel",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(barrel.as_deref(), Some(PARITY_BARREL));

    let base = host.resolve_loaded_dependency_canonical(
        PARITY_BARREL,
        "./flow_return_parity_aug_base",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(base.as_deref(), Some(PARITY_BASE));

    let patch = host.resolve_loaded_dependency_canonical(
        PARITY_BARREL,
        "./flow_return_parity_aug_patch",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(patch.as_deref(), Some(PARITY_PATCH));

    let unused = host.resolve_loaded_dependency_canonical(
        PARITY_BARREL,
        "./flow_return_parity_aug_unused",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(unused.as_deref(), Some(PARITY_UNUSED));
}

future_parity_contract!(
    flow_return_tp01_private_field_narrowing_inside_class_method,
    "TP01",
    "typeinfo currently does not evaluate private class field facts inside method return bodies; keep as the future TP01 private-field flow contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_parity_contract!(
    flow_return_tp02_protected_member_narrows_inside_base_method,
    "TP02",
    "typeinfo currently does not evaluate protected instance member narrowing inside class methods; keep as the future TP02 protected-member flow contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_parity_contract!(
    flow_return_tp03_class_accessor_getter_projects_union_value,
    "TP03",
    "typeinfo currently does not project class accessor getter return bodies through instance property access; keep as the future TP03 class-accessor contract",
    |expr| {
        assert_union_contains_string_literal(expr, "empty");
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_parity_contract!(
    flow_return_tp04_for_await_async_generator_protocol_preserves_yield_and_return,
    "TP04",
    "typeinfo currently does not model for-await narrowing inside async generator protocols; keep as the future TP04 async-iterator protocol contract",
    |expr| {
        let args = assert_ref_with_args(expr, "AsyncGenerator", 3);
        assert_primitive(&args[0], PrimitiveName::Number);
        assert_string_literal(&args[1], "done");
        assert_primitive(&args[2], PrimitiveName::Unknown);
    }
);

future_parity_contract!(
    flow_return_tp05_contextual_overload_callback_selects_constrained_branch,
    "TP05",
    "typeinfo currently does not select overloads whose branch depends on contextual generic callback return constraints; keep as the future TP05 overload-callback contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_parity_contract!(
    flow_return_tp06_jsx_like_factory_call_union_projects_props_value,
    "TP06",
    "typeinfo currently does not infer JSX-like factory generic calls and project unioned prop values; keep as the future TP06 JSX-like factory contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_parity_contract!(
    flow_return_tp07_satisfies_never_exhaustiveness_does_not_pollute_return,
    "TP07",
    "typeinfo currently does not treat satisfies-never exhaustive tails as unreachable for return joins; keep as the future TP07 satisfies-never contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_parity_contract!(
    flow_return_tp08_finally_disposal_side_effect_preserves_try_callback_return,
    "TP08",
    "typeinfo currently does not preserve callback return joins through cleanup-only finally blocks; keep as the future TP08 disposal-style finally contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_parity_contract!(
    flow_return_tp09_variadic_tuple_conditional_infers_first_literal,
    "TP09",
    "typeinfo currently does not infer variadic tuple conditional returns at body call sites; keep as the future TP09 variadic tuple conditional contract",
    |expr| assert_string_literal(expr, "first")
);

future_parity_contract!(
    flow_return_tp_kitchen_class_factory_callback_and_union_flow,
    "TPKitchen",
    "typeinfo currently does not solve the local parity kitchen with private class methods, optional Array.find, JSX-like factory props, discriminant flow, and object union returns; keep as the future TPKitchen contract",
    |expr| {
        let class_arm = object_props(object_arm_with_kind(expr, "class"));
        assert_primitive(&class_arm["value"].ty, PrimitiveName::String);
        assert_union_contains_primitive(&class_arm["found"].ty, PrimitiveName::String);
        assert_union_contains_primitive(&class_arm["found"].ty, PrimitiveName::Undefined);

        let node_arm = object_props(object_arm_with_kind(expr, "node"));
        assert_primitive(&node_arm["value"].ty, PrimitiveName::Number);
    }
);

future_parity_aug_contract!(
    flow_return_mp01_module_augmentation_member_return_loads_patch_not_unused,
    "MP01",
    "typeinfo currently does not merge module augmentation members while keeping unrelated barrel branches cold; keep as the future MP01 augmentation member contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_parity_aug_contract!(
    flow_return_mp02_augmented_assertion_optional_member_return,
    "MP02",
    "typeinfo currently does not apply assertion signatures over module-augmented interfaces before optional member projection; keep as the future MP02 augmentation assertion contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_parity_aug_contract!(
    flow_return_mp03_augmented_callback_helper_materializes_object_return,
    "MP03",
    "typeinfo currently does not combine module augmentation, imported generic callback helpers, and object return materialization; keep as the future MP03 augmentation callback contract",
    |expr| {
        let props = assert_object_has_props(expr, &["extra", "label"]);
        assert_primitive(&props["extra"].ty, PrimitiveName::Number);
        assert_primitive(&props["label"].ty, PrimitiveName::String);
    }
);

future_parity_aug_contract!(
    flow_return_mp_kitchen_augmented_cross_file_flow_keeps_unused_cold,
    "MPKitchen",
    "typeinfo currently does not solve the module-augmentation kitchen with discriminant flow, optional find, callback inference, nullish coalescing, object union returns, warm-cache reuse, and unused branch exclusion; keep as the future MPKitchen contract",
    |expr| {
        let registry_arm = object_props(object_arm_with_kind(expr, "registry"));
        assert_primitive(&registry_arm["extra"].ty, PrimitiveName::Number);
        assert_primitive(&registry_arm["label"].ty, PrimitiveName::String);

        let fallback_arm = object_props(object_arm_with_kind(expr, "fallback"));
        assert_primitive(&fallback_arm["id"].ty, PrimitiveName::String);
    }
);
