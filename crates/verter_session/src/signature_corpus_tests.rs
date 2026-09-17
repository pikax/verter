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
//! 2. **compares the current implementation's answer TO THE RECORDED
//!    PROBE** under the row's recorded verdict (`MatchesChecker |
//!    KnownOwed | Degraded`): the live observation lane drives the
//!    row's probe in TYPE position (a declared binding annotated with
//!    the probe, read back through the public audited flow-return
//!    boundary — never the witness's own return, which for wrapper
//!    probes like `Awaited<ReturnType<...>>` is a different question).
//!    The comparison bases are STRUCTURAL only — the typed
//!    checker-syntax projection of the recorded `checker` text, the
//!    recorded `decl_emit` signature return where the checker column is
//!    a display-only instantiation, and for diagnostic rows the
//!    recorded refusal pinned by a deferred carrier. A
//!    `MatchesChecker` row fails when the live answer stops matching,
//!    and an owed/degraded row fails when the live answer STARTS
//!    matching — a later block that changes an answer FLIPS A ROW here
//!    instead of a prose report (both directions proven by
//!    `signature_corpus_flip_law_fires_in_both_directions`);
//! 3. **locks the corpus identity** into the evidence manifest — the
//!    digest of the recorded observations is re-hashed and compared with
//!    `docs/evidence/signature-kernel/manifest.json`, so an edited
//!    observation without a re-locked manifest fails.
//!
//! The suite never invokes the checker: the observations are RECORDED
//! measurements against the pinned 7.0.2 toolchain whose digests the
//! `oracle_core` identity tests re-verify.

use sha2::{Digest, Sha256};

use crate::signature_corpus_rows_tests::{Row, Verdict, CORPUS};
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};

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

/// The fixed, versioned PROBE LANE appended to every row's module: the
/// recorded probe in TYPE position — a declared binding whose annotation
/// IS the probe, read back through the same public audited flow-return
/// boundary every other corpus row rides. The lane asks the live rail
/// the RECORDED QUESTION (the probe), never the witness's own return,
/// which for wrapper probes (`Awaited<ReturnType<...>>`) is a different
/// question entirely.
fn probe_lane_source(row: &Row) -> String {
    format!(
        "{}\nexport function __sig_probe_lane() {{ \
            const __probeWitness: {} = null as any; \
            return __probeWitness; \
        }}\n",
        row.source, row.probe
    )
}

/// What the live rail answered for one row's probe.
struct LiveProbeOutcome {
    /// The live answer structurally matches the row's recorded checker
    /// text through the checker-syntax projection (when the row records
    /// one). Structured ONLY — a Verter display string is never
    /// semantic identity.
    matched_checker: bool,
    /// The live answer structurally matches the ORDER claim recorded in
    /// the row's `decl_emit` signature return (the rows whose checker
    /// column is a display-only instantiation — e.g. `unknown` for a
    /// generic binder — while the declaration bytes carry the real
    /// union order).
    matched_declared_return: bool,
    /// The base declaration name when the live answer is the DEFERRED
    /// instantiation carrier (`InstantiationRef(<operator>)` — the
    /// substrate has not reduced the probe's outer operator), else
    /// `None`.
    deferred_operator: Option<std::sync::Arc<str>>,
    /// The rendered live answer (diagnostics only — never a comparison
    /// basis).
    rendered: Option<String>,
    /// The typed degradation on a completed answer.
    degraded: bool,
}

/// Drive one row's probe lane through the public audited boundary and
/// evaluate both structural bases against the live node.
fn live_probe_outcome(row: &Row) -> LiveProbeOutcome {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host;
    let host = make_audit_host();
    let canonical = "/wb/signature_probe.ts";
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        canonical,
        &crate::u6_flow_shape_corpus_tests::module_script(&probe_lane_source(row)),
        crate::FileLanguage::script_ts(),
    );
    let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: std::sync::Arc::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: std::sync::Arc::from("__sig_probe_lane"),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let carrier = host.get_flow_return_type_with_audit(
        &identity,
        crate::semantic_query::ReturnProjectionDemand::whole_return(),
    );
    let Ok(result) = carrier.as_result() else {
        return LiveProbeOutcome {
            matched_checker: false,
            matched_declared_return: false,
            deferred_operator: None,
            rendered: None,
            degraded: false,
        };
    };
    let degraded = result.degradation().is_some();
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(&host_ctx);
    let node = result.return_type();
    let rendered = Some(render_node(&dispatch, node, 0));
    let deferred_operator = match dispatch.graph().node_data(node) {
        Some(data) => match data.as_ref() {
            crate::semantic_query::SemanticNodeData::InstantiationRef { base, .. } => {
                Some(std::sync::Arc::clone(&base.decl_name))
            }
            _ => None,
        },
        None => None,
    };
    let structural_match = |text: &str| {
        let parsed = checker_syntax::parse(text).unwrap_or_else(|err| {
            panic!(
                "{}: recorded text `{}` does not parse ({err}) — extend the checker-syntax \
                 parser deliberately, never exempt the row",
                row.id, text
            )
        });
        checker_syntax::matches_node(&dispatch, node, &parsed, 0)
    };
    let matched_checker = !row.checker.is_empty() && structural_match(row.checker);
    let matched_declared_return = row
        .checker
        .is_empty()
        .then(|| recorded_signature_return(row.decl_emit, "witness").map(structural_match))
        .flatten()
        .unwrap_or(false);
    LiveProbeOutcome {
        matched_checker,
        matched_declared_return,
        deferred_operator,
        rendered,
        degraded,
    }
}

/// The signature return recorded in a row's `decl_emit` bytes for
/// `fn_name` — the machine-formatted `export declare function
/// <name><…>(…): <ret>;` line's return text. The declaration bytes are
/// the recorded STRUCTURED observation (the checker column may be a
/// display-only instantiation), so this is the comparison basis for
/// rows whose order claim lives only there.
fn recorded_signature_return<'a>(decl_emit: &'a str, fn_name: &str) -> Option<&'a str> {
    let needle = format!("function {fn_name}");
    let line = decl_emit
        .lines()
        .find(|line| line.contains(&needle) && line.contains("): "))
        .map(|line| line.trim())?;
    let ret_start = line.rfind("): ")? + 3;
    let ret = line[ret_start..].trim_end_matches(';').trim();
    (!ret.is_empty()).then_some(ret)
}

/// V0-AC3 (compare): the live answer to the RECORDED PROBE follows
/// every row's recorded verdict. The observation lane is the probe lane
/// above (the probe in type position through the public flow-return
/// boundary); the comparison bases are STRUCTURAL ONLY — the typed
/// checker-syntax projection of the recorded `checker` text, the
/// recorded `decl_emit` signature return where the checker column is a
/// display-only instantiation, and (for diagnostic rows) the recorded
/// REFUSAL: the checker refused to print a type, so the row pins that
/// the live rail still holds the probe DEFERRED (an unreduced carrier
/// is the honest non-answer; any reduction flips the row). A later
/// block that changes the answer to any recorded probe FLIPS its row
/// here instead of a prose report — in BOTH directions.
/// One row's verdict evaluation: `Some(failure)` when the live answer to
/// the recorded probe contradicts the row's recorded verdict (either
/// direction), or when a diagnostic row's recorded refusal is no longer
/// pinned by a deferred live carrier.
fn verdict_failure(row: &Row, live: &LiveProbeOutcome) -> Option<String> {
    {
        let rendered = live.rendered.as_deref().unwrap_or("<no value>");
        let note = match row.verdict {
            Verdict::MatchesChecker => {
                if !live.matched_checker {
                    Some(format!(
                        "labelled MatchesChecker but the live answer to the probe `{}` does \
                         not structurally equal the recorded observation `{}` — measured \
                         `{}` (degraded: {}). Either the answer regressed or the observation \
                         was edited; re-measure against the pinned oracle before re-pinning",
                        row.probe, row.checker, rendered, live.degraded
                    ))
                } else {
                    None
                }
            }
            Verdict::KnownOwed { .. } | Verdict::Degraded { .. } => {
                if live.matched_checker {
                    Some(format!(
                        "labelled {:?} but the live answer to the probe `{}` STRUCTURALLY \
                         EQUALS the recorded observation `{}` — the recorded divergence is \
                         GONE. This failure is the INTENDED signal: the answer looks \
                         implemented (or the observation was edited to the live value); \
                         re-pin the row and update the semantic-difference ledger in the \
                         same change",
                        row.verdict, row.probe, row.checker
                    ))
                } else if live.matched_declared_return {
                    Some(format!(
                        "labelled {:?} but the live answer to the probe `{}` STRUCTURALLY \
                         EQUALS the order claim recorded in the declaration bytes \
                         (`{}`) — re-pin the row and update the semantic-difference \
                         ledger in the same change",
                        row.verdict,
                        row.probe,
                        recorded_signature_return(row.decl_emit, "witness").unwrap_or(""),
                    ))
                } else {
                    None
                }
            }
        };
        let note = note.map(|note| format!("{}: {note}", row.id));
        // The diagnostic row pins the recorded REFUSAL: 7.0.2 printed no
        // type (TS2589), so the only honest live answer is the probe held
        // DEFERRED by its outer operator. A reduction to any value — or a
        // clean structural match of a type — flips the row for a re-pin
        // and a ledger review against the recorded refusal.
        if let Some(diagnostic) = row.diagnostic {
            let outer_operator = row.probe.split('<').next().unwrap_or("");
            if live.deferred_operator.as_deref() != Some(outer_operator) {
                return Some(format!(
                    "{}: the recorded observation is the checker's REFUSAL (`{diagnostic}`) \
                     but the live answer to the probe `{}` is no longer the deferred \
                     `{outer_operator}` carrier — measured `{}`. The substrate now has a \
                     disposition where the checker refused; re-pin the row and review the \
                     semantic-difference ledger",
                    row.id, row.probe, rendered
                ));
            }
        }
        note
    }
}

#[test]
fn signature_corpus_live_answers_follow_their_verdicts() {
    let mut failures: Vec<String> = Vec::new();
    for row in CORPUS {
        let live = live_probe_outcome(row);
        if let Some(failure) = verdict_failure(row, &live) {
            failures.push(failure);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The flip law, proven in BOTH directions over the REAL probe lane with
/// synthetic rows whose probes REDUCE today (a bare primitive annotation,
/// no deferred wrapper): a matching live answer satisfies a
/// `MatchesChecker` verdict and CONTRADICTS a `KnownOwed` one, and a
/// diagnostic row whose probe reduces loses its refusal pin. This is the
/// control that keeps the corpus driver's row-flip mechanism honest — a
/// later block implementing a probe reduction flips its row through THIS
/// rail, never a prose report.
#[test]
fn signature_corpus_flip_law_fires_in_both_directions() {
    use crate::signature_corpus_rows_tests::Family;
    let reduced_row = |verdict, diagnostic| Row {
        id: "SV_CONTROL_reduced_probe",
        family: Family::UnionValuedThen,
        source: "export function witness() { return 1; }",
        probe: "string",
        checker: "string",
        checker_is_any: false,
        checker_is_never: false,
        diagnostic,
        decl_emit: "export declare function witness(): number;\n",
        verdict,
    };
    // A reduced live answer satisfies MatchesChecker...
    let matches = reduced_row(Verdict::MatchesChecker, None);
    let live = live_probe_outcome(&matches);
    assert_eq!(verdict_failure(&matches, &live), None);
    assert!(
        live.matched_checker,
        "the control probe must reduce and match"
    );
    // ...and CONTRADICTS KnownOwed (the flip signal fires).
    let owed = reduced_row(
        Verdict::KnownOwed {
            note: "control: the answer is implemented",
        },
        None,
    );
    let live = live_probe_outcome(&owed);
    assert!(
        verdict_failure(&owed, &live)
            .is_some_and(|failure| failure.contains("STRUCTURALLY EQUALS")),
        "an implemented answer must flip a KnownOwed row"
    );
    // A diagnostic row whose probe REDUCES loses its refusal pin.
    let diagnostic = reduced_row(
        Verdict::KnownOwed { note: "control" },
        Some("TS9999 control"),
    );
    let live = live_probe_outcome(&diagnostic);
    assert!(
        verdict_failure(&diagnostic, &live).is_some_and(|failure| failure.contains("REFUSAL")),
        "a reduced probe must flip a diagnostic row's refusal pin"
    );
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
