use super::type_eval::*;
use super::type_eval_build::parse_and_build_env;
use crate::analysis::type_eval_build::{
    expand_macro_types_impl_with_expander, FieldExpansionContext, FieldKind, MacroExpansionScope,
    PathSegment,
};
use crate::analysis::type_expand::{ExpandedNormalizedExpr, ExpansionResult};
use crate::analysis::types::{
    AnalyzedEmitField, AnalyzedMacro, AnalyzedMacroKind, AnalyzedPropField, AnalyzedSlotField,
    AnalyzedSlotFieldBinding, TypeResolutionSource,
};
use std::sync::Arc;
use verter_type_expr::*;

// =============================================================================
// Type alias extraction
// =============================================================================

#[test]
fn extracts_type_alias() {
    let env = parse_and_build_env("type Color = \"red\" | \"blue\" | \"green\"");
    assert!(env.type_symbols.contains_key("Color"));
    let decl = &env.type_symbols["Color"];
    assert_eq!(decl.kind, TypeDeclKind::Alias);
    assert!(decl.type_parameters.is_empty());
    match &decl.body {
        TypeExpr::Union(types) => {
            assert_eq!(types.len(), 3);
            assert!(types.contains(&TypeExpr::string_literal("red")));
        }
        _ => panic!("expected union, got {:?}", decl.body),
    }
}

#[test]
fn extracts_generic_type_alias() {
    let env = parse_and_build_env("type Box<T> = { value: T }");
    let decl = &env.type_symbols["Box"];
    assert_eq!(decl.type_parameters.len(), 1);
    assert_eq!(decl.type_parameters[0].name, "T");
}

#[test]
fn parse_type_parameter_clause_preserves_constraint_and_default() {
    let params =
        super::type_eval_build::parse_type_parameter_clause("T extends Item = DefaultItem, U");

    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "T");
    assert!(matches!(
        params[0].constraint.as_deref(),
        Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
    ));
    assert!(matches!(
        params[0].default.as_deref(),
        Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "DefaultItem"
    ));
    assert_eq!(params[1].name, "U");
    assert!(params[1].constraint.is_none());
    assert!(params[1].default.is_none());
}

#[test]
fn parse_and_build_env_assigns_stable_type_declaration_ids_for_unchanged_source() {
    let env_a = parse_and_build_env("type Box<T> = { value: T }\ninterface User { id: number }");
    let env_b = parse_and_build_env("type Box<T> = { value: T }\ninterface User { id: number }");

    assert_eq!(
        env_a.type_declaration_id("Box"),
        env_b.type_declaration_id("Box")
    );
    assert_eq!(
        env_a.type_declaration_id("User"),
        env_b.type_declaration_id("User")
    );
    assert_eq!(
        env_a.type_symbols["Box"].declaration_id,
        env_b.type_symbols["Box"].declaration_id
    );
    assert_eq!(
        env_a.type_symbols["User"].declaration_id,
        env_b.type_symbols["User"].declaration_id
    );
    assert_ne!(env_a.type_symbols["Box"].declaration_id, 0);
    assert_ne!(env_a.type_symbols["User"].declaration_id, 0);
}

#[test]
fn parse_and_build_env_assigns_stable_value_declaration_ids_for_unchanged_source() {
    let env_a =
        parse_and_build_env("const count: number = 1\nfunction greet(): string { return '' }");
    let env_b =
        parse_and_build_env("const count: number = 1\nfunction greet(): string { return '' }");

    assert_eq!(
        env_a.value_declaration_id("count"),
        env_b.value_declaration_id("count")
    );
    assert_eq!(
        env_a.value_declaration_id("greet"),
        env_b.value_declaration_id("greet")
    );
    assert_eq!(
        env_a.value_symbols["count"].declaration_id,
        env_b.value_symbols["count"].declaration_id
    );
    assert_eq!(
        env_a.value_symbols["greet"].declaration_id,
        env_b.value_symbols["greet"].declaration_id
    );
    assert_ne!(env_a.value_symbols["count"].declaration_id, 0);
    assert_ne!(env_a.value_symbols["greet"].declaration_id, 0);
}

// =============================================================================
// Interface extraction
// =============================================================================

#[test]
fn extracts_interface() {
    let env = parse_and_build_env("interface User { id: number; name: string; email?: string }");
    assert!(env.type_symbols.contains_key("User"));
    let decl = &env.type_symbols["User"];
    assert_eq!(decl.kind, TypeDeclKind::Interface);

    match &decl.body {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 3);
            // Check optional property
            let email = obj.properties.iter().find_map(|m| match m {
                ObjectMember::Property(p) if p.name == "email" => Some(p),
                _ => None,
            });
            assert!(email.is_some());
            assert!(email.unwrap().optional);
        }
        _ => panic!("expected object, got {:?}", decl.body),
    }
}

#[test]
fn extracts_interface_with_extends() {
    let env = parse_and_build_env(
        r#"
        interface Base { id: number }
        interface User extends Base { name: string }
        "#,
    );
    assert!(env.type_symbols.contains_key("Base"));
    assert!(env.type_symbols.contains_key("User"));

    let user = &env.type_symbols["User"];
    // Should be intersection of Base & { name: string }
    match &user.body {
        TypeExpr::Intersection(parts) => {
            assert_eq!(parts.len(), 2);
            assert_eq!(parts[0], TypeExpr::named("Base"));
            assert!(matches!(&parts[1], TypeExpr::Object(_)));
        }
        _ => panic!("expected intersection, got {:?}", user.body),
    }
}

#[test]
fn extracts_interface_with_methods() {
    let env =
        parse_and_build_env("interface Logger { log(msg: string): void; warn(msg: string): void }");
    let decl = &env.type_symbols["Logger"];
    match &decl.body {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
            for member in &obj.properties {
                assert!(matches!(member, ObjectMember::Method(_)));
            }
        }
        _ => panic!("expected object, got {:?}", decl.body),
    }
}

#[test]
fn extracts_namespace_qualified_interfaces() {
    let env = parse_and_build_env(
        r#"
        interface NativeElements {
          div: { id?: string }
        }

        declare namespace JSX {
          interface IntrinsicElements extends NativeElements {}
          interface ElementChildrenAttribute {
            children: {}
          }
        }
        "#,
    );

    assert!(
        env.type_symbols.contains_key("JSX.IntrinsicElements"),
        "namespace interfaces should be registered under their qualified name"
    );
    assert!(
        env.type_symbols
            .contains_key("JSX.ElementChildrenAttribute"),
        "nested namespace members should remain addressable from the eval env"
    );

    let decl = &env.type_symbols["JSX.IntrinsicElements"];
    match &decl.body {
        TypeExpr::Intersection(parts) => {
            assert_eq!(
                parts[0],
                TypeExpr::named("NativeElements"),
                "namespace interfaces should preserve their extends clauses"
            );
            assert!(
                matches!(parts[1], TypeExpr::Object(_)),
                "qualified namespace interfaces should still lower their local members structurally"
            );
        }
        other => panic!("expected namespace interface intersection, got {other:?}"),
    }
}

// =============================================================================
// Function extraction
// =============================================================================

#[test]
fn extracts_function_declaration() {
    let env =
        parse_and_build_env("function greet(name: string, age?: number): string { return name }");
    assert!(env.value_symbols.contains_key("greet"));
    let decl = &env.value_symbols["greet"];
    assert_eq!(decl.kind, ValueDeclKind::Function);
    assert!(decl.function_signature.is_some());

    let sig = decl.function_signature.as_ref().unwrap();
    assert_eq!(sig.parameters.len(), 2);
    assert_eq!(sig.parameters[0].name.as_deref(), Some("name"));
    assert_eq!(
        sig.parameters[0].ty,
        TypeExpr::Primitive(PrimitiveName::String)
    );
    assert!(!sig.parameters[0].optional);
    assert!(sig.parameters[1].optional);
    assert_eq!(
        sig.return_type,
        Some(TypeExpr::Primitive(PrimitiveName::String))
    );
}

#[test]
fn extracts_async_function() {
    let env = parse_and_build_env("async function fetchData(): Promise<string> { return '' }");
    let decl = &env.value_symbols["fetchData"];
    assert_eq!(decl.kind, ValueDeclKind::AsyncFunction);
}

// =============================================================================
// Variable extraction
// =============================================================================

#[test]
fn extracts_const_with_type_annotation() {
    let env = parse_and_build_env("const MAX_SIZE: number = 100");
    assert!(env.value_symbols.contains_key("MAX_SIZE"));
    let decl = &env.value_symbols["MAX_SIZE"];
    assert_eq!(decl.kind, ValueDeclKind::Const);
    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Number))
    );
}

#[test]
fn extracts_const_arrow_function() {
    let env = parse_and_build_env("const add = (a: number, b: number): number => a + b");
    let decl = &env.value_symbols["add"];
    assert!(decl.function_signature.is_some());
    let sig = decl.function_signature.as_ref().unwrap();
    assert_eq!(sig.parameters.len(), 2);
    assert_eq!(
        sig.return_type,
        Some(TypeExpr::Primitive(PrimitiveName::Number))
    );
}

#[test]
fn extracts_const_object_literal() {
    let env = parse_and_build_env(r#"const defaults = { theme: "dark", debug: false }"#);
    let decl = &env.value_symbols["defaults"];
    assert!(decl.object_shape.is_some());
    let shape = decl.object_shape.as_ref().unwrap();
    assert_eq!(shape.properties.len(), 2);
}

#[test]
fn extracts_const_asserted_object_literal_without_degrading_to_unknown_const() {
    let env = parse_and_build_env(r#"const theme = { color: { primary: "" } } as const"#);
    let decl = &env.value_symbols["theme"];

    assert!(
        decl.object_shape.is_some(),
        "const assertions should preserve the underlying object literal shape"
    );
    assert!(
        matches!(decl.type_annotation, Some(TypeExpr::Object(_))),
        "const assertions should infer the object literal type instead of an opaque const marker, got {:?}",
        decl.type_annotation
    );
}

#[test]
fn extracts_let_variable() {
    let env = parse_and_build_env("let count: number = 0");
    let decl = &env.value_symbols["count"];
    assert_eq!(decl.kind, ValueDeclKind::Let);
}

#[test]
fn infers_non_empty_array_element_types() {
    let env = parse_and_build_env("const items = [1, 2, 3]");
    let decl = &env.value_symbols["items"];
    let Some(TypeExpr::Array { element, .. }) = decl.type_annotation.as_ref() else {
        panic!(
            "expected inferred array type, got {:?}",
            decl.type_annotation
        );
    };

    assert!(
        !matches!(element.as_ref(), TypeExpr::Primitive(PrimitiveName::Any)),
        "non-empty arrays should not infer Array<any>"
    );
    match element.as_ref() {
        TypeExpr::Primitive(PrimitiveName::Number) => {}
        TypeExpr::Literal(LiteralValue::Number(_)) => {}
        TypeExpr::Union(members) => {
            assert!(
                members.iter().all(|member| matches!(
                    member,
                    TypeExpr::Literal(LiteralValue::Number(_))
                        | TypeExpr::Primitive(PrimitiveName::Number)
                )),
                "array element union should stay numeric, got {members:?}"
            );
        }
        other => panic!("expected numeric element type, got {other:?}"),
    }
}

#[test]
fn infers_mixed_array_element_union() {
    let env = parse_and_build_env(r#"const mixed = [1, "hello", true]"#);
    let decl = &env.value_symbols["mixed"];
    let Some(TypeExpr::Array { element, .. }) = decl.type_annotation.as_ref() else {
        panic!(
            "expected inferred array type, got {:?}",
            decl.type_annotation
        );
    };

    let TypeExpr::Union(members) = element.as_ref() else {
        panic!("mixed arrays should infer a union element type, got {element:?}");
    };
    assert!(
        members.iter().any(|member| matches!(
            member,
            TypeExpr::Literal(LiteralValue::Number(_)) | TypeExpr::Primitive(PrimitiveName::Number)
        )),
        "mixed array should include a numeric branch"
    );
    assert!(
        members.iter().any(|member| matches!(
            member,
            TypeExpr::Literal(LiteralValue::String(value)) if value == "hello"
        ) || matches!(
            member,
            TypeExpr::Primitive(PrimitiveName::String)
        )),
        "mixed array should include a string branch"
    );
    assert!(
        members.iter().any(|member| matches!(
            member,
            TypeExpr::Literal(LiteralValue::Boolean(true))
                | TypeExpr::Primitive(PrimitiveName::Boolean)
        )),
        "mixed array should include a boolean branch"
    );
    assert!(
        !members
            .iter()
            .any(|member| matches!(member, TypeExpr::Primitive(PrimitiveName::Any))),
        "mixed arrays should not keep any once element types are known"
    );
}

#[test]
fn infers_array_spread_literal_element_types() {
    let env = parse_and_build_env(r#"const mixed = [...[1, 2], "hello"]"#);
    let decl = &env.value_symbols["mixed"];
    let Some(TypeExpr::Array { element, .. }) = decl.type_annotation.as_ref() else {
        panic!(
            "expected inferred array type, got {:?}",
            decl.type_annotation
        );
    };

    let TypeExpr::Union(members) = element.as_ref() else {
        panic!("array spread literal should infer a union element type, got {element:?}");
    };
    assert!(
        members.iter().any(|member| matches!(
            member,
            TypeExpr::Literal(LiteralValue::Number(_)) | TypeExpr::Primitive(PrimitiveName::Number)
        )),
        "spread literal array should contribute numeric element types"
    );
    assert!(
        members.iter().any(|member| matches!(
            member,
            TypeExpr::Literal(LiteralValue::String(value)) if value == "hello"
        ) || matches!(
            member,
            TypeExpr::Primitive(PrimitiveName::String)
        )),
        "array literal should retain the non-spread string branch"
    );
}

#[test]
fn empty_array_stays_any_array() {
    let env = parse_and_build_env("const empty = []");
    let decl = &env.value_symbols["empty"];

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::Any)),
            readonly: false,
        })
    );
}

#[test]
fn infers_template_literal_with_expressions_as_string() {
    let env = parse_and_build_env(r#"const name = "world"; const label = `hello ${name}`"#);
    let decl = &env.value_symbols["label"];

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::String))
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Any)),
        "template literals with expressions should not fall back to any"
    );
}

#[test]
fn const_preserves_literal_initializer_type() {
    let env = parse_and_build_env(r#"const greeting = "hello""#);
    let decl = &env.value_symbols["greeting"];

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::string_literal("hello"))
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::String)),
        "const literal initializers should remain literal types"
    );
}

#[test]
fn let_widens_string_literal_initializer() {
    let env = parse_and_build_env(r#"let greeting = "hello""#);
    let decl = &env.value_symbols["greeting"];

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::String))
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::string_literal("hello")),
        "let string initializers should widen away from literal types"
    );
}

#[test]
fn let_widens_number_literal_initializer() {
    let env = parse_and_build_env("let count = 42");
    let decl = &env.value_symbols["count"];

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Number))
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::number_literal(42.0)),
        "let number initializers should widen away from literal types"
    );
}

#[test]
fn let_widens_boolean_literal_initializer() {
    let env = parse_and_build_env("let enabled = true");
    let decl = &env.value_symbols["enabled"];

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Boolean))
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::boolean_literal(true)),
        "let boolean initializers should widen away from literal types"
    );
}

#[test]
fn var_widens_string_literal_initializer() {
    let env = parse_and_build_env(r#"var greeting = "hello""#);
    let decl = &env.value_symbols["greeting"];

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::String))
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::string_literal("hello")),
        "var string initializers should widen away from literal types"
    );
}

#[test]
fn let_widens_nested_object_literal_properties() {
    let env = parse_and_build_env(r#"let settings = { mode: "dark", nested: { count: 1 } }"#);
    let decl = &env.value_symbols["settings"];
    let Some(TypeExpr::Object(obj)) = decl.type_annotation.as_ref() else {
        panic!(
            "expected object type for let object initializer, got {:?}",
            decl.type_annotation
        );
    };

    let mode_ty = obj.properties.iter().find_map(|member| match member {
        ObjectMember::Property(prop) if prop.name == "mode" => Some(&prop.ty),
        _ => None,
    });
    assert_eq!(mode_ty, Some(&TypeExpr::Primitive(PrimitiveName::String)));

    let nested_ty = obj.properties.iter().find_map(|member| match member {
        ObjectMember::Property(prop) if prop.name == "nested" => Some(&prop.ty),
        _ => None,
    });
    let Some(TypeExpr::Object(nested)) = nested_ty else {
        panic!("expected nested object property, got {nested_ty:?}");
    };
    let count_ty = nested.properties.iter().find_map(|member| match member {
        ObjectMember::Property(prop) if prop.name == "count" => Some(&prop.ty),
        _ => None,
    });
    assert_eq!(count_ty, Some(&TypeExpr::Primitive(PrimitiveName::Number)));
}

#[test]
fn let_widens_array_element_literals() {
    let env = parse_and_build_env("let flags = [true, false]");
    let decl = &env.value_symbols["flags"];
    let Some(TypeExpr::Array { element, .. }) = decl.type_annotation.as_ref() else {
        panic!(
            "expected array type for let array initializer, got {:?}",
            decl.type_annotation
        );
    };

    assert_eq!(
        element.as_ref(),
        &TypeExpr::Primitive(PrimitiveName::Boolean)
    );
}

// =============================================================================
// satisfies expression inference
// =============================================================================

#[test]
fn satisfies_preserves_underlying_value_type() {
    let env = parse_and_build_env(
        r#"const config = { x: 1, y: "hello" } satisfies { x: number; y: string }"#,
    );
    let decl = &env.value_symbols["config"];
    let Some(TypeExpr::Object(obj)) = decl.type_annotation.as_ref() else {
        panic!(
            "satisfies should infer the underlying object literal type, got {:?}",
            decl.type_annotation
        );
    };

    // The value type should have literal/inferred properties from the expression,
    // not abstract types from the satisfies annotation
    let x_prop = obj.properties.iter().find_map(|member| match member {
        ObjectMember::Property(p) if p.name == "x" => Some(&p.ty),
        _ => None,
    });
    assert!(
        x_prop.is_some(),
        "satisfies result should include x property from the value"
    );

    // x should be a number literal (1), not just `number`
    assert!(
        matches!(x_prop.unwrap(), TypeExpr::Literal(LiteralValue::Number(_))),
        "satisfies should preserve literal types from the value expression, got {:?}",
        x_prop,
    );
}

#[test]
fn satisfies_does_not_use_annotation_type() {
    // When using satisfies, the expression type should win, not the annotation
    let env = parse_and_build_env(r#"const label = "hello" satisfies string"#);
    let decl = &env.value_symbols["label"];

    // Should be the literal "hello", not widened string
    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::string_literal("hello")),
        "satisfies should preserve the value's literal type"
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::String)),
        "satisfies should not widen to the annotation type"
    );
}

// =============================================================================
// Object spread in extract_object_literal
// =============================================================================

#[test]
fn object_spread_identifier_produces_intersection() {
    let env = parse_and_build_env(r#"const extended = { ...base, extra: true }"#);
    let decl = &env.value_symbols["extended"];

    // Should not lose the spread source — at minimum, the explicit props must be present
    // AND the spread source should be represented (as typeof base in an intersection)
    match decl.type_annotation.as_ref() {
        Some(TypeExpr::Intersection(members)) => {
            assert!(
                members.iter().any(|m| matches!(m, TypeExpr::TypeOf(_))),
                "spread identifier should produce a typeof reference in the intersection"
            );
            assert!(
                members.iter().any(|m| matches!(m, TypeExpr::Object(_))),
                "explicit properties should be present in the intersection"
            );
        }
        Some(TypeExpr::Object(obj)) => {
            // At minimum, if we flatten, the explicit property must exist
            assert!(
                obj.properties.iter().any(|member| matches!(
                    member,
                    ObjectMember::Property(p) if p.name == "extra"
                )),
                "explicit property 'extra' must be present"
            );
            panic!(
                "spread source was lost — expected intersection with typeof base, got plain object"
            );
        }
        other => panic!("expected intersection or object, got {other:?}"),
    }
}

#[test]
fn object_spread_object_literal_merges_properties() {
    let env = parse_and_build_env(r#"const merged = { ...{ a: 1, b: 2 }, c: 3 }"#);
    let decl = &env.value_symbols["merged"];

    let Some(TypeExpr::Object(obj)) = decl.type_annotation.as_ref() else {
        panic!(
            "expected object type for merged spread, got {:?}",
            decl.type_annotation
        );
    };

    let names: Vec<&str> = obj
        .properties
        .iter()
        .filter_map(|m| match m {
            ObjectMember::Property(p) => Some(p.name.as_str()),
            _ => None,
        })
        .collect();

    assert!(
        names.contains(&"a"),
        "spread object literal property 'a' should be merged"
    );
    assert!(
        names.contains(&"b"),
        "spread object literal property 'b' should be merged"
    );
    assert!(
        names.contains(&"c"),
        "explicit property 'c' should be present"
    );
    assert_eq!(
        names.len(),
        3,
        "should have exactly 3 properties after merge"
    );
}

#[test]
fn object_spread_later_property_overrides_spread_property() {
    let env = parse_and_build_env(r#"const merged = { ...{ a: 1 }, a: "override" }"#);
    let decl = &env.value_symbols["merged"];

    let Some(TypeExpr::Object(obj)) = decl.type_annotation.as_ref() else {
        panic!(
            "expected object type for merged spread override, got {:?}",
            decl.type_annotation
        );
    };

    let props: Vec<_> = obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(prop) if prop.name == "a" => Some(&prop.ty),
            _ => None,
        })
        .collect();

    assert_eq!(
        props.len(),
        1,
        "later explicit properties should replace earlier spread properties"
    );
    assert_eq!(props[0], &TypeExpr::string_literal("override"));
}

#[test]
fn object_spread_later_spread_overrides_earlier_property() {
    let env = parse_and_build_env(r#"const merged = { a: 1, ...{ a: "override" } }"#);
    let decl = &env.value_symbols["merged"];

    let Some(TypeExpr::Object(obj)) = decl.type_annotation.as_ref() else {
        panic!(
            "expected object type for merged spread override, got {:?}",
            decl.type_annotation
        );
    };

    let props: Vec<_> = obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(prop) if prop.name == "a" => Some(&prop.ty),
            _ => None,
        })
        .collect();

    assert_eq!(
        props.len(),
        1,
        "later spread properties should replace earlier explicit properties"
    );
    assert_eq!(props[0], &TypeExpr::string_literal("override"));
}

// =============================================================================
// MemberExpression inference
// =============================================================================

#[test]
fn static_member_expression_infers_typeof_path() {
    let env = parse_and_build_env(r#"const value = obj.foo"#);
    let decl = &env.value_symbols["value"];

    match decl.type_annotation.as_ref() {
        Some(TypeExpr::TypeOf(vr)) => {
            assert_eq!(
                vr.path,
                vec!["obj".to_string(), "foo".to_string()],
                "static member expression should produce typeof with dotted path"
            );
        }
        other => panic!("expected TypeOf with path [obj, foo], got {other:?}"),
    }
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Any)),
        "member expression should not degrade to any"
    );
}

#[test]
fn nested_member_expression_infers_deep_typeof_path() {
    let env = parse_and_build_env(r#"const value = a.b.c"#);
    let decl = &env.value_symbols["value"];

    match decl.type_annotation.as_ref() {
        Some(TypeExpr::TypeOf(vr)) => {
            assert_eq!(
                vr.path,
                vec!["a".to_string(), "b".to_string(), "c".to_string()],
                "nested member expression should produce typeof with full path"
            );
        }
        other => panic!("expected TypeOf with path [a, b, c], got {other:?}"),
    }
}

#[test]
fn member_on_call_expression_degrades_to_any() {
    // fn().prop — the root is a CallExpression, not an Identifier, so we can't build a simple path
    let env = parse_and_build_env(r#"const value = getObj().prop"#);
    let decl = &env.value_symbols["value"];

    // Should not produce a broken partial path like ["prop"] without the root
    // Any or None is acceptable — the key assertion is no broken partial path.
    if let Some(TypeExpr::TypeOf(vr)) = decl.type_annotation.as_ref() {
        panic!(
            "call-rooted member path should not produce TypeOf, got path {:?}",
            vr.path
        );
    }
}

// =============================================================================
// CallExpression inference
// =============================================================================

#[test]
fn simple_call_expression_does_not_degrade_to_any() {
    let env = parse_and_build_env(r#"const result = someFunction()"#);
    let decl = &env.value_symbols["result"];

    // For unknown function calls, should produce ReturnType<typeof someFunction>
    // rather than degrading to Any
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Any)),
        "call expression should not degrade to any — should produce ReturnType<typeof fn>"
    );
    // Should be some kind of structured type reference
    assert!(
        decl.type_annotation.is_some(),
        "call expression should produce a type annotation"
    );
}

#[test]
fn method_call_expression_does_not_degrade_to_any() {
    let env = parse_and_build_env(r#"const result = obj.create()"#);
    let decl = &env.value_symbols["result"];

    assert!(
        decl.type_annotation.is_some(),
        "method call expression should produce a type, not None (filtered-out Any)"
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Any)),
        "method call expression should not degrade to any"
    );
}

// =============================================================================
// Class extraction
// =============================================================================

#[test]
fn extracts_class_as_type_and_value() {
    let env = parse_and_build_env(
        r#"
        class Widget {
            readonly id: number;
            name?: string;
            constructor(id: number) {}
            render(): void {}
        }
        "#,
    );
    // Should be in both type and value symbols
    assert!(env.type_symbols.contains_key("Widget"));
    assert!(env.value_symbols.contains_key("Widget"));

    let type_decl = &env.type_symbols["Widget"];
    assert_eq!(type_decl.kind, TypeDeclKind::Class);
    match &type_decl.body {
        TypeExpr::Object(obj) => {
            // id, name, render (constructor is not a member)
            assert_eq!(obj.properties.len(), 3);
            let id_prop = obj.properties.iter().find_map(|m| match m {
                ObjectMember::Property(p) if p.name == "id" => Some(p),
                _ => None,
            });
            assert!(id_prop.unwrap().readonly);
        }
        _ => panic!("expected object, got {:?}", type_decl.body),
    }

    let value_decl = &env.value_symbols["Widget"];
    assert_eq!(value_decl.kind, ValueDeclKind::Class);
    assert!(value_decl.function_signature.is_some()); // constructor
}

// =============================================================================
// Export declarations
// =============================================================================

#[test]
fn extracts_exported_types() {
    let env = parse_and_build_env("export type Status = \"active\" | \"inactive\"");
    assert!(env.type_symbols.contains_key("Status"));
}

#[test]
fn extracts_exported_functions() {
    let env = parse_and_build_env("export function helper(): void {}");
    assert!(env.value_symbols.contains_key("helper"));
}

#[test]
fn extracts_exported_interfaces() {
    let env = parse_and_build_env("export interface Config { debug: boolean }");
    assert!(env.type_symbols.contains_key("Config"));
}

#[test]
fn extracts_export_default_object_expression_as_default_value() {
    let env = parse_and_build_env(
        r#"
        export default {
            item: "item",
            body: "body",
        }
        "#,
    );

    let decl = env
        .value_symbols
        .get("default")
        .expect("export default object should register a synthetic default value");

    let ty = decl
        .type_annotation
        .as_ref()
        .expect("default export should preserve a lowered type annotation");
    let TypeExpr::Object(obj) = ty else {
        panic!("expected default export type to be an object, got {ty:?}");
    };

    let names: Vec<&str> = obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(prop) => Some(prop.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(names.contains(&"item"));
    assert!(names.contains(&"body"));
}

// =============================================================================
// Negative tests
// =============================================================================

#[test]
fn no_type_symbols_for_plain_variables() {
    let env = parse_and_build_env("const x = 42");
    assert!(!env.type_symbols.contains_key("x"));
    assert!(env.value_symbols.contains_key("x"));
}

#[test]
fn no_value_symbols_for_type_aliases() {
    let env = parse_and_build_env("type Foo = string");
    assert!(env.type_symbols.contains_key("Foo"));
    assert!(!env.value_symbols.contains_key("Foo"));
}

#[test]
fn parse_and_build_env_preserves_union_type_aliases_with_local_interface_refs() {
    let env = parse_and_build_env(
        r#"
export interface St { path: string }
export interface vt { name: string }
type RouteLocationRaw = string | St | vt
export { RouteLocationRaw as Lt, St, vt }
"#,
    );

    let route = env
        .type_symbols
        .get("RouteLocationRaw")
        .expect("RouteLocationRaw alias should be registered");
    let TypeExpr::Union(types) = &route.body else {
        panic!(
            "RouteLocationRaw should stay a union before evaluation, got {:?}",
            route.body
        );
    };
    assert!(
        types
            .iter()
            .any(|ty| matches!(ty, TypeExpr::Primitive(PrimitiveName::String))),
        "RouteLocationRaw should preserve its string branch, got {:?}",
        route.body
    );
    assert!(
        types.iter().any(|ty| {
            matches!(ty, TypeExpr::Ref { name, .. } if name.as_ref() == "St")
                || matches!(
                    ty,
                    TypeExpr::Object(shape)
                        if shape.properties.iter().any(|member| matches!(member, ObjectMember::Property(property) if property.name == "path"))
                )
        }),
        "RouteLocationRaw should preserve its path-like branch, got {:?}",
        route.body
    );
    assert!(
        types.iter().any(|ty| {
            matches!(ty, TypeExpr::Ref { name, .. } if name.as_ref() == "vt")
                || matches!(
                    ty,
                    TypeExpr::Object(shape)
                        if shape.properties.iter().any(|member| matches!(member, ObjectMember::Property(property) if property.name == "name"))
                )
        }),
        "RouteLocationRaw should preserve its name-like branch, got {:?}",
        route.body
    );
}

// ───────────────────────────────────────────────────────────────────────────
// W2.5 — `expand_macro_types_impl_with_expander` reads
// `field.type_expr` / `field.payload_expr` / `binding.binding_expr`
// directly, never reparsing `type_annotation` text. The discriminator is
// to populate every typed field with a structural shape that the matching
// raw `*_annotation` text does NOT describe — pre-cutover the function
// would have reparsed the text and produced the WRONG shape; post-cutover
// it walks the typed form and the closure receives the producer-supplied
// expression unchanged.
// ───────────────────────────────────────────────────────────────────────────
fn passthrough_expander(
) -> impl FnMut(FieldExpansionContext, &TypeExpr) -> ExpansionResult<ExpandedNormalizedExpr> {
    |_ctx, expr| ExpansionResult::exact(ExpandedNormalizedExpr { expr: expr.clone() })
}

fn make_synth_typed_prop(name: &str, typed: TypeExpr) -> AnalyzedPropField {
    AnalyzedPropField {
        name: name.to_string(),
        is_optional: false,
        span: verter_span::Span::default(),
        // `type_annotation` text deliberately does NOT describe the typed
        // shape: pre-cutover reparse would have produced a different
        // structure, post-cutover the typed form survives.
        type_annotation: Some("garbage<<<unparseable".to_string()),
        type_expr: Some(typed),
        type_expr_scope: Some(TypeExprScope::new("test:fixture")),
        description: None,
        tags: Vec::new(),
        resolution_source: TypeResolutionSource::Rust,
        resolution_error: None,
    }
}

fn make_synth_typed_emit(name: &str, typed: TypeExpr) -> AnalyzedEmitField {
    AnalyzedEmitField {
        name: name.to_string(),
        span: verter_span::Span::default(),
        payload_type: Some("garbage<<<unparseable".to_string()),
        payload_expr: Some(typed),
        payload_expr_scope: Some(TypeExprScope::new("test:fixture")),
        description: None,
        tags: Vec::new(),
    }
}

fn make_synth_typed_slot(
    name: &str,
    binding_name: &str,
    binding_typed: TypeExpr,
) -> AnalyzedSlotField {
    AnalyzedSlotField {
        name: name.to_string(),
        is_required: false,
        span: verter_span::Span::default(),
        bindings: vec![AnalyzedSlotFieldBinding {
            name: binding_name.to_string(),
            type_annotation: Some("garbage<<<unparseable".to_string()),
            span: verter_span::Span::default(),
            binding_expr: Some(binding_typed),
            binding_expr_scope: Some(TypeExprScope::new("test:fixture")),
        }],
        return_type: None,
        description: None,
        tags: Vec::new(),
        return_expr: None,
        return_expr_scope: None,
    }
}

fn make_synth_macro(
    kind: AnalyzedMacroKind,
    props: Vec<AnalyzedPropField>,
    emits: Vec<AnalyzedEmitField>,
    slots: Vec<AnalyzedSlotField>,
) -> AnalyzedMacro {
    AnalyzedMacro {
        kind,
        is_type_based: true,
        type_references: Vec::new(),
        binding_name: None,
        model_name: None,
        has_inherit_attrs_false: false,
        prop_fields: props,
        emit_fields: emits,
        slot_fields: slots,
        default_keys: Vec::new(),
        default_values: Vec::new(),
        expose_fields: Vec::new(),
        resolved_local_types: Vec::new(),
        parsed_type_argument: None,
        parsed_type_argument_scope: None,
        span: verter_span::Span::default(),
    }
}

#[test]
fn expand_macro_types_reads_prop_field_type_expr_directly_without_reparse() {
    // A shape the producer captured (e.g. via cross-file external
    // resolution) that text reparsing cannot reproduce.
    let typed_indexed_access = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::Ref {
            name: "ImportedAlias".into(),
            type_arguments: Vec::<TypeExpr>::new().into(),
        }),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("a".to_string()))),
    };

    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineProps,
        vec![make_synth_typed_prop("foo", typed_indexed_access.clone())],
        Vec::new(),
        Vec::new(),
    )];

    let result = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        passthrough_expander(),
    );

    assert_eq!(
        result.props.len(),
        1,
        "the prop field's typed expression should drive expansion, got {result:?}"
    );
    assert_eq!(
        result.props[0].r#type, typed_indexed_access,
        "expand_macro_types_impl_with_expander must consume field.type_expr directly, not reparse type_annotation"
    );
    assert_eq!(
        result.props[0].raw_type.as_deref(),
        Some("garbage<<<unparseable"),
        "raw_type passthrough should preserve the original annotation text"
    );

    // Negative discrimination: prove the typed shape differs from what
    // the text parser would have produced. If they happened to coincide,
    // the test would not be characterising the typed-read change.
    let from_text = verter_type_expr_oxc::parse_type_annotation("garbage<<<unparseable");
    assert_ne!(
        from_text, typed_indexed_access,
        "annotation text MUST NOT round-trip back to the typed shape; otherwise the test does not discriminate"
    );
}

#[test]
fn expand_macro_types_reads_emit_payload_expr_directly_without_reparse() {
    let typed_tuple = TypeExpr::Tuple {
        elements: vec![TupleElement {
            label: Some("payload".to_string()),
            ty: TypeExpr::Ref {
                name: "ImportedPayload".into(),
                type_arguments: Vec::<TypeExpr>::new().into(),
            },
            optional: false,
            rest: false,
        }]
        .into(),
        readonly: false,
    };

    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineEmits,
        Vec::new(),
        vec![make_synth_typed_emit("update", typed_tuple.clone())],
        Vec::new(),
    )];

    let result = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        passthrough_expander(),
    );

    assert_eq!(
        result.emits.len(),
        1,
        "emit should expand from payload_expr"
    );
    assert_eq!(
        result.emits[0].r#type, typed_tuple,
        "expand_macro_types_impl_with_expander must consume field.payload_expr directly"
    );

    let from_text = verter_type_expr_oxc::parse_type_annotation("garbage<<<unparseable");
    assert_ne!(from_text, typed_tuple);
}

#[test]
fn expand_macro_types_reads_slot_binding_expr_directly_without_reparse() {
    let typed_indexed_access = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::Ref {
            name: "SlotProps".into(),
            type_arguments: Vec::<TypeExpr>::new().into(),
        }),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("item".to_string()))),
    };

    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineSlots,
        Vec::new(),
        Vec::new(),
        vec![make_synth_typed_slot(
            "default",
            "item",
            typed_indexed_access.clone(),
        )],
    )];

    let result = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        passthrough_expander(),
    );

    assert_eq!(
        result.slot_bindings.len(),
        1,
        "slot binding should expand from binding_expr"
    );
    assert_eq!(result.slot_bindings[0].name, "default.item");
    assert_eq!(
        result.slot_bindings[0].r#type, typed_indexed_access,
        "expand_macro_types_impl_with_expander must consume binding.binding_expr directly"
    );

    let from_text = verter_type_expr_oxc::parse_type_annotation("garbage<<<unparseable");
    assert_ne!(from_text, typed_indexed_access);
}

#[test]
fn expand_macro_types_skips_field_when_typed_form_is_absent_or_unknown() {
    // Producer left `type_expr` unset — the function does NOT fall back
    // to reparsing `type_annotation` text. The expansion vector stays
    // empty.
    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineProps,
        vec![AnalyzedPropField {
            name: "foo".to_string(),
            is_optional: false,
            span: verter_span::Span::default(),
            type_annotation: Some("string".to_string()),
            type_expr: None,
            type_expr_scope: None,
            description: None,
            tags: Vec::new(),
            resolution_source: TypeResolutionSource::Rust,
            resolution_error: None,
        }],
        Vec::new(),
        Vec::new(),
    )];

    let result = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        passthrough_expander(),
    );

    assert!(
        result.props.is_empty(),
        "no typed form ⇒ no expansion (would have been non-empty if reparse fallback existed); got {result:?}"
    );
}

#[test]
fn expand_macro_types_threads_field_kind_and_path_through_closure() {
    let typed_string = TypeExpr::Primitive(PrimitiveName::String);
    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineProps,
        vec![make_synth_typed_prop("alpha", typed_string.clone())],
        Vec::new(),
        Vec::new(),
    )];

    let mut captured: Vec<(FieldKind, Vec<String>)> = Vec::new();
    let _ = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        |ctx, expr| {
            let path: Vec<String> = ctx
                .output_path
                .iter()
                .map(|seg| match seg {
                    PathSegment::Member(name) => name.to_string(),
                })
                .collect();
            captured.push((ctx.kind, path));
            ExpansionResult::exact(ExpandedNormalizedExpr { expr: expr.clone() })
        },
    );

    assert_eq!(captured, vec![(FieldKind::Prop, vec!["alpha".to_string()])]);
}

// ── W1.1c: `ExpandedField.shallow_type_expr` carries the analyzer-side
//          shallow typed sidecar through the expander ──
//
// The producer at `expand_macro_types_impl_with_expander` reads
// `field.type_expr` / `field.payload_expr` / `binding.binding_expr`
// (shallow, analyzer-populated) and stamps each onto
// `ExpandedField.shallow_type_expr` (+ paired scope). Pre-W1.1c the
// field did not exist; consumers fell back to reparsing `raw_type`.
// Post-W1.1c the bare alias `Ref` is preserved alongside the
// (potentially distinct) post-expansion `r#type`.

#[test]
fn expand_macro_types_props_publish_shallow_type_expr_from_prop_field_typed_form() {
    // The analyzer captured a bare `Ref` for `foo: ImportedAlias`. The
    // passthrough expander leaves `r#type` equal to the input, but the
    // discriminating assertion is that `shallow_type_expr` is
    // independently populated from the producer's analyzer-side
    // `field.type_expr` and survives all the way to the `ExpandedField`.
    let bare_ref = TypeExpr::Ref {
        name: "ImportedAlias".into(),
        type_arguments: Vec::<TypeExpr>::new().into(),
    };
    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineProps,
        vec![make_synth_typed_prop("foo", bare_ref.clone())],
        Vec::new(),
        Vec::new(),
    )];

    let result = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        passthrough_expander(),
    );

    assert_eq!(result.props.len(), 1);
    // Discriminator: pre-W1.1c the field did not exist; post-W1.1c it
    // carries the bare alias `Ref` from `field.type_expr`.
    assert_eq!(
        result.props[0].shallow_type_expr.as_ref(),
        Some(&bare_ref),
        "shallow_type_expr must surface the analyzer-side bare alias Ref directly"
    );
    // Pairing invariant: scope present iff expr present.
    assert!(
        result.props[0].shallow_type_expr_scope.is_some(),
        "shallow_type_expr_scope must be populated when shallow_type_expr is Some"
    );
    assert_eq!(
        result.props[0]
            .shallow_type_expr_scope
            .as_ref()
            .map(|s| s.as_str()),
        Some("test:fixture"),
        "shallow_type_expr_scope must inherit the analyzer field's scope"
    );
}

#[test]
fn expand_macro_types_emits_publish_shallow_type_expr_from_emit_field_typed_form() {
    let bare_ref = TypeExpr::Ref {
        name: "ImportedPayload".into(),
        type_arguments: Vec::<TypeExpr>::new().into(),
    };
    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineEmits,
        Vec::new(),
        vec![make_synth_typed_emit("update", bare_ref.clone())],
        Vec::new(),
    )];

    let result = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        passthrough_expander(),
    );

    assert_eq!(result.emits.len(), 1);
    assert_eq!(
        result.emits[0].shallow_type_expr.as_ref(),
        Some(&bare_ref),
        "shallow_type_expr must surface the analyzer-side bare alias Ref for emits"
    );
    assert!(result.emits[0].shallow_type_expr_scope.is_some());
    assert_eq!(
        result.emits[0]
            .shallow_type_expr_scope
            .as_ref()
            .map(|s| s.as_str()),
        Some("test:fixture")
    );
}
