//! RED test: nested `with_fact_tracer` scopes both finalise non-empty signatures.
//!
//! Verifies that the ACTIVE_TRACERS stack allows a second installation while
//! an outer scope is already active. If nesting were still forbidden (pre-B0.2
//! behaviour) this test would panic; the new stack-based install must not panic
//! and each scope must finalise its own non-empty `FactReadSetFinalise::Ok`.

use verter_session::for_tests::{
    install_fact_tracer_for_tests, observe_fan_out_borrowed_for_tests,
};
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef};
use verter_session::VerterHost;

fn make_host() -> VerterHost {
    VerterHost::new_standalone(Default::default())
}

fn test_fact(n: u8) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: format!("nesting_test_{n}.ts"),
        hash: [n; 16],
    }
}

#[test]
fn tracer_stack_nesting_outer_and_inner_both_non_empty() {
    let host = make_host();

    let outer_fact = test_fact(1);
    let inner_fact = test_fact(2);

    // Install outer tracer scope.
    let (inner_result, outer_finalise) = install_fact_tracer_for_tests(&host, || {
        // Observe one fact in the outer scope before entering the inner scope.
        observe_fan_out_borrowed_for_tests(std::slice::from_ref(&outer_fact));

        // Install nested inner scope — must NOT panic on the new stack-based impl.
        let ((), inner_finalise) = install_fact_tracer_for_tests(&host, || {
            // Observe the inner fact; it should land in both the inner and outer cells
            // via fan-out.
            observe_fan_out_borrowed_for_tests(std::slice::from_ref(&inner_fact));
        });

        // Inner finalise must be Ok with exactly the inner fact (plus outer_fact
        // if fan-out bubbled it — the inner cell sees everything observed while
        // it was active, which includes the inner_fact observed via fan-out).
        match &inner_finalise {
            FactReadSetFinalise::Ok(sig) => {
                assert!(
                    !sig.is_empty(),
                    "inner tracer must capture at least the inner_fact"
                );
                assert!(
                    sig.iter().any(|f| f == &inner_fact),
                    "inner tracer must include inner_fact; got {sig:?}"
                );
            }
            FactReadSetFinalise::NonCacheable(_) => {
                panic!("inner scope unexpectedly non-cacheable")
            }
            FactReadSetFinalise::Overflow => panic!("inner scope overflowed unexpectedly"),
        }
        inner_finalise
    });

    // Outer finalise must be Ok and include both outer_fact AND inner_fact
    // (inner_fact was fanned out to all active tracers including the outer one).
    match outer_finalise {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.iter().any(|f| f == &outer_fact),
                "outer tracer must include outer_fact; got {sig:?}"
            );
            assert!(
                sig.iter().any(|f| f == &inner_fact),
                "outer tracer must include inner_fact via fan-out; got {sig:?}"
            );
        }
        FactReadSetFinalise::NonCacheable(_) => panic!("outer scope unexpectedly non-cacheable"),
        FactReadSetFinalise::Overflow => panic!("outer scope overflowed unexpectedly"),
    }

    // inner_result is the inner FactReadSetFinalise — just verify it was Ok
    assert!(
        matches!(inner_result, FactReadSetFinalise::Ok(_)),
        "inner finalise should be Ok"
    );
}
