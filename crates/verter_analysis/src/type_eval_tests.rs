use super::type_eval::*;
use super::type_expr::*;
use super::type_expr_lower::parse_type_annotation;

fn env_with_user_type() -> EvalEnv {
    let mut env = EvalEnv::new();
    // interface User { id: number; name: string; email: string; password: string }
    env.add_type(TypeDeclInfo {
        name: "User".to_string(),
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "id".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "name".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "email".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "password".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    });
    env
}

// =============================================================================
// Primitives and literals pass through
// =============================================================================

#[test]
fn eval_primitive_passthrough() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::Primitive(PrimitiveName::String);
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::String));
}

#[test]
fn eval_literal_passthrough() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::string_literal("hello");
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::string_literal("hello"));
}

// =============================================================================
// Type reference resolution
// =============================================================================

#[test]
fn eval_ref_resolves_interface() {
    let mut env = env_with_user_type();
    let expr = TypeExpr::named("User");
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 4);
            assert!(!result.is_unknown());
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

#[test]
fn eval_ref_unresolved_stays_ref() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::named("UnknownType");
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::named("UnknownType"));
}

#[test]
fn eval_ref_cycle_detection() {
    let mut env = EvalEnv::new();
    // type A = A (self-referential)
    env.add_type(TypeDeclInfo {
        name: "A".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::named("A"),
    });
    let result = evaluate(&TypeExpr::named("A"), &mut env);
    // Should not stack overflow — cycle detection kicks in
    assert_eq!(result, TypeExpr::named("A"));
}

// =============================================================================
// Generic instantiation
// =============================================================================

#[test]
fn eval_generic_alias() {
    let mut env = EvalEnv::new();
    // type Box<T> = { value: T }
    env.add_type(TypeDeclInfo {
        name: "Box".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "value".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        }),
    });

    let expr = TypeExpr::named_with_args("Box", vec![TypeExpr::Primitive(PrimitiveName::String)]);
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 1);
            if let ObjectMember::Property(p) = &obj.properties[0] {
                assert_eq!(p.name, "value");
                assert_eq!(p.ty, TypeExpr::Primitive(PrimitiveName::String));
            }
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

#[test]
fn eval_generic_with_default() {
    let mut env = EvalEnv::new();
    // type Wrapper<T = number> = { data: T }
    env.add_type(TypeDeclInfo {
        name: "Wrapper".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: Some(Box::new(TypeExpr::Primitive(PrimitiveName::Number))),
        }],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "data".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        }),
    });

    // Wrapper (no args → uses default)
    let result = evaluate(&TypeExpr::named("Wrapper"), &mut env);
    match &result {
        TypeExpr::Object(obj) => {
            if let ObjectMember::Property(p) = &obj.properties[0] {
                assert_eq!(p.ty, TypeExpr::Primitive(PrimitiveName::Number));
            }
        }
        _ => panic!("expected object, got {result:?}"),
    }

    // Wrapper<string> should override the default
    let expr3 =
        TypeExpr::named_with_args("Wrapper", vec![TypeExpr::Primitive(PrimitiveName::String)]);
    let result3 = evaluate(&expr3, &mut env);
    match &result3 {
        TypeExpr::Object(obj) => {
            if let ObjectMember::Property(p) = &obj.properties[0] {
                assert_eq!(p.ty, TypeExpr::Primitive(PrimitiveName::String));
            }
        }
        _ => panic!("expected object, got {result3:?}"),
    }
}

// =============================================================================
// Partial<T>
// =============================================================================

#[test]
fn eval_partial() {
    let mut env = env_with_user_type();
    let expr = parse_type_annotation("Partial<User>");
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 4);
            for member in &obj.properties {
                if let ObjectMember::Property(p) = member {
                    assert!(
                        p.optional,
                        "all properties should be optional, but {} is not",
                        p.name
                    );
                }
            }
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

// =============================================================================
// Required<T>
// =============================================================================

#[test]
fn eval_required() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "Config".to_string(),
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "theme".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: true,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "debug".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                    optional: true,
                    readonly: false,
                }),
            ],
        }),
    });

    let expr = parse_type_annotation("Required<Config>");
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                if let ObjectMember::Property(p) = member {
                    assert!(!p.optional, "{} should not be optional", p.name);
                }
            }
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

// =============================================================================
// Readonly<T>
// =============================================================================

#[test]
fn eval_readonly() {
    let mut env = env_with_user_type();
    let expr = parse_type_annotation("Readonly<User>");
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                if let ObjectMember::Property(p) = member {
                    assert!(p.readonly, "{} should be readonly", p.name);
                }
            }
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

// =============================================================================
// Pick<T, K>
// =============================================================================

#[test]
fn eval_pick() {
    let mut env = env_with_user_type();
    let expr = parse_type_annotation("Pick<User, \"id\" | \"name\">");
    let result = evaluate(&expr, &mut env);
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
            assert!(names.contains(&"id"), "should contain id");
            assert!(names.contains(&"name"), "should contain name");
            assert!(!names.contains(&"email"), "should NOT contain email");
            assert!(!names.contains(&"password"), "should NOT contain password");
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

// =============================================================================
// Omit<T, K>
// =============================================================================

#[test]
fn eval_omit() {
    let mut env = env_with_user_type();
    let expr = parse_type_annotation("Omit<User, \"password\">");
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 3);
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
            assert!(names.contains(&"email"));
            assert!(!names.contains(&"password"), "password should be omitted");
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

// =============================================================================
// Record<K, V>
// =============================================================================

#[test]
fn eval_record_literal_keys() {
    let mut env = EvalEnv::new();
    let expr = parse_type_annotation("Record<\"a\" | \"b\", number>");
    let result = evaluate(&expr, &mut env);
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
            assert!(names.contains(&"a"));
            assert!(names.contains(&"b"));
            // All values should be number
            for member in &obj.properties {
                if let ObjectMember::Property(p) = member {
                    assert_eq!(p.ty, TypeExpr::Primitive(PrimitiveName::Number));
                }
            }
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

#[test]
fn eval_record_string_index() {
    let mut env = EvalEnv::new();
    let expr = parse_type_annotation("Record<string, number>");
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 1);
            match &obj.properties[0] {
                ObjectMember::IndexSignature(idx) => {
                    assert_eq!(idx.key_type, TypeExpr::Primitive(PrimitiveName::String));
                    assert_eq!(idx.value_type, TypeExpr::Primitive(PrimitiveName::Number));
                }
                _ => panic!("expected index signature"),
            }
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

// =============================================================================
// Extract<T, U> / Exclude<T, U>
// =============================================================================

#[test]
fn eval_extract() {
    let mut env = EvalEnv::new();
    // Extract<"a" | "b" | "c", "a" | "b"> → "a" | "b"
    let source = TypeExpr::Union(vec![
        TypeExpr::string_literal("a"),
        TypeExpr::string_literal("b"),
        TypeExpr::string_literal("c"),
    ]);
    let target = TypeExpr::Union(vec![
        TypeExpr::string_literal("a"),
        TypeExpr::string_literal("b"),
    ]);
    let expr = TypeExpr::named_with_args("Extract", vec![source, target]);
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Union(types) => {
            assert_eq!(types.len(), 2);
            assert!(types.contains(&TypeExpr::string_literal("a")));
            assert!(types.contains(&TypeExpr::string_literal("b")));
            assert!(!types.contains(&TypeExpr::string_literal("c")));
        }
        _ => panic!("expected union, got {result:?}"),
    }
}

#[test]
fn eval_exclude() {
    let mut env = EvalEnv::new();
    // Exclude<"a" | "b" | "c", "a"> → "b" | "c"
    let source = TypeExpr::Union(vec![
        TypeExpr::string_literal("a"),
        TypeExpr::string_literal("b"),
        TypeExpr::string_literal("c"),
    ]);
    let target = TypeExpr::string_literal("a");
    let expr = TypeExpr::named_with_args("Exclude", vec![source, target]);
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Union(types) => {
            assert_eq!(types.len(), 2);
            assert!(
                !types.contains(&TypeExpr::string_literal("a")),
                "a should be excluded"
            );
            assert!(types.contains(&TypeExpr::string_literal("b")));
            assert!(types.contains(&TypeExpr::string_literal("c")));
        }
        _ => panic!("expected union, got {result:?}"),
    }
}

// =============================================================================
// NonNullable<T>
// =============================================================================

#[test]
fn eval_non_nullable() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::named_with_args(
        "NonNullable",
        vec![TypeExpr::Union(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Null),
            TypeExpr::Primitive(PrimitiveName::Undefined),
        ])],
    );
    let result = evaluate(&expr, &mut env);
    // Should be just string
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::String));
}

// =============================================================================
// keyof
// =============================================================================

#[test]
fn eval_keyof_object() {
    let mut env = env_with_user_type();
    let expr = TypeExpr::KeyOf(Box::new(TypeExpr::named("User")));
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Union(types) => {
            assert_eq!(types.len(), 4);
            assert!(types.contains(&TypeExpr::string_literal("id")));
            assert!(types.contains(&TypeExpr::string_literal("name")));
            assert!(types.contains(&TypeExpr::string_literal("email")));
            assert!(types.contains(&TypeExpr::string_literal("password")));
        }
        _ => panic!("expected union, got {result:?}"),
    }
}

#[test]
fn eval_keyof_inline_object() {
    let mut env = EvalEnv::new();
    let obj = TypeExpr::Object(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            }),
            ObjectMember::Property(ObjectProperty {
                name: "b".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: true,
                readonly: false,
            }),
        ],
    });
    let expr = TypeExpr::KeyOf(Box::new(obj));
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Union(types) => {
            assert_eq!(types.len(), 2);
            assert!(types.contains(&TypeExpr::string_literal("a")));
            assert!(types.contains(&TypeExpr::string_literal("b")));
        }
        _ => panic!("expected union, got {result:?}"),
    }
}

// =============================================================================
// typeof
// =============================================================================

#[test]
fn eval_typeof_function() {
    let mut env = EvalEnv::new();
    env.add_value(ValueDeclInfo {
        name: "createConfig".to_string(),
        kind: ValueDeclKind::Function,
        type_annotation: None,
        function_signature: Some(FunctionSignature {
            parameters: vec![],
            return_type: Some(TypeExpr::Object(ObjectExpr {
                properties: vec![
                    ObjectMember::Property(ObjectProperty {
                        name: "theme".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    }),
                    ObjectMember::Property(ObjectProperty {
                        name: "debug".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                        optional: false,
                        readonly: false,
                    }),
                ],
            })),
            type_parameters: vec![],
        }),
        object_shape: None,
    });

    let expr = TypeExpr::TypeOf(ValueRef {
        path: vec!["createConfig".to_string()],
    });
    let result = evaluate(&expr, &mut env);
    assert!(matches!(&result, TypeExpr::Function(_)));
}

#[test]
fn eval_typeof_const_object() {
    let mut env = EvalEnv::new();
    env.add_value(ValueDeclInfo {
        name: "defaults".to_string(),
        kind: ValueDeclKind::Const,
        type_annotation: None,
        function_signature: None,
        object_shape: Some(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "size".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                readonly: false,
            })],
        }),
    });

    let expr = TypeExpr::TypeOf(ValueRef {
        path: vec!["defaults".to_string()],
    });
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 1);
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

// =============================================================================
// ReturnType<typeof fn>
// =============================================================================

#[test]
fn eval_return_type() {
    let mut env = EvalEnv::new();
    env.add_value(ValueDeclInfo {
        name: "createConfig".to_string(),
        kind: ValueDeclKind::Function,
        type_annotation: None,
        function_signature: Some(FunctionSignature {
            parameters: vec![],
            return_type: Some(TypeExpr::Object(ObjectExpr {
                properties: vec![
                    ObjectMember::Property(ObjectProperty {
                        name: "theme".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    }),
                    ObjectMember::Property(ObjectProperty {
                        name: "debug".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                        optional: false,
                        readonly: false,
                    }),
                ],
            })),
            type_parameters: vec![],
        }),
        object_shape: None,
    });

    let expr = TypeExpr::named_with_args(
        "ReturnType",
        vec![TypeExpr::TypeOf(ValueRef {
            path: vec!["createConfig".to_string()],
        })],
    );
    let result = evaluate(&expr, &mut env);
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
        _ => panic!("expected object with theme and debug, got {result:?}"),
    }
}

// =============================================================================
// Parameters<typeof fn>
// =============================================================================

#[test]
fn eval_parameters() {
    let mut env = EvalEnv::new();
    env.add_value(ValueDeclInfo {
        name: "greet".to_string(),
        kind: ValueDeclKind::Function,
        type_annotation: None,
        function_signature: Some(FunctionSignature {
            parameters: vec![
                FunctionParam {
                    name: Some("name".to_string()),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    rest: false,
                },
                FunctionParam {
                    name: Some("age".to_string()),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: true,
                    rest: false,
                },
            ],
            return_type: Some(TypeExpr::Primitive(PrimitiveName::Void)),
            type_parameters: vec![],
        }),
        object_shape: None,
    });

    let expr = TypeExpr::named_with_args(
        "Parameters",
        vec![TypeExpr::TypeOf(ValueRef {
            path: vec!["greet".to_string()],
        })],
    );
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Tuple { elements, .. } => {
            assert_eq!(elements.len(), 2);
            assert_eq!(elements[0].label.as_deref(), Some("name"));
            assert_eq!(elements[0].ty, TypeExpr::Primitive(PrimitiveName::String));
            assert!(!elements[0].optional);
            assert_eq!(elements[1].label.as_deref(), Some("age"));
            assert!(elements[1].optional);
        }
        _ => panic!("expected tuple, got {result:?}"),
    }
}

// =============================================================================
// Constructor utilities
// =============================================================================

#[test]
fn eval_constructor_parameters_from_class_typeof() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "Widget".to_string(),
        kind: TypeDeclKind::Class,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "id".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                readonly: false,
            })],
        }),
    });
    env.add_value(ValueDeclInfo {
        name: "Widget".to_string(),
        kind: ValueDeclKind::Class,
        type_annotation: None,
        function_signature: Some(FunctionSignature {
            parameters: vec![FunctionParam {
                name: Some("id".to_string()),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                rest: false,
            }],
            return_type: Some(TypeExpr::named("Widget")),
            type_parameters: vec![],
        }),
        object_shape: Some(ObjectExpr {
            properties: vec![ObjectMember::ConstructSignature(FunctionExpr {
                parameters: vec![FunctionParam {
                    name: Some("id".to_string()),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    rest: false,
                }],
                return_type: Some(Box::new(TypeExpr::named("Widget"))),
                type_parameters: vec![],
            })],
        }),
    });

    let expr = TypeExpr::named_with_args(
        "ConstructorParameters",
        vec![TypeExpr::TypeOf(ValueRef {
            path: vec!["Widget".to_string()],
        })],
    );
    let result = evaluate(&expr, &mut env);

    match result {
        TypeExpr::Tuple { elements, .. } => {
            assert_eq!(elements.len(), 1);
            assert_eq!(elements[0].label.as_deref(), Some("id"));
            assert_eq!(elements[0].ty, TypeExpr::Primitive(PrimitiveName::Number));
        }
        other => panic!("expected constructor parameter tuple, got {other:?}"),
    }
}

#[test]
fn eval_instance_type_from_class_typeof() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "Widget".to_string(),
        kind: TypeDeclKind::Class,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "id".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "label".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    });
    env.add_value(ValueDeclInfo {
        name: "Widget".to_string(),
        kind: ValueDeclKind::Class,
        type_annotation: None,
        function_signature: Some(FunctionSignature {
            parameters: vec![],
            return_type: Some(TypeExpr::named("Widget")),
            type_parameters: vec![],
        }),
        object_shape: Some(ObjectExpr {
            properties: vec![ObjectMember::ConstructSignature(FunctionExpr {
                parameters: vec![],
                return_type: Some(Box::new(TypeExpr::named("Widget"))),
                type_parameters: vec![],
            })],
        }),
    });

    let expr = TypeExpr::named_with_args(
        "InstanceType",
        vec![TypeExpr::TypeOf(ValueRef {
            path: vec!["Widget".to_string()],
        })],
    );
    let result = evaluate(&expr, &mut env);

    match result {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"id"));
            assert!(names.contains(&"label"));
        }
        other => panic!("expected instance object type, got {other:?}"),
    }
}

// =============================================================================
// Awaited<T>
// =============================================================================

#[test]
fn eval_awaited_promise() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::named_with_args(
        "Awaited",
        vec![TypeExpr::named_with_args(
            "Promise",
            vec![TypeExpr::Primitive(PrimitiveName::String)],
        )],
    );
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::String));
}

#[test]
fn eval_awaited_nested_promise() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::named_with_args(
        "Awaited",
        vec![TypeExpr::named_with_args(
            "Promise",
            vec![TypeExpr::named_with_args(
                "Promise",
                vec![TypeExpr::Primitive(PrimitiveName::Number)],
            )],
        )],
    );
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::Number));
}

#[test]
fn eval_awaited_non_promise() {
    let mut env = EvalEnv::new();
    let expr =
        TypeExpr::named_with_args("Awaited", vec![TypeExpr::Primitive(PrimitiveName::String)]);
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::String));
}

// =============================================================================
// Indexed access
// =============================================================================

#[test]
fn eval_indexed_access_property() {
    let mut env = env_with_user_type();
    let expr = TypeExpr::IndexedAccess {
        object: Box::new(TypeExpr::named("User")),
        index: Box::new(TypeExpr::string_literal("name")),
    };
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::String));
}

#[test]
fn eval_indexed_access_array_number() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::IndexedAccess {
        object: Box::new(TypeExpr::Array {
            element: Box::new(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: false,
        }),
        index: Box::new(TypeExpr::Primitive(PrimitiveName::Number)),
    };
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::String));
}

#[test]
fn eval_indexed_access_tuple_literal() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::IndexedAccess {
        object: Box::new(TypeExpr::Tuple {
            elements: vec![
                TupleElement {
                    label: None,
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    rest: false,
                },
                TupleElement {
                    label: None,
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    rest: false,
                },
            ],
            readonly: false,
        }),
        index: Box::new(TypeExpr::number_literal(1.0)),
    };
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::Number));
}

#[test]
fn eval_indexed_access_union_keys() {
    let mut env = env_with_user_type();
    // User["id" | "name"] → number | string
    let expr = TypeExpr::IndexedAccess {
        object: Box::new(TypeExpr::named("User")),
        index: Box::new(TypeExpr::Union(vec![
            TypeExpr::string_literal("id"),
            TypeExpr::string_literal("name"),
        ])),
    };
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Union(types) => {
            assert_eq!(types.len(), 2);
            assert!(types.contains(&TypeExpr::Primitive(PrimitiveName::Number)));
            assert!(types.contains(&TypeExpr::Primitive(PrimitiveName::String)));
        }
        _ => panic!("expected union, got {result:?}"),
    }
}

// =============================================================================
// Conditional types
// =============================================================================

#[test]
fn eval_conditional_true_branch() {
    let mut env = EvalEnv::new();
    // "hello" extends string ? true : false → true
    let expr = TypeExpr::Conditional {
        check: Box::new(TypeExpr::string_literal("hello")),
        extends: Box::new(TypeExpr::Primitive(PrimitiveName::String)),
        true_type: Box::new(TypeExpr::boolean_literal(true)),
        false_type: Box::new(TypeExpr::boolean_literal(false)),
    };
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::boolean_literal(true));
}

#[test]
fn eval_conditional_false_branch() {
    let mut env = EvalEnv::new();
    // number extends string ? true : false → false
    let expr = TypeExpr::Conditional {
        check: Box::new(TypeExpr::Primitive(PrimitiveName::Number)),
        extends: Box::new(TypeExpr::Primitive(PrimitiveName::String)),
        true_type: Box::new(TypeExpr::boolean_literal(true)),
        false_type: Box::new(TypeExpr::boolean_literal(false)),
    };
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::boolean_literal(false));
}

// =============================================================================
// Mapped types
// =============================================================================

#[test]
fn eval_mapped_keyof() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "T".to_string(),
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "a".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "b".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    });

    // { [K in keyof T]?: T[K] } — essentially Partial<T>
    let expr = TypeExpr::Mapped {
        parameter: "K".to_string(),
        source: Box::new(TypeExpr::KeyOf(Box::new(TypeExpr::named("T")))),
        value: Box::new(TypeExpr::IndexedAccess {
            object: Box::new(TypeExpr::named("T")),
            index: Box::new(TypeExpr::named("K")),
        }),
        optional: MappedModifier::Add,
        readonly: MappedModifier::None,
        name_type: None,
    };
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
            for member in &obj.properties {
                if let ObjectMember::Property(p) = member {
                    assert!(p.optional, "{} should be optional", p.name);
                }
            }
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

// =============================================================================
// Template literal types
// =============================================================================

#[test]
fn eval_template_literal_finite() {
    let mut env = EvalEnv::new();
    // `btn-${"sm" | "lg"}` → "btn-sm" | "btn-lg"
    let expr = TypeExpr::TemplateLiteral {
        quasis: vec!["btn-".to_string(), "".to_string()],
        expressions: vec![TypeExpr::Union(vec![
            TypeExpr::string_literal("sm"),
            TypeExpr::string_literal("lg"),
        ])],
    };
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Union(types) => {
            assert_eq!(types.len(), 2);
            assert!(types.contains(&TypeExpr::string_literal("btn-sm")));
            assert!(types.contains(&TypeExpr::string_literal("btn-lg")));
        }
        _ => panic!("expected union, got {result:?}"),
    }
}

#[test]
fn eval_template_literal_infinite_degrades() {
    let mut env = EvalEnv::new();
    // `${number}px` → string (can't expand infinite set)
    let expr = TypeExpr::TemplateLiteral {
        quasis: vec!["".to_string(), "px".to_string()],
        expressions: vec![TypeExpr::Primitive(PrimitiveName::Number)],
    };
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::String));
}

// =============================================================================
// Composition: Partial<Pick<User, ...>>
// =============================================================================

#[test]
fn eval_nested_utility() {
    let mut env = env_with_user_type();
    // Partial<Pick<User, "name" | "email">>
    let expr = parse_type_annotation("Partial<Pick<User, \"name\" | \"email\">>");
    let result = evaluate(&expr, &mut env);
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
            assert!(names.contains(&"name"));
            assert!(names.contains(&"email"));
            assert!(!names.contains(&"id"), "id should not be present");
            // All should be optional
            for member in &obj.properties {
                if let ObjectMember::Property(p) = member {
                    assert!(p.optional, "{} should be optional", p.name);
                }
            }
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

// =============================================================================
// Depth limit
// =============================================================================

#[test]
fn eval_respects_depth_limit() {
    let mut env = EvalEnv::with_limits(EvalLimits {
        max_depth: 5,
        ..EvalLimits::default()
    });
    // type Deep = { inner: Deep }
    env.add_type(TypeDeclInfo {
        name: "Deep".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "inner".to_string(),
                ty: TypeExpr::named("Deep"),
                optional: false,
                readonly: false,
            })],
        }),
    });
    // Should not stack overflow
    let result = evaluate(&TypeExpr::named("Deep"), &mut env);
    assert!(!result.is_unknown());
}
