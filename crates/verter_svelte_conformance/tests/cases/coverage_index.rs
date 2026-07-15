//! Validation of the committed `corpus/coverage-index.json` review artifact
//! against the typed in-process manifest (the sole authority).
//!
//! The index is DERIVED — never an authority — so this gate proves the
//! committed derivation still agrees with the manifest: schema shape (closed,
//! `deny_unknown_fields` — a missing OR extra field fails), manifest
//! hash/version identity, the exact per-case inventory, and a NON-VACUOUS
//! coverage proof (selected-case count, full candidate product, per-partition
//! full-space counts, the strengthened interaction groups, and the presence
//! of the refused + oracle-rejected partitions).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use verter_svelte_conformance::manifest::{manifest, SCHEMA_VERSION};
use verter_svelte_conformance::model::{CompileTarget, Disposition, MatchOutcome};

/// The pinned selected-case count (mirrors the committed corpus) — the
/// shared test-side pin, so a manifest resize moves every conformance gate
/// in lockstep.
use crate::common::case_count;
use case_count::CASE_COUNT;

// ---------------------------------------------------------------------------
// The CLOSED index schema. Every struct denies unknown fields and every field
// is mandatory (no Option / no default), so a dropped or added field is a
// schema violation, not a silent skip.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Index {
    schema_version: u32,
    manifest_hash: String,
    cases: Vec<IndexCase>,
    proof: Proof,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct IndexCase {
    slug: String,
    disposition: String,
    expected_outcome: String,
    backends: [String; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Proof {
    selected_cases: u64,
    full_product: u64,
    compression: String,
    partitions: Partitions,
    groups: Vec<Group>,
    covering_proof: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Partitions {
    supported: u64,
    refused: BTreeMap<String, u64>,
    oracle_rejected: BTreeMap<String, u64>,
    invalid: BTreeMap<String, u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Group {
    factors: Vec<String>,
    strength: u8,
}

// ---------------------------------------------------------------------------
// Wire vocabulary (pinned here independently of the writer, so a silent
// writer-side vocabulary change fails this gate).
// ---------------------------------------------------------------------------

/// The index wire spelling of a typed disposition.
fn disposition_wire(disposition: Disposition) -> String {
    match disposition {
        Disposition::Supported => "supported".to_string(),
        Disposition::Refused(kind) => format!("refused:{}", kind.id()),
        Disposition::OracleRejected(kind) => format!("oracle-rejected:{}", kind.id()),
        Disposition::Invalid(kind) => {
            unreachable!("Invalid({kind:?}) rows are never manifest cases")
        }
    }
}

/// The index wire spelling of a declared outcome.
fn outcome_wire(outcome: MatchOutcome) -> &'static str {
    match outcome {
        MatchOutcome::Match => "match",
        MatchOutcome::NoMatch => "no-match",
        MatchOutcome::Maybe => "maybe",
    }
}

/// The pinned wire factor names of the two strengthened interaction groups,
/// in group order (part of the index schema vocabulary).
const EXPECTED_GROUPS: [(&[&str], u8); 2] = [
    (
        &[
            "template-value",
            "target",
            "quoting",
            "element-region",
            "match-outcome",
        ],
        5,
    ),
    (
        &[
            "selector-kind",
            "selector-value",
            "structural-kind",
            "match-outcome",
        ],
        4,
    ),
];

#[test]
fn committed_coverage_index_matches_the_manifest() {
    let manifest = manifest();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("corpus")
        .join("coverage-index.json");
    let text = std::fs::read_to_string(&path).expect("committed coverage-index.json reads");
    let index: Index = serde_json::from_str(&text)
        .expect("coverage-index.json matches the closed index schema exactly");

    // Version + hash identity with the in-process manifest.
    assert_eq!(index.schema_version, SCHEMA_VERSION, "schemaVersion");
    assert_eq!(
        index.manifest_hash,
        manifest.manifest_hash(),
        "manifestHash"
    );

    // Exact case inventory: same count, slug-ascending order, unique slugs,
    // and per-case disposition / outcome / backends equal to the typed cases.
    assert_eq!(index.cases.len(), manifest.cases().len(), "case count");
    assert_eq!(index.cases.len(), CASE_COUNT, "pinned case count");
    assert!(
        index
            .cases
            .windows(2)
            .all(|pair| pair[0].slug < pair[1].slug),
        "index cases must be strictly ascending by slug (deterministic, duplicate-free)"
    );
    let mut refused_cases = 0usize;
    let mut oracle_rejected_cases = 0usize;
    for indexed in &index.cases {
        let case = manifest
            .case_for_slug(&indexed.slug)
            .unwrap_or_else(|| panic!("index case {} is not a manifest case", indexed.slug));
        assert_eq!(
            indexed.disposition,
            disposition_wire(case.disposition),
            "disposition of {}",
            indexed.slug
        );
        assert_eq!(
            indexed.expected_outcome,
            outcome_wire(case.expected_outcome),
            "expectedOutcome of {}",
            indexed.slug
        );
        assert_eq!(
            [indexed.backends[0].as_str(), indexed.backends[1].as_str()],
            [CompileTarget::Client.id(), CompileTarget::Server.id()],
            "backends of {}",
            indexed.slug
        );
        match case.disposition {
            Disposition::Refused(_) => refused_cases += 1,
            Disposition::OracleRejected(_) => oracle_rejected_cases += 1,
            _ => {}
        }
    }

    // Proof non-vacuity: the selected-case count and the full candidate
    // product must be the manifest's own numbers.
    assert_eq!(
        index.proof.selected_cases, CASE_COUNT as u64,
        "proof.selectedCases"
    );
    let full_product: u64 = manifest
        .spec()
        .cardinalities
        .iter()
        .map(|&cardinality| u64::from(cardinality))
        .product();
    assert_eq!(index.proof.full_product, full_product, "proof.fullProduct");
    assert_eq!(full_product, 272_160, "pinned full candidate product");
    assert_eq!(
        index.proof.compression,
        format!("{CASE_COUNT}/{full_product}"),
        "proof.compression"
    );

    // Per-partition full-space counts: exactly the manifest inventories
    // (every declared kind present as a key).
    assert_eq!(
        index.proof.partitions.supported,
        manifest.supported_row_count(),
        "partitions.supported"
    );
    let expected_refused: BTreeMap<String, u64> = manifest
        .refused_inventory()
        .iter()
        .map(|(kind, count)| (kind.id().to_string(), *count))
        .collect();
    assert_eq!(
        index.proof.partitions.refused, expected_refused,
        "partitions.refused"
    );
    let expected_oracle: BTreeMap<String, u64> = manifest
        .oracle_rejected_inventory()
        .iter()
        .map(|(kind, count)| (kind.id().to_string(), *count))
        .collect();
    assert_eq!(
        index.proof.partitions.oracle_rejected, expected_oracle,
        "partitions.oracleRejected"
    );
    let expected_invalid: BTreeMap<String, u64> = manifest
        .invalid_inventory()
        .iter()
        .map(|(kind, count)| (kind.id().to_string(), *count))
        .collect();
    assert_eq!(
        index.proof.partitions.invalid, expected_invalid,
        "partitions.invalid"
    );

    // The refusal partition is UNINHABITED (`RefusalKind` has no variants):
    // the committed index must carry NO refused rows at either level.
    assert!(
        index.proof.partitions.refused.is_empty(),
        "the refused partition must stay uninhabited: {:?}",
        index.proof.partitions.refused
    );
    assert!(
        !index.proof.partitions.oracle_rejected.is_empty()
            && index
                .proof
                .partitions
                .oracle_rejected
                .values()
                .all(|&count| count > 0),
        "the oracle-rejected partition must be present and non-empty"
    );
    assert_eq!(
        refused_cases, 0,
        "the uninhabited refusal partition selects no cases"
    );
    assert!(
        oracle_rejected_cases > 0,
        "no oracle-rejected case was selected"
    );

    // The strengthened interaction groups, with their factors + strengths.
    assert_eq!(
        index.proof.groups.len(),
        manifest.spec().interaction_groups.len(),
        "group count"
    );
    assert_eq!(
        index.proof.groups.len(),
        EXPECTED_GROUPS.len(),
        "pinned group count"
    );
    for (position, (group, (expected_factors, expected_strength))) in
        index.proof.groups.iter().zip(EXPECTED_GROUPS).enumerate()
    {
        assert_eq!(
            group.strength, expected_strength,
            "strength of group {position}"
        );
        assert_eq!(
            group.strength,
            manifest.spec().interaction_groups[position].strength,
            "group {position} strength must match the manifest spec"
        );
        assert_eq!(
            group.factors, expected_factors,
            "factors of group {position}"
        );
        assert_eq!(
            group.factors.len(),
            manifest.spec().interaction_groups[position].factors.len(),
            "group {position} factor count must match the manifest spec"
        );
    }

    // The rendered covering proof is the manifest's own, verbatim.
    assert_eq!(
        index.proof.covering_proof,
        manifest.proof().render(),
        "proof.coveringProof"
    );
    assert!(
        !index.proof.covering_proof.is_empty(),
        "coveringProof must be non-empty"
    );
}
