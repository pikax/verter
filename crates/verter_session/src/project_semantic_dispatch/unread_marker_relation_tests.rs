//! A typed marker for a value Verter did not read — an absence, an
//! unmodelled position, an unsupported surface, a partial or control
//! carrier — is never a relation fact: the assignability and comparable
//! relations answer undecided over one, even against the same marker,
//! except against a top type, which every type relates to. This is a
//! Verter invariant, not a checker measurement: the checker has no such
//! markers.

use std::sync::Arc;

use super::checker_probe_lane_tests::with_probe;
use super::dispatch_txn::RelationStep;
use super::relation::ComparabilityVerdict;
use crate::semantic_query::{PartialReasonSet, PrimitiveKind, QueryError, SemanticNodeData};
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node;

/// Every marker kind the relation may meet as an operand.
fn marker_kinds() -> Vec<QueryError> {
    vec![
        QueryError::Miss,
        QueryError::RaiseMiss,
        QueryError::UnmodeledPosition,
        QueryError::OpenSurface,
        QueryError::UnrepresentableSurface,
        QueryError::UnrepresentableSurfaceMember,
        QueryError::AliasCycle {
            chain: Arc::from(Vec::<Arc<str>>::new()),
        },
        QueryError::RaiseAliasCycle,
        QueryError::TypeParamCycle,
        QueryError::Cancelled,
        QueryError::UnstableState { attempts: 3 },
        QueryError::SignatureOverflow,
        QueryError::StaleSemanticOperand,
        QueryError::IncompleteSemanticOperand {
            reasons: PartialReasonSet::BUDGET_EXCEEDED,
        },
    ]
}

/// A marker relates to nothing — not to itself, another marker, an object
/// or a primitive — in either relation, and every type relates to
/// `unknown` and `any`.
#[test]
fn an_unread_marker_is_never_a_relation_fact() {
    with_probe("", "1", |dispatch, _| {
        let graph = dispatch.graph();
        let object = graph.intern_node(SemanticNodeData::Object(
            crate::semantic_query::SurfaceView::from_members(Vec::new(), None),
        ));
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
        let any = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
        let markers: Vec<_> = marker_kinds()
            .into_iter()
            .map(|error| (format!("{error:?}"), dispatch.opaque(error)))
            .collect();
        let other = markers[0].1;
        let mut failures = Vec::new();
        for (name, marker) in &markers {
            for (what, source, target) in [
                ("itself", *marker, *marker),
                ("another marker", *marker, other),
                ("the empty object", *marker, object),
                ("`string`", *marker, string),
                ("the empty object, as a target", object, *marker),
            ] {
                if *marker == other && what == "another marker" {
                    continue;
                }
                if !matches!(
                    dispatch.execute_relate_pair(source, target),
                    RelationStep::Unknown
                ) {
                    failures.push(format!("{name} against {what}: assignability decided"));
                }
                if !matches!(
                    dispatch.nodes_comparable(source, target),
                    ComparabilityVerdict::Undecided
                ) {
                    failures.push(format!("{name} against {what}: comparability decided"));
                }
            }
            for top in [unknown, any] {
                if !matches!(
                    dispatch.execute_relate_pair(*marker, top),
                    RelationStep::Assignable { .. }
                ) {
                    failures.push(format!("{name} is not assignable to a top type"));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// Two unmodelled values stay an undecided conditional in a probe: a
/// function-local class's instance is an unmodelled position.
#[test]
fn a_conditional_over_two_unmodelled_values_stays_undecided() {
    let fixture = "\
type Ext<A, B> = [A] extends [B] ? 'y' : 'n';
function locals() { class LB { x = 1 } class LS extends LB { y = 2 } return [new LS(), new LB()] as const; }
";
    for probe in [
        "Ext<ReturnType<typeof locals>[0], ReturnType<typeof locals>[1]>",
        "Ext<ReturnType<typeof locals>[0], ReturnType<typeof locals>[0]>",
    ] {
        let measured = with_probe(fixture, probe, |dispatch, node| {
            render_node(dispatch, node, 0)
        });
        assert_eq!(measured, "Conditional(…)", "`{probe}` is no relation fact");
    }
}
