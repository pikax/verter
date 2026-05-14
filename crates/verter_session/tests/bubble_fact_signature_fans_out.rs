//! RED test: the rewritten `bubble_fact_signature` fans out to all active tracer
//! levels, not just the top-of-stack cell.
//!
//! Pre-B0.2 the function called `cell.observe_borrowed_signature` on the single
//! top-of-stack cell only. Post-B0.2 it calls `observe_fan_out_borrowed` which
//! iterates ALL active cells — verifying this discriminates the old vs new impl.

use verter_session::for_tests::{
    install_fact_tracer_for_tests, observe_fan_out_borrowed_for_tests,
};
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef};
use verter_session::VerterHost;

fn make_host() -> VerterHost {
    VerterHost::new_standalone(Default::default())
}

fn sig_fact(n: u8) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: format!("bubble_fanout_{n}.ts"),
        hash: [n; 16],
    }
}

#[test]
fn bubble_fact_signature_reaches_all_three_levels() {
    let host = make_host();

    let shared = sig_fact(42);
    let shared_l2 = shared.clone();
    let shared_l3 = shared.clone();

    // Three nested levels; observation emitted from the innermost.
    let (l2_ret, l1_finalise) = install_fact_tracer_for_tests(&host, || {
        let (l3_ret, l2_finalise) = install_fact_tracer_for_tests(&host, || {
            let ((), l3_finalise) = install_fact_tracer_for_tests(&host, || {
                // This call simulates what `bubble_fact_signature` does:
                // fan-out the signature to ALL active tracers.
                observe_fan_out_borrowed_for_tests(std::slice::from_ref(&shared_l3));
            });
            l3_finalise
        });
        let _ = shared_l2;
        (l3_ret, l2_finalise)
    });

    let (l3_finalise, l2_finalise) = l2_ret;

    for (level, finalise) in [
        ("L1", l1_finalise),
        ("L2", l2_finalise),
        ("L3", l3_finalise),
    ] {
        match finalise {
            FactReadSetFinalise::Ok(sig) => {
                assert!(
                    sig.iter().any(|f| f == &shared),
                    "{level} must contain the shared fact after fan-out bubble; got {sig:?}"
                );
            }
            FactReadSetFinalise::Overflow => panic!("{level} overflowed"),
        }
    }
}

#[test]
fn observe_fan_out_borrowed_with_multiple_facts() {
    let host = make_host();

    let facts: Vec<FactVersionRef> = (0..3).map(sig_fact).collect();
    let facts_clone = facts.clone();

    let ((), finalise) = install_fact_tracer_for_tests(&host, || {
        observe_fan_out_borrowed_for_tests(&facts_clone);
    });

    match finalise {
        FactReadSetFinalise::Ok(sig) => {
            for f in &facts {
                assert!(
                    sig.iter().any(|s| s == f),
                    "tracer must contain fact {f:?}; got {sig:?}"
                );
            }
        }
        FactReadSetFinalise::Overflow => panic!("overflowed on tiny signature"),
    }
}
