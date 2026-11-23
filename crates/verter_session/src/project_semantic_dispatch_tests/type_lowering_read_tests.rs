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

/// The declarations the conditional fixtures name.
const CONDITIONAL_DECLARATIONS: &str = concat!(
    "export type Alias = number;\n",
    "export type A = string;\n",
    "export type B = string;\n",
    "export type Bag = { value: string; other: number };\n",
    "export type Open<T> = T extends number ? 1 : 2;\n",
);

fn conditional(
    check: TypeExpr,
    extends: TypeExpr,
    true_type: TypeExpr,
    false_type: TypeExpr,
) -> TypeExpr {
    TypeExpr::Conditional {
        check: Arc::new(check),
        extends: Arc::new(extends),
        true_type: Arc::new(true_type),
        false_type: Arc::new(false_type),
    }
}

fn tuple(elements: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::Tuple {
        elements: elements
            .into_iter()
            .map(|ty| verter_type_expr::TupleElement {
                label: None,
                ty,
                optional: false,
                rest: false,
            })
            .collect(),
        readonly: false,
    }
}

fn infer(name: &str) -> TypeExpr {
    TypeExpr::Infer {
        name: name.into(),
        constraint: None,
    }
}

fn type_parameter(name: &str) -> TypeExpr {
    TypeExpr::TypeParameter(verter_type_expr::TypeParam {
        name: name.into(),
        constraint: None,
        default: None,
        is_const: false,
    })
}

fn number_template() -> TypeExpr {
    TypeExpr::TemplateLiteral {
        quasis: vec![String::new(), String::new()],
        expressions: Arc::from(vec![TypeExpr::Primitive(PrimitiveName::Number)]),
    }
}

/// One conditional per source of the reads a decision makes: a `typeof`
/// check and a named extends operand, declared tuple elements, a template
/// tuple element, a generic alias application, a declared alias, an indexed
/// check, a `keyof` check, a template extends operand, a nested conditional
/// check, a written union arm, a repeated `infer`, an `any` check whose
/// selection takes both branches, and a check bound to a union, which
/// distributes.
fn conditional_fixtures() -> Vec<(&'static str, TypeExpr)> {
    let one = || TypeExpr::number_literal(1.0);
    let two = || TypeExpr::number_literal(2.0);
    let number = || TypeExpr::Primitive(PrimitiveName::Number);
    let string = || TypeExpr::Primitive(PrimitiveName::String);
    vec![
        (
            "conditional typeof check against an alias",
            conditional(
                value_type(&["x"], vec![]),
                named("Alias", vec![]),
                one(),
                two(),
            ),
        ),
        (
            "conditional tuple operands",
            conditional(
                tuple(vec![named("A", vec![])]),
                tuple(vec![named("B", vec![])]),
                one(),
                two(),
            ),
        ),
        (
            "conditional template tuple element",
            conditional(
                tuple(vec![number_template()]),
                tuple(vec![string()]),
                one(),
                two(),
            ),
        ),
        (
            "conditional alias application",
            conditional(
                named("Open", vec![type_parameter("V")]),
                number(),
                one(),
                two(),
            ),
        ),
        (
            "conditional declared alias",
            conditional(named("Alias", vec![]), number(), one(), two()),
        ),
        (
            "conditional indexed check",
            conditional(
                TypeExpr::IndexedAccess {
                    object: Arc::new(named("Bag", vec![])),
                    index: Arc::new(TypeExpr::string_literal("value")),
                },
                string(),
                one(),
                two(),
            ),
        ),
        (
            "conditional keyof check",
            conditional(
                TypeExpr::KeyOf(Arc::new(named("Bag", vec![]))),
                string(),
                one(),
                two(),
            ),
        ),
        (
            "conditional template extends",
            conditional(
                TypeExpr::string_literal("5"),
                number_template(),
                one(),
                two(),
            ),
        ),
        (
            "conditional nested check",
            conditional(
                TypeExpr::Parenthesized(Arc::new(conditional(
                    string(),
                    number(),
                    string(),
                    number(),
                ))),
                number(),
                one(),
                two(),
            ),
        ),
        (
            "conditional written arm",
            conditional(
                number(),
                TypeExpr::union(vec![number(), named("Missing", vec![])]),
                one(),
                two(),
            ),
        ),
        (
            "conditional repeated infer",
            conditional(
                tuple(vec![one(), two()]),
                tuple(vec![infer("U"), infer("U")]),
                named("U", vec![]),
                two(),
            ),
        ),
        (
            "conditional any check takes both branches",
            conditional(
                TypeExpr::Primitive(PrimitiveName::Any),
                tuple(vec![infer("U")]),
                tuple(vec![named("U", vec![])]),
                two(),
            ),
        ),
        (
            "conditional distributes over a bound union",
            conditional(
                type_parameter("T"),
                tuple(vec![infer("U")]),
                tuple(vec![named("U", vec![])]),
                two(),
            ),
        ),
    ]
}

#[test]
fn type_lowering_preserves_read_order_shapes_and_substitutions() {
    use LowerReadShape::{Node, Whole};
    let host = host();
    upsert_ts(
        &host,
        "/lower.ts",
        &[concat!(
            "export declare const x: number;\n",
            "export declare const y: string;\n",
            "export declare const obj: { a: string };\n",
            "export declare const key: 'a';\n",
            "export type Box<T> = { value: T };\n",
            "export declare class C { static make<T>(value: T): T; }\n",
            "export declare const f: { tag: 'kept'; <T>(value: T): T; new<T>(value: T): { value: T } };\n",
            "export declare function overloaded<T>(value: T): T;\n",
            "export declare function overloaded<T>(value: T, second: T): T[];\n",
        ), CONDITIONAL_DECLARATIONS].concat(),
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
    // A held decision's reads are the lowering's own whole reads, after
    // its operands' reads, in the order the decision makes them.
    let conditional_rows = conditional_fixtures()
        .into_iter()
        .map(|(label, expr)| (label, expr, conditional_reads(label).0));
    let mut read_mismatches = Vec::new();
    for (label, expr, expected_reads) in cases.into_iter().chain(conditional_rows) {
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
            .map(|(key, shape)| (format!("{:?}", key.tag()), *shape))
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

        let expected: Vec<_> = expected_reads
            .iter()
            .map(|(family, shape)| ((*family).to_owned(), *shape))
            .collect();
        if requested != expected {
            read_mismatches.push(format!("{label}: {requested:?}"));
        }
        if label.starts_with("conditional ") {
            let decision: Vec<_> = trace
                .requested
                .iter()
                .zip(&trace.from_decision)
                .filter(|(_, from_decision)| **from_decision)
                .map(|((key, shape), _)| {
                    assert_eq!(*shape, Whole, "{label}: a decision reads whole answers");
                    format!("{:?}", key.tag())
                })
                .collect();
            if decision != conditional_reads(label).1 {
                read_mismatches.push(format!("{label}: decision {decision:?}"));
            }
        }
        assert_eq!(trace.first_advance_done, requested.is_empty(), "{label}");
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
    assert!(
        read_mismatches.is_empty(),
        "requested reads differ: {read_mismatches:#?}"
    );
}

/// The reads lowering each conditional fixture requests, in order: its
/// operands' reads, then its decision's, as whole reads. The second list is
/// the decision's own reads, in the order the decision makes them.
#[allow(clippy::type_complexity)]
fn conditional_reads(label: &str) -> (Vec<(&'static str, LowerReadShape)>, Vec<&'static str>) {
    use LowerReadShape::{Node, Whole};
    match label {
        "conditional typeof check against an alias" => (
            vec![
                ("TypeOf", Node),
                ("ResolveDecl", Whole),
                ("Instantiate", Whole),
            ],
            vec!["ResolveDecl", "Instantiate"],
        ),
        "conditional tuple operands" => (
            vec![
                ("ResolveDecl", Whole),
                ("Instantiate", Whole),
                ("ResolveDecl", Whole),
                ("Instantiate", Whole),
            ],
            vec!["ResolveDecl", "Instantiate", "ResolveDecl", "Instantiate"],
        ),
        "conditional template tuple element" => (
            vec![("TemplateLiteralReduce", Whole); 3],
            vec!["TemplateLiteralReduce"; 3],
        ),
        "conditional alias application" => (
            vec![
                ("Instantiate", Whole),
                ("Instantiate", Whole),
                ("Conditional", Whole),
            ],
            vec!["Instantiate", "Instantiate", "Conditional"],
        ),
        "conditional declared alias" => (
            vec![("ResolveDecl", Whole), ("ResolveDecl", Whole)],
            vec!["ResolveDecl", "ResolveDecl"],
        ),
        "conditional indexed check" => (vec![("IndexedAccess", Whole)], vec!["IndexedAccess"]),
        "conditional keyof check" => (vec![("KeyOf", Node)], vec![]),
        "conditional template extends" => (
            vec![("TemplateLiteralReduce", Whole)],
            vec!["TemplateLiteralReduce"],
        ),
        _ => (vec![], vec![]),
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

/// What lowering one conditional fixture on a fresh host produced and paid.
#[derive(Debug, PartialEq, Eq)]
struct LoweredConditional {
    text: String,
    /// The names the lowering bound, in push order.
    substitutions: Vec<Arc<str>>,
    /// Operand walk visits, operand preparations, distributed members and
    /// branch selections.
    work: [usize; 4],
    /// The branches the lowering descended into, false then true.
    descents: [usize; 2],
    lookups: u64,
}

fn lower_conditional(label: &str, stepped: bool) -> LoweredConditional {
    type Dispatch<'a> = ProjectSemanticDispatch<'a, crate::resolver_core::HostCapabilities>;
    let host = host();
    upsert_ts(
        &host,
        "/lower.ts",
        &[
            "export declare const x: number;\n",
            CONDITIONAL_DECLARATIONS,
        ]
        .concat(),
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
    // The distributive fixture's check is bound to `[1] | [2]`.
    let members = TypeExpr::union(vec![
        tuple(vec![TypeExpr::number_literal(1.0)]),
        tuple(vec![TypeExpr::number_literal(2.0)]),
    ]);
    let mut ignored = Vec::new();
    let union = dispatch.shallow_lower_type_expr_with_context_for_tests(
        &members,
        &FxHashMap::default(),
        &scope,
        &names,
        payload.as_ref(),
        &shadowing,
        &mut ignored,
        context,
    );
    let mut env = FxHashMap::default();
    env.insert("T".to_owned(), union);
    let expr = conditional_fixtures()
        .into_iter()
        .find_map(|(name, expr)| (name == label).then_some(expr))
        .expect("a conditional fixture");
    let work = || {
        [
            Dispatch::conditional_operand_walk_visits_for_tests(),
            Dispatch::conditional_operand_preparations_for_tests(),
            Dispatch::conditional_distributed_members_for_tests(),
            Dispatch::conditional_branch_selections_for_tests(),
        ]
    };
    let before = (
        work(),
        Dispatch::conditional_branch_descents_for_tests(),
        dispatch.graph().stats_snapshot(),
    );
    let (node, substitutions) = if stepped {
        let trace = dispatch.step_type_lowering_for_tests(
            &expr,
            &env,
            &scope,
            &names,
            payload.as_ref(),
            &shadowing,
            context,
        );
        (trace.node, trace.substitutions)
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
        (node, substitutions)
    };
    let after = (
        work(),
        Dispatch::conditional_branch_descents_for_tests(),
        dispatch.graph().stats_snapshot(),
    );
    LoweredConditional {
        text: crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
            &dispatch, node, 0,
        ),
        substitutions: substitutions.into_iter().map(|(name, _)| name).collect(),
        work: std::array::from_fn(|i| after.0[i] - before.0[i]),
        descents: std::array::from_fn(|i| after.1[i] - before.1[i]),
        lookups: (after.2.hits + after.2.misses) - (before.2.hits + before.2.misses),
    }
}

/// A conditional lowered stepped, its decision held in a lowering frame,
/// does the work the in-place lowering does: the same answer, bindings,
/// decision work, branch lowerings and lookups, for every source of the
/// reads a decision makes.
#[test]
fn stepped_conditional_lowering_does_the_work_of_in_place_lowering() {
    for (label, _) in conditional_fixtures() {
        let in_place = lower_conditional(label, false);
        assert!(
            in_place.work[1] > 0,
            "{label}: the decision prepares its operands"
        );
        assert_eq!(lower_conditional(label, true), in_place, "{label}");
    }
}

/// A lowered conditional lowers each branch its decision asks for once,
/// however often it asks: the `any` check's selection takes both branches,
/// and each member of a distribution over a bound union takes the true
/// branch. One descent per branch, one substitution per binder.
#[test]
fn a_lowered_conditional_lowers_each_branch_once() {
    for stepped in [false, true] {
        let both = lower_conditional("conditional any check takes both branches", stepped);
        assert_eq!(both.descents, [1, 1], "both branches, each once: {both:?}");
        assert_eq!(
            both.substitutions,
            [Arc::<str>::from("U")],
            "the true branch binds `U` once: {both:?}"
        );
        let distributed = lower_conditional("conditional distributes over a bound union", stepped);
        assert_eq!(
            distributed.work[2], 2,
            "each member is decided: {distributed:?}"
        );
        assert_eq!(
            distributed.descents,
            [0, 1],
            "the members share one true branch: {distributed:?}"
        );
        assert_eq!(
            distributed.substitutions,
            [Arc::<str>::from("T"), Arc::from("U")],
            "the check binds `T` and the true branch binds `U`, once each: {distributed:?}"
        );
    }
}

/// Which lowering entry lowers the conditional in
/// [`lower_with_superseded_branch_read`].
#[derive(Clone, Copy, Debug)]
enum ConditionalEntry {
    /// The in-place lowering every arm recurses through.
    InPlace,
    /// The public construction wrapper, which installs no validity scope.
    Public,
    /// The stepped lowering, its reads answered by a suspending holder.
    Stepped,
}

/// What a conditional lowered while its branch's read was superseded
/// produced.
#[derive(Debug)]
struct BranchAbortOutcome {
    text: String,
    /// The whole conditional lowered to `opaque(Miss)`: its decision aborted.
    aborted: bool,
    /// The read the plant targets ran cold, so the plant fired.
    fired: bool,
    /// The abort an enclosing validity scope saw, when one was entered.
    enclosing: Option<verter_type_engine::semantic_query::ExecutionAbort>,
    /// The file of the first cancellation poll that observed the request's
    /// cancellation, when the request was cancelled.
    cancel_observed_in: Option<String>,
}

/// Lower `number extends number ? [typeof x] : never` through `entry` on a
/// fresh host. With `superseded`, the true branch's `typeof x` read finds
/// its view superseded, as a concurrent edit makes it: the next cold build
/// observes `ExecutionAbort::Superseded`. The same conditional with a
/// literal true branch is lowered first, so every read the decision makes
/// is warm and the first cold build is the branch's. With `enclosing`, the
/// lowering runs inside a validity scope. With `cancel`, the request is
/// cancelled once the decision has passed its entry check with the lowered
/// branch in hand, before it resumes the operation that asked for it.
fn lower_with_superseded_branch_read(
    entry: ConditionalEntry,
    superseded: bool,
    enclosing: bool,
    cancel: bool,
) -> BranchAbortOutcome {
    use verter_type_engine::semantic_query::{ExecutionAbort, QueryError};
    let host = host();
    upsert_ts(&host, "/lower.ts", "export declare const x: number;\n");
    let dispatch = ProjectSemanticDispatch::new(&host);
    // Every node this lowers stays rooted until it is rendered.
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
    let with_true_branch = |true_type: TypeExpr| {
        conditional(
            TypeExpr::Primitive(PrimitiveName::Number),
            TypeExpr::Primitive(PrimitiveName::Number),
            true_type,
            TypeExpr::Primitive(PrimitiveName::Never),
        )
    };
    let lower = |expr: &TypeExpr| match entry {
        ConditionalEntry::InPlace => dispatch.shallow_lower_type_expr_with_context_for_tests(
            expr,
            &env,
            &scope,
            &names,
            payload.as_ref(),
            &shadowing,
            &mut Vec::new(),
            context,
        ),
        ConditionalEntry::Public => dispatch
            .lower_type_expr_in_scope_with_mode("/lower.ts", expr, ProjectionMode::Expanded)
            .expect("the file's scope"),
        ConditionalEntry::Stepped => {
            dispatch
                .step_type_lowering_for_tests(
                    expr,
                    &env,
                    &scope,
                    &names,
                    payload.as_ref(),
                    &shadowing,
                    context,
                )
                .node
        }
    };
    lower(&with_true_branch(TypeExpr::number_literal(1.0)));
    let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if superseded {
        let fired = Arc::clone(&fired);
        *host
            .test_force
            .engine
            .semantic_operand_cold_build_seam
            .0
            .lock() = Some(Arc::new(move || {
            fired.store(true, std::sync::atomic::Ordering::Relaxed);
            verter_type_engine::request_context::observe_execution_abort(
                ExecutionAbort::Superseded,
            );
        }));
    }
    let _job = cancel.then(|| {
        let token = verter_execution::cancellation::CancellationToken::new();
        let seam_token = token.clone();
        *host
            .test_force
            .engine
            .conditional_branch_delivery_seam
            .0
            .lock() = Some(Arc::new(move || seam_token.cancel()));
        verter_execution::cancellation::JobCancellationGuard::install(token)
    });
    let validity =
        enclosing.then(verter_type_engine::request_context::ExecutionValidityScope::enter);
    let (node, trace) = verter_type_engine::cancel_trace::traced(|| {
        lower(&with_true_branch(tuple(vec![value_type(&["x"], vec![])])))
    });
    let cancel_observed_in = trace.first_observed.map(|(site, _)| {
        std::path::Path::new(site.file())
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    });
    let enclosing = validity.and_then(|validity| {
        let abort = verter_type_engine::request_context::current_execution_abort();
        drop(validity);
        abort
    });
    BranchAbortOutcome {
        text: crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
            &dispatch, node, 0,
        ),
        aborted: matches!(
            dispatch.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::Opaque(QueryError::Miss))
        ),
        fired: fired.load(std::sync::atomic::Ordering::Relaxed),
        enclosing,
        cancel_observed_in,
    }
}

/// A conditional branch lowers between two advances of the decision that
/// asked for it, outside the asking operation's observation. When a read
/// the branch makes is superseded, the decision still owns that abort and
/// aborts: the whole conditional lowers to `opaque(Miss)`, not to a branch
/// value built over the failed read, with no validity scope enclosing the
/// lowering, through every lowering entry. Inside an enclosing scope the
/// abort also reaches that scope. Unplanted, the branch value stands.
#[test]
fn a_superseded_branch_read_aborts_its_conditional() {
    use verter_type_engine::semantic_query::ExecutionAbort;
    use ConditionalEntry::{InPlace, Public, Stepped};
    let sound = lower_with_superseded_branch_read(InPlace, false, false, false);
    assert!(
        !sound.aborted && !sound.fired && !sound.text.starts_with("Opaque"),
        "unplanted, the branch value stands: {sound:?}"
    );
    let mut failures = Vec::new();
    for entry in [InPlace, Public, Stepped] {
        let outcome = lower_with_superseded_branch_read(entry, true, false, false);
        assert!(
            outcome.fired,
            "{entry:?}: the branch's read ran cold: {outcome:?}"
        );
        if !outcome.aborted {
            failures.push(format!("{entry:?}: {outcome:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "a superseded branch read must abort its whole conditional:\n{}",
        failures.join("\n")
    );
    for entry in [InPlace, Stepped] {
        let outcome = lower_with_superseded_branch_read(entry, true, true, false);
        assert!(
            outcome.aborted && outcome.enclosing == Some(ExecutionAbort::Superseded),
            "{entry:?}: the decision aborts and its abort reaches the enclosing build: {outcome:?}"
        );
    }
}

/// A decision can be cancelled after its entry check has passed with a
/// lowered branch in hand, before the operation that asked for the branch
/// consumes it. The decision then stops without that operation, but the
/// abort the branch's superseded read raised is still the request's: the
/// enclosing validity scope sees `Superseded`, and the conditional lowers
/// to `opaque(Miss)`, through every lowering entry.
#[test]
fn a_cancelled_decision_keeps_its_undelivered_branch_abort() {
    use verter_type_engine::semantic_query::ExecutionAbort;
    use ConditionalEntry::{InPlace, Public, Stepped};
    let mut failures = Vec::new();
    for entry in [InPlace, Public, Stepped] {
        let outcome = lower_with_superseded_branch_read(entry, true, true, true);
        assert!(
            outcome.fired,
            "{entry:?}: the branch's read ran cold: {outcome:?}"
        );
        assert_eq!(
            outcome.cancel_observed_in.as_deref(),
            Some("conditional_decision.rs"),
            "{entry:?}: a check inside the decision is the first to see the cancellation: {outcome:?}"
        );
        if !(outcome.aborted && outcome.enclosing == Some(ExecutionAbort::Superseded)) {
            failures.push(format!("{entry:?}: {outcome:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "a cancelled decision must still hand its branch's abort to the enclosing build:\n{}",
        failures.join("\n")
    );
}

/// An alias whose application no lane can read: the whole global object is
/// a deferred `typeof globalThis` carrier, so the conditional stays
/// undecided and a read of `Dom<1>` answers partial.
const UNREADABLE_ARM_DECLARATION: &str =
    "export type Dom<T> = typeof globalThis extends { window: unknown } ? T : never;\n";

/// A conditional held by a lowering that a flow body drives makes each
/// read its decision names inside the decision operation that named it,
/// whether the body suspends at the read (its root read answering the
/// request) or answers it at once: the read's dependencies are observed
/// while that operation's observation is on top, its partiality and reuse
/// evidence land in that observation, and the decision decides with them.
/// In `"a" extends string | Dom<1> ? 1 : 2` the partial `Dom<1>` read
/// leaves the operand preparation incomplete, so the written
/// `string` arm proves the true branch and the unused operands contribute
/// only their reuse evidence: the flow-driven lowering answers what the
/// native lowering answers, as completely. A read answered ahead of the
/// decision instead lands its dependencies and partiality in the enclosing
/// build and none in the decision's observation.
#[test]
fn a_flow_driven_lowering_makes_its_conditional_decision_reads_inside_the_decision() {
    use verter_type_engine::project_semantic_dispatch::lower::OwnerLoweringDriverForTests as Driver;
    let expr = conditional(
        TypeExpr::string_literal("a"),
        TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            named("Dom", vec![TypeExpr::number_literal(1.0)]),
        ]),
        TypeExpr::number_literal(1.0),
        TypeExpr::number_literal(2.0),
    );
    let lower = |driver: Driver| {
        let host = host();
        upsert_ts(&host, "/lower.ts", UNREADABLE_ARM_DECLARATION);
        let dispatch = ProjectSemanticDispatch::new(&host);
        let _construction = dispatch.enter_construction_scope();
        let observed = dispatch.observed_owner_lowering_for_tests(
            "/lower.ts",
            &expr,
            ProjectionReductionContext::structural_transit(),
            driver,
        );
        let text = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
            &dispatch,
            observed.node,
            0,
        );
        (text, observed)
    };
    let (native_text, native) = lower(Driver::Native);
    assert_eq!(
        native.partial, None,
        "the native lowering proves the true branch through the written arm: {native_text} {native:?}"
    );
    assert!(
        native
            .decision_reads
            .iter()
            .any(|read| read.partial && !read.dependencies.is_empty()),
        "the native decision makes its partial read, with its dependencies, inside the decision: {native:?}"
    );
    for driver in [Driver::FlowRequested, Driver::FlowInPlace] {
        let (text, flow) = lower(driver);
        assert!(
            flow.decision_reads
                .iter()
                .any(|read| read.partial && !read.observed_partial[0]),
            "{driver:?}: the decision reads a partial answer into an observation that was complete: {flow:?}"
        );
        for read in &flow.decision_reads {
            assert!(
                !read.partial || !read.dependencies.is_empty(),
                "{driver:?}: a partial decision read observes its dependencies while the decision's observation is on top: {read:?}"
            );
            assert!(
                !read.partial || read.observed_partial[1],
                "{driver:?}: a partial decision read is partial in the decision's observation: {read:?}"
            );
            assert!(
                !read.suppressed || read.observed_suppressed[1],
                "{driver:?}: a non-reusable decision read is non-reusable in the decision's observation: {read:?}"
            );
        }
        assert_eq!(
            (text.as_str(), flow.partial),
            (native_text.as_str(), native.partial),
            "{driver:?}: the flow-driven lowering answers what the native lowering answers, as completely: {flow:?}"
        );
    }
}
