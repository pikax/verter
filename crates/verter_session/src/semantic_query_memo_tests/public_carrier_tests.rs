//! What a public delivery keeps alive. Every value, member, diagnostic and
//! cloned handle a consumer receives from the store owns the graph nodes it
//! names until its final holder drops, after the construction scope and the
//! cache that produced it are gone; a capability from another graph, or for
//! an ordinal its owner does not hold, reads nothing.

use std::cell::RefCell;
use std::sync::Arc;

use super::*;
use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount, StoreAccount};
use verter_type_engine::project_semantic_dispatch::walk::{QueryBuildOutput, ShallowDiagnostic};
use verter_type_engine::semantic_query::{
    AuthoredPropertyKey, CacheRead, LiteralValue, ResolveDeclKey, ScopeId, SurfaceMember,
    SurfaceView,
};
use verter_type_engine::semantic_query_memo::ownership_harness::CountedOwnershipHarness;
use verter_type_expr::TopLevelOwnerId;

const FILE: &str = "/public/carrier.ts";

fn host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(FILE.to_string()),
            input_id: FILE.to_string(),
            source: Arc::from("export interface Target { base: number; }\n"),
            file_language: crate::LanguageRegistry::global()
                .classify_static(FILE)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert of the fixture file succeeds");
    host
}

fn key(name: &str) -> SemanticQueryKey {
    SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from(FILE),
            owner: TopLevelOwnerId::ordinary_file(),
            local_scope: None,
            binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                TopLevelOwnerId::ordinary_file(),
            ),
        },
        name: Arc::from(name),
    })
}

fn literal(store: &SemanticGraphStore, text: &str) -> SemanticNodeId {
    store.intern_node(SemanticNodeData::Literal(LiteralValue::String(text.into())))
}

/// `{ member: <literal text> }`, the literal its only child.
fn object(store: &SemanticGraphStore, text: &str) -> SemanticNodeId {
    let value = literal(store, text);
    let member = SurfaceMember {
        key: AuthoredPropertyKey::String(Arc::from("member")),
        value,
        optional: false,
        readonly: false,
        method_kind: None,
        has_implementation_body: false,
        visibility: verter_type_expr::MemberVisibility::Public,
        spans: Default::default(),
        declaration_origin: None,
        declared_in_macro_type_arg: Default::default(),
        merge_role: Default::default(),
        excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
    };
    store.intern_node(SemanticNodeData::Object(SurfaceView::from_members(
        vec![member],
        None,
    )))
}

/// Read the delivered object, then its member through the payload's own
/// child capability.
fn member_text(read: &CacheRead<QueryResult<SemanticNodeId>>) -> String {
    let QueryResult::Value(node) = *read.value() else {
        panic!("the delivery carries its object: {read:?}");
    };
    let payload = read.node(node).unwrap().read().unwrap();
    let SemanticNodeData::Object(view) = &*payload else {
        panic!("the delivered node is the object");
    };
    let member = &view.positive_members()[0];
    let child = payload.child(member.value).unwrap().read().unwrap();
    match &*child {
        SemanticNodeData::Literal(LiteralValue::String(text)) => text.clone(),
        other => panic!("the member value is its literal: {other:?}"),
    }
}

fn harnesses() -> [(&'static str, CountedOwnershipHarness); 2] {
    [
        ("retained", CountedOwnershipHarness::new()),
        (
            "returned uncached at zero headroom",
            CountedOwnershipHarness::with_account(StoreAccount::new(
                SemanticRetentionAccount::new(RetentionLimits {
                    aggregate_ceiling_bytes: 0,
                    ..RetentionLimits::defaults()
                }),
            )),
        ),
    ]
}

/// A delivered value, the member it names and an independently cloned read
/// stay readable after the construction scope and the cache are gone, cold
/// and warm, retained or returned uncached; the final holder's drop
/// releases the whole topology.
#[test]
fn delivered_values_members_and_cloned_reads_outlive_every_producer_owner() {
    let host = host();
    for (label, harness) in harnesses() {
        let cold = harness.run(|store| {
            store.execute_cooperative(
                &host,
                key("Target"),
                || panic!("no recursion"),
                || (QueryResult::Value(object(store, label)), empty_signature()),
            )
        });
        let warm = harness.run(|store| {
            store.execute_cooperative(
                &host,
                key("Target"),
                || panic!("no recursion"),
                || (QueryResult::Value(object(store, label)), empty_signature()),
            )
        });
        let clone = cold.clone();
        harness.run(|store| {
            store.invalidate_all();
        });
        assert_eq!(
            harness.live_nodes(),
            2,
            "{label}: object and member stay live"
        );
        assert_eq!(member_text(&cold), label);
        drop(cold);
        assert_eq!(member_text(&clone), label, "{label}: a cloned read owns");
        assert_eq!(member_text(&warm), label, "{label}: a second read owns");
        drop(clone);
        assert_eq!(harness.live_nodes(), 2, "{label}: the last read still owns");
        drop(warm);
        assert_eq!(
            harness.live_nodes(),
            0,
            "{label}: the final holder releases the topology"
        );
    }
}

/// A borrowed member's child lease is an owner in its own right: it keeps
/// the member value alive after the parent payload and the delivery drop.
#[test]
fn a_member_child_lease_outlives_its_parent_read() {
    let host = host();
    let harness = CountedOwnershipHarness::new();
    let read = harness.run(|store| {
        store.execute_cooperative(
            &host,
            key("Target"),
            || panic!("no recursion"),
            || {
                (
                    QueryResult::Value(object(store, "child")),
                    empty_signature(),
                )
            },
        )
    });
    let QueryResult::Value(node) = *read.value() else {
        panic!("a value");
    };
    let payload = read.node(node).unwrap().read().unwrap();
    let SemanticNodeData::Object(view) = &*payload else {
        panic!("an object");
    };
    let child = payload
        .child(view.positive_members()[0].value)
        .unwrap()
        .to_root()
        .unwrap();
    drop(payload);
    harness.run(|store| store.invalidate_all());
    drop(read);
    assert_eq!(
        harness.live_nodes(),
        1,
        "the child root owns the member value"
    );
    let leased = child.node_ref().unwrap().read().unwrap();
    assert!(matches!(&*leased, SemanticNodeData::Literal(_)));
    drop((leased, child));
    assert_eq!(harness.live_nodes(), 0);
}

/// A delivered diagnostic's operand is owned by the read that carries the
/// diagnostic, and by every clone of that read.
#[test]
fn diagnostic_operands_stay_owned_by_the_delivering_read() {
    let host = host();
    let harness = CountedOwnershipHarness::new();
    let read = harness.run(|store| {
        store.execute_cooperative(
            &host,
            key("Target"),
            || panic!("no recursion"),
            || {
                let mut output: QueryBuildOutput<SemanticNodeId> = (
                    QueryResult::Value(literal(store, "value")),
                    empty_signature(),
                )
                    .into();
                output
                    .walker_diagnostics
                    .push(ShallowDiagnostic::OpenConditional {
                        node: literal(store, "diagnostic operand"),
                    });
                output
            },
        )
    });
    let clone = read.clone();
    harness.run(|store| store.invalidate_all());
    drop(read);
    let [ShallowDiagnostic::OpenConditional { node }] = clone.walker_diagnostics() else {
        panic!("the diagnostic is delivered: {clone:?}");
    };
    let operand = clone.node(*node).unwrap().read().unwrap();
    assert!(matches!(
        &*operand,
        SemanticNodeData::Literal(LiteralValue::String(text)) if text == "diagnostic operand"
    ));
    drop(operand);
    drop(clone);
    assert_eq!(harness.live_nodes(), 0);
}

/// A recursion carrier delivered to a re-entrant reader is owned by that
/// reader's read after the outer computation and its cache are gone.
#[test]
fn a_recursion_delivery_owns_its_carrier() {
    let host = host();
    let harness = CountedOwnershipHarness::new();
    let inner = RefCell::new(None);
    let outer = harness.run(|store| {
        store.execute_cooperative(
            &host,
            key("Target"),
            || panic!("the outer read is not re-entered"),
            || {
                let read = store.execute_cooperative(
                    &host,
                    key("Target"),
                    || literal(store, "recursion carrier"),
                    || -> (QueryResult<SemanticNodeId>, DepSignature) {
                        panic!("a re-entrant read takes the recursion carrier")
                    },
                );
                *inner.borrow_mut() = Some(read);
                (
                    QueryResult::Value(literal(store, "outer")),
                    empty_signature(),
                )
            },
        )
    });
    drop(outer);
    harness.run(|store| store.invalidate_all());
    let inner = inner.into_inner().expect("the re-entrant read ran");
    let QueryResult::Recursive(carrier) = *inner.value() else {
        panic!("a re-entrant read is a recursion carrier: {inner:?}");
    };
    let payload = inner.node(carrier).unwrap().read().unwrap();
    assert!(matches!(
        &*payload,
        SemanticNodeData::Literal(LiteralValue::String(text)) if text == "recursion carrier"
    ));
    drop((payload, inner));
    assert_eq!(harness.live_nodes(), 0);
}

/// A capability reads only in the graph that minted it, and an owner lends
/// only the ordinals it holds: a foreign graph, a non-child ordinal and an
/// unowned ordinal all fail as typed refusals, never as another node.
#[test]
fn foreign_and_unowned_capabilities_fail_safely() {
    let host = host();
    let first = CountedOwnershipHarness::new();
    let second = CountedOwnershipHarness::new();
    let read = first.run(|store| {
        store.execute_cooperative(
            &host,
            key("Target"),
            || panic!("no recursion"),
            || {
                (
                    QueryResult::Value(object(store, "first")),
                    empty_signature(),
                )
            },
        )
    });
    let QueryResult::Value(node) = *read.value() else {
        panic!("a value");
    };
    // The same ordinal names a different node in the second graph.
    let other = second.run(|store| {
        let other = object(store, "second");
        let foreign = read.node(node).unwrap();
        assert!(matches!(
            store.read(foreign),
            Err(QueryError::ForeignSemanticOperand)
        ));
        other
    });
    assert_eq!(other, node, "premise: equal ordinals in distinct graphs");
    let payload = read.node(node).unwrap().read().unwrap();
    assert!(matches!(
        payload.child(node),
        Err(QueryError::ForeignSemanticOperand)
    ));
    assert!(matches!(
        read.node(SemanticNodeId(node.0 + 1_000)),
        Err(QueryError::ForeignSemanticOperand)
    ));
}

/// A root survives no legacy canonical close: the production lifecycle may
/// still retire its slot, and a read through it then fails as stale instead
/// of answering with the retired payload.
#[test]
fn a_root_whose_slot_a_legacy_close_released_reads_as_stale() {
    let graph = SemanticGraphStore::new();
    let root = {
        let scope = graph.enter_node_root_scope();
        let node = graph.intern_node_with_scope(
            SemanticNodeData::Literal(LiteralValue::String("closed".into())),
            NodeScopeId::File {
                canonical_id: Arc::from(FILE),
                owner: TopLevelOwnerId::ordinary_file(),
                whole_hash: [0; 16],
                local_scope: None,
            },
        );
        let root = scope.node_ref(node).unwrap().to_root().unwrap();
        drop(scope);
        root
    };
    assert!(root.node_ref().unwrap().read().is_ok());
    let _ = graph.release_canonical(FILE);
    assert!(matches!(
        graph.read(root.node_ref().unwrap()),
        Err(QueryError::StaleSemanticOperand)
    ));
}

fn counted_host() -> VerterHost {
    let host = VerterHost::new_standalone_with_counted_ownership_for_test(HostConfig::default());
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(FILE.to_string()),
        input_id: FILE.to_string(),
        source: Arc::from("export interface Target { base: number; }\n"),
        file_language: crate::LanguageRegistry::global()
            .classify_static(FILE)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

/// The host's two-call projection: the first call resolves and delivers an
/// owning root, the request and every cache drop, and the second call still
/// projects through that root. A root another host's graph minted projects
/// nothing.
#[test]
fn a_resolved_root_projects_after_its_request_and_caches_are_gone() {
    let host = counted_host();
    let (outcome, _) = host
        .resolve_named_symbol_with_audit(FILE, "Target", Some(ProjectionMode::Expanded))
        .into_parts();
    let root = outcome.unwrap().expect("the interface resolves");
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    graph.invalidate_all();
    assert!(
        graph.node_is_live(root.id()),
        "the delivered root owns its node"
    );
    let bytes = host
        .project_node_to_type_expr_json_bytes(&root)
        .expect("the owned root projects");
    assert!(
        String::from_utf8(bytes).unwrap().contains("base"),
        "the projection reads the resolved interface"
    );
    let other = counted_host();
    let twin = other
        .resolve_named_symbol(FILE, "Target", Some(ProjectionMode::Expanded))
        .expect("the twin host resolves the same interface");
    assert_eq!(
        twin.id(),
        root.id(),
        "premise: the twin's equal ordinal is live"
    );
    assert!(
        other.project_node_to_type_expr_json_bytes(&root).is_none(),
        "a root minted by another graph is foreign"
    );
}

/// A one-level surface delivered by the shared resolver is an owning read of
/// its terminal object: its members and their values stay readable after
/// the dispatch and the cache that produced it are gone.
#[test]
fn a_delivered_surface_lease_owns_its_members() {
    use verter_type_engine::semantic_query::ProjectionReductionContext;
    let host = counted_host();
    let base = host
        .resolve_named_symbol(FILE, "Target", Some(ProjectionMode::Navigate))
        .expect("the interface resolves");
    let surface = {
        let d = host.semantic_dispatch();
        d.resolve_typeinfo_surface(
            base.id(),
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        )
        .expect("the interface has a one-level surface")
    };
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    graph.invalidate_all();
    drop(base);
    let member = &surface.positive_members()[0];
    assert_eq!(member.string_name(), Some("base"));
    let value = surface.lease().child(member.value).unwrap().read().unwrap();
    assert!(
        matches!(&*value, SemanticNodeData::Primitive(_)),
        "{value:?}"
    );
    assert!(graph.node_is_live(surface.node()));
}
