//! Roundtrip + closed-taxonomy coverage for the typeinfo graph wire
//! contracts.
//!
//! Every variant of every closed `oneof` in the typeinfo proto is
//! constructed once, encoded to a byte buffer via `prost::Message`,
//! and decoded back; the decoded value must compare equal to the
//! original. Adding a new arm requires extending the per-variant
//! constructor lists below — the cardinality assertions at the tail
//! of each helper guarantee a silent omission fails this test.
//!
//! The test depends on `crate::typeinfo::graph::*` (re-exports of the
//! prost-generated wire types in `crate::verter::v1`).

use prost::Message;

use verter_protocol::typeinfo::graph as g;

#[test]
fn graph_type_node_roundtrip_covers_every_oneof_variant() {
    let variants = build_every_type_node_variant();
    assert_eq!(
        variants.len(),
        32,
        "GraphTypeNode covers exactly 32 oneof variants — the audited closed taxonomy",
    );

    for (idx, original) in variants.iter().enumerate() {
        let bytes = original.encode_to_vec();
        let decoded = g::TypeNode::decode(bytes.as_slice())
            .unwrap_or_else(|e| panic!("variant {idx} should decode: {e}"));
        assert_eq!(decoded, *original, "variant {idx} must roundtrip cleanly");
        assert!(decoded.kind.is_some(), "variant {idx} kind must survive");
    }
}

#[test]
fn structured_type_expression_roundtrip_covers_every_oneof_variant() {
    let variants = build_every_structured_type_expression_variant();
    assert_eq!(
        variants.len(),
        22,
        "StructuredTypeExpression covers exactly 22 oneof variants — the audited closed taxonomy",
    );

    for (idx, original) in variants.iter().enumerate() {
        let bytes = original.encode_to_vec();
        let decoded = g::StructuredTypeExpression::decode(bytes.as_slice())
            .unwrap_or_else(|e| panic!("expr variant {idx} should decode: {e}"));
        assert_eq!(decoded, *original, "expr variant {idx} must roundtrip");
        assert!(
            decoded.kind.is_some(),
            "expr variant {idx} kind must survive"
        );
    }
}

#[test]
fn semantic_type_graph_roundtrip_preserves_envelope_shape() {
    let graph = g::SemanticTypeGraph {
        schema_version: g::TYPEINFO_GRAPH_SCHEMA_VERSION,
        query: Some(sample_query_identity()),
        nodes: build_every_type_node_variant(),
        symbols: vec![g::SymbolNode {
            name_id: 1,
            canonical_name_id: 2,
            namespace: g::SymbolNamespace::Type as i32,
            decl_slot_ref: 3,
        }],
        signatures: vec![sample_signature()],
        edges: vec![g::OriginEdge {
            source_node_id: 0,
            target_node_id: 1,
            kind: g::OriginEdgeKind::Declares as i32,
            meta_name_id: 4,
            has_meta: true,
        }],
        root_ids: vec![0],
        exactness: vec![g::NodeStatus {
            node_id: 0,
            exactness: g::Exactness::ExactResolved as i32,
        }],
        diagnostics: vec![g::Diagnostic {
            severity: g::DiagnosticSeverity::Warn as i32,
            message_name_id: 5,
            span_canonical_name_id: 6,
            span_start: 7,
            span_end: 11,
            has_span: true,
        }],
        node_id_map: vec![g::NodeIdMapEntry {
            node_id: 0,
            identity: Some(sample_decl_identity()),
        }],
        symbol_id_map: vec![g::SymbolIdMapEntry {
            symbol_id: 0,
            identity: Some(sample_decl_identity()),
        }],
        strings: Some(g::StringTable {
            entries: vec!["a".to_string(), "b".to_string()],
        }),
    };

    let bytes = graph.encode_to_vec();
    let decoded =
        g::SemanticTypeGraph::decode(bytes.as_slice()).expect("SemanticTypeGraph must roundtrip");
    assert_eq!(decoded, graph);
    assert_eq!(decoded.schema_version, g::TYPEINFO_GRAPH_SCHEMA_VERSION);
    assert_eq!(decoded.nodes.len(), 32);
}

#[test]
fn type_info_graph_request_roundtrip_covers_every_payload_arm() {
    use verter_protocol::verter::v1::type_info_graph_request::Payload;

    let arms: Vec<Payload> = vec![
        Payload::ResolveSymbol(sample_resolve_symbol_request()),
        Payload::EvaluateTypeExpression(sample_evaluate_type_expression_request()),
        Payload::ProjectPath(sample_project_path_request()),
        Payload::FlowNarrowing(sample_flow_narrowing_request()),
        Payload::ContextualType(sample_contextual_type_request()),
        Payload::ExpandAround(sample_expand_around_request()),
        Payload::FrameworkSurface(sample_framework_surface_request()),
    ];
    assert_eq!(
        arms.len(),
        7,
        "TypeInfoGraphRequest covers exactly 7 graph operation payload arms",
    );

    for (idx, payload) in arms.into_iter().enumerate() {
        let request = g::TypeInfoGraphRequest {
            schema_version: g::TYPEINFO_GRAPH_SCHEMA_VERSION,
            operation: graph_operation_for_payload_index(idx) as i32,
            payload: Some(payload),
        };
        let bytes = request.encode_to_vec();
        let decoded = g::TypeInfoGraphRequest::decode(bytes.as_slice())
            .unwrap_or_else(|e| panic!("request arm {idx} should decode: {e}"));
        assert_eq!(decoded, request);
        assert!(
            decoded.payload.is_some(),
            "request arm {idx} payload must survive"
        );
    }
}

#[test]
fn type_info_request_error_roundtrip_covers_every_variant() {
    use verter_protocol::verter::v1::type_info_request_error::Kind;

    let variants: Vec<Kind> = vec![
        Kind::MissingProjectionContext(g::wire_error_missing_projection_context()),
        Kind::MissingDisplayPolicy(g::wire_error_missing_display_policy()),
        Kind::InvalidMode(g::wire_error_invalid_mode("bogus")),
        Kind::MissingClosurePolicy(g::wire_error_missing_closure_policy()),
        Kind::UnknownSchemaVersion(g::wire_error_unknown_schema_version(7, 1, &[1])),
        Kind::MalformedPayload(g::wire_error_malformed_payload("boom")),
        Kind::OmittedRoots(g::wire_error_omitted_roots()),
        Kind::UnstableState(g::wire_error_unstable_state(3)),
        Kind::MalformedStructuredExpression(g::wire_error_malformed_structured_expression("cycle")),
        Kind::MissingProjectPath(g::wire_error_missing_project_path()),
        Kind::ExpansionBudgetOutOfRange(g::wire_error_expansion_budget_out_of_range(
            5000, 256, 4096, 64,
        )),
    ];
    assert_eq!(
        variants.len(),
        11,
        "TypeInfoRequestError covers exactly 11 closed variants (field 11 reserved)",
    );

    for (idx, kind) in variants.into_iter().enumerate() {
        let error = g::TypeInfoRequestError { kind: Some(kind) };
        let bytes = error.encode_to_vec();
        let decoded = g::TypeInfoRequestError::decode(bytes.as_slice())
            .unwrap_or_else(|e| panic!("error variant {idx} should decode: {e}"));
        assert_eq!(decoded, error);
        assert!(
            decoded.kind.is_some(),
            "error variant {idx} kind must survive"
        );
    }
}

#[test]
fn capability_handshake_roundtrips() {
    let request = g::TypeInfoCapabilityHandshakeRequest { client_version: 4 };
    let bytes = request.encode_to_vec();
    let decoded = g::TypeInfoCapabilityHandshakeRequest::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded, request);

    let response = g::TypeInfoCapabilityHandshakeResponse {
        server_version: g::TYPEINFO_GRAPH_SCHEMA_VERSION,
        server_supported_versions: vec![1, 2, 3],
    };
    let bytes = response.encode_to_vec();
    let decoded = g::TypeInfoCapabilityHandshakeResponse::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded, response);
}

#[test]
fn framework_surface_payload_roundtrips() {
    let payload = g::FrameworkSurfacePayload {
        schema_version: g::TYPEINFO_GRAPH_SCHEMA_VERSION,
        selector: Some(sample_component_selector()),
        framework: g::FrameworkTag::Vue as i32,
        graph: Some(g::SemanticTypeGraph {
            schema_version: g::TYPEINFO_GRAPH_SCHEMA_VERSION,
            query: Some(sample_query_identity()),
            nodes: vec![g::TypeNode {
                kind: Some(g::TypeNodeKind::Primitive(g::PrimitiveNode {
                    kind: g::PrimitiveKind::String as i32,
                })),
            }],
            ..Default::default()
        }),
        surfaces: vec![g::FrameworkSurfaceKindEntry {
            kind: g::FrameworkSurfaceKind::Props as i32,
            members: vec![g::FrameworkSurfaceMember {
                name_id: 1,
                type_node_id: 0,
                required: true,
                readonly: false,
            }],
            status: Some(supported_status()),
        }],
    };

    let bytes = payload.encode_to_vec();
    let decoded = g::FrameworkSurfacePayload::decode(bytes.as_slice())
        .expect("FrameworkSurfacePayload must roundtrip");
    assert_eq!(decoded, payload);
}

fn supported_status() -> g::FrameworkSurfaceKindStatus {
    g::FrameworkSurfaceKindStatus {
        support: g::FrameworkSurfaceKindSupport::Supported as i32,
        exactness: g::Exactness::ExactResolved as i32,
        diagnostics: vec![],
    }
}

fn unsupported_status() -> g::FrameworkSurfaceKindStatus {
    g::FrameworkSurfaceKindStatus {
        support: g::FrameworkSurfaceKindSupport::Unsupported as i32,
        exactness: g::Exactness::Unsupported as i32,
        diagnostics: vec![g::Diagnostic {
            severity: g::DiagnosticSeverity::Warn as i32,
            message_name_id: 9,
            span_canonical_name_id: 0,
            span_start: 0,
            span_end: 0,
            has_span: false,
        }],
    }
}

/// Every known `FrameworkSurfaceKind`, in tag order. A v3
/// framework-surface response carries EXACTLY ONE entry per kind.
const EVERY_FRAMEWORK_SURFACE_KIND: &[g::FrameworkSurfaceKind] = &[
    g::FrameworkSurfaceKind::Props,
    g::FrameworkSurfaceKind::Emits,
    g::FrameworkSurfaceKind::Slots,
    g::FrameworkSurfaceKind::Options,
    g::FrameworkSurfaceKind::Expose,
    g::FrameworkSurfaceKind::Model,
];

#[test]
fn type_info_graph_response_roundtrip_covers_every_kind_arm() {
    use verter_protocol::verter::v1::type_info_graph_response::Kind;

    let framework_surface = g::FrameworkSurfacePayload {
        schema_version: g::TYPEINFO_GRAPH_SCHEMA_VERSION,
        selector: Some(sample_component_selector()),
        framework: g::FrameworkTag::Vue as i32,
        graph: Some(g::SemanticTypeGraph {
            schema_version: g::TYPEINFO_GRAPH_SCHEMA_VERSION,
            ..Default::default()
        }),
        // Exactly one entry per known kind, each carrying a per-kind
        // status (UNSPECIFIED is invalid in server-produced v3
        // payloads).
        surfaces: EVERY_FRAMEWORK_SURFACE_KIND
            .iter()
            .map(|kind| g::FrameworkSurfaceKindEntry {
                kind: *kind as i32,
                members: vec![],
                status: Some(supported_status()),
            })
            .collect(),
    };

    let arms: Vec<Kind> = vec![
        Kind::Graph(g::SemanticTypeGraph {
            schema_version: g::TYPEINFO_GRAPH_SCHEMA_VERSION,
            ..Default::default()
        }),
        Kind::Error(g::TypeInfoRequestError {
            kind: Some(
                verter_protocol::verter::v1::type_info_request_error::Kind::MalformedPayload(
                    g::wire_error_malformed_payload("boom"),
                ),
            ),
        }),
        Kind::FrameworkSurface(framework_surface),
    ];
    assert_eq!(
        arms.len(),
        3,
        "TypeInfoGraphResponse covers exactly 3 closed oneof arms",
    );

    for (idx, kind) in arms.into_iter().enumerate() {
        let response = g::TypeInfoGraphResponse { kind: Some(kind) };
        let bytes = response.encode_to_vec();
        let decoded = g::TypeInfoGraphResponse::decode(bytes.as_slice())
            .unwrap_or_else(|e| panic!("response arm {idx} should decode: {e}"));
        assert_eq!(decoded, response, "response arm {idx} must roundtrip");
        assert!(
            decoded.kind.is_some(),
            "response arm {idx} kind must survive"
        );
    }
}

/// The §9 contract's wire proof: a kind entry with `SUPPORTED` +
/// empty members (supported-empty) and one with `UNSUPPORTED` + empty
/// members decode to DISTINCT typed states — an empty member list
/// alone never means "unsupported".
#[test]
fn supported_empty_and_unsupported_empty_decode_to_distinct_states() {
    let supported_empty = g::FrameworkSurfaceKindEntry {
        kind: g::FrameworkSurfaceKind::Slots as i32,
        members: vec![],
        status: Some(supported_status()),
    };
    let unsupported_empty = g::FrameworkSurfaceKindEntry {
        kind: g::FrameworkSurfaceKind::Slots as i32,
        members: vec![],
        status: Some(unsupported_status()),
    };

    let decoded_supported =
        g::FrameworkSurfaceKindEntry::decode(supported_empty.encode_to_vec().as_slice())
            .expect("supported-empty entry must decode");
    let decoded_unsupported =
        g::FrameworkSurfaceKindEntry::decode(unsupported_empty.encode_to_vec().as_slice())
            .expect("unsupported-empty entry must decode");

    // Both decode with empty member lists…
    assert!(decoded_supported.members.is_empty());
    assert!(decoded_unsupported.members.is_empty());
    // …yet remain distinct typed states.
    assert_ne!(decoded_supported, decoded_unsupported);

    let supported_state = decoded_supported.status.expect("status must survive");
    assert_eq!(
        supported_state.support,
        g::FrameworkSurfaceKindSupport::Supported as i32,
        "SUPPORTED + empty members = supported-empty (members authoritative)",
    );
    assert!(supported_state.diagnostics.is_empty());

    let unsupported_state = decoded_unsupported.status.expect("status must survive");
    assert_eq!(
        unsupported_state.support,
        g::FrameworkSurfaceKindSupport::Unsupported as i32,
    );
    assert_eq!(
        unsupported_state.exactness,
        g::Exactness::Unsupported as i32,
        "UNSUPPORTED carries `exactness = UNSUPPORTED`",
    );
    assert!(
        !unsupported_state.diagnostics.is_empty(),
        "UNSUPPORTED carries at least one diagnostic",
    );
}

// ---------------------------------------------------------------------------
// Discriminating cardinality guards
// ---------------------------------------------------------------------------

#[test]
fn closed_taxonomies_have_the_documented_cardinalities() {
    // Any drop / unintended add lights up immediately because the
    // variant constructors below must match these numbers.
    assert_eq!(build_every_type_node_variant().len(), 32);
    assert_eq!(build_every_structured_type_expression_variant().len(), 22);

    // Primitive kinds: 12 (ANY..OBJECT).
    let primitives: &[g::PrimitiveKind] = &[
        g::PrimitiveKind::Any,
        g::PrimitiveKind::Unknown,
        g::PrimitiveKind::Never,
        g::PrimitiveKind::Void,
        g::PrimitiveKind::Null,
        g::PrimitiveKind::Undefined,
        g::PrimitiveKind::String,
        g::PrimitiveKind::Number,
        g::PrimitiveKind::Boolean,
        g::PrimitiveKind::Bigint,
        g::PrimitiveKind::Symbol,
        g::PrimitiveKind::Object,
    ];
    assert_eq!(primitives.len(), 12);

    // Exactness: 9 statuses (matches the audit payload counters —
    // drift between them is a discriminating failure).
    let exactness: &[g::Exactness] = &[
        g::Exactness::ExactResolved,
        g::Exactness::ExactSymbolic,
        g::Exactness::UnresolvedGeneric,
        g::Exactness::Partial,
        g::Exactness::Miss,
        g::Exactness::Unsupported,
        g::Exactness::BudgetExceeded,
        g::Exactness::Unstable,
        g::Exactness::Cycle,
    ];
    assert_eq!(exactness.len(), 9);

    // Origin edges: 10 kinds. The audit-side origin graph in
    // `verter_audit::origin_graph` defines the canonical taxonomy;
    // the typeinfo wire surface MUST track it pairwise so the
    // audit and wire views agree on every edge kind.
    let origin: &[g::OriginEdgeKind] = &[
        g::OriginEdgeKind::Declares,
        g::OriginEdgeKind::Instantiates,
        g::OriginEdgeKind::References,
        g::OriginEdgeKind::MemberOf,
        g::OriginEdgeKind::ResolvesTo,
        g::OriginEdgeKind::SharedLoadReuse,
        g::OriginEdgeKind::Fallthrough,
        g::OriginEdgeKind::RelationProofStep,
        g::OriginEdgeKind::BackEdgeCycle,
        g::OriginEdgeKind::AugmentationStitch,
    ];
    assert_eq!(origin.len(), 10);

    // Framework tags: 6 — a tag value's existence is NOT a support
    // guarantee, and new tag values land only together with their
    // adapter's vertical (no additions ride the schema-3 bump).
    let framework_tags: &[g::FrameworkTag] = &[
        g::FrameworkTag::None,
        g::FrameworkTag::Vue,
        g::FrameworkTag::Svelte,
        g::FrameworkTag::React,
        g::FrameworkTag::Solid,
        g::FrameworkTag::OpenCanonical,
    ];
    assert_eq!(framework_tags.len(), 6);

    // Per-kind support: 4 (UNSPECIFIED / SUPPORTED / UNSUPPORTED /
    // PARTIAL). UNSPECIFIED is invalid in server-produced v3 payloads.
    let support: &[g::FrameworkSurfaceKindSupport] = &[
        g::FrameworkSurfaceKindSupport::Unspecified,
        g::FrameworkSurfaceKindSupport::Supported,
        g::FrameworkSurfaceKindSupport::Unsupported,
        g::FrameworkSurfaceKindSupport::Partial,
    ];
    assert_eq!(support.len(), 4);
}

// ---------------------------------------------------------------------------
// Constructor helpers
// ---------------------------------------------------------------------------

fn build_every_type_node_variant() -> Vec<g::TypeNode> {
    use g::TypeNodeKind as K;

    let kinds: Vec<K> = vec![
        K::Primitive(g::PrimitiveNode {
            kind: g::PrimitiveKind::String as i32,
        }),
        K::Literal(g::LiteralNode {
            value: Some(g::LiteralValue {
                kind: Some(
                    verter_protocol::verter::v1::graph_literal_value::Kind::BooleanValue(true),
                ),
            }),
        }),
        K::UniqueSymbol(g::UniqueSymbolNode { decl_symbol_id: 1 }),
        K::Union(g::UnionNode {
            member_node_ids: vec![0, 1],
        }),
        K::Intersection(g::IntersectionNode {
            member_node_ids: vec![0, 2],
        }),
        K::Object(g::ObjectNode {
            members: vec![g::ObjectMember {
                name_id: 1,
                name_kind: g::MemberNameKind::Identifier as i32,
                value_node_id: 0,
                optional: false,
                readonly: true,
                accessibility: g::Accessibility::Public as i32,
                static_side: false,
                declaration_symbol_id: 2,
            }],
            index_signatures: vec![g::IndexSignature {
                key_kind: g::IndexKeyKind::String as i32,
                value_node_id: 0,
                readonly: false,
            }],
            call_signature_refs: vec![0],
            construct_signature_refs: vec![],
            flags: 0,
        }),
        K::Array(g::ArrayNode {
            element_node_id: 0,
            readonly: false,
        }),
        K::Tuple(g::TupleNode {
            elements: vec![g::TupleElement {
                label_name_id: 0,
                value_node_id: 0,
                optional: false,
                rest: false,
            }],
            readonly: true,
        }),
        K::Reference(g::ReferenceNode { symbol_id: 1 }),
        K::AliasInstantiation(g::AliasInstantiationNode {
            alias_symbol_id: 3,
            type_argument_node_ids: vec![0],
            target_node_id: 0,
            display_ref_node_id: 0,
        }),
        K::TypeParameter(g::TypeParameterNode {
            symbol_id: 4,
            decl_slot_ref: 5,
            param_index: 0,
            name_id: 6,
            constraint_node_id: 0,
            default_node_id: 0,
            variance: g::Variance::Independent as i32,
            is_const: false,
            no_infer: false,
            binding: Some(g::TypeParameterBinding {
                kind: Some(
                    verter_protocol::verter::v1::graph_type_parameter_binding::Kind::Unbound(
                        verter_protocol::verter::v1::GraphTypeParameterBindingUnbound {},
                    ),
                ),
            }),
        }),
        K::KeyOf(g::KeyOfNode { base_node_id: 0 }),
        K::IndexedAccess(g::IndexedAccessNode {
            object_node_id: 0,
            index_node_id: 0,
        }),
        K::Conditional(g::ConditionalNode {
            check_node_id: 0,
            extends_node_id: 0,
            true_branch_node_id: 0,
            false_branch_node_id: 0,
            distributive: false,
            resolution: Some(g::ConditionalResolution {
                kind: Some(
                    verter_protocol::verter::v1::graph_conditional_resolution::Kind::SelectedTrue(
                        verter_protocol::verter::v1::GraphConditionalResolutionSelected {
                            proof_ref: 0,
                        },
                    ),
                ),
            }),
        }),
        K::Mapped(g::MappedNode {
            key_type_node_id: 0,
            source_node_id: 0,
            name_remap_node_id: 0,
            value_type_node_id: 0,
            readonly_modifier: g::MappedModifier::None as i32,
            optional_modifier: g::MappedModifier::Add as i32,
        }),
        K::TemplateLiteral(g::TemplateLiteralNode {
            quasi_name_ids: vec![1, 2],
            expression_node_ids: vec![0],
        }),
        K::TypeofNode(g::TypeOfNode {
            value_root_ref: 7,
            path_name_ids: vec![8, 9],
        }),
        K::SatisfiesNode(g::SatisfiesNode {
            value_node_id: 0,
            constraint_node_id: 0,
        }),
        K::ClassNode(g::ClassNode {
            symbol_id: 10,
            type_parameter_node_ids: vec![],
            heritage: vec![],
            members: vec![],
            static_members: vec![],
            construct_signature_refs: vec![],
            flags: 0,
        }),
        K::ThisType(g::ThisTypeNode { decl_symbol_id: 11 }),
        K::MergedDeclaration(g::MergedDeclarationNode {
            merged_symbol_id: 12,
            parts: vec![g::DeclarationPart {
                source_canonical_name_id: 13,
                declaration_node_id: 0,
                kind: g::DeclarationPartKind::Interface as i32,
            }],
        }),
        K::AmbientModule(g::AmbientModuleNode {
            specifier_name_id: 14,
            module_namespace_node_id: 0,
        }),
        K::ModuleAugmentation(g::ModuleAugmentationNode {
            specifier_name_id: 15,
            parts: vec![],
        }),
        K::AmbientNamespace(g::AmbientNamespaceNode {
            namespace_name_id: 16,
            namespace_node_id: 0,
        }),
        K::GlobalAugmentation(g::GlobalAugmentationNode { parts: vec![] }),
        K::FlowNarrowing(g::FlowNarrowingNode {
            site_span_ref: 17,
            narrowed_node_id: 0,
            base_node_id: 0,
        }),
        K::ContextualType(g::ContextualTypeNode {
            site_span_ref: 18,
            contextual_node_id: 0,
        }),
        K::RelationProof(g::RelationProofNode {
            outcome: g::RelationOutcome::True as i32,
            steps: vec![g::RelationStep {
                kind: g::RelationStepKind::Structural as i32,
                source_node_id: 0,
                target_node_id: 0,
            }],
        }),
        K::InferNode(g::InferNode {
            name_id: 19,
            constraint_node_id: 0,
        }),
        K::EnumNode(g::EnumNode {
            symbol_id: 20,
            members: vec![g::EnumMember {
                name_id: 21,
                value: Some(g::EnumMemberValue {
                    kind: Some(
                        verter_protocol::verter::v1::graph_enum_member_value::Kind::Numeric(42),
                    ),
                }),
            }],
            is_const: false,
        }),
        K::Opaque(g::OpaqueNode {
            error: Some(g::QueryError {
                kind: Some(verter_protocol::verter::v1::graph_query_error::Kind::Miss(
                    verter_protocol::verter::v1::GraphQueryErrorMiss {},
                )),
            }),
        }),
        K::Cycle(g::CycleNode {
            cycle_root_node_id: 0,
            participants: vec![22],
        }),
    ];

    kinds
        .into_iter()
        .map(|k| g::TypeNode { kind: Some(k) })
        .collect()
}

fn build_every_structured_type_expression_variant() -> Vec<g::StructuredTypeExpression> {
    use g::StructuredTypeExpressionKind as K;

    let kinds: Vec<K> = vec![
        K::Reference(g::ExprReference {
            scope_canonical: "/a.ts".to_string(),
            name: "Foo".to_string(),
            type_arguments: vec![],
            extra_imports: vec![],
        }),
        K::Union(g::ExprUnion { members: vec![] }),
        K::Intersection(g::ExprIntersection { members: vec![] }),
        K::IndexedAccess(Box::new(g::ExprIndexedAccess {
            object: Some(Box::new(prim_string_expr())),
            index: Some(Box::new(prim_string_expr())),
        })),
        K::Keyof(Box::new(g::ExprKeyOf {
            operand: Some(Box::new(prim_string_expr())),
        })),
        K::TypeofExpr(g::ExprTypeOf {
            value_root_canonical: "/a.ts".to_string(),
            path: vec!["x".to_string()],
        }),
        K::Tuple(g::ExprTuple {
            elements: vec![g::TupleElementExpr {
                label: String::new(),
                has_label: false,
                value: Some(prim_string_expr()),
                optional_element: false,
                rest: false,
            }],
            readonly: true,
        }),
        K::Array(Box::new(g::ExprArray {
            element: Some(Box::new(prim_string_expr())),
            readonly: false,
        })),
        K::ObjectLiteral(g::ExprObject {
            members: vec![g::ObjectMemberExpr {
                name: "x".to_string(),
                name_kind: g::MemberNameKind::Identifier as i32,
                value: Some(prim_string_expr()),
                optional_member: false,
                readonly: false,
            }],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        }),
        K::Mapped(Box::new(g::ExprMapped {
            type_param: Some(Box::new(g::MappedTypeParamExpr {
                binder_id: "T".to_string(),
                name: "T".to_string(),
                constraint: Some(Box::new(prim_string_expr())),
            })),
            name_remap: None,
            has_name_remap: false,
            value_type: Some(Box::new(prim_string_expr())),
            readonly_modifier: g::MappedModifier::None as i32,
            optional_modifier: g::MappedModifier::None as i32,
        })),
        K::Conditional(Box::new(g::ExprConditional {
            check: Some(Box::new(prim_string_expr())),
            extends_type: Some(Box::new(prim_string_expr())),
            true_branch: Some(Box::new(prim_string_expr())),
            false_branch: Some(Box::new(prim_string_expr())),
        })),
        K::Literal(g::ExprLiteral {
            value: Some(g::LiteralValue {
                kind: Some(
                    verter_protocol::verter::v1::graph_literal_value::Kind::BooleanValue(false),
                ),
            }),
        }),
        K::Primitive(g::ExprPrimitive {
            kind: g::PrimitiveKind::Number as i32,
        }),
        K::TemplateLiteral(g::ExprTemplateLiteral {
            quasis: vec!["a".to_string()],
            expressions: vec![prim_string_expr()],
        }),
        K::InferExpr(Box::new(g::ExprInfer {
            name: "U".to_string(),
            constraint: None,
            has_constraint: false,
        })),
        K::FunctionExpr(Box::new(g::ExprFunction {
            type_parameters: vec![],
            this_param: None,
            has_this_param: false,
            parameters: vec![],
            return_expr: Some(Box::new(g::FunctionReturnExpr {
                kind: Some(
                    verter_protocol::verter::v1::function_return_expr::Kind::Type(Box::new(
                        prim_string_expr(),
                    )),
                ),
            })),
            signature_kind: g::SignatureKind::Call as i32,
        })),
        K::ClassExpr(g::ExprClass {
            class_name: String::new(),
            has_class_name: false,
            type_parameters: vec![],
            instance_members: vec![],
            static_members: vec![],
        }),
        K::ThisType(g::ExprThisType {}),
        K::SatisfiesExpr(Box::new(g::ExprSatisfies {
            value: Some(Box::new(prim_string_expr())),
            constraint: Some(Box::new(prim_string_expr())),
        })),
        K::UniqueSymbol(g::ExprUniqueSymbol {
            decl_canonical: "/a.ts".to_string(),
            name: "id".to_string(),
        }),
        K::NoInfer(Box::new(g::ExprNoInfer {
            inner: Some(Box::new(prim_string_expr())),
        })),
        K::LocalTypeRef(g::ExprLocalTypeRef {
            binder_id: "T".to_string(),
        }),
    ];

    kinds
        .into_iter()
        .map(|k| g::StructuredTypeExpression { kind: Some(k) })
        .collect()
}

fn prim_string_expr() -> g::StructuredTypeExpression {
    g::StructuredTypeExpression {
        kind: Some(g::StructuredTypeExpressionKind::Primitive(
            g::ExprPrimitive {
                kind: g::PrimitiveKind::String as i32,
            },
        )),
    }
}

fn sample_query_identity() -> g::QueryIdentity {
    g::QueryIdentity {
        operation: g::Operation::ResolveSymbol as i32,
        resolved_roots: vec![g::ResolvedDeclSlotIdentityDto {
            canonical_name_id: 1,
            decl_name_id: 2,
            whole_hash: vec![0xAB, 0xCD],
            namespace: g::SymbolNamespace::Type as i32,
        }],
        path: vec![],
        closure: Some(g::ClosurePolicy {
            kind: Some(
                verter_protocol::verter::v1::graph_closure_policy::Kind::Expanded(
                    g::ClosureExpanded {
                        node_budget: 4096,
                        depth_budget: 64,
                    },
                ),
            ),
        }),
        context: Some(g::ProjectionReductionContext {
            mode: g::ProjectionMode::Expanded as i32,
            demand: g::ReductionDemand::Published as i32,
        }),
        display_policy: Some(sample_display_policy()),
        substitutions: vec![],
        solver_options_hash: vec![0u8; 16],
        parse_env_hash: vec![0u8; 16],
        resolve_env_hash: vec![0u8; 16],
        type_env_hash: vec![0u8; 16],
        lib_env_hash: vec![0u8; 16],
        project_identity: vec![0u8; 16],
        resolver_version: 1,
        include_provenance: true,
        include_diagnostics: true,
        include_projection: vec![g::ProjectionKind::Display as i32],
    }
}

fn sample_display_policy() -> g::DisplayPolicy {
    g::DisplayPolicy {
        qualification: g::DisplayQualification::Qualified as i32,
        branding: g::DisplayBranding::On as i32,
        budgets: Some(g::DisplayBudgets {
            max_string_length: 4096,
            max_depth: 16,
        }),
    }
}

fn sample_decl_identity() -> g::ResolvedDeclSlotIdentityDto {
    g::ResolvedDeclSlotIdentityDto {
        canonical_name_id: 1,
        decl_name_id: 2,
        whole_hash: vec![0u8; 16],
        namespace: g::SymbolNamespace::Type as i32,
    }
}

fn sample_signature() -> g::Signature {
    g::Signature {
        type_parameter_node_ids: vec![],
        this_param: Some(g::ThisParameter {
            present: false,
            type_node_id: 0,
        }),
        parameters: vec![],
        return_type_node_id: 0,
        return_predicate: Some(g::TypePredicate {
            present: false,
            subject: None,
            predicate_type_node_id: 0,
            asserts: false,
        }),
        asserts: Some(g::AssertionEffect {
            present: false,
            kind: None,
        }),
        overload_index: 0,
        is_construct: false,
        is_implementation: false,
        is_abstract: false,
        flags: 0,
        signature_kind: g::SignatureKind::Call as i32,
        signature_origin: g::SignatureOrigin::FunctionDeclaration as i32,
    }
}

fn sample_resolve_symbol_request() -> g::ResolveSymbolGraphRequest {
    g::ResolveSymbolGraphRequest {
        canonical_id: "/a.ts".to_string(),
        name: "Foo".to_string(),
        context: Some(g::ProjectionReductionContext {
            mode: g::ProjectionMode::Expanded as i32,
            demand: g::ReductionDemand::Published as i32,
        }),
        closure: Some(g::ClosurePolicy {
            kind: Some(
                verter_protocol::verter::v1::graph_closure_policy::Kind::OneLevel(
                    g::ClosureOneLevel {},
                ),
            ),
        }),
        display_policy: Some(sample_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
        include_degraded: false,
    }
}

fn sample_evaluate_type_expression_request() -> g::EvaluateTypeExpressionGraphRequest {
    g::EvaluateTypeExpressionGraphRequest {
        scope_canonical: "/a.ts".to_string(),
        expression: Some(prim_string_expr()),
        extra_imports: vec![],
        context: Some(g::ProjectionReductionContext {
            mode: g::ProjectionMode::Expanded as i32,
            demand: g::ReductionDemand::Published as i32,
        }),
        closure: Some(g::ClosurePolicy {
            kind: Some(
                verter_protocol::verter::v1::graph_closure_policy::Kind::OneLevel(
                    g::ClosureOneLevel {},
                ),
            ),
        }),
        display_policy: Some(sample_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
    }
}

fn sample_project_path_request() -> g::ProjectPathGraphRequest {
    g::ProjectPathGraphRequest {
        canonical_id: "/a.ts".to_string(),
        name: "Foo".to_string(),
        path: vec![g::TypePathSegment {
            kind: Some(
                verter_protocol::verter::v1::graph_type_path_segment::Kind::Property(
                    g::wire_path_segment_property(7),
                ),
            ),
        }],
        context: Some(g::ProjectionReductionContext {
            mode: g::ProjectionMode::Expanded as i32,
            demand: g::ReductionDemand::Published as i32,
        }),
        closure: Some(g::ClosurePolicy {
            kind: Some(
                verter_protocol::verter::v1::graph_closure_policy::Kind::OneLevel(
                    g::ClosureOneLevel {},
                ),
            ),
        }),
        display_policy: Some(sample_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
        include_degraded: false,
    }
}

fn sample_flow_narrowing_request() -> g::FlowNarrowingRequest {
    g::FlowNarrowingRequest {
        canonical_id: "/a.ts".to_string(),
        span: Some(g::SpanRef {
            canonical_id: "/a.ts".to_string(),
            start: 1,
            end: 4,
        }),
        context: Some(g::ProjectionReductionContext {
            mode: g::ProjectionMode::Expanded as i32,
            demand: g::ReductionDemand::Published as i32,
        }),
        display_policy: Some(sample_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
    }
}

fn sample_contextual_type_request() -> g::ContextualTypeRequest {
    g::ContextualTypeRequest {
        canonical_id: "/a.ts".to_string(),
        span: Some(g::SpanRef {
            canonical_id: "/a.ts".to_string(),
            start: 1,
            end: 4,
        }),
        context: Some(g::ProjectionReductionContext {
            mode: g::ProjectionMode::Expanded as i32,
            demand: g::ReductionDemand::Published as i32,
        }),
        display_policy: Some(sample_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
    }
}

fn sample_expand_around_request() -> g::ExpandGraphAroundRequest {
    g::ExpandGraphAroundRequest {
        parent_graph: Some(g::Handle {
            opaque: vec![1, 2, 3],
        }),
        target: Some(g::TypeNodeRef {
            node_id: 3,
            identity: None,
            is_canonical: false,
        }),
        context: Some(g::ProjectionReductionContext {
            mode: g::ProjectionMode::Expanded as i32,
            demand: g::ReductionDemand::Published as i32,
        }),
        closure: Some(g::ClosurePolicy {
            kind: Some(
                verter_protocol::verter::v1::graph_closure_policy::Kind::OneLevel(
                    g::ClosureOneLevel {},
                ),
            ),
        }),
        display_policy: Some(sample_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
    }
}

fn sample_framework_surface_request() -> g::FrameworkSurfaceRequest {
    g::FrameworkSurfaceRequest {
        selector: Some(sample_component_selector()),
        context: Some(g::ProjectionReductionContext {
            mode: g::ProjectionMode::Expanded as i32,
            demand: g::ReductionDemand::Published as i32,
        }),
        closure: Some(g::ClosurePolicy {
            kind: Some(
                verter_protocol::verter::v1::graph_closure_policy::Kind::OneLevel(
                    g::ClosureOneLevel {},
                ),
            ),
        }),
        display_policy: Some(sample_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
        schema_version: g::TYPEINFO_GRAPH_SCHEMA_VERSION,
    }
}

fn sample_component_selector() -> g::ComponentSelector {
    g::ComponentSelector {
        canonical_id: "/Foo.vue".to_string(),
        export_name: String::new(),
        has_export_name: false,
        framework_adapter_id: "vue".to_string(),
    }
}

fn graph_operation_for_payload_index(idx: usize) -> g::Operation {
    match idx {
        0 => g::Operation::ResolveSymbol,
        1 => g::Operation::EvaluateExpression,
        2 => g::Operation::ProjectPath,
        3 => g::Operation::FlowNarrowingAt,
        4 => g::Operation::ContextualTypeAt,
        5 => g::Operation::ExpandAround,
        6 => g::Operation::FrameworkSurfaces,
        _ => unreachable!("only 7 graph operation payload arms exist"),
    }
}
