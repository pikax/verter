use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_type_engine::project_semantic_dispatch::lower::LowerReadShape;
use verter_type_engine::resolver_core::bare_name_resolve::DeclarationScopePayload;
use verter_type_engine::resolver_core::scope_shadowing::ScopeShadowing;
use verter_type_engine::semantic_query::{
    PrimitiveKind, ProjectionMode, ProjectionReductionContext, SemanticNodeData, SemanticQueryKey,
};
use verter_type_expr::{LiteralValue, PrimitiveName, TypeExpr, ValueRef};

use super::carrier_head_resolution_tests::{file_scope, host, upsert_ts};
use super::ProjectSemanticDispatch;

fn value_type(path: &[&str], args: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::TypeOf(ValueRef {
        path: path.iter().map(|part| (*part).to_owned()).collect(),
        type_args: args,
    })
}

fn named(name: &str, args: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: args.into(),
    }
}

fn signature(constructor: bool, generics: bool) -> TypeExpr {
    let mut function = verter_type_expr::FunctionExpr::synthetic(
        vec![verter_type_expr::FunctionParam::synthetic(
            Some("a".into()),
            value_type(&["x"], vec![]),
            false,
            false,
        )],
        Some(Arc::new(if generics {
            named("T", vec![])
        } else {
            value_type(&["x"], vec![])
        })),
        if generics {
            vec![verter_type_expr::TypeParam {
                name: "T".into(),
                constraint: Some(Arc::new(value_type(&["x"], vec![]))),
                default: Some(Arc::new(value_type(&["y"], vec![]))),
                is_const: false,
            }]
        } else {
            vec![]
        },
    );
    if generics {
        function.predicate = Some(Arc::new(verter_type_expr::TypePredicate {
            subject: verter_type_expr::TypePredicateSubject::Parameter(Arc::from("a")),
            asserts: false,
            ty: Some(Arc::new(value_type(&["x"], vec![]))),
        }));
    }
    if constructor {
        TypeExpr::ConstructorType(Arc::new(function))
    } else {
        TypeExpr::Function(Arc::new(function))
    }
}

fn mapped(source: TypeExpr, value: TypeExpr, name: Option<TypeExpr>) -> TypeExpr {
    TypeExpr::Mapped {
        parameter: "K".into(),
        source: Arc::new(source),
        value: Arc::new(value),
        optional: verter_type_expr::MappedModifier::None,
        readonly: verter_type_expr::MappedModifier::None,
        name_type: name.map(Arc::new),
    }
}

fn imported(qualifier: &[&str], args: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::ImportType {
        specifier: "./m".into(),
        qualifier: qualifier.iter().map(|s| (*s).into()).collect(),
        typeof_query: false,
        type_arguments: args.into(),
    }
}

#[test]
fn type_lowering_preserves_read_order_shapes_and_substitutions() {
    use LowerReadShape::{Node, Whole};
    let host = host();
    upsert_ts(
        &host,
        "/lower.ts",
        concat!(
            "export declare const x: number;\n",
            "export declare const y: string;\n",
            "export declare const obj: { a: string };\n",
            "export declare const key: 'a';\n",
            "export type Box<T> = { value: T };\n",
            "export declare class C { static make<T>(value: T): T; }\n",
            "export declare const f: { tag: 'kept'; <T>(value: T): T; new<T>(value: T): { value: T } };\n",
            "export declare function overloaded<T>(value: T): T;\n",
            "export declare function overloaded<T>(value: T, second: T): T[];\n",
        ),
    );
    upsert_ts(
        &host,
        "/m.ts",
        "export type A<T> = { value: T }; export type Nest = { inner: string }; export declare function f<T>(arg: T): T;",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    let scope = file_scope(&dispatch, "/lower.ts");
    let payload = dispatch
        .ctx
        .prepared_decl_bundle("/lower.ts")
        .map(|bundle| {
            DeclarationScopePayload::from_bundle(
                &bundle,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
            )
        });
    let shadowing = ScopeShadowing::from_scope_payload(payload.as_ref());
    let names = FxHashMap::default();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    // A graph object bound to T exercises actual operator reduction and a
    // substitution without resolving a named declaration at the access.
    let mut ignored = Vec::new();
    let object_expr = TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
        empty_literal: false,
        properties: vec![verter_type_expr::ObjectMember::Property(
            verter_type_expr::ObjectProperty::synthetic_public_key(
                verter_type_expr::TypeAuthoredPropertyKey::string("a"),
                TypeExpr::Primitive(PrimitiveName::String),
                false,
                false,
            ),
        )],
    }));
    let object = dispatch.shallow_lower_type_expr_with_context_for_tests(
        &object_expr,
        &FxHashMap::default(),
        &scope,
        &names,
        payload.as_ref(),
        &shadowing,
        &mut ignored,
        context,
    );
    let mut env = FxHashMap::default();
    env.insert("T".to_owned(), object);
    let number = dispatch.shallow_lower_type_expr_with_context_for_tests(
        &TypeExpr::Primitive(PrimitiveName::Number),
        &FxHashMap::default(),
        &scope,
        &names,
        payload.as_ref(),
        &shadowing,
        &mut ignored,
        context,
    );
    env.insert("U".to_owned(), number);
    let cases = [
        (
            "mapped infer source preserves outer value",
            mapped(
                TypeExpr::Infer {
                    name: "U".into(),
                    constraint: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
                },
                named("U", vec![]),
                None,
            ),
            vec![],
        ),
        (
            "mapped infer source preserves outer remap",
            mapped(
                TypeExpr::Infer {
                    name: "U".into(),
                    constraint: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
                },
                TypeExpr::Primitive(PrimitiveName::String),
                Some(named("U", vec![])),
            ),
            vec![],
        ),
        (
            "mapped keyof infer binds value and remap",
            mapped(
                TypeExpr::KeyOf(Arc::new(TypeExpr::Infer {
                    name: "U".into(),
                    constraint: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
                })),
                named("U", vec![]),
                Some(named("U", vec![])),
            ),
            vec![],
        ),
        (
            "scope restoration",
            TypeExpr::Tuple {
                elements: [signature(false, true), named("T", vec![])]
                    .into_iter()
                    .map(|ty| verter_type_expr::TupleElement {
                        label: None,
                        ty,
                        optional: false,
                        rest: false,
                    })
                    .collect(),
                readonly: false,
            },
            vec![("TypeOf", Node); 6],
        ),
        (
            "function without binders or reads",
            TypeExpr::Function(Arc::new(verter_type_expr::FunctionExpr::synthetic(
                vec![],
                Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
                vec![],
            ))),
            vec![],
        ),
        (
            "constructor children",
            signature(true, false),
            vec![("TypeOf", Node); 2],
        ),
        (
            "function binder children",
            signature(false, true),
            vec![("TypeOf", Node); 6],
        ),
        (
            "constructor binder children",
            signature(true, true),
            vec![("TypeOf", Node); 6],
        ),
        (
            "mapped children",
            mapped(
                TypeExpr::KeyOf(Arc::new(value_type(&["obj"], vec![]))),
                value_type(&["x"], vec![]),
                Some(value_type(&["key"], vec![])),
            ),
            vec![
                ("TypeOf", Node),
                ("KeyOf", Node),
                ("TypeOf", Node),
                ("TypeOf", Node),
                ("MappedType", Node),
            ],
        ),
        (
            "mapped source",
            mapped(value_type(&["key"], vec![]), named("K", vec![]), None),
            vec![("TypeOf", Node), ("MappedType", Node)],
        ),
        (
            "mapped literal keys",
            mapped(TypeExpr::string_literal("a"), named("K", vec![]), None),
            vec![("MappedType", Node)],
        ),
        (
            "import argument",
            imported(&["A"], vec![value_type(&["x"], vec![])]),
            vec![
                ("ResolveDecl", Whole),
                ("TypeOf", Node),
                ("Instantiate", Whole),
            ],
        ),
        (
            "typeof import arguments",
            TypeExpr::ImportType {
                specifier: "./m".into(),
                qualifier: vec![Arc::from("f")].into(),
                typeof_query: true,
                type_arguments: vec![value_type(&["x"], vec![])].into(),
            },
            vec![("TypeOf", Node), ("TypeOf", Whole)],
        ),
        (
            "import tail",
            imported(&["Nest", "inner"], vec![]),
            vec![("ResolveDecl", Whole), ("ProjectPath", Whole)],
        ),
        (
            "conditional infer constraint",
            TypeExpr::Conditional {
                check: Arc::new(value_type(&["x"], vec![])),
                extends: Arc::new(TypeExpr::Infer {
                    name: "U".into(),
                    constraint: Some(Arc::new(value_type(&["x"], vec![]))),
                }),
                true_type: Arc::new(named("U", vec![])),
                false_type: Arc::new(TypeExpr::number_literal(0.0)),
            },
            vec![("TypeOf", Node); 2],
        ),
        (
            "function children",
            TypeExpr::Function(Arc::new(verter_type_expr::FunctionExpr::synthetic(
                vec![verter_type_expr::FunctionParam::synthetic(
                    Some("a".into()),
                    value_type(&["x"], vec![]),
                    false,
                    false,
                )],
                Some(Arc::new(value_type(&["x"], vec![]))),
                vec![],
            ))),
            vec![("TypeOf", Node), ("TypeOf", Node)],
        ),
        (
            "conditional operands",
            TypeExpr::Conditional {
                check: Arc::new(value_type(&["x"], vec![])),
                extends: Arc::new(value_type(&["x"], vec![])),
                true_type: Arc::new(TypeExpr::number_literal(1.0)),
                false_type: Arc::new(TypeExpr::number_literal(2.0)),
            },
            vec![("TypeOf", Node), ("TypeOf", Node)],
        ),
        (
            "typeof leaf",
            value_type(&["x"], vec![]),
            vec![("TypeOf", Node)],
        ),
        (
            "argument typeof",
            named("Box", vec![value_type(&["x"], vec![])]),
            vec![
                ("ResolveDecl", Whole),
                ("TypeOf", Node),
                ("Instantiate", Whole),
            ],
        ),
        (
            "applied callable object",
            value_type(&["f"], vec![TypeExpr::Primitive(PrimitiveName::String)]),
            vec![
                ("TypeOf", Node),
                ("SignaturesOfType", Whole),
                ("SignaturesOfType", Whole),
            ],
        ),
        (
            "applied overloads",
            value_type(
                &["overloaded"],
                vec![TypeExpr::Primitive(PrimitiveName::String)],
            ),
            vec![
                ("TypeOf", Node),
                ("SignaturesOfType", Whole),
                ("SignaturesOfType", Whole),
            ],
        ),
        (
            "applied typeof",
            value_type(
                &["C", "make"],
                vec![TypeExpr::Primitive(PrimitiveName::String)],
            ),
            vec![("TypeOf", Node)],
        ),
        (
            "generic head",
            named("Box", vec![TypeExpr::Primitive(PrimitiveName::String)]),
            vec![("ResolveDecl", Whole), ("Instantiate", Whole)],
        ),
        (
            "typeof argument read",
            value_type(&["C", "make"], vec![value_type(&["x"], vec![])]),
            vec![("TypeOf", Node), ("TypeOf", Node)],
        ),
        (
            "keyof",
            TypeExpr::KeyOf(Arc::new(named("T", vec![]))),
            vec![("KeyOf", Node)],
        ),
        (
            "indexed access",
            TypeExpr::IndexedAccess {
                object: Arc::new(named("T", vec![])),
                index: Arc::new(TypeExpr::Literal(LiteralValue::String("a".to_owned()))),
            },
            vec![("IndexedAccess", Node)],
        ),
        (
            "no read",
            TypeExpr::Primitive(PrimitiveName::String),
            vec![],
        ),
    ];
    for (label, expr, expected_reads) in cases {
        // Inspect the mapped descriptor before key enumeration so its value
        // and remap bindings remain observable.
        let context = if matches!(
            label,
            "mapped infer source preserves outer value"
                | "mapped infer source preserves outer remap"
                | "mapped keyof infer binds value and remap"
        ) {
            context.into_structural_transit_with_mode(ProjectionMode::Navigate)
        } else {
            context
        };
        let trace = dispatch.step_type_lowering_for_tests(
            &expr,
            &env,
            &scope,
            &names,
            payload.as_ref(),
            &shadowing,
            context,
        );
        let requested: Vec<_> = trace
            .requested
            .iter()
            .map(|(key, shape)| {
                let family = match key {
                    SemanticQueryKey::TypeOf { .. } => "TypeOf",
                    SemanticQueryKey::ResolveDecl { .. } => "ResolveDecl",
                    SemanticQueryKey::Instantiate { .. } => "Instantiate",
                    SemanticQueryKey::KeyOf { .. } => "KeyOf",
                    SemanticQueryKey::IndexedAccess { .. } => "IndexedAccess",
                    SemanticQueryKey::MappedType { .. } => "MappedType",
                    SemanticQueryKey::ProjectPath { .. } => "ProjectPath",
                    SemanticQueryKey::SignaturesOfType { .. } => "SignaturesOfType",
                    SemanticQueryKey::ReadSignatureResult(_) => "ReadSignatureResult",
                    other => panic!("unexpected lowering read: {other:?}"),
                };
                (family, *shape)
            })
            .collect();

        let value_reads: Vec<_> = trace
            .requested
            .iter()
            .filter_map(|(key, _)| match key {
                SemanticQueryKey::TypeOf { value_root, .. } => Some(value_root.root.name.as_str()),
                _ => None,
            })
            .collect();
        if label == "function binder children"
            || label == "constructor binder children"
            || label == "scope restoration"
        {
            assert_eq!(
                value_reads,
                ["x", "y", "x", "x", "y", "x"],
                "{label}: bounds precede parameters, published bounds and predicate"
            );
        }
        if label == "mapped children" {
            assert_eq!(
                value_reads,
                ["obj", "x", "key"],
                "source precedes value and remap"
            );
        }
        if label == "scope restoration" {
            let data = dispatch.graph().node_data(trace.node).unwrap();
            let SemanticNodeData::Tuple { elements, .. } = data.as_ref() else {
                panic!("tuple expected")
            };
            assert_eq!(
                elements[1].value, object,
                "the outer binding returns after the signature finishes"
            );
        }

        assert_eq!(requested, expected_reads, "{label}");
        assert_eq!(
            trace.first_advance_done,
            expected_reads.is_empty(),
            "{label}"
        );
        let mut substitutions = Vec::new();
        let inline = dispatch.shallow_lower_type_expr_with_context_for_tests(
            &expr,
            &env,
            &scope,
            &names,
            payload.as_ref(),
            &shadowing,
            &mut substitutions,
            context,
        );
        assert_eq!(trace.node, inline, "{label}");
        assert_eq!(trace.substitutions, substitutions, "{label}");
        assert_eq!(
            dispatch.graph().node_data(trace.node),
            dispatch.graph().node_data(inline),
            "{label}"
        );
        assert!(
            !matches!(
                dispatch.graph().node_data(trace.node).as_deref(),
                Some(SemanticNodeData::Opaque(_))
            ),
            "{label}"
        );
        if label == "applied callable object" || label == "applied overloads" {
            use verter_type_engine::semantic_query::{SignatureKind, SurfaceEntry};
            let kinds: Vec<_> = trace
                .requested
                .iter()
                .filter_map(|(key, _)| match key {
                    SemanticQueryKey::SignaturesOfType { kind, .. } => Some(*kind),
                    _ => None,
                })
                .collect();
            assert_eq!(
                kinds,
                [SignatureKind::Call, SignatureKind::Construct],
                "{label}"
            );
            let data = dispatch.graph().node_data(trace.node).unwrap();
            let SemanticNodeData::Object(surface) = data.as_ref() else {
                panic!("{label}: callable object expected: {data:?}")
            };
            let mut calls = 0;
            let mut constructs = 0;
            let mut members = 0;
            for entry in surface.entries().iter() {
                let signature = match entry {
                    SurfaceEntry::CallSignature(node) => {
                        calls += 1;
                        *node
                    }
                    SurfaceEntry::ConstructSignature(node) => {
                        constructs += 1;
                        *node
                    }
                    SurfaceEntry::Member(_) => {
                        members += 1;
                        continue;
                    }
                    _ => panic!("{label}: unexpected entry {entry:?}"),
                };
                let signature_data = dispatch.graph().node_data(signature).unwrap();
                let SemanticNodeData::Signature {
                    params,
                    type_parameters,
                    ..
                } = signature_data.as_ref()
                else {
                    panic!("{label}: signature expected")
                };
                assert!(
                    type_parameters.is_empty(),
                    "{label}: consumed clause removed"
                );
                for param in params.iter() {
                    assert!(
                        matches!(
                            dispatch.graph().node_data(param.ty).as_deref(),
                            Some(SemanticNodeData::Primitive(PrimitiveKind::String))
                        ),
                        "{label}: explicit argument substitutes each parameter"
                    );
                }
            }
            assert_eq!(
                (calls, constructs, members),
                if label == "applied callable object" {
                    (1, 1, 1)
                } else {
                    (2, 0, 0)
                },
                "{label}: retain overloads and non-signature members"
            );
        }
        if label == "mapped infer source preserves outer value"
            || label == "mapped infer source preserves outer remap"
            || label == "mapped keyof infer binds value and remap"
        {
            let data = dispatch.graph().node_data(trace.node).unwrap();
            let SemanticNodeData::Mapped { source, mapper } = data.as_ref() else {
                panic!("{label}: mapped carrier expected: {data:?}")
            };
            let expected = if label == "mapped keyof infer binds value and remap" {
                let source_data = dispatch.graph().node_data(*source).unwrap();
                let SemanticNodeData::Infer { name, binder, .. } = source_data.as_ref() else {
                    panic!("infer source expected")
                };
                assert_eq!(
                    dispatch.graph().node_data(mapper.value_expr).as_deref(),
                    Some(&SemanticNodeData::InferRef {
                        name: name.clone(),
                        binder: binder.clone(),
                    }),
                    "{label}"
                );
                assert_eq!(mapper.name_remap, Some(mapper.value_expr), "{label}");
                mapper.value_expr
            } else {
                let value = if label == "mapped infer source preserves outer value" {
                    mapper.value_expr
                } else {
                    mapper.name_remap.expect("mapped name remap")
                };
                assert_eq!(value, number, "{label}: outer U must remain number");
                value
            };
            let references = if mapper.name_remap == Some(mapper.value_expr) {
                2
            } else {
                1
            };
            assert_eq!(
                trace.substitutions,
                vec![(Arc::from("U"), expected); references],
                "{label}: preserve the selected binding's substitutions"
            );
        }
        if label == "indexed access" || label == "no read" {
            assert!(
                matches!(
                    dispatch.graph().node_data(trace.node).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::String))
                ),
                "{label}"
            );
        }
        if label == "keyof" || label == "indexed access" {
            assert_eq!(
                trace.substitutions,
                vec![(Arc::from("T"), object)],
                "{label}"
            );
        }
        if label == "applied typeof" || label == "typeof argument read" {
            let data = dispatch.graph().node_data(trace.node).unwrap();
            let SemanticNodeData::Signature {
                return_type,
                type_parameters,
                ..
            } = data.as_ref()
            else {
                panic!("applied typeof must yield a signature: {data:?}");
            };
            assert!(type_parameters.is_empty(), "{label}");
            let expected = if label == "applied typeof" {
                PrimitiveKind::String
            } else {
                PrimitiveKind::Number
            };
            assert_eq!(
                dispatch.graph().node_data(*return_type).as_deref(),
                Some(&SemanticNodeData::Primitive(expected)),
                "{label}"
            );
        }
        if label == "function binder children" || label == "constructor binder children" {
            let data = dispatch.graph().node_data(trace.node).unwrap();
            let SemanticNodeData::Signature {
                return_type,
                type_parameters,
                ..
            } = data.as_ref()
            else {
                panic!("signature expected")
            };
            assert_eq!(
                *return_type, type_parameters[0].param,
                "own binder must shadow outer T"
            );
            assert_ne!(
                *return_type, object,
                "outer substitution must not escape into the signature"
            );
        }
    }
}

/// What one lowering of `[typeof C.make<'x'>; elements]` on a fresh host
/// produced and paid: its node, the reads it made (stepped only), and the
/// substitution work the graph counted.
struct RepeatedInstantiation {
    /// Element nodes the tuple holds, and how many of them are distinct.
    elements: (usize, usize),
    template_reads: usize,
    child_visits: u64,
    memo_hits: u64,
    memo_misses: u64,
    lookups: u64,
}

fn lower_repeated_instantiation(elements: usize, stepped: bool) -> RepeatedInstantiation {
    let host = host();
    upsert_ts(
        &host,
        "/lower.ts",
        "export declare class C { static make<T extends string>(): `${T}`; }\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    let scope = file_scope(&dispatch, "/lower.ts");
    let payload = dispatch
        .ctx
        .prepared_decl_bundle("/lower.ts")
        .map(|bundle| {
            DeclarationScopePayload::from_bundle(
                &bundle,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
            )
        });
    let shadowing = ScopeShadowing::from_scope_payload(payload.as_ref());
    let names = FxHashMap::default();
    let env = FxHashMap::default();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let expr = TypeExpr::Tuple {
        elements: (0..elements)
            .map(|_| verter_type_expr::TupleElement {
                label: None,
                ty: value_type(&["C", "make"], vec![TypeExpr::string_literal("x")]),
                optional: false,
                rest: false,
            })
            .collect(),
        readonly: false,
    };
    let before = dispatch.graph().stats_snapshot();
    let (node, template_reads) = if stepped {
        let trace = dispatch.step_type_lowering_for_tests(
            &expr,
            &env,
            &scope,
            &names,
            payload.as_ref(),
            &shadowing,
            context,
        );
        let reads = trace
            .requested
            .iter()
            .filter(|(key, _)| matches!(key, SemanticQueryKey::TemplateLiteralReduce { .. }))
            .count();
        (trace.node, reads)
    } else {
        let mut substitutions = Vec::new();
        let node = dispatch.shallow_lower_type_expr_with_context_for_tests(
            &expr,
            &env,
            &scope,
            &names,
            payload.as_ref(),
            &shadowing,
            &mut substitutions,
            context,
        );
        (node, 0)
    };
    let after = dispatch.graph().stats_snapshot();
    let data = dispatch.graph().node_data(node).unwrap();
    let SemanticNodeData::Tuple {
        elements: lowered, ..
    } = data.as_ref()
    else {
        panic!("tuple expected: {data:?}")
    };
    for element in lowered.iter() {
        let signature = dispatch.graph().node_data(element.value).unwrap();
        let SemanticNodeData::Signature {
            return_type,
            type_parameters,
            ..
        } = signature.as_ref()
        else {
            panic!("an applied typeof yields a signature: {signature:?}")
        };
        assert!(type_parameters.is_empty(), "the clause is consumed");
        assert_eq!(
            dispatch.graph().node_data(*return_type).as_deref(),
            Some(&SemanticNodeData::Literal(LiteralValue::String(
                "x".to_owned()
            ))),
            "`${{T}}` with T = 'x' reduces to the literal"
        );
    }
    RepeatedInstantiation {
        elements: (
            lowered.len(),
            lowered
                .iter()
                .map(|element| element.value)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
        ),
        template_reads,
        child_visits: after.substitution_child_visits - before.substitution_child_visits,
        memo_hits: after.substitute_memo_hits - before.substitute_memo_hits,
        memo_misses: after.substitute_memo_misses - before.substitute_memo_misses,
        lookups: (after.hits + after.misses) - (before.hits + before.misses),
    }
}

/// An applied `typeof` lowered twice in one lowering instantiates once:
/// the first substitution walk, suspended at its template read, completes
/// inside the connected demand its driver holds, publishes its priced
/// memo entry, and the second element is served from it — with no second
/// walk and no second `TemplateLiteralReduce` read, in place and stepped.
#[test]
fn a_repeated_typeof_instantiation_reuses_its_published_substitution() {
    let single = lower_repeated_instantiation(1, false);
    let pair = lower_repeated_instantiation(2, false);
    let stepped = lower_repeated_instantiation(2, true);
    assert!(
        single.child_visits > 0,
        "the first element walks its substitution"
    );
    assert_eq!(
        pair.child_visits, single.child_visits,
        "the second element walks no substitution in place"
    );
    assert_eq!(
        pair.memo_misses, single.memo_misses,
        "the second element misses no substitution memo entry"
    );
    assert!(
        pair.memo_hits > single.memo_hits,
        "the second element is served by the published substitution"
    );
    assert_eq!(
        stepped.template_reads, 1,
        "the template is read once across both elements"
    );
    assert_eq!(
        (stepped.child_visits, stepped.memo_hits, stepped.memo_misses),
        (pair.child_visits, pair.memo_hits, pair.memo_misses),
        "stepped and in-place lowering pay the same substitution work"
    );
    assert_eq!(
        stepped.lookups, pair.lookups,
        "in-place lowering makes the reads the stepped lowering requests"
    );
    assert_eq!(single.elements, (1, 1));
    assert_eq!(
        (pair.elements, stepped.elements),
        ((2, 1), (2, 1)),
        "both elements are the one instantiated signature"
    );
}
