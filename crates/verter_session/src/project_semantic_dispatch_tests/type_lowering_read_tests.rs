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

#[test]
fn type_lowering_preserves_read_order_shapes_and_substitutions() {
    use LowerReadShape::{Node, Whole};
    let host = host();
    upsert_ts(
        &host,
        "/lower.ts",
        concat!(
            "export declare const x: number;\n",
            "export type Box<T> = { value: T };\n",
            "export declare class C { static make<T>(value: T): T; }\n",
        ),
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
    let cases = [
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
                    other => panic!("unexpected lowering read: {other:?}"),
                };
                (family, *shape)
            })
            .collect();
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
    }
}
