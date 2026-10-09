// A predicate's gated target is gated from the authored predicate's own
// target. Pairing an authored predicate with an independently supplied
// target does not compile.
use verter_session_query::flow::slice::{GatedType, SlicePredicate};

fn forge(predicate: &SlicePredicate, target: GatedType) -> SlicePredicate {
    SlicePredicate {
        predicate: std::sync::Arc::new(predicate.predicate().clone()),
        target: Some(target),
    }
}

fn main() {}
