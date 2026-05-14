//! RED test: exceeding FACT_SIGNATURE_CAP causes `install_fact_tracer` to emit
//! `StructuredAuditEvent::FactSignatureOverflow` and increment
//! `SIGNATURE_OVERFLOW_AT_INSTALL`.

use std::sync::Mutex;

use verter_session::for_tests::{
    install_fact_tracer_for_tests, observe_fan_out_borrowed_for_tests,
    read_signature_overflow_at_install,
};
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef, FACT_SIGNATURE_CAP};
use verter_session::VerterHost;

// Serializes the two tests that trigger overflow against the process-global
// `SIGNATURE_OVERFLOW_AT_INSTALL` counter. Without this guard, parallel test
// scheduling lets both overflows fire concurrently and `overflow_increments_counter`
// observes a delta > 1.
static OVERFLOW_TEST_MUTEX: Mutex<()> = Mutex::new(());

fn make_host() -> VerterHost {
    VerterHost::new_standalone(Default::default())
}

/// Generate FACT_SIGNATURE_CAP + 1 distinct facts so finalise returns Overflow.
fn overflow_facts() -> Vec<FactVersionRef> {
    (0u32..=(FACT_SIGNATURE_CAP as u32))
        .map(|i| {
            let mut hash = [0u8; 16];
            hash[0] = (i & 0xFF) as u8;
            hash[1] = ((i >> 8) & 0xFF) as u8;
            hash[2] = ((i >> 16) & 0xFF) as u8;
            FactVersionRef::FileWholeHash {
                canonical_id: format!("overflow_fact_{i}.ts"),
                hash,
            }
        })
        .collect()
}

#[test]
fn install_fact_tracer_returns_overflow_when_cap_exceeded() {
    let _serial = OVERFLOW_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let host = make_host();
    let facts = overflow_facts();

    let (_value, finalise) = install_fact_tracer_for_tests(&host, || {
        observe_fan_out_borrowed_for_tests(&facts);
    });

    assert!(
        matches!(finalise, FactReadSetFinalise::Overflow),
        "must return Overflow when observations exceed FACT_SIGNATURE_CAP ({FACT_SIGNATURE_CAP})"
    );
}

#[test]
fn overflow_increments_counter() {
    let _serial = OVERFLOW_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let host = make_host();
    let facts = overflow_facts();

    let before = read_signature_overflow_at_install();

    let (_value, finalise) = install_fact_tracer_for_tests(&host, || {
        observe_fan_out_borrowed_for_tests(&facts);
    });

    let after = read_signature_overflow_at_install();

    assert!(
        matches!(finalise, FactReadSetFinalise::Overflow),
        "must return Overflow"
    );
    assert_eq!(
        after,
        before + 1,
        "SIGNATURE_OVERFLOW_AT_INSTALL must increment by 1 on overflow; before={before}, after={after}"
    );
}

#[test]
fn install_fact_tracer_ok_when_exactly_at_cap() {
    let host = make_host();

    // Exactly FACT_SIGNATURE_CAP unique facts — must succeed (Ok, not Overflow).
    let facts: Vec<FactVersionRef> = (0u32..FACT_SIGNATURE_CAP as u32)
        .map(|i| {
            let mut hash = [0u8; 16];
            hash[0] = (i & 0xFF) as u8;
            hash[1] = ((i >> 8) & 0xFF) as u8;
            hash[2] = ((i >> 16) & 0xFF) as u8;
            FactVersionRef::FileWholeHash {
                canonical_id: format!("at_cap_fact_{i}.ts"),
                hash,
            }
        })
        .collect();

    let (_value, finalise) = install_fact_tracer_for_tests(&host, || {
        observe_fan_out_borrowed_for_tests(&facts);
    });

    assert!(
        matches!(finalise, FactReadSetFinalise::Ok(_)),
        "must return Ok when exactly at FACT_SIGNATURE_CAP ({FACT_SIGNATURE_CAP})"
    );
}
