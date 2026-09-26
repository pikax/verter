//! A literal related to a union target that holds it answers from the
//! union's membership, as the checker's `containsType` test does, so
//! relating a wide literal union to its superset costs one pass per member
//! rather than one relation per preceding arm.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact
//! fixture with `tsc --ignoreConfig --noEmit --strict` under each of
//! `strictNullChecks` and `noImplicitAny` on and off (the four settings
//! agree on every probe) as `const s: "q" = null! as <probe>;`, read off
//! the TS2322 message.

use super::checker_probe_lane_tests::{mismatches, with_probe};

/// `type W = "m0" | "m1" | … `, `width` members.
fn wide_union(width: usize) -> String {
    let members: Vec<String> = (0..width).map(|index| format!("\"m{index}\"")).collect();
    format!("type W = {};\n", members.join(" | "))
}

const EXCLUDED_TO_WHOLE: &str = "[Exclude<W, \"m0\">] extends [W] ? 1 : 2";

/// Measured on TypeScript 7.0.2 (all four settings): over an 800-member
/// `W`, `[Exclude<W, "m0">] extends [W] ? 1 : 2` is `1` and `[W] extends
/// [Exclude<W, "m0">] ? 1 : 2` is `2`.
#[test]
fn excluding_one_member_of_an_800_member_union_relates_back_to_it() {
    let failures = mismatches(
        &wide_union(800),
        &[
            (EXCLUDED_TO_WHOLE, "1"),
            ("[W] extends [Exclude<W, \"m0\">] ? 1 : 2", "2"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Relating `Exclude<W, "m0">` back to `W` makes the same number of
/// relation checks per added member at 100, 200 and 300 members.
///
/// Oracle: as for
/// [`excluding_one_member_of_an_800_member_union_relates_back_to_it`], at
/// every width.
#[test]
fn relating_a_wide_literal_union_to_its_superset_costs_the_same_checks_per_member() {
    let checks = |width: usize| {
        with_probe(&wide_union(width), EXCLUDED_TO_WHOLE, |dispatch, _| {
            dispatch.graph().stats_snapshot().relation_check_count
        })
    };
    let (at_100, at_200, at_300) = (checks(100), checks(200), checks(300));
    assert_eq!(
        at_200 - at_100,
        at_300 - at_200,
        "relation checks at 100 / 200 / 300 members: {at_100} / {at_200} / {at_300}"
    );
}

/// `type K = "k0" | … `, `width` members, and `last(x: K)` returning `x`
/// past a `throw` on each of its first `width - 1` members.
fn equality_guard_chain(width: usize) -> String {
    let members: Vec<String> = (0..width).map(|index| format!("\"k{index}\"")).collect();
    let mut guards = String::new();
    for index in 0..width - 1 {
        guards.push_str(&format!("  if (x === \"k{index}\") throw 0;\n"));
    }
    format!(
        "type K = {};\nexport function last(x: K) {{\n{guards}  return x;\n}}\n",
        members.join(" | ")
    )
}

/// A chain of `x === "k<i>"` guards over a literal union filters every
/// remaining arm against each guard's literal, as the checker's
/// `filterType` does; each such pair is two literal types, decided by
/// value, so no relation of the chain reaches the structural reducer.
///
/// Measured on TypeScript 7.0.2 (`--declaration --emitDeclarationOnly`,
/// all four settings): `last` is `"k799"` over an 800-member union and
/// `"k99"` over a 100-member one.
#[test]
fn an_equality_guard_chain_over_a_literal_union_reduces_no_relation() {
    let before = super::ProjectSemanticDispatch::relation_reductions_for_tests();
    let failures = mismatches(
        &equality_guard_chain(100),
        &[("ReturnType<typeof last>", "\"k99\"")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        super::ProjectSemanticDispatch::relation_reductions_for_tests() - before,
        0,
        "every relation a literal guard chain asks is a pair of literals"
    );
    let failures = mismatches(
        &equality_guard_chain(800),
        &[("ReturnType<typeof last>", "\"k799\"")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
