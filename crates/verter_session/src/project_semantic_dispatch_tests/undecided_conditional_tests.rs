//! A conditional type the checker decides and the lane cannot is published
//! as a typed gap (`PartialReasonSet::UNDECIDED_CONDITIONAL`), never as a
//! complete answer; a conditional the checker itself defers stays complete.

use super::checker_probe_lane_tests::{default_probe_host, with_probe_outcome_on_host};
use super::evaluate::StructuralFactDemandOutcome;
use crate::semantic_query::PartialReasonSet;

const SOURCE: &str = "type R<T> = { [K in keyof T as K]: T[K] };\n";

/// The published outcome of `probe`: the reduced node's data, or the
/// partial reasons.
fn published(probe: &str) -> Result<String, PartialReasonSet> {
    let host = default_probe_host();
    with_probe_outcome_on_host(
        &host,
        Default::default(),
        SOURCE,
        probe,
        |dispatch, outcome| match outcome {
            StructuralFactDemandOutcome::Complete(node) => {
                Ok(format!("{:?}", dispatch.graph().node_data(node)))
            }
            StructuralFactDemandOutcome::Recovered { reasons, .. }
            | StructuralFactDemandOutcome::Partial(reasons) => Err(reasons),
        },
    )
}

/// tsc 7.0.2 (`--strict`, and the same in the other three settings)
/// decides every conditional here: `{ x: 1 } extends R<infer X> ? X : "no"`
/// is `unknown` (a key-remapped mapped type infers nothing), `"a" extends
/// string ? 1 : 2` is `1`, and in `{ f<T>(x: T): T extends string ? 1 : 2
/// }` the conditional over the method's own `T` stays deferred, printed as
/// written.
///
/// The lane does not relate a source to a key-remapped mapped pattern, so
/// that conditional is undecided: its shell publishes as a partial demand
/// carrying `UNDECIDED_CONDITIONAL`, where it used to publish complete. The
/// decided conditional and the checker's own deferral stay complete.
#[test]
fn a_conditional_the_lane_cannot_decide_is_a_typed_gap_never_complete() {
    let undecided = published("{ x: 1 } extends R<infer X> ? X : \"no\"");
    assert!(
        matches!(undecided, Err(reasons) if reasons.contains(PartialReasonSet::UNDECIDED_CONDITIONAL)),
        "an undecided conditional is a typed gap, got {undecided:?}"
    );
    let decided = published("\"a\" extends string ? 1 : 2");
    assert!(
        matches!(&decided, Ok(text) if text.contains("Number(1.0)")),
        "a decided conditional publishes its branch, got {decided:?}"
    );
    let deferred = published("{ f<T>(x: T): T extends string ? 1 : 2 }");
    assert!(
        matches!(&deferred, Ok(text) if text.starts_with("Some(Object")),
        "the checker's own deferral publishes complete, got {deferred:?}"
    );
}
