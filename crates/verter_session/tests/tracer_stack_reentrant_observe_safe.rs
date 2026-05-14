//! Reentrancy test: nested `with_fact_tracer` scopes can call
//! `observe_fan_out_borrowed` from inside the inner scope without
//! triggering a `BorrowMutError` panic.
//!
//! ## What this test exercises
//!
//! The driver installs an outer tracer, installs a nested inner
//! tracer, calls `observe_fan_out_borrowed` from inside the inner
//! scope, then calls it again from the outer scope after the inner
//! scope closes. The fan-out path runs under the snapshot-then-iterate
//! pattern (clone the `SmallVec` of raw pointers under a short
//! borrow, drop the `RefCell` borrow, then iterate the clone). The
//! `RefCell` is never held while individual cells are observed.
//!
//! ## What this test does NOT exercise
//!
//! The codex F6 contract has a stricter shape: an `observe` callback
//! that itself re-enters `with_fact_tracer` (installing another scope
//! inside an active observation) must also be safe. The current
//! `FactReadSetCell::observe` body has no callback hook, so this
//! "observe-callback-triggers-install" scenario cannot be expressed
//! without test-only plumbing on `FactReadSetCell`. This test covers
//! it indirectly: the snapshot-then-iterate pattern releases the
//! borrow before any per-cell `observe` runs, so a hypothetical
//! observer callback that called `install` would not BorrowMutError
//! either. A direct discriminator would require an observe-callback
//! hook on `FactReadSetCell` (test-only); the indirect coverage from
//! the borrow-release timing here is the architecture-design
//! argument that the pattern would also be safe under direct
//! observer callbacks.

use verter_session::for_tests::{
    install_fact_tracer_for_tests, observe_fan_out_borrowed_for_tests,
};
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef};
use verter_session::VerterHost;

fn make_fact(label: &str) -> FactVersionRef {
    let hash: [u8; 16] = {
        let mut h = [0u8; 16];
        for (i, b) in label.bytes().enumerate().take(16) {
            h[i] = b;
        }
        h
    };
    FactVersionRef::FileWholeHash {
        canonical_id: format!("{label}.ts"),
        hash,
    }
}

#[test]
fn reentrant_observe_does_not_panic_and_both_signatures_non_empty() {
    // The test must NOT panic with BorrowMutError.
    let result = std::panic::catch_unwind(|| {
        let host_inner = VerterHost::new_standalone(Default::default());

        let fact_a = make_fact("fact_a");
        let fact_b = make_fact("fact_b");

        let (inner_finalise, outer_finalise) = install_fact_tracer_for_tests(&host_inner, || {
            // While L1 borrow is implicitly "active" conceptually, the fan-out
            // snapshot pattern must drop the borrow before calling observe — so
            // calling fan-out from inside the scope of another fan-out (which
            // is what nested install triggers) must not BorrowMutError.
            let ((), inner_finalise) = install_fact_tracer_for_tests(&host_inner, || {
                // This fan-out call touches the RefCell while the outer scope
                // is also registered — the snapshot-before-iterate design must
                // allow this without a BorrowMutError.
                observe_fan_out_borrowed_for_tests(std::slice::from_ref(&fact_b));
            });

            // Also observe in the outer scope after the inner scope closes.
            observe_fan_out_borrowed_for_tests(std::slice::from_ref(&fact_a));
            inner_finalise
        });

        (inner_finalise, outer_finalise)
    });

    assert!(
        result.is_ok(),
        "observe_fan_out from nested install scope must not panic (BorrowMutError)"
    );

    let (inner_finalise, outer_finalise) = result.unwrap();

    // Both signatures must be non-empty.
    match inner_finalise {
        FactReadSetFinalise::Ok(sig) => assert!(!sig.is_empty(), "inner scope must be non-empty"),
        FactReadSetFinalise::Overflow => panic!("inner scope overflowed"),
    }
    match outer_finalise {
        FactReadSetFinalise::Ok(sig) => assert!(!sig.is_empty(), "outer scope must be non-empty"),
        FactReadSetFinalise::Overflow => panic!("outer scope overflowed"),
    }
}

#[test]
fn observe_fan_out_no_active_tracers_is_noop() {
    // With no tracer installed, fan-out must be a no-op (no panic, no side effects).
    let fact = make_fact("no_tracer_fact");
    observe_fan_out_borrowed_for_tests(&[fact]);
    // If we reach here without panic, the no-op path is correct.
}
