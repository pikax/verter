//! Discriminating coverage for the bounded terminal `TypeExpr` ->
//! `SemanticTypeGraph` export (`verter_protocol::typeinfo::graph_export`).
//!
//! The export is the bounded graph-answer encoder for the typeinfo graph
//! operations: it projects ONE already-materialized terminal `TypeExpr`
//! (the sealed output of the typeinfo raise pipeline) into the wire node
//! arena — interned strings, deduplicated nodes, minted symbols, and the
//! signatures arena — under explicit node/depth budgets. It is a pure
//! projection: no resolution, no parsing, no second walk of semantic state.
//!
//! Discriminating boundaries pinned here:
//! - member/union ordering is the source order (deterministic encode);
//! - identical sub-expressions share one node id (identity, no aliasing);
//! - every representable `TypeExpr` arm maps to its wire node kind with no
//!   silent fallback, and the by-design opaque degradations are explicit;
//! - budget exhaustion is a fail-closed opaque marker, never truncation;
//! - function shapes ride the signatures arena;
//! - spread-bearing objects encode the ordered construction program.

use std::sync::Arc;

use prost::Message;

use verter_protocol::typeinfo::graph as g;
use verter_protocol::typeinfo::graph_export::{
    encode_type_expr_graph, GraphExportBudgets, UNBOUNDED_SENTINEL_BUDGET,
};
use verter_protocol::verter::v1::graph_type_node;
use verter_type_expr::{
    FunctionExpr, FunctionParam, IndexSignature, LiteralValue, MappedModifier, MethodSignature,
    ObjectMember, ObjectProperty, TypeExpr, TypeParam,
};

/// Budgets wide enough that nothing degenerates in the mapping tests.
fn wide_budgets() -> GraphExportBudgets {
    GraphExportBudgets {
        node_budget: UNBOUNDED_SENTINEL_BUDGET,
        depth_budget: UNBOUNDED_SENTINEL_BUDGET,
    }
}

fn encode(root: &TypeExpr) -> g::SemanticTypeGraph {
    encode_type_expr_graph(root, &wide_budgets())
}

/// Resolve an interned string id through the graph table.
fn str_at(graph: &g::SemanticTypeGraph, id: u32) -> &str {
    graph
        .strings
        .as_ref()
        .and_then(|t| t.entries.get(id as usize))
        .map(String::as_str)
        .unwrap_or("")
}

/// Borrow the root node of a single-root export.
fn root_node(graph: &g::SemanticTypeGraph) -> &g::TypeNode {
    let root_id = *graph
        .root_ids
        .first()
        .expect("export records its root node id");
    &graph.nodes[root_id as usize]
}

fn prop(name: &str, ty: TypeExpr) -> ObjectMember {
    ObjectMember::Property(ObjectProperty::synthetic_public_key(
        name.into(),
        ty,
        false,
        false,
    ))
}

fn object(members: Vec<ObjectMember>) -> TypeExpr {
    TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
        properties: members,
    }))
}

#[test]
fn object_export_preserves_member_order_and_dedups_value_nodes() {
    let ty = object(vec![
        prop(
            "b",
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
        ),
        prop(
            "a",
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
        ),
        prop(
            "c",
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
        ),
    ]);
    let graph = encode(&ty);

    let node = root_node(&graph);
    let g::TypeNode {
        kind: Some(graph_type_node::Kind::Object(obj)),
        ..
    } = node
    else {
        panic!(
            "object root must encode as a GraphObject, got {:?}",
            node.kind
        );
    };
    assert_eq!(obj.members.len(), 3, "member order is source order");

    let names: Vec<String> = obj
        .members
        .iter()
        .map(
            |m| match m.property_key.as_ref().and_then(|k| k.key.as_ref()) {
                Some(verter_protocol::verter::v1::graph_property_key::Key::StringId(id)) => {
                    str_at(&graph, *id).to_string()
                }
                other => panic!("string property key expected, got {other:?}"),
            },
        )
        .collect();
    assert_eq!(names, vec!["b", "a", "c"], "member order is preserved");

    // Identity: the two `string` value nodes are ONE interned primitive.
    let b_value = obj.members[0].value_node_id;
    let c_value = obj.members[2].value_node_id;
    assert_eq!(
        b_value, c_value,
        "identical sub-expressions share one node id"
    );
    assert_ne!(
        obj.members[1].value_node_id, b_value,
        "distinct sub-expressions get distinct node ids"
    );
}

#[test]
fn parameterized_ref_mints_symbol_and_alias_instantiation() {
    let ty = TypeExpr::Ref {
        name: Arc::from("Partial"),
        type_arguments: Arc::from(vec![TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::String,
        )]),
    };
    let graph = encode(&ty);

    let node = root_node(&graph);
    let g::TypeNode {
        kind: Some(graph_type_node::Kind::AliasInstantiation(inst)),
        ..
    } = node
    else {
        panic!(
            "parameterized ref must encode as GraphAliasInstantiation, got {:?}",
            node.kind
        );
    };
    let symbol = &graph.symbols[inst.alias_symbol_id as usize];
    assert_eq!(str_at(&graph, symbol.name_id), "Partial");
    assert_eq!(inst.type_argument_node_ids.len(), 1);
    assert!(
        inst.target_node_id == 0,
        "the bounded export does not fabricate an unexpanded alias body"
    );
}

#[test]
fn bare_ref_encodes_reference_and_mints_symbol() {
    let ty = TypeExpr::Ref {
        name: Arc::from("IFoo"),
        type_arguments: Arc::from(Vec::new()),
    };
    let graph = encode(&ty);
    let node = root_node(&graph);
    let g::TypeNode {
        kind: Some(graph_type_node::Kind::Reference(reference)),
        ..
    } = node
    else {
        panic!(
            "bare ref must encode as GraphReference, got {:?}",
            node.kind
        );
    };
    let symbol = &graph.symbols[reference.symbol_id as usize];
    assert_eq!(str_at(&graph, symbol.name_id), "IFoo");
}

#[test]
fn budget_exhaustion_is_a_fail_closed_opaque_marker() {
    // A left-nested union chain: depth n needs n union levels.
    let mut ty = TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
    for _ in 0..8 {
        ty = TypeExpr::Union(Arc::from(vec![
            ty,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
        ]));
    }
    let budgets = GraphExportBudgets {
        node_budget: UNBOUNDED_SENTINEL_BUDGET,
        depth_budget: 2,
    };
    let graph = encode_type_expr_graph(&ty, &budgets);

    // Walking below the budget floor must hit a budget_exceeded opaque
    // node — never a silently truncated subtree.
    let mut frontier = vec![*graph.root_ids.first().expect("root present")];
    let mut saw_budget_marker = false;
    while let Some(id) = frontier.pop() {
        let node = &graph.nodes[id as usize];
        match node.kind.as_ref().expect("node kind present") {
            graph_type_node::Kind::Union(union) => {
                frontier.extend(union.member_node_ids.iter().copied());
            }
            graph_type_node::Kind::Opaque(opaque) => {
                match opaque.error.as_ref().and_then(|e| e.kind.as_ref()) {
                    Some(verter_protocol::verter::v1::graph_query_error::Kind::BudgetExceeded(
                        b,
                    )) => {
                        assert_eq!(b.limit, 2, "the marker carries the enforced limit");
                        saw_budget_marker = true;
                    }
                    other => panic!("unexpected opaque error {other:?}"),
                }
            }
            _ => {}
        }
    }
    assert!(
        saw_budget_marker,
        "depth exhaustion must surface a budget_exceeded opaque marker"
    );
}

#[test]
fn node_budget_caps_the_real_arena() {
    let members: Vec<ObjectMember> = (0..24)
        .map(|i| {
            prop(
                &format!("p{i}"),
                TypeExpr::Literal(LiteralValue::Number(f64::from(i))),
            )
        })
        .collect();
    let ty = object(members);

    // A budget the tree cannot fit: the arena stops at the budget (plus
    // the reserved absent-sentinel slot and the shared marker), and the
    // root itself degrades to the budget marker — fail-closed, never a
    // truncated member surface.
    let budgets = GraphExportBudgets {
        node_budget: 5,
        depth_budget: UNBOUNDED_SENTINEL_BUDGET,
    };
    let graph = encode_type_expr_graph(&ty, &budgets);
    let real = graph
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, None | Some(graph_type_node::Kind::Opaque(_))))
        .count();
    assert!(real <= 5, "node budget bounds the real arena, got {real}");
    assert!(
        graph.nodes.len() <= 5 + 2,
        "bounded arena, got {}",
        graph.nodes.len()
    );
    match root_node(&graph).kind.as_ref() {
        Some(graph_type_node::Kind::Opaque(opaque)) => assert!(matches!(
            opaque.error.as_ref().and_then(|e| e.kind.as_ref()),
            Some(verter_protocol::verter::v1::graph_query_error::Kind::BudgetExceeded(_))
        )),
        other => panic!("an over-budget root must be the budget marker, got {other:?}"),
    }

    // A budget the tree fits: the full member surface survives.
    let budgets = GraphExportBudgets {
        node_budget: 30,
        depth_budget: UNBOUNDED_SENTINEL_BUDGET,
    };
    let graph = encode_type_expr_graph(&ty, &budgets);
    let g::TypeNode {
        kind: Some(graph_type_node::Kind::Object(obj)),
        ..
    } = root_node(&graph)
    else {
        panic!("root object expected");
    };
    assert_eq!(obj.members.len(), 24);
}

#[test]
fn function_types_ride_the_signature_arena() {
    let ty = TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("x".to_string()),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
            false,
            false,
        )],
        Some(Arc::new(TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::Boolean,
        ))),
        vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
            is_const: false,
        }],
    )));
    let graph = encode(&ty);
    let node = root_node(&graph);
    let g::TypeNode {
        kind: Some(graph_type_node::Kind::Object(obj)),
        ..
    } = node
    else {
        panic!("function encodes as callable object, got {:?}", node.kind);
    };
    assert_eq!(obj.call_signature_refs.len(), 1);
    let sig = &graph.signatures[obj.call_signature_refs[0] as usize];
    assert!(!sig.is_construct);
    assert_eq!(sig.parameters.len(), 1);
    assert_eq!(str_at(&graph, sig.parameters[0].name_id), "x");
    assert_eq!(sig.type_parameter_node_ids.len(), 1);
    let ret = &graph.nodes[sig.return_type_node_id as usize];
    assert!(matches!(
        ret.kind,
        Some(graph_type_node::Kind::Primitive(_))
    ));

    // The constructor sibling flips the construct axis.
    let ty = TypeExpr::ConstructorType(Arc::new(FunctionExpr::synthetic(
        Vec::new(),
        Some(Arc::new(TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::Object,
        ))),
        Vec::new(),
    )));
    let graph = encode(&ty);
    let node = root_node(&graph);
    let g::TypeNode {
        kind: Some(graph_type_node::Kind::Object(obj)),
        ..
    } = node
    else {
        panic!("constructor type encodes as constructible object");
    };
    assert_eq!(obj.construct_signature_refs.len(), 1);
    assert!(graph.signatures[obj.construct_signature_refs[0] as usize].is_construct);
}

#[test]
fn spread_bearing_object_encodes_the_ordered_construction_program() {
    let ty = object(vec![
        ObjectMember::Spread(verter_type_expr::SpreadMember::new(TypeExpr::Ref {
            name: Arc::from("Base"),
            type_arguments: Arc::from(Vec::new()),
        })),
        prop(
            "own",
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
        ),
    ]);
    let graph = encode(&ty);
    let node = root_node(&graph);
    let g::TypeNode {
        kind: Some(graph_type_node::Kind::ObjectSpreadProgram(program)),
        ..
    } = node
    else {
        panic!(
            "spread-bearing object must encode as GraphObjectSpreadProgram, got {:?}",
            node.kind
        )
    };
    assert_eq!(program.effects.len(), 2, "source-ordered effects");
    assert!(matches!(
        program.effects[0].kind,
        Some(verter_protocol::verter::v1::graph_object_construction_effect::Kind::Spread(_))
    ));
    assert!(matches!(
        program.effects[1].kind,
        Some(
            verter_protocol::verter::v1::graph_object_construction_effect::Kind::DirectProperty(_)
        )
    ));
}

#[test]
fn every_representable_arm_maps_to_its_wire_node_kind() {
    // (label, expression, expected wire discriminant)
    let cases: Vec<(&str, TypeExpr, graph_type_node::Kind)> = vec![
        (
            "primitive",
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::BigInt),
            graph_type_node::Kind::Primitive(g::PrimitiveNode { kind: 9 }),
        ),
        (
            "literal",
            TypeExpr::Literal(LiteralValue::String("hello".into())),
            graph_type_node::Kind::Literal(g::LiteralNode {
                value: Some(verter_protocol::verter::v1::GraphLiteralValue {
                    kind: Some(
                        verter_protocol::verter::v1::graph_literal_value::Kind::StringNameId(0),
                    ),
                }),
            }),
        ),
        (
            "union",
            TypeExpr::Union(Arc::from(vec![TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::String,
            )])),
            graph_type_node::Kind::Union(g::UnionNode {
                member_node_ids: vec![0],
            }),
        ),
        (
            "intersection",
            TypeExpr::Intersection(Arc::from(vec![TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::String,
            )])),
            graph_type_node::Kind::Intersection(g::IntersectionNode {
                member_node_ids: vec![0],
            }),
        ),
        (
            "array",
            TypeExpr::Array {
                element: Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)),
                readonly: true,
            },
            graph_type_node::Kind::Array(g::ArrayNode {
                element_node_id: 0,
                readonly: true,
            }),
        ),
        (
            "tuple",
            TypeExpr::Tuple {
                elements: Arc::from(vec![verter_type_expr::TupleElement {
                    label: Some("first".to_string()),
                    ty: TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
                    optional: true,
                    rest: false,
                }]),
                readonly: false,
            },
            graph_type_node::Kind::Tuple(g::TupleNode {
                elements: vec![verter_protocol::verter::v1::GraphTupleElement {
                    label_name_id: 0,
                    value_node_id: 0,
                    optional: true,
                    rest: false,
                }],
                readonly: false,
            }),
        ),
        (
            "typeParameter",
            TypeExpr::TypeParameter(TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
                is_const: false,
            }),
            graph_type_node::Kind::TypeParameter(g::TypeParameterNode {
                symbol_id: 0,
                decl_slot_ref: 0,
                param_index: 0,
                name_id: 0,
                constraint_node_id: 0,
                default_node_id: 0,
                variance: 0,
                is_const: false,
                no_infer: false,
                binding: None,
            }),
        ),
        (
            "keyOf",
            TypeExpr::KeyOf(Arc::new(TypeExpr::Ref {
                name: Arc::from("K"),
                type_arguments: Arc::from(Vec::new()),
            })),
            graph_type_node::Kind::KeyOf(g::KeyOfNode { base_node_id: 0 }),
        ),
        (
            "indexedAccess",
            TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::Ref {
                    name: Arc::from("T"),
                    type_arguments: Arc::from(Vec::new()),
                }),
                index: Arc::new(TypeExpr::Literal(LiteralValue::String("k".into()))),
            },
            graph_type_node::Kind::IndexedAccess(g::IndexedAccessNode {
                object_node_id: 0,
                index_node_id: 0,
            }),
        ),
        (
            "conditional",
            TypeExpr::Conditional {
                check: Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Any)),
                extends: Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Any)),
                true_type: Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)),
                false_type: Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Never)),
            },
            graph_type_node::Kind::Conditional(g::ConditionalNode {
                check_node_id: 0,
                extends_node_id: 0,
                true_branch_node_id: 0,
                false_branch_node_id: 0,
                distributive: false,
                resolution: None,
            }),
        ),
        (
            "mapped",
            TypeExpr::Mapped {
                parameter: "K".to_string(),
                source: Arc::new(TypeExpr::Ref {
                    name: Arc::from("Keys"),
                    type_arguments: Arc::from(Vec::new()),
                }),
                value: Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)),
                optional: MappedModifier::Add,
                readonly: MappedModifier::None,
                name_type: None,
            },
            graph_type_node::Kind::Mapped(g::MappedNode {
                key_type_node_id: 0,
                source_node_id: 0,
                name_remap_node_id: 0,
                value_type_node_id: 0,
                readonly_modifier: 0,
                optional_modifier: 1,
            }),
        ),
        (
            "templateLiteral",
            TypeExpr::TemplateLiteral {
                quasis: vec!["a".to_string(), "b".to_string()],
                expressions: Arc::from(vec![TypeExpr::Primitive(
                    verter_type_expr::PrimitiveName::String,
                )]),
            },
            graph_type_node::Kind::TemplateLiteral(g::TemplateLiteralNode {
                quasi_name_ids: vec![0, 0],
                expression_node_ids: vec![0],
            }),
        ),
        (
            "infer",
            TypeExpr::Infer {
                name: "U".to_string(),
            },
            graph_type_node::Kind::InferNode(g::InferNode {
                name_id: 0,
                constraint_node_id: 0,
            }),
        ),
        (
            "typeOf",
            TypeExpr::TypeOf(verter_type_expr::ValueRef {
                path: vec!["a".to_string(), "b".to_string()],
                type_args: Vec::new(),
            }),
            graph_type_node::Kind::TypeofNode(verter_protocol::verter::v1::GraphTypeOf {
                value_root_ref: 0,
                path_name_ids: vec![0, 0],
            }),
        ),
    ];

    for (label, ty, expected_kind) in cases {
        let graph = encode(&ty);
        let node = root_node(&graph);
        let discriminant = std::mem::discriminant(node.kind.as_ref().expect("kind present"));
        assert_eq!(
            discriminant,
            std::mem::discriminant(&expected_kind),
            "`{label}` must encode as its wire node kind"
        );
        // No representable arm may fall through to Opaque.
        assert!(
            !matches!(node.kind, Some(graph_type_node::Kind::Opaque(_))),
            "`{label}` must not degrade to opaque"
        );
    }
}

#[test]
fn index_and_method_members_encode_with_keys_and_signatures() {
    let ty = object(vec![
        ObjectMember::IndexSignature(IndexSignature::synthetic(
            "key".to_string(),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
            false,
        )),
        ObjectMember::Method(MethodSignature::synthetic_public_key(
            "greet".into(),
            FunctionExpr::synthetic(Vec::new(), None, Vec::new()),
            false,
        )),
    ]);
    let graph = encode(&ty);
    let node = root_node(&graph);
    let g::TypeNode {
        kind: Some(graph_type_node::Kind::Object(obj)),
        ..
    } = node
    else {
        panic!("object root expected, got {:?}", node.kind);
    };
    assert_eq!(obj.index_signatures.len(), 1);
    let index = &obj.index_signatures[0];
    assert_eq!(index.key_kind, g::IndexKeyKind::String as i32);
    assert_eq!(obj.members.len(), 1, "the method is a member");
    let method = &obj.members[0];
    assert_eq!(method.member_kind, g::ObjectMemberKind::Method as i32);
    let value = &graph.nodes[method.value_node_id as usize];
    assert!(
        matches!(value.kind, Some(graph_type_node::Kind::Object(_))),
        "method value rides a callable-object node"
    );
}

#[test]
fn by_design_opaque_degradations_carry_explicit_markers() {
    // SyntheticSlotBinding / ImportType / Unknown have no wire node arm:
    // they degrade to Opaque with an interned message — never silently.
    let ty = TypeExpr::SyntheticSlotBinding(Arc::new(verter_type_expr::SyntheticCarrierKey {
        scope_canonical_id: Arc::from("/Comp.vue"),
        surface_kind: verter_type_expr::SyntheticCarrierSurfaceKind::SlotBinding,
        slot_name: Some(Arc::from("default")),
        binding_name: Arc::from("slots"),
        value_node: 7,
    }));
    let graph = encode(&ty);
    let node = root_node(&graph);
    let g::TypeNode {
        kind: Some(graph_type_node::Kind::Opaque(opaque)),
        ..
    } = node
    else {
        panic!("synthetic carrier degrades to opaque, got {:?}", node.kind);
    };
    assert!(
        matches!(
            opaque.error.as_ref().and_then(|e| e.kind.as_ref()),
            Some(verter_protocol::verter::v1::graph_query_error::Kind::Other(
                _
            ))
        ),
        "degradation is an explicit typed marker"
    );

    let ty = TypeExpr::Unknown(verter_type_expr::UnknownValue::unsupported_syntax("raw!"));
    let graph = encode(&ty);
    assert!(matches!(
        root_node(&graph).kind,
        Some(graph_type_node::Kind::Opaque(_))
    ));
}

#[test]
fn encode_is_deterministic_and_wire_roundtrips() {
    let ty = object(vec![
        prop(
            "a",
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
        ),
        prop(
            "fn",
            TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
                Vec::new(),
                Some(Arc::new(TypeExpr::Primitive(
                    verter_type_expr::PrimitiveName::Void,
                ))),
                Vec::new(),
            ))),
        ),
    ]);
    let first = encode(&ty);
    let second = encode(&ty);
    let first_bytes = first.encode_to_vec();
    let second_bytes = second.encode_to_vec();
    assert_eq!(first_bytes, second_bytes, "encoding is deterministic");

    let decoded = g::SemanticTypeGraph::decode(first_bytes.as_slice())
        .expect("wire graph round-trips through protobuf");
    assert_eq!(decoded, first);
    assert_eq!(decoded.schema_version, g::TYPEINFO_GRAPH_SCHEMA_VERSION);
}

#[test]
fn recursive_ref_encodes_a_cycle_marker_rooted_at_the_reference() {
    let ty = TypeExpr::RecursiveRef {
        name: Arc::from("Json"),
        type_arguments: Arc::from(Vec::new()),
        conditional_context: Arc::from(Vec::new()),
    };
    let graph = encode(&ty);
    let node = root_node(&graph);
    let g::TypeNode {
        kind: Some(graph_type_node::Kind::Cycle(cycle)),
        ..
    } = node
    else {
        panic!(
            "recursive ref must encode as GraphCycle, got {:?}",
            node.kind
        )
    };
    assert!(
        cycle.cycle_root_node_id != 0,
        "the cycle roots at the recursive reference node"
    );
    assert_eq!(cycle.participants.len(), 1);
    let symbol = &graph.symbols[cycle.participants[0] as usize];
    assert_eq!(str_at(&graph, symbol.name_id), "Json");
}
