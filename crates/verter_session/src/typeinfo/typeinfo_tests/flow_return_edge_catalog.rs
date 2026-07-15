//! @ai-generated - Synthetic edge contracts for native flow-return typeinfo.

use super::support::*;
use crate::VerterHost;
use verter_type_expr::LiteralValue;

const SYNTHETIC_EDGE_VALUES_PACKAGE_JSON: &str = r#"{
  "name": "synthetic-edge-values",
  "exports": {
    "./tools": {
      "types": "./dist/tools.d.ts",
      "import": "./dist/tools.js"
    }
  }
}"#;

const SYNTHETIC_EDGE_VALUES_RUNTIME: &str = r#"
export function edgeGetMap() { return new Map(); }
export function edgeAssertReady(input) { if (!input || input.ready !== true) throw new Error('not ready'); }
export function edgeMaybe(value) { return value; }
export function edgePick(kind) {
  return kind === 'left'
    ? { side: 'left', value: '' }
    : { side: 'right', value: 0 };
}
"#;

fn upsert_edge_fixture(host: &VerterHost) {
    upsert_ts(
        host,
        "/fixtures/flow_return_edge_catalog.ts",
        FLOW_RETURN_EDGE_CATALOG,
    );
}

fn make_edge_package_host() -> Arc<VerterHost> {
    make_host_with_workspace_files_footprint(&[
        (
            "/workspace/node_modules/synthetic-edge-values/package.json",
            SYNTHETIC_EDGE_VALUES_PACKAGE_JSON,
        ),
        (
            "/workspace/node_modules/synthetic-edge-values/dist/tools.d.ts",
            FLOW_RETURN_EDGE_PACKAGE_DECLARATIONS,
        ),
        (
            "/workspace/node_modules/synthetic-edge-values/dist/tools.js",
            SYNTHETIC_EDGE_VALUES_RUNTIME,
        ),
        (
            "/workspace/node_modules/synthetic-edge-values/dist/unused.d.ts",
            "export declare const unused: unique symbol;",
        ),
    ])
}

fn upsert_edge_cross_fixture(host: &VerterHost) {
    upsert_ts(
        host,
        "/workspace/src/flow_return_edge_cross.ts",
        FLOW_RETURN_EDGE_CROSS,
    );
}

fn assert_edge_package_route_preconditions(host: &VerterHost) {
    let package_runtime = host.resolve_loaded_dependency_canonical(
        "/workspace/src/flow_return_edge_cross.ts",
        "synthetic-edge-values/tools",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    assert_eq!(
        package_runtime.as_deref(),
        Some("/workspace/node_modules/synthetic-edge-values/dist/tools.js"),
        "fixture precondition: host must resolve the synthetic package subpath runtime route",
    );
    let package_declaration = host.resolve_eval_dependency_canonical(
        "/workspace/node_modules/synthetic-edge-values/dist/tools.js",
    );
    assert_eq!(
        package_declaration.as_deref(),
        Some("/workspace/node_modules/synthetic-edge-values/dist/tools.d.ts"),
        "fixture precondition: host must model the package subpath runtime-to-declaration route",
    );
}

fn assert_edge_alias<F>(alias: &str, check: F)
where
    F: FnOnce(&TypeExpr),
{
    let host = make_host_with_footprint();
    upsert_edge_fixture(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/flow_return_edge_catalog.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    check(&expr);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

fn assert_edge_package_alias<F>(alias: &str, check: F)
where
    F: FnOnce(&TypeExpr),
{
    let host = make_edge_package_host();
    upsert_edge_cross_fixture(&host);
    assert_edge_package_route_preconditions(&host);
    let (expr, record) = resolve_expr(
        &host,
        "/workspace/src/flow_return_edge_cross.ts",
        alias,
        &[],
        ProjectionMode::Expanded,
    );
    assert_declared_dependency_includes(
        &record,
        "/workspace/node_modules/synthetic-edge-values/dist/tools.d.ts",
    );
    assert_declared_dependency_excludes(
        &record,
        "/workspace/node_modules/synthetic-edge-values/dist/unused.d.ts",
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
    check(&expr);
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

fn assert_union_contains_function_returning_string(expr: &TypeExpr, expected: &str) {
    let TypeExpr::Union(types) = expr else {
        panic!("expected function | undefined union, got {expr:?}");
    };
    assert!(
        types.iter().any(|ty| {
            let TypeExpr::Function(function) = ty else {
                return false;
            };
            matches!(
                function.return_type.as_deref(),
                Some(TypeExpr::Literal(LiteralValue::String(value))) if value == expected
            )
        }),
        "expected union {expr:?} to contain a function returning {expected:?}"
    );
}

macro_rules! future_edge_contract {
    ($name:ident, $alias:literal, $reason:literal, $check:expr) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            assert_edge_alias($alias, $check);
        }
    };
}

macro_rules! future_edge_package_contract {
    ($name:ident, $alias:literal, $reason:literal, $check:expr) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            assert_edge_package_alias($alias, $check);
        }
    };
}

#[test]
fn flow_return_xf17_fixture_resolves_synthetic_package_subpath_declaration_route() {
    let host = make_edge_package_host();
    upsert_edge_cross_fixture(&host);
    assert_edge_package_route_preconditions(&host);
}

future_edge_contract!(
    flow_return_lr13_destructuring_nested_default_returns_widened_string,
    "LR13",
    "typeinfo currently does not apply nested destructuring defaults or widen the returned defaulted binding; keep as the future LR13 destructuring-default contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_edge_contract!(
    flow_return_lr14_rest_destructuring_omits_removed_property,
    "LR14",
    "typeinfo currently does not materialize object-rest destructuring with omitted keys and optional members; keep as the future LR14 object-rest contract",
    |expr| {
        let props = assert_object_has_props(expr, &["extra", "keep"]);
        assert!(props["extra"].optional);
        assert_primitive(&props["extra"].ty, PrimitiveName::Boolean);
        assert_primitive(&props["keep"].ty, PrimitiveName::String);
    }
);

future_edge_contract!(
    flow_return_lr15_array_destructuring_default_preserves_readonly_tuple,
    "LR15",
    "typeinfo currently does not combine array destructuring defaults with readonly tuple return inference; keep as the future LR15 tuple-destructure contract",
    |expr| {
        let TypeExpr::Tuple { elements, readonly } = expr else {
            panic!("expected readonly tuple, got {expr:?}");
        };
        assert!(*readonly);
        assert_eq!(elements.len(), 2);
        assert_primitive(&elements[0].ty, PrimitiveName::String);
        assert_primitive(&elements[1].ty, PrimitiveName::Number);
    }
);

future_edge_contract!(
    flow_return_lr16_logical_assignment_narrows_local_before_return,
    "LR16",
    "typeinfo currently does not model ??= assignment effects as non-nullish local facts; keep as the future LR16 logical-assignment contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

// CN21 — TS7 emits `string | 0`. The narrowed branch returns
// `input.nested.value: string` (after `kind === "a" && input.nested?.value`).
// The fallback returns `0 as const`, preserving the literal `0` rather than
// widening to `number`. The expectation pairs a string primitive with a
// number-literal `0`, NOT a `number` primitive.
future_edge_contract!(
    flow_return_cn21_optional_member_fact_combines_with_discriminant,
    "CN21",
    "typeinfo currently does not compose discriminant facts with optional member truthiness before nested returns; keep as the future CN21 discriminant-plus-optional contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_number_literal(expr, 0.0);
    }
);

future_edge_contract!(
    flow_return_cn22_tuple_discriminant_narrows_indexed_slot,
    "CN22",
    "typeinfo currently does not correlate tuple discriminant slots with indexed tuple payload slots; keep as the future CN22 discriminated-tuple contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_edge_contract!(
    flow_return_cn23_negative_discriminant_guard_keeps_ready_tail,
    "CN23",
    "typeinfo currently does not carry negative discriminant exclusions into the continuation branch; keep as the future CN23 negative-discriminant contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

// CN24 — TS7 emits `unknown` (NOT `unknown | undefined`). The narrowed branch
// returns `input.id` (typed `unknown` after the `"id" in input` guard) and the
// fallback returns `undefined`. Union normalisation collapses
// `unknown | undefined` to `unknown` because `unknown` is the top of the
// assignability lattice — every type, including `undefined`, is already
// assignable to `unknown`, so the union is structurally `unknown`.
future_edge_contract!(
    flow_return_cn24_unknown_object_in_guard_projects_unknown_property,
    "CN24",
    "typeinfo currently does not model object/non-null/in guards over unknown values as Record-key facts; keep as the future CN24 unknown-object guard contract",
    |expr| assert_primitive(expr, PrimitiveName::Unknown)
);

future_edge_contract!(
    flow_return_pa13_applies_this_is_method_predicate,
    "PA13",
    "typeinfo currently does not apply this-is method predicates as receiver flow facts; keep as the future PA13 method-predicate contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_edge_contract!(
    flow_return_pa14_assertion_refines_dotted_member_path,
    "PA14",
    "typeinfo currently does not apply assertion signatures to dotted member paths before projected returns; keep as the future PA14 dotted-assertion contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_edge_contract!(
    flow_return_pa15_generic_key_predicate_enables_property_narrowing,
    "PA15",
    "typeinfo currently does not instantiate generic key predicates or narrow newly proven properties; keep as the future PA15 key-predicate contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_edge_contract!(
    flow_return_pa16_never_assertion_preserves_exhaustive_switch_return,
    "PA16",
    "typeinfo currently does not use never assertion helpers to preserve exhaustive switch return joins; keep as the future PA16 assert-never contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_edge_contract!(
    flow_return_cg17_selects_interface_call_signature_overload,
    "CG17",
    "typeinfo currently does not select overloads declared on interface call signatures from literal arguments; keep as the future CG17 call-signature overload contract",
    |expr| assert_primitive(expr, PrimitiveName::Number)
);

future_edge_contract!(
    flow_return_cg18_instantiates_generic_arrow_value_call,
    "CG18",
    "typeinfo currently does not instantiate generic arrow-function values when called from a body; keep as the future CG18 generic-arrow contract",
    |expr| assert_string_literal(expr, "ok")
);

future_edge_contract!(
    flow_return_cg19_projects_keyof_indexed_return_after_call,
    "CG19",
    "typeinfo currently does not infer keyof call arguments and reduce the returned indexed-access slot; keep as the future CG19 keyof-call contract",
    |expr| assert_string_literal(expr, "x")
);

future_edge_contract!(
    flow_return_cg20_instantiates_generic_class_constructor_value,
    "CG20",
    "typeinfo currently does not instantiate generic class constructors before projecting instance fields; keep as the future CG20 generic-class contract",
    |expr| assert_string_literal(expr, "boxed")
);

future_edge_contract!(
    flow_return_cg21_optional_generic_callback_call_adds_undefined,
    "CG21",
    "typeinfo currently does not infer optional generic callback calls as callback return | undefined; keep as the future CG21 optional-callback contract",
    |expr| {
        assert_union_contains_string_literal(expr, "ok");
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_edge_contract!(
    flow_return_ho16_find_optional_chain_maps_refined_element,
    "HO16",
    "typeinfo currently does not combine Array.find predicate overloads with optional-call member returns; keep as the future HO16 find-optional-call contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_edge_contract!(
    flow_return_ho17_from_entries_preserves_explicit_record_value,
    "HO17",
    "typeinfo currently does not preserve explicit Record casts through Object.fromEntries callback pipelines; keep as the future HO17 fromEntries contract",
    |expr| {
        let signatures = object_index_signatures(expr);
        assert_eq!(signatures.len(), 1);
        assert_primitive(&signatures[0].value_type, PrimitiveName::String);
    }
);

future_edge_contract!(
    flow_return_ho18_promise_all_tuple_then_maps_payload_object,
    "HO18",
    "typeinfo currently does not preserve Promise.all tuple payloads through then callbacks; keep as the future HO18 Promise.all tuple contract",
    |expr| {
        let args = assert_ref_with_args(expr, "Promise", 1);
        let props = assert_object_has_props(&args[0], &["count", "label"]);
        assert_primitive(&props["count"].ty, PrimitiveName::Number);
        assert_primitive(&props["label"].ty, PrimitiveName::String);
    }
);

future_edge_contract!(
    flow_return_ho19_flat_map_object_result_filters_empty_arrays,
    "HO19",
    "typeinfo currently does not infer object array arms from flatMap callbacks while dropping empty-array arms; keep as the future HO19 flatMap-object contract",
    |expr| {
        let element = array_element(expr);
        let props = assert_object_has_props(element, &["value"]);
        assert_primitive(&props["value"].ty, PrimitiveName::String);
    }
);

future_edge_contract!(
    flow_return_ob19_object_assign_intersection_materializes_return_shape,
    "OB19",
    "typeinfo currently does not model Object.assign return intersections as a materialized object surface; keep as the future OB19 assign-return contract",
    |expr| {
        let props = assert_object_has_props(expr, &["a", "b"]);
        assert_number_literal(&props["a"].ty, 1.0);
        assert_string_literal(&props["b"].ty, "x");
    }
);

future_edge_contract!(
    flow_return_ob20_function_or_undefined_property_preserves_function_return,
    "OB20",
    "typeinfo currently does not preserve function-valued conditional object properties with undefined arms; keep as the future OB20 optional-function-property contract",
    |expr| {
        let props = assert_object_has_props(expr, &["maybe"]);
        assert_union_contains_function_returning_string(&props["maybe"].ty, "yes");
        assert_union_contains_primitive(&props["maybe"].ty, PrimitiveName::Undefined);
    }
);

future_edge_contract!(
    flow_return_ob21_generic_spread_keeps_substituted_literal_and_extra_prop,
    "OB21",
    "typeinfo currently does not substitute generic object spread inputs before adding returned literal members; keep as the future OB21 generic-spread contract",
    |expr| {
        let props = assert_object_has_props(expr, &["extra", "id"]);
        assert_boolean_literal(&props["extra"].ty, true);
        assert_string_literal(&props["id"].ty, "edge");
    }
);

future_edge_contract!(
    flow_return_ob22_satisfies_record_widens_literal_values,
    "OB22",
    "typeinfo currently does not use Record satisfies targets to widen returned object literal values; keep as the future OB22 satisfies-Record contract",
    |expr| {
        let props = assert_object_has_props(expr, &["one", "two"]);
        assert_primitive(&props["one"].ty, PrimitiveName::Number);
        assert_primitive(&props["two"].ty, PrimitiveName::Number);
    }
);

future_edge_contract!(
    flow_return_cf17_do_while_definite_execution_updates_return_fact,
    "CF17",
    "typeinfo currently does not model do-while definite execution and assignment effects before final returns; keep as the future CF17 do-while contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_edge_contract!(
    flow_return_cf18_default_throw_does_not_add_undefined_to_switch_return,
    "CF18",
    "typeinfo currently does not treat default throw edges as terminating in switch return joins; keep as the future CF18 terminating-default contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_edge_contract!(
    flow_return_cf19_catch_unknown_absorbs_try_return_union,
    "CF19",
    "typeinfo currently does not model catch variable unknown flow and union absorption for try/catch returns; keep as the future CF19 catch-unknown contract",
    |expr| assert_primitive(expr, PrimitiveName::Unknown)
);

future_edge_contract!(
    flow_return_cf20_for_in_key_return_adds_empty_object_undefined_path,
    "CF20",
    "typeinfo currently does not model for-in key types or the empty-loop fallthrough path; keep as the future CF20 for-in contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_edge_contract!(
    flow_return_cf21_bare_return_after_null_guard_joins_undefined,
    "CF21",
    "typeinfo currently does not join bare return paths with narrowed continuation return values; keep as the future CF21 null-guard bare-return contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_edge_contract!(
    flow_return_cf22_finally_without_return_preserves_try_return,
    "CF22",
    "typeinfo currently does not preserve try return facts across non-returning finally blocks; keep as the future CF22 non-overriding-finally contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_edge_contract!(
    flow_return_vv21_writable_computed_contextually_types_getter_and_setter,
    "VV21",
    "typeinfo currently does not contextually type writable-computed style get/set callbacks and publish the getter return; keep as the future VV21 writable-computed contract",
    |expr| assert_string_literal(expr, "ready")
);

future_edge_contract!(
    flow_return_vv22_props_destructure_default_returns_widened_string,
    "VV22",
    "typeinfo currently does not model defineProps-like destructuring defaults as widened local values; keep as the future VV22 props-destructure contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_edge_contract!(
    flow_return_vv23_template_slot_record_optional_call_projects_node,
    "VV23",
    "typeinfo currently does not resolve template-literal Record slot keys through optional function calls; keep as the future VV23 dynamic-slot Record contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
        assert_union_has_object_arm(expr, &["node"]);
    }
);

future_edge_contract!(
    flow_return_vv24_callback_result_tracks_discriminated_input_facts,
    "VV24",
    "typeinfo currently does not infer helper callback results while preserving discriminant facts from outer parameters; keep as the future VV24 captured-prop callback contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Number);
    }
);

future_edge_package_contract!(
    flow_return_xf17_package_subpath_map_get_optional_member,
    "XF17",
    "typeinfo currently does not route package subpath declarations through Map.get optional member returns; keep as the future XF17 package-subpath Map contract",
    |expr| {
        assert_union_contains_primitive(expr, PrimitiveName::String);
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_edge_package_contract!(
    flow_return_xf18_package_subpath_assertion_refines_unknown,
    "XF18",
    "typeinfo currently does not apply assertion signatures imported from package subpath declarations; keep as the future XF18 package-subpath assertion contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_edge_package_contract!(
    flow_return_xf19_package_subpath_generic_optional_call_instantiates_literal,
    "XF19",
    "typeinfo currently does not instantiate generic package subpath declarations through optional member access; keep as the future XF19 package-subpath generic contract",
    |expr| {
        assert_union_contains_string_literal(expr, "x");
        assert_union_contains_primitive(expr, PrimitiveName::Undefined);
    }
);

future_edge_package_contract!(
    flow_return_xf20_package_subpath_overload_selects_literal_branch,
    "XF20",
    "typeinfo currently does not select overloaded package subpath declarations from literal arguments; keep as the future XF20 package-subpath overload contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);

future_edge_package_contract!(
    flow_return_xf21_package_subpath_kitchen_combines_assertion_generic_overload_and_coalesce,
    "XF21",
    "typeinfo currently does not combine package subpath assertions, generic optional returns, overload selection, and nullish coalescing while keeping unrelated declaration files cold; keep as the future XF21 package kitchen contract",
    |expr| assert_primitive(expr, PrimitiveName::String)
);
