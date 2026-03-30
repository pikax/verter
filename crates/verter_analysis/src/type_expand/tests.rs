use super::*;
use crate::type_eval::{EvalEnv, EvalLookup, TypeDeclInfo, TypeDeclKind};
use crate::type_expr::{
    FunctionExpr, FunctionParam, MappedModifier, ObjectExpr, ObjectMember, ObjectProperty,
    PrimitiveName, TypeExpr, TypeParam,
};
use rustc_hash::FxHashMap;
use std::sync::Arc;

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

#[derive(Default)]
struct SlotReturnLookup {
    decls: FxHashMap<String, TypeDeclInfo>,
    root_identities: FxHashMap<String, (String, String)>,
}

impl EvalLookup for SlotReturnLookup {
    fn resolve_type_decl(&mut self, name: &str) -> Option<TypeDeclInfo> {
        self.decls.get(name).cloned()
    }

    fn resolve_type_root_identity(&mut self, name: &str) -> Option<(String, String)> {
        self.root_identities.get(name).cloned()
    }
}

fn object_decl(name: &str, members: Vec<ObjectMember>) -> TypeDeclInfo {
    TypeDeclInfo {
        name: name.to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: members,
        })),
    }
}

fn function_slot_decl(name: &str, return_type: TypeExpr) -> TypeDeclInfo {
    object_decl(
        name,
        vec![ObjectMember::Property(ObjectProperty {
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
                return_type: Some(Arc::new(return_type)),
                type_parameters: vec![],
            })),
            optional: false,
            readonly: false,
        })],
    )
}

fn vnode_like_decl(name: &str) -> TypeDeclInfo {
    object_decl(
        name,
        vec![ObjectMember::Property(ObjectProperty {
            name: "children".to_string(),
            ty: TypeExpr::Array {
                element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                readonly: false,
            },
            optional: true,
            readonly: false,
        })],
    )
}

fn slot_return_type(result: &ExpansionResult<ExpandedObjectShape>) -> &TypeExpr {
    let default_slot = result
        .value
        .properties
        .iter()
        .find(|prop| prop.name == "default")
        .expect("default slot should exist");
    let TypeExpr::Function(func) = &default_slot.ty else {
        panic!("default slot should be a function: {:?}", default_slot.ty);
    };
    func.return_type
        .as_deref()
        .expect("slot return type should exist")
}

#[test]
fn apply_expansion_budget_resets_counters_for_fresh_expansion() {
    let mut env = EvalEnv::new();
    let expr = TypeExpr::Conditional {
        check: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        true_type: Arc::new(TypeExpr::string_literal("yes")),
        false_type: Arc::new(TypeExpr::string_literal("no")),
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
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
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
        body: TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
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
fn expand_object_shape_normalizes_nested_script_generic_bindings() {
    let item = TypeDeclInfo {
        name: "Item".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "id".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        })),
    };
    let props = TypeDeclInfo {
        name: "Props".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![TypeParam {
            name: "U".to_string(),
            constraint: Some(Arc::new(TypeExpr::named("Item"))),
            default: Some(Arc::new(TypeExpr::named("Item"))),
        }],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "items".to_string(),
                    ty: TypeExpr::Array {
                        element: Arc::new(TypeExpr::named("U")),
                        readonly: false,
                    },
                    optional: true,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "selected".to_string(),
                    ty: TypeExpr::Conditional {
                        check: Arc::new(TypeExpr::named("U")),
                        extends: Arc::new(TypeExpr::Infer {
                            name: "Selected".to_string(),
                        }),
                        true_type: Arc::new(TypeExpr::named("Selected")),
                        false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
                    },
                    optional: true,
                    readonly: false,
                }),
            ],
        })),
    };
    let mut env = env_with_types(vec![item, props]);
    let script_generic = TypeParam {
        name: "T".to_string(),
        constraint: Some(Arc::new(TypeExpr::named("Item"))),
        default: Some(Arc::new(TypeExpr::named("Item"))),
    };
    env.type_bindings.insert(
        "T".to_string(),
        Arc::new(TypeExpr::type_parameter(script_generic.clone())),
    );

    let result = expand_object_shape(
        &TypeExpr::named_with_args("Props", vec![TypeExpr::named("T")]),
        &mut env,
        &default_budget(),
    );

    let items = result
        .value
        .properties
        .iter()
        .find(|prop| prop.name == "items")
        .expect("items property should exist");
    match &items.ty {
        TypeExpr::Array { element, .. } => match element.as_ref() {
            TypeExpr::TypeParameter(param) => {
                assert_eq!(param.name, "T");
                assert!(matches!(
                    param.constraint.as_deref(),
                    Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                ));
                assert!(matches!(
                    param.default.as_deref(),
                    Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                ));
            }
            other => {
                panic!("expected array element to preserve script generic metadata, got {other:?}")
            }
        },
        other => panic!("expected items prop to be an array, got {other:?}"),
    }

    let selected = result
        .value
        .properties
        .iter()
        .find(|prop| prop.name == "selected")
        .expect("selected property should exist");
    match &selected.ty {
        TypeExpr::TypeParameter(param) => {
            assert_eq!(param.name, "T");
            assert!(matches!(
                param.constraint.as_deref(),
                Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
            ));
        }
        other => panic!(
            "expected infer conditional to collapse to the bound type parameter, got {other:?}"
        ),
    }
}

#[test]
fn expand_object_shape_preserves_canonical_vue_vnode_slot_return_type_symbolically() {
    let slots = function_slot_decl(
        "Slots",
        TypeExpr::Array {
            element: Arc::new(TypeExpr::named("VNode")),
            readonly: false,
        },
    );
    let vnode = vnode_like_decl("VNode");
    let mut env = env_with_types(vec![slots, vnode]);
    env.preserve_canonical_vue_vnode_slot_returns = true;
    let mut lookup = SlotReturnLookup {
        root_identities: FxHashMap::from_iter([(
            "VNode".to_string(),
            (
                "/node_modules/vue/index.d.ts".to_string(),
                "VNode".to_string(),
            ),
        )]),
        ..SlotReturnLookup::default()
    };

    let result = expand_object_shape_with_lookup(
        &TypeExpr::named("Slots"),
        &mut env,
        &default_budget(),
        &mut lookup,
    );

    assert!(
        result.is_exact(),
        "symbolic vue slot return types should remain exact"
    );
    assert_eq!(
        slot_return_type(&result),
        &TypeExpr::Array {
            element: Arc::new(TypeExpr::named("VNode")),
            readonly: false,
        },
        "canonical vue VNode should stay symbolic in slot return types"
    );
    assert_ne!(
        slot_return_type(&result),
        &TypeExpr::Array {
            element: Arc::new(TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "children".to_string(),
                    ty: TypeExpr::Array {
                        element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                        readonly: false,
                    },
                    optional: true,
                    readonly: false,
                })],
            }))),
            readonly: false,
        },
        "canonical vue VNode slot returns must not expand into the full object body"
    );
}

#[test]
fn expand_object_shape_preserves_alias_imported_vue_vnode_slot_return_type_symbolically() {
    let slots = function_slot_decl(
        "Slots",
        TypeExpr::Array {
            element: Arc::new(TypeExpr::named("VueVNode")),
            readonly: false,
        },
    );
    let vnode = vnode_like_decl("VueVNode");
    let mut env = env_with_types(vec![slots, vnode]);
    env.preserve_canonical_vue_vnode_slot_returns = true;
    let mut lookup = SlotReturnLookup {
        root_identities: FxHashMap::from_iter([(
            "VueVNode".to_string(),
            (
                "/node_modules/vue/index.d.ts".to_string(),
                "VNode".to_string(),
            ),
        )]),
        ..SlotReturnLookup::default()
    };

    let result = expand_object_shape_with_lookup(
        &TypeExpr::named("Slots"),
        &mut env,
        &default_budget(),
        &mut lookup,
    );

    assert_eq!(
        slot_return_type(&result),
        &TypeExpr::Array {
            element: Arc::new(TypeExpr::named("VueVNode")),
            readonly: false,
        },
        "aliased vue VNode should stay symbolic in slot return types"
    );
    assert!(
        !matches!(slot_return_type(&result), TypeExpr::Object(_)),
        "aliased vue VNode slot return should not flatten to an object"
    );
}

#[test]
fn expand_object_shape_preserves_barrel_resolved_vue_vnode_slot_return_type_symbolically() {
    let slots = function_slot_decl(
        "Slots",
        TypeExpr::Array {
            element: Arc::new(TypeExpr::named("BarrelVNode")),
            readonly: false,
        },
    );
    let barrel = TypeDeclInfo {
        name: "BarrelVNode".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::named("VueVNode"),
    };
    let vue_vnode = vnode_like_decl("VueVNode");
    let mut env = env_with_types(vec![slots, barrel, vue_vnode]);
    env.preserve_canonical_vue_vnode_slot_returns = true;
    let mut lookup = SlotReturnLookup {
        root_identities: FxHashMap::from_iter([(
            "VueVNode".to_string(),
            (
                "/node_modules/vue/index.d.ts".to_string(),
                "VNode".to_string(),
            ),
        )]),
        ..SlotReturnLookup::default()
    };

    let result = expand_object_shape_with_lookup(
        &TypeExpr::named("Slots"),
        &mut env,
        &default_budget(),
        &mut lookup,
    );

    assert_eq!(
        slot_return_type(&result),
        &TypeExpr::Array {
            element: Arc::new(TypeExpr::named("BarrelVNode")),
            readonly: false,
        },
        "barrel-resolved vue VNode should stay symbolic in slot return types"
    );
    assert!(
        !matches!(slot_return_type(&result), TypeExpr::Array { element, .. } if matches!(element.as_ref(), TypeExpr::Object(_))),
        "barrel-resolved vue VNode slot return should not expand the underlying object"
    );
}

#[test]
fn expand_object_shape_keeps_same_name_local_vnode_slot_return_type_expandable() {
    let slots = function_slot_decl(
        "Slots",
        TypeExpr::Array {
            element: Arc::new(TypeExpr::named("VNode")),
            readonly: false,
        },
    );
    let vnode = vnode_like_decl("VNode");
    let mut env = env_with_types(vec![slots, vnode]);
    env.preserve_canonical_vue_vnode_slot_returns = true;
    let mut lookup = SlotReturnLookup::default();

    let result = expand_object_shape_with_lookup(
        &TypeExpr::named("Slots"),
        &mut env,
        &default_budget(),
        &mut lookup,
    );

    assert!(
        matches!(
            slot_return_type(&result),
            TypeExpr::Array { element, .. } if matches!(element.as_ref(), TypeExpr::Object(_))
        ),
        "same-name local VNode should still expand in slot return types"
    );
    assert_ne!(
        slot_return_type(&result),
        &TypeExpr::Array {
            element: Arc::new(TypeExpr::named("VNode")),
            readonly: false,
        },
        "same-name local VNode must not be short-circuited"
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
            source: Arc::new(TypeExpr::union(vec![
                TypeExpr::string_literal("a"),
                TypeExpr::string_literal("b"),
                TypeExpr::string_literal("c"),
            ])),
            value: Arc::new(TypeExpr::Primitive(PrimitiveName::Boolean)),
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
            check: Arc::new(TypeExpr::named("X")),
            extends: Arc::new(TypeExpr::named("SomeRef")),
            true_type: Arc::new(TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "a".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                })],
            }))),
            false_type: Arc::new(TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "b".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                    readonly: false,
                })],
            }))),
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
    let keys = TypeExpr::union(vec![
        TypeExpr::string_literal("x"),
        TypeExpr::string_literal("y"),
        TypeExpr::string_literal("z"),
        TypeExpr::string_literal("w"),
    ]);

    // Level 3 (innermost): { [I in Keys]: string }
    let level3 = TypeExpr::Mapped {
        parameter: "I".to_string(),
        source: Arc::new(keys.clone()),
        value: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        optional: MappedModifier::None,
        readonly: MappedModifier::None,
        name_type: None,
    };

    // Level 2: { [J in Keys]: Level3 }
    let level2 = TypeExpr::Mapped {
        parameter: "J".to_string(),
        source: Arc::new(keys.clone()),
        value: Arc::new(level3),
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
            source: Arc::new(keys),
            value: Arc::new(level2),
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
    let expr = TypeExpr::intersection(vec![
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        })),
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "b".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::Number),
                optional: false,
                readonly: false,
            })],
        })),
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
    let expr = TypeExpr::union(vec![
        TypeExpr::Object(Arc::new(ObjectExpr {
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
        TypeExpr::Object(Arc::new(ObjectExpr {
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
        })),
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
                            "Tree",
                            vec![TypeExpr::named("T")],
                        )),
                        readonly: false,
                    },
                    optional: false,
                    readonly: false,
                }),
            ],
        })),
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
        source: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        value: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
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
    let expr = TypeExpr::intersection(vec![
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: true,
                readonly: false,
            })],
        })),
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        })),
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
        check: Arc::new(TypeExpr::named("T")),
        extends: Arc::new(TypeExpr::named("U")),
        true_type: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
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
                optional: false,
                readonly: false,
            }),
        ],
    }));
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
    let expr = TypeExpr::Object(Arc::new(ObjectExpr {
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
    }));

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
        check: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        true_type: Arc::new(TypeExpr::string_literal("yes")),
        false_type: Arc::new(TypeExpr::string_literal("no")),
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
        check: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
        extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        true_type: Arc::new(TypeExpr::string_literal("yes")),
        false_type: Arc::new(TypeExpr::string_literal("no")),
    };

    let mut env = EvalEnv::new();
    let result = expand_normalized_expr(&expr, &mut env, &default_budget());

    assert!(result.is_exact());
    assert_eq!(result.value.expr, TypeExpr::string_literal("no"));
}

#[test]
fn expand_object_shape_nested_object_preserves_types() {
    // { outer: { inner: string } }
    let expr = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty {
            name: "outer".to_string(),
            ty: TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "inner".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                })],
            })),
            optional: false,
            readonly: false,
        })],
    }));

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
    let expr = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty {
            name: "config".to_string(),
            ty: TypeExpr::named_with_args(
                "Partial",
                vec![TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "ready".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                        optional: false,
                        readonly: false,
                    })],
                }))],
            ),
            optional: false,
            readonly: false,
        })],
    }));

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

fn add_deep_alias_chain(env: &mut EvalEnv, prefix: &str, depth: usize) -> String {
    let leaf = format!("{prefix}Leaf");
    env.add_type(TypeDeclInfo {
        name: leaf.clone(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Primitive(PrimitiveName::String),
    });

    let mut current = leaf;
    for index in (0..depth).rev() {
        let name = format!("{prefix}Layer{index}");
        env.add_type(TypeDeclInfo {
            name: name.clone(),
            declaration_id: 0,
            kind: TypeDeclKind::Alias,
            type_parameters: vec![],
            body: TypeExpr::named(current),
        });
        current = name;
    }

    current
}

#[test]
fn expand_object_shape_keeps_wide_utility_member_types_shallow() {
    let mut env = EvalEnv::new();
    let mut members = vec![
        ObjectMember::Property(ObjectProperty {
            name: "element".to_string(),
            ty: TypeExpr::named(add_deep_alias_chain(&mut env, "Element", 24)),
            optional: false,
            readonly: false,
        }),
        ObjectMember::Property(ObjectProperty {
            name: "content".to_string(),
            ty: TypeExpr::named(add_deep_alias_chain(&mut env, "Content", 24)),
            optional: false,
            readonly: false,
        }),
    ];

    for index in 0..6 {
        members.push(ObjectMember::Property(ObjectProperty {
            name: format!("prop{index}"),
            ty: TypeExpr::named(add_deep_alias_chain(&mut env, &format!("Prop{index}"), 24)),
            optional: false,
            readonly: false,
        }));
    }

    env.add_type(TypeDeclInfo {
        name: "EditorOptions".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: members,
        })),
    });

    let expr = TypeExpr::named_with_args(
        "Omit",
        vec![
            TypeExpr::named_with_args("Partial", vec![TypeExpr::named("EditorOptions")]),
            TypeExpr::union(vec![
                TypeExpr::string_literal("content"),
                TypeExpr::string_literal("element"),
            ]),
        ],
    );
    let mut budget = default_budget();
    budget.max_symbolic_work = 48;

    let result = expand_object_shape(&expr, &mut env, &budget);

    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.reason != ExpansionStopReason::BudgetExceeded),
        "utility shape extraction should keep wide member types shallow, got diagnostics: {:?}",
        result.diagnostics
    );
    assert_eq!(result.value.properties.len(), 6);
    assert!(
        result.value.properties.iter().all(|prop| prop.optional),
        "Partial<EditorOptions> should still make every prop optional"
    );
    assert!(
        result
            .value
            .properties
            .iter()
            .all(|prop| matches!(&prop.ty, TypeExpr::Ref { name, .. } if name.starts_with("Prop"))),
        "wide local member types should stay symbolic instead of resolving deep alias chains: {:?}",
        result.value.properties
    );
}

#[test]
fn expand_object_shape_normalizes_direct_union_alias_member_types() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "RouteLocationRaw".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "path".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                })],
            })),
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "name".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                })],
            })),
        ]),
    });
    env.add_type(TypeDeclInfo {
        name: "Props".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "to".to_string(),
                ty: TypeExpr::named("RouteLocationRaw"),
                optional: true,
                readonly: false,
            })],
        })),
    });

    let result = expand_object_shape(&TypeExpr::named("Props"), &mut env, &default_budget());
    let to = result
        .value
        .properties
        .iter()
        .find(|prop| prop.name == "to")
        .expect("to property should exist");

    match &to.ty {
        TypeExpr::Union(types) => assert!(
            types
                .iter()
                .any(|ty| matches!(ty, TypeExpr::Primitive(PrimitiveName::String))),
            "normalized union should retain the string route variant: {:?}",
            to.ty
        ),
        other => panic!("expected direct union alias member to normalize, got {other:?}"),
    }
}

#[test]
fn expand_object_shape_normalizes_short_ref_chain_member_types() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "Leaf".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::Primitive(PrimitiveName::String),
    });
    env.add_type(TypeDeclInfo {
        name: "Middle".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::named("Leaf"),
    });
    env.add_type(TypeDeclInfo {
        name: "Props".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "value".to_string(),
                ty: TypeExpr::named("Middle"),
                optional: true,
                readonly: false,
            })],
        })),
    });

    let result = expand_object_shape(&TypeExpr::named("Props"), &mut env, &default_budget());
    let value = result
        .value
        .properties
        .iter()
        .find(|prop| prop.name == "value")
        .expect("value property should exist");

    assert_eq!(
        value.ty,
        TypeExpr::Primitive(PrimitiveName::String),
        "short alias chains should still normalize to their concrete member type"
    );
}

#[test]
fn expand_object_shape_normalizes_direct_indexed_access_member_types() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "RouteLocationRaw".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "path".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                })],
            })),
        ]),
    });
    env.add_type(TypeDeclInfo {
        name: "Props".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty {
                    name: "to".to_string(),
                    ty: TypeExpr::named("RouteLocationRaw"),
                    optional: true,
                    readonly: false,
                }),
                ObjectMember::Property(ObjectProperty {
                    name: "href".to_string(),
                    ty: TypeExpr::IndexedAccess {
                        object: Arc::new(TypeExpr::named("Props")),
                        index: Arc::new(TypeExpr::string_literal("to")),
                    },
                    optional: true,
                    readonly: false,
                }),
            ],
        })),
    });

    let result = expand_object_shape(&TypeExpr::named("Props"), &mut env, &default_budget());
    let href = result
        .value
        .properties
        .iter()
        .find(|prop| prop.name == "href")
        .expect("href property should exist");

    match &href.ty {
        TypeExpr::Union(types) => assert!(
            types
                .iter()
                .any(|ty| matches!(ty, TypeExpr::Primitive(PrimitiveName::String))),
            "normalized indexed access should retain the string route variant: {:?}",
            href.ty
        ),
        other => panic!("expected indexed access member to normalize, got {other:?}"),
    }
}

#[test]
fn expand_object_shape_normalizes_indexed_access_through_short_alias_target_chain() {
    let mut env = EvalEnv::new();
    env.add_type(TypeDeclInfo {
        name: "RouteLocationRaw".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "path".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    readonly: false,
                })],
            })),
        ]),
    });
    env.add_type(TypeDeclInfo {
        name: "BaseProps".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "to".to_string(),
                ty: TypeExpr::named("RouteLocationRaw"),
                optional: true,
                readonly: false,
            })],
        })),
    });
    env.add_type(TypeDeclInfo {
        name: "Props".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: vec![],
        body: TypeExpr::named("BaseProps"),
    });
    env.add_type(TypeDeclInfo {
        name: "Wrapper".to_string(),
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters: vec![],
        body: TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "href".to_string(),
                ty: TypeExpr::IndexedAccess {
                    object: Arc::new(TypeExpr::named("Props")),
                    index: Arc::new(TypeExpr::string_literal("to")),
                },
                optional: true,
                readonly: false,
            })],
        })),
    });

    let result = expand_object_shape(&TypeExpr::named("Wrapper"), &mut env, &default_budget());
    let href = result
        .value
        .properties
        .iter()
        .find(|prop| prop.name == "href")
        .expect("href property should exist");

    match &href.ty {
        TypeExpr::Union(types) => assert!(
            types
                .iter()
                .any(|ty| matches!(ty, TypeExpr::Primitive(PrimitiveName::String))),
            "indexed access through a short alias chain should retain the string branch: {:?}",
            href.ty
        ),
        other => panic!("expected indexed access member to normalize, got {other:?}"),
    }
}

#[test]
fn expand_object_shape_intersection_different_types() {
    // { a: string | number } & { a: string }
    // Should intersect the property types
    let expr = TypeExpr::intersection(vec![
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::union(vec![
                    TypeExpr::Primitive(PrimitiveName::String),
                    TypeExpr::Primitive(PrimitiveName::Number),
                ]),
                optional: false,
                readonly: false,
            })],
        })),
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        })),
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
    let expr = TypeExpr::union(vec![
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: true,
                readonly: false,
            })],
        })),
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        })),
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
