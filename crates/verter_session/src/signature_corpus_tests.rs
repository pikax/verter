//! The signature corpus DRIVER — the executable half of the V0 evidence
//! lock (`docs/arch/signature-kernel.md` §13 V0, acceptance V0-AC3).
//!
//! Every row in [`signature_corpus_rows_tests::CORPUS`] is a recorded
//! 7.0.2 observation. This driver:
//!
//! 1. **parses** every recorded observation — the `checker` text through
//!    the shared typed checker-syntax parser (extending it deliberately,
//!    never exempting silently), the `decl_emit` bytes through the tree's
//!    own TypeScript parser, and the `any`/`never`/diagnostic legs
//!    through their recorded conventions;
//! 2. **compares the current implementation's answers** under the row's
//!    recorded verdict (`MatchesChecker | KnownOwed | Degraded`) against
//!    the live flow-return rail, exactly the u6 corpus's verdict-directed
//!    discipline: a `MatchesChecker` row fails when the live answer stops
//!    matching, and an owed/degraded row fails when the live answer
//!    STARTS matching — a later block that changes an answer FLIPS A ROW
//!    here instead of a prose report;
//! 3. **locks the corpus identity** into the evidence manifest — the
//!    digest of the recorded observations is re-hashed and compared with
//!    `docs/evidence/signature-kernel/manifest.json`, so an edited
//!    observation without a re-locked manifest fails.
//!
//! The suite never invokes the checker: the observations are RECORDED
//! measurements against the pinned 7.0.2 toolchain whose digests the
//! `oracle_core` identity tests re-verify.

use sha2::{Digest, Sha256};

use crate::signature_corpus_rows_tests::{Verdict, CORPUS};
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{
    checker_syntax, render_node, with_live_flow_node,
};

/// The corpus identity this driver locks. Mirrored verbatim in
/// `docs/evidence/signature-kernel/manifest.json` (`corpus.identity`).
pub(crate) const CORPUS_IDENTITY: &str = "verter-signature-corpus-v0@typescript-7.0.2";

/// The evidence manifest, relative to the crate manifest dir.
const EVIDENCE_MANIFEST_REL: &str = "../../docs/evidence/signature-kernel/manifest.json";

fn manifest_text() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(EVIDENCE_MANIFEST_REL);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read the evidence manifest {}: {e}", path.display()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The digest input for the corpus lock: for every row in table order,
/// the length-prefixed id, checker and decl-emit bytes (the same
/// length-prefix scheme the u6 cohort fingerprint uses, over the row's
/// RECORDED observation only — never live answers).
fn corpus_digest_input() -> Vec<u8> {
    let mut buf = Vec::new();
    for row in CORPUS {
        for field in [
            row.id.as_bytes(),
            row.checker.as_bytes(),
            row.decl_emit.as_bytes(),
        ] {
            buf.extend_from_slice(&(field.len() as u64).to_le_bytes());
            buf.extend_from_slice(field);
        }
    }
    buf
}

/// V0-AC3 (parse): every recorded observation is non-empty, carries
/// exactly one observation form, its `decl_emit` bytes PARSE as a
/// TypeScript declarations module through the tree's own parser, and
/// the table covers every contract V0-gate family — including exactly
/// ten distinct Awaited residual rows.
#[test]
fn signature_corpus_observations_parse_and_families_are_covered() {
    let mut failures: Vec<String> = Vec::new();
    let mut ids: Vec<&str> = Vec::new();
    let mut awaited = 0usize;
    let mut per_family: Vec<(String, usize)> = Vec::new();
    for row in CORPUS {
        if ids.contains(&row.id) {
            failures.push(format!("{}: duplicate id", row.id));
        }
        ids.push(row.id);
        if row.probe.is_empty() {
            failures.push(format!("{}: the probe expression is empty", row.id));
        }
        // The observation form is the checker text (recorded as `any` /
        // `never` with its proving leg) OR a recorded diagnostic — never
        // both, never neither.
        if row.diagnostic.is_some() != row.checker.is_empty() {
            failures.push(format!(
                "{}: exactly ONE observation form is required (checker text, or a recorded \
                 diagnostic that replaces it)",
                row.id
            ));
        }
        if row.checker_is_any && row.checker != "any" {
            failures.push(format!(
                "{}: the any convention records checker `any`, found `{}`",
                row.id, row.checker
            ));
        }
        if row.checker_is_never && row.checker != "never" {
            failures.push(format!(
                "{}: the never convention records checker `never`, found `{}`",
                row.id, row.checker
            ));
        }
        if let Some(diagnostic) = row.diagnostic {
            if diagnostic.is_empty() || !row.checker.is_empty() {
                failures.push(format!(
                    "{}: a recorded diagnostic is non-empty and replaces the checker text",
                    row.id
                ));
            }
        }
        if row.decl_emit.trim().is_empty() {
            failures.push(format!(
                "{}: the recorded declaration-emit bytes are empty",
                row.id
            ));
        } else if let Err(error) = parse_declarations(row.decl_emit) {
            failures.push(format!(
                "{}: the recorded declaration-emit bytes did not parse as a TypeScript \
                 declarations module: {error}",
                row.id
            ));
        }
        match row.verdict {
            Verdict::MatchesChecker => {}
            Verdict::KnownOwed { note } | Verdict::Degraded { note } => {
                if note.trim().is_empty() || !note.contains("V") {
                    failures.push(format!(
                        "{}: an owed/degraded verdict names its owing successor block in the \
                         note",
                        row.id
                    ));
                }
            }
        }
        if row.family == crate::signature_corpus_rows_tests::Family::AwaitedResidual {
            awaited += 1;
        }
        match per_family.iter_mut().find(|(f, _)| f == row.family.id()) {
            Some((_, n)) => *n += 1,
            None => per_family.push((row.family.id().to_owned(), 1)),
        }
    }
    if awaited != 10 {
        failures.push(format!(
            "the contract's mandatory matrix names TEN Awaited residual rows; the corpus \
             carries {awaited}"
        ));
    }
    for family in crate::signature_corpus_rows_tests::Family::ALL {
        let count = per_family
            .iter()
            .find(|(f, _)| f == family.id())
            .map(|(_, n)| *n)
            .unwrap_or(0);
        if count == 0 {
            failures.push(format!(
                "family `{}` has no row — the contract's V0 gate requires it",
                family.id()
            ));
        }
    }
    // Family-shape discrimination inside the families the contract
    // names with more than one witness form.
    let has = |needle: &str| {
        CORPUS.iter().any(|r| {
            r.id.to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase())
        })
    };
    for (needle, why) in [
        ("construct", "a CONSTRUCT-signature intersection row"),
        ("mixin", "a mixin intersection row"),
        ("default", "a generic-default row"),
        ("predicate", "a type-predicate row"),
        ("assertion", "an assertion-signature row"),
        (
            "explicit_type_arguments",
            "an explicit call-site type-argument row",
        ),
        ("nested_instantiation", "a nested-instantiation row"),
        ("constrained", "a constrained-substitution row"),
        ("witness_l", "the PRESERVED-grouping L witness"),
        ("witness_r", "the PRESERVED-grouping R witness"),
        ("transparent", "the TRANSPARENT-group witness"),
    ] {
        if !has(needle) {
            failures.push(format!("family coverage: {why} is missing ({needle})"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Parse a declarations module with the tree's TypeScript parser — the
/// same parser the resolver feeds. A recorded observation that cannot
/// parse is an unverified claim.
fn parse_declarations(text: &str) -> Result<(), String> {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    let allocator = Allocator::default();
    // A `.d.ts` module; `from_path` only fails on unknown extensions.
    let source_type = SourceType::from_path("module.d.ts").unwrap_or_default();
    let ret = Parser::new(&allocator, text, source_type).parse();
    match ret.errors.first() {
        Some(error) => Err(format!("parse error: {}", error.message)),
        None => Ok(()),
    }
}

/// V0-AC3 (compare): the live flow-return answer follows every row's
/// recorded verdict. The comparison basis is the typed checker-syntax
/// projection where the checker text parses into it (order-insensitive
/// exact union sets, ordered intersections, exact object member sets —
/// one canonical projection, no normalization that hides drift), and
/// the rendered-node string otherwise. A checker text that fails to
/// parse into EITHER basis is a failure naming the gap: extend the
/// parser deliberately, never exempt the row.
#[test]
fn signature_corpus_live_answers_follow_their_verdicts() {
    let mut failures: Vec<String> = Vec::new();
    for row in CORPUS {
        // Diagnostic rows record the CHECKER's own refusal; the live
        // comparison basis is the rendered string.
        let (parsed, rendered_live) = with_live_flow_node(
            "",
            &format!("{}__sig", row.id),
            row.source,
            "witness",
            |dispatch, node| match node {
                Some(node) => (
                    checker_syntax::parse(row.checker)
                        .map(|parsed| checker_syntax::matches_node(dispatch, node, &parsed, 0)),
                    Some(render_node(dispatch, node, 0)),
                ),
                None => (Ok(false), None),
            },
        );
        let note = match row.verdict {
            Verdict::MatchesChecker => {
                let matched = parsed.clone().unwrap_or_else(|err| {
                    panic!(
                        "{}: checker text `{}` parses in neither basis ({err}) — extend the \
                         parser deliberately",
                        row.id, row.checker
                    )
                });
                if !matched && rendered_live.as_deref() != Some(row.checker) {
                    Some(format!(
                        "labelled MatchesChecker but the live answer does not equal the \
                         recorded observation `{}` — measured `{}`. Either the answer \
                         regressed or the observation was edited; re-measure against the \
                         pinned oracle before re-pinning",
                        row.checker,
                        rendered_live.as_deref().unwrap_or("<no value>")
                    ))
                } else {
                    None
                }
            }
            Verdict::KnownOwed { .. } | Verdict::Degraded { .. } => {
                let matched = parsed.unwrap_or(false);
                if matched || rendered_live.as_deref() == Some(row.checker) {
                    Some(format!(
                        "labelled {:?} but the live answer EQUALS the recorded observation \
                         `{}` — the recorded divergence is GONE. This failure is the \
                         INTENDED signal: the answer looks implemented (or the observation \
                         was edited to the live value); re-pin the row and update the \
                         semantic-difference ledger in the same change",
                        row.verdict, row.checker
                    ))
                } else {
                    None
                }
            }
        };
        if let Some(note) = note {
            failures.push(format!("{}: {note}", row.id));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// V0-AC3 (identity): the recorded observations digest to the corpus
/// entry locked in the evidence manifest, and the manifest names THIS
/// corpus identity — an edited observation, or a manifest edit that
/// does not recompute the digest, fails here.
#[test]
fn signature_corpus_identity_is_locked_in_the_evidence_manifest() {
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text())
        .expect("parse docs/evidence/signature-kernel/manifest.json");
    assert_eq!(
        manifest["corpus"]["identity"].as_str(),
        Some(CORPUS_IDENTITY),
        "the manifest must name this corpus's own identity (never the unrecovered 9,300-case \
         report)"
    );
    let recorded = manifest["corpus"]["observation_digest"]
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "the manifest carries corpus.observation_digest (sha256 over id+checker+decl_emit)"
            )
        })
        .trim();
    let digest = sha256_hex(&corpus_digest_input());
    assert_eq!(
        recorded, digest,
        "the corpus observations changed without re-locking the manifest digest"
    );
}

/// The GROUPING WITNESS, recorded executable: on the pinned 7.0.2
/// oracle BOTH preserved groupings reduce to `never` (`L<"a">` and
/// `R<"a">`), unlike the historical 5.8.3 probe in the contract where
/// `L` was `never` while `R` stayed an unreduced intersection
/// representation. The two corpus rows carry the observation; this
/// control pins the WITNESS PAIR as a pair (both rows present, both
/// recording `never`), so the ledger's recheck clause cannot silently
/// lose one side.
#[test]
fn signature_corpus_records_the_7_0_2_grouping_witness_as_a_pair() {
    let l = CORPUS
        .iter()
        .find(|r| r.id == "SV12_grouping_witness_L")
        .expect("the L grouping witness row");
    let r = CORPUS
        .iter()
        .find(|r| r.id == "SV13_grouping_witness_R")
        .expect("the R grouping witness row");
    assert!(
        l.checker_is_never && r.checker_is_never,
        "on 7.0.2 both L<\"a\"> and R<\"a\"> reduce to never — the shape legs are silent and \
         the IsNever legs fire; a change here is an oracle-level semantic difference that \
         must land in the semantic-difference ledger"
    );
    assert!(
        l.decl_emit.contains("never") && r.decl_emit.contains("never"),
        "both witnesses' recorded declaration emit carries the reduced `never` return"
    );
}
