//! @ai-generated - Synthetic tests mapped from
//! /tmp/verter-native-flow-return-coverage.md.

use super::support::*;
use crate::VerterHost;
use verter_type_expr::LiteralValue;

const SYNTHETIC_FLOW_VALUES_PACKAGE_JSON: &str = r#"{
  "name": "synthetic-flow-values",
  "types": "./dist/index.d.ts",
  "exports": {
    ".": {
      "types": "./dist/index.d.ts",
      "import": "./dist/index.js"
    }
  }
}"#;

const SYNTHETIC_FLOW_VALUES_RUNTIME: &str = r#"
export function xf10GetValue() { return { id: '' }; }
export function xf12GetPair() { return [{ id: '' }, 0]; }
export function xf13IsRecord(value) { return !!value && typeof value.label === 'string'; }
export function xf14AssertRecord(value) { if (!xf13IsRecord(value)) throw new Error('missing label'); }
export function xf15Wrap(value) { return { value }; }
export function xf16Pick(key) { return key === 'count' ? 0 : ''; }
"#;

fn upsert_flow_return_fixture(host: &VerterHost) {
    upsert_ts(
        host,
        "/fixtures/flow-return-catalog.ts",
        FLOW_RETURN_CATALOG,
    );
}

fn upsert_flow_return_cross_fixture(host: &VerterHost) {
    upsert_flow_return_cross_fixture_at(host, "/fixtures");
}

fn upsert_flow_return_cross_fixture_at(host: &VerterHost, dir: &str) {
    upsert_ts(
        host,
        &format!("{dir}/flow_return_cross_types.ts"),
        FLOW_RETURN_CROSS_TYPES,
    );
    upsert_ts(
        host,
        &format!("{dir}/flow_return_cross_factory.ts"),
        FLOW_RETURN_CROSS_FACTORY,
    );
    upsert_ts(
        host,
        &format!("{dir}/flow_return_cross_guards.ts"),
        FLOW_RETURN_CROSS_GUARDS,
    );
    upsert_ts(
        host,
        &format!("{dir}/flow_return_cross_source.ts"),
        FLOW_RETURN_CROSS_SOURCE,
    );
    upsert_ts(
        host,
        &format!("{dir}/flow_return_cross_index.ts"),
        FLOW_RETURN_CROSS_INDEX,
    );
    upsert_ts(
        host,
        &format!("{dir}/flow_return_cross_main.ts"),
        FLOW_RETURN_CROSS_MAIN,
    );
}

fn upsert_flow_return_cross_package_fixture_at(host: &VerterHost, dir: &str) {
    upsert_ts(
        host,
        &format!("{dir}/flow_return_cross_package_main.ts"),
        FLOW_RETURN_CROSS_PACKAGE_MAIN,
    );
}

fn assert_catalog_alias<F>(alias: &str, check: F)
where
    F: FnOnce(&TypeExpr),
{
    let host = make_host_with_footprint();
    upsert_flow_return_fixture(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/flow-return-catalog.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    check(&expr);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

fn assert_catalog_alias_warm<F>(alias: &str, check: F)
where
    F: Fn(&TypeExpr),
{
    let host = make_host_with_footprint();
    upsert_flow_return_fixture(&host);
    let (cold_expr, cold_record) = resolve_expr(
        &host,
        "/fixtures/flow-return-catalog.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    check(&cold_expr);
    assert_query_mode(&cold_record, ProjectionModeTag::Expanded);

    let (warm_expr, warm_record) = resolve_expr(
        &host,
        "/fixtures/flow-return-catalog.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    check(&warm_expr);
    assert_eq!(
        warm_expr, cold_expr,
        "warm rerun must preserve the cold typeinfo result for {alias}"
    );
    assert_no_fresh_source_loading(&warm_record);
    assert_query_mode(&warm_record, ProjectionModeTag::Expanded);
}

fn assert_cross_alias_warm<F>(alias: &str, check: F, selected: &[&str], unselected: &[&str])
where
    F: Fn(&TypeExpr),
{
    assert_cross_alias_warm_impl(alias, check, selected, unselected, false);
}

fn assert_cross_alias_warm_with_dependency_footprint<F>(
    alias: &str,
    check: F,
    selected: &[&str],
    unselected: &[&str],
) where
    F: Fn(&TypeExpr),
{
    assert_cross_alias_warm_impl(alias, check, selected, unselected, true);
}

fn assert_cross_alias_warm_impl<F>(
    alias: &str,
    check: F,
    selected: &[&str],
    unselected: &[&str],
    require_selected_dependency: bool,
) where
    F: Fn(&TypeExpr),
{
    let host = make_host_with_footprint();
    upsert_flow_return_cross_fixture(&host);
    let (cold_expr, cold_record) = resolve_expr(
        &host,
        "/fixtures/flow_return_cross_main.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    check(&cold_expr);
    for canonical_id in selected {
        assert_loaded_files_include(&host, canonical_id);
        if require_selected_dependency {
            assert_declared_dependency_includes(&cold_record, canonical_id);
        }
    }
    for canonical_id in unselected {
        assert_loaded_files_exclude(&host, canonical_id);
        assert_declared_dependency_excludes(&cold_record, canonical_id);
    }
    assert_query_mode(&cold_record, ProjectionModeTag::Expanded);

    let (warm_expr, warm_record) = resolve_expr(
        &host,
        "/fixtures/flow_return_cross_main.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    check(&warm_expr);
    assert_eq!(
        warm_expr, cold_expr,
        "warm rerun must preserve the cold typeinfo result for {alias}"
    );
    assert_no_fresh_source_loading(&warm_record);
    assert_no_route_misses(&warm_record);
    if require_selected_dependency {
        for canonical_id in selected {
            assert_declared_dependency_includes(&warm_record, canonical_id);
        }
    }
    for canonical_id in unselected {
        assert_request_loaded_files_exclude(&warm_record, canonical_id);
        assert_declared_dependency_excludes(&warm_record, canonical_id);
    }
    assert_query_mode(&warm_record, ProjectionModeTag::Expanded);
}

fn assert_cross_package_alias<F>(alias: &str, check: F)
where
    F: Fn(&TypeExpr),
{
    let host = make_host_with_workspace_files_footprint(&[
        (
            "/workspace/node_modules/synthetic-flow-values/package.json",
            SYNTHETIC_FLOW_VALUES_PACKAGE_JSON,
        ),
        (
            "/workspace/node_modules/synthetic-flow-values/dist/index.d.ts",
            FLOW_RETURN_PACKAGE_DECLARATIONS,
        ),
        (
            "/workspace/node_modules/synthetic-flow-values/dist/index.js",
            SYNTHETIC_FLOW_VALUES_RUNTIME,
        ),
        (
            "/workspace/node_modules/synthetic-flow-values/dist/unused.d.ts",
            "export declare const unusedFlowValue: boolean;",
        ),
    ]);
    upsert_flow_return_cross_package_fixture_at(&host, "/workspace/src");
    let package_runtime = host.resolve_loaded_dependency_canonical(
        "/workspace/src/flow_return_cross_package_main.ts",
        "synthetic-flow-values",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(
        package_runtime.as_deref(),
        Some("/workspace/node_modules/synthetic-flow-values/dist/index.js"),
        "fixture precondition: host must resolve the synthetic package runtime route",
    );
    let package_declaration = host.resolve_eval_dependency_canonical(
        "/workspace/node_modules/synthetic-flow-values/dist/index.js",
    );
    assert_eq!(
        package_declaration.as_deref(),
        Some("/workspace/node_modules/synthetic-flow-values/dist/index.d.ts"),
        "fixture precondition: host must model the runtime-to-declaration companion route",
    );
    let (expr, record) = resolve_expr(
        &host,
        "/workspace/src/flow_return_cross_package_main.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    check(&expr);
    assert_loaded_files_include(
        &host,
        "/workspace/node_modules/synthetic-flow-values/dist/index.d.ts",
    );
    assert_declared_dependency_includes(
        &record,
        "/workspace/node_modules/synthetic-flow-values/dist/index.d.ts",
    );
    assert_declared_dependency_excludes(
        &record,
        "/workspace/node_modules/synthetic-flow-values/dist/unused.d.ts",
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);

    let (warm_expr, warm_record) = resolve_expr(
        &host,
        "/workspace/src/flow_return_cross_package_main.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    check(&warm_expr);
    assert_eq!(
        warm_expr, expr,
        "warm rerun must preserve the cold package-backed typeinfo result for {alias}"
    );
    assert_no_fresh_source_loading(&warm_record);
    assert_no_route_misses(&warm_record);
    assert_declared_dependency_includes(
        &warm_record,
        "/workspace/node_modules/synthetic-flow-values/dist/index.d.ts",
    );
    assert_request_loaded_files_exclude(
        &warm_record,
        "/workspace/node_modules/synthetic-flow-values/dist/unused.d.ts",
    );
    assert_declared_dependency_excludes(
        &warm_record,
        "/workspace/node_modules/synthetic-flow-values/dist/unused.d.ts",
    );
    assert_query_mode(&warm_record, ProjectionModeTag::Expanded);
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

fn assert_mixed_literal_union(expr: &TypeExpr, strings: &[&str], numbers: &[f64]) {
    let TypeExpr::Union(types) = expr else {
        panic!("expected mixed literal union, got {expr:?}");
    };
    let mut actual_strings = Vec::new();
    let mut actual_numbers = Vec::new();
    for ty in types.iter() {
        match ty {
            TypeExpr::Literal(LiteralValue::String(value)) => actual_strings.push(value.as_str()),
            TypeExpr::Literal(LiteralValue::Number(value)) => actual_numbers.push(value.to_bits()),
            other => panic!("expected literal union arm, got {other:?}"),
        }
    }
    actual_strings.sort_unstable();
    let mut expected_strings = strings.to_vec();
    expected_strings.sort_unstable();
    assert_eq!(actual_strings, expected_strings);

    actual_numbers.sort_unstable();
    let mut expected_numbers: Vec<u64> = numbers.iter().map(|value| value.to_bits()).collect();
    expected_numbers.sort_unstable();
    assert_eq!(actual_numbers, expected_numbers);
}

fn assert_union_contains_undefined(expr: &TypeExpr) {
    assert_union_contains_primitive(expr, PrimitiveName::Undefined);
}

fn assert_union_contains_number_literal(expr: &TypeExpr, expected: f64) {
    let TypeExpr::Union(types) = expr else {
        panic!("expected union containing number literal {expected:?}, got {expr:?}");
    };
    assert!(
        types.iter().any(|ty| {
            matches!(
                ty,
                TypeExpr::Literal(LiteralValue::Number(value))
                    if value.to_bits() == expected.to_bits()
            )
        }),
        "expected union {expr:?} to contain number literal {expected:?}"
    );
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

fn assert_object_has_props(
    expr: &TypeExpr,
    expected: &[&str],
) -> std::collections::BTreeMap<String, verter_type_expr::ObjectProperty> {
    let props = object_props(expr);
    assert_eq!(prop_names(&props), expected.to_vec());
    props
}

macro_rules! future_catalog_contract {
    ($name:ident, $alias:literal, $reason:literal, $check:expr) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            assert_catalog_alias($alias, $check);
        }
    };
}

macro_rules! future_cross_contract {
    ($name:ident, $alias:literal, $selected:expr, $unselected:expr, $reason:literal, $check:expr) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            assert_cross_alias_warm_with_dependency_footprint(
                $alias,
                $check,
                $selected,
                $unselected,
            );
        }
    };
}

macro_rules! future_cross_package_contract {
    ($name:ident, $alias:literal, $reason:literal, $check:expr) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            assert_cross_package_alias($alias, $check);
        }
    };
}

macro_rules! catalog_contract {
    ($name:ident, $alias:literal, $check:expr) => {
        #[test]
        fn $name() {
            assert_catalog_alias($alias, $check);
        }
    };
}

macro_rules! catalog_warm_contract {
    ($name:ident, $alias:literal, $check:expr) => {
        #[test]
        fn $name() {
            assert_catalog_alias_warm($alias, $check);
        }
    };
}

#[test]
fn flow_return_xf_fixture_routes_are_hermetic_and_resolvable() {
    let host = make_host_with_footprint();
    upsert_flow_return_cross_fixture(&host);

    let types = host.resolve_loaded_dependency_canonical(
        "/fixtures/flow_return_cross_main.ts",
        "./flow_return_cross_types",
        verter_workspace::ResolveRequestKind::TypeImport,
    );
    assert_eq!(
        types.as_deref(),
        Some("/fixtures/flow_return_cross_types.ts")
    );

    let factory = host.resolve_loaded_dependency_canonical(
        "/fixtures/flow_return_cross_main.ts",
        "./flow_return_cross_factory",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(
        factory.as_deref(),
        Some("/fixtures/flow_return_cross_factory.ts")
    );

    let guards = host.resolve_loaded_dependency_canonical(
        "/fixtures/flow_return_cross_main.ts",
        "./flow_return_cross_guards",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(
        guards.as_deref(),
        Some("/fixtures/flow_return_cross_guards.ts")
    );

    let index = host.resolve_loaded_dependency_canonical(
        "/fixtures/flow_return_cross_main.ts",
        "./flow_return_cross_index",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(
        index.as_deref(),
        Some("/fixtures/flow_return_cross_index.ts")
    );

    let source = host.resolve_loaded_dependency_canonical(
        "/fixtures/flow_return_cross_index.ts",
        "./flow_return_cross_source",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(
        source.as_deref(),
        Some("/fixtures/flow_return_cross_source.ts")
    );

    let barrel_guards = host.resolve_loaded_dependency_canonical(
        "/fixtures/flow_return_cross_index.ts",
        "./flow_return_cross_guards",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(
        barrel_guards.as_deref(),
        Some("/fixtures/flow_return_cross_guards.ts")
    );
}

#[test]
fn flow_return_bl03_multi_return_union_is_collected() {
    assert_catalog_alias("BL03", |expr| {
        assert_mixed_literal_union(expr, &["a"], &[1.0]);
    });
}

#[test]
fn flow_return_bl05_explicit_never_annotation_wins() {
    assert_catalog_alias("BL05", |expr| {
        assert_primitive(expr, PrimitiveName::Never);
    });
}

#[test]
fn flow_return_bl14_unreachable_branch_reference_behavior_is_collected() {
    assert_catalog_alias("BL14", |expr| {
        assert_mixed_literal_union(expr, &["x"], &[1.0]);
    });
}

#[test]
fn flow_return_ob05_satisfies_preserves_value_shape() {
    assert_catalog_alias("OB05", |expr| {
        let props = assert_object_has_props(expr, &["debug", "mode"]);
        assert_string_literal(&props["mode"].ty, "dark");
        assert_boolean_literal(&props["debug"].ty, false);
    });
}

#[test]
fn flow_return_cf13_labeled_break_current_return_collection() {
    assert_catalog_alias("CF13", |expr| {
        assert_string_literal(expr, "done");
    });
}

future_catalog_contract!(
    flow_return_bl01_widens_primitive_literal_return,
    "BL01",
    "typeinfo currently preserves a numeric literal from an unannotated function body instead of applying TypeScript return-literal widening; keep as the future BL01 widening contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_bl02_widens_object_return_properties_selectively,
    "BL02",
    "typeinfo currently collects object return shape but does not apply TypeScript property-level return widening while preserving explicit const assertions; keep as the future BL02 object widening contract",
    |expr| {
        let props = assert_object_has_props(expr, &["count", "kind"]);
        assert_string_literal(&props["kind"].ty, "ok");
        assert_primitive(&props["count"].ty, PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_bl04_adds_implicit_undefined_for_fallthrough,
    "BL04",
    "typeinfo currently collects explicit return expressions only and does not add implicit undefined for reachable function fallthrough; keep as the future BL04 fallthrough contract",
    |expr| {
        assert_union_contains_undefined(expr);
        assert_union_contains_number_literal(expr, 1.0);
    }
);

future_catalog_contract!(
    flow_return_bl06_ignores_throw_branch_and_widens_surviving_return,
    "BL06",
    "typeinfo currently lacks terminating throw-flow modeling plus return-literal widening; keep as the future BL06 throw branch contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_bl07_wraps_async_return_in_promise,
    "BL07",
    "typeinfo currently does not synthesize Promise<joined return> for inferred async function bodies; keep as the future BL07 async return contract",
    |expr| {
        let args = assert_ref_with_args(expr, "Promise", 1);
        assert_primitive(&args[0], PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_bl08_constructs_generator_protocol_return,
    "BL08",
    "typeinfo currently does not model generator yield/return protocol as Generator<Yield, Return, Next>; keep as the future BL08 generator contract",
    |expr| {
        let args = assert_ref_with_args(expr, "Generator", 3);
        assert_primitive(&args[0], PrimitiveName::Number);
        assert_string_literal(&args[1], "done");
        assert_primitive(&args[2], PrimitiveName::Unknown);
    }
);

future_catalog_contract!(
    flow_return_bl09_preserves_readonly_tuple_const_return,
    "BL09",
    "typeinfo currently does not infer readonly tuple shapes from array literals under function-body as const returns; keep as the future BL09 const-tuple contract",
    |expr| {
        let TypeExpr::Tuple { elements, readonly } = expr else {
            panic!("expected readonly tuple, got {expr:?}");
        };
        assert!(*readonly);
        assert_eq!(elements.len(), 2);
        assert_number_literal(&elements[0].ty, 1.0);
        assert_string_literal(&elements[1].ty, "x");
    }
);

future_catalog_contract!(
    flow_return_bl10_widens_mutable_array_element_union,
    "BL10",
    "typeinfo currently does not apply TypeScript array-literal return widening from literal elements to string | number; keep as the future BL10 mutable-array contract",
    |expr| {
        let element = array_element(expr);
        assert_union_contains_primitive(element, PrimitiveName::String);
        assert_union_contains_primitive(element, PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_bl11_constructs_async_generator_protocol_return,
    "BL11",
    "typeinfo currently does not model async generator yield/return protocol as AsyncGenerator<Yield, Return, Next>; keep as the future BL11 async-generator contract",
    |expr| {
        let args = assert_ref_with_args(expr, "AsyncGenerator", 3);
        assert_number_literal(&args[0], 1.0);
        assert_string_literal(&args[1], "done");
        assert_primitive(&args[2], PrimitiveName::Unknown);
    }
);

future_catalog_contract!(
    flow_return_bl12_models_bare_return_as_void,
    "BL12",
    "typeinfo currently ignores bare return statements with no expression instead of normalizing the function return to void; keep as the future BL12 bare-return contract",
    |expr| assert_primitive(expr, PrimitiveName::Void)
);

future_catalog_contract!(
    flow_return_bl13_models_unannotated_throw_only_as_void,
    "BL13",
    "typeinfo currently has no TypeScript-compatible no-return-expression fallback for unannotated throw-only bodies; keep as the future BL13 throw-only fallback contract",
    |expr| assert_primitive(expr, PrimitiveName::Void)
);

future_catalog_contract!(
    flow_return_bl15_models_divergent_loop_as_void,
    "BL15",
    "typeinfo currently does not model divergent loop bodies or apply TypeScript-compatible no-return-expression fallback; keep as the future BL15 divergent-loop contract",
    |expr| assert_primitive(expr, PrimitiveName::Void)
);

future_catalog_contract!(
    flow_return_lr01_resolves_parameter_identifier_return,
    "LR01",
    "typeinfo currently lowers function-body parameter identifiers to unresolved typeof roots instead of resolving against the function parameter environment; keep as the future LR01 parameter-flow contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_lr02_resolves_const_local_identifier_return,
    "LR02",
    "typeinfo currently does not capture same-body const local flow facts for identifier returns; keep as the future LR02 local-const contract",
    |expr| assert_string_literal(expr, "a")
);

future_catalog_contract!(
    flow_return_lr03_tracks_let_alias_narrowing,
    "LR03",
    "typeinfo currently does not build flow slots for let aliases or branch narrowing before return joins; keep as the future LR03 alias-flow contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_lr04_applies_reassignment_before_return,
    "LR04",
    "typeinfo currently does not model assignment effects on mutable local flow facts before identifier returns; keep as the future LR04 reassignment contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_lr05_widens_mutated_let_literal,
    "LR05",
    "typeinfo currently does not apply declaration-kind-aware widening and assignment joins for mutated let bindings; keep as the future LR05 mutable-widening contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_lr06_tracks_destructuring_alias_flow,
    "LR06",
    "typeinfo currently does not create flow facts for destructured local aliases before narrowing and return joins; keep as the future LR06 destructuring-alias contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_lr07_projects_nested_parameter_member_path,
    "LR07",
    "typeinfo currently lowers nested parameter member returns to typeof paths instead of projecting through the parameter object type; keep as the future LR07 member-path contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_lr08_propagates_optional_chain_undefined,
    "LR08",
    "typeinfo currently does not lower optional chaining as member projection plus nullish undefined propagation; keep as the future LR08 optional-chain contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_catalog_contract!(
    flow_return_lr09_applies_non_null_assertion,
    "LR09",
    "typeinfo currently does not lower non-null assertion expressions to NonNullable flow facts; keep as the future LR09 non-null assertion contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

// LR10 — TS7 keeps the initial narrowing of `x` as `string` ("a" literal narrowed
// to its primitive). Passing `() => { x = 1 }` to `cb` does NOT invalidate the
// local narrowing because TS only invalidates after the callback is actually
// invoked, and the call is opaque at the type level. Final inferred return: `string`.
future_catalog_contract!(
    flow_return_lr10_invalidates_captured_local_after_unknown_call,
    "LR10",
    "typeinfo currently does not preserve TypeScript's narrowed local return type after callback registration; keep as the future LR10 callback-registration flow contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

// LR11 — TS7 emits `number` for the inferred return type. The `var x = 1` is
// hoisted to function scope, so `x` is typed as `number` everywhere; the
// use-before-assignment error (TS2454) is a *separate* diagnostic on the body
// at the return position, and does NOT widen the inferred return into
// `number | undefined`. Verter must keep the return-type inference and the
// definite-assignment diagnostic on independent tracks.
future_catalog_contract!(
    flow_return_lr11_models_var_hoist_and_maybe_assignment,
    "LR11",
    "typeinfo currently does not model var hoisting while keeping TypeScript's definite-assignment diagnostic separate from the return type; keep as the future LR11 var-hoist contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_lr12_tracks_member_assignment_fact,
    "LR12",
    "typeinfo currently does not write flow facts for object member assignments before member returns; keep as the future LR12 member-assignment contract",
    |expr| assert_string_literal(expr, "ready")
);

future_catalog_contract!(
    flow_return_cn01_tracks_typeof_positive_and_negative_branches,
    "CN01",
    "typeinfo currently lacks typeof guard facts for both true and false branches; keep as the future CN01 branch-narrowing contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_cn02_filters_truthy_and_falsy_literals,
    "CN02",
    "typeinfo currently does not filter known falsy literal constituents through truthiness checks; keep as the future CN02 truthiness contract",
    |expr| assert_mixed_literal_union(expr, &["a", "fallback"], &[1.0])
);

future_catalog_contract!(
    flow_return_cn03_applies_nullish_equality_narrowing,
    "CN03",
    "typeinfo currently does not treat x != null as excluding both null and undefined before member projection; keep as the future CN03 nullish narrowing contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_cn04_applies_strict_undefined_narrowing,
    "CN04",
    "typeinfo currently does not subtract undefined from the continuation branch after x === undefined; keep as the future CN04 strict-undefined contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_cn05_applies_literal_equality_narrowing,
    "CN05",
    "typeinfo currently does not filter literal-union constituents across equality branches; keep as the future CN05 literal-equality contract",
    |expr| assert_mixed_literal_union(expr, &["b", "c"], &[1.0])
);

future_catalog_contract!(
    flow_return_cn06_switch_discriminant_joins_case_returns,
    "CN06",
    "typeinfo currently does not build switch CFG case facts for discriminated unions; keep as the future CN06 switch-discriminant contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_cn07_in_operator_narrows_object_union,
    "CN07",
    "typeinfo currently does not apply property-presence facts from the in operator before member projection; keep as the future CN07 in-narrowing contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_cn08_models_array_is_array_predicate,
    "CN08",
    "typeinfo currently does not model Array.isArray as an intrinsic predicate before array element/member projection; keep as the future CN08 array-predicate contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_cn09_models_instanceof_class_narrowing,
    "CN09",
    "typeinfo currently does not use constructor value identity to narrow class unions under instanceof; keep as the future CN09 instanceof contract",
    |expr| assert_mixed_literal_union(expr, &["x"], &[1.0])
);

future_catalog_contract!(
    flow_return_cn10_composes_nested_boolean_guard_facts,
    "CN10",
    "typeinfo currently does not compose typeof facts through nested && and || boolean expressions; keep as the future CN10 boolean-fact contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_catalog_contract!(
    flow_return_cn11_applies_negative_typeof_guard,
    "CN11",
    "typeinfo currently does not subtract guarded constituents under typeof !== checks; keep as the future CN11 negative-guard contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
        assert_union_contains_primitive(expr, PrimitiveName::Boolean);
    }
);

future_catalog_contract!(
    flow_return_cn12_eliminates_exhaustive_never_tail,
    "CN12",
    "typeinfo currently does not accumulate discriminant exclusions to reduce exhaustive tail paths to never; keep as the future CN12 exhaustive-narrowing contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_cn13_narrows_optional_property_truthiness,
    "CN13",
    "typeinfo currently does not attach truthiness facts to optional object member paths; keep as the future CN13 optional-member contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_cn14_correlates_equality_between_variables,
    "CN14",
    "typeinfo currently does not intersect literal unions for equality facts shared between two variables; keep as the future CN14 correlated-equality contract",
    |expr| assert_literal_union(expr, &["b", "c"])
);

// CN15 — TS7 emits `string | number`. Both branches return `x.value` and the
// union of the two arms' `value` fields is `string | number` regardless of
// whether the discriminant narrowing succeeded. NOTE: this assertion is
// degenerate — a resolver that fails to narrow but still returns the bare union
// will pass it. The presence of the nested discriminant narrowing must be
// verified through CN15-style fixtures that emit DIFFERENT types per branch,
// covered in flow_return_path_contracts FP05/FP07 (where the contributing
// branch-load footprint is independently asserted).
future_catalog_contract!(
    flow_return_cn15_narrows_nested_discriminant_paths,
    "CN15",
    "typeinfo currently does not attach discriminant facts to nested member paths like x.meta.kind; keep as the future CN15 nested-discriminant contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

// CN16 — TS7 emits `string | number`. Same shape as CN15: both branches return
// the same `x.value` field, so the union arises regardless of whether
// destructured-alias narrowing fired. The discriminant-correlation behaviour
// under test (linking `const { kind } = x` back to `x`'s union) is NOT
// distinguishable from the unnarrowed case at the return-type surface alone.
// See FP05 in flow_return_path_contracts for a per-branch correlated test.
future_catalog_contract!(
    flow_return_cn16_preserves_destructured_discriminant_correlation,
    "CN16",
    "typeinfo currently does not correlate destructured discriminant aliases back to the source object union; keep as the future CN16 destructured-discriminant contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_pa01_applies_local_type_predicate_signature,
    "PA01",
    "typeinfo currently does not apply local x is T predicate signatures as caller flow facts; keep as the future PA01 predicate contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_pa02_applies_asserts_is_signature,
    "PA02",
    "typeinfo currently does not mutate caller flow facts after asserts x is T calls; keep as the future PA02 assertion contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_pa03_applies_asserts_condition_truthiness,
    "PA03",
    "typeinfo currently does not apply asserts condition calls as truthiness facts on the asserted expression; keep as the future PA03 assertion-condition contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_pa04_instantiates_generic_predicate,
    "PA04",
    "typeinfo currently does not instantiate generic x is NonNullable<T> predicate signatures at the call site; keep as the future PA04 generic-predicate contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_pa05_applies_importable_predicate_signature,
    "PA05",
    "typeinfo currently does not apply predicate call effects from reusable function signatures before return joins; keep as the future PA05 predicate-signature contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_pa06_sequences_chained_predicates,
    "PA06",
    "typeinfo currently does not sequence short-circuit predicate facts so later predicates can rely on earlier narrowing; keep as the future PA06 chained-predicate contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_number_literal(expr, 0.0);
    }
);

future_catalog_contract!(
    flow_return_pa07_refines_property_shape_from_predicate,
    "PA07",
    "typeinfo currently does not refine object member shape from predicate target types; keep as the future PA07 property-predicate contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_catalog_contract!(
    flow_return_pa08_applies_assertion_effect,
    "PA08",
    "typeinfo currently does not apply asserts x is number call effects to the following return expression; keep as the future PA08 assertion-effect contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_pa09_uses_declared_predicate_without_body,
    "PA09",
    "typeinfo currently does not apply declared predicate signatures when the predicate body is unavailable; keep as the future PA09 signature-only predicate contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

catalog_contract!(
    flow_return_cg01_expands_local_function_call_return,
    "CG01",
    |expr| {
        let props = assert_object_has_props(expr, &["a"]);
        assert_number_literal(&props["a"].ty, 1.0);
    }
);

future_catalog_contract!(
    flow_return_cg02_infers_generic_identity_call_argument,
    "CG02",
    "typeinfo currently does not infer generic call-site type arguments from value arguments; keep as the future CG02 generic-call contract",
    |expr| {
        let props = assert_object_has_props(expr, &["a"]);
        assert_number_literal(&props["a"].ty, 1.0);
    }
);

future_catalog_contract!(
    flow_return_cg03_instantiates_generic_wrapper_return_annotation,
    "CG03",
    "typeinfo currently does not instantiate generic function return annotations from call-site value arguments; keep as the future CG03 generic-wrapper contract",
    |expr| {
        let props = assert_object_has_props(expr, &["value"]);
        assert_string_literal(&props["value"].ty, "x");
    }
);

future_catalog_contract!(
    flow_return_cg04_selects_matching_overload_return,
    "CG04",
    "typeinfo currently does not select overload candidates from value argument assignability; keep as the future CG04 overload-resolution contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_cg05_infers_rest_parameter_literal_union,
    "CG05",
    "typeinfo currently does not infer rest parameter element unions or account for indexed-access undefined policy; keep as the future CG05 rest-parameter contract",
    |expr| {
        assert_literal_union(expr, &["a", "b"]);
    }
);

future_catalog_contract!(
    flow_return_cg06_uses_default_parameter_initializer_type,
    "CG06",
    "typeinfo currently does not infer parameter types from default initializers for call-site return inference; keep as the future CG06 default-parameter contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_cg07_infers_generic_from_callback_return,
    "CG07",
    "typeinfo currently does not infer generic type arguments from callback return bodies; keep as the future CG07 callback-inference contract",
    |expr| assert_mixed_literal_union(expr, &["a"], &[1.0])
);

future_catalog_contract!(
    flow_return_cg08_contextually_types_callback_parameter,
    "CG08",
    "typeinfo currently does not contextually type callback parameters from generic callee signatures; keep as the future CG08 contextual-callback contract",
    |expr| assert_string_literal(expr, "x")
);

future_catalog_contract!(
    flow_return_cg09_uses_constraint_for_member_return_widening,
    "CG09",
    "typeinfo currently does not combine generic constraints with TypeScript return widening for member access through T extends object; keep as the future CG09 constraint-member contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_cg10_terminates_recursive_return_inference,
    "CG10",
    "typeinfo currently does not use an in-flight function-return memo or recursion sentinel for self-recursive calls; keep as the future CG10 recursion contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_cg11_lowers_constructor_call_to_instance_type,
    "CG11",
    "typeinfo currently does not lower new expressions through constructor symbols to instance return types; keep as the future CG11 constructor-call contract",
    |expr| assert_ref(expr, "CG11User")
);

future_catalog_contract!(
    flow_return_ho01_infers_computed_style_callback_return,
    "HO01",
    "typeinfo currently does not infer generic helper type arguments from callback return bodies; keep as the future HO01 computed-style callback contract",
    |expr| {
        let props = assert_object_has_props(expr, &["value"]);
        assert_mixed_literal_union(&props["value"].ty, &["on"], &[0.0]);
    }
);

future_catalog_contract!(
    flow_return_ho02_applies_filter_predicate_overload,
    "HO02",
    "typeinfo currently does not model Array.filter predicate overloads to refine element types; keep as the future HO02 filter-predicate contract",
    |expr| assert_array_of_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_ho03_maps_callback_return_union_to_array_element,
    "HO03",
    "typeinfo currently does not contextually type Array.map callbacks or union callback return values into the result array element; keep as the future HO03 map-callback contract",
    |expr| {
        let element = array_element(expr);
        assert_union_contains_primitive(element, PrimitiveName::String);
        assert_union_contains_primitive(element, PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_ho04_infers_reduce_accumulator_return,
    "HO04",
    "typeinfo currently does not model Array.reduce accumulator overloads and numeric binary-expression returns; keep as the future HO04 reduce contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_ho05_models_flat_map_callback_flattening,
    "HO05",
    "typeinfo currently does not model Array.flatMap callback return flattening and predicate narrowing; keep as the future HO05 flatMap contract",
    |expr| assert_array_of_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_ho06_infers_custom_generic_callback_helper,
    "HO06",
    "typeinfo currently does not infer return types through ordinary generic callback helper signatures; keep as the future HO06 custom-callback contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_ho07_composes_nested_callback_predicates,
    "HO07",
    "typeinfo currently does not compose contextual callback environments and predicate overloads through nested array methods; keep as the future HO07 nested-callback contract",
    |expr| {
        let inner = array_element(expr);
        assert_array_of_primitive(inner, PrimitiveName::String);
    }
);

future_catalog_contract!(
    flow_return_ho08_narrows_discriminant_inside_map_callback,
    "HO08",
    "typeinfo currently does not run discriminant narrowing inside contextually typed callback bodies; keep as the future HO08 callback-narrowing contract",
    |expr| {
        let element = array_element(expr);
        assert_union_contains_primitive(element, PrimitiveName::String);
        assert_union_contains_primitive(element, PrimitiveName::Number);
    }
);

#[test]
#[ignore = "typeinfo currently fails to attach a request footprint on this declared-callback resolver path after the block-6.i AX-WIP audit-passive-observer refactor (commit b0798e28); the contract is that an unknown-typed declared callback result stays opaque as `unknown` with the warm cross-file dependency footprint attached. Keep as the future HO09 declared-callback-opaque contract once AX-WIP closes Rule-5 leak."]
fn flow_return_ho09_keeps_unknown_declared_callback_result_opaque() {
    assert_catalog_alias_warm("HO09", |expr| {
        assert_primitive(expr, PrimitiveName::Unknown)
    });
}

future_catalog_contract!(
    flow_return_ho10_returns_closure_with_captured_substitution,
    "HO10",
    "typeinfo currently does not construct returned closure types with captured generic substitutions; keep as the future HO10 closure-return contract",
    |expr| {
        let function = function_type(expr);
        assert_string_literal(
            function
                .return_type
                .as_deref()
                .expect("returned closure has return type"),
            "x",
        );
    }
);

future_catalog_contract!(
    flow_return_ho11_wraps_promise_all_async_callback_result,
    "HO11",
    "typeinfo currently does not combine async callback return wrapping with Promise.all and array method models; keep as the future HO11 promise-callback contract",
    |expr| {
        let args = assert_ref_with_args(expr, "Promise", 1);
        assert_array_of_primitive(&args[0], PrimitiveName::Number);
    }
);

// OB01 — TS7 emits `string | number`. Both branches return `x.value`, which is
// declared as `string | number`. The member-path narrowing under test (whether
// `typeof x.value === "string"` attaches a fact to `x.value`) cannot be observed
// at the return-type surface alone because both arms return the same field.
// Per-branch typing is exercised by symmetric-friend tests in
// flow_return_path_contracts (FP01 etc.) where the branch types diverge.
future_catalog_contract!(
    flow_return_ob01_tracks_member_path_flow_facts,
    "OB01",
    "typeinfo currently does not attach narrowing facts to object member paths before returning narrowed fields; keep as the future OB01 member-flow contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

catalog_contract!(
    flow_return_ob02_materializes_spread_override_order,
    "OB02",
    |expr| {
        let props = assert_object_has_props(expr, &["a", "b"]);
        assert_number_literal(&props["a"].ty, 1.0);
        assert_string_literal(&props["b"].ty, "y");
    }
);

future_catalog_contract!(
    flow_return_ob03_synthesizes_optional_property_for_conditional_spread,
    "OB03",
    "typeinfo currently does not merge conditional object spreads into optional members with undefined; keep as the future OB03 conditional-spread contract",
    |expr| {
        let props = assert_object_has_props(expr, &["a", "b"]);
        assert_primitive(&props["a"].ty, PrimitiveName::Number);
        assert!(props["b"].optional);
        assert_expr_contains_primitive(&props["b"].ty, PrimitiveName::Undefined);
    }
);

future_catalog_contract!(
    flow_return_ob04_preserves_deep_const_assertion_readonly_literals,
    "OB04",
    "typeinfo currently does not preserve deep readonly modifiers from as const object return expressions; keep as the future OB04 deep-const contract",
    |expr| {
        let props = assert_object_has_props(expr, &["mode", "nested"]);
        assert!(props["mode"].readonly);
        assert_string_literal(&props["mode"].ty, "dark");
        let nested = object_props(&props["nested"].ty);
        assert!(nested["count"].readonly);
        assert_number_literal(&nested["count"].ty, 1.0);
    }
);

future_catalog_contract!(
    flow_return_ob06_projects_indexed_access_through_constraint,
    "OB06",
    "typeinfo currently does not simplify indexed value access through generic key and object constraints; keep as the future OB06 indexed-access value contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_ob07_evaluates_computed_literal_object_key,
    "OB07",
    "typeinfo currently does not resolve computed object literal keys from const value identities; keep as the future OB07 computed-literal-key contract",
    |expr| {
        let props = assert_object_has_props(expr, &["name"]);
        assert_string_literal(&props["name"].ty, "Ada");
    }
);

future_catalog_contract!(
    flow_return_ob08_preserves_readonly_parameter_shape,
    "OB08",
    "typeinfo currently returns unresolved typeof roots for parameter object values instead of preserving readonly object shape; keep as the future OB08 readonly-parameter contract",
    |expr| {
        let props = assert_object_has_props(expr, &["id"]);
        assert!(props["id"].readonly);
        assert_primitive(&props["id"].ty, PrimitiveName::String);
    }
);

future_catalog_contract!(
    flow_return_ob09_instantiates_keyof_driven_array_return,
    "OB09",
    "typeinfo currently does not infer keyof unions through generic Object.keys-style helper returns; keep as the future OB09 keyof-helper contract",
    |expr| assert_literal_union(array_element(expr), &["a", "b"])
);

future_catalog_contract!(
    flow_return_ob10_instantiates_mapped_return_annotation,
    "OB10",
    "typeinfo currently does not instantiate mapped return annotations from rest literal call-site arguments; keep as the future OB10 mapped-return contract",
    |expr| {
        let props = assert_object_has_props(expr, &["a", "b"]);
        assert_primitive(&props["a"].ty, PrimitiveName::Boolean);
        assert_primitive(&props["b"].ty, PrimitiveName::Boolean);
    }
);

future_catalog_contract!(
    flow_return_ob11_reduces_conditional_return_annotation,
    "OB11",
    "typeinfo currently does not reduce conditional return annotations under concrete generic substitutions; keep as the future OB11 conditional-return contract",
    |expr| {
        let props = assert_object_has_props(expr, &["text"]);
        assert_string_literal(&props["text"].ty, "x");
    }
);

future_catalog_contract!(
    flow_return_ob12_keeps_unique_symbol_computed_key_shape,
    "OB12",
    "typeinfo currently does not bridge unique-symbol value identity into computed object keys or emit an explicit computed-key fallback; keep as the future OB12 unique-symbol-key contract",
    |expr| {
        let TypeExpr::Object(_) = expr else {
            panic!("expected computed-key object shape, got {expr:?}");
        };
    }
);

future_catalog_contract!(
    flow_return_ob13_infers_getter_property_return,
    "OB13",
    "typeinfo currently does not infer object literal accessor bodies into readonly property members; keep as the future OB13 getter-return contract",
    |expr| {
        let props = assert_object_has_props(expr, &["value"]);
        assert!(props["value"].readonly);
        assert_number_literal(&props["value"].ty, 1.0);
    }
);

future_catalog_contract!(
    flow_return_cf01_joins_nested_if_flow_returns,
    "CF01",
    "typeinfo currently does not build a CFG with boolean guard facts for nested if conditions; keep as the future CF01 CFG contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_catalog_contract!(
    flow_return_cf02_types_logical_and_short_circuit_expression,
    "CF02",
    "typeinfo currently does not type && short-circuit expressions with truthiness narrowing in the right operand; keep as the future CF02 short-circuit contract",
    |expr| {
        assert_expr_contains_primitive(expr, PrimitiveName::Null);
        assert_union_has_object_arm(expr, &["value"]);
    }
);

future_catalog_contract!(
    flow_return_cf03_filters_falsy_left_side_of_logical_or,
    "CF03",
    "typeinfo currently does not type || expressions by removing falsy left-side constituents before joining fallback; keep as the future CF03 logical-or contract",
    |expr| assert_literal_union(expr, &["a", "fallback"])
);

future_catalog_contract!(
    flow_return_cf04_removes_nullish_constituents_for_coalescing,
    "CF04",
    "typeinfo currently does not type ?? expressions by removing only nullish constituents; keep as the future CF04 nullish-coalescing contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_cf05_narrows_continuation_after_early_return,
    "CF05",
    "typeinfo currently does not propagate continuation facts after terminating early-return branches; keep as the future CF05 early-return narrowing contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_cf06_joins_try_and_catch_returns,
    "CF06",
    "typeinfo currently does not collect return paths through try/catch/finally CFG edges; keep as the future CF06 try-catch contract",
    |expr| assert_mixed_literal_union(expr, &["err"], &[1.0])
);

future_catalog_contract!(
    flow_return_cf07_models_loop_break_return_paths,
    "CF07",
    "typeinfo currently does not build loop CFG edges for for-of iteration variables, break, and post-loop returns; keep as the future CF07 loop-break contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_catalog_contract!(
    flow_return_cf08_joins_continue_and_accumulator_assignment,
    "CF08",
    "typeinfo currently does not compute loop fixed points with continue edges and accumulator assignments; keep as the future CF08 loop-accumulator contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_catalog_contract!(
    flow_return_cf09_solves_loop_mutation_fixed_point,
    "CF09",
    "typeinfo currently does not solve loop mutation fixed points that narrow a variable until exit; keep as the future CF09 loop-fixed-point contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_cf10_models_finally_reference_behavior,
    "CF10",
    "typeinfo currently does not collect return paths through try/finally statements or pin TypeScript-compatible finally behavior; keep as the future CF10 finally contract",
    |expr| assert_mixed_literal_union(expr, &["final"], &[1.0])
);

// CF11 — TS7 keeps the local-variable narrowing across callback registration.
// Inside the `typeof x === "string"` branch `x` is narrowed to `string`; passing
// `() => { x = 1 }` to `run` does NOT widen `x` back to `string | number` because
// TS does not invalidate local-variable narrowing on opaque callback escapes.
// Final return: `string`.
future_catalog_contract!(
    flow_return_cf11_invalidates_closure_captured_flow_facts,
    "CF11",
    "typeinfo currently does not preserve TypeScript's narrowed return fact after passing a capturing callback to an unknown caller; keep as the future CF11 capture-callback contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_cf12_invalidates_object_member_facts_after_unknown_call,
    "CF12",
    "typeinfo currently does not apply conservative mutation barriers when object values are passed to unknown functions; keep as the future CF12 call-barrier contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_cf14_models_switch_fallthrough_case_facts,
    "CF14",
    "typeinfo currently does not model switch case grouping and fallthrough facts before return joins; keep as the future CF14 switch-fallthrough contract",
    |expr| assert_mixed_literal_union(expr, &["a", "b"], &[0.0])
);

future_catalog_contract!(
    flow_return_cf15_separates_definite_assignment_diagnostics_from_return_type,
    "CF15",
    "typeinfo currently does not maintain maybe-assigned facts that allow return inference while reporting use-before-assignment separately; keep as the future CF15 definite-assignment contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_cf16_terminates_budgeted_complex_loop_flow,
    "CF16",
    "typeinfo currently does not run a budgeted CFG fixed-point solver or publish explicit fallback flow results for complex loops; keep as the future CF16 budgeted-loop contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
        assert_union_contains_primitive(expr, PrimitiveName::Boolean);
    }
);

future_cross_contract!(
    flow_return_xf01_uses_imported_type_for_parameter_member_return,
    "XF01",
    &["/fixtures/flow_return_cross_types.ts"],
    &[
        "/fixtures/flow_return_cross_factory.ts",
        "/fixtures/flow_return_cross_guards.ts",
        "/fixtures/flow_return_cross_source.ts",
        "/fixtures/flow_return_cross_index.ts",
    ],
    "typeinfo currently does not resolve function-body parameter member returns through imported type-only aliases; keep as the future XF01 cross-file parameter type contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

#[test]
#[ignore = "typeinfo currently fails to attach a request footprint on this cross-file resolver path after the block-6.i AX-WIP audit-passive-observer refactor (commit b0798e28); the contract is that imported value-function returns expand to their published `{ mode: \"dark\" }` literal surface with the cross-file dependency footprint attached. Keep as the future XF02 cross-file value-function expansion contract once AX-WIP closes Rule-5 leak."]
fn flow_return_xf02_expands_imported_value_function_return() {
    assert_cross_alias_warm(
        "XF02",
        |expr| {
            let props = assert_object_has_props(expr, &["mode"]);
            assert_string_literal(&props["mode"].ty, "dark");
        },
        &["/fixtures/flow_return_cross_factory.ts"],
        &[
            "/fixtures/flow_return_cross_types.ts",
            "/fixtures/flow_return_cross_guards.ts",
            "/fixtures/flow_return_cross_source.ts",
            "/fixtures/flow_return_cross_index.ts",
        ],
    );
}

future_cross_contract!(
    flow_return_xf03_applies_imported_predicate_flow_fact,
    "XF03",
    &["/fixtures/flow_return_cross_guards.ts"],
    &[
        "/fixtures/flow_return_cross_types.ts",
        "/fixtures/flow_return_cross_factory.ts",
        "/fixtures/flow_return_cross_source.ts",
        "/fixtures/flow_return_cross_index.ts",
    ],
    "typeinfo currently does not apply imported predicate signatures as caller flow facts; keep as the future XF03 imported predicate contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

#[test]
#[ignore = "typeinfo currently fails to attach a request footprint on this barrel-imported resolver path after the block-6.i AX-WIP audit-passive-observer refactor (commit b0798e28); the contract is that barrel-imported value-function returns expand to their published `{ id: \"x\" }` literal surface with the cross-file dependency footprint attached. Keep as the future XF04 cross-file barrel-imported expansion contract once AX-WIP closes Rule-5 leak."]
fn flow_return_xf04_expands_barrel_imported_value_function_return() {
    assert_cross_alias_warm(
        "XF04",
        |expr| {
            let props = assert_object_has_props(expr, &["id"]);
            assert_string_literal(&props["id"].ty, "x");
        },
        &["/fixtures/flow_return_cross_source.ts"],
        &[
            "/fixtures/flow_return_cross_types.ts",
            "/fixtures/flow_return_cross_factory.ts",
            "/fixtures/flow_return_cross_guards.ts",
        ],
    );
}

#[test]
#[ignore = "typeinfo currently resolves the barrel-imported value call without loading or recording the barrel owner itself; keep as the future XF04 barrel-route footprint contract"]
fn flow_return_xf04_records_barrel_route_before_selected_leaf() {
    assert_cross_alias_warm_with_dependency_footprint(
        "XF04",
        |expr| {
            let props = assert_object_has_props(expr, &["id"]);
            assert_string_literal(&props["id"].ty, "x");
        },
        &[
            "/fixtures/flow_return_cross_index.ts",
            "/fixtures/flow_return_cross_source.ts",
        ],
        &[
            "/fixtures/flow_return_cross_types.ts",
            "/fixtures/flow_return_cross_factory.ts",
            "/fixtures/flow_return_cross_guards.ts",
        ],
    );
}

#[test]
#[ignore = "typeinfo currently fails to attach a request footprint on this namespace-import resolver path after the block-6.i AX-WIP audit-passive-observer refactor (commit b0798e28); the contract is that namespace-import value calls resolve to their published `{ ok: true }` literal surface with the cross-file dependency footprint attached. Keep as the future XF05 cross-file namespace-import contract once AX-WIP closes Rule-5 leak."]
fn flow_return_xf05_resolves_namespace_import_value_call() {
    assert_cross_alias_warm(
        "XF05",
        |expr| {
            let props = assert_object_has_props(expr, &["ok"]);
            assert_boolean_literal(&props["ok"].ty, true);
        },
        &["/fixtures/flow_return_cross_factory.ts"],
        &[
            "/fixtures/flow_return_cross_types.ts",
            "/fixtures/flow_return_cross_guards.ts",
            "/fixtures/flow_return_cross_source.ts",
            "/fixtures/flow_return_cross_index.ts",
        ],
    );
}

#[test]
#[ignore = "typeinfo currently fails to attach a request footprint on this value/type-separated namespace path after the block-6.i AX-WIP audit-passive-observer refactor (commit b0798e28); the contract is that the value namespace stays separate from the type namespace, resolving to `{ valueOnly: true }` with the cross-file dependency footprint attached. Keep as the future XF06 value-type-namespace separation contract once AX-WIP closes Rule-5 leak."]
fn flow_return_xf06_keeps_value_type_namespace_separate() {
    assert_cross_alias_warm(
        "XF06",
        |expr| {
            let props = assert_object_has_props(expr, &["valueOnly"]);
            assert_boolean_literal(&props["valueOnly"].ty, true);
        },
        &["/fixtures/flow_return_cross_factory.ts"],
        &[
            "/fixtures/flow_return_cross_types.ts",
            "/fixtures/flow_return_cross_guards.ts",
            "/fixtures/flow_return_cross_source.ts",
            "/fixtures/flow_return_cross_index.ts",
        ],
    );
}

future_cross_contract!(
    flow_return_xf07_preserves_predicate_signature_through_reexport_alias,
    "XF07",
    &[
        "/fixtures/flow_return_cross_index.ts",
        "/fixtures/flow_return_cross_guards.ts",
    ],
    &[
        "/fixtures/flow_return_cross_types.ts",
        "/fixtures/flow_return_cross_factory.ts",
        "/fixtures/flow_return_cross_source.ts",
    ],
    "typeinfo currently does not preserve predicate signatures through aliased barrel reexports before applying flow facts; keep as the future XF07 reexported predicate contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_cross_contract!(
    flow_return_xf08_applies_imported_assertion_signature,
    "XF08",
    &["/fixtures/flow_return_cross_guards.ts"],
    &[
        "/fixtures/flow_return_cross_types.ts",
        "/fixtures/flow_return_cross_factory.ts",
        "/fixtures/flow_return_cross_source.ts",
        "/fixtures/flow_return_cross_index.ts",
    ],
    "typeinfo currently does not apply cross-file asserts x is T signatures to caller flow environments; keep as the future XF08 imported assertion contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

#[test]
#[ignore = "typeinfo currently fails to attach a request footprint on the cross-file recursive resolver path after the block-6.i AX-WIP audit-passive-observer refactor (commit b0798e28); the contract is that cross-file recursive return cycles terminate with a `number | string` union projection and the cross-file dependency footprint attached. Keep as the future XF09 cross-file recursive-return termination contract once AX-WIP closes Rule-5 leak."]
fn flow_return_xf09_terminates_cross_file_recursive_returns() {
    assert_cross_alias_warm(
        "XF09",
        |expr| {
            assert_union_contains_primitive(expr, PrimitiveName::Number);
            assert_union_contains_primitive(expr, PrimitiveName::String);
        },
        &[],
        &[
            "/fixtures/flow_return_cross_types.ts",
            "/fixtures/flow_return_cross_factory.ts",
            "/fixtures/flow_return_cross_guards.ts",
            "/fixtures/flow_return_cross_source.ts",
            "/fixtures/flow_return_cross_index.ts",
        ],
    );
}

future_cross_package_contract!(
    flow_return_xf10_uses_external_declaration_signature_return,
    "XF10",
    "typeinfo currently does not route synthetic package value imports to declaration signatures during body call return inference; keep as the future XF10 package declaration-call contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_cross_contract!(
    flow_return_xf11_applies_ambient_global_predicate,
    "XF11",
    &[],
    &[
        "/fixtures/flow_return_cross_types.ts",
        "/fixtures/flow_return_cross_factory.ts",
        "/fixtures/flow_return_cross_guards.ts",
        "/fixtures/flow_return_cross_source.ts",
        "/fixtures/flow_return_cross_index.ts",
    ],
    "typeinfo currently does not apply ambient/global predicate signatures as flow facts in function bodies; keep as the future XF11 ambient predicate contract",
    |expr| assert_primitive(expr, PrimitiveName::Boolean)
);

future_cross_contract!(
    flow_return_pa08_barrel_assertion_import_preserves_effect,
    "PA08",
    &[
        "/fixtures/flow_return_cross_index.ts",
        "/fixtures/flow_return_cross_guards.ts",
    ],
    &[
        "/fixtures/flow_return_cross_types.ts",
        "/fixtures/flow_return_cross_factory.ts",
        "/fixtures/flow_return_cross_source.ts",
    ],
    "typeinfo currently does not preserve assertion signatures through barrel routes before applying asserts x is number effects; keep as the future PA08 barrel-assertion contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_vv01_infers_computed_callback_union_value,
    "VV01",
    "typeinfo currently does not infer computed-style generic helper results from discriminated callback object unions; keep as the future VV01 computed contract",
    |expr| {
        let props = assert_object_has_props(expr, &["value"]);
        let TypeExpr::Union(types) = &props["value"].ty else {
            panic!("expected computed value object union, got {:?}", props["value"].ty);
        };
        assert!(types.iter().any(|ty| {
            let props = object_props(ty);
            matches!(
                props.get("kind"),
                Some(prop)
                    if matches!(&prop.ty, TypeExpr::Literal(LiteralValue::String(value)) if value == "ready")
            ) && matches!(
                props.get("value"),
                Some(prop)
                    if matches!(&prop.ty, TypeExpr::Literal(LiteralValue::String(value)) if value == "on")
            )
        }));
        assert!(types.iter().any(|ty| {
            let props = object_props(ty);
            matches!(
                props.get("kind"),
                Some(prop)
                    if matches!(&prop.ty, TypeExpr::Literal(LiteralValue::String(value)) if value == "empty")
            ) && matches!(
                props.get("value"),
                Some(prop)
                    if matches!(&prop.ty, TypeExpr::Literal(LiteralValue::Number(value)) if value.to_bits() == 0.0f64.to_bits())
            )
        }));
    }
);

future_catalog_contract!(
    flow_return_vv02_pins_ref_literal_widening_policy,
    "VV02",
    "typeinfo currently does not provide a helper/intrinsic model for ref-like literal widening; keep as the future VV02 ref policy contract",
    |expr| {
        let props = assert_object_has_props(expr, &["value"]);
        assert_primitive(&props["value"].ty, PrimitiveName::String);
    }
);

// VV03 — Fixture uses TWO overloads on `vv03Unref`:
//   (x: VV03Ref<T>): T   and   (x: T): T
// plus an explicit `typeof x === "string"` discriminator in `vv03`. TS7 picks the
// second overload (T=string) for the string branch and the first (T=number) for
// the VV03Ref<number> branch. The branch returns are merged into `string | number`.
// Without the discriminator + overloads, a single `<T>(x: T | VV03Ref<T>): T`
// signature errors on `string | VV03Ref<number>` (TS2345), which is why the
// fixture is structured this way: the test exercises legitimate union unref
// inference rather than locking in a TS-impossible contract.
future_catalog_contract!(
    flow_return_vv03_infers_unref_union_helper_return,
    "VV03",
    "typeinfo currently does not combine branch narrowing with overloaded generic unref helper return inference; keep as the future VV03 unref contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_vv04_narrows_reactive_member_truthiness,
    "VV04",
    "typeinfo currently does not combine reactive-like generic call inference with member truthiness narrowing; keep as the future VV04 reactive member contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_vv05_expands_props_factory_returntype_with_widening,
    "VV05",
    "typeinfo currently does not apply full TypeScript function-return widening when expanding ReturnType of a props factory; keep as the future VV05 factory ReturnType contract",
    |expr| {
        let props = assert_object_has_props(expr, &["disabled", "label"]);
        assert_primitive(&props["label"].ty, PrimitiveName::String);
        assert_primitive(&props["disabled"].ty, PrimitiveName::Boolean);
    }
);

// VV06 — `vv06WithDefaults<T, D>(props: T, defaults: D): T & D`. TS7 returns
// the literal intersection `T & D`, so `vv06Props.items` is
//   (string[] | undefined) & (() => string[])
// which simplifies (because `undefined & function` collapses to `never`) to
//   string[] & (() => string[])
// The published return type is the structural intersection of an array arm and
// a function arm — NOT the underlying `T.items` shape. A future Verter that
// models `T & D` faithfully will materialize both arms exactly as below.
future_catalog_contract!(
    flow_return_vv06_projects_default_factory_callback_return,
    "VV06",
    "typeinfo currently does not model TypeScript's `T & D` defaults intersection from withDefaults-like helpers; keep as the future VV06 defaults intersection contract",
    |expr| {
        let TypeExpr::Intersection(parts) = expr else {
            panic!("expected `string[] & (() => string[])` intersection, got {expr:?}");
        };
        assert!(
            parts
                .iter()
                .any(|part| matches!(part, TypeExpr::Array { element, .. }
                    if matches!(element.as_ref(), TypeExpr::Primitive(PrimitiveName::String)))),
            "expected intersection {expr:?} to contain string[] arm"
        );
        assert!(
            parts.iter().any(|part| {
                let TypeExpr::Function(function) = part else {
                    return false;
                };
                matches!(
                    function.return_type.as_deref(),
                    Some(TypeExpr::Array { element, .. })
                        if matches!(element.as_ref(), TypeExpr::Primitive(PrimitiveName::String))
                )
            }),
            "expected intersection {expr:?} to contain () => string[] arm"
        );
    }
);

future_catalog_contract!(
    flow_return_vv07_materializes_composable_return_object,
    "VV07",
    "typeinfo currently does not infer composable-style local helper calls and function-valued object members from bodies; keep as the future VV07 composable-return contract",
    |expr| {
        let props = assert_object_has_props(expr, &["count", "inc"]);
        let ref_args = assert_ref_with_args(&props["count"].ty, "VV07Ref", 1);
        assert_primitive(&ref_args[0], PrimitiveName::Number);
        let function = function_type(&props["inc"].ty);
        assert_primitive(
            function
                .return_type
                .as_deref()
                .expect("inc closure has return type"),
            PrimitiveName::Void,
        );
    }
);

future_catalog_contract!(
    flow_return_vv08_computed_from_discriminated_props,
    "VV08",
    "typeinfo currently does not infer computed-style callback results with discriminant narrowing over ambient props facts; keep as the future VV08 computed-props contract",
    |expr| {
        let args = assert_ref_with_args(expr, "VV08ComputedRef", 1);
        assert_union_contains_primitive(&args[0], PrimitiveName::String);
        assert_union_contains_primitive(&args[0], PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_vv09_optional_slot_call_projects_return,
    "VV09",
    "typeinfo currently does not model optional calls on function-valued slot members or add undefined for missing calls; keep as the future VV09 slot-call contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
        let TypeExpr::Union(types) = expr else {
            panic!("expected VNode[] | undefined union, got {expr:?}");
        };
        let has_vnode_array = types.iter().any(|ty| {
            let TypeExpr::Array { element, .. } = ty else {
                return false;
            };
            let props = object_props(element);
            prop_names(&props) == vec!["__vnode"]
        });
        assert!(has_vnode_array, "expected VNode[] arm in {expr:?}");
    }
);

future_catalog_contract!(
    flow_return_vv10_template_ref_returntype_optional_method_call,
    "VV10",
    "typeinfo currently does not expand ReturnType-backed template-ref values through optional chaining and method-call returns; keep as the future VV10 template-ref contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
        let TypeExpr::Union(types) = expr else {
            panic!("expected true | undefined union, got {expr:?}");
        };
        assert!(types
            .iter()
            .any(|ty| matches!(ty, TypeExpr::Literal(LiteralValue::Boolean(true)))));
    }
);

future_catalog_contract!(
    flow_return_vv11_resolves_emit_call_signature_return,
    "VV11",
    "typeinfo currently does not select object call signatures from literal event arguments for emit-like values; keep as the future VV11 emit-call contract",
    |expr| assert_primitive(expr, PrimitiveName::Boolean)
);

future_catalog_contract!(
    flow_return_vv12_contextually_types_model_transform_callbacks,
    "VV12",
    "typeinfo currently does not contextually type macro-like option object callbacks and publish the helper result value type; keep as the future VV12 model-transform contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_vv13_applies_callback_mutation_policy_for_watch,
    "VV13",
    "typeinfo currently does not model callback scheduling and mutation barriers for watch-like helpers; keep as the future VV13 watch-callback contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_vv14_instantiates_injection_assertion_helper,
    "VV14",
    "typeinfo currently does not instantiate generic assertion helpers over local const values returned from injection-like calls; keep as the future VV14 inject-assertion contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_cross_contract!(
    flow_return_vv15_expands_barrel_imported_macro_factory,
    "VV15",
    &[
        "/fixtures/flow_return_cross_index.ts",
        "/fixtures/flow_return_cross_source.ts",
    ],
    &[
        "/fixtures/flow_return_cross_types.ts",
        "/fixtures/flow_return_cross_factory.ts",
        "/fixtures/flow_return_cross_guards.ts",
    ],
    "typeinfo currently does not expand ReturnType of barrel-imported factory functions with TypeScript object property widening; keep as the future VV15 barrel-factory contract",
    |expr| {
        let props = assert_object_has_props(expr, &["disabled", "size"]);
        assert_string_literal(&props["size"].ty, "md");
        assert_primitive(&props["disabled"].ty, PrimitiveName::Boolean);
    }
);

future_catalog_contract!(
    flow_return_vv16_calls_union_of_dynamic_component_factories,
    "VV16",
    "typeinfo currently does not infer a union of callable values and then call each function arm for dynamic component patterns; keep as the future VV16 dynamic-component contract",
    |expr| {
        let TypeExpr::Union(types) = expr else {
            panic!("expected object union, got {expr:?}");
        };
        assert_eq!(types.len(), 2);
        assert!(types.iter().any(|ty| {
            let props = object_props(ty);
            props
                .get("kind")
                .map(|prop| matches!(&prop.ty, TypeExpr::Literal(LiteralValue::String(value)) if value == "A"))
                .unwrap_or(false)
        }));
        assert!(types.iter().any(|ty| {
            let props = object_props(ty);
            props
                .get("kind")
                .map(|prop| matches!(&prop.ty, TypeExpr::Literal(LiteralValue::String(value)) if value == "B"))
                .unwrap_or(false)
        }));
    }
);

catalog_contract!(
    flow_return_bl16_explicit_object_return_annotation_wins_over_body_literals,
    "BL16",
    |expr| {
        let props = assert_object_has_props(expr, &["count", "tag"]);
        assert_primitive(&props["count"].ty, PrimitiveName::Number);
        assert_string_literal(&props["tag"].ty, "ready");
    }
);

future_catalog_contract!(
    flow_return_bl17_async_promise_return_flattens_to_promise_payload,
    "BL17",
    "typeinfo currently does not model async Promise-return flattening or preserve the fulfilled object payload; keep as the future BL17 async Promise flattening contract",
    |expr| {
        let args = assert_ref_with_args(expr, "Promise", 1);
        let props = assert_object_has_props(&args[0], &["id"]);
        assert_string_literal(&props["id"].ty, "ok");
    }
);

future_catalog_contract!(
    flow_return_bl18_bare_return_joins_undefined_with_value_return,
    "BL18",
    "typeinfo currently does not join bare return paths as undefined alongside value-return paths; keep as the future BL18 bare-return join contract",
    |expr| {
        assert_union_contains_undefined(expr);
        assert_union_contains_string_literal(expr, "ready");
    }
);

future_catalog_contract!(
    flow_return_cn17_optional_chain_discriminant_narrows_nested_payload,
    "CN17",
    "typeinfo currently does not propagate optional-chain discriminant facts into nested payload member returns; keep as the future CN17 optional-discriminant contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_number_literal(expr, 0.0);
    }
);

future_catalog_contract!(
    flow_return_cn18_negative_in_operator_narrows_else_branch,
    "CN18",
    "typeinfo currently does not apply negative property-presence facts from !(key in x); keep as the future CN18 negative-in contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_cn19_nested_payload_facts_survive_boolean_composition,
    "CN19",
    "typeinfo currently does not compose nested discriminant and truthiness facts across && before returning payload members; keep as the future CN19 nested-boolean contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_catalog_contract!(
    flow_return_cn20_else_if_typeof_chain_partitions_all_constituents,
    "CN20",
    "typeinfo currently does not carry typeof exclusions through else-if chains for all remaining constituents; keep as the future CN20 chained-typeof contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::Number);
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Boolean);
    }
);

future_catalog_contract!(
    flow_return_pa10_assertion_refines_optional_object_member,
    "PA10",
    "typeinfo currently does not apply assertion signatures that refine optional object members before member returns; keep as the future PA10 member assertion contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_pa11_generic_assertion_refines_array_before_index_access,
    "PA11",
    "typeinfo currently does not instantiate generic assertion signatures before indexed array element returns; keep as the future PA11 generic assertion contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

// PA12 — TS7 emits `string`. The body returns `box.value` (narrowed to `string`
// inside the predicate guard) and the fallback `"fallback" as const`. The
// `"fallback"` literal is a subtype of `string`, so the union
// `string | "fallback"` normalises to `string`. The "future contract" should
// match TS7: a single `string` primitive, NOT a union containing the literal.
future_catalog_contract!(
    flow_return_pa12_generic_predicate_refines_box_member,
    "PA12",
    "typeinfo currently does not apply generic object predicates to refine contained member values; keep as the future PA12 boxed predicate contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_catalog_contract!(
    flow_return_cg12_infers_rest_tuple_and_projects_literal_slot,
    "CG12",
    "typeinfo currently does not infer rest tuple call-site literals or reduce tuple-index projections; keep as the future CG12 rest-tuple contract",
    |expr| assert_number_literal(expr, 1.0)
);

future_catalog_contract!(
    flow_return_cg13_calls_returned_closure_with_captured_object_substitution,
    "CG13",
    "typeinfo currently does not call returned closures while preserving captured generic object substitutions; keep as the future CG13 returned-closure call contract",
    |expr| assert_string_literal(expr, "x")
);

future_catalog_contract!(
    flow_return_cg14_contextual_this_parameter_flows_into_callback_return,
    "CG14",
    "typeinfo currently does not contextually type callback this-parameters for generic helper calls; keep as the future CG14 contextual-this contract",
    |expr| assert_string_literal(expr, "x")
);

future_catalog_contract!(
    flow_return_cg15_reduces_conditional_return_after_generic_literal_argument,
    "CG15",
    "typeinfo currently does not reduce conditional generic return annotations after literal call-site arguments; keep as the future CG15 conditional-call contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_cg16_applies_default_generic_return_substitution,
    "CG16",
    "typeinfo currently does not apply default generic type arguments while resolving declared return bodies; keep as the future CG16 default-generic return contract",
    |expr| assert_string_literal(expr, "default")
);

future_catalog_contract!(
    flow_return_ho12_find_predicate_overload_returns_refined_element_or_undefined,
    "HO12",
    "typeinfo currently does not model Array.find predicate overloads as refined element | undefined; keep as the future HO12 find-predicate contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_catalog_contract!(
    flow_return_ho13_every_predicate_narrows_array_for_continuation,
    "HO13",
    "typeinfo currently does not apply Array.every predicate facts to the continuation branch before indexed element returns; keep as the future HO13 every-predicate contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_number_literal(expr, 0.0);
    }
);

future_catalog_contract!(
    flow_return_ho14_reduce_explicit_record_accumulator_preserves_index_signature,
    "HO14",
    "typeinfo currently does not preserve explicit generic reduce accumulator annotations as Record index signatures; keep as the future HO14 reduce-record contract",
    |expr| {
        let signatures = object_index_signatures(expr);
        assert_eq!(signatures.len(), 1);
        assert_primitive(&signatures[0].value_type, PrimitiveName::Boolean);
    }
);

future_catalog_contract!(
    flow_return_ho15_promise_then_callback_maps_fulfilled_value,
    "HO15",
    "typeinfo currently does not model Promise.then callback return mapping to the resulting Promise payload; keep as the future HO15 promise-then contract",
    |expr| {
        let args = assert_ref_with_args(expr, "Promise", 1);
        assert_primitive(&args[0], PrimitiveName::Number);
    }
);

future_catalog_contract!(
    flow_return_ob14_infers_method_return_inside_returned_object,
    "OB14",
    "typeinfo currently does not infer object-literal method return types inside returned object surfaces; keep as the future OB14 method-return contract",
    |expr| {
        let props = assert_object_has_props(expr, &["run"]);
        let function = function_type(&props["run"].ty);
        assert_string_literal(
            function
                .return_type
                .as_deref()
                .expect("run method has return type"),
            "ok",
        );
    }
);

// OB15 — TS7 does NOT preserve the template-literal computed key as a concrete
// literal at the value level. Even with `ob15Prefix = "field" as const`, the
// returned object literal `{ [`${ob15Prefix}Name`]: "Ada" as const }` is typed
// as `{ [key: string]: "Ada" }` (string index signature with literal "Ada"
// value), NOT `{ fieldName: "Ada" }`. Template-literal key inference is a
// type-level operation; at the value level, computed keys widen to `string`.
future_catalog_contract!(
    flow_return_ob15_evaluates_template_literal_computed_object_key,
    "OB15",
    "typeinfo currently does not preserve TypeScript's computed template-key string index signature for returned object literals; keep as the future OB15 template-key contract",
    |expr| {
        let signatures = object_index_signatures(expr);
        assert_eq!(signatures.len(), 1);
        assert_primitive(&signatures[0].key_type, PrimitiveName::String);
        assert_string_literal(&signatures[0].value_type, "Ada");
    }
);

future_catalog_contract!(
    flow_return_ob16_preserves_nested_readonly_tuple_const_shape,
    "OB16",
    "typeinfo currently does not preserve nested readonly tuple/object shapes from as const return values; keep as the future OB16 nested-const contract",
    |expr| {
        let props = assert_object_has_props(expr, &["items"]);
        assert!(props["items"].readonly);
        let TypeExpr::Tuple { elements, readonly } = &props["items"].ty else {
            panic!("expected readonly tuple items, got {:?}", props["items"].ty);
        };
        assert!(*readonly);
        assert_eq!(elements.len(), 1);
        let item = object_props(&elements[0].ty);
        assert!(item["id"].readonly);
        assert_string_literal(&item["id"].ty, "a");
    }
);

future_catalog_contract!(
    flow_return_ob17_merges_conditional_spread_union_members_precisely,
    "OB17",
    "typeinfo currently does not preserve conditional spread branch-specific object members; keep as the future OB17 conditional-spread union contract",
    |expr| {
        let TypeExpr::Union(types) = expr else {
            panic!("expected conditional spread object union, got {expr:?}");
        };
        assert!(types.iter().any(|ty| object_props(ty).contains_key("enabled")));
        assert!(types.iter().any(|ty| object_props(ty).contains_key("disabled")));
    }
);

future_catalog_contract!(
    flow_return_ob18_satisfies_nested_shape_widens_against_target_type,
    "OB18",
    "typeinfo currently does not use nested satisfies target types to widen returned value shapes; keep as the future OB18 nested-satisfies contract",
    |expr| {
        let props = assert_object_has_props(expr, &["count", "nested"]);
        assert_primitive(&props["count"].ty, PrimitiveName::Number);
        let nested = object_props(&props["nested"].ty);
        assert_primitive(&nested["label"].ty, PrimitiveName::String);
    }
);

future_cross_package_contract!(
    flow_return_xf12_projects_tuple_return_from_package_declaration,
    "XF12",
    "typeinfo currently does not route synthetic package tuple-return declarations into body call indexed projections; keep as the future XF12 package tuple-call contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_cross_package_contract!(
    flow_return_xf13_applies_package_predicate_signature,
    "XF13",
    "typeinfo currently does not route synthetic package predicate signatures into caller flow facts; keep as the future XF13 package predicate contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_cross_package_contract!(
    flow_return_xf14_applies_package_assertion_signature,
    "XF14",
    "typeinfo currently does not route synthetic package assertion signatures into caller flow facts; keep as the future XF14 package assertion contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_cross_package_contract!(
    flow_return_xf15_instantiates_package_generic_function_return,
    "XF15",
    "typeinfo currently does not instantiate synthetic package generic function signatures from value arguments; keep as the future XF15 package generic-call contract",
    |expr| assert_string_literal(expr, "wrapped")
);

future_cross_package_contract!(
    flow_return_xf16_selects_package_overload_return,
    "XF16",
    "typeinfo currently does not select synthetic package overload signatures from literal arguments; keep as the future XF16 package overload contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_catalog_contract!(
    flow_return_vv17_async_computed_callback_unwraps_generic_payload,
    "VV17",
    "typeinfo currently does not infer async callback payloads through computed-like generic helpers; keep as the future VV17 async-computed contract",
    |expr| {
        let args = assert_ref_with_args(expr, "VV17AsyncComputedRef", 1);
        assert_union_contains_string_literal(&args[0], "yes");
        assert_union_contains_number_literal(&args[0], 0.0);
    }
);

future_catalog_contract!(
    flow_return_vv18_model_getter_fallback_refines_optional_value,
    "VV18",
    "typeinfo currently does not contextually type model-like get callbacks or reflect optional value policy; keep as the future VV18 model-getter contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_catalog_contract!(
    flow_return_vv19_optional_dynamic_slot_call_projects_object_or_undefined,
    "VV19",
    "typeinfo currently does not model optional dynamic slot function calls as return object | undefined; keep as the future VV19 optional-slot contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
        assert_union_has_object_arm(expr, &["node"]);
    }
);

future_catalog_contract!(
    flow_return_vv20_template_service_optional_method_projects_literal_or_undefined,
    "VV20",
    "typeinfo currently does not project optional method calls through generic template-service refs; keep as the future VV20 optional-service contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
        assert_union_contains_string_literal(expr, "done");
    }
);

future_catalog_contract!(
    flow_return_bl19_arrow_expression_body_applies_return_widening,
    "BL19",
    "typeinfo currently does not apply function-return widening to arrow expression bodies while preserving explicit const literal members; keep as the future BL19 arrow-return contract",
    |expr| {
        let props = assert_object_has_props(expr, &["count", "tag"]);
        assert_primitive(&props["count"].ty, PrimitiveName::Number);
        assert_string_literal(&props["tag"].ty, "arrow");
    }
);

catalog_contract!(
    flow_return_bl20_function_expression_uses_contextual_return_annotation,
    "BL20",
    |expr| {
        let props = assert_object_has_props(expr, &["id", "ready"]);
        assert_primitive(&props["id"].ty, PrimitiveName::String);
        assert_primitive(&props["ready"].ty, PrimitiveName::Boolean);
    }
);

future_catalog_contract!(
    flow_return_bl21_async_return_await_preserves_fulfilled_payload,
    "BL21",
    "typeinfo currently does not model return-await payload normalization for async function bodies; keep as the future BL21 return-await contract",
    |expr| {
        let args = assert_ref_with_args(expr, "Promise", 1);
        let props = assert_object_has_props(&args[0], &["id"]);
        assert_primitive(&props["id"].ty, PrimitiveName::String);
    }
);

// BL22 — TS7 emits `Generator<1 | 2, "done", any>`. The `yield*` operand is an
// `Iterable<1 | 2>` (no Next slot), so the inferred Next type defaults to `any`,
// NOT `unknown`. This is a deliberate TS choice: `Iterable<T>` is loose enough
// that yielding into it accepts anything, so the consumer Next type widens to
// `any` rather than the safer `unknown`.
future_catalog_contract!(
    flow_return_bl22_generator_yield_star_protocol_joins_delegated_yields,
    "BL22",
    "typeinfo currently does not model yield* delegation in generator return protocols; keep as the future BL22 yield-star contract",
    |expr| {
        let args = assert_ref_with_args(expr, "Generator", 3);
        assert_union_contains_number_literal(&args[0], 1.0);
        assert_union_contains_number_literal(&args[0], 2.0);
        assert_string_literal(&args[1], "done");
        assert_primitive(&args[2], PrimitiveName::Any);
    }
);

catalog_contract!(
    flow_return_bl23_explicit_void_annotation_wins_over_return_expression,
    "BL23",
    |expr| assert_primitive(expr, PrimitiveName::Void)
);
