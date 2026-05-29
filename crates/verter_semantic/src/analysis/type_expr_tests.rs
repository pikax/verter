use std::sync::Arc;
use verter_type_expr::*;

use crate::analysis::jsdoc::parse_jsdoc_tag_type_payload;

// =============================================================================
// Primitive types
// =============================================================================

#[test]
fn primitive_string() {
    let expr = parse_jsdoc_tag_type_payload("string", None);
    assert_eq!(expr, TypeExpr::Primitive(PrimitiveName::String));
    assert!(expr.is_primitive());
    assert!(!expr.is_unknown());
}

#[test]
fn primitive_number() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("number", None),
        TypeExpr::Primitive(PrimitiveName::Number)
    );
}

#[test]
fn primitive_boolean() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("boolean", None),
        TypeExpr::Primitive(PrimitiveName::Boolean)
    );
}

#[test]
fn primitive_void() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("void", None),
        TypeExpr::Primitive(PrimitiveName::Void)
    );
}

#[test]
fn primitive_never() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("never", None),
        TypeExpr::Primitive(PrimitiveName::Never)
    );
}

#[test]
fn primitive_any() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("any", None),
        TypeExpr::Primitive(PrimitiveName::Any)
    );
}

#[test]
fn primitive_unknown() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("unknown", None),
        TypeExpr::Primitive(PrimitiveName::Unknown)
    );
}

#[test]
fn primitive_null() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("null", None),
        TypeExpr::Primitive(PrimitiveName::Null)
    );
}

#[test]
fn primitive_undefined() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("undefined", None),
        TypeExpr::Primitive(PrimitiveName::Undefined)
    );
}

#[test]
fn primitive_symbol() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("symbol", None),
        TypeExpr::Primitive(PrimitiveName::Symbol)
    );
}

#[test]
fn primitive_bigint() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("bigint", None),
        TypeExpr::Primitive(PrimitiveName::BigInt)
    );
}

#[test]
fn primitive_object() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("object", None),
        TypeExpr::Primitive(PrimitiveName::Object)
    );
}

// =============================================================================
// Literal types
// =============================================================================

#[test]
fn literal_string() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("\"hello\"", None),
        TypeExpr::string_literal("hello")
    );
}

#[test]
fn literal_number() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("42", None),
        TypeExpr::number_literal(42.0)
    );
}

#[test]
fn literal_negative_number() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("-1", None),
        TypeExpr::number_literal(-1.0)
    );
}

#[test]
fn literal_float() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("2.5", None),
        TypeExpr::number_literal(2.5)
    );
}

#[test]
fn literal_true() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("true", None),
        TypeExpr::boolean_literal(true)
    );
}

#[test]
fn literal_false() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("false", None),
        TypeExpr::boolean_literal(false)
    );
}

// =============================================================================
// Union types
// =============================================================================

#[test]
fn union_two_primitives() {
    let expr = parse_jsdoc_tag_type_payload("string | number", None);
    assert_eq!(
        expr,
        TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Number),
        ])
    );
    assert!(!expr.is_primitive());
}

#[test]
fn union_three_types() {
    let expr = parse_jsdoc_tag_type_payload("string | number | boolean", None);
    assert_eq!(
        expr,
        TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Number),
            TypeExpr::Primitive(PrimitiveName::Boolean),
        ])
    );
}

#[test]
fn union_with_null() {
    let expr = parse_jsdoc_tag_type_payload("string | null", None);
    assert_eq!(
        expr,
        TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Null),
        ])
    );
}

#[test]
fn union_with_literals() {
    let expr = parse_jsdoc_tag_type_payload("\"red\" | \"blue\" | \"green\"", None);
    assert_eq!(
        expr,
        TypeExpr::union(vec![
            TypeExpr::string_literal("red"),
            TypeExpr::string_literal("blue"),
            TypeExpr::string_literal("green"),
        ])
    );
}

#[test]
fn union_single_element_collapses() {
    // TypeExpr::union with a single element should collapse
    let expr = TypeExpr::union(vec![TypeExpr::Primitive(PrimitiveName::String)]);
    assert_eq!(expr, TypeExpr::Primitive(PrimitiveName::String));
}

// =============================================================================
// Intersection types
// =============================================================================

#[test]
fn intersection_two_refs() {
    let expr = parse_jsdoc_tag_type_payload("A & B", None);
    assert_eq!(
        expr,
        TypeExpr::intersection(vec![TypeExpr::named("A"), TypeExpr::named("B"),])
    );
}

#[test]
fn intersection_ref_and_object() {
    let expr = parse_jsdoc_tag_type_payload("Base & { extra: boolean }", None);
    match &expr {
        TypeExpr::Intersection(types) => {
            assert_eq!(types.len(), 2);
            assert_eq!(types[0], TypeExpr::named("Base"));
            assert!(matches!(&types[1], TypeExpr::Object(_)));
        }
        _ => panic!("expected intersection, got {expr:?}"),
    }
}

// =============================================================================
// Array types
// =============================================================================

#[test]
fn array_bracket_syntax() {
    let expr = parse_jsdoc_tag_type_payload("string[]", None);
    assert_eq!(
        expr,
        TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: false,
        }
    );
}

#[test]
fn array_generic_syntax() {
    let expr = parse_jsdoc_tag_type_payload("Array<number>", None);
    assert_eq!(
        expr,
        TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
            readonly: false,
        }
    );
}

#[test]
fn readonly_array() {
    let expr = parse_jsdoc_tag_type_payload("ReadonlyArray<string>", None);
    assert_eq!(
        expr,
        TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: true,
        }
    );
}

#[test]
fn readonly_operator_array() {
    let expr = parse_jsdoc_tag_type_payload("readonly string[]", None);
    assert_eq!(
        expr,
        TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: true,
        }
    );
}

#[test]
fn nested_array() {
    let expr = parse_jsdoc_tag_type_payload("string[][]", None);
    assert_eq!(
        expr,
        TypeExpr::Array {
            element: Arc::new(TypeExpr::Array {
                element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                readonly: false,
            }),
            readonly: false,
        }
    );
}

// =============================================================================
// Tuple types
// =============================================================================

#[test]
fn tuple_basic() {
    let expr = parse_jsdoc_tag_type_payload("[string, number]", None);
    match &expr {
        TypeExpr::Tuple { elements, readonly } => {
            assert_eq!(elements.len(), 2);
            assert!(!readonly);
            assert_eq!(elements[0].ty, TypeExpr::Primitive(PrimitiveName::String));
            assert_eq!(elements[1].ty, TypeExpr::Primitive(PrimitiveName::Number));
            assert!(!elements[0].optional);
            assert!(!elements[0].rest);
        }
        _ => panic!("expected tuple, got {expr:?}"),
    }
}

#[test]
fn tuple_with_optional() {
    let expr = parse_jsdoc_tag_type_payload("[string, number?]", None);
    match &expr {
        TypeExpr::Tuple { elements, .. } => {
            assert_eq!(elements.len(), 2);
            assert!(!elements[0].optional);
            assert!(elements[1].optional);
        }
        _ => panic!("expected tuple, got {expr:?}"),
    }
}

#[test]
fn tuple_with_rest() {
    let expr = parse_jsdoc_tag_type_payload("[string, ...number[]]", None);
    match &expr {
        TypeExpr::Tuple { elements, .. } => {
            assert_eq!(elements.len(), 2);
            assert!(!elements[0].rest);
            assert!(elements[1].rest);
        }
        _ => panic!("expected tuple, got {expr:?}"),
    }
}

#[test]
fn tuple_with_labels() {
    let expr = parse_jsdoc_tag_type_payload("[name: string, age: number]", None);
    match &expr {
        TypeExpr::Tuple { elements, .. } => {
            assert_eq!(elements.len(), 2);
            assert_eq!(elements[0].label.as_deref(), Some("name"));
            assert_eq!(elements[1].label.as_deref(), Some("age"));
        }
        _ => panic!("expected tuple, got {expr:?}"),
    }
}

#[test]
fn readonly_tuple() {
    let expr = parse_jsdoc_tag_type_payload("readonly [string, number]", None);
    match &expr {
        TypeExpr::Tuple { readonly, .. } => {
            assert!(readonly);
        }
        _ => panic!("expected tuple, got {expr:?}"),
    }
}

// =============================================================================
// Object types
// =============================================================================

#[test]
fn object_basic() {
    let expr = parse_jsdoc_tag_type_payload("{ name: string; age: number }", None);
    match &expr {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
            match &obj.properties[0] {
                ObjectMember::Property(p) => {
                    assert_eq!(p.name, "name");
                    assert_eq!(p.ty, TypeExpr::Primitive(PrimitiveName::String));
                    assert!(!p.optional);
                }
                _ => panic!("expected property"),
            }
            match &obj.properties[1] {
                ObjectMember::Property(p) => {
                    assert_eq!(p.name, "age");
                    assert_eq!(p.ty, TypeExpr::Primitive(PrimitiveName::Number));
                    assert!(!p.optional);
                }
                _ => panic!("expected property"),
            }
        }
        _ => panic!("expected object, got {expr:?}"),
    }
}

#[test]
fn object_optional_property() {
    let expr = parse_jsdoc_tag_type_payload("{ name?: string }", None);
    match &expr {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 1);
            match &obj.properties[0] {
                ObjectMember::Property(p) => {
                    assert_eq!(p.name, "name");
                    assert!(p.optional);
                }
                _ => panic!("expected property"),
            }
        }
        _ => panic!("expected object, got {expr:?}"),
    }
}

#[test]
fn object_readonly_property() {
    let expr = parse_jsdoc_tag_type_payload("{ readonly id: number }", None);
    match &expr {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 1);
            match &obj.properties[0] {
                ObjectMember::Property(p) => {
                    assert_eq!(p.name, "id");
                    assert!(p.readonly);
                }
                _ => panic!("expected property"),
            }
        }
        _ => panic!("expected object, got {expr:?}"),
    }
}

#[test]
fn object_index_signature() {
    let expr = parse_jsdoc_tag_type_payload("{ [key: string]: number }", None);
    match &expr {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 1);
            match &obj.properties[0] {
                ObjectMember::IndexSignature(idx) => {
                    assert_eq!(idx.key_name, "key");
                    assert_eq!(idx.key_type, TypeExpr::Primitive(PrimitiveName::String));
                    assert_eq!(idx.value_type, TypeExpr::Primitive(PrimitiveName::Number));
                }
                _ => panic!("expected index signature"),
            }
        }
        _ => panic!("expected object, got {expr:?}"),
    }
}

#[test]
fn object_method_signature() {
    let expr = parse_jsdoc_tag_type_payload("{ greet(name: string): void }", None);
    match &expr {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 1);
            match &obj.properties[0] {
                ObjectMember::Method(m) => {
                    assert_eq!(m.name, "greet");
                    assert_eq!(m.function.parameters.len(), 1);
                    assert_eq!(
                        m.function.parameters[0].ty,
                        TypeExpr::Primitive(PrimitiveName::String)
                    );
                    assert_eq!(
                        m.function.return_type.as_deref(),
                        Some(&TypeExpr::Primitive(PrimitiveName::Void))
                    );
                }
                _ => panic!("expected method"),
            }
        }
        _ => panic!("expected object, got {expr:?}"),
    }
}

#[test]
fn object_call_signature() {
    let expr = parse_jsdoc_tag_type_payload("{ (x: number): string }", None);
    match &expr {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 1);
            match &obj.properties[0] {
                ObjectMember::CallSignature(func) => {
                    assert_eq!(func.parameters.len(), 1);
                    assert_eq!(
                        func.return_type.as_deref(),
                        Some(&TypeExpr::Primitive(PrimitiveName::String))
                    );
                }
                _ => panic!("expected call signature"),
            }
        }
        _ => panic!("expected object, got {expr:?}"),
    }
}

// =============================================================================
// Function types
// =============================================================================

#[test]
fn function_basic() {
    let expr = parse_jsdoc_tag_type_payload("(x: string) => number", None);
    match &expr {
        TypeExpr::Function(func) => {
            assert_eq!(func.parameters.len(), 1);
            assert_eq!(func.parameters[0].name.as_deref(), Some("x"));
            assert_eq!(
                func.parameters[0].ty,
                TypeExpr::Primitive(PrimitiveName::String)
            );
            assert_eq!(
                func.return_type.as_deref(),
                Some(&TypeExpr::Primitive(PrimitiveName::Number))
            );
        }
        _ => panic!("expected function, got {expr:?}"),
    }
}

#[test]
fn function_no_params() {
    let expr = parse_jsdoc_tag_type_payload("() => void", None);
    match &expr {
        TypeExpr::Function(func) => {
            assert!(func.parameters.is_empty());
            assert_eq!(
                func.return_type.as_deref(),
                Some(&TypeExpr::Primitive(PrimitiveName::Void))
            );
        }
        _ => panic!("expected function, got {expr:?}"),
    }
}

#[test]
fn function_optional_param() {
    let expr = parse_jsdoc_tag_type_payload("(x?: string) => void", None);
    match &expr {
        TypeExpr::Function(func) => {
            assert_eq!(func.parameters.len(), 1);
            assert!(func.parameters[0].optional);
        }
        _ => panic!("expected function, got {expr:?}"),
    }
}

#[test]
fn function_rest_param() {
    let expr = parse_jsdoc_tag_type_payload("(...args: string[]) => void", None);
    match &expr {
        TypeExpr::Function(func) => {
            assert_eq!(func.parameters.len(), 1);
            assert!(func.parameters[0].rest);
            assert_eq!(func.parameters[0].name.as_deref(), Some("args"));
        }
        _ => panic!("expected function, got {expr:?}"),
    }
}

#[test]
fn function_with_type_params() {
    let expr = parse_jsdoc_tag_type_payload("<T>(x: T) => T", None);
    match &expr {
        TypeExpr::Function(func) => {
            assert_eq!(func.type_parameters.len(), 1);
            assert_eq!(func.type_parameters[0].name, "T");
        }
        _ => panic!("expected function, got {expr:?}"),
    }
}

#[test]
fn function_type_json_roundtrip_preserves_generic_parameter_metadata() {
    let expr = parse_jsdoc_tag_type_payload("<T extends Base = string>(value: T) => T", None);

    let TypeExpr::Function(func) = &expr else {
        panic!("expected function, got {expr:?}");
    };
    assert_eq!(func.type_parameters.len(), 1);
    assert_eq!(func.type_parameters[0].name, "T");
    assert!(matches!(
        func.type_parameters[0].constraint.as_deref(),
        Some(TypeExpr::Ref { name, type_arguments }) if name.as_ref() == "Base" && type_arguments.is_empty()
    ));
    assert!(matches!(
        func.type_parameters[0].default.as_deref(),
        Some(TypeExpr::Primitive(PrimitiveName::String))
    ));
    assert!(matches!(
        &func.parameters[0].ty,
        TypeExpr::TypeParameter(param) if param.name == "T"
    ));
    assert!(matches!(
        func.return_type.as_deref(),
        Some(TypeExpr::TypeParameter(param)) if param.name == "T"
    ));

    let json = serde_json::to_value(&expr).expect("serialize function");
    assert_eq!(json["typeParameters"][0]["name"], "T");
    assert_eq!(json["returnType"]["kind"], "typeParameter");
    assert_eq!(json["returnType"]["constraint"]["kind"], "ref");

    let roundtrip: TypeExpr = serde_json::from_value(json.clone()).expect("deserialize function");
    // OXC spans are in-memory provenance and are intentionally NOT part of the
    // JSON wire schema (`to_json_value` does not emit them), so a JSON
    // round-trip is span-lossy by design — a full `roundtrip == expr` would
    // (correctly) differ only on spans. Assert wire-losslessness instead:
    // re-serialising the round-trip yields byte-identical JSON, so everything
    // the wire carries — including the generic parameter metadata this test is
    // named for — survived verbatim.
    let reserialized = serde_json::to_value(&roundtrip).expect("re-serialize round-trip");
    assert_eq!(reserialized, json);
    assert!(!roundtrip.is_unknown());
    assert!(!matches!(roundtrip, TypeExpr::Object(_)));
}

#[test]
fn function_nested_type_parameter_usages_are_normalized() {
    let expr = parse_jsdoc_tag_type_payload("<T>(values: Array<T>) => Promise<T>", None);

    let TypeExpr::Function(func) = &expr else {
        panic!("expected function, got {expr:?}");
    };
    match &func.parameters[0].ty {
        TypeExpr::Array { element, readonly } => {
            assert!(!readonly);
            assert!(matches!(
                element.as_ref(),
                TypeExpr::TypeParameter(param) if param.name == "T"
            ));
            assert!(
                !matches!(element.as_ref(), TypeExpr::Ref { .. }),
                "nested generic parameters must not stay as plain refs"
            );
        }
        other => panic!("expected array parameter type, got {other:?}"),
    }

    match func.return_type.as_deref() {
        Some(TypeExpr::Ref {
            name,
            type_arguments,
        }) => {
            assert_eq!(name.as_ref(), "Promise");
            assert_eq!(type_arguments.len(), 1);
            assert!(matches!(
                &type_arguments[0],
                TypeExpr::TypeParameter(param) if param.name == "T"
            ));
        }
        other => panic!("expected Promise<T> return type, got {other:?}"),
    }
}

// =============================================================================
// Type references
// =============================================================================

#[test]
fn ref_simple() {
    assert_eq!(
        parse_jsdoc_tag_type_payload("MyType", None),
        TypeExpr::named("MyType")
    );
}

#[test]
fn ref_with_single_arg() {
    let expr = parse_jsdoc_tag_type_payload("Promise<string>", None);
    assert_eq!(
        expr,
        TypeExpr::named_with_args("Promise", vec![TypeExpr::Primitive(PrimitiveName::String)])
    );
}

#[test]
fn ref_with_multiple_args() {
    let expr = parse_jsdoc_tag_type_payload("Map<string, number>", None);
    assert_eq!(
        expr,
        TypeExpr::named_with_args(
            "Map",
            vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Number),
            ]
        )
    );
}

#[test]
fn ref_qualified_name() {
    let expr = parse_jsdoc_tag_type_payload("Foo.Bar.Baz", None);
    assert_eq!(expr, TypeExpr::named("Foo.Bar.Baz"));
}

#[test]
fn ref_array_normalized() {
    // Array<string> normalizes to Array { element, readonly: false }
    let expr = parse_jsdoc_tag_type_payload("Array<string>", None);
    assert_eq!(
        expr,
        TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: false,
        }
    );
    // Should NOT be a Ref
    assert!(!matches!(expr, TypeExpr::Ref { .. }));
}

#[test]
fn ref_partial() {
    let expr = parse_jsdoc_tag_type_payload("Partial<MyType>", None);
    assert_eq!(
        expr,
        TypeExpr::named_with_args("Partial", vec![TypeExpr::named("MyType")])
    );
}

#[test]
fn ref_record() {
    let expr = parse_jsdoc_tag_type_payload("Record<string, number>", None);
    assert_eq!(
        expr,
        TypeExpr::named_with_args(
            "Record",
            vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Number),
            ]
        )
    );
}

#[test]
fn ref_pick() {
    let expr = parse_jsdoc_tag_type_payload("Pick<User, \"id\" | \"name\">", None);
    match &expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(&**name, "Pick");
            assert_eq!(type_arguments.len(), 2);
            assert_eq!(type_arguments[0], TypeExpr::named("User"));
            assert_eq!(
                type_arguments[1],
                TypeExpr::union(vec![
                    TypeExpr::string_literal("id"),
                    TypeExpr::string_literal("name"),
                ])
            );
        }
        _ => panic!("expected ref, got {expr:?}"),
    }
}

// =============================================================================
// keyof
// =============================================================================

#[test]
fn keyof_ref() {
    let expr = parse_jsdoc_tag_type_payload("keyof T", None);
    assert_eq!(expr, TypeExpr::KeyOf(Arc::new(TypeExpr::named("T"))));
}

#[test]
fn keyof_object() {
    let expr = parse_jsdoc_tag_type_payload("keyof { a: string; b: number }", None);
    match &expr {
        TypeExpr::KeyOf(inner) => {
            assert!(matches!(inner.as_ref(), TypeExpr::Object(_)));
        }
        _ => panic!("expected keyof, got {expr:?}"),
    }
}

// =============================================================================
// typeof
// =============================================================================

#[test]
fn typeof_simple() {
    let expr = parse_jsdoc_tag_type_payload("typeof myVar", None);
    match &expr {
        TypeExpr::TypeOf(value_ref) => {
            assert_eq!(value_ref.path, vec!["myVar"]);
        }
        _ => panic!("expected typeof, got {expr:?}"),
    }
}

#[test]
fn typeof_qualified() {
    let expr = parse_jsdoc_tag_type_payload("typeof module.exports", None);
    match &expr {
        TypeExpr::TypeOf(value_ref) => {
            assert_eq!(value_ref.path, vec!["module", "exports"]);
        }
        _ => panic!("expected typeof, got {expr:?}"),
    }
}

// =============================================================================
// Indexed access
// =============================================================================

#[test]
fn indexed_access_basic() {
    let expr = parse_jsdoc_tag_type_payload("T[\"key\"]", None);
    assert_eq!(
        expr,
        TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("T")),
            index: Arc::new(TypeExpr::string_literal("key")),
        }
    );
}

#[test]
fn indexed_access_number() {
    let expr = parse_jsdoc_tag_type_payload("T[number]", None);
    assert_eq!(
        expr,
        TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("T")),
            index: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
        }
    );
}

// =============================================================================
// Conditional types
// =============================================================================

#[test]
fn conditional_basic() {
    let expr = parse_jsdoc_tag_type_payload("T extends string ? true : false", None);
    match &expr {
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            assert_eq!(check.as_ref(), &TypeExpr::named("T"));
            assert_eq!(
                extends.as_ref(),
                &TypeExpr::Primitive(PrimitiveName::String)
            );
            assert_eq!(true_type.as_ref(), &TypeExpr::boolean_literal(true));
            assert_eq!(false_type.as_ref(), &TypeExpr::boolean_literal(false));
        }
        _ => panic!("expected conditional, got {expr:?}"),
    }
}

#[test]
fn conditional_with_infer() {
    let expr = parse_jsdoc_tag_type_payload("T extends Array<infer U> ? U : never", None);
    match &expr {
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            assert_eq!(check.as_ref(), &TypeExpr::named("T"));
            // extends should be Array<infer U> which normalizes to Array { element: Infer }
            match extends.as_ref() {
                TypeExpr::Array { element, .. } => {
                    assert!(matches!(element.as_ref(), TypeExpr::Infer { name } if name == "U"));
                }
                _ => panic!("expected array with infer, got {extends:?}"),
            }
            assert_eq!(true_type.as_ref(), &TypeExpr::named("U"));
            assert_eq!(
                false_type.as_ref(),
                &TypeExpr::Primitive(PrimitiveName::Never)
            );
        }
        _ => panic!("expected conditional, got {expr:?}"),
    }
}

// =============================================================================
// Mapped types
// =============================================================================

#[test]
fn mapped_basic() {
    let expr = parse_jsdoc_tag_type_payload("{ [K in keyof T]: T[K] }", None);
    match &expr {
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            assert_eq!(parameter, "K");
            assert!(matches!(source.as_ref(), TypeExpr::KeyOf(_)));
            assert!(matches!(value.as_ref(), TypeExpr::IndexedAccess { .. }));
            assert_eq!(*optional, MappedModifier::None);
            assert_eq!(*readonly, MappedModifier::None);
            assert!(name_type.is_none());
        }
        _ => panic!("expected mapped, got {expr:?}"),
    }
}

#[test]
fn mapped_optional_add() {
    let expr = parse_jsdoc_tag_type_payload("{ [K in keyof T]?: T[K] }", None);
    match &expr {
        TypeExpr::Mapped { optional, .. } => {
            assert_eq!(*optional, MappedModifier::Add);
        }
        _ => panic!("expected mapped, got {expr:?}"),
    }
}

#[test]
fn mapped_readonly_remove() {
    let expr = parse_jsdoc_tag_type_payload("{ -readonly [K in keyof T]: T[K] }", None);
    match &expr {
        TypeExpr::Mapped { readonly, .. } => {
            assert_eq!(*readonly, MappedModifier::Remove);
        }
        _ => panic!("expected mapped, got {expr:?}"),
    }
}

// =============================================================================
// Template literal types
// =============================================================================

#[test]
fn template_literal_basic() {
    let expr = parse_jsdoc_tag_type_payload("`btn-${string}`", None);
    match &expr {
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => {
            assert_eq!(quasis.len(), 2);
            assert_eq!(quasis[0], "btn-");
            assert_eq!(quasis[1], "");
            assert_eq!(expressions.len(), 1);
            assert_eq!(expressions[0], TypeExpr::Primitive(PrimitiveName::String));
        }
        _ => panic!("expected template literal, got {expr:?}"),
    }
}

#[test]
fn template_literal_multiple_parts() {
    let expr = parse_jsdoc_tag_type_payload("`${number}px`", None);
    match &expr {
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => {
            assert_eq!(quasis.len(), 2);
            assert_eq!(quasis[0], "");
            assert_eq!(quasis[1], "px");
            assert_eq!(expressions.len(), 1);
            assert_eq!(expressions[0], TypeExpr::Primitive(PrimitiveName::Number));
        }
        _ => panic!("expected template literal, got {expr:?}"),
    }
}

// =============================================================================
// Parenthesized types
// =============================================================================

#[test]
fn parenthesized_union() {
    let expr = parse_jsdoc_tag_type_payload("(string | number)", None);
    match &expr {
        TypeExpr::Parenthesized(inner) => {
            assert!(matches!(inner.as_ref(), TypeExpr::Union(_)));
        }
        _ => panic!("expected parenthesized, got {expr:?}"),
    }
}

// =============================================================================
// Complex / real-world types
// =============================================================================

#[test]
fn complex_return_type() {
    let expr = parse_jsdoc_tag_type_payload("ReturnType<typeof createConfig>", None);
    match &expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(&**name, "ReturnType");
            assert_eq!(type_arguments.len(), 1);
            assert!(matches!(
                &type_arguments[0],
                TypeExpr::TypeOf(ValueRef { path }) if path == &["createConfig"]
            ));
        }
        _ => panic!("expected ReturnType ref, got {expr:?}"),
    }
}

#[test]
fn complex_pick_omit() {
    let expr = parse_jsdoc_tag_type_payload("Pick<User, \"id\" | \"name\">", None);
    assert!(matches!(&expr, TypeExpr::Ref { name, .. } if &**name == "Pick"));

    let expr2 = parse_jsdoc_tag_type_payload("Omit<User, \"password\">", None);
    assert!(matches!(&expr2, TypeExpr::Ref { name, .. } if &**name == "Omit"));
}

#[test]
fn complex_record_string_literal_keys() {
    let expr = parse_jsdoc_tag_type_payload("Record<\"a\" | \"b\", number>", None);
    match &expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(&**name, "Record");
            assert_eq!(type_arguments.len(), 2);
            assert!(matches!(&type_arguments[0], TypeExpr::Union(_)));
            assert_eq!(
                type_arguments[1],
                TypeExpr::Primitive(PrimitiveName::Number)
            );
        }
        _ => panic!("expected Record ref, got {expr:?}"),
    }
}

#[test]
fn complex_nested_utility() {
    let expr = parse_jsdoc_tag_type_payload("Partial<Pick<User, \"name\" | \"age\">>", None);
    match &expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(&**name, "Partial");
            assert_eq!(type_arguments.len(), 1);
            assert!(matches!(
                &type_arguments[0],
                TypeExpr::Ref { name, .. } if &**name == "Pick"
            ));
        }
        _ => panic!("expected Partial ref, got {expr:?}"),
    }
}

#[test]
fn complex_union_of_objects() {
    let expr = parse_jsdoc_tag_type_payload(
        "{ type: \"a\"; value: string } | { type: \"b\"; count: number }",
        None,
    );
    match &expr {
        TypeExpr::Union(types) => {
            assert_eq!(types.len(), 2);
            assert!(matches!(&types[0], TypeExpr::Object(_)));
            assert!(matches!(&types[1], TypeExpr::Object(_)));
        }
        _ => panic!("expected union, got {expr:?}"),
    }
}

#[test]
fn complex_awaited() {
    let expr = parse_jsdoc_tag_type_payload("Awaited<Promise<string>>", None);
    match &expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(&**name, "Awaited");
            assert_eq!(type_arguments.len(), 1);
            assert!(matches!(
                &type_arguments[0],
                TypeExpr::Ref { name, .. } if &**name == "Promise"
            ));
        }
        _ => panic!("expected Awaited ref, got {expr:?}"),
    }
}

// =============================================================================
// Negative tests: no partial parses, no Unknown leak for valid types
// =============================================================================

#[test]
fn no_unknown_for_primitives() {
    for name in [
        "string",
        "number",
        "boolean",
        "symbol",
        "bigint",
        "any",
        "unknown",
        "void",
        "never",
        "null",
        "undefined",
        "object",
    ] {
        let expr = parse_jsdoc_tag_type_payload(name, None);
        assert!(
            !expr.is_unknown(),
            "{name} should not produce Unknown, got {expr:?}"
        );
    }
}

#[test]
fn no_unknown_for_basic_types() {
    let test_cases = vec![
        "string | number",
        "string & { extra: boolean }",
        "string[]",
        "Array<string>",
        "[string, number]",
        "{ name: string }",
        "(x: string) => void",
        "MyType",
        "Promise<string>",
        "keyof T",
        "typeof myVar",
        "T[\"key\"]",
        "T extends string ? true : false",
        "\"hello\"",
        "42",
        "true",
    ];

    for input in test_cases {
        let expr = parse_jsdoc_tag_type_payload(input, None);
        assert!(
            !expr.is_unknown(),
            "{input} should not produce Unknown, got {expr:?}"
        );
    }
}

#[test]
fn empty_input_is_unknown() {
    let expr = parse_jsdoc_tag_type_payload("", None);
    assert!(expr.is_unknown());
}

#[test]
fn whitespace_only_is_unknown() {
    let expr = parse_jsdoc_tag_type_payload("   ", None);
    assert!(expr.is_unknown());
}

#[test]
fn invalid_union_syntax_is_unknown() {
    let expr = parse_jsdoc_tag_type_payload("string |", None);
    assert!(
        expr.is_unknown(),
        "invalid union should produce Unknown, got {expr:?}"
    );
    assert!(
        !matches!(expr, TypeExpr::Union(_) | TypeExpr::Primitive(_)),
        "invalid union must not lower to a partial type, got {expr:?}"
    );
}

#[test]
fn invalid_object_member_syntax_is_unknown() {
    let expr = parse_jsdoc_tag_type_payload("{ name: }", None);
    assert!(
        expr.is_unknown(),
        "invalid object member should produce Unknown, got {expr:?}"
    );
    assert!(
        !matches!(expr, TypeExpr::Object(_)),
        "invalid object member must not lower to a partial object, got {expr:?}"
    );
}

// =============================================================================
// PrimitiveName round-trip
// =============================================================================

#[test]
fn primitive_name_from_str_round_trip() {
    for name in [
        "string",
        "number",
        "boolean",
        "symbol",
        "bigint",
        "any",
        "unknown",
        "void",
        "never",
        "null",
        "undefined",
        "object",
    ] {
        let parsed = PrimitiveName::parse(name);
        assert!(parsed.is_some(), "{name} should be a recognized primitive");
        assert_eq!(parsed.unwrap().as_str(), name);
    }
}

#[test]
fn primitive_name_from_str_rejects_non_primitives() {
    assert!(PrimitiveName::parse("String").is_none());
    assert!(PrimitiveName::parse("Number").is_none());
    assert!(PrimitiveName::parse("MyType").is_none());
    assert!(PrimitiveName::parse("").is_none());
}
