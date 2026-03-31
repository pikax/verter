use super::type_eval::*;
use super::type_eval_build::parse_and_build_env;
use super::type_expr::*;
use super::type_expr_lower::parse_type_annotation;
use rustc_hash::FxHashMap;
use std::collections::BTreeSet;
use std::sync::Arc;

fn env_with_user_type() -> EvalEnv {
    let mut env = EvalEnv::new();
    // interface User { id: number; name: string; email: string; password: string }
    env.add_type(TypeDeclInfo {
        name: "User".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
    });
    env
}

fn assert_union_string_literals(expr: &TypeExpr, expected: &[&str]) {
    let mut actual = BTreeSet::new();
    match expr {
        TypeExpr::Literal(LiteralValue::String(value)) => {
            actual.insert(value.as_str());
        }
        TypeExpr::Union(types) => {
            for ty in types.iter() {
                match ty {
                    TypeExpr::Literal(LiteralValue::String(value)) => {
                        actual.insert(value.as_str());
                    }
                    TypeExpr::Primitive(PrimitiveName::Undefined) => {}
                    other => panic!(
                        "expected only string literal members (plus optional undefined), got {other:?}"
                    ),
                }
            }
        }
        other => panic!("expected string literal union, got {other:?}"),
    }

    assert_eq!(actual, BTreeSet::from_iter(expected.iter().copied()));
}

#[test]
fn add_type_preserves_existing_stable_declaration_id_on_reinsert() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "User".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr { properties: vec![] })),
    });
    let stable_id = env.type_symbols["User"].declaration_id;

    env.add_type(TypeDeclInfo {
        name: "User".to_string(),
        declaration_id: stable_id + 41,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr { properties: vec![] })),
    });

    assert_eq!(
        env.type_symbols["User"].declaration_id, stable_id,
        "reinserted declarations should keep the existing stable id"
    );
    assert_eq!(env.type_declaration_id("User"), Some(stable_id));
}

#[test]
fn add_value_with_explicit_declaration_id_advances_allocator_without_collision() {
    let mut env = EvalEnv::new();
    env.add_value(ValueDeclInfo {
        name: "remoteValue".to_string(),
        declaration_id: 42,
        kind: ValueDeclKind::Const,
        type_annotation: Some(TypeExpr::Primitive(PrimitiveName::Number)),
        function_signature: None,
        object_shape: None,
    });
    env.add_value(ValueDeclInfo {
        name: "localValue".to_string(),
        declaration_id: 0,
        kind: ValueDeclKind::Const,
        type_annotation: Some(TypeExpr::Primitive(PrimitiveName::String)),
        function_signature: None,
        object_shape: None,
    });

    assert_eq!(env.value_symbols["remoteValue"].declaration_id, 42);
    assert_eq!(env.value_declaration_id("remoteValue"), Some(42));
    assert!(
        env.value_symbols["localValue"].declaration_id > 42,
        "allocator should not reuse ids below the highest explicit declaration id"
    );
}

#[test]
fn extend_missing_preserves_and_synchronizes_declaration_ids() {
    let mut base = EvalEnv::new();
    base.add_type(TypeDeclInfo {
        name: "Local".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Primitive(PrimitiveName::String),
    });

    let mut imported = EvalEnv::new();
    imported.add_type(TypeDeclInfo {
        name: "Remote".to_string(),
        declaration_id: 17,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Primitive(PrimitiveName::Number),
    });
    imported.add_value(ValueDeclInfo {
        name: "remoteValue".to_string(),
        declaration_id: 23,
        kind: ValueDeclKind::Const,
        type_annotation: Some(TypeExpr::Primitive(PrimitiveName::Boolean)),
        function_signature: None,
        object_shape: None,
    });

    base.extend_missing(imported);

    assert_eq!(base.type_symbols["Remote"].declaration_id, 17);
    assert_eq!(base.type_declaration_id("Remote"), Some(17));
    assert_eq!(base.value_symbols["remoteValue"].declaration_id, 23);
    assert_eq!(base.value_declaration_id("remoteValue"), Some(23));
}

#[test]
fn extend_missing_from_ref_preserves_and_synchronizes_declaration_ids() {
    let mut base = EvalEnv::new();
    base.add_type(TypeDeclInfo {
        name: "Local".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Primitive(PrimitiveName::String),
    });

    let mut imported = EvalEnv::new();
    imported.add_type(TypeDeclInfo {
        name: "Remote".to_string(),
        declaration_id: 17,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Primitive(PrimitiveName::Number),
    });
    imported.add_value(ValueDeclInfo {
        name: "remoteValue".to_string(),
        declaration_id: 23,
        kind: ValueDeclKind::Const,
        type_annotation: Some(TypeExpr::Primitive(PrimitiveName::Boolean)),
        function_signature: None,
        object_shape: None,
    });

    base.extend_missing_from_ref(&imported);

    assert_eq!(base.type_symbols["Remote"].declaration_id, 17);
    assert_eq!(base.type_declaration_id("Remote"), Some(17));
    assert_eq!(base.value_symbols["remoteValue"].declaration_id, 23);
    assert_eq!(base.value_declaration_id("remoteValue"), Some(23));
}

#[derive(Default)]
struct TestLookup {
    type_decls: FxHashMap<String, TypeDeclInfo>,
    value_decls: FxHashMap<String, ValueDeclInfo>,
    utility_sources: FxHashMap<String, BuiltinUtilitySource>,
}

impl EvalLookup for TestLookup {
    fn resolve_type_decl(&mut self, name: &str) -> Option<TypeDeclInfo> {
        self.type_decls.get(name).cloned()
    }

    fn resolve_value_decl(&mut self, path: &[String]) -> Option<ValueDeclInfo> {
        if path.len() != 1 {
            return None;
        }
        self.value_decls.get(&path[0]).cloned()
    }

    fn utility_source(&mut self, name: &str) -> BuiltinUtilitySource {
        self.utility_sources.get(name).copied().unwrap_or_else(|| {
            if matches!(
                name,
                "Partial"
                    | "Required"
                    | "Readonly"
                    | "Pick"
                    | "Omit"
                    | "Record"
                    | "Extract"
                    | "Exclude"
                    | "NonNullable"
                    | "ReturnType"
                    | "Parameters"
                    | "ConstructorParameters"
                    | "InstanceType"
                    | "Awaited"
            ) {
                BuiltinUtilitySource::Builtin
            } else {
                BuiltinUtilitySource::Unknown
            }
        })
    }
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::named("A"),
    });
    let result = evaluate(&TypeExpr::named("A"), &mut env);
    // Should not stack overflow — cycle detection kicks in
    assert_eq!(result, TypeExpr::named("A"));
}

#[test]
fn eval_intersection_child_override_wins() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::intersection(vec![
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "label".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "shared".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                }),
            ],
        })),
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "shared".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "count".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                    optional: false,
                    readonly: false,
                }),
            ],
        })),
    ]);

    let result = evaluate(&expr, &mut env);
    let TypeExpr::Object(obj) = result else {
        panic!("expected merged object, got {result:?}");
    };

    let shared = obj
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(prop) if prop.name == "shared" => Some(prop),
            _ => None,
        })
        .expect("intersection should keep shared property");
    assert_eq!(
        shared.ty,
        TypeExpr::Primitive(PrimitiveName::Number),
        "later child declarations should override the base property type"
    );
    assert!(
        obj.properties
            .iter()
            .any(|member| matches!(member, ObjectMember::Property(prop) if prop.name == "count")),
        "merged intersection should retain child-only properties"
    );
    assert!(
        !obj.properties
            .iter()
            .any(|member| matches!(member, ObjectMember::Property(prop) if prop.name == "missing")),
        "intersection merge must not fabricate unrelated properties"
    );
}

#[test]
fn eval_with_lookup_resolves_external_type_reference() {
    let mut env = EvalEnv::new();
    let mut lookup = TestLookup::default();
    lookup.type_decls.insert(
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

    let result = evaluate_with_lookup(&TypeExpr::named("RemoteProps"), &mut env, &mut lookup);
    let TypeExpr::Object(obj) = result else {
        panic!("expected object from external lookup");
    };
    assert_eq!(obj.properties.len(), 1);
    let ObjectMember::Property(prop) = &obj.properties[0] else {
        panic!("expected property member");
    };
    assert_eq!(prop.name, "title");
    assert_eq!(prop.ty, TypeExpr::Primitive(PrimitiveName::String));
}

#[test]
fn eval_with_lookup_resolves_external_typeof() {
    let mut env = EvalEnv::new();
    let mut lookup = TestLookup::default();
    lookup.value_decls.insert(
        "remoteConfig".to_string(),
        ValueDeclInfo {
            name: "remoteConfig".to_string(),
            declaration_id: 0,
            kind: ValueDeclKind::Const,
            type_annotation: Some(TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "theme".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                })],
            }))),
            function_signature: None,
            object_shape: None,
        },
    );

    let result = evaluate_with_lookup(
        &TypeExpr::TypeOf(ValueRef {
            path: vec!["remoteConfig".to_string()],
        }),
        &mut env,
        &mut lookup,
    );

    let TypeExpr::Object(obj) = result else {
        panic!("expected object from external typeof lookup");
    };
    let ObjectMember::Property(prop) = &obj.properties[0] else {
        panic!("expected property member");
    };
    assert_eq!(prop.name, "theme");
}

#[test]
fn eval_with_lookup_resolves_lazy_member_projection_from_external_ref() {
    let mut env = EvalEnv::new();
    let mut lookup = TestLookup::default();
    lookup.type_decls.insert(
        "RemoteProps".to_string(),
        TypeDeclInfo {
            name: "RemoteProps".to_string(),
            declaration_id: 0,
            kind: TypeDeclKind::Interface,
            type_parameters: vec![],
            body: TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![
                    ObjectMember::Property(ObjectProperty {
                        name: "title".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    }),
                    ObjectMember::Property(ObjectProperty {
                        name: "count".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::Number),
                        optional: false,
                        readonly: false,
                    }),
                ],
            })),
        },
    );

    let result = evaluate_with_lookup(
        &TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("RemoteProps")),
            index: Arc::new(TypeExpr::string_literal("title")),
        },
        &mut env,
        &mut lookup,
    );

    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::String));
}

#[test]
fn eval_with_lookup_allows_shadowing_builtin_pick() {
    let mut env = EvalEnv::new();
    let mut lookup = TestLookup::default();
    lookup
        .utility_sources
        .insert("Pick".to_string(), BuiltinUtilitySource::Shadowed);
    lookup.type_decls.insert(
        "Pick".to_string(),
        TypeDeclInfo {
            name: "Pick".to_string(),
            declaration_id: 0,
            kind: TypeDeclKind::Alias,
            type_parameters: vec![TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            }],
            body: TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "picked".to_string(),
                    ty: TypeExpr::named("T"),
                    optional: false,
                    readonly: false,
                })],
            })),
        },
    );

    let result = evaluate_with_lookup(
        &TypeExpr::named_with_args("Pick", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
        &mut lookup,
    );

    let TypeExpr::Object(obj) = result else {
        panic!("expected shadowed Pick alias to resolve as normal type");
    };
    let ObjectMember::Property(prop) = &obj.properties[0] else {
        panic!("expected property member");
    };
    assert_eq!(prop.name, "picked");
    assert_eq!(prop.ty, TypeExpr::Primitive(PrimitiveName::String));
}

#[test]
fn eval_with_lookup_instantiates_generic_with_external_argument() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "Box".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "value".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        })),
    });

    let mut lookup = TestLookup::default();
    lookup.type_decls.insert(
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

    let result = evaluate_with_lookup(
        &TypeExpr::named_with_args("Box", vec![TypeExpr::named("RemoteProps")]),
        &mut env,
        &mut lookup,
    );

    let TypeExpr::Object(obj) = result else {
        panic!("expected generic alias to instantiate with external argument");
    };
    let ObjectMember::Property(prop) = &obj.properties[0] else {
        panic!("expected property member");
    };
    assert_eq!(prop.name, "value");
    let TypeExpr::Object(inner) = &prop.ty else {
        panic!("expected external argument to resolve before instantiation");
    };
    let ObjectMember::Property(inner_prop) = &inner.properties[0] else {
        panic!("expected nested property");
    };
    assert_eq!(inner_prop.name, "title");
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "value".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        })),
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
fn eval_generic_alias_substitutes_normalized_type_parameter_nodes() {
    let mut env = EvalEnv::new();
    let generic = TypeParam {
        name: "T".to_string(),
        constraint: None,
        default: None,
    };
    env.add_type(TypeDeclInfo {
        name: "Box".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![generic.clone()],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "value".to_string(),
                ty: TypeExpr::type_parameter(generic),
                optional: false,
                readonly: false,
            })],
        })),
    });

    let expr = TypeExpr::named_with_args("Box", vec![TypeExpr::Primitive(PrimitiveName::String)]);
    let result = evaluate(&expr, &mut env);
    let TypeExpr::Object(obj) = result else {
        panic!("expected object result for generic alias");
    };
    let ObjectMember::Property(prop) = &obj.properties[0] else {
        panic!("expected object property");
    };
    assert_eq!(prop.name, "value");
    assert_eq!(prop.ty, TypeExpr::Primitive(PrimitiveName::String));
}

#[test]
fn eval_generic_with_default() {
    let mut env = EvalEnv::new();
    // type Wrapper<T = number> = { data: T }
    env.add_type(TypeDeclInfo {
        name: "Wrapper".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Number))),
        }],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "data".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        })),
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

#[test]
fn eval_generic_without_default_falls_back_to_constraint() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "Wrapper".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
            default: None,
        }],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "data".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        })),
    });

    let result = evaluate(&TypeExpr::named("Wrapper"), &mut env);
    let TypeExpr::Object(obj) = result else {
        panic!("expected object");
    };
    let ObjectMember::Property(prop) = &obj.properties[0] else {
        panic!("expected property");
    };
    assert_eq!(prop.ty, TypeExpr::Primitive(PrimitiveName::String));
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
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::union(vec![
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
    let source = TypeExpr::union(vec![
        TypeExpr::string_literal("a"),
        TypeExpr::string_literal("b"),
        TypeExpr::string_literal("c"),
    ]);
    let target = TypeExpr::union(vec![
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
    let source = TypeExpr::union(vec![
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
        vec![TypeExpr::union(vec![
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
    let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::named("User")));
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
    let obj = TypeExpr::Object(Arc::new(ObjectExpr {
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
    }));
    let expr = TypeExpr::KeyOf(Arc::new(obj));
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
        declaration_id: 0,
        kind: ValueDeclKind::Function,
        type_annotation: None,
        function_signature: Some(FunctionSignature {
            parameters: vec![],
            return_type: Some(TypeExpr::Object(Arc::new(ObjectExpr {
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
            }))),
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
        declaration_id: 0,
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
        declaration_id: 0,
        kind: ValueDeclKind::Function,
        type_annotation: None,
        function_signature: Some(FunctionSignature {
            parameters: vec![],
            return_type: Some(TypeExpr::Object(Arc::new(ObjectExpr {
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
            }))),
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
        declaration_id: 0,
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
        declaration_id: 0,
        kind: TypeDeclKind::Class,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "id".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                readonly: false,
            })],
        })),
    });
    env.add_value(ValueDeclInfo {
        name: "Widget".to_string(),
        declaration_id: 0,
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
                return_type: Some(Arc::new(TypeExpr::named("Widget"))),
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
        declaration_id: 0,
        kind: TypeDeclKind::Class,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
    });
    env.add_value(ValueDeclInfo {
        name: "Widget".to_string(),
        declaration_id: 0,
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
                return_type: Some(Arc::new(TypeExpr::named("Widget"))),
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
        object: Arc::new(TypeExpr::named("User")),
        index: Arc::new(TypeExpr::string_literal("name")),
    };
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::String));
}

#[test]
fn eval_indexed_access_array_number() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            readonly: false,
        }),
        index: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
    };
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::String));
}

#[test]
fn eval_indexed_access_tuple_literal() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::Tuple {
            elements: Arc::from(vec![
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
            ]),
            readonly: false,
        }),
        index: Arc::new(TypeExpr::number_literal(1.0)),
    };
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::Number));
}

#[test]
fn eval_indexed_access_union_keys() {
    let mut env = env_with_user_type();
    // User["id" | "name"] → number | string
    let expr = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::named("User")),
        index: Arc::new(TypeExpr::union(vec![
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

#[test]
fn eval_indexed_access_can_project_other_member_from_active_local_decl() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "Props".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: Vec::new(),
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "type".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: true,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "mirror".to_string(),
                    ty: TypeExpr::IndexedAccess {
                        object: Arc::new(TypeExpr::named("Props")),
                        index: Arc::new(TypeExpr::string_literal("type")),
                    },
                    optional: true,
                    readonly: false,
                }),
            ],
        })),
    });

    let expr = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::named("Props")),
        index: Arc::new(TypeExpr::string_literal("mirror")),
    };
    let result = evaluate(&expr, &mut env);

    assert_eq!(result, TypeExpr::Primitive(PrimitiveName::String));
}

// =============================================================================
// Conditional types
// =============================================================================

#[test]
fn eval_conditional_true_branch() {
    let mut env = EvalEnv::new();
    // "hello" extends string ? true : false → true
    let expr = TypeExpr::Conditional {
        check: Arc::new(TypeExpr::string_literal("hello")),
        extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        true_type: Arc::new(TypeExpr::boolean_literal(true)),
        false_type: Arc::new(TypeExpr::boolean_literal(false)),
    };
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::boolean_literal(true));
}

#[test]
fn eval_conditional_false_branch() {
    let mut env = EvalEnv::new();
    // number extends string ? true : false → false
    let expr = TypeExpr::Conditional {
        check: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
        extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        true_type: Arc::new(TypeExpr::boolean_literal(true)),
        false_type: Arc::new(TypeExpr::boolean_literal(false)),
    };
    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::boolean_literal(false));
}

#[test]
fn eval_conditional_resolves_required_object_record_assignability() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::Conditional {
        check: Arc::new(TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "ui".to_string(),
                ty: TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "button".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                    })],
                })),
                optional: false,
                readonly: false,
            })],
        }))),
        extends: Arc::new(TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "ui".to_string(),
                ty: TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "button".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::Any),
                        optional: false,
                        readonly: false,
                    })],
                })),
                optional: false,
                readonly: false,
            })],
        }))),
        true_type: Arc::new(TypeExpr::string_literal("yes")),
        false_type: Arc::new(TypeExpr::string_literal("no")),
    };

    let result = evaluate(&expr, &mut env);
    assert_eq!(result, TypeExpr::string_literal("yes"));
}

#[test]
fn eval_keyof_merges_intersection_object_keys() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::KeyOf(Arc::new(TypeExpr::Intersection(
        vec![
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "primary".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                })],
            })),
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "neutral".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                })],
            })),
        ]
        .into(),
    )));

    let result = evaluate(&expr, &mut env);
    assert_union_string_literals(&result, &["neutral", "primary"]);
}

#[test]
fn eval_indexed_access_merges_intersection_object_members() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::Intersection(
            vec![
                TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "color".to_string(),
                        ty: TypeExpr::Object(Arc::new(ObjectExpr {
                            properties: vec![ObjectMember::Property(ObjectProperty {
                                name: "primary".to_string(),
                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                optional: false,
                                readonly: false,
                            })],
                        })),
                        optional: false,
                        readonly: false,
                    })],
                })),
                TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "color".to_string(),
                        ty: TypeExpr::Object(Arc::new(ObjectExpr {
                            properties: vec![ObjectMember::Property(ObjectProperty {
                                name: "neutral".to_string(),
                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                optional: false,
                                readonly: false,
                            })],
                        })),
                        optional: false,
                        readonly: false,
                    })],
                })),
            ]
            .into(),
        )),
        index: Arc::new(TypeExpr::string_literal("color")),
    };

    let result = evaluate(&expr, &mut env);
    match result {
        TypeExpr::Object(_) => {
            let keys = evaluate(&TypeExpr::KeyOf(Arc::new(result.clone())), &mut env);
            assert_union_string_literals(&keys, &["neutral", "primary"]);
        }
        other => panic!("expected merged intersection member, got {other:?}"),
    }
}

#[test]
fn eval_ref_uses_outer_binding_when_generic_arg_shadows_type_parameter_name() {
    let mut env = EvalEnv::new();
    env.type_symbols.insert(
        "Id".to_string(),
        TypeDeclInfo {
            name: "Id".to_string(),
            declaration_id: 0,
            kind: TypeDeclKind::Alias,
            type_parameters: vec![TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            }],
            body: TypeExpr::named("T"),
        },
    );
    env.type_bindings.insert(
        "T".to_string(),
        Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
    );

    let expr = TypeExpr::named_with_args("Id", vec![TypeExpr::named("T")]);

    assert_eq!(
        evaluate(&expr, &mut env),
        TypeExpr::Primitive(PrimitiveName::String)
    );
}

// =============================================================================
// Mapped types
// =============================================================================

#[test]
fn eval_mapped_keyof() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "T".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
    });

    // { [K in keyof T]?: T[K] } — essentially Partial<T>
    let expr = TypeExpr::Mapped {
        parameter: "K".to_string(),
        source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
        value: Arc::new(TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("T")),
            index: Arc::new(TypeExpr::named("K")),
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
        expressions: Arc::from(vec![TypeExpr::union(vec![
            TypeExpr::string_literal("sm"),
            TypeExpr::string_literal("lg"),
        ])]),
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
        expressions: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::Number)]),
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "inner".to_string(),
                ty: TypeExpr::named("Deep"),
                optional: false,
                readonly: false,
            })],
        })),
    });
    // Should not stack overflow
    let result = evaluate(&TypeExpr::named("Deep"), &mut env);
    assert!(!result.is_unknown());
}

fn add_deep_alias_chain(env: &mut EvalEnv, prefix: &str, depth: usize) -> String {
    let leaf_name = format!("{prefix}{depth}");
    env.add_type(TypeDeclInfo {
        name: leaf_name.clone(),
        declaration_id: 0,
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
            declaration_id: 0,
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
        declaration_id: 0,
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
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
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
        declaration_id: 0,
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
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "slots".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        })),
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "data".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        })),
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
        declaration_id: 0,
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
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
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
        declaration_id: 0,
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
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
    });

    let nested_missing = TypeExpr::Array {
        element: Arc::new(TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "item".to_string(),
                ty: TypeExpr::named("MissingType"),
                optional: false,
                readonly: false,
            })],
        }))),
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
        declaration_id: 0,
        type_annotation: Some(TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "color".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        }))),
        kind: ValueDeclKind::Const,
        function_signature: None,
        object_shape: None,
    });

    // Simple generic: type Wrapper<T> = { value: T }
    env.add_type(TypeDeclInfo {
        name: "Wrapper".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "value".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        })),
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
    });

    // Config<SomeType>['cheap'] — should get `string` without touching `expensive`
    let expr = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::named_with_args(
            "Config",
            vec![TypeExpr::named("SomeArg")],
        )),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("cheap".to_string()))),
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                readonly: false,
            })],
        })),
    });

    // Config[keyof Config] — non-literal index, should still work via fallback
    let expr = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::named("Config")),
        index: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("Config")))),
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "header".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        })),
    });

    env.add_type(TypeDeclInfo {
        name: "ComponentConfig".to_string(),
        declaration_id: 0,
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
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
    });

    env.add_type(TypeDeclInfo {
        name: "Accordion".to_string(),
        declaration_id: 0,
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
            object: Arc::new(TypeExpr::named("Accordion")),
            index: Arc::new(TypeExpr::Literal(LiteralValue::String("slots".to_string()))),
        },
        &mut env,
    );

    assert!(
        env.steps() < 160,
        "lazy indexed access should not evaluate unrelated expensive generic args (steps={})",
        env.steps()
    );
    assert!(
        matches!(result, TypeExpr::Ref { ref name, .. } if &**name == "ComponentSlots"),
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
    });

    let result = evaluate(
        &TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named_with_args(
                "Required",
                vec![TypeExpr::named("Config")],
            )),
            index: Arc::new(TypeExpr::Literal(LiteralValue::String("cheap".to_string()))),
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

/// Generic-bound indexed access should still use the lazy path when the
/// binding is an intersection and only one branch contributes the target key.
#[test]
fn lazy_indexed_access_through_bound_intersection_skips_unrelated_expensive_branches() {
    let mut env = EvalEnv::new();
    env.limits.max_depth = 2048;
    env.limits.max_steps = 20_000;
    let expensive_name = add_deep_alias_chain(&mut env, "ExpensiveBoundLayer", 512);

    env.type_bindings.insert(
        "T".to_string(),
        Arc::new(TypeExpr::Intersection(Arc::from(vec![
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "padding".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: true,
                    readonly: false,
                })],
            })),
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "side".to_string(),
                    ty: TypeExpr::named(&expensive_name),
                    optional: true,
                    readonly: false,
                })],
            })),
        ]))),
    );

    let result = evaluate(
        &TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("T")),
            index: Arc::new(TypeExpr::Literal(LiteralValue::String(
                "padding".to_string(),
            ))),
        },
        &mut env,
    );

    assert!(
        matches!(result, TypeExpr::Primitive(PrimitiveName::String)),
        "bound-intersection indexed access should resolve to string, got: {:?}",
        result
    );
    assert!(
        env.steps() < 120,
        "bound-intersection lazy indexed access should skip unrelated expensive branches (steps={})",
        env.steps()
    );
}

/// Pick/Omit projection should project directly from a bound generic surface
/// instead of eagerly evaluating omitted branches first.
#[test]
fn pick_from_bound_intersection_skips_unrelated_expensive_branches() {
    let mut env = EvalEnv::new();
    env.limits.max_depth = 2048;
    env.limits.max_steps = 20_000;
    let expensive_name = add_deep_alias_chain(&mut env, "ExpensiveProjectedLayer", 512);

    env.type_bindings.insert(
        "T".to_string(),
        Arc::new(TypeExpr::Intersection(Arc::from(vec![
            TypeExpr::Object(Arc::new(ObjectExpr {
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
            })),
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "extra".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                    optional: false,
                    readonly: false,
                })],
            })),
        ]))),
    );

    let result = evaluate(
        &TypeExpr::named_with_args(
            "Pick",
            vec![
                TypeExpr::named("T"),
                TypeExpr::Literal(LiteralValue::String("keep".to_string())),
            ],
        ),
        &mut env,
    );

    match &result {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(names, vec!["keep"]);
        }
        _ => panic!("expected object, got {result:?}"),
    }
    assert!(
        env.steps() < 120,
        "Pick from bound intersection should skip unrelated expensive branches (steps={})",
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Method(MethodSignature {
                    name: "render".to_string(),
                    function: FunctionExpr {
                        parameters: vec![],
                        return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
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
        })),
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Method(MethodSignature {
                    name: "render".to_string(),
                    function: FunctionExpr {
                        parameters: vec![],
                        return_type: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
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
        })),
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
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
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::union(vec![
            TypeExpr::Literal(LiteralValue::String("replace".to_string())),
            TypeExpr::Literal(LiteralValue::String("activeClass".to_string())),
            TypeExpr::Literal(LiteralValue::String("ariaCurrentValue".to_string())),
        ]),
    });
    env.add_type(TypeDeclInfo {
        name: "RouterLinkOptions".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
    });
    env.add_type(TypeDeclInfo {
        name: "LinkProps".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::intersection(vec![
            TypeExpr::named("RouterLinkOptions"),
            TypeExpr::Object(Arc::new(ObjectExpr {
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
            })),
        ]),
    });
    env.add_type(TypeDeclInfo {
        name: "UseComponentIconsProps".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
    });
    env.add_type(TypeDeclInfo {
        name: "ButtonProps".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::intersection(vec![
            TypeExpr::named("UseComponentIconsProps"),
            TypeExpr::named_with_args(
                "Omit",
                vec![
                    TypeExpr::named("LinkProps"),
                    TypeExpr::union(vec![
                        TypeExpr::Literal(LiteralValue::String("raw".to_string())),
                        TypeExpr::Literal(LiteralValue::String("custom".to_string())),
                    ]),
                ],
            ),
            TypeExpr::Object(Arc::new(ObjectExpr {
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
            })),
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

#[test]
fn self_bound_generic_reference_stays_symbolic() {
    let mut env = EvalEnv::new();
    env.type_bindings
        .insert("T".to_string(), Arc::new(TypeExpr::named("T")));

    let result = evaluate(&TypeExpr::named("T"), &mut env);

    assert_eq!(result, TypeExpr::named("T"));
    assert!(
        env.steps() < 8,
        "self-bound generic references should not recurse indefinitely (steps={})",
        env.steps()
    );
}

#[test]
fn self_bound_generic_component_config_body_keeps_slots_surface() {
    let mut env = parse_and_build_env(
        r#"
type Id<T> = {} & { [P in keyof T]: T[P] }
type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
}>
type ComponentConfig<T extends { slots?: Record<string, any> }> = {
  slots: ComponentSlots<T>
}
"#,
    );
    let decl = env
        .type_symbols
        .get("ComponentConfig")
        .expect("ComponentConfig decl should exist")
        .clone();
    env.type_bindings
        .insert("T".to_string(), Arc::new(TypeExpr::named("T")));

    let result = evaluate(&decl.body, &mut env);

    match result {
        TypeExpr::Object(obj) => {
            let slots = obj.properties.iter().find_map(|member| match member {
                ObjectMember::Property(prop) if prop.name == "slots" => Some(prop.ty.clone()),
                _ => None,
            });
            assert!(
                slots.is_some(),
                "expected slots property on ComponentConfig"
            );
        }
        _ => panic!("expected object result, got {result:?}"),
    }
    assert!(
        env.steps() < 256,
        "symbolic generic component config evaluation should remain bounded (steps={})",
        env.steps()
    );
}
