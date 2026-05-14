//! RED test: a single `observe_fan_out` call reaches all 3 nested tracer levels.
//!
//! When the stack has 3 active tracer scopes and one observation is fanned out,
//! every level's FactReadSet must contain that fact. This verifies the
//! snapshot-then-iterate fan-out pattern (ACTIVE_TRACERS is snapshotted, borrow
//! dropped, then each cell is visited) rather than a single-pointer assignment.

use verter_session::for_tests::{
    install_fact_tracer_for_tests, observe_fan_out_borrowed_for_tests,
};
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef};
use verter_session::VerterHost;

fn make_host() -> VerterHost {
    VerterHost::new_standalone(Default::default())
}

fn shared_fact() -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: "shared_fanout.ts".to_string(),
        hash: [42u8; 16],
    }
}

#[test]
#[ignore = "block-0 RED — closed by same-block implementation"]
fn fan_out_reaches_all_three_levels() {
    let host = make_host();
    let fact = shared_fact();

    // Level 1 (outermost)
    let (l2_result, l1_finalise) = install_fact_tracer_for_tests(&host, || {
        // Level 2
        let (l3_result, l2_finalise) = install_fact_tracer_for_tests(&host, || {
            // Level 3 (innermost)
            let ((), l3_finalise) = install_fact_tracer_for_tests(&host, || {
                // Fan-out from level 3 — must reach L3, L2, and L1 cells.
                observe_fan_out_borrowed_for_tests(&[fact.clone()]);
            });
            l3_finalise
        });
        (l3_result, l2_finalise)
    });

    // Every level must contain the shared fact.
    let (l3_finalise, l2_finalise) = l2_result;

    for (name, finalise) in [
        ("L1", l1_finalise),
        ("L2", l2_finalise),
        ("L3", l3_finalise),
    ] {
        match finalise {
            FactReadSetFinalise::Ok(sig) => {
                assert!(
                    sig.iter().any(|f| f == &fact),
                    "{name} tracer must contain the fanned-out fact; got {sig:?}"
                );
            }
            FactReadSetFinalise::Overflow => panic!("{name} scope overflowed unexpectedly"),
        }
    }
}
