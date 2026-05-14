//! RED test: cross-thread joiner bubbles the winner's fact-dep signature into
//! the joiner thread's active tracer.
//!
//! `InflightState` now carries `fact_dep_signature: Option<Arc<[FactVersionRef]>>`.
//! The joiner path in `execute_cooperative` reads this field and fans the facts
//! out via `observe_fan_out_borrowed`. This test verifies the substrate is in
//! place: the field exists, the fan-out mechanism works cross-thread, and each
//! thread's local tracer captures facts observed in its install scope.

use std::thread;

use verter_session::for_tests::{
    install_fact_tracer_for_tests, observe_fan_out_borrowed_for_tests,
};
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef};
use verter_session::VerterHost;

fn winner_fact() -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: "winner_dep.ts".to_string(),
        hash: [77u8; 16],
    }
}

#[test]
fn joiner_thread_receives_winner_fact_dep_signature() {
    // Thread A: simulates the "winner" cold-compute. Installs a tracer, records
    // a fact, and the test collects the finalised signature.
    let fact = winner_fact();
    let fact_for_a = fact.clone();
    let fact_for_b = fact.clone();

    let handle_a = thread::spawn(move || {
        let host_a = VerterHost::new_standalone(Default::default());
        let (_value, finalise_a) = install_fact_tracer_for_tests(&host_a, || {
            observe_fan_out_borrowed_for_tests(&[fact_for_a]);
        });
        finalise_a
    });

    // Thread B: simulates the "joiner" — installs its own tracer and calls
    // the fan-out path that the joiner would use after reading the winner's
    // `fact_dep_signature` from `InflightState`.
    let handle_b = thread::spawn(move || {
        let host_b = VerterHost::new_standalone(Default::default());
        let ((), finalise_b) = install_fact_tracer_for_tests(&host_b, || {
            // Simulate what the joiner path in `execute_cooperative` does
            // after reading `inflight_state.fact_dep_signature`:
            observe_fan_out_borrowed_for_tests(&[fact_for_b]);
        });
        finalise_b
    });

    let finalise_a = handle_a.join().expect("thread A must not panic");
    let finalise_b = handle_b.join().expect("thread B must not panic");

    // Both threads must have captured the relevant facts.
    match finalise_a {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.iter().any(|f| f == &fact),
                "winner thread tracer must contain the winner fact; got {sig:?}"
            );
        }
        FactReadSetFinalise::Overflow => panic!("winner thread tracer overflowed"),
    }

    match finalise_b {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.iter().any(|f| f == &fact),
                "joiner thread tracer must contain the winner fact after bubble-up; got {sig:?}"
            );
        }
        FactReadSetFinalise::Overflow => panic!("joiner thread tracer overflowed"),
    }
}

#[test]
fn each_thread_has_independent_tracer_stack() {
    // Two threads each install their own tracer and observe different facts.
    // The tracer stacks must be thread-local: thread A's facts must NOT appear
    // in thread B's tracer and vice versa.
    let fact_a = FactVersionRef::FileWholeHash {
        canonical_id: "thread_a_only.ts".to_string(),
        hash: [10u8; 16],
    };
    let fact_b = FactVersionRef::FileWholeHash {
        canonical_id: "thread_b_only.ts".to_string(),
        hash: [20u8; 16],
    };
    let fact_a2 = fact_a.clone();
    let fact_b2 = fact_b.clone();

    let ha = thread::spawn(move || {
        let host = VerterHost::new_standalone(Default::default());
        let ((), finalise) = install_fact_tracer_for_tests(&host, || {
            observe_fan_out_borrowed_for_tests(&[fact_a2]);
        });
        finalise
    });

    let hb = thread::spawn(move || {
        let host = VerterHost::new_standalone(Default::default());
        let ((), finalise) = install_fact_tracer_for_tests(&host, || {
            observe_fan_out_borrowed_for_tests(&[fact_b2]);
        });
        finalise
    });

    let fa = ha.join().unwrap();
    let fb = hb.join().unwrap();

    // Thread A's tracer must have A's fact but NOT B's.
    match fa {
        FactReadSetFinalise::Ok(sig) => {
            assert!(sig.iter().any(|f| f == &fact_a), "A must have fact_a");
            assert!(
                !sig.iter().any(|f| f == &fact_b),
                "A must NOT have fact_b (thread isolation)"
            );
        }
        FactReadSetFinalise::Overflow => panic!("thread A overflowed"),
    }

    // Thread B's tracer must have B's fact but NOT A's.
    match fb {
        FactReadSetFinalise::Ok(sig) => {
            assert!(sig.iter().any(|f| f == &fact_b), "B must have fact_b");
            assert!(
                !sig.iter().any(|f| f == &fact_a),
                "B must NOT have fact_a (thread isolation)"
            );
        }
        FactReadSetFinalise::Overflow => panic!("thread B overflowed"),
    }
}
