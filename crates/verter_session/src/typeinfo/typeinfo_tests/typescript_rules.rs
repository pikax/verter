//! @ai-generated - Broad synthetic TypeScript type-system rule tests.

use super::support::*;

#[test]
fn typescript_rules_literals_primitives_and_object_modifiers() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "LiteralAndPrimitiveSurface",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_string_literal(&props["stringLiteral"].ty, "ready");
    assert_number_literal(&props["numberLiteral"].ty, 42.0);
    assert_boolean_literal(&props["booleanLiteral"].ty, true);
    assert_primitive(&props["stringValue"].ty, PrimitiveName::String);
    assert_primitive(&props["numberValue"].ty, PrimitiveName::Number);
    assert_primitive(&props["booleanValue"].ty, PrimitiveName::Boolean);
    assert_primitive(&props["symbolValue"].ty, PrimitiveName::Symbol);
    assert_primitive(&props["bigintValue"].ty, PrimitiveName::BigInt);
    assert_primitive(&props["nullValue"].ty, PrimitiveName::Null);
    assert_primitive(&props["undefinedValue"].ty, PrimitiveName::Undefined);
    assert_primitive(&props["unknownValue"].ty, PrimitiveName::Unknown);
    assert_primitive(&props["anyValue"].ty, PrimitiveName::Any);
    assert_primitive(&props["neverValue"].ty, PrimitiveName::Never);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn typescript_rules_methods_readonly_optional_and_index_signatures() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "MethodAndIndexSurface",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert!(props["id"].readonly);
    assert!(!props["id"].optional);
    assert!(props["label"].optional);
    let method = function_type(&props["method"].ty);
    assert_eq!(method.parameters.len(), 2);
    assert_primitive(&method.parameters[0].ty, PrimitiveName::String);
    assert!(method.parameters[1].optional);
    assert_primitive(
        method.return_type.as_ref().expect("method return type"),
        PrimitiveName::Boolean,
    );
    let index_sigs = object_index_signatures(&expr);
    assert_eq!(index_sigs.len(), 1);
    assert_primitive(&index_sigs[0].key_type, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn typescript_rules_tuples_arrays_and_functions_publish_structured_shapes() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (tuple, tuple_record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "TupleRules",
        &[],
        ProjectionMode::Expanded,
    );
    let TypeExpr::Tuple { elements, readonly } = tuple else {
        panic!("expected tuple, got {tuple:?}");
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 3);
    assert_eq!(elements[0].label.as_deref(), Some("name"));
    assert_primitive(&elements[0].ty, PrimitiveName::String);
    assert!(elements[1].optional);
    assert!(elements[2].rest);

    let (readonly_tuple, readonly_record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "ReadonlyTupleRules",
        &[],
        ProjectionMode::Expanded,
    );
    let TypeExpr::Tuple { elements, readonly } = readonly_tuple else {
        panic!("expected readonly tuple, got {readonly_tuple:?}");
    };
    assert!(readonly);
    assert_string_literal(&elements[0].ty, "view");
    match &elements[1].ty {
        TypeExpr::Array { element, readonly } => {
            assert!(*readonly);
            assert_primitive(element, PrimitiveName::Number);
        }
        other => panic!("expected readonly number array, got {other:?}"),
    }

    let (function, function_record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "FunctionRules",
        &[],
        ProjectionMode::Expanded,
    );
    let function = function_type(&function);
    assert_eq!(function.parameters.len(), 2);
    assert!(function.parameters[1].rest);
    let return_props = object_props(function.return_type.as_ref().expect("function return type"));
    assert_array_of_primitive(&return_props["flags"].ty, PrimitiveName::Boolean);
    assert_query_mode(&tuple_record, ProjectionModeTag::Expanded);
    assert_query_mode(&readonly_record, ProjectionModeTag::Expanded);
    assert_query_mode(&function_record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently marks tuple rest element value types as semanticMiss instead of preserving the array element primitive; keep as the future tuple rest-element contract"]
fn typescript_rules_tuple_rest_element_resolves_array_element_type() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (tuple, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "TupleRules",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, .. } = tuple else {
        panic!("expected tuple, got {tuple:?}");
    };
    assert!(elements[2].rest);
    assert_primitive(&elements[2].ty, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn typescript_rules_record_and_mapped_modifiers_materialize_simple_surfaces() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (record, record_audit) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "RecordLiteralKeys",
        &[],
        ProjectionMode::Expanded,
    );
    let record_props = object_props(&record);
    assert_eq!(prop_names(&record_props), vec!["alpha", "beta"]);
    assert_primitive(&record_props["alpha"].ty, PrimitiveName::Number);

    let (mapped, mapped_audit) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "MappedModifierSurface",
        &[],
        ProjectionMode::Expanded,
    );
    let mapped_props = object_props(&mapped);
    assert_eq!(prop_names(&mapped_props), vec!["count", "id"]);
    assert!(mapped_props["id"].readonly);
    assert!(!mapped_props["id"].optional);
    assert_primitive(&mapped_props["id"].ty, PrimitiveName::String);
    assert_query_mode(&record_audit, ProjectionModeTag::Expanded);
    assert_query_mode(&mapped_audit, ProjectionModeTag::Expanded);
}

#[test]
fn typescript_rules_union_and_intersection_object_contributions() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (union, union_record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "UnionObjectRules",
        &[],
        ProjectionMode::Expanded,
    );
    assert_union_has_object_arm(&union, &["a", "kind", "shared"]);
    assert_union_has_object_arm(&union, &["b", "kind", "shared"]);

    let (intersection, intersection_record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "IntersectionObjectRules",
        &[],
        ProjectionMode::Expanded,
    );
    let intersection_props = object_props(&intersection);
    assert_eq!(
        prop_names(&intersection_props),
        vec!["count", "id", "ready"]
    );
    assert!(intersection_props["ready"].readonly);
    assert!(intersection_props["count"].optional);
    assert_query_mode(&union_record, ProjectionModeTag::Expanded);
    assert_query_mode(&intersection_record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently preserves keyof aliases instead of materializing a literal key union for named typeinfo requests; keep as the future keyof contract"]
fn typescript_rules_keyof_materializes_literal_key_union() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "KeyOfRules",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["count", "id", "nested"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently preserves multi-hop indexed-access aliases instead of reducing KeySource['nested']['value']; keep as the future indexed-access contract"]
fn typescript_rules_indexed_access_reduces_terminal_property() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "IndexedRules",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not distribute conditional aliases over union checks into a union of branch objects; keep as the future distributive conditional contract"]
fn typescript_rules_distributive_conditional_expands_each_union_arm() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "ConditionalDistributedRules",
        &[],
        ProjectionMode::Expanded,
    );

    assert_union_has_object_arm(&expr, &["text"]);
    assert_union_has_object_arm(&expr, &["other"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn typescript_rules_non_distributive_conditional_selects_false_branch() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "ConditionalNonDistributedRules",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert!(props.contains_key("other"));
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently leaves ConstructorParameters<T> tuple projection unresolved for construct signatures; keep as the future constructor utility contract"]
fn typescript_rules_constructor_parameters_resolve_tuple() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "ConstructorParamsRules",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, .. } = expr else {
        panic!("expected constructor parameter tuple, got {expr:?}");
    };
    assert_eq!(elements.len(), 1);
    assert_primitive(&elements[0].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not materialize InstanceType<T> from construct signatures; keep as the future constructor instance contract"]
fn typescript_rules_instance_type_resolves_constructed_object() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "InstanceRules",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert_primitive(&props["ready"].ty, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not project class instance members through InstanceType<typeof Class>; keep as the future class instance contract"]
fn typescript_rules_class_instance_type_includes_fields_and_methods() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "ClassInstanceRules",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    let method = function_type(&props["method"].ty);
    assert_primitive(
        method.return_type.as_ref().expect("method return type"),
        PrimitiveName::String,
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not lower typeof const assertions into readonly literal object surfaces; keep as the future typeof const contract"]
fn typescript_rules_typeof_const_preserves_readonly_literals() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "TypeOfConstRules",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_string_literal(&props["mode"].ty, "view");
    assert!(props["mode"].readonly);
    let nested = object_props(&props["nested"].ty);
    assert_number_literal(&nested["value"].ty, 1.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn typescript_rules_typeof_const_nested_value_resolves_literal() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "TypeOfConstNestedValue",
        &[],
        ProjectionMode::Expanded,
    );

    assert_number_literal(&expr, 1.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not recursively unwrap Awaited<Promise<...>> to its fulfilled object type; keep as the future Awaited utility contract"]
fn typescript_rules_awaited_recursively_unwraps_promises() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "AwaitedRules",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_boolean_literal(&props["done"].ty, true);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not evaluate template-literal intrinsic string utilities like Capitalize over unions; keep as the future string-intrinsic contract"]
fn typescript_rules_template_intrinsic_evaluates_union() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "TemplateIntrinsicRules",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["onCancel", "onSubmit"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not apply key remapping with never-filtered keys and template-literal output names; keep as the future key-remap filter contract"]
fn typescript_rules_key_remap_exclude_filters_and_renames_keys() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/typescript-rules.ts", TYPESCRIPT_RULES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/typescript-rules.ts",
        "KeyRemapExcludeSurface",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["public:count", "public:id"]);
    assert_primitive(&props["public:id"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
