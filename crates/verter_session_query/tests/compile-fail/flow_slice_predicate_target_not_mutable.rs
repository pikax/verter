// A built predicate's gated target cannot be replaced or dropped in place.
use verter_session_query::flow::slice::SlicePredicate;

fn retarget(predicate: &mut SlicePredicate) {
    predicate.target = None;
}

fn main() {}
