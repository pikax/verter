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
use verter_type_engine::semantic_query_memo::graph_fixture::GraphFixture;
use verter_type_expr::TopLevelOwnerId;

const FILE: &str = "/public/carrier.ts";

fn host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(FILE.to_string()),
            input_id: FILE.to_string(),
            source: Arc::from("export interface Target { base: number; }\nexport type Callback = (value: { arg: string }) => { returned: number };\n"),
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
            canonical_id: verter_type_expr::OwnedStr::from(FILE),
            owner: TopLevelOwnerId::ordinary_file(),
            local_scope: None,
            binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                TopLevelOwnerId::ordinary_file(),
            ),
        },
        name: verter_type_expr::OwnedStr::from(name),
    })
}

fn literal(store: &SemanticGraphStore, text: &str) -> SemanticNodeId {
    store.intern_node(SemanticNodeData::Literal(LiteralValue::String(text.into())))
}

/// `{ member: <literal text> }`, the literal its only child.
fn object(store: &SemanticGraphStore, text: &str) -> SemanticNodeId {
    let value = literal(store, text);
    let member = SurfaceMember {
        key: AuthoredPropertyKey::String(verter_type_expr::OwnedStr::from("member")),
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

fn harnesses() -> [(&'static str, GraphFixture); 2] {
    [
        ("retained", GraphFixture::new()),
        (
            "returned uncached at zero headroom",
            GraphFixture::with_account(StoreAccount::new(SemanticRetentionAccount::new(
                RetentionLimits {
                    aggregate_ceiling_bytes: 0,
                    ..RetentionLimits::defaults()
                },
            ))),
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
    let harness = GraphFixture::new();
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
    let harness = GraphFixture::new();
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
    let harness = GraphFixture::new();
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
    let first = GraphFixture::new();
    let second = GraphFixture::new();
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

/// A root survives its document's close: the close evicts only cache
/// owners, so a read through the root still answers its payload, and the
/// node is destroyed with the root's last clone. A destroyed ordinal can
/// then never be rooted again.
///
/// Discriminating: a canonical sweep at close turns the read into
/// `StaleSemanticOperand` while the root is held; a close that leaked the
/// node keeps it live after the root drops.
#[test]
fn a_root_survives_its_documents_close_and_drains_with_its_last_clone() {
    let graph = SemanticGraphStore::new();
    let root = {
        let scope = graph.enter_node_root_scope();
        let node = graph.intern_node_with_scope(
            SemanticNodeData::Literal(LiteralValue::String("closed".into())),
            NodeScopeId::File {
                canonical_id: verter_type_expr::OwnedStr::from(FILE),
                owner: TopLevelOwnerId::ordinary_file(),
                whole_hash: [0; 16],
                local_scope: None,
            },
        );
        let root = scope.node_ref(node).unwrap().to_root().unwrap();
        drop(scope);
        root
    };
    let clone = root.clone();
    let _ = graph.close_canonical(FILE);
    let payload = graph.read(root.node_ref().unwrap()).unwrap();
    assert!(matches!(
        &*payload,
        SemanticNodeData::Literal(LiteralValue::String(text)) if text == "closed"
    ));
    drop(payload);
    let id = root.id();
    drop(root);
    assert!(graph.node_is_live(id), "the clone still owns the node");
    drop(clone);
    assert!(!graph.node_is_live(id), "the last clone's drop destroys it");
    assert!(matches!(
        graph.root_nodes([id]),
        Err(QueryError::StaleSemanticOperand)
    ));
}

fn counted_host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(FILE.to_string()),
        input_id: FILE.to_string(),
        source: Arc::from("export interface Target { base: number; }\nexport type Callback = (value: { arg: string }) => { returned: number };\n"),
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

/// A reduced output node owns the node it reduced to: its capability reads
/// after the reducing dispatch and the caches are gone.
#[test]
fn a_reduced_output_owns_its_node() {
    use verter_type_engine::semantic_query::ProjectionReductionContext;
    let host = counted_host();
    let base = host
        .resolve_named_symbol(FILE, "Target", Some(ProjectionMode::Navigate))
        .expect("the interface resolves");
    let reduced = {
        let d = host.semantic_dispatch();
        d.reduce_output_node_with_context(
            base.id(),
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
    };
    drop(base);
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    graph.invalidate_all();
    let payload = graph.read(reduced.node().unwrap()).unwrap();
    assert!(
        matches!(&*payload, SemanticNodeData::Object(_)),
        "{payload:?}"
    );
    assert!(graph.node_is_live(reduced.node_id()));
}

/// A deferred conditional delivered through a public read owns the operands
/// its pending substitution will apply later: they stay readable through
/// the payload's child capabilities after the construction scope and the
/// cache are gone, and the final read's drop releases them.
#[test]
fn a_deferred_substitutions_pending_operands_stay_owned_by_the_delivering_read() {
    use verter_type_engine::semantic_query::ConditionalPendingSubstitution;
    let host = host();
    let harness = GraphFixture::new();
    let pending_argument = RefCell::new(None);
    let read = harness.run(|store| {
        store.execute_cooperative(
            &host,
            key("Target"),
            || panic!("no recursion"),
            || {
                let param = literal(store, "parameter");
                let argument = literal(store, "pending argument");
                *pending_argument.borrow_mut() = Some(argument);
                let conditional = store.intern_node(SemanticNodeData::Conditional {
                    check: param,
                    extends: literal(store, "extends"),
                    true_branch_ref: literal(store, "true branch"),
                    false_branch_ref: literal(store, "false branch"),
                    distributive: false,
                    pending: Some(verter_session_query::retention::owned::Owned::new(
                        ConditionalPendingSubstitution::empty().append_true(param, argument),
                    )),
                });
                (QueryResult::Value(conditional), empty_signature())
            },
        )
    });
    let argument = pending_argument.into_inner().expect("the build ran");
    let clone = read.clone();
    harness.run(|store| store.invalidate_all());
    drop(read);
    let QueryResult::Value(node) = *clone.value() else {
        panic!("the deferred conditional is delivered: {clone:?}");
    };
    let payload = clone.node(node).unwrap().read().unwrap();
    let operand = payload.child(argument).unwrap().read().unwrap();
    assert!(matches!(
        &*operand,
        SemanticNodeData::Literal(LiteralValue::String(text)) if text == "pending argument"
    ));
    drop((operand, payload));
    assert_eq!(harness.live_nodes(), 6, "the read owns the deferred shell");
    drop(clone);
    assert_eq!(harness.live_nodes(), 0, "the final read releases it");
}

/// A dispatcher's construction lane owns no label it hands out: a
/// construction ordinal minted with no owner open on the thread is released
/// at once, while one minted inside an explicit builder owner the caller
/// holds stays owned by that builder through every dispatcher's drop, in
/// either order, and is released when the builder closes.
#[test]
fn construction_labels_are_owned_by_the_callers_builder_never_by_a_dispatcher() {
    let host = counted_host();
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let first = host.semantic_dispatch();
    let unowned = first.opaque(QueryError::Other(verter_type_expr::OwnedStr::from(
        "no owner open",
    )));
    assert!(
        !graph.node_is_live(unowned),
        "the dispatcher's lane keeps nothing a caller does not own"
    );
    let builder = first.enter_construction_scope();
    let second = host.semantic_dispatch();
    let owned = second.opaque(QueryError::Other(verter_type_expr::OwnedStr::from(
        "builder operand",
    )));
    drop(first);
    assert!(
        graph.node_is_live(owned),
        "a dispatcher's drop leaves it owned"
    );
    drop(second);
    assert!(
        graph.node_is_live(owned),
        "the builder owns its operand past every dispatcher"
    );
    let payload = graph
        .read_in_scope(owned)
        .expect("the builder's lane reads it");
    assert!(
        matches!(&*payload, SemanticNodeData::Opaque(_)),
        "{payload:?}"
    );
    drop(payload);
    drop(builder);
    assert!(
        !graph.node_is_live(owned),
        "closing the builder releases what it owned"
    );
}

/// Completed evaluation receipts belong to the operation, even when its
/// caller keeps a builder open. Closing that builder must release the last
/// operand while the dispatcher remains usable.
#[test]
fn completed_receipts_release_when_the_callers_builder_closes() {
    let host = host();
    let harness = zero_headroom_harness();
    let d =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::with_graph_fixture(
            &host, harness,
        );
    let graph = Arc::clone(d.graph());
    let baseline = graph.node_ownership_counts().permanent_nodes;
    let builder = d.enter_construction_scope();
    let operand = d.opaque(QueryError::Other(verter_type_expr::OwnedStr::from(
        "builder receipt",
    )));
    assert_eq!(
        d.evaluate_deferred_semantic_node_for_tests(operand),
        operand
    );
    assert!(graph.node_is_live(operand));
    drop(builder);
    graph.invalidate_all();
    assert_eq!(
        graph.node_count(),
        baseline,
        "no receipt outlives the closed builder"
    );
    let next = d.owning().lower_type_expr_in_scope_with_mode(
        FILE,
        &verter_type_expr::TypeExpr::Literal(verter_type_expr::LiteralValue::String("next".into())),
        ProjectionMode::Navigate,
    );
    assert!(next.value().is_some(), "the dispatcher remains usable");
    drop(next);
    assert_eq!(graph.node_count(), baseline);
}

fn zero_headroom_harness() -> GraphFixture {
    GraphFixture::with_account(StoreAccount::new(SemanticRetentionAccount::new(
        RetentionLimits {
            aggregate_ceiling_bytes: 0,
            ..RetentionLimits::defaults()
        },
    )))
}

/// Public construction adapters own both new nodes and operands reused
/// without interning. A builder may close before the result or its clone.
#[test]
fn construction_carriers_own_new_and_reused_results_past_every_builder() {
    let host = host();
    let d =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::with_graph_fixture(
            &host,
            zero_headroom_harness(),
        );
    let graph = Arc::clone(d.graph());
    let baseline = graph.node_ownership_counts().permanent_nodes;
    let context = verter_type_engine::semantic_query::ProjectionReductionContext::published(
        ProjectionMode::Navigate,
    );
    for text in ["first construction", "second construction"] {
        let result = d
            .owning()
            .opaque(QueryError::Other(verter_type_expr::OwnedStr::from(text)));
        assert!(result.node(*result.value()).unwrap().read().is_ok());
        drop(result);
        assert_eq!(
            graph.node_count(),
            baseline,
            "discarded construction releases while the dispatcher lives"
        );
    }
    // A projected child does not retain its unused parent through the
    // construction operation's nested query delivery.
    let builder = d.enter_construction_scope();
    let parent = object(&graph, "kept");
    let result = d
        .owning()
        .indexed_member_value(FILE, parent, &Arc::from("member"))
        .map(|value| value.expect("the member projects"));
    drop(builder);
    graph.invalidate_all();
    assert!(matches!(
        &*result.node(*result.value()).unwrap().read().unwrap(),
        SemanticNodeData::Literal(_)
    ));
    let counts = graph.node_ownership_counts();
    assert_eq!(
        counts.live_nodes - counts.permanent_nodes,
        1,
        "only the represented literal remains while its result and dispatcher live"
    );
    drop(result);
    assert_eq!(graph.node_count(), baseline);
    let builder = d.enter_construction_scope();
    let source = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "reused".into(),
    )));
    let reused = d.owning().resolve_carrier_subject_node(source, context);
    assert_eq!(*reused.value(), source);
    let clone = reused.clone();
    drop(builder);
    drop(reused);
    assert!(matches!(
        &*clone.node(source).unwrap().read().unwrap(),
        SemanticNodeData::Literal(_)
    ));
    drop(d);
    graph.invalidate_all();
    assert!(clone.node(source).unwrap().read().is_ok());
    drop(clone);
    assert_eq!(graph.node_count(), baseline);
}

/// A carrier handed to an explicit builder keeps its construction exactly as
/// long as that builder is open: the label it yields names a live node until
/// the builder closes, and nothing after.
#[test]
fn a_construction_handed_to_a_builder_lives_until_the_builder_closes() {
    let host = host();
    let d =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::with_graph_fixture(
            &host,
            zero_headroom_harness(),
        );
    let graph = Arc::clone(d.graph());
    let baseline = graph.node_ownership_counts().permanent_nodes;
    // Built with no owner open, so only the carrier holds it.
    let carrier = d
        .owning()
        .opaque(QueryError::Other(verter_type_expr::OwnedStr::from(
            "handed to a builder",
        )));
    let builder = d.enter_construction_scope();
    let label = carrier.into_value_in(&builder);
    graph.invalidate_all();
    assert!(
        graph.node_is_live(label),
        "the builder keeps the construction it was handed"
    );
    drop(builder);
    graph.invalidate_all();
    assert_eq!(
        graph.node_count(),
        baseline,
        "closing the builder releases it while the dispatcher lives"
    );
}

#[cfg(not(feature = "semantic-observe"))]
const DELIVERY_FILE: &str = "/public/deliveries.ts";

#[cfg(not(feature = "semantic-observe"))]
fn delivery_decl_key(name: &str) -> ResolveDeclKey {
    ResolveDeclKey {
        scope: ScopeId {
            canonical_id: verter_type_expr::OwnedStr::from(DELIVERY_FILE),
            owner: TopLevelOwnerId::ordinary_file(),
            local_scope: None,
            binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                TopLevelOwnerId::ordinary_file(),
            ),
        },
        name: verter_type_expr::OwnedStr::from(name),
    }
}

#[cfg(not(feature = "semantic-observe"))]
/// The expanded body of the fixture's declaration `name`.
fn delivery_body_key<C: verter_type_engine::resolver_core::ResolverCapabilities>(
    d: &verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<'_, C>,
    name: &str,
) -> SemanticQueryKey {
    let scope: Arc<str> = Arc::from(DELIVERY_FILE);
    SemanticQueryKey::Instantiate(verter_type_engine::semantic_query::InstantiateKey::new(
        d.type_slot_for(
            Arc::clone(&scope),
            TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
        ),
        Arc::from([]),
        d.instantiate_context_for(
            &scope,
            verter_type_engine::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Expanded,
            ),
        ),
    ))
}

/// What a delivered node reads as through the capability its owner lends:
/// an object's member names, or the declaration a placeholder names.
fn delivered_shape(
    node: verter_type_engine::semantic_query_memo::node_roots::NodeRef<'_>,
) -> String {
    let payload = node.read().expect("a retained delivery reads");
    match &*payload {
        SemanticNodeData::Object(view) => view
            .positive_members()
            .iter()
            .map(|member| member.string_name().unwrap_or_default().to_string())
            .collect::<Vec<_>>()
            .join(","),
        SemanticNodeData::Opaque(QueryError::DeclPlaceholder { name, .. }) => {
            format!("declaration {name}")
        }
        other => panic!("an object or a declaration placeholder: {other:?}"),
    }
}

/// A live dispatcher keeps no result it delivers. With zero retention
/// headroom every result is returned uncached, so the delivery is the only
/// owner: a result the caller discards releases its nodes at the end of its
/// operation while the dispatcher stays alive — and so does everything the
/// operation kept for replay or paid for, once the graph's own caches let go
/// of what they hold — while a result the caller keeps, through every public
/// query delivery, and every clone of it, outlives the dispatcher and drains
/// when its final holder drops.
///
/// A build with `semantic-observe` also records relation explanations, whose
/// leases root their operands for the dispatcher's lifetime by design; the
/// "keeps nothing while the dispatcher lives" premise holds only without
/// them, so this runs in the default build.
#[cfg(not(feature = "semantic-observe"))]
#[test]
fn a_live_dispatchers_discarded_results_release_while_retained_ones_outlive_it() {
    use verter_type_engine::semantic_query::{SemanticQueryApi, SemanticQueryValue};
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(DELIVERY_FILE.to_string()),
        input_id: DELIVERY_FILE.to_string(),
        source: Arc::from(
            "type Cond<T> = T extends string ? { derived: T } : { other: T };\n\
             export type Derived = Cond<\"x\">;\n\
             export interface First { first: string; }\n\
             export interface Second { second: number; }\n\
             export interface Kept { kept: string; }\n\
             export interface Valued { valued: number; }\n\
             export interface Declared { declared: string; }\n\
             export interface Read { read: boolean; }\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static(DELIVERY_FILE)
            .static_resolution(),
        aliases: Vec::new(),
    });
    let harness = GraphFixture::with_account(StoreAccount::new(SemanticRetentionAccount::new(
        RetentionLimits {
            aggregate_ceiling_bytes: 0,
            ..RetentionLimits::defaults()
        },
    )));
    let d =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::with_graph_fixture(
            &host, harness,
        );
    let graph = Arc::clone(d.graph());
    let baseline = graph.node_ownership_counts().permanent_nodes;
    for name in ["First", "Second"] {
        let QueryResult::Value(discarded) = d.execute_type_node(delivery_body_key(&d, name)) else {
            panic!("{name} resolves");
        };
        assert_eq!(
            delivered_shape(discarded.node(discarded.value).unwrap()),
            name.to_lowercase(),
        );
        assert!(
            graph.node_count() > baseline,
            "{name}: the delivery owns it"
        );
        drop(discarded);
        assert_eq!(
            graph.node_count(),
            baseline,
            "{name}: a discarded result releases its nodes while the dispatcher lives"
        );
    }
    // A conditional's evaluation reads nested request-local completions the
    // dispatcher keeps for replay and pays receipts that root their
    // operands; the graph's own hash-cons caches may also keep what it
    // built. Once those caches let go, nothing the dispatcher held for the
    // discarded result remains.
    let QueryResult::Value(discarded) = d.execute_type_node(delivery_body_key(&d, "Derived"))
    else {
        panic!("Derived resolves");
    };
    assert_eq!(
        delivered_shape(discarded.node(discarded.value).unwrap()),
        "derived"
    );
    drop(discarded);
    let cached = graph.hash_cons_retained_nodes_for_tests();
    assert!(cached.iter().all(|node| graph.node_is_live(*node)));
    assert_eq!(
        graph.node_count(),
        baseline + cached.len(),
        "only the hash-cons caches retain nodes after the evaluation ends"
    );
    graph.invalidate_all();
    assert_eq!(
        graph.node_count(),
        baseline,
        "the live dispatcher keeps nothing for a discarded evaluation"
    );
    let QueryResult::Value(kept) = d.execute_type_node(delivery_body_key(&d, "Kept")) else {
        panic!("Kept resolves");
    };
    let clone = kept.clone();
    let QueryResult::Value(valued) = d.execute(delivery_body_key(&d, "Valued")) else {
        panic!("Valued resolves");
    };
    let SemanticQueryValue::TypeNode(valued_node) = valued.value else {
        panic!("a declaration resolves to a type node");
    };
    let QueryResult::Value(declared) = d.resolve_decl(delivery_decl_key("Declared")) else {
        panic!("Declared resolves");
    };
    let read = d.execute_read(delivery_body_key(&d, "Read"));
    let QueryResult::Value(read_node) = *read.value() else {
        panic!("Read resolves: {read:?}");
    };
    drop(d);
    graph.invalidate_all();
    assert_eq!(delivered_shape(kept.node(kept.value).unwrap()), "kept");
    drop(kept);
    assert_eq!(
        delivered_shape(clone.node(clone.value).unwrap()),
        "kept",
        "a clone owns the result after the original and the dispatcher drop"
    );
    assert_eq!(delivered_shape(valued.node(valued_node).unwrap()), "valued");
    assert_eq!(
        delivered_shape(declared.node(declared.value).unwrap()),
        "declaration Declared"
    );
    assert_eq!(delivered_shape(read.node(read_node).unwrap()), "read");
    drop((clone, valued, declared));
    assert!(
        graph.node_count() > baseline,
        "the last retained read still owns its result"
    );
    drop(read);
    assert_eq!(
        graph.node_count(),
        baseline,
        "the final holders drain every retained result"
    );
}

/// Hot-handle adapters return owners even with no ambient builder. Both
/// lowering a declaration and resolving the returned hot handle survive
/// every producing owner and drain independently of the live dispatcher.
#[test]
fn public_hot_handle_adapters_deliver_owners_without_an_ambient_builder() {
    let host = host();
    let d =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::with_graph_fixture(
            &host,
            zero_headroom_harness(),
        );
    let graph = Arc::clone(d.graph());
    let baseline = graph.node_ownership_counts().permanent_nodes;
    let context = verter_type_engine::semantic_query::ProjectionReductionContext::published(
        ProjectionMode::Expanded,
    );
    let declared = d.owning().decl_body_hot_ref(
        FILE,
        TopLevelOwnerId::ordinary_file(),
        "Target",
        Arc::from([]),
        context,
    );
    let hot = (*declared.value()).expect("the declaration has a body");
    assert_eq!(delivered_shape(declared.node(hot.node()).unwrap()), "base");
    let resolved = d.owning().resolve_hot_handle_with_context(hot, context);
    drop(declared);
    assert_eq!(
        delivered_shape(resolved.node(*resolved.value()).unwrap()),
        "base"
    );
    let clone = resolved.clone();
    drop(d);
    graph.invalidate_all();
    drop(resolved);
    assert_eq!(delivered_shape(clone.node(*clone.value()).unwrap()), "base");
    drop(clone);
    assert_eq!(graph.node_count(), baseline);
}

/// A one-level surface the shared projection hands out is an owning
/// adapter: its members stay readable through it after the dispatch, the
/// caches and the resolved base are gone.
#[test]
fn a_projected_one_level_surface_owns_its_members() {
    use verter_type_engine::project_semantic_dispatch::one_level_surface::project_one_level_surface;
    use verter_type_engine::semantic_query::{ProjectionReductionContext, SurfaceEntry};
    let host = counted_host();
    let base = host
        .resolve_named_symbol(FILE, "Target", Some(ProjectionMode::Navigate))
        .expect("the interface resolves");
    let surface = {
        let d = host.semantic_dispatch();
        project_one_level_surface(
            &host,
            &d,
            base.id(),
            verter_type_expr::OwnedSlice::from([]),
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            None,
        )
        .resolved_for_tests()
        .expect("the interface has a one-level surface")
    };
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    graph.invalidate_all();
    drop(base);
    let [SurfaceEntry::Member(member)] = surface.entries() else {
        panic!("one member: {surface:?}");
    };
    assert!(
        graph.node_is_live(member.value),
        "the surface owns its member"
    );
    let value = surface.node(member.value).unwrap().read().unwrap();
    assert!(
        matches!(&*value, SemanticNodeData::Primitive(_)),
        "{value:?}"
    );
}

/// A resolved node the delivery cannot root is a typed fault, never an
/// absent result indistinguishable from "not found".
#[test]
fn a_resolved_node_that_cannot_be_rooted_is_a_typed_fault() {
    let host = counted_host();
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let unowned =
        SemanticNodeId(verter_type_engine::semantic_query_memo::UNALLOCATABLE_ID_FLOOR + 1);
    assert!(
        crate::typeinfo::resolve_named_symbol::deliver_resolved_root(&graph, unowned).is_err(),
        "outside a construction lane nothing roots"
    );
    let _lane = host.project_type_store().construction_grant().enter_scope();
    assert!(
        crate::typeinfo::resolve_named_symbol::deliver_resolved_root(&graph, unowned).is_err(),
        "an ordinal the graph never allocated does not root"
    );
}

/// A delivered flow-return result owns its return node for as long as the
/// caller holds it: after its request ends, every cache is invalidated and
/// the declaring document is closed, the node is still live; the result's
/// last drop destroys it.
///
/// Discriminating: a result carrying only the raw return ordinal leaves the
/// node owned by caches alone, so the invalidation destroys it under the
/// caller.
#[test]
fn a_delivered_flow_return_result_owns_its_node_past_its_request_and_close() {
    const FLOW: &str = "/public/flow.ts";
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        FLOW,
        "export function pf() { return { owned: [1, 'two'] as const }; }\n",
        crate::FileLanguage::script_ts(),
    );
    let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: verter_type_expr::OwnedStr::from(FLOW),
            owner: TopLevelOwnerId::ordinary_file(),
            symbol: verter_type_expr::OwnedStr::from("pf"),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let result = host
        .get_flow_return_type_with_audit(
            &identity,
            verter_type_engine::semantic_query::ReturnProjectionDemand::whole_return(),
        )
        .into_result()
        .expect("the flow return answers");
    let node = result.return_type();
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    graph.invalidate_all();
    host.evict(FLOW);
    assert!(
        graph.node_is_live(node),
        "the delivered result owns its node after every cache let go"
    );
    drop(result);
    assert!(
        !graph.node_is_live(node),
        "the result's last drop destroys its node"
    );
}

/// A signature adapter and its parameter demands own their roots even when
/// no builder encloses the callable view.
#[test]
fn public_callable_adapters_own_their_parameters_past_dispatcher_teardown() {
    use verter_type_engine::project_semantic_dispatch::callable_view::CallableNodeView;
    let host = host();
    let d =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::with_graph_fixture(
            &host,
            zero_headroom_harness(),
        );
    let graph = Arc::clone(d.graph());
    let baseline = graph.node_ownership_counts().permanent_nodes;
    let context = verter_type_engine::semantic_query::ProjectionReductionContext::published(
        ProjectionMode::Expanded,
    );
    let declared = d.owning().decl_body_hot_ref(
        FILE,
        TopLevelOwnerId::ordinary_file(),
        "Callback",
        Arc::from([]),
        context,
    );
    let callable = CallableNodeView::new(&d, (*declared.value()).expect("callback body").node());
    let signature = callable.owning().signature(context).expect("a signature");
    drop(declared);
    let parameters = signature.raw_params();
    let parameter = parameters.value()[0].ty;
    let first = signature.first_param();
    assert_eq!(*first.value(), Some(parameter));
    drop(signature);
    drop(d);
    graph.invalidate_all();
    assert_eq!(delivered_shape(parameters.node(parameter).unwrap()), "arg");
    drop(parameters);
    assert_eq!(delivered_shape(first.node(parameter).unwrap()), "arg");
    drop(first);
    assert_eq!(graph.node_count(), baseline);
}
