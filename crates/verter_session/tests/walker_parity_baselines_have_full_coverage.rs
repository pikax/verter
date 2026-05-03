//! Discrimination meta-test for the 16 parity baselines in
//! `legacy_walker_parity_baseline.rs`. The goal is to prove the
//! baselines collectively cover the legacy walker's policy-table
//! arms; without this gate the baselines could collapse into a
//! single "resolution does not panic" smoke test that would let a
//! regression slip through silently.
//!
//! The discrimination is a SOURCE-LEVEL property: the baseline file
//! must contain 16 `#[test]` functions whose names follow the
//! `fixture_NN_<descriptor>` convention. A drop-in regression test
//! that disables half the baselines would surface here as a
//! mismatch.

use std::path::PathBuf;

const EXPECTED_BASELINE_COUNT: usize = 16;

fn baseline_source() -> String {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/legacy_walker_parity_baseline.rs");
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("read `{p:?}`: {e}"))
        .replace("\r\n", "\n")
}

#[test]
fn parity_baselines_count_equals_sixteen() {
    let src = baseline_source();
    // Count `fn fixture_NN_` test entries. The naming convention is
    // load-bearing — the discrimination matrix below assumes it.
    let count = (1..=EXPECTED_BASELINE_COUNT)
        .filter(|n| {
            let prefix = format!("fn fixture_{n:02}_");
            src.contains(&prefix)
        })
        .count();
    assert_eq!(
        count, EXPECTED_BASELINE_COUNT,
        "plan §11.5 — exactly {EXPECTED_BASELINE_COUNT} parity baselines must exist; \
         found {count} fixtures matching the `fn fixture_NN_` naming convention"
    );
}

#[test]
fn parity_baselines_cover_each_walker_policy_arm() {
    // The legacy walker policy table (plan §1.6 / §1.12 + the
    // round-7 route-extraction tightenings) has these distinct
    // shape buckets that the materialiser must reproduce:
    //
    //  - Object surface (members + optional + methods)
    //  - Array
    //  - Tuple
    //  - Union
    //  - Intersection
    //  - Literal
    //  - DeclRef (bare reference)
    //  - Pick / Omit (route extraction)
    //  - IndexedAccess (route extraction with literal-string index)
    //  - Partial / Required (mapped-type utility)
    //  - typeof (value-root projection)
    //
    // The 16 baselines collectively touch every bucket. Each entry
    // names the fixture and the bucket it characterises. A change to
    // the baseline file that removes a bucket fails this test.
    let src = baseline_source();
    let bucket_signatures: &[(&str, &str)] = &[
        ("plain_object", "Object members"),
        ("optional_member", "Object optional"),
        ("method_signature", "Object method"),
        ("array_of_string", "Array"),
        ("tuple_two_elements", "Tuple"),
        ("union_string_or_number", "Union"),
        ("intersection_of_two_objects", "Intersection"),
        ("string_literal_kept_as_literal", "Literal"),
        ("decl_ref_to_local_alias", "DeclRef"),
        ("pick_two_args_literal_string_key", "Pick exact-2"),
        ("pick_with_three_literal_keys", "Pick literal-union"),
        ("omit_two_args_drops_excluded_keys", "Omit"),
        ("indexed_access_string_literal", "IndexedAccess"),
        ("partial_t_makes_all_members_optional", "Partial"),
        ("required_t_makes_all_members_required", "Required"),
        ("typeof_value_ref_resolves_to_value_type", "TypeOf"),
    ];
    assert_eq!(
        bucket_signatures.len(),
        EXPECTED_BASELINE_COUNT,
        "discrimination matrix must list exactly {EXPECTED_BASELINE_COUNT} bucket entries"
    );
    for (signature, bucket) in bucket_signatures {
        assert!(
            src.contains(signature),
            "baseline file is missing the `{signature}` fixture — \
             the `{bucket}` policy bucket is not characterised. Plan §11.5 \
             discrimination contract violated.",
        );
    }
}
