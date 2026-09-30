//! A relation's work is charged to the connected-work ledger, the one
//! operational envelope: every worklist step and every alternative a union
//! target offers is a unit, and a refusal is the typed budget outcome,
//! never a verdict. There is no allowance of the relation's own below the
//! checker's relation-complexity limit.

use super::checker_probe_lane_tests::{mismatches, with_probe};
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

/// Forty object arms against the same forty written in reverse: no arm of
/// `S` holds at its own position, so each scans the arms of `T` up to its
/// partner, 820 alternatives in all, and holds. Every alternative is
/// charged to the ledger; a ledger that cannot pay for them ends the
/// relation on the typed budget outcome, never on a verdict.
///
/// Measured on TypeScript 7.0.2 (all four `strictNullChecks` ×
/// `noImplicitAny` settings agree): `[S] extends [T] ? 1 : 2` is `1` over
/// 600 and 1,800 reversed arms each.
#[test]
fn a_union_targets_alternatives_are_connected_work() {
    let source = reversed_object_unions(40);
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

/// `object_unions` with `T`'s arms written in reverse, so no arm of `S`
/// finds its partner at its own position.
fn reversed_object_unions(count: usize) -> String {
    let source: Vec<String> = (0..count).map(|i| format!("{{ p{i}: {i} }}")).collect();
    let target: Vec<String> = (0..count)
        .rev()
        .map(|i| format!("{{ p{i}: number }}"))
        .collect();
    format!(
        "type S = {};\ntype T = {};\n",
        source.join(" | "),
        target.join(" | ")
    )
}

/// A union source relates each arm first to the target arm at its own
/// position (`eachTypeRelatedToType`), so aligned unions of 200 arms take
/// about one structured comparison per arm: they hold within an allowance
/// of 3 × 200 comparisons. Reversed, every arm scans the target, about
/// 200² / 2 comparisons: past the same allowance the check overflows and is
/// the checker's false (TS2859) — a resource partial of the operation
/// budget, never a proof that the unions do not relate.
///
/// Measured on TypeScript 7.0.2 (all four settings agree), through `[S]
/// extends [T] ? 1 : 2`: aligned, `1` over 600, 1,800 and 3,200 arms each;
/// reversed, `1` over 600 and 1,800 arms and `2` under TS2859 ("Excessive
/// complexity comparing types '[S]' and '[T]'") over 2,100 and 3,200 — the
/// checker's 2,000,000 recorded comparisons.
#[test]
fn a_union_source_relates_each_arm_to_its_position_first() {
    let relate = "[S] extends [T] ? 1 : 2";
    crate::semantic_query::checker_policy::with_relation_comparisons_for_tests(600, || {
        let aligned = mismatches(&object_unions(200), &[(relate, "1")]);
        assert!(aligned.is_empty(), "{}", aligned.join("\n"));
        let reversed = mismatches(&reversed_object_unions(200), &[(relate, "2")]);
        assert!(reversed.is_empty(), "{}", reversed.join("\n"));
        super::checker_probe_lane_tests::with_recovered_probe(
            &reversed_object_unions(200),
            relate,
            |_, _| {},
        );
    });
    let reversed = mismatches(&reversed_object_unions(200), &[(relate, "1")]);
    assert!(reversed.is_empty(), "{}", reversed.join("\n"));
}

/// A relation refused for complexity is never kept: its false depends on
/// the allowance, not the types, so a second read on the same host relates
/// the unions again and is again the recovered partial, never a complete
/// negative served from the memo.
#[test]
fn a_relation_refused_for_complexity_is_never_kept() {
    use super::evaluate::StructuralFactDemandOutcome;
    let source = reversed_object_unions(200);
    let host = super::checker_probe_lane_tests::default_probe_host();
    let read = || {
        super::checker_probe_lane_tests::with_probe_outcome_on_host(
            &host,
            Default::default(),
            &source,
            "[S] extends [T] ? 1 : 2",
            |_, outcome| matches!(outcome, StructuralFactDemandOutcome::Recovered { .. }),
        )
    };
    crate::semantic_query::checker_policy::with_relation_comparisons_for_tests(600, || {
        assert!(read(), "cold");
        assert!(read(), "warm");
    });
}

/// Aligned unions of 600, 1,800 and 3,200 arms relate within the production
/// ledger, as the checker relates them (see
/// [`a_union_source_relates_each_arm_to_its_position_first`]).
#[test]
fn aligned_object_unions_relate_in_linear_work() {
    for count in [600, 1800, 3200] {
        let failures = mismatches(&object_unions(count), &[("[S] extends [T] ? 1 : 2", "1")]);
        assert!(failures.is_empty(), "{count} arms: {}", failures.join("\n"));
    }
}

/// A structured comparison the relation records costs it at most
/// `RELATION_UNITS_PER_COMPARISON` ledger units, the mapping the production
/// ledger is to be sized by so a relation reaches the checker's TS2859
/// before the ledger refuses it: 200 reversed arms record about 200 × 201 /
/// 2 comparisons.
#[test]
fn a_recorded_comparison_costs_the_ledgers_mapped_units() {
    let count = 200usize;
    with_probe(
        &reversed_object_unions(count),
        "[S, T]",
        |dispatch, node| {
            let (s, t) = pair(dispatch, node);
            let related = dispatch.execute_relate(dispatch.relate_key_for(s, t));
            assert!(
                matches!(related, RelationStep::Assignable { .. }),
                "every arm of S fits an arm of T, got {related:?}"
            );
            let comparisons = count * (count + 1) / 2;
            let used = dispatch.connected_demand.work_used_for_tests();
            assert!(
                used <= comparisons * super::connected_demand::RELATION_UNITS_PER_COMPARISON,
                "{comparisons} comparisons charged {used} units"
            );
        },
    );
}

/// Each structured pair reserves its bytes before it is related: under a
/// construction allowance of 100 pairs, 40 reversed object arms (820 pairs)
/// stop on the memory rail, typed: the allowance is used up, the work cap
/// is not.
#[test]
fn a_relation_past_its_byte_allowance_stops_on_the_memory_rail() {
    let limit = 100 * super::connected_demand::RELATION_PAIR_BYTES;
    with_probe(&reversed_object_unions(40), "[S, T]", |dispatch, node| {
        let (s, t) = pair(dispatch, node);
        dispatch.set_construction_byte_limit_for_tests(limit);
        let refused = dispatch.execute_relate(dispatch.relate_key_for(s, t));
        assert!(
            matches!(refused, RelationStep::BudgetExceeded(_)),
            "the byte allowance stops the relation, got {refused:?}"
        );
        assert_eq!(
            dispatch.connected_demand.bytes_used_for_tests(),
            limit,
            "the relation used up its byte allowance"
        );
        assert!(
            dispatch.connected_demand.work_used_for_tests()
                < super::connected_demand::MAX_CONNECTED_PROJECTION_WORK,
            "the work cap did not stop it"
        );
    });
}
