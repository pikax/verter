use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

use super::*;

fn parse_and_extract(alloc: &Allocator, source: &str) -> Vec<AnalyzedMacro> {
    let parser = Parser::new(alloc, source, SourceType::ts()).with_options(ParseOptions::default());
    let result = parser.parse();
    assert!(!result.panicked, "failed to parse: {source}");
    analyze_macros_from_program(&result.program, source)
}

fn parse_type_refs(type_annotation: &str) -> Vec<String> {
    // Parse as a type annotation inside a variable declaration
    let code = format!("let _x: {type_annotation};");
    let alloc = Allocator::new();
    let parser = Parser::new(&alloc, &code, SourceType::ts()).with_options(ParseOptions::default());
    let result = parser.parse();
    assert!(!result.panicked, "failed to parse: {}", code);

    // In OXC 0.112, type annotations are on VariableDeclarator, not BindingPattern
    for stmt in &result.program.body {
        if let Statement::VariableDeclaration(var_decl) = stmt {
            if let Some(decl) = var_decl.declarations.first() {
                if let Some(ref ta) = decl.type_annotation {
                    return collect_type_references(&ta.type_annotation);
                }
            }
        }
    }
    panic!("could not find type annotation in parsed code");
}

/// @ai-generated - Object literal with type reference
#[test]
fn object_with_type_ref() {
    let refs = parse_type_refs("{foo: MyType}");
    assert_eq!(refs, vec!["MyType"]);
}

/// @ai-generated - Simple type reference
#[test]
fn simple_type_ref() {
    let refs = parse_type_refs("MyType");
    assert_eq!(refs, vec!["MyType"]);
}

/// @ai-generated - Intersection type
#[test]
fn intersection_type() {
    let refs = parse_type_refs("MyType & OtherType");
    assert_eq!(refs, vec!["MyType", "OtherType"]);
}

/// @ai-generated - Only primitives, no type references
#[test]
fn only_primitives() {
    let refs = parse_type_refs("{foo: string, bar: number}");
    assert!(refs.is_empty());
}

/// @ai-generated - Generic type with nested type reference
#[test]
fn generic_with_nested_ref() {
    let refs = parse_type_refs("Partial<MyType>");
    assert_eq!(refs, vec!["Partial", "MyType"]);
}

/// @ai-generated - Union type
#[test]
fn union_type() {
    let refs = parse_type_refs("MyType | OtherType");
    assert_eq!(refs, vec!["MyType", "OtherType"]);
}

/// @ai-generated - Array of type reference
#[test]
fn array_type() {
    let refs = parse_type_refs("MyType[]");
    assert_eq!(refs, vec!["MyType"]);
}

/// @ai-generated - Nested object with type refs
#[test]
fn nested_object() {
    let refs = parse_type_refs("{foo: {bar: MyType}, baz: OtherType}");
    assert_eq!(refs, vec!["MyType", "OtherType"]);
}

/// @ai-generated - Conditional type
#[test]
fn conditional_type() {
    let refs = parse_type_refs("A extends B ? C : D");
    assert_eq!(refs, vec!["A", "B", "C", "D"]);
}

/// @ai-generated - Mapped type
#[test]
fn mapped_type() {
    let refs = parse_type_refs("{[K in keyof T]: V}");
    assert_eq!(refs, vec!["T", "V"]);
}

/// @ai-generated - Indexed access type
#[test]
fn indexed_access() {
    let refs = parse_type_refs("T[K]");
    assert_eq!(refs, vec!["T", "K"]);
}

#[test]
fn tuple_with_optional_element() {
    // Regression: TSTupleElement::TSOptionalType panics with to_ts_type()
    let refs = parse_type_refs("[string, MyType?]");
    assert_eq!(refs, vec!["MyType"]);
}

#[test]
fn tuple_with_rest_element() {
    // Regression: TSTupleElement::TSRestType panics with to_ts_type()
    let refs = parse_type_refs("[string, ...MyType[]]");
    assert_eq!(refs, vec!["MyType"]);
}

#[test]
fn tuple_with_named_element() {
    let refs = parse_type_refs("[name: string, value: MyType]");
    assert_eq!(refs, vec!["MyType"]);
}

fn parse_macros(code: &str) -> Vec<AnalyzedMacro> {
    let alloc = Allocator::new();
    let parser = Parser::new(&alloc, code, SourceType::ts()).with_options(ParseOptions::default());
    let result = parser.parse();
    assert!(!result.panicked, "failed to parse: {}", code);
    analyze_macros_from_program(&result.program, code)
}

/// @ai-generated - Detect defineProps with type param
#[test]
fn detect_define_props_type_based() {
    let macros = parse_macros("const props = defineProps<{foo: MyType}>()");
    assert_eq!(macros.len(), 1);
    assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineProps);
    assert!(macros[0].is_type_based);
    assert_eq!(macros[0].type_references, vec!["MyType"]);
    assert_eq!(macros[0].binding_name.as_deref(), Some("props"));
}

/// @ai-generated - Detect defineProps without type param
#[test]
fn detect_define_props_runtime() {
    let macros = parse_macros("const props = defineProps({foo: String})");
    assert_eq!(macros.len(), 1);
    assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineProps);
    assert!(!macros[0].is_type_based);
    assert!(macros[0].type_references.is_empty());
}

/// @ai-generated - Detect defineEmits
#[test]
fn detect_define_emits() {
    let macros = parse_macros("const emit = defineEmits<{(e: 'click'): void}>()");
    assert_eq!(macros.len(), 1);
    assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineEmits);
    assert!(macros[0].is_type_based);
}

/// @ai-generated - Detect defineModel
#[test]
fn detect_define_model() {
    let macros = parse_macros("const model = defineModel<string>()");
    assert_eq!(macros.len(), 1);
    assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineModel);
    assert!(macros[0].is_type_based);
}

/// @ai-generated - Bare macro call (no binding)
#[test]
fn bare_macro_call() {
    let macros = parse_macros("defineExpose({})");
    assert_eq!(macros.len(), 1);
    assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineExpose);
    assert!(macros[0].binding_name.is_none());
}

/// @ai-generated - Imported type reference in defineProps
#[test]
fn imported_type_in_define_props() {
    let macros = parse_macros("defineProps<MyImportedType>()");
    assert_eq!(macros.len(), 1);
    assert_eq!(macros[0].type_references, vec!["MyImportedType"]);
}

/// @ai-generated - Multiple macros
#[test]
fn multiple_macros() {
    let code = r#"
const props = defineProps<{foo: string}>()
const emit = defineEmits<{(e: 'click'): void}>()
defineExpose({ props })
"#;
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 3);
}

/// @ai-generated - withDefaults wrapping defineProps extracts both macros
#[test]
fn with_defaults_extracts_inner_define_props() {
    let code = r#"const props = withDefaults(defineProps<{foo: MyType}>(), { foo: 'bar' })"#;
    let macros = parse_macros(code);
    assert!(
        macros.len() >= 2,
        "should extract both withDefaults and defineProps, got {}",
        macros.len()
    );
    assert!(
        macros
            .iter()
            .any(|m| m.kind == AnalyzedMacroKind::WithDefaults),
        "should have withDefaults"
    );
    assert!(
        macros
            .iter()
            .any(|m| m.kind == AnalyzedMacroKind::DefineProps),
        "should have defineProps"
    );
    // The inner defineProps should capture type references
    let define_props = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();
    assert!(
        define_props.is_type_based,
        "inner defineProps should be type-based"
    );
    assert!(
        define_props.type_references.contains(&"MyType".to_string()),
        "inner defineProps should capture type references"
    );
}

/// @ai-generated - import("./foo").Bar in type refs returns empty (not tracked)
#[test]
fn import_type_in_type_refs_returns_empty() {
    // TSImportType is intentionally not tracked — returns empty
    let refs = parse_type_refs("import('./foo').Bar");
    assert!(
        refs.is_empty(),
        "import() type references should not be collected, got: {:?}",
        refs
    );
}

/// @ai-generated - typeof X in type refs extracts the identifier
#[test]
fn typeof_in_type_refs_extracts_identifier() {
    // TSTypeQuery (typeof) should collect the referenced identifier
    let refs = parse_type_refs("typeof X");
    assert_eq!(refs, vec!["X".to_string()]);
}

// =========================================================================
// Prop field extraction tests
// =========================================================================

#[test]
fn prop_fields_type_based_literal() {
    let code = "defineProps<{ count: number, name: string }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].prop_fields;
    assert_eq!(
        fields.len(),
        2,
        "should extract 2 prop fields: {:?}",
        fields
    );
    assert_eq!(fields[0].name, "count");
    assert_eq!(fields[1].name, "name");
    // Verify spans point to prop keys
    assert_eq!(
        &code[fields[0].span.start as usize..fields[0].span.end as usize],
        "count"
    );
    assert_eq!(
        &code[fields[1].span.start as usize..fields[1].span.end as usize],
        "name"
    );
}

#[test]
fn prop_fields_type_based_with_assignment() {
    let code = "const props = defineProps<{ msg: string }>()";
    let macros = parse_macros(code);
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();
    assert_eq!(dp.prop_fields.len(), 1);
    assert_eq!(dp.prop_fields[0].name, "msg");
    assert_eq!(
        &code[dp.prop_fields[0].span.start as usize..dp.prop_fields[0].span.end as usize],
        "msg"
    );
}

#[test]
fn prop_fields_runtime_object() {
    let code = "defineProps({ count: { type: Number }, name: String })";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].prop_fields;
    assert_eq!(
        fields.len(),
        2,
        "should extract 2 runtime prop fields: {:?}",
        fields
    );
    assert_eq!(fields[0].name, "count");
    assert_eq!(fields[1].name, "name");
    assert_eq!(
        &code[fields[0].span.start as usize..fields[0].span.end as usize],
        "count"
    );
    assert_eq!(
        &code[fields[1].span.start as usize..fields[1].span.end as usize],
        "name"
    );
}

#[test]
fn prop_fields_runtime_array() {
    let code = "defineProps(['count', 'name'])";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].prop_fields;
    assert_eq!(
        fields.len(),
        2,
        "should extract 2 array prop fields: {:?}",
        fields
    );
    assert_eq!(fields[0].name, "count");
    assert_eq!(fields[1].name, "name");
}

#[test]
fn prop_fields_with_defaults() {
    let code = "withDefaults(defineProps<{ msg: string, count: number }>(), { msg: 'hi' })";
    let macros = parse_macros(code);
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();
    assert_eq!(
        dp.prop_fields.len(),
        2,
        "withDefaults inner defineProps should have prop fields: {:?}",
        dp.prop_fields
    );
    assert_eq!(dp.prop_fields[0].name, "msg");
    assert_eq!(dp.prop_fields[1].name, "count");
}

#[test]
fn prop_fields_non_define_props_empty() {
    let code = "defineEmits<{(e: 'click'): void}>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    assert!(
        macros[0].prop_fields.is_empty(),
        "defineEmits should have no prop fields"
    );
}

#[test]
fn prop_fields_type_reference_empty() {
    // Interface reference — can't resolve inline, prop_fields is empty
    let code = "defineProps<MyProps>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    assert!(
        macros[0].prop_fields.is_empty(),
        "type reference should yield empty prop fields"
    );
}

// =========================================================================
// Emit field extraction tests
// =========================================================================

#[test]
fn emit_fields_type_based_property_signature() {
    let code = "defineEmits<{ custom: [payload: string]; click: [] }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].emit_fields;
    assert_eq!(
        fields.len(),
        2,
        "should extract 2 emit fields: {:?}",
        fields
    );
    assert_eq!(fields[0].name, "custom");
    assert_eq!(fields[1].name, "click");
    // Verify spans point to event name keys
    assert_eq!(
        &code[fields[0].span.start as usize..fields[0].span.end as usize],
        "custom"
    );
    assert_eq!(
        &code[fields[1].span.start as usize..fields[1].span.end as usize],
        "click"
    );
}

#[test]
fn emit_fields_type_based_call_signature() {
    let code = "defineEmits<{ (e: 'change', id: number): void }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].emit_fields;
    assert_eq!(
        fields.len(),
        1,
        "should extract 1 emit field from call signature: {:?}",
        fields
    );
    assert_eq!(fields[0].name, "change");
}

#[test]
fn emit_fields_type_based_mixed_signatures() {
    let code = "defineEmits<{ (e: 'change', id: number): void; custom: [payload: string] }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].emit_fields;
    assert_eq!(
        fields.len(),
        2,
        "should extract 2 emit fields from mixed signatures: {:?}",
        fields
    );
    assert_eq!(fields[0].name, "change");
    assert_eq!(fields[1].name, "custom");
}

#[test]
fn emit_fields_runtime_array() {
    let code = "defineEmits(['custom', 'click'])";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].emit_fields;
    assert_eq!(
        fields.len(),
        2,
        "should extract 2 runtime array emit fields: {:?}",
        fields
    );
    assert_eq!(fields[0].name, "custom");
    assert_eq!(fields[1].name, "click");
}

#[test]
fn emit_fields_runtime_object() {
    let code = "defineEmits({ custom: null })";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].emit_fields;
    assert_eq!(
        fields.len(),
        1,
        "should extract 1 runtime object emit field: {:?}",
        fields
    );
    assert_eq!(fields[0].name, "custom");
}

#[test]
fn emit_fields_non_define_emits_empty() {
    let code = "defineProps<{ count: number }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    assert!(
        macros[0].emit_fields.is_empty(),
        "defineProps should have no emit fields"
    );
}

#[test]
fn emit_fields_type_reference_empty() {
    // Interface reference — can't resolve inline, emit_fields is empty
    let code = "defineEmits<MyEvents>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    assert!(
        macros[0].emit_fields.is_empty(),
        "type reference should yield empty emit fields"
    );
}

#[test]
fn prop_fields_intersection_type() {
    let code = "defineProps<{ a: string } & { b: number }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].prop_fields;
    assert_eq!(
        fields.len(),
        2,
        "intersection should merge fields: {:?}",
        fields
    );
    assert_eq!(fields[0].name, "a");
    assert_eq!(fields[1].name, "b");
}

// =========================================================================
// Type annotation extraction tests
// =========================================================================

#[test]
fn prop_field_type_annotation_string_literal_union() {
    let code = "defineProps<{ variant: 'primary' | 'secondary' }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let field = &macros[0].prop_fields[0];
    assert_eq!(field.name, "variant");
    assert_eq!(
        field.type_annotation.as_deref(),
        Some("'primary' | 'secondary'"),
        "should capture string literal union type annotation"
    );
}

#[test]
fn prop_field_type_annotation_primitive() {
    let code = "defineProps<{ count: number }>()";
    let macros = parse_macros(code);
    let field = &macros[0].prop_fields[0];
    assert_eq!(field.name, "count");
    assert_eq!(
        field.type_annotation.as_deref(),
        Some("number"),
        "should capture primitive type annotation"
    );
}

#[test]
fn prop_field_type_annotation_runtime_constructor() {
    let code = "defineProps({ count: Number })";
    let macros = parse_macros(code);
    let field = &macros[0].prop_fields[0];
    assert_eq!(field.name, "count");
    // Runtime constructor shorthand is mapped to TS type
    assert_eq!(field.type_annotation.as_deref(), Some("number"));
}

#[test]
fn prop_field_type_annotation_multiple() {
    let code = "defineProps<{ variant: 'a' | 'b', size: 'sm' | 'lg' }>()";
    let macros = parse_macros(code);
    let fields = &macros[0].prop_fields;
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].type_annotation.as_deref(), Some("'a' | 'b'"));
    assert_eq!(fields[1].type_annotation.as_deref(), Some("'sm' | 'lg'"));
}

// =========================================================================
// Slot field extraction tests
// =========================================================================

#[test]
fn slot_fields_property_signature() {
    let code = "defineSlots<{ default(props: {}): any; header?(props: {}): any }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].slot_fields;
    assert_eq!(
        fields.len(),
        2,
        "should extract 2 slot fields: {:?}",
        fields
    );
    assert_eq!(fields[0].name, "default");
    assert!(fields[0].is_required, "default should be required (no ?)");
    assert_eq!(fields[1].name, "header");
    assert!(!fields[1].is_required, "header should be optional (has ?)");
    // Negative: non-defineSlots macros should NOT have slot_fields
    assert!(
        macros[0].prop_fields.is_empty(),
        "defineSlots should not have prop_fields"
    );
}

#[test]
fn slot_fields_method_signature() {
    // Method shorthand syntax: `default(props: {}): any` vs `default?(props: {}): any`
    let code = "defineSlots<{ default(props: { item: string }): any; footer?(props: {}): any }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].slot_fields;
    assert_eq!(
        fields.len(),
        2,
        "should extract 2 slot fields: {:?}",
        fields
    );
    assert_eq!(fields[0].name, "default");
    assert!(
        fields[0].is_required,
        "default (method, no ?) should be required"
    );
    assert_eq!(fields[1].name, "footer");
    assert!(
        !fields[1].is_required,
        "footer? (method, ?) should be optional"
    );
}

#[test]
fn slot_fields_intersection_type() {
    let code = "defineSlots<{ default(p: {}): any } & { sidebar?(p: {}): any }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].slot_fields;
    assert_eq!(
        fields.len(),
        2,
        "intersection should merge slot fields: {:?}",
        fields
    );
    assert_eq!(fields[0].name, "default");
    assert!(fields[0].is_required);
    assert_eq!(fields[1].name, "sidebar");
    assert!(!fields[1].is_required);
}

#[test]
fn slot_fields_type_reference_empty() {
    let code = "defineSlots<MySlots>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    assert!(
        macros[0].slot_fields.is_empty(),
        "type reference should yield empty slot fields"
    );
}

#[test]
fn slot_fields_no_type_params() {
    let code = "defineSlots()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    assert!(
        macros[0].slot_fields.is_empty(),
        "no type params should yield empty slot fields"
    );
}

#[test]
fn slot_fields_not_on_other_macros() {
    let code = "defineProps<{ count: number }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    assert!(
        macros[0].slot_fields.is_empty(),
        "defineProps should not have slot_fields"
    );
}

#[test]
fn slot_fields_all_required() {
    let code = "defineSlots<{ default(p: {}): any; header(p: {}): any; footer(p: {}): any }>()";
    let macros = parse_macros(code);
    let fields = &macros[0].slot_fields;
    assert_eq!(fields.len(), 3);
    for field in fields {
        assert!(
            field.is_required,
            "slot '{}' should be required",
            field.name
        );
    }
}

#[test]
fn slot_fields_all_optional() {
    let code = "defineSlots<{ default?(p: {}): any; header?(p: {}): any }>()";
    let macros = parse_macros(code);
    let fields = &macros[0].slot_fields;
    assert_eq!(fields.len(), 2);
    for field in fields {
        assert!(
            !field.is_required,
            "slot '{}' should be optional",
            field.name
        );
    }
}

// =========================================================================
// Slot field binding extraction tests
// =========================================================================

#[test]
fn slot_fields_method_bindings() {
    let code = "defineSlots<{ default(props: { item: string, index: number }): any }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].slot_fields;
    assert_eq!(fields.len(), 1);
    let bindings = &fields[0].bindings;
    assert_eq!(
        bindings.len(),
        2,
        "should extract 2 bindings: {:?}",
        bindings
    );
    assert_eq!(bindings[0].name, "item");
    assert_eq!(bindings[0].type_annotation.as_deref(), Some("string"));
    assert_eq!(bindings[1].name, "index");
    assert_eq!(bindings[1].type_annotation.as_deref(), Some("number"));
    // Negative: no binding named "props" (that's the param name, not a binding)
    assert!(
        !bindings.iter().any(|b| b.name == "props"),
        "should not include 'props' as a binding name"
    );
}

#[test]
fn slot_fields_property_fn_bindings() {
    let code = "defineSlots<{ default: (props: { row: MyItem }) => any }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].slot_fields;
    assert_eq!(fields.len(), 1);
    let bindings = &fields[0].bindings;
    assert_eq!(
        bindings.len(),
        1,
        "should extract 1 binding: {:?}",
        bindings
    );
    assert_eq!(bindings[0].name, "row");
    // Negative: type_annotation must NOT be None
    assert!(
        bindings[0].type_annotation.is_some(),
        "type_annotation should be present, not None"
    );
    assert_eq!(bindings[0].type_annotation.as_deref(), Some("MyItem"));
}

#[test]
fn slot_fields_no_params_empty_bindings() {
    let code = "defineSlots<{ header(): any }>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].slot_fields;
    assert_eq!(fields.len(), 1);
    assert!(
        fields[0].bindings.is_empty(),
        "slot with no params should have empty bindings"
    );
}

#[test]
fn slot_fields_complex_type_bindings() {
    let code =
        "defineSlots<{ default(props: { items: string[], active: boolean | null }): any }>()";
    let macros = parse_macros(code);
    let fields = &macros[0].slot_fields;
    assert_eq!(fields.len(), 1);
    let bindings = &fields[0].bindings;
    assert_eq!(
        bindings.len(),
        2,
        "should extract 2 bindings: {:?}",
        bindings
    );
    assert_eq!(bindings[0].name, "items");
    assert_eq!(bindings[0].type_annotation.as_deref(), Some("string[]"));
    assert_eq!(bindings[1].name, "active");
    assert_eq!(
        bindings[1].type_annotation.as_deref(),
        Some("boolean | null")
    );
}

#[test]
fn slot_fields_multiple_slots_bindings() {
    let code = "defineSlots<{ default(props: { item: string }): any; header(props: { title: number }): any }>()";
    let macros = parse_macros(code);
    let fields = &macros[0].slot_fields;
    assert_eq!(fields.len(), 2);
    // First slot
    assert_eq!(fields[0].bindings.len(), 1);
    assert_eq!(fields[0].bindings[0].name, "item");
    assert_eq!(
        fields[0].bindings[0].type_annotation.as_deref(),
        Some("string")
    );
    // Second slot
    assert_eq!(fields[1].bindings.len(), 1);
    assert_eq!(fields[1].bindings[0].name, "title");
    assert_eq!(
        fields[1].bindings[0].type_annotation.as_deref(),
        Some("number")
    );
    // Negative: no cross-contamination
    assert!(
        !fields[0].bindings.iter().any(|b| b.name == "title"),
        "default slot should not have header's bindings"
    );
    assert!(
        !fields[1].bindings.iter().any(|b| b.name == "item"),
        "header slot should not have default's bindings"
    );
}

#[test]
fn slot_fields_intersection_bindings() {
    let code =
        "defineSlots<{ default(p: { a: string }): any } & { footer(p: { b: number }): any }>()";
    let macros = parse_macros(code);
    let fields = &macros[0].slot_fields;
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name, "default");
    assert_eq!(fields[0].bindings.len(), 1);
    assert_eq!(fields[0].bindings[0].name, "a");
    assert_eq!(
        fields[0].bindings[0].type_annotation.as_deref(),
        Some("string")
    );
    assert_eq!(fields[1].name, "footer");
    assert_eq!(fields[1].bindings.len(), 1);
    assert_eq!(fields[1].bindings[0].name, "b");
    assert_eq!(
        fields[1].bindings[0].type_annotation.as_deref(),
        Some("number")
    );
}

#[test]
fn slot_field_binding_span_is_correct() {
    let code = "defineSlots<{ default: (props: { item: string, index: number }) => any }>()";
    let macros = parse_macros(code);
    let bindings = &macros[0].slot_fields[0].bindings;
    assert_eq!(bindings.len(), 2);

    // Verify span points to the binding key in source
    assert_eq!(
        &code[bindings[0].span.start as usize..bindings[0].span.end as usize],
        "item"
    );
    assert_eq!(
        &code[bindings[1].span.start as usize..bindings[1].span.end as usize],
        "index"
    );

    // Negative: span should not be zero
    assert!(
        bindings[0].span.start > 0 || bindings[0].span.end > 0,
        "item span should be non-zero"
    );
    assert!(
        bindings[1].span.start > 0 || bindings[1].span.end > 0,
        "index span should be non-zero"
    );
}

// ── JSDoc extraction tests ───────────────────────────────────

#[test]
fn jsdoc_on_prop_fields() {
    let code = r#"defineProps<{
        /** The display label */
        label: string
        /** Size variant
         * @default 'md'
         */
        size: string
        noDoc: number
    }>()"#;
    let macros = parse_macros(code);
    let fields = &macros[0].prop_fields;

    assert_eq!(fields.len(), 3);

    // label has description, no tags
    assert_eq!(fields[0].description.as_deref(), Some("The display label"));
    assert!(fields[0].tags.is_empty());

    // size has description and @default tag
    assert_eq!(fields[1].description.as_deref(), Some("Size variant"));
    assert_eq!(fields[1].tags.len(), 1);
    assert_eq!(fields[1].tags[0].name, "default");
    assert_eq!(fields[1].tags[0].text.as_deref(), Some("'md'"));

    // noDoc has no JSDoc
    assert!(fields[2].description.is_none());
    assert!(fields[2].tags.is_empty());
}

#[test]
fn jsdoc_on_runtime_prop_fields() {
    let code = r#"defineProps({
        /** The display label */
        label: String,
        /** Size variant
         * @default 'md'
         */
        size: { type: String, default: 'md' },
        noDoc: Number,
    })"#;
    let macros = parse_macros(code);
    let fields = &macros[0].prop_fields;
    assert_eq!(fields.len(), 3);

    // Positive: label has description, no tags
    assert_eq!(fields[0].description.as_deref(), Some("The display label"));
    assert!(fields[0].tags.is_empty());

    // Positive: size has description and @default tag
    assert_eq!(fields[1].description.as_deref(), Some("Size variant"));
    assert_eq!(fields[1].tags.len(), 1);
    assert_eq!(fields[1].tags[0].name, "default");
    assert_eq!(fields[1].tags[0].text.as_deref(), Some("'md'"));

    // Negative: noDoc has no JSDoc
    assert!(fields[2].description.is_none());
    assert!(fields[2].tags.is_empty());
}

#[test]
fn jsdoc_on_emit_fields() {
    let code = r#"defineEmits<{
        /** Fired on click */
        click: []
        /** @deprecated use 'input' instead */
        change: [value: string]
    }>()"#;
    let macros = parse_macros(code);
    let fields = &macros[0].emit_fields;

    assert_eq!(fields.len(), 2);

    assert_eq!(fields[0].description.as_deref(), Some("Fired on click"));
    assert!(fields[0].tags.is_empty());

    assert!(
        fields[1].description.is_none(),
        "tag-only JSDoc should not have description"
    );
    assert_eq!(fields[1].tags.len(), 1);
    assert_eq!(fields[1].tags[0].name, "deprecated");
    assert_eq!(
        fields[1].tags[0].text.as_deref(),
        Some("use 'input' instead")
    );
}

#[test]
fn jsdoc_on_slot_fields() {
    let code = r#"defineSlots<{
        /** The main content area */
        default(props: { item: string }): any
    }>()"#;
    let macros = parse_macros(code);
    let fields = &macros[0].slot_fields;

    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].description.as_deref(),
        Some("The main content area")
    );
    assert!(fields[0].tags.is_empty());
}

#[test]
fn jsdoc_with_multiple_tags() {
    let code = r#"defineProps<{
        /**
         * User identifier
         * @param {string} id - The user ID
         * @deprecated Use userId instead
         * @see https://example.com
         */
        id: string
    }>()"#;
    let macros = parse_macros(code);
    let fields = &macros[0].prop_fields;

    assert_eq!(fields[0].description.as_deref(), Some("User identifier"));
    assert_eq!(fields[0].tags.len(), 3);
    assert_eq!(fields[0].tags[0].name, "param");
    assert_eq!(
        fields[0].tags[0].text.as_deref(),
        Some("{string} id - The user ID")
    );
    assert_eq!(fields[0].tags[1].name, "deprecated");
    assert_eq!(
        fields[0].tags[1].text.as_deref(),
        Some("Use userId instead")
    );
    assert_eq!(fields[0].tags[2].name, "see");
    assert_eq!(
        fields[0].tags[2].text.as_deref(),
        Some("https://example.com")
    );
}

#[test]
fn no_jsdoc_produces_none_and_empty() {
    let code = r#"defineProps<{ count: number }>()"#;
    let macros = parse_macros(code);
    let fields = &macros[0].prop_fields;

    assert_eq!(fields.len(), 1);
    assert!(fields[0].description.is_none());
    assert!(fields[0].tags.is_empty());
}

// =========================================================================
// Issue 1: Prop field is_optional
// =========================================================================

#[test]
fn prop_field_optional_type_based() {
    let code = "defineProps<{ name?: string, count: number }>()";
    let macros = parse_macros(code);
    let fields = &macros[0].prop_fields;
    assert_eq!(fields.len(), 2);
    assert!(fields[0].is_optional, "name? should be optional");
    assert!(
        !fields[1].is_optional,
        "count (no ?) should NOT be optional"
    );
}

#[test]
fn prop_field_optional_runtime_default() {
    // Vue semantics: runtime props are optional by default (unless required: true)
    let code = "defineProps({ count: Number })";
    let macros = parse_macros(code);
    let field = &macros[0].prop_fields[0];
    assert!(
        field.is_optional,
        "runtime props without required:true should be optional (Vue default)"
    );
}

#[test]
fn prop_field_optional_array_default() {
    // Vue semantics: array-form props have no required info → optional by default
    let code = "defineProps(['count'])";
    let macros = parse_macros(code);
    let field = &macros[0].prop_fields[0];
    assert!(
        field.is_optional,
        "array-form props should be optional by default"
    );
}

// =========================================================================
// Issue 2: withDefaults default_keys
// =========================================================================

#[test]
fn with_defaults_extracts_default_keys() {
    let code = r#"withDefaults(defineProps<{ foo: string, bar: number, baz: boolean }>(), { foo: 'hello', baz: true })"#;
    let macros = parse_macros(code);
    let wd = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::WithDefaults)
        .unwrap();
    let mut keys = wd.default_keys.clone();
    keys.sort();
    assert_eq!(
        keys,
        vec!["baz", "foo"],
        "should extract default keys from object literal"
    );
    // Negative: defineProps should NOT have default_keys
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();
    assert!(
        dp.default_keys.is_empty(),
        "defineProps should have empty default_keys"
    );
}

#[test]
fn with_defaults_no_object_arg_empty_keys() {
    // withDefaults with non-object second arg (rare, but should not crash)
    let code = "withDefaults(defineProps<{ foo: string }>(), defaults)";
    let macros = parse_macros(code);
    let wd = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::WithDefaults)
        .unwrap();
    assert!(
        wd.default_keys.is_empty(),
        "non-object second arg should yield empty default_keys"
    );
}

#[test]
fn with_defaults_extracts_default_values() {
    let code = r#"withDefaults(defineProps<{ foo: string, bar: number, baz: boolean }>(), { foo: 'hello', baz: true })"#;
    let macros = parse_macros(code);
    let wd = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::WithDefaults)
        .unwrap();
    assert_eq!(
        wd.default_values.len(),
        2,
        "should extract 2 default values"
    );
    let foo_val = wd.default_values.iter().find(|d| d.key == "foo").unwrap();
    assert_eq!(foo_val.value, "hello", "string default should strip quotes");
    let baz_val = wd.default_values.iter().find(|d| d.key == "baz").unwrap();
    assert_eq!(baz_val.value, "true");
}

// =========================================================================
// Issue 4: defineExpose expose_fields
// =========================================================================

#[test]
fn define_expose_extracts_fields() {
    let code = "defineExpose({ foo, bar, baz: computed(() => 1) })";
    let macros = parse_macros(code);
    let de = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineExpose)
        .unwrap();
    assert_eq!(de.expose_fields.len(), 3, "should extract 3 expose fields");
    assert_eq!(de.expose_fields[0].name, "foo");
    assert_eq!(de.expose_fields[1].name, "bar");
    assert_eq!(de.expose_fields[2].name, "baz");
}

#[test]
fn define_expose_empty_object() {
    let code = "defineExpose({})";
    let macros = parse_macros(code);
    let de = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineExpose)
        .unwrap();
    assert!(
        de.expose_fields.is_empty(),
        "empty object should yield empty expose_fields"
    );
}

#[test]
fn define_expose_no_args() {
    let code = "defineExpose()";
    let macros = parse_macros(code);
    let de = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineExpose)
        .unwrap();
    assert!(
        de.expose_fields.is_empty(),
        "no args should yield empty expose_fields"
    );
}

#[test]
fn define_expose_identifier_arg_empty() {
    let code = "defineExpose(myObj)";
    let macros = parse_macros(code);
    let de = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineExpose)
        .unwrap();
    assert!(
        de.expose_fields.is_empty(),
        "identifier arg should yield empty expose_fields (can't resolve)"
    );
}

#[test]
fn expose_fields_not_on_other_macros() {
    let code = "defineProps<{ count: number }>()";
    let macros = parse_macros(code);
    assert!(
        macros[0].expose_fields.is_empty(),
        "defineProps should not have expose_fields"
    );
}

// =========================================================================
// Issue 5: Emit field payload_type
// =========================================================================

#[test]
fn emit_field_payload_type_property_signature() {
    let code = "defineEmits<{ change: [id: number]; click: [] }>()";
    let macros = parse_macros(code);
    let fields = &macros[0].emit_fields;
    assert_eq!(fields.len(), 2);
    assert_eq!(
        fields[0].payload_type.as_deref(),
        Some("[id: number]"),
        "change should have payload type"
    );
    assert_eq!(
        fields[1].payload_type.as_deref(),
        Some("[]"),
        "click should have empty tuple payload"
    );
}

#[test]
fn emit_field_payload_type_call_signature() {
    let code = "defineEmits<{ (e: 'change', id: number): void }>()";
    let macros = parse_macros(code);
    let fields = &macros[0].emit_fields;
    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].payload_type.as_deref(),
        Some("[id: number]"),
        "call signature should extract params after event name as tuple"
    );
}

#[test]
fn emit_field_payload_type_call_signature_no_payload() {
    let code = "defineEmits<{ (e: 'click'): void }>()";
    let macros = parse_macros(code);
    let fields = &macros[0].emit_fields;
    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].payload_type.as_deref(),
        Some("[]"),
        "call signature with no extra params should have empty tuple"
    );
}

#[test]
fn emit_field_payload_type_runtime_none() {
    let code = "defineEmits(['click'])";
    let macros = parse_macros(code);
    let fields = &macros[0].emit_fields;
    assert_eq!(fields.len(), 1);
    assert!(
        fields[0].payload_type.is_none(),
        "runtime emits should have no payload type"
    );
}

#[test]
fn parse_jsdoc_unit_tests() {
    // Simple description
    let (desc, tags) = crate::analysis::jsdoc::parse_jsdoc("/** Hello world */");
    assert_eq!(desc.as_deref(), Some("Hello world"));
    assert!(tags.is_empty());

    // Tag only
    let (desc, tags) = crate::analysis::jsdoc::parse_jsdoc("/** @deprecated */");
    assert!(desc.is_none());
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "deprecated");
    assert!(tags[0].text.is_none());

    // Tag with text
    let (desc, tags) = crate::analysis::jsdoc::parse_jsdoc("/** @default 'hello' */");
    assert!(desc.is_none());
    assert_eq!(tags[0].name, "default");
    assert_eq!(tags[0].text.as_deref(), Some("'hello'"));

    // Multi-line
    let (desc, tags) = crate::analysis::jsdoc::parse_jsdoc(
        "/**\n * A description\n * @param name - the name\n * @returns nothing\n */",
    );
    assert_eq!(desc.as_deref(), Some("A description"));
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0].name, "param");
    assert_eq!(tags[0].text.as_deref(), Some("name - the name"));
    assert_eq!(tags[1].name, "returns");
    assert_eq!(tags[1].text.as_deref(), Some("nothing"));
}

// =========================================================================
// defineModel type extraction
// =========================================================================

#[test]
fn define_model_type_string() {
    let code = "defineModel<string>()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineModel);
    let fields = &macros[0].prop_fields;
    assert_eq!(
        fields.len(),
        1,
        "defineModel<string> should produce 1 prop field"
    );
    assert_eq!(fields[0].name, "modelValue");
    assert_eq!(
        fields[0].type_annotation.as_deref(),
        Some("string"),
        "type_annotation should be 'string'"
    );
    assert!(fields[0].is_optional);
}

#[test]
fn define_model_named_with_type() {
    let code = "defineModel<number>('count')";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let fields = &macros[0].prop_fields;
    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].name, "count",
        "named model should use the name argument"
    );
    assert_eq!(fields[0].type_annotation.as_deref(), Some("number"));
}

#[test]
fn define_model_complex_type() {
    let code = "defineModel<string | number>()";
    let macros = parse_macros(code);
    let fields = &macros[0].prop_fields;
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "modelValue");
    assert_eq!(
        fields[0].type_annotation.as_deref(),
        Some("string | number")
    );
}

#[test]
fn define_model_no_type_param() {
    let code = "defineModel()";
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    assert!(
        macros[0].prop_fields.is_empty(),
        "defineModel without type param should have no prop_fields"
    );
}

#[test]
fn define_model_required_option_marks_prop_required() {
    let code = "defineModel<string>({ required: true })";
    let macros = parse_macros(code);
    let fields = &macros[0].prop_fields;

    assert_eq!(fields.len(), 1);
    assert!(
        !fields[0].is_optional,
        "defineModel({{ required: true }}) should keep the generated prop required"
    );
}

// ── Type resolution tests ──

#[test]
fn resolve_local_interface_in_define_props() {
    let source = r#"
        interface Props { title: string; count: number; active?: boolean }
        defineProps<Props>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();
    assert_eq!(
        dp.prop_fields.len(),
        3,
        "should resolve 3 fields from local interface"
    );
    assert_eq!(dp.prop_fields[0].name, "title");
    assert_eq!(dp.prop_fields[0].type_annotation.as_deref(), Some("string"));
    assert!(!dp.prop_fields[0].is_optional);
    assert_eq!(dp.prop_fields[1].name, "count");
    assert_eq!(dp.prop_fields[1].type_annotation.as_deref(), Some("number"));
    assert_eq!(dp.prop_fields[2].name, "active");
    assert!(dp.prop_fields[2].is_optional);
    assert!(
        !dp.prop_fields.iter().any(|f| f.resolution_error.is_some()),
        "all fields should be resolved without errors"
    );
}

#[test]
fn resolve_local_type_alias_in_define_props() {
    let source = r#"
        type MyProps = { name: string; age?: number }
        defineProps<MyProps>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();
    assert_eq!(
        dp.prop_fields.len(),
        2,
        "should resolve 2 fields from type alias"
    );
    assert_eq!(dp.prop_fields[0].name, "name");
    assert!(dp.prop_fields[1].is_optional);
}

#[test]
fn resolve_interface_extends_chain() {
    let source = r#"
        interface Base { id: number; name: string }
        interface Extended extends Base { email: string; active?: boolean }
        defineProps<Extended>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();
    assert_eq!(
        dp.prop_fields.len(),
        4,
        "should have all 4 fields (2 inherited + 2 own)"
    );
    let names: Vec<&str> = dp.prop_fields.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"id"), "should have inherited 'id'");
    assert!(names.contains(&"name"), "should have inherited 'name'");
    assert!(names.contains(&"email"), "should have own 'email'");
    assert!(names.contains(&"active"), "should have own 'active'");
}

#[test]
fn resolve_mixed_intersection_type() {
    let source = r#"
        interface Identifiable { id: number }
        type Named = { name: string; label?: string }
        defineProps<Identifiable & Named & { extra: boolean }>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();
    assert_eq!(
        dp.prop_fields.len(),
        4,
        "should merge all 4 fields from intersection"
    );
    let names: Vec<&str> = dp.prop_fields.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"id"));
    assert!(names.contains(&"name"));
    assert!(names.contains(&"label"));
    assert!(names.contains(&"extra"));
    let label = dp.prop_fields.iter().find(|f| f.name == "label").unwrap();
    assert!(label.is_optional, "label should be optional");
}

#[test]
fn resolve_partial_wrapping_local_interface() {
    let source = r#"
        interface Props { title: string; count: number }
        defineProps<Partial<Props>>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();
    assert_eq!(dp.prop_fields.len(), 2);
    assert!(
        dp.prop_fields.iter().all(|f| f.is_optional),
        "Partial should make all fields optional"
    );
}

#[test]
fn unresolvable_class_type_returns_empty() {
    let source = r#"
        class UserModel { constructor(public id: number) {} }
        defineProps<{ model: UserModel }>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();
    // The inline literal has one field "model" with type "UserModel"
    assert_eq!(dp.prop_fields.len(), 1);
    assert_eq!(dp.prop_fields[0].name, "model");
}

#[test]
fn resolved_local_types_populated_for_interface() {
    let source = r#"
        interface Props { title: string; count: number }
        defineProps<Props>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();
    assert_eq!(
        dp.resolved_local_types.len(),
        1,
        "should have one resolved local type"
    );
    assert_eq!(dp.resolved_local_types[0].name, "Props");
    assert!(
        dp.resolved_local_types[0]
            .expanded
            .contains("title: string"),
        "expanded text should contain field definitions"
    );
}

#[test]
fn pick_omit_return_none_unresolvable() {
    let source = r#"
        interface Full { id: number; name: string; password: string }
        defineProps<{ display: Pick<Full, 'id' | 'name'> }>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();
    // The inline literal extracts "display" field but cannot resolve Pick<Full,...>
    assert_eq!(dp.prop_fields.len(), 1);
    assert_eq!(dp.prop_fields[0].name, "display");
    assert!(
        dp.resolved_local_types.is_empty(),
        "inline wrapper refs should not be published as resolved_local_types, got {:?}",
        dp.resolved_local_types
            .iter()
            .map(|ty| ty.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn inline_nested_local_refs_do_not_publish_resolved_local_macro_roots() {
    let source = r#"
        interface User { id: number }
        defineProps<{ user: User; label?: string }>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();

    let field_names: Vec<_> = dp
        .prop_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(field_names, vec!["user", "label"]);
    assert!(
        dp.resolved_local_types.is_empty(),
        "nested inline refs should stay off resolved_local_types, got {:?}",
        dp.resolved_local_types
            .iter()
            .map(|ty| ty.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn interface_with_unresolved_heritage_keeps_own_prop_fields_without_partial_expansion() {
    let source = r#"
        interface Full { id: number; name: string }
        interface Props extends Pick<Full, 'id'> { label: string }
        defineProps<Props>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();

    let field_names: Vec<_> = dp
        .prop_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(
        field_names,
        vec!["label"],
        "unresolved heritage should still keep the interface's own members, got {:?}",
        field_names
    );
    assert!(
        dp.resolved_local_types.is_empty(),
        "resolved_local_types should not publish a partial expansion for unresolved heritage, got {:?}",
        dp.resolved_local_types
            .iter()
            .map(|ty| ty.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn runtime_define_props_extracts_type_and_default() {
    let source = r#"defineProps({ message: { type: String, default: 'Hello from JS' }, count: Number, active: { type: Boolean, default: true } })"#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let dp = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .unwrap();

    // Should extract 3 prop fields
    assert_eq!(dp.prop_fields.len(), 3);

    // message: { type: String, default: 'Hello from JS' }
    let msg = dp.prop_fields.iter().find(|f| f.name == "message").unwrap();
    assert_eq!(msg.type_annotation.as_deref(), Some("string"));

    // count: Number (shorthand)
    let cnt = dp.prop_fields.iter().find(|f| f.name == "count").unwrap();
    assert_eq!(cnt.type_annotation.as_deref(), Some("number"));

    // active: { type: Boolean, default: true }
    let act = dp.prop_fields.iter().find(|f| f.name == "active").unwrap();
    assert_eq!(act.type_annotation.as_deref(), Some("boolean"));

    // Should have default keys
    assert!(dp.default_keys.contains(&"message".to_string()));
    assert!(dp.default_keys.contains(&"active".to_string()));
    assert!(!dp.default_keys.contains(&"count".to_string()));

    // Should have default values
    let msg_default = dp
        .default_values
        .iter()
        .find(|d| d.key == "message")
        .unwrap();
    assert_eq!(msg_default.value, "Hello from JS");

    let act_default = dp
        .default_values
        .iter()
        .find(|d| d.key == "active")
        .unwrap();
    assert_eq!(act_default.value, "true");
}

// =========================================================================
// TSAsExpression type assertion extraction tests
// =========================================================================

#[test]
fn prop_ts_as_prop_type_angle() {
    // `Object as PropType<typeof Card>` should extract `typeof Card`, not `object`
    let code = "defineProps({ foz: { type: Object as PropType<typeof Card> } })";
    let macros = parse_macros(code);
    let field = &macros[0].prop_fields[0];
    assert_eq!(field.name, "foz");
    assert_eq!(
        field.type_annotation.as_deref(),
        Some("typeof Card"),
        "PropType<T> assertion should yield T, not the base constructor type"
    );
    assert!(
        field.type_annotation.as_deref() != Some("object"),
        "should not degrade to 'object'"
    );
}

#[test]
fn prop_ts_as_arrow_return() {
    // `Object as () => typeof Card` should extract `typeof Card` (the return type)
    let code = "defineProps({ baz: { type: Object as () => typeof Card } })";
    let macros = parse_macros(code);
    let field = &macros[0].prop_fields[0];
    assert_eq!(field.name, "baz");
    assert_eq!(
        field.type_annotation.as_deref(),
        Some("typeof Card"),
        "() => T assertion should yield T, not the callable type"
    );
    assert!(
        field.type_annotation.as_deref() != Some("object"),
        "should not degrade to 'object'"
    );
    assert!(
        field.type_annotation.as_deref() != Some("Function"),
        "should not degrade to 'Function'"
    );
}

#[test]
fn prop_ts_as_new_ctor_return() {
    // `Object as new () => typeof Card` should extract `typeof Card` (the return type)
    let code = "defineProps({ comp: { type: Object as new () => typeof Card } })";
    let macros = parse_macros(code);
    let field = &macros[0].prop_fields[0];
    assert_eq!(field.name, "comp");
    assert_eq!(
        field.type_annotation.as_deref(),
        Some("typeof Card"),
        "new () => T assertion should yield T"
    );
    assert!(
        field.type_annotation.as_deref() != Some("object"),
        "should not degrade to 'object'"
    );
}

// =========================================================================
// Runtime prop optionality tests (Vue semantics: optional unless required:true)
// =========================================================================

#[test]
fn prop_shorthand_defaults_to_optional() {
    // `bar: Number` — no required field → is_optional: true (Vue default)
    let code = "defineProps({ bar: Number })";
    let macros = parse_macros(code);
    let field = &macros[0].prop_fields[0];
    assert_eq!(field.name, "bar");
    assert!(
        field.is_optional,
        "shorthand runtime prop without required:true should be optional"
    );
}

#[test]
fn prop_required_true_is_not_optional() {
    // `required: true` → is_optional: false
    let code = "defineProps({ foo: { type: String, required: true } })";
    let macros = parse_macros(code);
    let field = &macros[0].prop_fields[0];
    assert_eq!(field.name, "foo");
    assert!(
        !field.is_optional,
        "runtime prop with required:true should not be optional"
    );
}

#[test]
fn prop_required_false_is_optional() {
    // `required: false` → is_optional: true
    let code = "defineProps({ bar: { type: String, required: false } })";
    let macros = parse_macros(code);
    let field = &macros[0].prop_fields[0];
    assert!(
        field.is_optional,
        "runtime prop with required:false should be optional"
    );
}

#[test]
fn prop_with_default_is_optional() {
    // Props with a default value are optional (no required:true)
    let code = "defineProps({ count: { type: Number, default: 0 } })";
    let macros = parse_macros(code);
    let field = &macros[0].prop_fields[0];
    assert!(
        field.is_optional,
        "runtime prop with default but no required:true should be optional"
    );
}

#[test]
fn prop_array_form_is_optional() {
    // Array form props have no type or required info → all optional
    let code = "defineProps(['title', 'active'])";
    let macros = parse_macros(code);
    let fields = &macros[0].prop_fields;
    assert_eq!(fields.len(), 2);
    assert!(
        fields[0].is_optional,
        "array-form props should be optional by default"
    );
    assert!(
        fields[1].is_optional,
        "array-form props should be optional by default"
    );
}

#[test]
fn prop_mixed_fixture() {
    // Full regression fixture covering PropType<T>, () => T, required:true, and defaults
    let code = r#"defineProps({
  bar: Number,
  foo: { type: String, required: true },
  baz: { type: Object as () => typeof Card, default: () => { return Card } },
  foz: { type: Object as PropType<typeof Card>, default: () => { return Card } }
})"#;
    let macros = parse_macros(code);
    assert_eq!(macros.len(), 1);
    let dp = &macros[0];
    assert_eq!(dp.prop_fields.len(), 4);

    let bar = dp.prop_fields.iter().find(|f| f.name == "bar").unwrap();
    assert_eq!(bar.type_annotation.as_deref(), Some("number"));
    assert!(
        bar.is_optional,
        "bar has no required:true, should be optional"
    );

    let foo = dp.prop_fields.iter().find(|f| f.name == "foo").unwrap();
    assert_eq!(foo.type_annotation.as_deref(), Some("string"));
    assert!(
        !foo.is_optional,
        "foo has required:true, should not be optional"
    );

    let baz = dp.prop_fields.iter().find(|f| f.name == "baz").unwrap();
    assert_eq!(
        baz.type_annotation.as_deref(),
        Some("typeof Card"),
        "baz: Object as () => typeof Card should extract 'typeof Card'"
    );
    assert!(
        baz.is_optional,
        "baz has default, no required:true — should be optional"
    );
    assert!(
        baz.type_annotation.as_deref() != Some("object"),
        "baz must not degrade to 'object'"
    );
    assert!(
        baz.type_annotation.as_deref() != Some("Function"),
        "baz must not degrade to 'Function'"
    );
    assert!(
        baz.type_annotation.as_deref() != Some("unknown"),
        "baz must not degrade to 'unknown'"
    );

    let foz = dp.prop_fields.iter().find(|f| f.name == "foz").unwrap();
    assert_eq!(
        foz.type_annotation.as_deref(),
        Some("typeof Card"),
        "foz: Object as PropType<typeof Card> should extract 'typeof Card'"
    );
    assert!(
        foz.is_optional,
        "foz has default, no required:true — should be optional"
    );
    assert!(
        foz.type_annotation.as_deref() != Some("object"),
        "foz must not degrade to 'object'"
    );
    assert!(
        foz.type_annotation.as_deref() != Some("Function"),
        "foz must not degrade to 'Function'"
    );
    assert!(
        foz.type_annotation.as_deref() != Some("unknown"),
        "foz must not degrade to 'unknown'"
    );

    // Default values should contain the arrow function source
    let baz_default = dp.default_values.iter().find(|d| d.key == "baz").unwrap();
    assert!(
        baz_default.value.contains("=>"),
        "baz default should preserve arrow function source"
    );
    let foz_default = dp.default_values.iter().find(|d| d.key == "foz").unwrap();
    assert!(
        foz_default.value.contains("=>"),
        "foz default should preserve arrow function source"
    );
}

// ── Local defineEmits type resolution tests ──

#[test]
fn resolve_local_interface_in_define_emits() {
    let source = r#"
        interface Emits { change: [value: string]; submit: [] }
        defineEmits<Emits>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let de = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineEmits)
        .unwrap();
    assert_eq!(
        de.emit_fields.len(),
        2,
        "should resolve 2 emit fields from local interface, got: {:?}",
        de.emit_fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    let names: Vec<&str> = de.emit_fields.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"change"), "should have 'change' event");
    assert!(names.contains(&"submit"), "should have 'submit' event");
    // Negative: no spurious fields
    assert!(
        !names.iter().any(|n| *n != "change" && *n != "submit"),
        "should have no extra fields"
    );
}

#[test]
fn resolve_local_type_alias_in_define_emits() {
    let source = r#"
        type MyEmits = { change: [val: string] }
        defineEmits<MyEmits>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let de = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineEmits)
        .unwrap();
    assert_eq!(
        de.emit_fields.len(),
        1,
        "should resolve 1 emit field from type alias, got: {:?}",
        de.emit_fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    assert_eq!(de.emit_fields[0].name, "change");
}

#[test]
fn resolve_local_interface_extends_in_define_emits() {
    let source = r#"
        interface BaseEmits { close: [] }
        interface Emits extends BaseEmits { open: [value: boolean] }
        defineEmits<Emits>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let de = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineEmits)
        .unwrap();
    assert_eq!(
        de.emit_fields.len(),
        2,
        "should resolve 2 emit fields (1 inherited + 1 own), got: {:?}",
        de.emit_fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    let names: Vec<&str> = de.emit_fields.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"close"), "should have inherited 'close'");
    assert!(names.contains(&"open"), "should have own 'open'");
}

#[test]
fn resolve_local_intersection_in_define_emits() {
    let source = r#"
        interface BaseEmits { close: [] }
        defineEmits<BaseEmits & { extra: [val: number] }>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let de = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineEmits)
        .unwrap();
    assert_eq!(
        de.emit_fields.len(),
        2,
        "should resolve 2 emit fields from intersection, got: {:?}",
        de.emit_fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    let names: Vec<&str> = de.emit_fields.iter().map(|f| f.name.as_str()).collect();
    assert!(
        names.contains(&"close"),
        "should have 'close' from interface"
    );
    assert!(names.contains(&"extra"), "should have 'extra' from literal");
}

// ── Local defineSlots type resolution tests ──

#[test]
fn resolve_local_interface_in_define_slots() {
    let source = r#"
        interface Slots {
            default(props: { item: string }): any
            header?(): any
        }
        defineSlots<Slots>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let ds = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineSlots)
        .unwrap();
    assert_eq!(
        ds.slot_fields.len(),
        2,
        "should resolve 2 slot fields from local interface, got: {:?}",
        ds.slot_fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    let names: Vec<&str> = ds.slot_fields.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"default"), "should have 'default' slot");
    assert!(names.contains(&"header"), "should have 'header' slot");
    // Check required/optional
    let default_slot = ds.slot_fields.iter().find(|f| f.name == "default").unwrap();
    assert!(default_slot.is_required, "default slot should be required");
    let header_slot = ds.slot_fields.iter().find(|f| f.name == "header").unwrap();
    assert!(!header_slot.is_required, "header slot should be optional");
    // Negative: no spurious slots
    assert!(
        !names.iter().any(|n| *n != "default" && *n != "header"),
        "should have no extra slots"
    );
}

#[test]
fn resolve_local_interface_in_define_slots_preserves_pick_binding_source() {
    let source = r#"
        interface CalendarCellTriggerProps {
            day: Date
            month: number
        }
        interface Slots {
            day(props: Pick<CalendarCellTriggerProps, 'day'>): any
        }
        defineSlots<Slots>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let ds = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineSlots)
        .unwrap();
    let day_slot = ds
        .slot_fields
        .iter()
        .find(|slot| slot.name == "day")
        .expect("should have day slot");
    assert_eq!(day_slot.bindings.len(), 1);
    assert_eq!(day_slot.bindings[0].name, "day");
    assert_eq!(
        day_slot.bindings[0].type_annotation.as_deref(),
        Some("CalendarCellTriggerProps['day']")
    );
}

#[test]
fn resolve_local_type_alias_in_define_slots() {
    let source = r#"
        type MySlots = { default(props: { row: string }): any }
        defineSlots<MySlots>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let ds = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineSlots)
        .unwrap();
    assert_eq!(
        ds.slot_fields.len(),
        1,
        "should resolve 1 slot field from type alias, got: {:?}",
        ds.slot_fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    assert_eq!(ds.slot_fields[0].name, "default");
}

#[test]
fn resolve_local_interface_extends_in_define_slots() {
    let source = r#"
        interface BaseSlots { default(props: { id: number }): any }
        interface Slots extends BaseSlots { footer(): any }
        defineSlots<Slots>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let ds = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineSlots)
        .unwrap();
    assert_eq!(
        ds.slot_fields.len(),
        2,
        "should resolve 2 slot fields (1 inherited + 1 own), got: {:?}",
        ds.slot_fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    let names: Vec<&str> = ds.slot_fields.iter().map(|f| f.name.as_str()).collect();
    assert!(
        names.contains(&"default"),
        "should have inherited 'default'"
    );
    assert!(names.contains(&"footer"), "should have own 'footer'");
}

#[test]
fn resolve_local_slots_intersection_keeps_resolvable_branches_when_utility_branch_is_unresolvable()
{
    let source = r#"
        type DynamicSlots = Record<string, (props: { value: string }) => any>
        type Slots = { default(props: { id: number }): any } & DynamicSlots
        defineSlots<Slots>()
    "#;
    let alloc = Allocator::default();
    let macros = parse_and_extract(&alloc, source);
    let ds = macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineSlots)
        .unwrap();
    let names: Vec<&str> = ds.slot_fields.iter().map(|f| f.name.as_str()).collect();
    assert!(
        names.contains(&"default"),
        "resolvable local intersection branches should survive even when a utility branch cannot be expanded, got: {:?}",
        names
    );
}

// ---------------------------------------------------------------------------
// W1.1 — discriminating regression tests for typed-form lowering at every
// macros.rs producer site. Each test would FAIL on the pre-W1.1 tree (where
// `*_expr` was unconditionally `None`) and PASS post-W1.1.
// ---------------------------------------------------------------------------

mod w1_1_typed_form_regression {
    use super::*;
    use verter_type_expr::{LiteralValue, PrimitiveName, TypeExpr};

    /// Site #1: `extract_fields_from_interface_body_like` — the inline prop
    /// type literal lowers each property's type annotation directly.
    #[test]
    fn inline_prop_type_literal_populates_type_expr_per_field() {
        let alloc = Allocator::new();
        let macros = parse_and_extract(
            &alloc,
            "defineProps<{ count: number; label: 'a' | 'b' }>();",
        );
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .expect("DefineProps macro");
        let count = dp
            .prop_fields
            .iter()
            .find(|f| f.name == "count")
            .expect("count field");
        assert!(
            matches!(
                count.type_expr,
                Some(TypeExpr::Primitive(PrimitiveName::Number))
            ),
            "count.type_expr must be Primitive(Number); got {:?}",
            count.type_expr
        );
        assert!(
            count.type_expr_scope.is_some(),
            "pairing invariant: type_expr_scope must be Some when type_expr is Some"
        );
        let label = dp
            .prop_fields
            .iter()
            .find(|f| f.name == "label")
            .expect("label field");
        match &label.type_expr {
            Some(TypeExpr::Union(arms)) => {
                assert_eq!(arms.len(), 2, "union arity");
                assert!(arms
                    .iter()
                    .all(|t| matches!(t, TypeExpr::Literal(LiteralValue::String(_)))));
            }
            other => panic!("label.type_expr expected Union of string literals; got {other:?}"),
        }
    }

    /// Site #2: `try_extract_macro::parsed_type_argument` lowers the macro's
    /// first type argument directly via `lower_ts_type` and stamps a paired
    /// scope.
    #[test]
    fn macro_parsed_type_argument_lowers_directly() {
        let alloc = Allocator::new();
        let macros = parse_and_extract(&alloc, "defineProps<MyProps>();");
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .expect("DefineProps macro");
        match dp.parsed_type_argument.as_deref() {
            Some(TypeExpr::Ref {
                name,
                type_arguments,
            }) => {
                assert_eq!(name.as_ref(), "MyProps");
                assert!(type_arguments.is_empty());
            }
            other => {
                panic!("parsed_type_argument expected Ref {{ name: \"MyProps\" }}; got {other:?}")
            }
        }
        assert!(
            dp.parsed_type_argument_scope.is_some(),
            "pairing invariant: parsed_type_argument_scope must be Some when parsed_type_argument is Some"
        );
    }

    /// Site #3: `extract_define_model_type` lowers the model type annotation
    /// directly.
    #[test]
    fn define_model_type_lowers_directly() {
        let alloc = Allocator::new();
        let macros = parse_and_extract(&alloc, "defineModel<string>();");
        let dm = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineModel)
            .expect("DefineModel macro");
        let field = dm
            .prop_fields
            .first()
            .expect("DefineModel produces a single prop field");
        assert!(
            matches!(
                field.type_expr,
                Some(TypeExpr::Primitive(PrimitiveName::String))
            ),
            "defineModel type_expr should be Primitive(String); got {:?}",
            field.type_expr
        );
        assert!(field.type_expr_scope.is_some(), "pairing invariant");
    }

    /// Site #4: `extract_ts_as_type` returns `Option<TypeExpr>`. The
    /// `as PropType<T>` form lowers the inner type — NOT a string. The
    /// runtime-prop caller stores the typed form on the field.
    #[test]
    fn as_prop_type_lowers_to_type_expr() {
        let alloc = Allocator::new();
        let source =
            "defineProps({\n  foo: { type: Object as PropType<{ a: string; b: number }> }\n});\n";
        let macros = parse_and_extract(&alloc, source);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .expect("DefineProps macro");
        let foo = dp
            .prop_fields
            .iter()
            .find(|f| f.name == "foo")
            .expect("foo field");
        match &foo.type_expr {
            Some(TypeExpr::Object(obj)) => {
                let names: Vec<String> = obj
                    .properties
                    .iter()
                    .filter_map(|m| match m {
                        verter_type_expr::ObjectMember::Property(p) => Some(p.name.clone()),
                        _ => None,
                    })
                    .collect();
                assert!(names.contains(&"a".to_string()));
                assert!(names.contains(&"b".to_string()));
            }
            other => {
                panic!("as PropType<{{a,b}}> expected to lower to TypeExpr::Object; got {other:?}")
            }
        }
        assert!(foo.type_expr_scope.is_some(), "pairing invariant");
    }

    /// Site #5: emit `payload_type` lowers via `lower_ts_type` for property
    /// signatures. Tuple-form lowers to `TypeExpr::Tuple`.
    #[test]
    fn emit_property_signature_payload_lowers_to_typed_tuple() {
        let alloc = Allocator::new();
        let macros = parse_and_extract(
            &alloc,
            "defineEmits<{ change: [id: number, label: string] }>();",
        );
        let de = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineEmits)
            .expect("DefineEmits macro");
        let change = de
            .emit_fields
            .iter()
            .find(|f| f.name == "change")
            .expect("change emit");
        match &change.payload_expr {
            Some(TypeExpr::Tuple { elements, .. }) => {
                assert_eq!(elements.len(), 2);
            }
            other => panic!("emit payload_expr expected Tuple; got {other:?}"),
        }
        assert!(change.payload_expr_scope.is_some(), "pairing invariant");
    }

    /// Site #6: slot `return_expr` lowers via `lower_ts_type`. Property-
    /// signature slots and method-signature slots both populate.
    #[test]
    fn slot_return_type_lowers_to_type_expr() {
        let alloc = Allocator::new();
        let source = "defineSlots<{\n    default(props: { item: string }): boolean;\n    header: (props: { count: number }) => void;\n}>();\n";
        let macros = parse_and_extract(&alloc, source);
        let ds = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineSlots)
            .expect("DefineSlots macro");
        let default_slot = ds
            .slot_fields
            .iter()
            .find(|f| f.name == "default")
            .expect("default slot");
        assert!(
            matches!(
                default_slot.return_expr,
                Some(TypeExpr::Primitive(PrimitiveName::Boolean))
            ),
            "default slot return_expr should be Primitive(Boolean); got {:?}",
            default_slot.return_expr
        );
        assert!(
            default_slot.return_expr_scope.is_some(),
            "pairing invariant"
        );

        let header = ds
            .slot_fields
            .iter()
            .find(|f| f.name == "header")
            .expect("header slot");
        assert!(
            matches!(
                header.return_expr,
                Some(TypeExpr::Primitive(PrimitiveName::Void))
            ),
            "header slot return_expr should be Primitive(Void); got {:?}",
            header.return_expr
        );
    }

    /// Site #7 (analyzer-side): Pick AST shape recovery emits the symbolic
    /// shape `IndexedAccess { object, index }` directly. The analyzer does
    /// NOT resolve userland alias keys — only emits `Ref { name: alias }`
    /// inside the IndexedAccess.
    #[test]
    fn pick_with_string_literal_keys_emits_indexed_access_with_literal_index() {
        let alloc = Allocator::new();
        let source =
            "defineSlots<{\n    row(props: Pick<RowApi, 'name' | 'value'>): void;\n}>();\n";
        let macros = parse_and_extract(&alloc, source);
        let ds = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineSlots)
            .expect("DefineSlots macro");
        let row = ds
            .slot_fields
            .iter()
            .find(|f| f.name == "row")
            .expect("row slot");
        let mut binding_names: Vec<&str> = row.bindings.iter().map(|b| b.name.as_str()).collect();
        binding_names.sort();
        assert_eq!(binding_names, vec!["name", "value"]);
        for b in &row.bindings {
            match &b.binding_expr {
                Some(TypeExpr::IndexedAccess { object, index }) => {
                    assert!(
                        matches!(
                            object.as_ref(),
                            TypeExpr::Ref { name, .. } if name.as_ref() == "RowApi"
                        ),
                        "Pick object should be Ref {{ name: \"RowApi\" }}; got {object:?}"
                    );
                    assert!(
                        matches!(index.as_ref(), TypeExpr::Literal(LiteralValue::String(_))),
                        "Pick literal-key index should be Literal(String); got {index:?}"
                    );
                }
                other => panic!("binding_expr should be IndexedAccess; got {other:?}"),
            }
            assert!(b.binding_expr_scope.is_some(), "pairing invariant");
        }
    }

    /// Site #7 — userland alias key form: `Pick<RowApi, BindingKey>`. The
    /// analyzer emits a SINGLE binding entry whose `binding_expr` is
    /// `IndexedAccess { object, index: Ref }`. The projector / cross-file
    /// resolver enumerates the literal-union members downstream.
    #[test]
    fn pick_with_userland_alias_key_emits_indexed_access_with_ref_index() {
        let alloc = Allocator::new();
        let source = "defineSlots<{\n    row(props: Pick<RowApi, BindingKey>): void;\n}>();\n";
        let macros = parse_and_extract(&alloc, source);
        let ds = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineSlots)
            .expect("DefineSlots macro");
        let row = ds
            .slot_fields
            .iter()
            .find(|f| f.name == "row")
            .expect("row slot");
        assert_eq!(
            row.bindings.len(),
            1,
            "analyzer emits one binding for the alias-key form (resolution is downstream)"
        );
        let b = &row.bindings[0];
        match &b.binding_expr {
            Some(TypeExpr::IndexedAccess { object, index }) => {
                assert!(
                    matches!(
                        object.as_ref(),
                        TypeExpr::Ref { name, .. } if name.as_ref() == "RowApi"
                    ),
                    "Pick object should be Ref {{ name: \"RowApi\" }}; got {object:?}"
                );
                match index.as_ref() {
                    TypeExpr::Ref {
                        name,
                        type_arguments,
                    } => {
                        assert_eq!(name.as_ref(), "BindingKey");
                        assert!(type_arguments.is_empty());
                    }
                    other => panic!(
                        "alias-key index should be Ref {{ name: \"BindingKey\" }}; got {other:?}"
                    ),
                }
            }
            other => panic!("binding_expr should be IndexedAccess; got {other:?}"),
        }
        assert!(b.binding_expr_scope.is_some(), "pairing invariant");
    }

    /// Pairing invariant: every `*_expr` field is paired with a populated
    /// `*_expr_scope` (the §3.1 invariant) at every analyzer producer site.
    #[test]
    fn analyzer_pairing_invariant_holds_across_all_producer_sites() {
        let alloc = Allocator::new();
        let source = "interface Props { count: number; label: 'a' | 'b'; }\ndefineProps<Props>();\ndefineEmits<{ change: [id: number] }>();\ndefineSlots<{ default(props: { item: string }): void }>();\ndefineModel<string>();\n";
        let macros = parse_and_extract(&alloc, source);
        for m in &macros {
            assert_eq!(
                m.parsed_type_argument.is_some(),
                m.parsed_type_argument_scope.is_some(),
                "AnalyzedMacro pairing invariant violated for kind {:?}",
                m.kind
            );
            for f in &m.prop_fields {
                assert_eq!(
                    f.type_expr.is_some(),
                    f.type_expr_scope.is_some(),
                    "AnalyzedPropField pairing invariant violated for {}",
                    f.name
                );
            }
            for f in &m.emit_fields {
                assert_eq!(
                    f.payload_expr.is_some(),
                    f.payload_expr_scope.is_some(),
                    "AnalyzedEmitField pairing invariant violated for {}",
                    f.name
                );
            }
            for f in &m.slot_fields {
                assert_eq!(
                    f.return_expr.is_some(),
                    f.return_expr_scope.is_some(),
                    "AnalyzedSlotField pairing invariant violated for {}",
                    f.name
                );
                for b in &f.bindings {
                    assert_eq!(
                        b.binding_expr.is_some(),
                        b.binding_expr_scope.is_some(),
                        "AnalyzedSlotFieldBinding pairing invariant violated for {} -> {}",
                        f.name,
                        b.name
                    );
                }
            }
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// W2.5 — `build_expanded_type_expr` consumes the analyzer-populated
// `AnalyzedPropField.type_expr` directly. The pre-cutover code parsed
// `type_annotation` text via `parse_jsdoc_tag_type_payload`, which loses the
// producer's typed precision and replaces complex shapes with whatever the
// text parser reconstructs. The discriminator is to construct a synthetic
// prop field whose `type_expr` shape would NOT round-trip through text:
// the resulting object property's `ty` must equal the typed input exactly.
// ───────────────────────────────────────────────────────────────────────────
#[test]
fn build_expanded_type_expr_consumes_type_expr_field_directly_without_reparse() {
    use crate::analysis::types::{AnalyzedPropField, TypeResolutionSource};
    use std::sync::Arc;
    use verter_type_expr::{
        LiteralValue, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr, TypeExprScope,
    };

    // A shape the producer captured but the text annotation does NOT
    // describe (the annotation says one thing, the typed form says
    // another). Discriminating invariant: `build_expanded_type_expr`
    // must consume the typed form directly without falling back to
    // reparsing the text annotation; the typed shape survives end-to-end.
    let typed_indexed_access = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::Ref {
            name: "ImportedAlias".into(),
            type_arguments: Vec::<TypeExpr>::new().into(),
        }),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("a".to_string()))),
    };

    let fields = vec![AnalyzedPropField {
        name: "prop".to_string(),
        is_optional: false,
        span: verter_span::Span::default(),
        type_annotation: Some("Garbage<<<unparseable".to_string()),
        type_expr: Some(typed_indexed_access.clone()),
        type_expr_scope: Some(TypeExprScope::new("test:fixture")),
        description: None,
        tags: Vec::new(),
        resolution_source: TypeResolutionSource::Rust,
        resolution_error: None,
        declared_in_macro_type_arg: false,
    }];

    let result = super::build_expanded_type_expr(&fields);

    let expected = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic(
            "prop".to_string(),
            typed_indexed_access.clone(),
            false,
            false,
        ))],
    }));

    assert_eq!(
        result, expected,
        "build_expanded_type_expr must read field.type_expr directly, not reparse type_annotation"
    );

    // Negative discrimination: prove the typed shape differs from what
    // the text parser would have produced. If they happened to coincide
    // (e.g. via accidental annotation choice), the test would not be
    // characterising anything.
    let from_text =
        crate::analysis::jsdoc::parse_jsdoc_tag_type_payload("Garbage<<<unparseable", None);
    assert_ne!(
        from_text, typed_indexed_access,
        "the annotation text MUST NOT round-trip back to the typed shape; \
         otherwise the test does not discriminate the post-W2.5 typed read \
         from the pre-W2.5 reparse"
    );

    // Sanity: when type_expr is None the function emits Unknown { raw }
    // (no string parsing).
    let fields_no_typed = vec![AnalyzedPropField {
        name: "prop".to_string(),
        is_optional: false,
        span: verter_span::Span::default(),
        type_annotation: Some("AnythingHere".to_string()),
        type_expr: None,
        type_expr_scope: None,
        description: None,
        tags: Vec::new(),
        resolution_source: TypeResolutionSource::Rust,
        resolution_error: None,
        declared_in_macro_type_arg: false,
    }];
    let result_no_typed = super::build_expanded_type_expr(&fields_no_typed);
    let expected_no_typed = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic(
            "prop".to_string(),
            TypeExpr::Unknown {
                raw: "AnythingHere".to_string(),
            },
            false,
            false,
        ))],
    }));
    assert_eq!(
        result_no_typed, expected_no_typed,
        "type_expr=None must yield Unknown {{ raw: type_annotation }} — never a parsed shape"
    );

    // Floor: type_annotation=None and type_expr=None yields Unknown { raw: \"unknown\" }.
    let fields_none = vec![AnalyzedPropField {
        name: "p".to_string(),
        is_optional: false,
        span: verter_span::Span::default(),
        type_annotation: None,
        type_expr: None,
        type_expr_scope: None,
        description: None,
        tags: Vec::new(),
        resolution_source: TypeResolutionSource::Rust,
        resolution_error: None,
        declared_in_macro_type_arg: false,
    }];
    let result_none = super::build_expanded_type_expr(&fields_none);
    let expected_none = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic(
            "p".to_string(),
            TypeExpr::Unknown {
                raw: "unknown".to_string(),
            },
            false,
            false,
        ))],
    }));
    assert_eq!(result_none, expected_none);
}
