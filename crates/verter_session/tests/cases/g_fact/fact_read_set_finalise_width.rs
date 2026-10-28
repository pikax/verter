//! A host tracer scope records and seals EVERY fact a compute observed,
//! however many: below, at and above a wide signature's width the finalised
//! signature is one canonical set holding exactly the read set, in canonical
//! order, and is never refused for its width.

use verter_session::for_tests::{
    install_fact_tracer_for_tests, observe_fan_out_borrowed_for_tests,
};
use verter_session::VerterHost;
use verter_session_query::facts::{
    fact_cache::{FactVersionRef, ReadSetSignature},
    fact_read_set::FactReadSetFinalise,
};

/// The production fresh-slice width these fixtures straddle.
const WIDE_SIGNATURE_WIDTH: usize =
    verter_session_query::facts::policy::FactPolicy::production().fresh_slice_max();

fn make_host() -> VerterHost {
    VerterHost::new_standalone(Default::default())
}

/// `count` distinct whole-hash facts.
fn distinct_facts(count: usize) -> Vec<FactVersionRef> {
    (0..count)
        .map(|i| {
            let mut hash = [0u8; 16];
            hash[0] = (i & 0xFF) as u8;
            hash[1] = ((i >> 8) & 0xFF) as u8;
            hash[2] = ((i >> 16) & 0xFF) as u8;
            FactVersionRef::FileWholeHash {
                canonical_id: format!("wide_fact_{i}.ts").into(),
                hash,
            }
        })
        .collect()
}

#[test]
fn install_fact_tracer_seals_every_fact_around_and_beyond_the_wide_width() {
    let host = make_host();
    for width in [
        WIDE_SIGNATURE_WIDTH - 1,
        WIDE_SIGNATURE_WIDTH,
        WIDE_SIGNATURE_WIDTH + 1,
        4 * WIDE_SIGNATURE_WIDTH + 7,
    ] {
        let facts = distinct_facts(width);
        let (_value, finalise) = install_fact_tracer_for_tests(&host, || {
            observe_fan_out_borrowed_for_tests(&facts);
        });
        let FactReadSetFinalise::Ok(sealed) = finalise else {
            panic!("{width}: a wide observation set seals into its signature, got {finalise:?}");
        };
        let signature = ReadSetSignature::new(sealed);
        assert_eq!(
            signature.facts.len(),
            width,
            "{width}: one fact per observation"
        );
        let entries: Vec<FactVersionRef> = signature.facts.iter().cloned().collect();
        let mut expected = facts.clone();
        expected.sort();
        assert_eq!(
            entries, expected,
            "{width}: every observed fact survives, in canonical order"
        );
    }
}
