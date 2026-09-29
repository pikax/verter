//! A relation's work is charged to the connected-work ledger, the one
//! operational envelope: every worklist step and every alternative a union
//! target offers is a unit, and a refusal is the typed budget outcome,
//! never a verdict. There is no allowance of the relation's own below the
//! checker's relation-complexity limit.

use super::checker_probe_lane_tests::with_probe;
use super::dispatch_txn::RelationStep;
use super::ProjectSemanticDispatch;
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};

/// `type S = { p0: 0 } | … ; type T = { p0: number } | … ;` over `count`
/// arms each: every arm of `S` fits the arm of `T` with its property and no
/// arm before it.
fn object_unions(count: usize) -> String {
    let arms = |value: &dyn Fn(usize) -> String| {
        (0..count)
            .map(|i| format!("{{ p{i}: {} }}", value(i)))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    format!(
        "type S = {};\ntype T = {};\n",
        arms(&|i| i.to_string()),
        arms(&|_| "number".to_owned())
    )
}

/// The `S` and `T` a `[S, T]` probe reads.
fn pair(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> (SemanticNodeId, SemanticNodeId) {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Tuple { elements, .. }) if elements.len() == 2 => {
            (elements[0].value, elements[1].value)
        }
        other => panic!("the probe must read the pair [S, T], got {other:?}"),
    }
}

/// Forty object arms against forty: each arm of `S` tries the arms of `T`
/// up to its own, 820 alternatives in all, and holds. Every alternative is
/// charged to the ledger; a ledger that cannot pay for them ends the
/// relation on the typed budget outcome, never on a verdict.
///
/// Measured on TypeScript 7.0.2 (all four `strictNullChecks` ×
/// `noImplicitAny` settings agree): `[S] extends [T] ? 1 : 2` is `1` over
/// 600, 1,800 and 3,200 arms each.
#[test]
fn a_union_targets_alternatives_are_connected_work() {
    let source = object_unions(40);
    with_probe(&source, "[S, T]", |dispatch, node| {
        let (s, t) = pair(dispatch, node);
        dispatch.set_connected_limits_for_tests(256, 24);
        let refused = dispatch.execute_relate(dispatch.relate_key_for(s, t));
        assert!(
            matches!(
                refused,
                RelationStep::BudgetExceeded(_) | RelationStep::Unknown
            ),
            "a ledger of 256 units cannot pay for 820 alternatives, got {refused:?}"
        );
    });
    with_probe(&source, "[S, T]", |dispatch, node| {
        let (s, t) = pair(dispatch, node);
        let related = dispatch.execute_relate(dispatch.relate_key_for(s, t));
        assert!(
            matches!(related, RelationStep::Assignable { .. }),
            "every arm of S fits an arm of T, got {related:?}"
        );
        assert!(
            dispatch.connected_demand.work_used_for_tests() >= 820,
            "the 820 alternatives are charged, charged {}",
            dispatch.connected_demand.work_used_for_tests()
        );
    });
}
