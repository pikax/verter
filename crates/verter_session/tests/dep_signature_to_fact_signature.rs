//! RED test: `dep_signature_to_fact_signature` maps `DepVersion::WholeHash` →
//! `FactVersionRef::FileWholeHash` and silently drops other variants.

use std::sync::Arc;

use verter_session::for_tests::dep_signature_to_fact_signature_for_tests;
use verter_session::resolver_core::FactVersionRef;
use verter_session::semantic_query::{DepSignature, DepVersion};

fn make_dep_sig(entries: Vec<(&str, DepVersion)>) -> DepSignature {
    Arc::from(
        entries
            .into_iter()
            .map(|(canon, ver)| (Arc::from(canon), ver))
            .collect::<Vec<_>>(),
    )
}

#[test]
#[ignore = "block-0 RED — closed by same-block implementation"]
fn whole_hash_converts_to_file_whole_hash() {
    let hash = [7u8; 16];
    let sig = make_dep_sig(vec![("src/foo.ts", DepVersion::WholeHash(hash))]);

    let result = dep_signature_to_fact_signature_for_tests(&sig);

    assert_eq!(
        result.len(),
        1,
        "one WholeHash entry must produce one FactVersionRef"
    );
    assert_eq!(
        result[0],
        FactVersionRef::FileWholeHash {
            canonical_id: "src/foo.ts".to_string(),
            hash,
        },
        "converted fact must carry the canonical_id and hash from the DepSignature entry"
    );
}

#[test]
#[ignore = "block-0 RED — closed by same-block implementation"]
fn route_generation_is_dropped() {
    let sig = make_dep_sig(vec![
        ("a.ts", DepVersion::RouteGeneration(42)),
        ("b.ts", DepVersion::WholeHash([1u8; 16])),
    ]);

    let result = dep_signature_to_fact_signature_for_tests(&sig);

    assert_eq!(
        result.len(),
        1,
        "RouteGeneration must be dropped; only WholeHash survives"
    );
    match &result[0] {
        FactVersionRef::FileWholeHash { canonical_id, .. } => {
            assert_eq!(
                canonical_id, "b.ts",
                "must keep the WholeHash entry for b.ts"
            );
        }
        other => panic!("unexpected variant: {other:?}"),
    }
}

#[test]
#[ignore = "block-0 RED — closed by same-block implementation"]
fn project_generation_is_dropped() {
    let sig = make_dep_sig(vec![("x.ts", DepVersion::ProjectGeneration(99))]);

    let result = dep_signature_to_fact_signature_for_tests(&sig);

    assert!(
        result.is_empty(),
        "ProjectGeneration must be dropped; result must be empty"
    );
}

#[test]
#[ignore = "block-0 RED — closed by same-block implementation"]
fn empty_dep_sig_produces_empty_result() {
    let sig: DepSignature = Arc::from(vec![]);
    let result = dep_signature_to_fact_signature_for_tests(&sig);
    assert!(
        result.is_empty(),
        "empty DepSignature must produce empty Vec"
    );
}

#[test]
#[ignore = "block-0 RED — closed by same-block implementation"]
fn multiple_whole_hashes_all_convert() {
    let entries: Vec<(&str, DepVersion)> = (0u8..5)
        .map(|i| {
            let canon: &'static str = Box::leak(format!("file_{i}.ts").into_boxed_str());
            (canon, DepVersion::WholeHash([i; 16]))
        })
        .collect();

    let sig = make_dep_sig(entries);
    let result = dep_signature_to_fact_signature_for_tests(&sig);

    assert_eq!(result.len(), 5, "all 5 WholeHash entries must convert");
    for (i, fact) in result.iter().enumerate() {
        match fact {
            FactVersionRef::FileWholeHash { canonical_id, hash } => {
                assert!(canonical_id.starts_with("file_"), "canonical_id must match");
                assert_eq!(
                    hash[0], i as u8,
                    "hash must match DepVersion::WholeHash value"
                );
            }
            other => panic!("unexpected variant at index {i}: {other:?}"),
        }
    }
}
