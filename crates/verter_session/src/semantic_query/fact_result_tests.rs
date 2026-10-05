//! The fact-result contract: the member-domain distinctions a surface
//! claim answers, and the strict composition algebra.

use super::fact_result::summarize;
use super::{FactResult, FactStatus, MemberDomain, PartialReason, ResultCompleteness};
use crate::semantic_query::surface_resolution::{NonEmptyReasons, SurfaceResolution};

fn budget() -> NonEmptyReasons {
    NonEmptyReasons::of(PartialReason::BudgetExceeded)
}

fn missing() -> NonEmptyReasons {
    NonEmptyReasons::of(PartialReason::MissingDependency)
}

#[test]
fn an_exact_empty_domain_is_complete_and_an_unbuilt_one_is_unavailable() {
    let empty = SurfaceResolution::<Vec<&str>>::resolved(Vec::new()).into_fact();
    assert_eq!(
        empty,
        FactResult::complete(MemberDomain::Closed(Vec::new()))
    );
    assert_eq!(empty.exact(), Some(&MemberDomain::Closed(Vec::new())));
    assert_eq!(empty.completeness(), ResultCompleteness::Complete);

    let unbuilt = SurfaceResolution::<Vec<&str>>::incomplete(budget()).into_fact();
    assert_eq!(unbuilt, FactResult::unavailable(budget()));
    assert_eq!(unbuilt.exact(), None, "an unbuilt domain is never empty");
    assert!(matches!(
        unbuilt.completeness(),
        ResultCompleteness::Partial(_)
    ));
}

#[test]
fn an_open_domain_is_complete_and_distinct_from_a_closed_one() {
    let open = SurfaceResolution::open_presence(vec!["a"]).into_fact();
    let closed = SurfaceResolution::resolved(vec!["a"]).into_fact();
    assert_eq!(open, FactResult::complete(MemberDomain::Open(vec!["a"])));
    assert_eq!(
        closed,
        FactResult::complete(MemberDomain::Closed(vec!["a"]))
    );
    assert_ne!(
        open, closed,
        "an omission from an open domain proves nothing; from a closed one it is absence"
    );
    assert_eq!(
        SurfaceResolution::<Vec<&str>>::no_surface().into_fact(),
        FactResult::complete(MemberDomain::NoSurface),
        "no such surface is the complete negative answer"
    );
}

#[test]
fn a_built_subset_is_an_approximate_presence_only_domain() {
    let subset = SurfaceResolution::incomplete_with(missing(), vec!["a"]).into_fact();
    assert_eq!(
        subset,
        FactResult::approximate(MemberDomain::Open(vec!["a"]), missing())
    );
    assert_eq!(subset.exact(), None);

    // A closed claim demoted by a partial read keeps its members as a
    // lower bound: it no longer proves any absence.
    let demoted = SurfaceResolution::resolved(vec!["a"])
        .with_read_partiality(Some(budget()))
        .into_fact();
    assert_eq!(
        demoted,
        FactResult::approximate(MemberDomain::Open(vec!["a"]), budget())
    );
    // A negative answer demoted by a partial read has no subset to offer.
    assert_eq!(
        SurfaceResolution::<Vec<&str>>::no_surface()
            .with_read_partiality(Some(budget()))
            .into_fact(),
        FactResult::unavailable(budget())
    );
}

#[test]
fn a_dependent_fact_is_never_better_than_what_it_derives_from() {
    let derived = FactResult::approximate(2, budget()).and_then(|n| FactResult::complete(n * 3));
    assert_eq!(derived, FactResult::approximate(6, budget()));

    let joined = FactResult::approximate(2, budget())
        .and_then(|n| FactResult::approximate(n * 3, missing()));
    assert_eq!(
        joined,
        FactResult::approximate(6, budget().union(missing()))
    );

    let mut ran = false;
    let short = FactResult::<i32>::unavailable(missing()).and_then(|n| {
        ran = true;
        FactResult::complete(n)
    });
    assert_eq!(short, FactResult::unavailable(missing()));
    assert!(!ran, "nothing derives from an unavailable fact");

    let unavailable_from_approximate = FactResult::approximate(2, budget())
        .and_then(|_| FactResult::<i32>::unavailable(missing()));
    assert_eq!(
        unavailable_from_approximate,
        FactResult::unavailable(missing().union(budget()))
    );
}

#[test]
fn a_conjunction_is_complete_only_when_both_inputs_are() {
    assert_eq!(
        FactResult::complete(1).zip(FactResult::complete("a")),
        FactResult::complete((1, "a"))
    );
    assert_eq!(
        FactResult::complete(1).zip(FactResult::approximate("a", budget())),
        FactResult::approximate((1, "a"), budget())
    );
    assert_eq!(
        FactResult::approximate(1, missing()).zip(FactResult::<&str>::unavailable(budget())),
        FactResult::unavailable(missing().union(budget()))
    );
}

#[test]
fn the_summary_is_complete_only_when_every_published_fact_is() {
    assert_eq!(summarize([]), ResultCompleteness::Complete);
    assert_eq!(
        summarize(&[FactStatus::Complete, FactStatus::Complete]),
        ResultCompleteness::Complete
    );
    assert_eq!(
        summarize(&[
            FactStatus::Complete,
            FactStatus::Approximate(budget()),
            FactStatus::Unavailable(missing()),
        ]),
        ResultCompleteness::Partial(budget().union(missing()).get())
    );
}

#[test]
fn only_a_complete_result_yields_an_exact_value() {
    assert_eq!(FactResult::complete(1).into_exact(), Some(1));
    assert_eq!(FactResult::approximate(1, budget()).into_exact(), None);
    assert_eq!(FactResult::<i32>::unavailable(budget()).into_exact(), None);
    assert_eq!(
        FactResult::approximate(1, budget()).status(),
        FactStatus::Approximate(budget())
    );
}
