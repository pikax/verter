use super::type_eval::*;
use super::type_expr::*;
use std::hash::Hash;
use std::sync::Arc;

/// Build a chain of type aliases: A -> B -> C -> ... -> target_body.
fn build_ref_chain(env: &mut EvalEnv, names: &[&str], target_body: TypeExpr) {
    for (index, name) in names.iter().enumerate() {
        let body = if index + 1 < names.len() {
            TypeExpr::named(names[index + 1])
        } else {
            target_body.clone()
        };
        env.add_type(TypeDeclInfo {
            name: name.to_string(),
            declaration_id: 0,
            kind: TypeDeclKind::Alias,
            type_parameters: vec![],
            body,
        });
    }
}

#[test]
fn ref_depth_limit_caps_deep_chains() {
    let mut env = EvalEnv::new();
    env.limits.max_ref_depth = 5;
    let names: Vec<&str> = vec!["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];
    build_ref_chain(&mut env, &names, TypeExpr::Primitive(PrimitiveName::String));

    let result = evaluate(&TypeExpr::named("A"), &mut env);

    assert!(
        !matches!(result, TypeExpr::Primitive(PrimitiveName::String)),
        "deep chain should not fully resolve with ref_depth limit 5, got: {result:?}"
    );
    assert!(
        matches!(result, TypeExpr::Ref { .. }),
        "depth-limit fallback should stay symbolic, got: {result:?}"
    );
}

#[test]
fn ref_depth_does_not_affect_shallow_wide_shapes() {
    let mut env = EvalEnv::new();
    env.limits.max_ref_depth = 4;

    for index in 0..20 {
        let name = format!("Type{index}");
        env.add_type(TypeDeclInfo {
            name,
            declaration_id: 0,
            kind: TypeDeclKind::Alias,
            type_parameters: vec![],
            body: TypeExpr::Primitive(PrimitiveName::String),
        });
    }

    let properties: Vec<ObjectMember> = (0..20)
        .map(|index| {
            ObjectMember::Property(ObjectProperty {
                name: format!("prop{index}"),
                ty: TypeExpr::named(format!("Type{index}")),
                optional: false,
                readonly: false,
            })
        })
        .collect();

    env.add_type(TypeDeclInfo {
        name: "Wide".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr { properties })),
    });

    let result = evaluate(&TypeExpr::named("Wide"), &mut env);

    let TypeExpr::Object(obj) = &result else {
        panic!("expected object, got: {result:?}");
    };
    for member in &obj.properties {
        if let ObjectMember::Property(prop) = member {
            assert!(
                matches!(prop.ty, TypeExpr::Primitive(PrimitiveName::String)),
                "property {} should resolve fully, got: {:?}",
                prop.name,
                prop.ty
            );
        }
    }
}

#[test]
fn ref_depth_resets_after_evaluation() {
    let mut env = EvalEnv::new();
    env.limits.max_ref_depth = 5;
    let names: Vec<&str> = vec!["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];
    build_ref_chain(&mut env, &names, TypeExpr::Primitive(PrimitiveName::String));

    let _ = evaluate(&TypeExpr::named("A"), &mut env);

    assert_eq!(
        env.ref_depth(),
        0,
        "ref_depth should reset after evaluation"
    );
}

#[test]
fn ref_depth_resets_on_cycle_detection() {
    let mut env = EvalEnv::new();
    env.limits.max_ref_depth = 8;

    env.add_type(TypeDeclInfo {
        name: "X".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::named("Y"),
    });
    env.add_type(TypeDeclInfo {
        name: "Y".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::named("Z"),
    });
    env.add_type(TypeDeclInfo {
        name: "Z".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::named("X"),
    });

    let _ = evaluate(&TypeExpr::named("X"), &mut env);

    assert_eq!(env.ref_depth(), 0, "ref_depth should reset after cycles");
}

#[test]
fn ref_depth_with_recursive_generic_and_cache_stays_stable() {
    let mut env = EvalEnv::new();
    env.limits.max_ref_depth = 3;

    env.add_type(TypeDeclInfo {
        name: "Wrap".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "inner".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        })),
    });
    env.add_type(TypeDeclInfo {
        name: "Deep".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::named_with_args(
            "Wrap",
            vec![TypeExpr::named_with_args(
                "Wrap",
                vec![TypeExpr::named_with_args(
                    "Wrap",
                    vec![TypeExpr::named_with_args(
                        "Wrap",
                        vec![TypeExpr::named("T")],
                    )],
                )],
            )],
        ),
    });

    let result1 = evaluate(
        &TypeExpr::named_with_args("Deep", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );
    let result2 = evaluate(
        &TypeExpr::named_with_args("Deep", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );

    assert_eq!(result1, result2, "cached and fresh results must match");
    assert_eq!(
        env.ref_depth(),
        0,
        "ref_depth should reset after evaluation"
    );
}

#[test]
fn cache_key_requires_registered_decl_id() {
    let mut env = EvalEnv::new();
    let result = evaluate(&TypeExpr::named("Unknown"), &mut env);

    assert!(
        matches!(result, TypeExpr::Ref { .. }),
        "unregistered types should stay symbolic, got: {result:?}"
    );
}

#[test]
fn cache_hits_share_arc_children() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "Props".to_string(),
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
                    optional: true,
                    readonly: false,
                }),
            ],
        })),
    });

    let result1 = evaluate(&TypeExpr::named("Props"), &mut env);
    let result2 = evaluate(&TypeExpr::named("Props"), &mut env);

    assert_eq!(
        result1, result2,
        "cache hits should preserve value equality"
    );
    let (TypeExpr::Object(obj1), TypeExpr::Object(obj2)) = (&result1, &result2) else {
        panic!("expected repeated object results, got {result1:?} and {result2:?}");
    };
    assert!(
        Arc::ptr_eq(obj1, obj2),
        "cache hits should reuse the same Arc-backed object allocation"
    );
}

#[test]
fn cache_key_hash_consistency_preserves_hits() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "MyBox".to_string(),
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

    let steps_before = env.steps();
    let result1 = evaluate(
        &TypeExpr::named_with_args("MyBox", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );
    let steps_first = env.steps() - steps_before;

    let steps_before = env.steps();
    let result2 = evaluate(
        &TypeExpr::named_with_args("MyBox", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );
    let steps_second = env.steps() - steps_before;

    assert_eq!(result1, result2, "same generic instantiation must be equal");
    assert!(
        steps_second < steps_first,
        "second evaluation should hit cache (first={steps_first}, second={steps_second})"
    );
}

#[test]
fn interner_preserves_identical_args() {
    let mut env = EvalEnv::new();
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
                name: "val".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        })),
    });
    env.add_type(TypeDeclInfo {
        name: "Alias".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::named_with_args("Wrapper", vec![TypeExpr::named("T")]),
    });

    let result1 = evaluate(
        &TypeExpr::named_with_args("Wrapper", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );
    let result2 = evaluate(
        &TypeExpr::named_with_args("Alias", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );

    assert!(matches!(result1, TypeExpr::Object(_)));
    assert!(matches!(result2, TypeExpr::Object(_)));
}

#[test]
fn interner_keeps_distinct_args_distinct() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "Id".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::named("T"),
    });

    let result1 = evaluate(
        &TypeExpr::named_with_args("Id", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );
    let result2 = evaluate(
        &TypeExpr::named_with_args("Id", vec![TypeExpr::Primitive(PrimitiveName::Number)]),
        &mut env,
    );

    assert_ne!(
        result1, result2,
        "different args must produce different results"
    );
    assert!(matches!(
        result1,
        TypeExpr::Primitive(PrimitiveName::String)
    ));
    assert!(matches!(
        result2,
        TypeExpr::Primitive(PrimitiveName::Number)
    ));
}

#[test]
fn arc_clone_shares_identity() {
    let inner_obj = Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty {
                name: "x".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            }),
            ObjectMember::Property(ObjectProperty {
                name: "y".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                readonly: false,
            }),
        ],
    });
    let original = TypeExpr::Object(Arc::clone(&inner_obj));
    let cloned = original.clone();

    let (TypeExpr::Object(a), TypeExpr::Object(b)) = (&original, &cloned) else {
        panic!("expected object variants");
    };
    assert!(
        Arc::ptr_eq(a, b),
        "cloned Object values should share the same Arc allocation"
    );
}

#[test]
fn deep_type_graph_stays_bounded_with_cache() {
    let mut env = EvalEnv::new();

    for level in (0..4).rev() {
        let properties = if level == 3 {
            vec![ObjectMember::Property(ObjectProperty {
                name: "g".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })]
        } else {
            let prop_name = ["a", "c", "e"][level].to_string();
            let child_name = format!("Level{}", level + 1);
            vec![
                ObjectMember::Property(ObjectProperty {
                    name: prop_name,
                    ty: TypeExpr::named("T"),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: ["b", "d", "f"][level].to_string(),
                    ty: TypeExpr::named_with_args(&child_name, vec![TypeExpr::named("T")]),
                    optional: false,
                    readonly: false,
                }),
            ]
        };

        env.add_type(TypeDeclInfo {
            name: format!("Level{level}"),
            declaration_id: 0,
            kind: TypeDeclKind::Alias,
            type_parameters: vec![TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            }],
            body: TypeExpr::Object(Arc::new(ObjectExpr { properties })),
        });
    }

    let result = evaluate(
        &TypeExpr::named_with_args("Level0", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );
    assert!(matches!(result, TypeExpr::Object(_)));

    let steps_before = env.steps();
    let result2 = evaluate(
        &TypeExpr::named_with_args("Level0", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );
    let cache_steps = env.steps() - steps_before;

    assert_eq!(result, result2, "cached result must equal original");
    assert!(
        cache_steps < 5,
        "cache hit should be near-zero, got {cache_steps}"
    );
}

#[test]
fn serialization_roundtrip_preserves_arc_backed_shape() {
    use std::hash::{BuildHasher, Hasher as _};

    let original = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty {
                name: "name".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            }),
            ObjectMember::Property(ObjectProperty {
                name: "items".to_string(),
                ty: TypeExpr::Array {
                    element: Arc::new(TypeExpr::named("T")),
                    readonly: true,
                },
                optional: true,
                readonly: false,
            }),
        ],
    }));

    let json_str = serde_json::to_string(&original).expect("serialize should succeed");
    let roundtripped: TypeExpr =
        serde_json::from_str(&json_str).expect("deserialization should succeed");

    assert_eq!(
        original, roundtripped,
        "roundtrip should preserve structure"
    );

    let hash_orig = {
        let mut hasher = rustc_hash::FxBuildHasher.build_hasher();
        original.hash(&mut hasher);
        hasher.finish()
    };
    let hash_roundtripped = {
        let mut hasher = rustc_hash::FxBuildHasher.build_hasher();
        roundtripped.hash(&mut hasher);
        hasher.finish()
    };
    assert_eq!(
        hash_orig, hash_roundtripped,
        "hash should survive roundtrip"
    );
}

#[test]
fn structural_hash_equality_is_stable() {
    use std::hash::{BuildHasher, Hasher as _};

    let type_a = TypeExpr::Ref {
        name: Arc::from("MyType"),
        type_arguments: Arc::from(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Number),
        ]),
    };
    let type_b = TypeExpr::Ref {
        name: Arc::from("MyType"),
        type_arguments: Arc::from(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Number),
        ]),
    };

    let (TypeExpr::Ref { name: name_a, .. }, TypeExpr::Ref { name: name_b, .. }) =
        (&type_a, &type_b)
    else {
        unreachable!("constructed refs above")
    };
    assert!(
        !Arc::ptr_eq(name_a, name_b),
        "precondition: independent Arc allocations"
    );
    assert_eq!(
        type_a, type_b,
        "structurally equal types must compare equal"
    );

    let hash_a = {
        let mut hasher = rustc_hash::FxBuildHasher.build_hasher();
        type_a.hash(&mut hasher);
        hasher.finish()
    };
    let hash_b = {
        let mut hasher = rustc_hash::FxBuildHasher.build_hasher();
        type_b.hash(&mut hasher);
        hasher.finish()
    };
    assert_eq!(hash_a, hash_b, "equal types must produce equal hashes");
}

#[test]
fn ref_depth_fallback_reuses_interned_args() {
    let mut env = EvalEnv::new();
    env.limits.max_ref_depth = 0;

    env.add_type(TypeDeclInfo {
        name: "Wrap".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::named("T"),
    });

    let result1 = evaluate(
        &TypeExpr::named_with_args("Wrap", vec![TypeExpr::Primitive(PrimitiveName::Number)]),
        &mut env,
    );
    let result2 = evaluate(
        &TypeExpr::named_with_args("Wrap", vec![TypeExpr::Primitive(PrimitiveName::Number)]),
        &mut env,
    );

    let (
        TypeExpr::Ref {
            name: name1,
            type_arguments: args1,
        },
        TypeExpr::Ref {
            name: name2,
            type_arguments: args2,
        },
    ) = (&result1, &result2)
    else {
        panic!("expected repeated fallback refs, got {result1:?} and {result2:?}");
    };

    assert_eq!(&**name1, "Wrap");
    assert_eq!(&**name2, "Wrap");
    assert!(
        Arc::ptr_eq(args1, args2),
        "direct fallback should reuse the interned arg allocation"
    );
    assert_eq!(args1.len(), 1);
    assert!(matches!(
        args1[0],
        TypeExpr::Primitive(PrimitiveName::Number)
    ));
}

#[test]
fn bind_type_parameters_store_arc_bindings() {
    let mut env = EvalEnv::new();
    let decl = TypeDeclInfo {
        name: "Box".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::named("T"),
    };

    let saved = bind_type_parameters(
        &decl,
        &[TypeExpr::Primitive(PrimitiveName::String)],
        &mut env,
    );

    let binding = env.type_bindings.get("T").expect("binding should exist");
    let binding_arc: &Arc<TypeExpr> = binding;
    assert!(matches!(
        binding_arc.as_ref(),
        TypeExpr::Primitive(PrimitiveName::String)
    ));

    restore_type_parameters(saved, &mut env);
    assert!(
        !env.type_bindings.contains_key("T"),
        "restore should remove bindings introduced by the call"
    );
}

#[test]
fn cache_key_ptr_eq_fast_path_stays_fast() {
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
        body: TypeExpr::named("T"),
    });

    let _ = evaluate(
        &TypeExpr::named_with_args("Box", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );

    let steps_before = env.steps();
    let result = evaluate(
        &TypeExpr::named_with_args("Box", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );
    let steps = env.steps() - steps_before;

    assert!(matches!(result, TypeExpr::Primitive(PrimitiveName::String)));
    assert!(steps < 5, "cache hit should stay minimal, got {steps}");
}

#[test]
fn cache_key_structural_fallback_still_hits() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "Id".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::named("T"),
    });

    let result1 = evaluate(
        &TypeExpr::named_with_args("Id", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );
    let result2 = evaluate(
        &TypeExpr::named_with_args("Id", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );

    assert_eq!(
        result1, result2,
        "structural fallback should preserve cache hits"
    );
}

#[test]
fn cycle_cache_arc_interaction_stays_stable() {
    let mut env = EvalEnv::new();
    env.limits.max_ref_depth = 3;
    env.add_type(TypeDeclInfo {
        name: "Node".to_string(),
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
                    name: "value".to_string(),
                    ty: TypeExpr::named("T"),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "children".to_string(),
                    ty: TypeExpr::Array {
                        element: Arc::new(TypeExpr::named_with_args(
                            "Node",
                            vec![TypeExpr::named("T")],
                        )),
                        readonly: false,
                    },
                    optional: false,
                    readonly: false,
                }),
            ],
        })),
    });

    let result1 = evaluate(
        &TypeExpr::named_with_args("Node", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );
    let result2 = evaluate(
        &TypeExpr::named_with_args("Node", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );
    let result3 = evaluate(
        &TypeExpr::named_with_args("Node", vec![TypeExpr::Primitive(PrimitiveName::String)]),
        &mut env,
    );

    assert_eq!(result1, result2);
    assert_eq!(result2, result3);
    assert_eq!(
        env.ref_depth(),
        0,
        "ref_depth must reset after repeated recursion"
    );
}

#[test]
fn pathological_type_graph_does_not_explode() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "Leaf".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "val".to_string(),
                ty: TypeExpr::named("T"),
                optional: false,
                readonly: false,
            })],
        })),
    });
    env.add_type(TypeDeclInfo {
        name: "Mid".to_string(),
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
                    name: "left".to_string(),
                    ty: TypeExpr::named_with_args("Leaf", vec![TypeExpr::named("T")]),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "right".to_string(),
                    ty: TypeExpr::named_with_args("Leaf", vec![TypeExpr::named("T")]),
                    optional: false,
                    readonly: false,
                }),
            ],
        })),
    });

    let properties: Vec<ObjectMember> = (0..50)
        .map(|index| {
            ObjectMember::Property(ObjectProperty {
                name: format!("prop{index}"),
                ty: TypeExpr::named_with_args(
                    "Mid",
                    vec![TypeExpr::Primitive(PrimitiveName::String)],
                ),
                optional: false,
                readonly: false,
            })
        })
        .collect();

    env.add_type(TypeDeclInfo {
        name: "Big".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr { properties })),
    });

    let steps_before = env.steps();
    let result = evaluate(&TypeExpr::named("Big"), &mut env);
    let total_steps = env.steps() - steps_before;
    assert!(
        matches!(result, TypeExpr::Object(_)),
        "Big should resolve to Object"
    );

    let TypeExpr::Object(big_obj) = &result else {
        unreachable!("assert above ensures object result")
    };
    let mid_objects: Vec<&Arc<ObjectExpr>> = big_obj
        .properties
        .iter()
        .map(|member| match member {
            ObjectMember::Property(ObjectProperty {
                ty: TypeExpr::Object(mid),
                ..
            }) => mid,
            _ => panic!("expected every Big property to resolve to an object surface"),
        })
        .collect();
    let first_mid = mid_objects[0];
    assert!(
        mid_objects.iter().all(|mid| Arc::ptr_eq(first_mid, mid)),
        "all repeated Mid<string> instantiations should share one Arc-backed object"
    );

    let leaf_objects: Vec<&Arc<ObjectExpr>> = first_mid
        .properties
        .iter()
        .map(|member| match member {
            ObjectMember::Property(ObjectProperty {
                ty: TypeExpr::Object(leaf),
                ..
            }) => leaf,
            _ => panic!("expected Mid<string> properties to resolve to Leaf<string> objects"),
        })
        .collect();
    assert!(
        Arc::ptr_eq(leaf_objects[0], leaf_objects[1]),
        "repeated Leaf<string> children should share one Arc-backed object"
    );
    assert!(
        total_steps < 500,
        "pathological graph should stay bounded by cache, got {total_steps}"
    );

    let steps_before = env.steps();
    let result2 = evaluate(&TypeExpr::named("Big"), &mut env);
    let cache_steps = env.steps() - steps_before;
    assert_eq!(result, result2);
    assert!(
        cache_steps < 5,
        "re-evaluation should be near-zero, got {cache_steps}"
    );
}
