//! Block 1.H RED test — when `install_fact_tracer` observes more
//! than `FACT_SIGNATURE_CAP` (1024) distinct facts, the producer
//! refuses to admit the entry to cache.
//!
//! Pre-Block-1.H the Family B/C/D caches had no
//! `install_fact_tracer` wrapping. Overflow at the tracer
//! boundary was unobservable. Post-Block-1.H the producer
//! advances the `<cache>_overflow_refusals` counter and refuses
//! cache admission so the next request cold-recomputes.
//!
//! Discrimination: overflowing the tracer inside an outer scope
//! that exceeds the cap advances `SIGNATURE_OVERFLOW_AT_INSTALL`
//! and the outer install returns `FactReadSetFinalise::Overflow`.

use std::sync::Mutex;

use verter_session::for_tests::{
    install_fact_tracer_for_tests, observe_fan_out_borrowed_for_tests,
    read_signature_overflow_at_install,
};
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef};
use verter_session::VerterHost;

// Serialize this test against other process-global SIGNATURE_OVERFLOW_AT_INSTALL
// observers so the counter delta isolation is robust under `--test-threads > 1`.
static MUTEX: Mutex<()> = Mutex::new(());

fn sig_fact(n: u32) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: format!("overflow_fixture_{n}.ts"),
        hash: [n as u8; 16],
    }
}

#[test]
fn overflow_at_install_refuses_admission_and_advances_telemetry() {
    let _serial = MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let host = VerterHost::new_standalone(Default::default());

    let overflow_before = read_signature_overflow_at_install();

    // Observe more than FACT_SIGNATURE_CAP (1024) distinct facts
    // inside one tracer scope. The tracer's `finalise` returns
    // `Overflow` and the SIGNATURE_OVERFLOW_AT_INSTALL counter
    // advances.
    let ((), finalise) = install_fact_tracer_for_tests(&host, || {
        // 1100 > 1024 (FACT_SIGNATURE_CAP) — guarantees overflow.
        for n in 0..1100u32 {
            observe_fan_out_borrowed_for_tests(std::slice::from_ref(&sig_fact(n)));
        }
    });

    let overflow_after = read_signature_overflow_at_install();

    match finalise {
        FactReadSetFinalise::Overflow => {
            // expected
        }
        FactReadSetFinalise::Ok(sig) => {
            panic!(
                "expected FactReadSetFinalise::Overflow after 1100 fact observations; got Ok with sig.len() = {}",
                sig.len()
            );
        }
    }
    assert!(
        overflow_after > overflow_before,
        "SIGNATURE_OVERFLOW_AT_INSTALL must advance on overflow. before={overflow_before}, after={overflow_after}"
    );
}
