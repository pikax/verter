//! Discriminating coverage for the resolve-symbol typeinfo graph
//! operation executor (`VerterHost::resolve_symbol_graph_with_audit`).
//!
//! The executor is the graph-protocol answer route for the
//! resolve-symbol operation: it runs the envelope validator FIRST (a
//! malformed envelope gets the typed wire `error` arm before any
//! semantic work), then the ONE shared resolution engine, then the
//! bounded terminal `TypeExpr` -> `SemanticTypeGraph` export — the
//! operation-DTO answer that displaces the general `TypeExpr` JSON
//! transit on this route.
//!
//! Discriminating boundaries pinned here:
//! - validation-first: a malformed envelope answers with the typed
//!   `error` arm and NEVER reaches resolution;
//! - an operation this entry does not serve is refused fail-closed (no
//!   silent dual route, no second evaluator);
//! - a well-formed resolve answers with the `graph` arm: the resolved
//!   terminal type as the bounded exported graph, query identity
//!   echoed, deterministic interned strings;
//! - a non-fault miss is a typed `graph`-arm answer (an opaque `Miss`
//!   root), NOT an error and NOT an empty graph;
//! - the audit record rides both arms (`TypeInfoGraph` kind, the
//!   resolve-symbol operation tag, snapshot node count).

use std::sync::Arc;

use verter_audit::payloads::typeinfo_graph::GraphOperationTag;
use verter_audit::RequestKind;
use verter_protocol::typeinfo::graph::{
    self as wire, Operation, ProjectionMode, TYPEINFO_GRAPH_SCHEMA_VERSION,
};
use verter_protocol::typeinfo::graph_export::UNBOUNDED_SENTINEL_BUDGET;
use verter_protocol::typeinfo::TypeInfoRequest;
use verter_protocol::verter::v1::{
    graph_closure_policy, graph_type_node, type_info_graph_request, type_info_graph_response,
    type_info_request_error,
};
use verter_session::{HostConfig, UpsertRequest, VerterHost};

const TS_FIXTURE: &str = "export type Foo = { msg: string };\n";

fn host_with_fixture() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/types.ts".to_string()),
        input_id: "/types.ts".to_string(),
        source: Arc::from(TS_FIXTURE),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static("/types.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

/// A well-formed resolve-symbol envelope for `(canonical, name)` under
/// the given wire projection mode.
fn resolve_envelope(canonical: &str, name: &str, mode: ProjectionMode) -> TypeInfoRequest {
    TypeInfoRequest {
        schema_version: TYPEINFO_GRAPH_SCHEMA_VERSION,
        operation: Operation::ResolveSymbol as i32,
        payload: Some(type_info_graph_request::Payload::ResolveSymbol(
            wire::ResolveSymbolGraphRequest {
                canonical_id: canonical.to_string(),
                name: name.to_string(),
                context: Some(wire::ProjectionReductionContext {
                    mode: mode as i32,
                    demand: wire::ReductionDemand::Published as i32,
                }),
                closure: Some(wire::ClosurePolicy {
                    kind: Some(graph_closure_policy::Kind::OneLevel(
                        wire::ClosureOneLevel {},
                    )),
                }),
                display_policy: Some(wire::DisplayPolicy {
                    qualification: wire::DisplayQualification::Qualified as i32,
                    branding: wire::DisplayBranding::On as i32,
                    budgets: Some(wire::DisplayBudgets {
                        max_string_length: 4096,
                        max_depth: 16,
                    }),
                }),
                include_provenance: false,
                include_diagnostics: true,
                include_projection: vec![],
                include_degraded: false,
            },
        )),
    }
}

#[test]
fn resolve_symbol_graph_answers_the_bounded_graph_arm() {
    let host = host_with_fixture();
    let (response, record) = host
        .resolve_symbol_graph_with_audit(resolve_envelope(
            "/types.ts",
            "Foo",
            ProjectionMode::Expanded,
        ))
        .into_parts();
    let response = response.expect("a well-formed resolve answers with the graph arm");
    assert_eq!(
        record.kind,
        RequestKind::TypeInfoGraph,
        "the graph operation audits under the graph kind"
    );

    let type_info_graph_response::Kind::Graph(graph) =
        response.kind.expect("the graph arm is present")
    else {
        panic!("the resolve-symbol operation answers with the graph arm");
    };
    assert_eq!(graph.schema_version, TYPEINFO_GRAPH_SCHEMA_VERSION);

    // Query identity echoes the operation that produced the graph.
    let query = graph.query.as_ref().expect("query identity present");
    assert_eq!(query.operation, Operation::ResolveSymbol as i32);

    // The resolved terminal type is the exported root: `{ msg: string }`.
    let root_id = graph.root_ids.first().copied().expect("one root");
    let root = &graph.nodes[root_id as usize];
    let graph_type_node::Kind::Object(object) = root.kind.as_ref().expect("root kind") else {
        panic!("the expanded Foo body is an object node");
    };
    assert_eq!(object.members.len(), 1);
    let entries = graph
        .strings
        .as_ref()
        .map(|t| t.entries.as_slice())
        .unwrap_or(&[] as &[String]);
    let key = object.members[0]
        .property_key
        .as_ref()
        .and_then(|k| k.key.as_ref());
    let Some(verter_protocol::verter::v1::graph_property_key::Key::StringId(id)) = key else {
        panic!("the msg member carries a string key");
    };
    assert_eq!(entries[*id as usize], "msg");
    let value = &graph.nodes[object.members[0].value_node_id as usize];
    assert!(
        matches!(value.kind, Some(graph_type_node::Kind::Primitive(_))),
        "msg: string"
    );
}

#[test]
fn validation_first_rejects_a_malformed_envelope_before_resolution() {
    let host = host_with_fixture();
    // Missing closure policy: the validator rejects before any semantic
    // work, so the response is the typed error arm.
    let mut envelope = resolve_envelope("/types.ts", "Foo", ProjectionMode::Expanded);
    if let Some(type_info_graph_request::Payload::ResolveSymbol(request)) =
        envelope.payload.as_mut()
    {
        request.closure = None;
    }
    let (response, _record) = host.resolve_symbol_graph_with_audit(envelope).into_parts();
    let error = response.expect_err("a malformed envelope is a typed error arm");
    assert!(matches!(
        error.kind,
        Some(type_info_request_error::Kind::MissingClosurePolicy(_))
    ));
}

#[test]
fn unbounded_expansion_is_structurally_rejected() {
    let host = host_with_fixture();
    // An expanded closure with budgets beyond the validator caps is
    // rejected with the typed out-of-range error — the bounded-export
    // contract refuses unbounded export requests structurally.
    let mut envelope = resolve_envelope("/types.ts", "Foo", ProjectionMode::Expanded);
    if let Some(type_info_graph_request::Payload::ResolveSymbol(request)) =
        envelope.payload.as_mut()
    {
        request.closure = Some(wire::ClosurePolicy {
            kind: Some(graph_closure_policy::Kind::Expanded(
                wire::ClosureExpanded {
                    node_budget: UNBOUNDED_SENTINEL_BUDGET,
                    depth_budget: UNBOUNDED_SENTINEL_BUDGET,
                },
            )),
        });
    }
    let (response, _record) = host.resolve_symbol_graph_with_audit(envelope).into_parts();
    let error = response.expect_err("unbounded budgets are rejected");
    assert!(matches!(
        error.kind,
        Some(type_info_request_error::Kind::ExpansionBudgetOutOfRange(_))
    ));
}

#[test]
fn an_unserved_operation_is_refused_fail_closed() {
    let host = host_with_fixture();
    // A VALIDATED evaluate-expression envelope (op and payload arm
    // coherent) reaching THIS entry is refused with a typed malformed
    // error naming the operation gate — this entry serves the
    // resolve-symbol operation only; it never silently re-routes to the
    // legacy string-expression evaluator (no dual-running authority).
    let envelope = TypeInfoRequest {
        schema_version: TYPEINFO_GRAPH_SCHEMA_VERSION,
        operation: Operation::EvaluateExpression as i32,
        payload: Some(type_info_graph_request::Payload::EvaluateTypeExpression(
            wire::EvaluateTypeExpressionGraphRequest {
                scope_canonical: "/types.ts".to_string(),
                expression: Some(wire::StructuredTypeExpression {
                    kind: Some(wire::StructuredTypeExpressionKind::Primitive(
                        wire::ExprPrimitive {
                            kind: wire::PrimitiveKind::String as i32,
                        },
                    )),
                }),
                extra_imports: Vec::new(),
                context: Some(wire::ProjectionReductionContext {
                    mode: ProjectionMode::Expanded as i32,
                    demand: wire::ReductionDemand::Published as i32,
                }),
                closure: Some(wire::ClosurePolicy {
                    kind: Some(graph_closure_policy::Kind::OneLevel(
                        wire::ClosureOneLevel {},
                    )),
                }),
                display_policy: Some(wire::DisplayPolicy {
                    qualification: wire::DisplayQualification::Qualified as i32,
                    branding: wire::DisplayBranding::On as i32,
                    budgets: Some(wire::DisplayBudgets {
                        max_string_length: 4096,
                        max_depth: 16,
                    }),
                }),
                include_provenance: false,
                include_diagnostics: true,
                include_projection: vec![],
            },
        )),
    };
    let (response, _record) = host.resolve_symbol_graph_with_audit(envelope).into_parts();
    let error = response.expect_err("an operation this entry does not serve is a typed error");
    match error.kind {
        Some(type_info_request_error::Kind::MalformedPayload(payload)) => {
            let detail = payload.detail;
            assert!(
                detail.contains("resolve-symbol operation only"),
                "the refusal names the operation gate, got {detail:?}"
            );
        }
        other => panic!("expected the typed malformed refusal, got {other:?}"),
    }
}

#[test]
fn an_op_payload_mismatch_is_rejected_by_the_envelope_validator() {
    let host = host_with_fixture();
    let mut envelope = resolve_envelope("/types.ts", "Foo", ProjectionMode::Expanded);
    envelope.operation = Operation::EvaluateExpression as i32;
    let _ = &mut envelope; // payload arm intentionally left as resolve_symbol
    let (response, _record) = host.resolve_symbol_graph_with_audit(envelope).into_parts();
    let error = response.expect_err("op/payload mismatch is a typed error");
    assert!(matches!(
        error.kind,
        Some(type_info_request_error::Kind::MalformedPayload(_))
    ));
}

#[test]
fn an_unresolved_symbol_is_a_typed_graph_fault_answer() {
    let host = host_with_fixture();
    let (response, _record) = host
        .resolve_symbol_graph_with_audit(resolve_envelope(
            "/types.ts",
            "DoesNotExist",
            ProjectionMode::Expanded,
        ))
        .into_parts();
    // The envelope was VALID — the resolution faulted (the engine
    // classifies an unknown symbol through its text-bearing fault
    // channel, the same classification the legacy route surfaces as a
    // dispatch fault). The graph-protocol answer is therefore the typed
    // `graph` arm carrying the fault as an opaque root — never the
    // `error` arm (that is reserved for envelope rejections) and never
    // an empty graph.
    let response = response.expect("a resolution fault is not an envelope error");
    let type_info_graph_response::Kind::Graph(graph) = response.kind.expect("graph arm") else {
        panic!("a resolution fault still answers with the graph arm");
    };
    let root_id = graph.root_ids.first().copied().expect("one root");
    let root = &graph.nodes[root_id as usize];
    match root.kind.as_ref() {
        Some(graph_type_node::Kind::Opaque(opaque)) => {
            // The fault is the TYPED opaque root; its context is interned
            // so the answer is self-describing on the wire.
            let has_typed_error = opaque.error.is_some();
            assert!(has_typed_error, "the opaque root carries a typed error");
            let entries = graph
                .strings
                .as_ref()
                .map(|t| t.entries.as_slice())
                .unwrap_or(&[] as &[String]);
            assert!(
                entries.first().is_some_and(|m| !m.is_empty()),
                "the fault context is interned, got {entries:?}"
            );
        }
        other => panic!("a fault degrades to an opaque root, got {other:?}"),
    }
}

#[test]
fn the_audit_payload_carries_the_resolve_symbol_operation_tag() {
    let host = host_with_fixture();
    let carrier = host.resolve_symbol_graph_with_audit(resolve_envelope(
        "/types.ts",
        "Foo",
        ProjectionMode::Expanded,
    ));
    let record = carrier.audit();
    assert_eq!(record.kind, RequestKind::TypeInfoGraph);
    match &record.kind_payload {
        verter_audit::RequestKindPayload::TypeInfoGraph(payload) => {
            assert_eq!(payload.operation, GraphOperationTag::ResolveSymbol);
            assert!(payload.snapshot_node_count > 0);
        }
        other => panic!("expected a TypeInfoGraph audit payload, got {other:?}"),
    }
}
