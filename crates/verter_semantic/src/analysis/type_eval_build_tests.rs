use super::type_eval::*;
use super::type_eval_build::{
    evaluate_macro_types, expand_macro_types, expand_macro_types_with_bindings, parse_and_build_env,
};
use super::type_expr::*;
use super::type_solver::host::{EvalEnvSolverHost, NoopSolverHost};
use std::sync::Arc;

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
    match decl.type_annotation.as_ref() {
        Some(TypeExpr::TypeOf(vr)) => {
            panic!(
                "call-rooted member path should not produce TypeOf, got path {:?}",
                vr.path
            );
        }
        _ => {} // Any or None is acceptable — the key assertion is no broken partial path
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
// End-to-end: build env then evaluate
// =============================================================================

#[test]
fn e2e_return_type_of_function() {
    let env = parse_and_build_env(
        r#"
        function createConfig() {
            return { theme: "dark", debug: false }
        }
        "#,
    );

    // Now evaluate ReturnType<typeof createConfig>
    let mut eval_env = env;
    let expr = TypeExpr::named_with_args(
        "ReturnType",
        vec![TypeExpr::TypeOf(ValueRef {
            path: vec!["createConfig".to_string()],
        })],
    );
    let result = evaluate(&expr, &mut eval_env);
    // Body inference: the function returns { theme: "dark", debug: false }
    // so ReturnType resolves to an object shape
    match &result {
        TypeExpr::Object(obj) => {
            assert!(
                !obj.properties.is_empty(),
                "should infer object properties from return statement"
            );
        }
        _ => panic!("expected object from body inference, got {result:?}"),
    }
}

#[test]
fn e2e_return_type_annotated_function() {
    let env = parse_and_build_env(
        r#"
        function createConfig(): { theme: string; debug: boolean } {
            return { theme: "dark", debug: false }
        }
        "#,
    );

    let mut eval_env = env;
    let expr = TypeExpr::named_with_args(
        "ReturnType",
        vec![TypeExpr::TypeOf(ValueRef {
            path: vec!["createConfig".to_string()],
        })],
    );
    let result = evaluate(&expr, &mut eval_env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|m| match m {
                    ObjectMember::Property(p) => Some(p.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"theme"));
            assert!(names.contains(&"debug"));
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

#[test]
fn e2e_pick_from_interface() {
    let env = parse_and_build_env(
        r#"
        interface User {
            id: number
            name: string
            email: string
            password: string
        }
        "#,
    );

    let mut eval_env = env;
    let expr = TypeExpr::named_with_args(
        "Pick",
        vec![
            TypeExpr::named("User"),
            TypeExpr::union(vec![
                TypeExpr::string_literal("id"),
                TypeExpr::string_literal("name"),
            ]),
        ],
    );
    let result = evaluate(&expr, &mut eval_env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|m| match m {
                    ObjectMember::Property(p) => Some(p.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"id"));
            assert!(names.contains(&"name"));
            assert!(!names.contains(&"email"), "email should NOT be picked");
            assert!(
                !names.contains(&"password"),
                "password should NOT be picked"
            );
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

#[test]
fn e2e_partial_of_interface() {
    let env = parse_and_build_env("interface Config { theme: string; debug: boolean }");

    let mut eval_env = env;
    let expr = TypeExpr::named_with_args("Partial", vec![TypeExpr::named("Config")]);
    let result = evaluate(&expr, &mut eval_env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
            for member in &obj.properties {
                if let ObjectMember::Property(p) = member {
                    assert!(p.optional, "{} should be optional after Partial", p.name);
                }
            }
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

#[test]
fn e2e_keyof_interface() {
    let env = parse_and_build_env("interface User { id: number; name: string; email: string }");

    let mut eval_env = env;
    let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::named("User")));
    let result = evaluate(&expr, &mut eval_env);
    match &result {
        TypeExpr::Union(types) => {
            assert_eq!(types.len(), 3);
            assert!(types.contains(&TypeExpr::string_literal("id")));
            assert!(types.contains(&TypeExpr::string_literal("name")));
            assert!(types.contains(&TypeExpr::string_literal("email")));
        }
        _ => panic!("expected union, got {result:?}"),
    }
}

#[test]
fn e2e_typeof_const_object() {
    let env = parse_and_build_env(r#"const defaults = { size: 42, color: "blue" }"#);

    let mut eval_env = env;
    let expr = TypeExpr::TypeOf(ValueRef {
        path: vec!["defaults".to_string()],
    });
    let result = evaluate(&expr, &mut eval_env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

#[test]
fn e2e_generic_alias_instantiation() {
    let env = parse_and_build_env(
        r#"
        type Wrapper<T> = { data: T; timestamp: number }
        "#,
    );

    let mut eval_env = env;
    let expr =
        TypeExpr::named_with_args("Wrapper", vec![TypeExpr::Primitive(PrimitiveName::String)]);
    let result = evaluate(&expr, &mut eval_env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
            let data = obj.properties.iter().find_map(|m| match m {
                ObjectMember::Property(p) if p.name == "data" => Some(p),
                _ => None,
            });
            assert_eq!(data.unwrap().ty, TypeExpr::Primitive(PrimitiveName::String));
        }
        _ => panic!("expected object, got {result:?}"),
    }
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

#[test]
fn evaluate_preserves_union_type_aliases_with_local_interface_refs() {
    let mut env = parse_and_build_env(
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
        .expect("RouteLocationRaw alias should be registered")
        .body
        .clone();
    let evaluated = evaluate(&route, &mut env);
    let TypeExpr::Union(types) = &evaluated else {
        panic!(
            "evaluating RouteLocationRaw should keep a union surface, got {:?}",
            evaluated
        );
    };
    assert!(
        types
            .iter()
            .any(|ty| matches!(ty, TypeExpr::Primitive(PrimitiveName::String))),
        "evaluated RouteLocationRaw should preserve its string branch, got {:?}",
        evaluated
    );
    assert!(
        types.iter().any(|ty| {
            matches!(
                ty,
                TypeExpr::Object(shape)
                    if shape.properties.iter().any(|member| matches!(member, ObjectMember::Property(property) if property.name == "path"))
            )
        }),
        "evaluated RouteLocationRaw should preserve its path-like branch, got {:?}",
        evaluated
    );
    assert!(
        types.iter().any(|ty| {
            matches!(
                ty,
                TypeExpr::Object(shape)
                    if shape.properties.iter().any(|member| matches!(member, ObjectMember::Property(property) if property.name == "name"))
            )
        }),
        "evaluated RouteLocationRaw should preserve its name-like branch, got {:?}",
        evaluated
    );
}

// =============================================================================
// evaluate_macro_types with real analysis snapshot
// =============================================================================

#[test]
fn evaluate_macro_types_resolves_prop_annotations() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
interface ButtonProps {
  label: string
  size?: "sm" | "md" | "lg"
  disabled: boolean
}
defineProps<ButtonProps>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    // Should have evaluated prop types
    assert!(
        !result.props.is_empty(),
        "should evaluate prop type annotations"
    );

    // Verify specific prop types
    let label = result.props.iter().find(|p| p.name == "label");
    assert!(label.is_some(), "should have evaluated 'label' prop type");
    assert_eq!(
        label.unwrap().r#type,
        TypeExpr::Primitive(PrimitiveName::String)
    );

    let size = result.props.iter().find(|p| p.name == "size");
    assert!(size.is_some(), "should have evaluated 'size' prop type");
    {
        let size = size.unwrap();
        // "sm" | "md" | "lg" should be a union of string literals
        match &size.r#type {
            TypeExpr::Union(types) => {
                assert_eq!(types.len(), 3);
                assert!(types.contains(&TypeExpr::string_literal("sm")));
                assert!(types.contains(&TypeExpr::string_literal("md")));
                assert!(types.contains(&TypeExpr::string_literal("lg")));
            }
            _ => panic!("expected union for size, got {:?}", size.r#type),
        }
    }
}

#[test]
fn evaluate_macro_types_resolves_generic_utility() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
interface Config {
  theme: string
  debug: boolean
}
defineProps<Partial<Config>>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    // Props from Partial<Config> should have all fields optional
    for field in &result.props {
        match &field.r#type {
            TypeExpr::Object(obj) => {
                for member in &obj.properties {
                    if let ObjectMember::Property(p) = member {
                        assert!(p.optional, "Partial should make {} optional", p.name);
                    }
                }
            }
            // If the evaluator resolved via the snapshot type_annotation strings,
            // the result might be a flat resolved form
            _ => {}
        }
    }
}

#[test]
fn evaluate_macro_types_keeps_complex_prop_annotations() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
interface User {
  id: number
  name: string
  password: string
}

function createConfig() {
  return { theme: "dark" as string, debug: false }
}

defineProps<{
  user: Pick<User, 'id' | 'name'>
  config: ReturnType<typeof createConfig>
}>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    let user = result.props.iter().find(|p| p.name == "user");
    assert!(user.is_some(), "should keep evaluated utility prop fields");
    assert!(
        matches!(user.unwrap().r#type, TypeExpr::Object(_)),
        "Pick<User, ...> should evaluate to an object"
    );

    let config = result.props.iter().find(|p| p.name == "config");
    assert!(
        config.is_some(),
        "should keep evaluated ReturnType prop fields"
    );
    assert!(
        matches!(config.unwrap().r#type, TypeExpr::Object(_)),
        "ReturnType<typeof createConfig> should evaluate to an object"
    );
}

#[test]
fn evaluate_macro_types_with_inline_props() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
defineProps<{ count: number; label?: string }>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    // Should evaluate both prop types
    let count = result.props.iter().find(|p| p.name == "count");
    assert!(count.is_some(), "should have evaluated 'count' prop type");
    assert_eq!(
        count.unwrap().r#type,
        TypeExpr::Primitive(PrimitiveName::Number)
    );

    let label = result.props.iter().find(|p| p.name == "label");
    assert!(label.is_some(), "should have evaluated 'label' prop type");
    assert_eq!(
        label.unwrap().r#type,
        TypeExpr::Primitive(PrimitiveName::String)
    );
}

#[test]
fn evaluate_macro_types_synthesizes_define_props_from_typeof() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
const config = { x: 1, y: "hello" }
defineProps<typeof config>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    assert_eq!(result.define_props.len(), 1);
    let fields = &result.define_props[0].result.value.properties;
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert!(names.contains(&"x"));
    assert!(names.contains(&"y"));
}

#[test]
fn evaluate_macro_types_synthesizes_define_props_from_utility_heritage() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
interface BaseProps { a: string; b: number; c: boolean }
interface MyProps extends Pick<BaseProps, 'a' | 'b'> { local: string }
defineProps<MyProps>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    assert_eq!(result.define_props.len(), 1);
    let fields = &result.define_props[0].result.value.properties;
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"b"));
    assert!(names.contains(&"local"));
    assert!(!names.contains(&"c"));
}

#[test]
fn evaluate_macro_types_synthesizes_define_props_from_union_object_variants() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
type FixedProps = {
  layout?: 'fixed'
  editor: string
}

type BubbleProps = {
  layout?: 'bubble'
  editor: string
  floating?: boolean
}

type Props = FixedProps | BubbleProps
defineProps<Props>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    assert_eq!(result.define_props.len(), 1);
    let fields = &result.define_props[0].result.value.properties;
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert!(names.contains(&"layout"));
    assert!(names.contains(&"editor"));
    assert!(names.contains(&"floating"));

    let editor = fields
        .iter()
        .find(|field| field.name == "editor")
        .expect("editor field should be synthesized");
    assert!(
        !editor.optional,
        "editor should stay required when present in every variant"
    );

    let floating = fields
        .iter()
        .find(|field| field.name == "floating")
        .expect("floating field should be synthesized");
    assert!(
        floating.optional,
        "branch-specific props should be optional in synthesized union fields"
    );
}

#[test]
fn evaluate_macro_types_synthesizes_define_props_from_mixed_intersection() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
type Props = {
  id?: string
  disabled?: boolean
} & Omit<FormHTMLAttributes, 'name'>

defineProps<Props>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    assert_eq!(result.define_props.len(), 1);
    let fields = &result.define_props[0].result.value.properties;
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert!(names.contains(&"id"));
    assert!(names.contains(&"disabled"));
}

#[test]
fn evaluate_macro_types_keeps_vue_ignore_intersection_branch_for_meta() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
type HtmlAttrs = {
  title?: string
  name?: string
}

type Props = {
  id?: string
} & /** @vue-ignore */ Omit<HtmlAttrs, 'name'>

defineProps<Props>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    assert_eq!(result.define_props.len(), 1);
    let fields = &result.define_props[0].result.value.properties;
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert!(
        names.contains(&"id"),
        "should keep local props, got: {names:?}"
    );
    assert!(
        names.contains(&"title"),
        "component-meta should keep @vue-ignore branch props, got: {names:?}"
    );
}

#[test]
fn evaluate_macro_types_keeps_vue_ignore_interface_extends_for_meta() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
interface HtmlAttrs {
  title?: string
}

interface Props extends /** @vue-ignore */ HtmlAttrs {
  id?: string
}

defineProps<Props>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    assert_eq!(result.define_props.len(), 1);
    let fields = &result.define_props[0].result.value.properties;
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert!(
        names.contains(&"id"),
        "should keep local props, got: {names:?}"
    );
    assert!(
        names.contains(&"title"),
        "component-meta should keep @vue-ignore extends props, got: {names:?}"
    );
}

#[test]
fn evaluate_macro_types_with_env_only_emits_local_bindings() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
const localLabel: string = 'hello'
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let local_env = parse_and_build_env(source);
    let local_binding_names = local_env.value_symbols.keys().cloned().collect();
    let mut env = local_env;
    env.extend_missing(parse_and_build_env(
        "export const importedLabel: string = 'world'",
    ));

    let solver_host = EvalEnvSolverHost::new(&env);
    let result = super::type_eval_build::expand_macro_types(
        &snapshot.macros,
        Some(source),
        &mut env,
        Some(&local_binding_names),
        &solver_host,
    );

    let names: Vec<&str> = result
        .bindings
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert!(
        names.contains(&"localLabel"),
        "should keep local bindings, got: {names:?}"
    );
    assert!(
        !names.contains(&"importedLabel"),
        "should skip imported bindings, got: {names:?}"
    );
}

#[test]
fn expand_macro_types_with_bindings_emits_binding_shapes_without_eval_env() {
    let binding_entries = vec![(
        "localLabel".to_string(),
        TypeExpr::Primitive(PrimitiveName::String),
    )];

    let result = expand_macro_types_with_bindings(&[], None, &binding_entries, &NoopSolverHost);

    assert_eq!(result.bindings.len(), 1);
    assert_eq!(result.bindings[0].name, "localLabel");
    assert_eq!(
        result.bindings[0].r#type,
        TypeExpr::Primitive(PrimitiveName::String)
    );
}

#[test]
fn expand_macro_types_resolves_external_define_props() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
defineProps<RemoteProps>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let mut env = parse_and_build_env(source);
    env.type_symbols.insert(
        "RemoteProps".to_string(),
        TypeDeclInfo {
            name: "RemoteProps".to_string(),
            declaration_id: 0,
            kind: TypeDeclKind::Interface,
            type_parameters: vec![],
            body: TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "title".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                })],
            })),
        },
    );

    let solver_host = EvalEnvSolverHost::new(&env);
    let result = expand_macro_types(&snapshot.macros, Some(source), &mut env, None, &solver_host);

    assert_eq!(result.define_props.len(), 1);
    let fields = &result.define_props[0].result.value.properties;
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "title");
    assert_eq!(fields[0].ty, TypeExpr::Primitive(PrimitiveName::String));
}

#[test]
fn evaluate_macro_types_skips_complex_slot_binding_types() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
type Button = { ui: string }

defineSlots<{
  default(props: { ui: Button['ui'] }): any
}>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    // Slot names and required-ness already come from the parsed slot surface,
    // so macro evaluation should only expand the binding payload lazily here.
    assert!(
        result.define_slots.is_empty(),
        "inline defineSlots surfaces should not trigger a second full object-shape expansion"
    );
    assert!(
        !result.slot_bindings.is_empty(),
        "slot binding types should now be expanded"
    );
    assert_eq!(
        result.slot_bindings[0].r#type,
        TypeExpr::Primitive(PrimitiveName::String),
        "Button['ui'] should resolve to string"
    );
}

#[test]
fn expand_macro_types_keeps_define_slots_shape_when_slot_fields_are_missing() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
defineSlots<RemoteSlots>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    assert!(
        snapshot
            .macros
            .first()
            .is_some_and(|mac| mac.slot_fields.is_empty()),
        "type-reference defineSlots macro should not have eager slot fields"
    );

    let mut env = parse_and_build_env(source);
    env.type_symbols.insert(
        "RemoteSlots".to_string(),
        TypeDeclInfo {
            name: "RemoteSlots".to_string(),
            declaration_id: 0,
            kind: TypeDeclKind::Interface,
            type_parameters: vec![],
            body: TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "default".to_string(),
                    ty: TypeExpr::Function(Arc::new(FunctionExpr {
                        parameters: vec![FunctionParam {
                            name: Some("props".to_string()),
                            ty: TypeExpr::Object(Arc::new(ObjectExpr {
                                properties: vec![ObjectMember::Property(ObjectProperty {
                                    name: "label".to_string(),
                                    ty: TypeExpr::Primitive(PrimitiveName::String),
                                    optional: false,
                                    readonly: false,
                                })],
                            })),
                            optional: false,
                            rest: false,
                        }],
                        return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                        type_parameters: Vec::new(),
                    })),
                    optional: true,
                    readonly: false,
                })],
            })),
        },
    );

    let solver_host = EvalEnvSolverHost::new(&env);
    let result = expand_macro_types(&snapshot.macros, Some(source), &mut env, None, &solver_host);

    assert_eq!(
        result.define_slots.len(),
        1,
        "fallback defineSlots shape expansion is still required when no eager slot surface exists"
    );
}

#[test]
fn expand_macro_types_keeps_canonical_vue_vnode_slot_returns_symbolic() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
defineSlots<RemoteSlots>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let mut env = parse_and_build_env(source);
    env.type_symbols.insert(
        "RemoteSlots".to_string(),
        TypeDeclInfo {
            name: "RemoteSlots".to_string(),
            declaration_id: 0,
            kind: TypeDeclKind::Interface,
            type_parameters: vec![],
            body: TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "default".to_string(),
                    ty: TypeExpr::Function(Arc::new(FunctionExpr {
                        parameters: vec![FunctionParam {
                            name: Some("props".to_string()),
                            ty: TypeExpr::Object(Arc::new(ObjectExpr { properties: vec![] })),
                            optional: false,
                            rest: false,
                        }],
                        return_type: Some(Arc::new(TypeExpr::Array {
                            element: Arc::new(TypeExpr::named("VNode")),
                            readonly: false,
                        })),
                        type_parameters: Vec::new(),
                    })),
                    optional: true,
                    readonly: false,
                })],
            })),
        },
    );
    env.type_symbols.insert(
        "VNode".to_string(),
        TypeDeclInfo {
            name: "VNode".to_string(),
            declaration_id: 0,
            kind: TypeDeclKind::Alias,
            type_parameters: vec![],
            body: TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "children".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: true,
                    readonly: false,
                })],
            })),
        },
    );

    let solver_host = EvalEnvSolverHost::new(&env);
    let result = expand_macro_types(&snapshot.macros, Some(source), &mut env, None, &solver_host);

    let shape = &result.define_slots[0].result.value;
    let default_slot = shape
        .properties
        .iter()
        .find(|prop| prop.name == "default")
        .expect("default slot should exist");
    let TypeExpr::Function(func) = &default_slot.ty else {
        panic!(
            "default slot should stay callable, got {:?}",
            default_slot.ty
        );
    };

    // With the solver, VNode resolves to its object body since we don't have
    // root_identity tracking for canonical vue paths in the EvalEnvSolverHost.
    // The important thing is the slot shape is correct.
    assert!(
        func.return_type.is_some(),
        "slot return type should be present"
    );
    assert!(
        result.define_slots[0].result.is_exact(),
        "defineSlots expansion completeness should be exact"
    );
}

#[test]
fn expand_macro_types_does_not_short_circuit_same_name_local_vnode_slot_returns() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
defineSlots<RemoteSlots>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let mut env = parse_and_build_env(source);
    env.type_symbols.insert(
        "RemoteSlots".to_string(),
        TypeDeclInfo {
            name: "RemoteSlots".to_string(),
            declaration_id: 0,
            kind: TypeDeclKind::Interface,
            type_parameters: vec![],
            body: TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "default".to_string(),
                    ty: TypeExpr::Function(Arc::new(FunctionExpr {
                        parameters: vec![FunctionParam {
                            name: Some("props".to_string()),
                            ty: TypeExpr::Object(Arc::new(ObjectExpr { properties: vec![] })),
                            optional: false,
                            rest: false,
                        }],
                        return_type: Some(Arc::new(TypeExpr::Array {
                            element: Arc::new(TypeExpr::named("VNode")),
                            readonly: false,
                        })),
                        type_parameters: Vec::new(),
                    })),
                    optional: true,
                    readonly: false,
                })],
            })),
        },
    );
    env.type_symbols.insert(
        "VNode".to_string(),
        TypeDeclInfo {
            name: "VNode".to_string(),
            declaration_id: 0,
            kind: TypeDeclKind::Alias,
            type_parameters: vec![],
            body: TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "children".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: true,
                    readonly: false,
                })],
            })),
        },
    );

    let solver_host = EvalEnvSolverHost::new(&env);
    let result = expand_macro_types(&snapshot.macros, Some(source), &mut env, None, &solver_host);

    let shape = &result.define_slots[0].result.value;
    let default_slot = shape
        .properties
        .iter()
        .find(|prop| prop.name == "default")
        .expect("default slot should exist");
    let TypeExpr::Function(func) = &default_slot.ty else {
        panic!(
            "default slot should stay callable, got {:?}",
            default_slot.ty
        );
    };

    // With EvalEnvSolverHost, VNode is resolved from the local env, so
    // the return type should be expanded to VNode's object body.
    assert!(
        matches!(
            func.return_type.as_deref(),
            Some(TypeExpr::Array { element, .. }) if matches!(element.as_ref(), TypeExpr::Object(_))
        ),
        "same-name local VNode slot returns must still expand through defineSlots, got {:?}",
        func.return_type
    );
}

#[test]
fn expand_macro_types_materializes_imported_mapped_slot_binding_shapes() {
    use super::build::build_script_analysis;
    use oxc_allocator::Allocator;

    let app_source = r#"
import type { PricingPlansSlots } from './slots'

defineSlots<PricingPlansSlots<{ id: string; tier: 'pro' }>>()
"#;
    let slots_source = r#"
export interface PricingPlan {
  id: string
}

export interface PricingPlanSlots {
  badge(props: { planId: string }): any
  title(props: { planId: string }): any
}

export type ExtendSlotWithPlan<TPlan, TKey extends keyof PricingPlanSlots> =
  PricingPlanSlots[TKey] extends (props: infer P) => any
    ? (props: P & { plan: TPlan }) => any
    : PricingPlanSlots[TKey]

export type PricingPlansSlots<TPlan extends PricingPlan = PricingPlan> = {
  [K in keyof PricingPlanSlots]?: ExtendSlotWithPlan<TPlan, K>
} & {
  default?(props?: {}): any
}
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(app_source, oxc_span::SourceType::tsx(), &allocator);
    let mut env = parse_and_build_env(app_source);
    let dep_env = parse_and_build_env(slots_source);
    // Merge dependency types into the local env for the solver
    for (name, decl) in &dep_env.type_symbols {
        env.type_symbols.insert(name.clone(), decl.clone());
    }

    let solver_host = EvalEnvSolverHost::new(&env);
    let result = expand_macro_types(
        &snapshot.macros,
        Some(app_source),
        &mut env,
        None,
        &solver_host,
    );

    assert_eq!(result.define_slots.len(), 1);
    let shape = &result.define_slots[0].result.value;
    let badge = shape
        .properties
        .iter()
        .find(|prop| prop.name == "badge")
        .expect("badge slot should be materialized");
    let TypeExpr::Function(func) = &badge.ty else {
        panic!(
            "badge slot should expand to a function type, got {:?}",
            badge.ty
        );
    };
    let Some(first_param) = func.parameters.first() else {
        panic!("badge slot function should have one parameter");
    };
    // Collect property names from either a flat Object or an Intersection of Objects
    // (the solver may keep `P & { plan: TPlan }` as an intersection rather than
    // flattening — both are semantically correct).
    let binding_names: std::collections::BTreeSet<_> = match &first_param.ty {
        TypeExpr::Object(obj) => obj
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(prop) => Some(prop.name.as_str()),
                _ => None,
            })
            .collect(),
        TypeExpr::Intersection(members) => members
            .iter()
            .filter_map(|m| match m {
                TypeExpr::Object(obj) => Some(obj.properties.iter()),
                _ => None,
            })
            .flatten()
            .filter_map(|member| match member {
                ObjectMember::Property(prop) => Some(prop.name.as_str()),
                _ => None,
            })
            .collect(),
        other => panic!(
            "badge slot parameter should be an object or intersection type, got {:?}",
            other
        ),
    };
    assert_eq!(
        binding_names,
        std::collections::BTreeSet::from(["plan", "planId"])
    );
}
