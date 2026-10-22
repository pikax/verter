//! The connected-demand ledger's construction-byte rail: reservations are
//! charged before a construction and never refunded, the one past the
//! allowance is refused with the sticky memory reason, and a new connected
//! demand starts from zero.

use super::ProjectSemanticDispatch;
use crate::{HostConfig, VerterHost};
use verter_type_engine::semantic_query::PartialReasonSet;

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

/// Reservations within the allowance are granted and accumulate; the one
/// that would pass it is refused with `CONNECTED_MEMORY_LIMIT`, and the trip
/// is sticky: every later charge on the demand, work or bytes, is refused
/// with it.
#[test]
fn a_reservation_past_the_allowance_trips_the_memory_rail() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    dispatch.connected_demand().set_byte_limit_for_tests(1_000);
    let (_guard, trip) = dispatch.enter_connected_demand(false);
    assert!(trip.is_none(), "a fresh demand has not tripped");
    dispatch
        .connected_demand()
        .reserve_bytes(600)
        .expect("600 of 1,000 bytes are granted");
    dispatch
        .connected_demand()
        .reserve_bytes(400)
        .expect("the allowance is inclusive");
    assert_eq!(dispatch.connected_demand().bytes_used_for_tests(), 1_000);
    let refused = dispatch
        .connected_demand()
        .reserve_bytes(1)
        .expect_err("a byte past the allowance is refused");
    assert!(refused.contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT));
    assert_eq!(
        dispatch.connected_demand().bytes_used_for_tests(),
        1_000,
        "a refused reservation charges nothing"
    );
    let work = dispatch
        .connected_demand()
        .charge()
        .expect_err("the trip is sticky for the demand's work too");
    assert!(work.contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT));
    assert_eq!(
        dispatch
            .connected_demand()
            .limit_report(PartialReasonSet::CONNECTED_MEMORY_LIMIT),
        (1_000, 1_000, "construction-bytes"),
        "the budget verdict names the memory rail and its numbers"
    );
}

/// A reservation that would overflow the counter is refused, not wrapped.
#[test]
fn an_overflowing_reservation_is_refused() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let (_guard, _) = dispatch.enter_connected_demand(false);
    dispatch
        .connected_demand()
        .reserve_bytes(10)
        .expect("ten bytes are granted");
    let refused = dispatch
        .connected_demand()
        .reserve_bytes(usize::MAX)
        .expect_err("an overflowing reservation is refused");
    assert!(refused.contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT));
}

/// Every connected demand starts from zero bytes, and the charges of one
/// never reach the next.
#[test]
fn each_connected_demand_starts_from_zero_bytes() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    dispatch.connected_demand().set_byte_limit_for_tests(100);
    {
        let (_guard, _) = dispatch.enter_connected_demand(false);
        dispatch
            .connected_demand()
            .reserve_bytes(100)
            .expect("the whole allowance is granted");
        assert!(dispatch.connected_demand().reserve_bytes(1).is_err());
    }
    let (_guard, trip) = dispatch.enter_connected_demand(false);
    assert!(trip.is_none(), "the next demand starts untripped");
    assert_eq!(dispatch.connected_demand().bytes_used_for_tests(), 0);
    dispatch
        .connected_demand()
        .reserve_bytes(100)
        .expect("the next demand has its whole allowance");
}

#[test]
fn stopped_demands_never_offer_unevaluated_subjects_as_positional_values() {
    use std::sync::Arc;
    use verter_type_engine::semantic_query::{
        FactStatus, IndexKey, PrimitiveKind, ProjectionMode, ProjectionReductionContext,
        QueryResult, SemanticNodeData, SemanticQueryKey,
    };
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _builder = dispatch.enter_construction_scope();
    let number = dispatch
        .graph()
        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = dispatch
        .graph()
        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let object = super::tests::simple_object(dispatch.graph_arc(), &[("x", number)]);
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let keys = [
        SemanticQueryKey::ProjectPath {
            base: object,
            path: verter_type_expr::OwnedSlice::from([]),
            context,
        },
        SemanticQueryKey::KeyOf {
            base: object,
            context,
        },
        SemanticQueryKey::IndexedAccess {
            base: object,
            index: IndexKey::String(verter_type_expr::OwnedStr::from("x")),
            mode: ProjectionMode::Navigate,
        },
        SemanticQueryKey::ReduceUnion {
            members: Arc::from([number, string]),
            nullability: verter_session_query::flow::policy::NullabilityPolicy::Strict,
        },
        SemanticQueryKey::Conditional {
            check: number,
            extends: number,
            true_branch: string,
            false_branch: number,
            distributive: false,
            pending: None,
        },
    ];
    dispatch.connected_demand().set_byte_limit_for_tests(0);
    {
        let (_demand, _) = dispatch.enter_connected_demand(false);
        assert!(dispatch.connected_demand().reserve_bytes(1).is_err());
        for key in keys.iter().cloned() {
            let read = dispatch.execute_read(key);
            assert!(
                matches!(read.fact_status(), FactStatus::Unavailable(_)),
                "stopped work establishes no positional guarantee: {read:?}"
            );
            assert!(!matches!(read.value().clone(), QueryResult::Value(_)));
        }
    }
    dispatch
        .connected_demand()
        .set_byte_limit_for_tests(u64::MAX);
    for key in keys {
        let read = dispatch.execute_read(key);
        assert_eq!(
            read.fact_status().clone(),
            FactStatus::Complete,
            "the unaffected control completes: {read:?}"
        );
        assert!(matches!(read.value().clone(), QueryResult::Value(_)));
    }
}

#[test]
fn finite_mapped_batch_stops_at_its_first_refusal_and_warms_when_complete() {
    use std::sync::Arc;
    use verter_type_engine::project_semantic_dispatch::raise::{
        enable_dispatch_trace_for_test, DISPATCH_TRACE,
    };
    use verter_type_engine::semantic_query::{
        DeclIdentity, IndexKey, LiteralValue, MapperKey, MapperKind, OptionalityMod,
        ProjectionMode, ReadonlyMod, SemanticNodeData, SemanticQueryKey,
    };
    for remap in [false, true] {
        let host = host();
        let _scope = host
            .project_type_store()
            .semantic_graph()
            .enter_node_root_scope();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();
        let keys = (0..64)
            .map(|i| {
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(format!(
                    "k{i}"
                ))))
            })
            .collect::<Vec<_>>();
        let domain = graph.intern_node(SemanticNodeData::Union(
            verter_type_engine::semantic_query::composite::CompositeList::test_fixture(Arc::from(
                keys.into_boxed_slice(),
            )),
        ));
        let binder = graph.intern_node(SemanticNodeData::TypeParam {
            decl: DeclIdentity::synthetic("K"),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: verter_type_expr::OwnedStr::from("K"),
        });
        let mapped = graph.intern_node(SemanticNodeData::Mapped {
            source: domain,
            mapper: MapperKey {
                over_type_variable: false,
                modifiers_type: None,
                parameter_node: binder,
                key_space: domain,
                value_expr: binder,
                optionality: OptionalityMod::Keep,
                readonly: ReadonlyMod::Keep,
                name_remap: remap.then_some(binder),
                kind: MapperKind::Computed,
            },
        });
        let key = SemanticQueryKey::IndexedAccess {
            base: mapped,
            index: IndexKey::Computed(domain),
            mode: ProjectionMode::Expanded,
        };
        dispatch.connected_demand().set_limits_for_tests(200, 24);
        let _trace = enable_dispatch_trace_for_test();
        let tight = dispatch.execute_read(key.clone());
        assert!(tight.result_is_partial());
        assert!(tight
            .partial_reason_classes()
            .contains(PartialReasonSet::PROJECTION_WORK_LIMIT));
        assert!(tight.cache_suppress());
        let accesses = DISPATCH_TRACE.with(|trace| {
            trace
                .borrow()
                .iter()
                .filter(|kind| **kind == "IndexedAccess")
                .count()
        });
        assert_eq!(
            accesses, 1,
            "a started batch must not restart through per-key dispatch after refusal"
        );
        dispatch.connected_demand().set_limits_for_tests(10_000, 24);
        let complete = dispatch.execute_read(key.clone());
        assert!(!complete.result_is_partial() && !complete.cache_suppress());
        let before = graph.stats_snapshot();
        let repeat = dispatch.execute_read(key);
        assert!(!repeat.result_is_partial() && !repeat.cache_suppress());
        assert!(matches!((repeat.value().clone(), complete.value().clone()),
        (verter_type_engine::semantic_query::QueryResult::Value(a), verter_type_engine::semantic_query::QueryResult::Value(b)) if a == b));
        assert!(graph.stats_snapshot().hits > before.hits);
    }
}
