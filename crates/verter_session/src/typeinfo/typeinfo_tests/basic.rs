//! @ai-generated - Basic synthetic component-surface typeinfo tests.

use super::support::*;

#[test]
fn component_like_simple_surface_extracts_primitive_literal_union_members() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/component-types.ts", COMPONENT_TYPES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/component-types.ts",
        "PrimitiveSurface",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(
        prop_names(&props),
        vec!["count", "disabled", "label", "variant"]
    );
    assert_primitive(&props["label"].ty, PrimitiveName::String);
    assert!(!props["label"].optional);
    assert_primitive(&props["disabled"].ty, PrimitiveName::Boolean);
    assert!(props["disabled"].optional);
    assert_primitive(&props["count"].ty, PrimitiveName::Number);
    assert_literal_union(&props["variant"].ty, &["ghost", "solid"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn component_like_generic_box_instantiates_descriptor_arguments() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/component-types.ts", COMPONENT_TYPES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/component-types.ts",
        "GenericBox",
        &[Arc::new(TypeExpr::Primitive(PrimitiveName::String))],
        ProjectionMode::Expanded,
    );

    // Fixture: GenericBox<TValue> = { value: TValue; list: TValue[]; maybe?: TValue | null }.
    // TS7: instantiated with TValue=string yields value:string (required),
    // list:string[] (required), maybe:string | null | undefined (optional via the `?`).
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["list", "maybe", "value"]);
    assert!(!props["value"].optional);
    assert_primitive(&props["value"].ty, PrimitiveName::String);
    assert!(!props["list"].optional);
    assert_array_of_primitive(&props["list"].ty, PrimitiveName::String);
    assert!(props["maybe"].optional);
    assert_union_contains_primitive(&props["maybe"].ty, PrimitiveName::String);
    assert_union_contains_primitive(&props["maybe"].ty, PrimitiveName::Null);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn component_like_surface_resolves_through_structured_alias_imports() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/component-types.ts", COMPONENT_TYPES);
    upsert_ts(&host, "/fixtures/scope.ts", SCOPE_TYPES);

    let (outcome, record) = host
        .evaluate_type_expression_with_audit(EvaluateTypeExpressionRequest {
            scope: "/fixtures/scope.ts".to_string(),
            expression: "Surface<string, Item>".to_string(),
            extra_imports: vec![ImportSpec {
                specifier: "/fixtures/component-types".to_string(),
                bindings: vec![
                    NamedImport::Named {
                        exported_name: "ComponentSurface".to_string(),
                        local_alias: Some("Surface".to_string()),
                        type_only: true,
                    },
                    NamedImport::Named {
                        exported_name: "ConcreteItem".to_string(),
                        local_alias: Some("Item".to_string()),
                        type_only: true,
                    },
                ],
            }],
            mode: ProjectionMode::Expanded,
            cacheable: false,
        })
        .into_parts();
    let node = outcome.ok().flatten();
    let expr = host
        .project_node_to_type_expr_for_test(node.expect("Surface<string, Item> must resolve"))
        .expect("resolved node projects to TypeExpr");

    let props = object_props(&expr);
    assert_eq!(
        prop_names(&props),
        vec![
            "config",
            "defaultValue",
            "items",
            "labelFor",
            "modelValue",
            "passthrough",
            "slots",
            "state",
            "status",
            "ui",
            "variant",
        ]
    );
    assert_primitive(&props["modelValue"].ty, PrimitiveName::String);
    assert_array_of_ref(&props["items"].ty, "ConcreteItem");
    let label_for = function_type(&props["labelFor"].ty);
    assert_eq!(label_for.parameters.len(), 2);
    assert_ref(&label_for.parameters[0].ty, "ConcreteItem");
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
fn component_like_utility_aliases_extract_pick_and_omit_surfaces() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/component-types.ts", COMPONENT_TYPES);

    let (picked, picked_record) = resolve_expr(
        &host,
        "/fixtures/component-types.ts",
        "ConfigSubset",
        &[],
        ProjectionMode::Expanded,
    );
    let picked_props = object_props(&picked);
    assert_eq!(prop_names(&picked_props), vec!["items", "size", "tone"]);
    assert_ref(&picked_props["size"].ty, "SizeToken");
    assert_ref(&picked_props["tone"].ty, "ColorToken");
    assert_array_of_ref(&picked_props["items"].ty, "ConcreteItem");

    let (omitted, omitted_record) = resolve_expr(
        &host,
        "/fixtures/component-types.ts",
        "PassthroughSettings",
        &[],
        ProjectionMode::Expanded,
    );
    let omitted_props = object_props(&omitted);
    assert_eq!(
        prop_names(&omitted_props),
        vec!["items", "lazy", "size", "tone"]
    );
    assert!(
        !omitted_props.contains_key("debugOnly"),
        "Omit<ExternalSettings, 'debugOnly'> must not publish debugOnly"
    );
    assert_query_mode(&picked_record, ProjectionModeTag::Expanded);
    assert_query_mode(&omitted_record, ProjectionModeTag::Expanded);
}

#[test]
fn component_like_conditional_aliases_select_concrete_branches() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/component-types.ts", COMPONENT_TYPES);

    let (string_status, string_record) = resolve_expr(
        &host,
        "/fixtures/component-types.ts",
        "StatusForString",
        &[],
        ProjectionMode::Expanded,
    );
    let string_props = object_props(&string_status);
    assert_string_literal(&string_props["kind"].ty, "text");
    assert_primitive(&string_props["value"].ty, PrimitiveName::String);

    let (number_status, number_record) = resolve_expr(
        &host,
        "/fixtures/component-types.ts",
        "StatusForNumber",
        &[],
        ProjectionMode::Expanded,
    );
    let number_props = object_props(&number_status);
    assert_string_literal(&number_props["kind"].ty, "other");
    assert_primitive(&number_props["value"].ty, PrimitiveName::Number);
    assert_query_mode(&string_record, ProjectionModeTag::Expanded);
    assert_query_mode(&number_record, ProjectionModeTag::Expanded);
}

#[test]
fn component_like_slot_payload_extracts_nested_parameter_object() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/component-types.ts", COMPONENT_TYPES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/component-types.ts",
        "SlotPayload",
        &[],
        ProjectionMode::Expanded,
    );

    // Fixture: RenderPayload<TItem, TValue> = { item: TItem; value: TValue; active: boolean;
    //                                            attrs?: { role: "option"; tabindex: 0 | -1 } }.
    // TS7: SlotPayload = RenderPayload<ConcreteItem, string>:
    //   item / value / active are required; attrs is optional via the `?` marker.
    //   attrs.role and attrs.tabindex are required literals.
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["active", "attrs", "item", "value"]);
    assert!(!props["item"].optional);
    assert_ref(&props["item"].ty, "ConcreteItem");
    assert!(!props["value"].optional);
    assert_primitive(&props["value"].ty, PrimitiveName::String);
    assert!(!props["active"].optional);
    assert_primitive(&props["active"].ty, PrimitiveName::Boolean);
    assert!(props["attrs"].optional);
    let attrs = object_props(&props["attrs"].ty);
    assert!(!attrs["role"].optional);
    assert_string_literal(&attrs["role"].ty, "option");
    assert!(!attrs["tabindex"].optional);
    assert_number_literal_union(&attrs["tabindex"].ty, &[0.0, -1.0]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently leaves Parameters<NonNullable<T['slot']>>[0] behind a semanticMiss indexed access; keep as the future slot-payload extraction contract"]
fn component_like_slot_payload_extracts_parameters_from_nested_slot_property() {
    // TS7 contract: SlotPayloadFromDefault =
    //   Parameters<NonNullable<NonNullable<ConcreteSurface["slots"]>["default"]>>[0]
    // reduces through:
    //   ConcreteSurface["slots"] = SlotContract<ConcreteItem, string> | undefined
    //   NonNullable<...> = SlotContract<ConcreteItem, string>
    //   ["default"] = ((payload: RenderPayload<ConcreteItem, string>) => unknown) | undefined
    //   NonNullable<...> = (payload: RenderPayload<ConcreteItem, string>) => unknown
    //   Parameters<...>[0] = RenderPayload<ConcreteItem, string>
    // Expected published shape is identical to the SlotPayload test above. Verter currently
    // leaves the multi-hop NonNullable + Parameters[0] chain as `Unknown { raw: "semanticMiss" }`.
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/component-types.ts", COMPONENT_TYPES);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/component-types.ts",
        "SlotPayloadFromDefault",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["active", "attrs", "item", "value"]);
    assert_ref(&props["item"].ty, "ConcreteItem");
    assert_primitive(&props["value"].ty, PrimitiveName::String);
    assert_primitive(&props["active"].ty, PrimitiveName::Boolean);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}
