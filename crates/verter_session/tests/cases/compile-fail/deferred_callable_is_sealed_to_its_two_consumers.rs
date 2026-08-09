//! Compile-fail fixture: the index-composed `DeferredCallable` carrier is
//! sealed to the `ResolveOverloadSet` / `ResolveCall` consumers.
//!
//! Four independent barriers, each of which must be a compile error from
//! outside the crate:
//!
//! 1. The carrier's composed parts are private fields — no field read.
//! 2. `DeferredCallable::compose` is `pub(crate)` — no forged carrier.
//! 3. `DeferredCallableConsumer` has a PRIVATE sealed supertrait, so no new
//!    consumer kind can be added; the consumer set is closed at its
//!    defining module.
//! 4. The two consumer witnesses have a private tuple field and a
//!    `pub(crate)` mint, so neither can be produced — and `parts` therefore
//!    cannot be called at all.
//!
//! There is deliberately NO `return_type` accessor to attempt: the carrier
//! has no return-type slot, so "observe the deferred return as a failed
//! one" is unrepresentable rather than merely unreachable.

use verter_session::semantic_query::deferred_callable::{
    DeferredCallable, DeferredCallableConsumer, ResolveCallConsumer, ResolveOverloadSetConsumer,
};

struct ForeignConsumer;

// (3) The sealed supertrait is a private module inside `deferred_callable`,
// so an out-of-crate impl cannot satisfy it.
impl DeferredCallableConsumer for ForeignConsumer {}

fn read_parts(callable: &DeferredCallable) {
    // (4) Neither witness is mintable from outside the crate.
    let _ = ResolveOverloadSetConsumer::witness();
    let _ = ResolveCallConsumer::witness();
    let _ = ResolveOverloadSetConsumer(());
    let _ = ResolveCallConsumer(());
    // (3)/(4) `parts` needs a sealed witness value.
    let _ = callable.parts(&ForeignConsumer);
}

fn forge() {
    // (2) The composer is `pub(crate)`.
    let _ = DeferredCallable::compose;
}

fn main() {}
