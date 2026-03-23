use super::*;
use crate::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
use crate::type_expr::{
    MappedModifier, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr, TypeParam,
};

/// Helper to build an EvalEnv with the given type declarations.
fn env_with_types(types: Vec<TypeDeclInfo>) -> EvalEnv {
    let mut env = EvalEnv::new();
    for decl in types {
        env.add_type(decl);
    }
    env
}

fn default_budget() -> ExpansionBudget {
    ExpansionBudget::default()
}

#[test]
fn apply_expansion_budget_resets_counters_for_fresh_expansion() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::Conditional {
        check: Box::new(TypeExpr::Primitive(PrimitiveName::String)),
        extends: Box::new(TypeExpr::Primitive(PrimitiveName::String)),
        true_type: Box::new(TypeExpr::string_literal("yes")),
        false_type: Box::new(TypeExpr::string_literal("no")),
    };

    let _ = expand_normalized_expr(&expr, &mut env, &default_budget());
    assert!(
        env.steps() > 0,
        "first expansion should consume some budget"
    );

    env.apply_expansion_budget(&default_budget());
    assert_eq!(
        env.steps(),
        0,
        "fresh top-level expansions should start with a fresh step budget"
    );
}

// ===========================================================================
// ObjectShape expansion
// ===========================================================================

#[test]
fn expand_object_shape_simple_interface() {
    // interface User { name: string; age: number; active?: boolean }
    let user = TypeDeclInfo {
        name: "User".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "name".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "age".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "active".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                    optional: true,
                    readonly: false,
                }),
            ],
        }),
    };
    let mut env = env_with_types(vec![user]);
    let result = expand_object_shape(&TypeExpr::named("User"), &mut env, &default_budget());

    assert!(result.is_exact(), "simple interface should be exact");
    assert_eq!(result.value.properties.len(), 3, "should have 3 properties");
    assert_eq!(result.value.properties[0].name, "name");
    assert_eq!(
        result.value.properties[0].ty,
        TypeExpr::Primitive(PrimitiveName::String)
    );
    assert!(!result.value.properties[0].optional);
    assert_eq!(result.value.properties[2].name, "active");
    assert!(result.value.properties[2].optional);
    assert!(
        result.diagnostics.is_empty(),
        "no diagnostics for exact result"
    );
}

#[test]
fn expand_object_shape_generic_instantiation() {
    // type Wrapper<T> = { value: T; label: string }
    let wrapper = TypeDeclInfo {
        name: "Wrapper".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "value".to_string(),
                    ty: TypeExpr::named("T"),
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
    };
    let mut env = env_with_types(vec![wrapper]);
    let expr =
        TypeExpr::named_with_args("Wrapper", vec![TypeExpr::Primitive(PrimitiveName::Number)]);
    let result = expand_object_shape(&expr, &mut env, &default_budget());

    assert!(result.is_exact());
    assert_eq!(result.value.properties.len(), 2);
    assert_eq!(result.value.properties[0].name, "value");
    assert_eq!(
        result.value.properties[0].ty,
        TypeExpr::Primitive(PrimitiveName::Number),
        "T should be instantiated to number"
    );
}

#[test]
fn expand_object_shape_mapped_type() {
    // type Flags = { [K in "a" | "b" | "c"]: boolean }
    let flags = TypeDeclInfo {
        name: "Flags".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Mapped {
            parameter: "K".to_string(),
            source: Box::new(TypeExpr::Union(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
                TypeExpr::string_literal("c"),
            ])),
            value: Box::new(TypeExpr::Primitive(PrimitiveName::Boolean)),
            optional: MappedModifier::None,
            readonly: MappedModifier::None,
            name_type: None,
        },
    };
    let mut env = env_with_types(vec![flags]);
    let result = expand_object_shape(&TypeExpr::named("Flags"), &mut env, &default_budget());

    assert!(result.is_exact());
    assert_eq!(result.value.properties.len(), 3, "should have 3 properties");
    let names: Vec<&str> = result
        .value
        .properties
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"b"));
    assert!(names.contains(&"c"));
    for prop in &result.value.properties {
        assert_eq!(prop.ty, TypeExpr::Primitive(PrimitiveName::Boolean));
    }
}

#[test]
fn expand_object_shape_indeterminate_conditional_no_hang() {
    // type T<X> = X extends SomeRef ? { a: string } : { b: number }
    // SomeRef is unresolved — conditional is indeterminate
    let t = TypeDeclInfo {
        name: "T".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "X".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Conditional {
            check: Box::new(TypeExpr::named("X")),
            extends: Box::new(TypeExpr::named("SomeRef")),
            true_type: Box::new(TypeExpr::Object(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "a".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                })],
            })),
            false_type: Box::new(TypeExpr::Object(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "b".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    readonly: false,
                })],
            })),
        },
    };
    let mut env = env_with_types(vec![t]);
    let expr = TypeExpr::named_with_args("T", vec![TypeExpr::named("UnknownType")]);

    let start = std::time::Instant::now();
    let result = expand_object_shape(&expr, &mut env, &default_budget());
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 100,
        "should not hang — completed in {}ms",
        elapsed.as_millis()
    );
    assert!(
        !result.is_exact(),
        "indeterminate conditional should be partial"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.reason == ExpansionStopReason::IndeterminateConditional),
        "should have IndeterminateConditional diagnostic"
    );
}

#[test]
fn expand_object_shape_nested_mapped_depth_limit() {
    // 3 levels of nested mapped types, each with 4 keys
    // Budget: max_mapped_depth = 2
    // Outer and middle levels expand; innermost preserved as Mapped
    let keys = TypeExpr::Union(vec![
        TypeExpr::string_literal("x"),
        TypeExpr::string_literal("y"),
        TypeExpr::string_literal("z"),
        TypeExpr::string_literal("w"),
    ]);

    // Level 3 (innermost): { [I in Keys]: string }
    let level3 = TypeExpr::Mapped {
        parameter: "I".to_string(),
        source: Box::new(keys.clone()),
        value: Box::new(TypeExpr::Primitive(PrimitiveName::String)),
        optional: MappedModifier::None,
        readonly: MappedModifier::None,
        name_type: None,
    };

    // Level 2: { [J in Keys]: Level3 }
    let level2 = TypeExpr::Mapped {
        parameter: "J".to_string(),
        source: Box::new(keys.clone()),
        value: Box::new(level3),
        optional: MappedModifier::None,
        readonly: MappedModifier::None,
        name_type: None,
    };

    // Level 1 (outermost): { [K in Keys]: Level2 }
    let level1_type = TypeDeclInfo {
        name: "Nested".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Mapped {
            parameter: "K".to_string(),
            source: Box::new(keys),
            value: Box::new(level2),
            optional: MappedModifier::None,
            readonly: MappedModifier::None,
            name_type: None,
        },
    };

    let mut env = env_with_types(vec![level1_type]);
    let mut budget = default_budget();
    budget.max_mapped_depth = 2;

    let result = expand_object_shape(&TypeExpr::named("Nested"), &mut env, &budget);

    // Outer level should be expanded (4 properties)
    assert_eq!(
        result.value.properties.len(),
        4,
        "outer level should have 4 properties"
    );
    // Result should be partial because innermost level was not expanded
    assert!(
        !result.is_exact(),
        "should be partial due to mapped depth limit"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.reason == ExpansionStopReason::MappedDepthExceeded),
        "should have MappedDepthExceeded diagnostic"
    );
}

#[test]
fn expand_object_shape_intersection_merge() {
    // { a: string } & { b: number }
    let expr = TypeExpr::Intersection(vec![
        TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        }),
        TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "b".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                readonly: false,
            })],
        }),
    ]);

    let mut env = EvalEnv::new();
    let result = expand_object_shape(&expr, &mut env, &default_budget());

    assert!(result.is_exact());
    assert_eq!(result.value.properties.len(), 2);
    let names: Vec<&str> = result
        .value
        .properties
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"b"));
}

#[test]
fn expand_object_shape_union_variants_vue_merge() {
    // { a: string; b: number } | { a: string; c: boolean }
    // Vue merge semantics: a = required, b = optional, c = optional
    let expr = TypeExpr::Union(vec![
        TypeExpr::Object(ObjectExpr {
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
        TypeExpr::Object(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "a".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "c".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    ]);

    let mut env = EvalEnv::new();
    let result = expand_object_shape(&expr, &mut env, &default_budget());

    assert!(result.is_exact());
    assert_eq!(result.value.properties.len(), 3);

    let a = result
        .value
        .properties
        .iter()
        .find(|p| p.name == "a")
        .unwrap();
    assert!(
        !a.optional,
        "a is present in all variants, should be required"
    );

    let b = result
        .value
        .properties
        .iter()
        .find(|p| p.name == "b")
        .unwrap();
    assert!(b.optional, "b is only in one variant, should be optional");

    let c = result
        .value
        .properties
        .iter()
        .find(|p| p.name == "c")
        .unwrap();
    assert!(c.optional, "c is only in one variant, should be optional");
}

// ===========================================================================
// Pathological cases
// ===========================================================================

#[test]
fn expand_recursive_generic_terminates() {
    // type Tree<T> = { value: T; children: Tree<T>[] }
    let tree = TypeDeclInfo {
        name: "Tree".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        body: TypeExpr::Object(ObjectExpr {
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
                        element: Box::new(TypeExpr::named_with_args(
                            "Tree",
                            vec![TypeExpr::named("T")],
                        )),
                        readonly: false,
                    },
                    optional: false,
                    readonly: false,
                }),
            ],
        }),
    };
    let mut env = env_with_types(vec![tree]);
    let expr = TypeExpr::named_with_args("Tree", vec![TypeExpr::Primitive(PrimitiveName::String)]);

    let start = std::time::Instant::now();
    let result = expand_object_shape(&expr, &mut env, &default_budget());
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1000,
        "recursive type should terminate — took {}ms",
        elapsed.as_millis()
    );
    // Should have at least the two top-level properties
    assert!(
        result.value.properties.len() >= 2,
        "should have value and children properties"
    );
}

#[test]
fn expand_mapped_infinite_key_space_partial() {
    // { [K in string]: number } — infinite key space, cannot expand
    let expr = TypeExpr::Mapped {
        parameter: "K".to_string(),
        source: Box::new(TypeExpr::Primitive(PrimitiveName::String)),
        value: Box::new(TypeExpr::Primitive(PrimitiveName::Number)),
        optional: MappedModifier::None,
        readonly: MappedModifier::None,
        name_type: None,
    };

    let mut env = EvalEnv::new();
    let result = expand_object_shape(&expr, &mut env, &default_budget());

    assert!(!result.is_exact(), "infinite key space should be partial");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.reason == ExpansionStopReason::InfiniteKeySpace),
        "should have InfiniteKeySpace diagnostic"
    );
    // Should preserve as index signature
    assert!(
        !result.value.index_signatures.is_empty() || !result.value.properties.is_empty(),
        "should preserve some representation of the mapped type"
    );
}

#[test]
fn expand_intersection_conflicting_optionality() {
    // { a?: string } & { a: string }
    // Intersection removes optionality — a should be required
    let expr = TypeExpr::Intersection(vec![
        TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: true,
                readonly: false,
            })],
        }),
        TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        }),
    ]);

    let mut env = EvalEnv::new();
    let result = expand_object_shape(&expr, &mut env, &default_budget());

    assert!(result.is_exact());
    assert_eq!(result.value.properties.len(), 1);
    assert!(
        !result.value.properties[0].optional,
        "intersection should make a required"
    );
}

// ===========================================================================
// NormalizedExpr expansion
// ===========================================================================

#[test]
fn expand_normalized_preserves_indeterminate_conditional() {
    // T extends U ? A : B where T and U are unresolved
    let expr = TypeExpr::Conditional {
        check: Box::new(TypeExpr::named("T")),
        extends: Box::new(TypeExpr::named("U")),
        true_type: Box::new(TypeExpr::Primitive(PrimitiveName::String)),
        false_type: Box::new(TypeExpr::Primitive(PrimitiveName::Number)),
    };

    let mut env = EvalEnv::new();
    let result = expand_normalized_expr(&expr, &mut env, &default_budget());

    // Should preserve the conditional node
    assert!(
        matches!(result.value.expr, TypeExpr::Conditional { .. }),
        "should preserve Conditional node, got {:?}",
        result.value.expr
    );
    assert!(
        !result.is_exact(),
        "indeterminate conditional should be partial"
    );

    // Branches should NOT be deeply evaluated (they should be the original types)
    if let TypeExpr::Conditional {
        true_type,
        false_type,
        ..
    } = &result.value.expr
    {
        assert_eq!(**true_type, TypeExpr::Primitive(PrimitiveName::String));
        assert_eq!(**false_type, TypeExpr::Primitive(PrimitiveName::Number));
    }
}

#[test]
fn expand_normalized_resolves_utility_types() {
    // Partial<{ a: string; b: number }> -> { a?: string; b?: number }
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
                optional: false,
                readonly: false,
            }),
        ],
    });
    let expr = TypeExpr::named_with_args("Partial", vec![obj]);

    let mut env = EvalEnv::new();
    let result = expand_normalized_expr(&expr, &mut env, &default_budget());

    assert!(result.is_exact());
    if let TypeExpr::Object(ref obj) = result.value.expr {
        assert_eq!(obj.properties.len(), 2);
        for member in &obj.properties {
            if let ObjectMember::Property(prop) = member {
                assert!(prop.optional, "Partial should make all properties optional");
            }
        }
    } else {
        panic!("expected Object, got {:?}", result.value.expr);
    }
}

#[test]
fn expand_normalized_marks_unresolved_reference_partial() {
    let mut env = EvalEnv::new();
    let result = expand_normalized_expr(&TypeExpr::named("Missing"), &mut env, &default_budget());

    assert!(!result.is_exact(), "unresolved refs must be partial");
    assert!(
        matches!(result.value.expr, TypeExpr::Ref { .. }),
        "unresolved refs should stay symbolic"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.reason == ExpansionStopReason::UnresolvedReference),
        "unresolved refs should surface a diagnostic"
    );
}

// ===========================================================================
// Determinism
// ===========================================================================

#[test]
fn expand_object_shape_deterministic_ordering() {
    // Run the same expansion twice — property order, diagnostics, completeness must match
    let expr = TypeExpr::Object(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty {
                name: "z".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            }),
            ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                readonly: false,
            }),
            ObjectMember::Property(ObjectProperty {
                name: "m".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                optional: true,
                readonly: false,
            }),
        ],
    });

    let mut env1 = EvalEnv::new();
    let result1 = expand_object_shape(&expr, &mut env1, &default_budget());
    let mut env2 = EvalEnv::new();
    let result2 = expand_object_shape(&expr, &mut env2, &default_budget());

    assert_eq!(
        result1, result2,
        "same input should produce identical output"
    );
}

// ===========================================================================
// Additional coverage (from review)
// ===========================================================================

#[test]
fn expand_normalized_resolves_determinate_conditional() {
    // string extends string ? "yes" : "no" → Literal("yes")
    let expr = TypeExpr::Conditional {
        check: Box::new(TypeExpr::Primitive(PrimitiveName::String)),
        extends: Box::new(TypeExpr::Primitive(PrimitiveName::String)),
        true_type: Box::new(TypeExpr::string_literal("yes")),
        false_type: Box::new(TypeExpr::string_literal("no")),
    };

    let mut env = EvalEnv::new();
    let result = expand_normalized_expr(&expr, &mut env, &default_budget());

    assert!(result.is_exact(), "resolved conditional should be exact");
    assert_eq!(
        result.value.expr,
        TypeExpr::string_literal("yes"),
        "string extends string should resolve to true branch"
    );
    // Negative: should NOT be a Conditional node
    assert!(
        !matches!(result.value.expr, TypeExpr::Conditional { .. }),
        "resolved conditional should not remain as Conditional"
    );
}

#[test]
fn expand_normalized_resolves_negative_conditional() {
    // number extends string ? "yes" : "no" → Literal("no")
    let expr = TypeExpr::Conditional {
        check: Box::new(TypeExpr::Primitive(PrimitiveName::Number)),
        extends: Box::new(TypeExpr::Primitive(PrimitiveName::String)),
        true_type: Box::new(TypeExpr::string_literal("yes")),
        false_type: Box::new(TypeExpr::string_literal("no")),
    };

    let mut env = EvalEnv::new();
    let result = expand_normalized_expr(&expr, &mut env, &default_budget());

    assert!(result.is_exact());
    assert_eq!(result.value.expr, TypeExpr::string_literal("no"));
}

#[test]
fn expand_object_shape_nested_object_preserves_types() {
    // { outer: { inner: string } }
    let expr = TypeExpr::Object(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty {
            name: "outer".to_string(),
            ty: TypeExpr::Object(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "inner".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                })],
            }),
            optional: false,
            readonly: false,
        })],
    });

    let mut env = EvalEnv::new();
    let result = expand_object_shape(&expr, &mut env, &default_budget());

    assert!(result.is_exact());
    assert_eq!(result.value.properties.len(), 1);
    assert_eq!(result.value.properties[0].name, "outer");
    // Property type should be preserved as-is (nested Object)
    if let TypeExpr::Object(inner_obj) = &result.value.properties[0].ty {
        assert_eq!(inner_obj.properties.len(), 1);
        if let ObjectMember::Property(p) = &inner_obj.properties[0] {
            assert_eq!(p.name, "inner");
            assert_eq!(p.ty, TypeExpr::Primitive(PrimitiveName::String));
        } else {
            panic!("expected Property member");
        }
    } else {
        panic!(
            "expected Object type for 'outer', got {:?}",
            result.value.properties[0].ty
        );
    }
}

#[test]
fn expand_object_shape_normalizes_inline_property_types() {
    let expr = TypeExpr::Object(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty {
            name: "config".to_string(),
            ty: TypeExpr::named_with_args(
                "Partial",
                vec![TypeExpr::Object(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "ready".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                        optional: false,
                        readonly: false,
                    })],
                })],
            ),
            optional: false,
            readonly: false,
        })],
    });

    let mut env = EvalEnv::new();
    let result = expand_object_shape(&expr, &mut env, &default_budget());

    assert!(result.is_exact());
    assert_eq!(result.value.properties.len(), 1);
    match &result.value.properties[0].ty {
        TypeExpr::Object(obj) => match &obj.properties[0] {
            ObjectMember::Property(prop) => {
                assert_eq!(prop.name, "ready");
                assert!(
                    prop.optional,
                    "Partial<T> should normalize inline property types"
                );
            }
            other => panic!("expected property member, got {other:?}"),
        },
        other => panic!("expected normalized object property type, got {other:?}"),
    }
}

#[test]
fn expand_object_shape_intersection_different_types() {
    // { a: string | number } & { a: string }
    // Should intersect the property types
    let expr = TypeExpr::Intersection(vec![
        TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Union(vec![
                    TypeExpr::Primitive(PrimitiveName::String),
                    TypeExpr::Primitive(PrimitiveName::Number),
                ]),
                optional: false,
                readonly: false,
            })],
        }),
        TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        }),
    ]);

    let mut env = EvalEnv::new();
    let result = expand_object_shape(&expr, &mut env, &default_budget());

    assert!(result.is_exact());
    assert_eq!(result.value.properties.len(), 1);
    // The type should be an intersection of both types
    assert!(
        matches!(result.value.properties[0].ty, TypeExpr::Intersection(_)),
        "different property types in intersection should produce Intersection type, got {:?}",
        result.value.properties[0].ty
    );
}

#[test]
fn expand_union_optional_in_some_required_in_others() {
    // { a?: string } | { a: string }
    // Vue semantics: a is present in both but optional in one → optional
    let expr = TypeExpr::Union(vec![
        TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: true,
                readonly: false,
            })],
        }),
        TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        }),
    ]);

    let mut env = EvalEnv::new();
    let result = expand_object_shape(&expr, &mut env, &default_budget());

    assert!(result.is_exact());
    assert_eq!(result.value.properties.len(), 1);
    assert!(
        result.value.properties[0].optional,
        "a should be optional because it's optional in one variant"
    );
}
