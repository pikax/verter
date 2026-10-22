use std::sync::Arc;
use std::time::Duration;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::semantic_query::{
    BinderScopeId, FactStatus, PartialReasonSet, ProjectionMode, ResolveDeclKey, ScopeId,
    SemanticNodeData, SemanticQueryKey,
};

const OWNER: &str = "/src/component.vue";
const NAMED: &str = "/src/named.ts";
const HANG_BOUND: Duration = Duration::from_secs(10);
#[path = "support/reconfiguring_workspace.rs"]
mod reconfiguring_workspace;

fn upsert(host: &VerterHost, path: &str, source: &str, file_language: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.into()),
            input_id: path.into(),
            source: Arc::from(source),
            file_language,
            aliases: Vec::new(),
        })
        .expect("upsert");
}

fn component_host(budget: usize, source: &str) -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig {
        projection_op_budget: budget,
        ..HostConfig::default()
    });
    upsert(&host, OWNER, source, FileLanguage::vue());
    host
}

fn non_admitted_entries_are_real_host_work() {
    let workspace = Arc::new(reconfiguring_workspace::ReconfiguringWorkspace::default());
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace.clone()));
    upsert(
        &host,
        OWNER,
        "<script setup lang=\"ts\">import type { Missing } from './missing';</script>",
        FileLanguage::vue(),
    );
    workspace.bind(&host);
    let foreign = component_host(
        0,
        "<script setup lang=\"ts\">defineProps<{ x: string }>()</script>",
    );
    let graph = host.project_type_store().semantic_graph();
    let before = host.work_snapshot();
    let graph_before = graph.work_snapshot();
    assert_eq!(graph.memo_entry_count(), 0);
    assert!(foreign
        .get_component_meta(OWNER)
        .expect("foreign answer")
        .is_complete());
    assert_eq!(
        host.work_snapshot(),
        before,
        "foreign work cannot satisfy this host's delta"
    );
    assert!(foreign.work_snapshot().resolver_node_cache_misses > 0);
    for expected in 1..=2 {
        let prior = host.work_snapshot();
        let epoch = host.store_view_epoch();
        let answer = host
            .resolve_component_meta(OWNER, ProjectionMode::Expanded)
            .expect("return-only recovery");
        assert_eq!(answer.fact_status(), FactStatus::Complete);
        assert!(!answer.synthesis_should_suppress);
        assert!(answer.resolved_macros.is_empty());
        assert!(
            host.store_view_epoch() > epoch,
            "the adapter really superseded the request"
        );
        let after = host.work_snapshot();
        assert!(
            after.component_meta_resolved_state_recomputes
                > prior.component_meta_resolved_state_recomputes,
            "non-admitted producer entries must be observed without lower graph work"
        );
        assert!(
            after.component_meta_resolved_state_recomputes
                - prior.component_meta_resolved_state_recomputes
                > after.resolver_node_cache_misses - prior.resolver_node_cache_misses,
            "producer attempts and request-result misses have different meanings"
        );
        assert_eq!(
            after.resolver_node_cache_misses - before.resolver_node_cache_misses,
            expected
        );
        assert_eq!(
            graph.work_snapshot(),
            graph_before,
            "this producer makes no lower graph queries"
        );
        assert_eq!(
            graph.memo_entry_count(),
            0,
            "no lower memo hit can exist either"
        );
    }
}

fn complete_and_warm_graph_work() {
    let host = component_host(
        0,
        "<script setup lang=\"ts\">defineProps<{ x: string }>()</script>",
    );
    let graph = host.project_type_store().semantic_graph();
    let first = host.get_component_meta(OWNER).expect("cold answer");
    assert!(first.is_complete(), "{:?}", first.status());
    assert_eq!(
        first
            .props
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["x"]
    );
    let cold = host.work_snapshot();
    assert!(cold.resolver_node_cache_misses > 0);
    assert!(cold.component_meta_resolved_state_recomputes > 0);
    let warm = host.get_component_meta(OWNER).expect("warm answer");
    assert_eq!(warm.status(), first.status());
    assert_eq!(warm.props.len(), first.props.len());
    assert_eq!(
        host.work_snapshot(),
        cold,
        "a retained answer performs no host work"
    );
    // A different resolver mode enters its own producer with the same graph warm.
    let graph_before = graph.work_snapshot();
    let identity = host
        .resolve_component_meta(OWNER, ProjectionMode::Identity)
        .expect("identity answer");
    assert!(!identity.synthesis_should_suppress);
    assert!(
        host.work_snapshot()
            .component_meta_resolved_state_recomputes
            > cold.component_meta_resolved_state_recomputes
    );
    assert_eq!(
        graph.work_snapshot().misses,
        graph_before.misses,
        "graph misses cannot stand in for host recomputation"
    );
    upsert(
        &host,
        OWNER,
        "<script setup lang=\"ts\">defineProps<{ changed: number }>()</script>",
        FileLanguage::vue(),
    );
    let changed = host.get_component_meta(OWNER).expect("edited answer");
    assert!(changed.is_complete());
    assert_eq!(
        changed
            .props
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["changed"]
    );
}

fn retained_refusals() {
    let mut deep = String::from("<script setup lang=\"ts\">type P0 = { x: string };\n");
    for level in 1..=30 {
        deep.push_str(&format!("type P{level} = Partial<P{}>;\n", level - 1));
    }
    deep.push_str("defineProps<P30>()</script>");
    for (budget, source, reason) in [
        (
            HostConfig::default().projection_op_budget,
            deep.as_str(),
            PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT,
        ),
        (
            1,
            "<script setup lang=\"ts\">defineProps<{ [K in 'x' | 'y']: string }>()</script>",
            PartialReasonSet::PROJECTION_WORK_LIMIT,
        ),
    ] {
        let host = component_host(budget, source);
        let first = host.get_component_meta(OWNER).expect("refusal recovery");
        let FactStatus::Unavailable(reasons) = first.status() else {
            panic!("expected typed refusal: {:?}", first.status());
        };
        assert!(reasons.get().contains(reason), "{:?}", first.status());
        assert!(!reasons.get().contains(PartialReasonSet::MISSING_DEPENDENCY));
        let cold = host.work_snapshot();
        assert!(cold.resolver_node_cache_misses > 0);
        assert!(cold.component_meta_resolved_state_recomputes > 0);
        let again = host.get_component_meta(OWNER).expect("retained refusal");
        let (_, resolution) = host
            .get_component_meta_with_resolution(OWNER)
            .expect("resolution recovery");
        assert_eq!(again.status(), first.status());
        assert_eq!(resolution.fact_status(), first.status());
        assert!(resolution.synthesis_should_suppress);
        assert_eq!(
            host.work_snapshot(),
            cold,
            "valid refusal replay runs no relevant host work"
        );
    }
}

fn named_host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        NAMED,
        "export interface Box { label: string }",
        FileLanguage::script_ts(),
    );
    host
}

fn key() -> SemanticQueryKey {
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: NAMED.into(),
            owner,
            local_scope: None,
            binder_scope_id: BinderScopeId::file_scope(owner),
        },
        name: "Box".into(),
    })
}

fn request(host: &VerterHost) -> verter_type_engine::semantic_query::SemanticNodeId {
    let answer = host
        .resolve_named_symbol(NAMED, "Box", None)
        .expect("named answer");
    let data = answer
        .node_ref()
        .expect("answer capability")
        .read()
        .expect("live answer");
    let SemanticNodeData::Object(surface) = data.as_ref() else {
        panic!("{data:?}");
    };
    let [member] = surface.closed().complete_members() else {
        panic!("expected one member: {data:?}");
    };
    assert!(
        matches!(&member.key, verter_type_expr::AuthoredPropertyKey::String(name) if name.as_str() == "label")
    );
    assert!(!member.optional);
    let value = data
        .child(member.value)
        .expect("retained member")
        .read()
        .expect("member value");
    assert!(
        matches!(
            &*value,
            SemanticNodeData::Primitive(verter_type_engine::semantic_query::PrimitiveKind::String)
        ),
        "{value:?}"
    );
    answer.id()
}

fn joins_require_a_real_subscriber() {
    for overlap in [true, false] {
        let host = named_host();
        let foreign = named_host();
        let graph = host.project_type_store().semantic_graph();
        let before = graph.work_snapshot();
        request(&foreign);
        assert_eq!(
            graph.work_snapshot(),
            before,
            "foreign graph work is isolated"
        );
        let (observed, answers, flight) = std::thread::scope(|threads| {
            let held = graph.arm_flight_rendezvous(key());
            let producer = threads.spawn(|| request(&host));
            held.wait_producer_parked(HANG_BOUND)
                .expect("producer parked");
            let flight = held.held_flight_weak().expect("held flight");
            let (go, start) = std::sync::mpsc::channel();
            let host = &host;
            let subscriber = threads.spawn(move || {
                start.recv().expect("start subscriber");
                request(host)
            });
            let answers = if overlap {
                go.send(()).unwrap();
                held.wait_subscriber_registered(HANG_BOUND)
                    .expect("real subscriber registered");
                held.release();
                [producer.join().unwrap(), subscriber.join().unwrap()]
            } else {
                held.release();
                let first = producer.join().unwrap();
                go.send(()).unwrap();
                [first, subscriber.join().unwrap()]
            };
            (held.observe(), answers, flight)
        });
        assert_eq!(answers[0], answers[1]);
        assert!(graph.flight_rendezvous_is_disarmed());
        assert!(flight.upgrade().is_none(), "completed flight drains");
        let after = graph.work_snapshot();
        assert!(after.misses > before.misses, "cold memo claim misses");
        let joined = after.joined_waits - before.joined_waits;
        let oracle = observed.producer_claims == 1 && observed.subscribers > 0 && joined > 0;
        assert_eq!(
            oracle, overlap,
            "serialized callers must fail the overlap oracle"
        );
        if !overlap {
            assert_eq!(joined, 0);
        }
        assert_eq!(request(&host), answers[0]);
        assert_eq!(
            graph.work_snapshot(),
            after,
            "valid warm replay adds no misses or joins"
        );
    }
}

fn cancellation_and_unwind_drain() {
    let host = named_host();
    let graph = host.project_type_store().semantic_graph();
    let (answers, flight) = std::thread::scope(|threads| {
        let held = graph.arm_flight_rendezvous(key());
        let producer = threads.spawn(|| host.resolve_named_symbol(NAMED, "Box", None));
        held.wait_producer_parked(HANG_BOUND)
            .expect("parked producer");
        let subscriber = threads.spawn(|| host.resolve_named_symbol(NAMED, "Box", None));
        held.wait_subscriber_registered(HANG_BOUND)
            .expect("registered subscriber");
        let flight = held.held_flight_weak().expect("held flight");
        assert_eq!(held.cancel_participants(), 2);
        (
            [producer.join().unwrap(), subscriber.join().unwrap()],
            flight,
        )
    });
    assert!(
        answers.iter().all(Option::is_none),
        "cancelled requests do not publish answers"
    );
    assert!(flight.upgrade().is_none());
    assert!(graph.flight_rendezvous_is_disarmed());
    request(&host);

    let host = named_host();
    let graph = host.project_type_store().semantic_graph();
    let flight = std::sync::Mutex::new(None);
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        std::thread::scope(|threads| {
            let held = graph.arm_flight_rendezvous(key());
            threads.spawn(|| request(&host));
            held.wait_producer_parked(HANG_BOUND)
                .expect("parked producer");
            *flight.lock().unwrap() = held.held_flight_weak();
            panic!("planted assertion failure while held");
        });
    }));
    assert!(unwind.is_err());
    assert!(flight
        .into_inner()
        .unwrap()
        .expect("held flight")
        .upgrade()
        .is_none());
    assert!(graph.flight_rendezvous_is_disarmed());
    request(&host);
}

fn run() {
    let host = VerterHost::new_standalone(HostConfig::default());
    assert_eq!(host.work_snapshot().resolver_node_cache_misses, 0);
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .work_snapshot()
            .misses,
        0
    );
    // Capture stays off in this profile; the capture-available agreement
    // proof lives in the observation contract crate's fixture, which builds
    // the full readers through its own dependency edge.
    assert_eq!(
        verter_session::capture::availability(),
        verter_audit::CaptureAvailability::Unavailable
    );
    assert_eq!(
        std::mem::size_of::<verter_audit::observe::ObservedU64>(),
        0,
        "unrelated optional storage stays absent"
    );
    assert_eq!(
        std::mem::size_of::<verter_audit::observe::WorkObservedU64>(),
        8
    );
    non_admitted_entries_are_real_host_work();
    complete_and_warm_graph_work();
    retained_refusals();
    joins_require_a_real_subscriber();
    cancellation_and_unwind_drain();
}

fn main() {
    // The depth-refusal fixture must reach the semantic bound, independently
    // of the platform's default executable stack reservation.
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}
