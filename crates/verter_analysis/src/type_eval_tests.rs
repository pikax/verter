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

#[test]
fn eval_omit_with_nested_literal_union_keys() {
    let mut env = env_with_user_type();
    env.add_type(TypeDeclInfo {
        name: "SensitiveKeys".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Union(vec![
            TypeExpr::Literal(LiteralValue::String("email".to_string())),
            TypeExpr::Literal(LiteralValue::String("password".to_string())),
        ]),
    });

    let expr = parse_type_annotation("Omit<User, SensitiveKeys | \"name\">");
    let result = evaluate(&expr, &mut env);
    match &result {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|m| match m {
                    ObjectMember::Property(p) => Some(p.name.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(names, vec!["id"], "nested union keys should be flattened");
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

fn add_deep_alias_chain(env: &mut EvalEnv, prefix: &str, depth: usize) -> String {
    let leaf_name = format!("{prefix}{depth}");
    env.add_type(TypeDeclInfo {
        name: leaf_name.clone(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Primitive(PrimitiveName::String),
    });

    for index in (0..depth).rev() {
        let name = format!("{prefix}{index}");
        let next = if index + 1 == depth {
            leaf_name.clone()
        } else {
            format!("{prefix}{}", index + 1)
        };
        env.add_type(TypeDeclInfo {
            name,
            kind: TypeDeclKind::Alias,
            type_parameters: vec![],
            body: TypeExpr::named(&next),
        });
    }

    format!("{prefix}0")
}

// ── Opaque typeof bailout + lazy indexed access tests ──────────────────

/// Test 0 (regression gate): A ComponentConfig-shaped generic with an
/// unresolved `typeof theme` arg must NOT hang. It should bail out quickly
/// and return the reference with evaluated args, not brute-force the body.
#[test]
fn opaque_typeof_arg_bails_generic_instantiation_quickly() {
    let mut env = EvalEnv::new();

    // Define a ComponentConfig-like type with 4 members
    env.add_type(TypeDeclInfo {
        name: "ComponentConfig".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![
            TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            },
            TypeParam {
                name: "A".to_string(),
                constraint: None,
                default: None,
            },
        ],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "slots".to_string(),
                    ty: TypeExpr::named_with_args("SlotsHelper", vec![TypeExpr::named("T")]),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "variants".to_string(),
                    ty: TypeExpr::named_with_args("VariantsHelper", vec![TypeExpr::named("T")]),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "ui".to_string(),
                    ty: TypeExpr::named_with_args("UIHelper", vec![TypeExpr::named("T")]),
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    });

    // `typeof theme` — theme is NOT in value_symbols → opaque
    let typeof_theme = TypeExpr::TypeOf(ValueRef {
        path: vec!["theme".to_string()],
    });

    // ComponentConfig<typeof theme, AppConfig>
    let expr = TypeExpr::named_with_args(
        "ComponentConfig",
        vec![typeof_theme, TypeExpr::named("AppConfig")],
    );

    let result = evaluate(&expr, &mut env);

    // Must complete without exhausting budget
    assert!(
        !env.budget_exhausted(),
        "opaque typeof arg should bail immediately, not exhaust step budget (steps={})",
        env.steps(),
    );

    // Result should be the symbolic ref, not an expanded object
    assert!(
        !matches!(result, TypeExpr::Object(_)),
        "should NOT eagerly expand the generic body with opaque args; got object"
    );
}

/// Regression: the motivating case includes an opaque `typeof theme` arg and an
/// unrelated but very expensive second arg (`AppConfig`). Bailout must happen
/// before eagerly evaluating that unrelated arg.
#[test]
fn opaque_typeof_bailout_skips_unrelated_expensive_generic_args() {
    let mut env = EvalEnv::new();
    env.limits.max_depth = 2048;
    env.limits.max_steps = 20_000;
    let expensive_name = add_deep_alias_chain(&mut env, "AppConfigLayer", 512);

    env.add_type(TypeDeclInfo {
        name: "ComponentConfig".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![
            TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            },
            TypeParam {
                name: "A".to_string(),
                constraint: None,
                default: None,
            },
        ],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "slots".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        }),
    });

    let result = evaluate(
        &TypeExpr::named_with_args(
            "ComponentConfig",
            vec![
                TypeExpr::TypeOf(ValueRef {
                    path: vec!["theme".to_string()],
                }),
                TypeExpr::named(&expensive_name),
            ],
        ),
        &mut env,
    );

    assert!(
        env.steps() < 100,
        "opaque bailout should happen before evaluating unrelated expensive args (steps={})",
        env.steps()
    );
    assert!(
        matches!(result, TypeExpr::Ref { .. }),
        "opaque bailout should return a symbolic ref, got: {:?}",
        result
    );
}

/// Nested opaque typeof in type arg: Container<Pick<typeof theme, 'key'>> should bail.
#[test]
fn nested_opaque_typeof_in_arg_bails_generic() {
    let mut env = EvalEnv::new();

    // type Container<T> = { data: T }
    env.add_type(TypeDeclInfo {
        name: "Container".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
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

    // Container<Pick<typeof theme, 'key'>> — typeof theme is opaque
    let typeof_theme = TypeExpr::TypeOf(ValueRef {
        path: vec!["theme".to_string()],
    });
    let pick_typeof = TypeExpr::named_with_args(
        "Pick",
        vec![
            typeof_theme,
            TypeExpr::Literal(LiteralValue::String("key".to_string())),
        ],
    );
    let expr = TypeExpr::named_with_args("Container", vec![pick_typeof]);

    let result = evaluate(&expr, &mut env);

    assert!(
        !env.budget_exhausted(),
        "nested opaque typeof should bail (steps={})",
        env.steps(),
    );
    assert!(
        !matches!(result, TypeExpr::Object(_)),
        "should NOT expand generic with nested opaque typeof arg"
    );
}

/// Unresolved Ref args should also bail — they provide no usable structure and
/// should not trigger expansion of unrelated generic siblings.
#[test]
fn unresolved_ref_arg_bails_generic() {
    let mut env = EvalEnv::new();
    env.limits.max_depth = 2048;
    env.limits.max_steps = 20_000;
    let expensive_name = add_deep_alias_chain(&mut env, "AppConfigLayer", 512);

    // type Wrapper<T, U> = { value: T, other: U }
    env.add_type(TypeDeclInfo {
        name: "Wrapper".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![
            TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            },
            TypeParam {
                name: "U".to_string(),
                constraint: None,
                default: None,
            },
        ],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "value".to_string(),
                    ty: TypeExpr::named("T"),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "other".to_string(),
                    ty: TypeExpr::named("U"),
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    });

    // Wrapper<SomeUnknownType, Expensive> — unresolved ref should bail before
    // forcing evaluation of the unrelated expensive arg.
    let expr = TypeExpr::named_with_args(
        "Wrapper",
        vec![
            TypeExpr::named("SomeUnknownType"),
            TypeExpr::named(&expensive_name),
        ],
    );

    let result = evaluate(&expr, &mut env);

    assert!(
        env.steps() < 100,
        "unresolved ref arg should bail before unrelated expensive evaluation (steps={})",
        env.steps()
    );
    assert!(
        matches!(result, TypeExpr::Ref { .. }),
        "unresolved ref arg should trigger symbolic bailout, got: {:?}",
        result
    );
}

/// Missing types nested inside structural wrappers should still trigger the
/// same early bailout instead of leaking through and expanding the generic.
#[test]
fn nested_missing_type_inside_structural_arg_bails_generic() {
    let mut env = EvalEnv::new();
    env.limits.max_depth = 2048;
    env.limits.max_steps = 20_000;
    let expensive_name = add_deep_alias_chain(&mut env, "AppConfigLayer", 512);

    env.add_type(TypeDeclInfo {
        name: "Wrapper".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![
            TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            },
            TypeParam {
                name: "U".to_string(),
                constraint: None,
                default: None,
            },
        ],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "value".to_string(),
                    ty: TypeExpr::named("T"),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "other".to_string(),
                    ty: TypeExpr::named("U"),
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    });

    let nested_missing = TypeExpr::Array {
        element: Box::new(TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "item".to_string(),
                ty: TypeExpr::named("MissingType"),
                optional: false,
                readonly: false,
            })],
        })),
        readonly: false,
    };

    let result = evaluate(
        &TypeExpr::named_with_args(
            "Wrapper",
            vec![nested_missing, TypeExpr::named(&expensive_name)],
        ),
        &mut env,
    );

    assert!(
        env.steps() < 100,
        "nested structural missing types should bail before unrelated expensive evaluation (steps={})",
        env.steps()
    );
    assert!(
        matches!(result, TypeExpr::Ref { .. }),
        "nested structural missing types should trigger symbolic bailout, got: {:?}",
        result
    );
}

/// Negative test: a generic with fully resolved args should still expand normally.
#[test]
fn resolved_typeof_arg_still_expands_generic() {
    let mut env = EvalEnv::new();

    // Register `theme` as a concrete value
    env.add_value(ValueDeclInfo {
        name: "theme".to_string(),
        type_annotation: Some(TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "color".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        })),
        kind: ValueDeclKind::Const,
        function_signature: None,
        object_shape: None,
    });

    // Simple generic: type Wrapper<T> = { value: T }
    env.add_type(TypeDeclInfo {
        name: "Wrapper".to_string(),
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

    // Wrapper<typeof theme> — theme IS resolvable
    let typeof_theme = TypeExpr::TypeOf(ValueRef {
        path: vec!["theme".to_string()],
    });
    let expr = TypeExpr::named_with_args("Wrapper", vec![typeof_theme]);

    let result = evaluate(&expr, &mut env);

    // Should expand to { value: { color: string } }
    assert!(
        matches!(result, TypeExpr::Object(_)),
        "resolved typeof arg should expand the generic body normally"
    );
}

/// Lazy indexed access: `{ a: X, b: Y, c: Z }['b']` should return Y
/// without evaluating X or Z.
#[test]
fn lazy_indexed_access_only_evaluates_requested_member() {
    let mut env = EvalEnv::new();

    // Define a type with an expensive member and a cheap member
    // type Config<T> = { expensive: ExpensiveType<T>, cheap: string }
    env.add_type(TypeDeclInfo {
        name: "Config".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "expensive".to_string(),
                    ty: TypeExpr::named_with_args("UnknownExpensive", vec![TypeExpr::named("T")]),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "cheap".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    });

    // Config<SomeType>['cheap'] — should get `string` without touching `expensive`
    let expr = TypeExpr::IndexedAccess {
        object: Box::new(TypeExpr::named_with_args(
            "Config",
            vec![TypeExpr::named("SomeArg")],
        )),
        index: Box::new(TypeExpr::Literal(LiteralValue::String("cheap".to_string()))),
    };

    let result = evaluate(&expr, &mut env);

    // Should resolve to `string`
    assert!(
        matches!(result, TypeExpr::Primitive(PrimitiveName::String)),
        "lazy indexed access should resolve 'cheap' to string, got: {:?}",
        result
    );
}

/// Non-literal index must fall back to eager evaluation.
#[test]
fn indexed_access_with_non_literal_index_falls_back() {
    let mut env = EvalEnv::new();

    env.add_type(TypeDeclInfo {
        name: "Config".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                readonly: false,
            })],
        }),
    });

    // Config[keyof Config] — non-literal index, should still work via fallback
    let expr = TypeExpr::IndexedAccess {
        object: Box::new(TypeExpr::named("Config")),
        index: Box::new(TypeExpr::KeyOf(Box::new(TypeExpr::named("Config")))),
    };

    let result = evaluate(&expr, &mut env);

    // Should fall back to eager and produce number
    assert!(
        matches!(result, TypeExpr::Primitive(PrimitiveName::Number)),
        "non-literal index should fall back to eager evaluation, got: {:?}",
        result
    );
}

/// Regression: `Accordion['slots']` should not evaluate an unrelated expensive
/// `AppConfig`-like generic arg when lazy member lookup can target `slots`.
#[test]
fn lazy_indexed_access_skips_unrelated_expensive_generic_args() {
    let mut env = EvalEnv::new();
    env.limits.max_depth = 2048;
    env.limits.max_steps = 20_000;
    let expensive_name = add_deep_alias_chain(&mut env, "AppConfigLayer", 512);

    env.add_type(TypeDeclInfo {
        name: "ComponentSlots".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "header".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        }),
    });

    env.add_type(TypeDeclInfo {
        name: "ComponentConfig".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![
            TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            },
            TypeParam {
                name: "A".to_string(),
                constraint: None,
                default: None,
            },
            TypeParam {
                name: "K".to_string(),
                constraint: None,
                default: None,
            },
        ],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "AppConfig".to_string(),
                    ty: TypeExpr::named("A"),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "variants".to_string(),
                    ty: TypeExpr::named("A"),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "slots".to_string(),
                    ty: TypeExpr::named_with_args("ComponentSlots", vec![TypeExpr::named("T")]),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "ui".to_string(),
                    ty: TypeExpr::named("A"),
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    });

    env.add_type(TypeDeclInfo {
        name: "Accordion".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::named_with_args(
            "ComponentConfig",
            vec![
                TypeExpr::TypeOf(ValueRef {
                    path: vec!["theme".to_string()],
                }),
                TypeExpr::named(&expensive_name),
                TypeExpr::Literal(LiteralValue::String("accordion".to_string())),
            ],
        ),
    });

    let result = evaluate(
        &TypeExpr::IndexedAccess {
            object: Box::new(TypeExpr::named("Accordion")),
            index: Box::new(TypeExpr::Literal(LiteralValue::String("slots".to_string()))),
        },
        &mut env,
    );

    assert!(
        env.steps() < 160,
        "lazy indexed access should not evaluate unrelated expensive generic args (steps={})",
        env.steps()
    );
    assert!(
        matches!(result, TypeExpr::Ref { ref name, .. } if name == "ComponentSlots"),
        "should still resolve the targeted slots member symbolically, got: {:?}",
        result
    );
}

/// Utility-wrapper indexed access should still use the lazy member path.
/// `Required<Config>['cheap']` must not evaluate unrelated siblings on Config.
#[test]
fn lazy_indexed_access_through_required_wrapper_skips_unrelated_members() {
    let mut env = EvalEnv::new();
    env.limits.max_depth = 2048;
    env.limits.max_steps = 20_000;
    let expensive_name = add_deep_alias_chain(&mut env, "ExpensiveRequiredLayer", 512);

    env.add_type(TypeDeclInfo {
        name: "Config".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "cheap".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: true,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "expensive".to_string(),
                    ty: TypeExpr::named(&expensive_name),
                    optional: true,
                    readonly: false,
                }),
            ],
        }),
    });

    let result = evaluate(
        &TypeExpr::IndexedAccess {
            object: Box::new(TypeExpr::named_with_args(
                "Required",
                vec![TypeExpr::named("Config")],
            )),
            index: Box::new(TypeExpr::Literal(LiteralValue::String("cheap".to_string()))),
        },
        &mut env,
    );

    assert!(
        matches!(result, TypeExpr::Primitive(PrimitiveName::String)),
        "Required<Config>['cheap'] should resolve to string, got: {:?}",
        result
    );
    assert!(
        env.steps() < 120,
        "Required-wrapped indexed access should not evaluate unrelated expensive members (steps={})",
        env.steps()
    );
}

/// Pick should project from the raw object/ref surface and avoid evaluating
/// omitted siblings.
#[test]
fn pick_skips_unselected_expensive_members() {
    let mut env = EvalEnv::new();
    env.limits.max_depth = 2048;
    env.limits.max_steps = 20_000;
    let expensive_name = add_deep_alias_chain(&mut env, "ExpensivePickLayer", 512);

    env.add_type(TypeDeclInfo {
        name: "Config".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "keep".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "omit".to_string(),
                    ty: TypeExpr::named(&expensive_name),
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    });

    let result = evaluate(
        &TypeExpr::named_with_args(
            "Pick",
            vec![
                TypeExpr::named("Config"),
                TypeExpr::Literal(LiteralValue::String("keep".to_string())),
            ],
        ),
        &mut env,
    );

    let TypeExpr::Object(obj) = result else {
        panic!("Pick should resolve to an object");
    };
    let names: Vec<&str> = obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(prop) => Some(prop.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["keep"]);
    assert!(
        env.steps() < 120,
        "Pick should not evaluate omitted expensive members (steps={})",
        env.steps()
    );
}

/// Pick projection should preserve selected methods, not just plain properties.
#[test]
fn pick_projection_keeps_selected_methods() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "Config".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Method(MethodSignature {
                    name: "render".to_string(),
                    function: FunctionExpr {
                        parameters: vec![],
                        return_type: Some(Box::new(TypeExpr::Primitive(PrimitiveName::String))),
                        type_parameters: vec![],
                    },
                    optional: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "other".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    });

    let result = evaluate(
        &TypeExpr::named_with_args(
            "Pick",
            vec![
                TypeExpr::named("Config"),
                TypeExpr::Literal(LiteralValue::String("render".to_string())),
            ],
        ),
        &mut env,
    );

    let TypeExpr::Object(obj) = result else {
        panic!("Pick should resolve to an object");
    };
    assert_eq!(obj.properties.len(), 1);
    assert!(
        matches!(&obj.properties[0], ObjectMember::Method(method) if method.name == "render"),
        "Pick should preserve selected method members, got: {:?}",
        obj.properties
    );
}

/// Omit projection should drop selected methods rather than keeping them.
#[test]
fn omit_projection_drops_selected_methods() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "Config".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Method(MethodSignature {
                    name: "render".to_string(),
                    function: FunctionExpr {
                        parameters: vec![],
                        return_type: Some(Box::new(TypeExpr::Primitive(PrimitiveName::String))),
                        type_parameters: vec![],
                    },
                    optional: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "other".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    });

    let result = evaluate(
        &TypeExpr::named_with_args(
            "Omit",
            vec![
                TypeExpr::named("Config"),
                TypeExpr::Literal(LiteralValue::String("render".to_string())),
            ],
        ),
        &mut env,
    );

    let TypeExpr::Object(obj) = result else {
        panic!("Omit should resolve to an object");
    };
    assert_eq!(obj.properties.len(), 1);
    assert!(
        matches!(&obj.properties[0], ObjectMember::Property(prop) if prop.name == "other"),
        "Omit should drop selected method members, got: {:?}",
        obj.properties
    );
}

/// Omit should also avoid evaluating the removed members.
#[test]
fn omit_skips_removed_expensive_members() {
    let mut env = EvalEnv::new();
    env.limits.max_depth = 2048;
    env.limits.max_steps = 20_000;
    let expensive_name = add_deep_alias_chain(&mut env, "ExpensiveOmitLayer", 512);

    env.add_type(TypeDeclInfo {
        name: "Config".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "keep".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "omit".to_string(),
                    ty: TypeExpr::named(&expensive_name),
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    });

    let result = evaluate(
        &TypeExpr::named_with_args(
            "Omit",
            vec![
                TypeExpr::named("Config"),
                TypeExpr::Literal(LiteralValue::String("omit".to_string())),
            ],
        ),
        &mut env,
    );

    let TypeExpr::Object(obj) = result else {
        panic!("Omit should resolve to an object");
    };
    let names: Vec<&str> = obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(prop) => Some(prop.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["keep"]);
    assert!(
        env.steps() < 120,
        "Omit should not evaluate removed expensive members (steps={})",
        env.steps()
    );
}

#[test]
fn omit_projection_keeps_nested_utility_members() {
    let mut env = EvalEnv::new();

    env.add_type(TypeDeclInfo {
        name: "LinkPropsKeys".to_string(),
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Union(vec![
            TypeExpr::Literal(LiteralValue::String("replace".to_string())),
            TypeExpr::Literal(LiteralValue::String("activeClass".to_string())),
            TypeExpr::Literal(LiteralValue::String("ariaCurrentValue".to_string())),
        ]),
    });
    env.add_type(TypeDeclInfo {
        name: "RouterLinkOptions".to_string(),
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "replace".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                    optional: true,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "activeClass".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: true,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "ariaCurrentValue".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: true,
                    readonly: false,
                }),
            ],
        }),
    });
    env.add_type(TypeDeclInfo {
        name: "LinkProps".to_string(),
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Intersection(vec![
            TypeExpr::named("RouterLinkOptions"),
            TypeExpr::Object(ObjectExpr {
                properties: vec![
                    ObjectMember::Property(ObjectProperty {
                        name: "href".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: true,
                        readonly: false,
                    }),
                    ObjectMember::Property(ObjectProperty {
                        name: "raw".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                        optional: true,
                        readonly: false,
                    }),
                    ObjectMember::Property(ObjectProperty {
                        name: "custom".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                        optional: true,
                        readonly: false,
                    }),
                ],
            }),
        ]),
    });
    env.add_type(TypeDeclInfo {
        name: "UseComponentIconsProps".to_string(),
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "icon".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: true,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "loading".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                    optional: true,
                    readonly: false,
                }),
            ],
        }),
    });
    env.add_type(TypeDeclInfo {
        name: "ButtonProps".to_string(),
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Intersection(vec![
            TypeExpr::named("UseComponentIconsProps"),
            TypeExpr::named_with_args(
                "Omit",
                vec![
                    TypeExpr::named("LinkProps"),
                    TypeExpr::Union(vec![
                        TypeExpr::Literal(LiteralValue::String("raw".to_string())),
                        TypeExpr::Literal(LiteralValue::String("custom".to_string())),
                    ]),
                ],
            ),
            TypeExpr::Object(ObjectExpr {
                properties: vec![
                    ObjectMember::Property(ObjectProperty {
                        name: "label".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: true,
                        readonly: false,
                    }),
                    ObjectMember::Property(ObjectProperty {
                        name: "color".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: true,
                        readonly: false,
                    }),
                ],
            }),
        ]),
    });

    let result = evaluate(
        &parse_type_annotation("Omit<ButtonProps, LinkPropsKeys | \"icon\" | \"color\">"),
        &mut env,
    );

    let TypeExpr::Object(obj) = result else {
        panic!("nested Omit projection should resolve to an object");
    };
    let names: Vec<&str> = obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(prop) => Some(prop.name.as_str()),
            _ => None,
        })
        .collect();

    assert!(
        names.contains(&"loading"),
        "loading should survive, got {names:?}"
    );
    assert!(
        names.contains(&"label"),
        "label should survive, got {names:?}"
    );
    assert!(
        names.contains(&"href"),
        "nested utility-derived members should survive projection, got {names:?}"
    );
    assert!(
        !names.contains(&"icon"),
        "icon should be omitted, got {names:?}"
    );
    assert!(
        !names.contains(&"replace"),
        "nested utility-derived omitted keys should stay omitted, got {names:?}"
    );
}
