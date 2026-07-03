//! Compile-FAIL fixture: the inverse of `recursive_self_rejects_typeexpr_arm` —
//! the `#[no_storedspan(recursive_self)]` escape omits ONLY the `Arc<[Self]>`
//! self-bound. A carrier that uses the escape AND grows a NEW non-recursive arm
//! owning a `verter_span::Span` still FAILS `NoStoredSpan`: the non-recursive
//! arm keeps its `Span: NoStoredSpan` witness bound, which is unsatisfiable
//! (the marker deliberately provides no `Span` leaf).

use std::sync::Arc;

#[derive(verter_no_storedspan::NoStoredSpan)]
#[no_storedspan(recursive_self)]
enum Bad {
    // The approved fixed-point self-container arm — its self-bound is omitted.
    Rec(Arc<[Bad]>),
    // A NEW non-recursive arm owning a `Span` — its bound is KEPT and fails.
    NonRecursive(verter_span::Span),
}

fn main() {
    let _ = std::mem::size_of::<Bad>();
}
