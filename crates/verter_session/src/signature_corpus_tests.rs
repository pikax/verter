//! The signature corpus DRIVER — the executable half of the pinned-oracle
//! evidence lock (`docs/arch/signature-kernel.md` §14).
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
//!    StableOrderDifference | KnownOwed | Degraded`): the live observation lane drives the
//!    row's probe in TYPE position (a declared binding annotated with
//!    the probe, read back through the public audited flow-return
//!    boundary — never the witness's own return, which for wrapper
//!    probes like `Awaited<ReturnType<...>>` is a different question),
//!    then EXPANDED the way a consumer expands it: publication keeps an
//!    alias/builtin instantiation carrier (the checker keeps the alias
//!    label too) and the `Instantiate` family reduces it on demand,
//!    while the recorded `checker` column is a REDUCED print, so both
//!    sides have to be brought to the same question before they are
//!    comparable. The comparison bases are STRUCTURAL only — the typed
//!    checker-syntax projection of the recorded `checker` text, the
//!    recorded `decl_emit` signature return where the checker column is
//!    a display-only binder instantiation (checker print output — never
//!    a live basis itself) or a recorded refusal
//!    (compared ORDER-SENSITIVELY — the declaration bytes carry union
//!    arm order as a structured field), and for diagnostic rows the
//!    recorded DIAGNOSTIC and the recovery type the checker continues
//!    with, matched against the live typed recovery carrier (an owed
//!    diagnostic row instead pins a live NON-ANSWER: the probe held
//!    deferred, or a typed gap). A
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

use crate::signature_corpus_rows_tests::{RecordedDiagnostic, Row, Verdict, CORPUS};
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
/// the length-prefixed id, checker, decl-emit and recorded-diagnostic
/// bytes (the same length-prefix scheme the u6 cohort fingerprint uses,
/// over the row's RECORDED observation only — never live answers). A row
/// with no diagnostic contributes an empty diagnostic field.
fn corpus_digest_input() -> Vec<u8> {
    let mut buf = Vec::new();
    for row in CORPUS {
        let diagnostic = row
            .diagnostic
            .map(|diagnostic| spell_recorded_diagnostic(&diagnostic))
            .unwrap_or_default();
        for field in [
            row.id.as_bytes(),
            row.checker.as_bytes(),
            row.decl_emit.as_bytes(),
            diagnostic.as_bytes(),
        ] {
            buf.extend_from_slice(&(field.len() as u64).to_le_bytes());
            buf.extend_from_slice(field);
        }
    }
    buf
}

/// The digest spelling of a recorded diagnostic:
/// `TS<code>: <message> => <recovery>`.
fn spell_recorded_diagnostic(diagnostic: &RecordedDiagnostic) -> String {
    format!(
        "TS{}: {} => {}",
        diagnostic.code, diagnostic.message, diagnostic.recovery
    )
}

/// Observation well-formedness: every recorded observation is non-empty, carries
/// exactly one observation form, its `decl_emit` bytes PARSE as a
/// TypeScript declarations module through the tree's own parser, and
/// the table covers every observation family — including exactly
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
        if row.checker_display_only && row.checker.is_empty() {
            failures.push(format!(
                "{}: checker_display_only modifies a RECORDED checker column (a diagnostic row \
                 has no checker text to be display-only)",
                row.id
            ));
        }
        // Where the declaration bytes are a live comparison basis (a
        // display-only checker column, or a recorded refusal with no
        // checker text), the declared return must EXIST and parse into
        // the typed checker-syntax form — the basis the driver compares
        // order-sensitively.
        if row.checker.is_empty() || row.checker_display_only {
            match recorded_signature_return(row.decl_emit, "witness")
                .ok_or_else(|| "no `export declare function witness(…): <ret>;` line".to_owned())
                .and_then(|ret| checker_syntax::parse(ret).map(|_| ()))
            {
                Ok(()) => {}
                Err(error) => failures.push(format!(
                    "{}: the declared-return comparison basis is unusable: {error}",
                    row.id
                )),
            }
        }
        if let Some(diagnostic) = row.diagnostic {
            if diagnostic.code == 0 || diagnostic.message.is_empty() || !row.checker.is_empty() {
                failures.push(format!(
                    "{}: a recorded diagnostic carries its code and message, and replaces the \
                     checker text",
                    row.id
                ));
            }
            // The recovery is a recorded checker print, so it parses through
            // the same typed checker syntax as every checker column.
            if let Err(error) = checker_syntax::parse(diagnostic.recovery) {
                failures.push(format!(
                    "{}: the recorded diagnostic's recovery `{}` does not parse: {error}",
                    row.id, diagnostic.recovery
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
            Verdict::StableOrderDifference { note } => {
                // An order-only difference is admitted only as the ORDER the
                // contract mandates, so the note names it.
                if !note.contains("VerterStableV1") {
                    failures.push(format!(
                        "{}: a stable-order verdict names the `VerterStableV1` order it \
                         attributes the difference to (found {note:?})",
                        row.id
                    ));
                }
                if row.checker_display_only || row.checker.is_empty() {
                    failures.push(format!(
                        "{}: a stable-order verdict compares members against a RECORDED \
                         structural checker column",
                        row.id
                    ));
                }
            }
            Verdict::KnownOwed { note } | Verdict::Degraded { note } => {
                // An owed verdict must explain what the substrate does NOT
                // do, in terms a reader can act on: WHICH CAPABILITY is
                // missing, never a coordination identifier. A bare
                // identifier rots into an unresolvable reference the moment
                // the coordination state moves, and tells a reader nothing
                // about what has to be built for the row to flip.
                let lower = note.to_ascii_lowercase();
                let names_a_gap = ["owed", "defer", "not yet", "gap"]
                    .iter()
                    .any(|phrase| lower.contains(phrase));
                if note.trim().len() < 60 || !names_a_gap {
                    failures.push(format!(
                        "{}: an owed/degraded verdict names the MISSING CAPABILITY in its \
                         note (found {} chars: {note:?})",
                        row.id,
                        note.trim().len()
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
                "family `{}` has no row — the mandatory matrix requires it",
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
    // The ORDER-HEAVY row's basis is the ORDERED union itself: SV26's
    // declared return parses to the exact 3-arm sequence the
    // declaration bytes record (literals in authored order, the binder
    // last) — the input the order-sensitive comparator observes.
    let sv26 = CORPUS
        .iter()
        .find(|r| r.id == "SV26_literal_generic_union_order")
        .expect("the SV26 order-claim row");
    let sv26_basis = recorded_signature_return(sv26.decl_emit, "witness")
        .unwrap_or_else(|| panic!("SV26's declared-return basis is missing"));
    assert_eq!(
        checker_syntax::parse(sv26_basis),
        Ok(checker_syntax::CheckerType::Union(vec![
            checker_syntax::CheckerType::StringLit("a".to_owned()),
            checker_syntax::CheckerType::StringLit("b".to_owned()),
            checker_syntax::CheckerType::Ref("T".to_owned()),
        ])),
        "SV26's declared-return basis must parse to the recorded arm ORDER"
    );
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
    /// one that is not a display-only column). Structured ONLY — a
    /// Verter display string is never semantic identity, and neither is
    /// a display-only checker print (those rows compare through their
    /// declared return alone).
    matched_checker: bool,
    /// The live answer matches the recorded checker text with union arm
    /// ORDER included. Differs from `matched_checker` only for an
    /// order-only difference.
    matched_checker_in_order: bool,
    /// The live answer structurally matches the claim recorded in the
    /// row's `decl_emit` signature return — the rows whose checker
    /// column is a DISPLAY-ONLY instantiation of the binders (e.g.
    /// `unknown` where the declaration bytes carry the dedup and the
    /// literal/generic arm ORDER) or whose checker text is a recorded
    /// refusal. The declaration bytes carry union arm order as a
    /// structured field, so this basis compares ORDER-SENSITIVELY.
    matched_declared_return: bool,
    /// The live answer is the checker's error type after the row's RECORDED
    /// diagnostic — the typed recovery carrier naming that code, whose
    /// recovery matches the recorded recovery print. Only a row that records
    /// a diagnostic has this basis.
    matched_diagnostic: bool,
    /// The base declaration name when the live answer is the DEFERRED
    /// instantiation carrier (`InstantiationRef(<operator>)` — the
    /// substrate has not reduced the probe's outer operator), else
    /// `None`.
    deferred_operator: Option<std::sync::Arc<str>>,
    /// The rail answered the structural-fact demand with a SEMANTIC
    /// non-answer: a `Partial` demand (truncated/faulted, no node at all),
    /// or an `Opaque` carrier whose disposition is `OptionalAbsence` ("no
    /// result under this view") or the §22 `Failure` error type.
    ///
    /// Deliberately NARROW. The other dispositions are excluded because
    /// none of them is a refusal: `ExpandableDecl` and `RecursionCarrier`
    /// DENOTE a type the rail has not finished reading (accepting them
    /// would re-admit exactly the publication laziness this lane expands
    /// away), and `ControlCarrier` / `Partial` / `UnsupportedSurface` are
    /// resource or boundary control, not a semantic answer about the
    /// program.
    refused: bool,
    /// The audited flow-return BOUNDARY produced no result at all. This is
    /// never an observation: it means the lane itself is broken (a renamed
    /// probe function, a malformed probe module), so it fails EVERY row
    /// rather than satisfying a row that pins a non-answer.
    boundary_failed: bool,
    /// The rendered live answer (diagnostics only — never a comparison
    /// basis).
    rendered: Option<String>,
    /// The typed degradation on a completed answer.
    degraded: bool,
}

/// Drive one row's probe lane through the public audited boundary and
/// evaluate both structural bases against the live node.
fn live_probe_outcome(row: &Row) -> LiveProbeOutcome {
    live_probe_outcome_on(
        row,
        &crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host(),
    )
}

/// [`live_probe_outcome`] on a caller-built host (a fresh one per row).
fn live_probe_outcome_on(row: &Row, host: &crate::VerterHost) -> LiveProbeOutcome {
    let canonical = "/wb/signature_probe.ts";
    crate::u6_flow_shape_corpus_tests::upsert(
        host,
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
        // The boundary produced NO result: the lane is broken, not the
        // substrate answering. Reported as such, never as a refusal.
        return LiveProbeOutcome {
            matched_checker: false,
            matched_checker_in_order: false,
            matched_declared_return: false,
            matched_diagnostic: false,
            deferred_operator: None,
            refused: false,
            boundary_failed: true,
            rendered: None,
            degraded: false,
        };
    };
    let degraded = result.degradation().is_some();
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(&host_ctx);
    // Publication KEEPS an alias/builtin instantiation carrier: the
    // checker keeps the alias label too, and the carrier is resolved when
    // a consumer DEMANDS the structural fact behind it. The recorded
    // `checker` column, by contrast, is what 7.0.2 prints through the
    // corpus's two-step wrapper — a REDUCED form. Comparing the published
    // carrier against a reduced print measures publication laziness, not
    // a semantic difference, and no row could ever match.
    //
    // Reduce the carrier to the altitude the checker PRINTS at, through the
    // production structural-fact loop (evaluate deferred shells, instantiate
    // a residual `InstantiationRef` through the shared `Instantiate` query),
    // bounded by exact-identity cycle detection and fail-closed. It is not a
    // second resolver, and this lane must not grow one: a hand-rolled
    // expansion loop would drop the cycle detection and the typed `Partial`
    // outcome, and a cyclic alias would silently decide the compared node by
    // the iteration budget.
    //
    // The loop runs in its declaration-KEEPING mode: it stops where the
    // checker prints a NAME rather than resolving it to the declaration's
    // body — an interface or class by its name
    // (`InstanceType<typeof CtorA & typeof CtorB>` prints `B`), an alias
    // application the checker names by its alias (`G<boolean>` for
    // `type G<U> = F<U[]>`), each with its omitted defaulted arguments
    // filled the way the checker fills them (`WithDefault` prints
    // `WithDefault<string>`). The checker-syntax matcher compares a name
    // against that carrier — never against an expanded body — so resolving
    // it would overshoot the printed answer by one step and read a correct
    // answer as owed.
    let demand = dispatch.normalize_node_keeping_declaration_refs_for_tests(
        result.return_type(),
        crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    );
    // A `Partial` demand carries NO node: truncated or faulted, which is a
    // semantic non-answer, not a type. Nothing to compare, so the row's
    // structural bases stay false and only a non-answer pin is satisfied.
    let Some(node) = demand.into_complete_node() else {
        return LiveProbeOutcome {
            matched_checker: false,
            matched_checker_in_order: false,
            matched_declared_return: false,
            matched_diagnostic: false,
            deferred_operator: None,
            refused: true,
            boundary_failed: false,
            rendered: Some("<partial structural-fact demand>".to_owned()),
            degraded,
        };
    };
    let rendered = Some(render_node(&dispatch, node, 0));
    let (deferred_operator, refused) = match dispatch.graph().node_data(node) {
        Some(data) => match data.as_ref() {
            crate::semantic_query::SemanticNodeData::InstantiationRef { base, .. } => {
                (Some(std::sync::Arc::clone(&base.decl_name)), false)
            }
            // Only a SEMANTIC non-answer counts as a refusal — see the
            // `refused` field. An identity carrier that still denotes a
            // type, or a resource/control sentinel, does not.
            crate::semantic_query::SemanticNodeData::Opaque(error) => {
                use crate::project_semantic_dispatch::query_error_disposition::{
                    query_error_disposition, QueryErrorDisposition,
                };
                (
                    None,
                    matches!(
                        query_error_disposition(error),
                        QueryErrorDisposition::OptionalAbsence | QueryErrorDisposition::Failure
                    ),
                )
            }
            _ => (None, false),
        },
        None => (None, false),
    };
    let structural_match = |text: &str, ordered: bool| {
        let parsed = checker_syntax::parse(text).unwrap_or_else(|err| {
            panic!(
                "{}: recorded text `{}` does not parse ({err}) — extend the checker-syntax \
                 parser deliberately, never exempt the row",
                row.id, text
            )
        });
        if ordered {
            checker_syntax::matches_node_ordered(&dispatch, node, &parsed, 0)
        } else {
            checker_syntax::matches_node(&dispatch, node, &parsed, 0)
        }
    };
    // The checker text is a basis only where it carries a STRUCTURAL
    // claim: a display-only column (a binder-at-constraint display,
    // SV25/26) is checker PRINT OUTPUT, never semantic identity, so
    // those rows compare through their declared return alone and a
    // reduction to the display text must not satisfy or flip them.
    let matched_checker = !row.checker.is_empty()
        && !row.checker_display_only
        && structural_match(row.checker, false);
    let matched_checker_in_order = matched_checker && structural_match(row.checker, true);
    // The declared return is a LIVE basis when the checker column is a
    // display-only binder instantiation (the dedup/order claim lives in
    // the declaration bytes — SV25/26) or when the row records a
    // refusal (no checker text at all — SV21). Union arm ORDER is a
    // structured field of the declaration bytes, so this basis rides
    // the order-sensitive comparator: a wrong-order implementation must
    // NOT flip the row as if the order matched.
    let declared_return_basis = row.checker.is_empty() || row.checker_display_only;
    let matched_declared_return = declared_return_basis
        .then(|| {
            recorded_signature_return(row.decl_emit, "witness")
                .map(|ret| structural_match(ret, true))
        })
        .flatten()
        .unwrap_or(false);
    // A recorded diagnostic is matched by the checker's error type after
    // that diagnostic: the typed recovery carrier naming the recorded code
    // and message, whose recovery reads as the recorded recovery print.
    let live_diagnostic = match dispatch.graph().node_data(node).as_deref() {
        Some(crate::semantic_query::SemanticNodeData::Opaque(
            crate::semantic_query::QueryError::CheckerRecovery(diagnostic),
        )) => Some(*diagnostic),
        _ => None,
    };
    let matched_diagnostic =
        match (row.diagnostic, live_diagnostic) {
            (Some(recorded), Some(live)) => {
                let recovery = dispatch.graph().intern_node(
                    crate::semantic_query::SemanticNodeData::Primitive(live.recovery()),
                );
                let recorded_recovery =
                    checker_syntax::parse(recorded.recovery).unwrap_or_else(|err| {
                        panic!(
                            "{}: recorded recovery `{}` does not parse ({err})",
                            row.id, recorded.recovery
                        )
                    });
                live.code.code() == recorded.code
                    && live.code.message() == recorded.message
                    && checker_syntax::matches_node(&dispatch, recovery, &recorded_recovery, 0)
            }
            _ => false,
        };
    LiveProbeOutcome {
        matched_checker,
        matched_checker_in_order,
        matched_declared_return,
        matched_diagnostic,
        deferred_operator,
        refused,
        boundary_failed: false,
        rendered,
        degraded,
    }
}

/// The signature return recorded in a row's `decl_emit` bytes for
/// `fn_name` — the machine-formatted `export declare function
/// <name><…>(…): <ret>;` line's return text. The declaration bytes are
/// the recorded STRUCTURED observation (the checker column may be a
/// display-only binder instantiation), so this is the comparison basis
/// — order-sensitively — for rows whose checker column is display-only
/// or a recorded refusal, whose claim lives only here.
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

/// Verdict fidelity: the live answer to the RECORDED PROBE follows
/// every row's recorded verdict. The observation lane is the probe lane
/// above (the probe in type position through the public flow-return
/// boundary); the comparison bases are STRUCTURAL ONLY — the typed
/// checker-syntax projection of the recorded `checker` text, the
/// recorded `decl_emit` signature return where the checker column is a
/// display-only instantiation, and (for diagnostic rows) the recorded
/// DIAGNOSTIC with the recovery the checker continues with: a matching
/// row's live answer is the typed recovery carrier naming that code, and
/// an owed row pins that the live rail still refuses (an unreduced carrier
/// or a typed gap; any other reduction flips the row). A later block that
/// changes the answer to any recorded probe FLIPS its row here instead of
/// a prose report — in BOTH directions.
/// One row's verdict evaluation: `Some(failure)` when the live answer to
/// the recorded probe contradicts the row's recorded verdict (either
/// direction), when an owed diagnostic row's recorded refusal is no longer
/// pinned by a live non-answer, or when the observation lane itself
/// failed to produce a result.
fn verdict_failure(row: &Row, live: &LiveProbeOutcome) -> Option<String> {
    // A broken lane is never evidence. Checked FIRST and for EVERY row, so
    // a boundary that answers nothing cannot satisfy a row that pins a
    // non-answer — the row would report green having observed nothing.
    if live.boundary_failed {
        return Some(format!(
            "{}: the audited flow-return boundary produced NO result for the probe `{}`, so \
             this row observed nothing. The observation lane is broken (the probe module or \
             its `__sig_probe_lane` entry), not the substrate answering",
            row.id, row.probe
        ));
    }
    {
        let rendered = live.rendered.as_deref().unwrap_or("<no value>");
        let note = match row.verdict {
            // A diagnostic row's basis is the recorded diagnostic and the
            // recovery the checker continues with.
            Verdict::MatchesChecker if row.diagnostic.is_some() => {
                (!live.matched_diagnostic).then(|| {
                    format!(
                        "labelled MatchesChecker but the live answer to the probe `{}` is not \
                         the checker's recovery after the recorded diagnostic `{}` — measured \
                         `{}` (degraded: {}). Either the answer regressed or the observation \
                         was edited; re-measure against the pinned oracle before re-pinning",
                        row.probe,
                        row.diagnostic
                            .map(|recorded| spell_recorded_diagnostic(&recorded))
                            .unwrap_or_default(),
                        rendered,
                        live.degraded
                    )
                })
            }
            Verdict::MatchesChecker => {
                if !(live.matched_checker || live.matched_declared_return) {
                    let bases = if row.checker_display_only {
                        format!(
                            "the display-only checker text `{}` nor the declared return `{}`",
                            row.checker,
                            recorded_signature_return(row.decl_emit, "witness").unwrap_or("")
                        )
                    } else {
                        format!("the recorded observation `{}`", row.checker)
                    };
                    Some(format!(
                        "labelled MatchesChecker but the live answer to the probe `{}` matches \
                         NEITHER recorded basis — {bases} — measured `{}` (degraded: {}). \
                         Either the answer regressed or the observation was edited; \
                         re-measure against the pinned oracle before re-pinning",
                        row.probe, rendered, live.degraded
                    ))
                } else {
                    None
                }
            }
            Verdict::StableOrderDifference { .. } => {
                if !live.matched_checker {
                    Some(format!(
                        "labelled StableOrderDifference but the live answer to the probe `{}` no \
                         longer equals the recorded observation `{}` even as a SET of members — \
                         measured `{}` (degraded: {}). That is a semantic difference, not an \
                         order one: re-measure and move the row and its ledger entry",
                        row.probe, row.checker, rendered, live.degraded
                    ))
                } else if live.matched_checker_in_order {
                    Some(format!(
                        "labelled StableOrderDifference but the live answer to the probe `{}` now \
                         equals the recorded observation `{}` IN ORDER — the order difference is \
                         gone. Re-pin the row MatchesChecker and move its ledger entry in the \
                         same change",
                        row.probe, row.checker
                    ))
                } else {
                    None
                }
            }
            Verdict::KnownOwed { .. } | Verdict::Degraded { .. } => {
                if live.matched_diagnostic {
                    Some(format!(
                        "labelled {:?} but the live answer to the probe `{}` IS the checker's \
                         recovery after the recorded diagnostic `{}` — the recorded divergence \
                         is GONE. Re-pin the row and update the semantic-difference ledger in \
                         the same change",
                        row.verdict,
                        row.probe,
                        row.diagnostic
                            .map(|recorded| spell_recorded_diagnostic(&recorded))
                            .unwrap_or_default(),
                    ))
                } else if live.matched_checker {
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
                         EQUALS the claim recorded in the declaration bytes (`{}`) — the \
                         checker column is a display-only instantiation, so the declared \
                         return (union arm ORDER included) is this row's basis. Re-pin the \
                         row and update the semantic-difference ledger in the same change",
                        row.verdict,
                        row.probe,
                        recorded_signature_return(row.decl_emit, "witness").unwrap_or(""),
                    ))
                } else {
                    None
                }
            }
        };
        // An OWED diagnostic row pins the recorded REFUSAL with a live
        // NON-ANSWER: the probe held DEFERRED by its outer operator, or a
        // typed gap. Neither fabricates a type where the checker printed
        // none; a reduction to an actual VALUE flips the row for a re-pin
        // and a ledger review. (A matching diagnostic row answers the
        // checker's own recovery instead, checked above.)
        if let (Some(diagnostic), Verdict::KnownOwed { .. } | Verdict::Degraded { .. }) =
            (row.diagnostic, &row.verdict)
        {
            let outer_operator = row.probe.split('<').next().unwrap_or("");
            let held_deferred = live.deferred_operator.as_deref() == Some(outer_operator);
            if !held_deferred && !live.refused {
                return Some(format!(
                    "{}: the recorded observation is the checker's REFUSAL (`{}`) but the \
                     live answer to the probe `{}` is neither the deferred `{outer_operator}` \
                     carrier nor a typed gap — measured `{}`. The substrate now publishes a \
                     TYPE where the checker refused; re-pin the row and review the \
                     semantic-difference ledger",
                    row.id,
                    spell_recorded_diagnostic(&diagnostic),
                    row.probe,
                    rendered
                ));
            }
        }
        note.map(|note| format!("{}: {note}", row.id))
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
/// `MatchesChecker` verdict and CONTRADICTS a `KnownOwed` one, a
/// diagnostic row whose probe reduces loses its refusal pin, and a
/// DISPLAY-ONLY row flips through its declared-return basis alone (the
/// rail the union-ordering rows ride) while a live answer matching ONLY
/// the display text never satisfies or flips one. This is the control
/// that keeps the
/// corpus driver's row-flip mechanism honest — a later block implementing
/// a probe reduction flips its row through THIS rail, never a prose
/// report.
/// A BROKEN observation lane is not a refusal.
///
/// A diagnostic row pins the checker's refusal with a live non-answer, so
/// the one thing that must never satisfy it is "the boundary produced
/// nothing": the row would report green having observed no program at
/// all. The `refused` disjunct makes that reachable, so it is pinned here
/// rather than left to inspection — the outcome is constructed directly
/// because a genuinely broken lane cannot be provoked from a well-formed
/// corpus row.
#[test]
fn a_boundary_failure_never_satisfies_a_diagnostic_row() {
    use crate::signature_corpus_rows_tests::Family;
    let row = Row {
        id: "SV_CONTROL_boundary_failure",
        family: Family::UnionValuedThen,
        source: "export function witness() { return 1; }",
        probe: "Awaited<ReturnType<typeof witness>>",
        checker: "",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: Some(RecordedDiagnostic {
            code: 2589,
            message: "Type instantiation is excessively deep and possibly infinite.",
            recovery: "any",
        }),
        decl_emit: "export declare function witness(): number;\n",
        verdict: Verdict::KnownOwed {
            note: "control: the lane is broken",
        },
    };
    let broken = LiveProbeOutcome {
        matched_checker: false,
        matched_checker_in_order: false,
        matched_declared_return: false,
        matched_diagnostic: false,
        deferred_operator: None,
        refused: false,
        boundary_failed: true,
        rendered: None,
        degraded: false,
    };
    assert!(
        verdict_failure(&row, &broken)
            .is_some_and(|failure| failure.contains("produced NO result")),
        "a boundary that answered nothing must fail the row, never pin its refusal"
    );
    // The same row IS satisfied by a genuine semantic non-answer, so the
    // check above discriminates a broken lane from a real refusal rather
    // than simply rejecting everything.
    let refused = LiveProbeOutcome {
        refused: true,
        boundary_failed: false,
        rendered: Some("Opaque(Miss)".to_owned()),
        ..broken
    };
    assert_eq!(
        verdict_failure(&row, &refused),
        None,
        "a typed gap where the checker refused is an honest live non-answer"
    );
}

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
        checker_display_only: false,
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
        Some(RecordedDiagnostic {
            code: 9999,
            message: "control",
            recovery: "any",
        }),
    );
    let live = live_probe_outcome(&diagnostic);
    assert!(
        verdict_failure(&diagnostic, &live).is_some_and(|failure| failure.contains("REFUSAL")),
        "a reduced probe must flip a diagnostic row's refusal pin"
    );
    // A DISPLAY-ONLY row (the checker column is a binder display; the
    // claim is the DECLARED return) rides the declared basis through
    // the same rail: a matching live answer satisfies MatchesChecker
    // WITHOUT the display text matching...
    let mut display = reduced_row(Verdict::MatchesChecker, None);
    display.checker = "unknown";
    display.checker_display_only = true;
    display.decl_emit = "export declare function witness(): string;\n";
    let live = live_probe_outcome(&display);
    assert!(
        !live.matched_checker && live.matched_declared_return,
        "the display-only control must match through the declared return alone"
    );
    assert_eq!(verdict_failure(&display, &live), None);
    // ...and CONTRADICTS KnownOwed through that same declared basis
    // (the flip signal fires on the rail the union-ordering rows ride).
    let owed_display = Row {
        verdict: Verdict::KnownOwed {
            note: "control: the declared answer is implemented",
        },
        ..display
    };
    let live = live_probe_outcome(&owed_display);
    assert!(
        verdict_failure(&owed_display, &live)
            .is_some_and(|failure| failure.contains("STRUCTURALLY EQUALS")),
        "an implemented declared-return claim must flip a KnownOwed display-only row"
    );
    // ...but a live answer that matches ONLY the display text must NOT
    // satisfy or flip a display-only row: the display instantiation is
    // checker print output, never semantic identity, so the ORDERED
    // declared return stays the row's only basis (without the
    // exclusion, a later `ReturnType` reduction to the display text
    // would flip SV25/SV26 without the order/dedup claim).
    let mut display_text = reduced_row(
        Verdict::KnownOwed {
            note: "control: display text alone is not an answer",
        },
        None,
    );
    display_text.probe = "unknown";
    display_text.checker = "unknown";
    display_text.checker_display_only = true;
    display_text.decl_emit = "export declare function witness(): string;\n";
    let live = live_probe_outcome(&display_text);
    assert!(
        !live.matched_checker && !live.matched_declared_return,
        "the display text must not be a live basis for a display-only row"
    );
    assert_eq!(
        verdict_failure(&display_text, &live),
        None,
        "a KnownOwed display-only row must NOT flip when only the display text matches"
    );
}

/// The flip law for a DIAGNOSTIC row, in both directions over the real
/// probe lane: the checker's recovery after the recorded diagnostic
/// satisfies a `MatchesChecker` row and contradicts an owed one, while a
/// different recorded code, a different recorded recovery, or a probe the
/// substrate answers without a diagnostic never matches.
///
/// The probe is the corpus's recursive thenable (`Awaited<Rec>`), measured on
/// tsc 7.0.2: TS2589 at the `Awaited` reference, `[any]` through the tuple
/// wrapper. `Awaited<NumberThen>` is `number` with no diagnostic.
#[test]
fn diagnostic_rows_flip_on_the_checker_recovery_in_both_directions() {
    use crate::signature_corpus_rows_tests::Family;
    const TS2589: RecordedDiagnostic = RecordedDiagnostic {
        code: 2589,
        message: "Type instantiation is excessively deep and possibly infinite.",
        recovery: "any",
    };
    let row = |probe, diagnostic, verdict| Row {
        id: "SV_CONTROL_recursive_thenable",
        family: Family::AwaitedResidual,
        source: "interface Rec { then(onfulfilled: (v: Rec) => void): void }\n\
                 interface NumberThen { then(onfulfilled: (v: number) => void): void }\n\
                 export function witness() { const t: Rec = null as any; return t; }",
        probe,
        checker: "",
        checker_is_any: false,
        checker_is_never: false,
        checker_display_only: false,
        diagnostic: Some(diagnostic),
        decl_emit: "export declare function witness(): Rec;\n",
        verdict,
    };
    let matching = row("Awaited<Rec>", TS2589, Verdict::MatchesChecker);
    let live = live_probe_outcome(&matching);
    assert!(
        live.matched_diagnostic,
        "the recursive thenable answers the TS2589 recovery, measured `{}`",
        live.rendered.as_deref().unwrap_or("<no value>")
    );
    assert_eq!(verdict_failure(&matching, &live), None);
    let owed = row(
        "Awaited<Rec>",
        TS2589,
        Verdict::KnownOwed {
            note: "control: the recovery is implemented",
        },
    );
    assert!(
        verdict_failure(&owed, &live_probe_outcome(&owed)).is_some(),
        "an implemented recovery must flip an owed diagnostic row"
    );
    for (why, diagnostic) in [
        (
            "a different code",
            RecordedDiagnostic {
                code: 1062,
                message: "Type is referenced directly or indirectly in the fulfillment callback \
                          of its own 'then' method.",
                recovery: "any",
            },
        ),
        (
            "a different recovery",
            RecordedDiagnostic {
                recovery: "never",
                ..TS2589
            },
        ),
    ] {
        let other = row("Awaited<Rec>", diagnostic, Verdict::MatchesChecker);
        let live = live_probe_outcome(&other);
        assert!(!live.matched_diagnostic, "{why} must not match");
        assert!(
            verdict_failure(&other, &live).is_some(),
            "{why} must fail a MatchesChecker diagnostic row"
        );
    }
    let no_diagnostic = row("Awaited<NumberThen>", TS2589, Verdict::MatchesChecker);
    let live = live_probe_outcome(&no_diagnostic);
    assert!(
        !live.matched_diagnostic && verdict_failure(&no_diagnostic, &live).is_some(),
        "a probe answered without a diagnostic never satisfies a diagnostic row"
    );
}

/// Corpus identity: the recorded observations digest to the corpus
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

/// The §5.8 counterfactual behind every stable-order verdict: the union
/// ORDER is the only cause of the difference. On a fresh host whose store
/// reverses the `VerterStableV1` order — an isolated cache namespace, its
/// union views and memo entries included, where nothing else changes — the
/// same probe answers the checker's recorded observation IN ORDER; on a
/// fresh host under the stable order it answers the recorded order
/// difference again.
#[test]
fn union_order_is_the_only_difference_from_the_checker() {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host;
    let rows: Vec<&Row> = CORPUS
        .iter()
        .filter(|row| matches!(row.verdict, Verdict::StableOrderDifference { .. }))
        .collect();
    assert!(
        !rows.is_empty(),
        "the corpus admits an order-only difference; the counterfactual must cover it"
    );
    for row in rows {
        let reversed = make_audit_host();
        reversed
            .project_type_store()
            .semantic_graph()
            .reverse_union_order_for_tests();
        let under_reversal = live_probe_outcome_on(row, &reversed);
        assert!(
            under_reversal.matched_checker_in_order,
            "{}: with ONLY the union order reversed, the probe must answer the recorded \
             observation `{}` in order — measured `{}`",
            row.id,
            row.checker,
            under_reversal.rendered.as_deref().unwrap_or("<no value>")
        );
        let restored = live_probe_outcome(row);
        assert!(
            restored.matched_checker && !restored.matched_checker_in_order,
            "{}: under the stable order the probe must answer the recorded members in the \
             stable order again — measured `{}`",
            row.id,
            restored.rendered.as_deref().unwrap_or("<no value>")
        );
    }
}

/// The observation lane's print altitude names a declaration application
/// the way the checker prints it: an interface by its name, an alias
/// application by its alias when the alias constructs the type it settles
/// on — each with its omitted defaulted arguments filled — and every other
/// alias (a conditional, a bare parameter, a primitive, a union that
/// collapsed to one member) as what it resolves to. An alias of a
/// non-generic declaration is that declaration (`ToFace` prints `Face`,
/// `ToToObj` prints `ObjNoGen`), while one of a generic interface
/// application keeps its own name (`ToGFace`, `PromAlias<number>`). A
/// mapped utility application is printed by the utility's name
/// (`Partial<{ a: 1; }>`, `Pick<…, "a">`); an alias of a homomorphic one
/// (`Partial` / `Readonly`, or an alias declaring one) takes the mapped
/// name unless its declared source is a union (`P<{ a: 1 }>` and
/// `PP<{ a: 1 }>` print `Partial<{ a: 1; }>`, `MpA<{ a: 1 }>` prints
/// `Mp<{ a: 1; }>`, `PU` prints `PU`), and an alias of a keyed one keeps
/// its own (`PickA<…>`, `Rec<"x">`). Every checker print is TypeScript
/// 7.0.2's, measured on this exact module through the corpus's two-step
/// wrapper.
#[test]
fn the_print_altitude_names_declaration_applications_as_the_checker_does() {
    use crate::signature_corpus_rows_tests::Family;
    const SOURCE: &str = "\
type WithDefault<T = string> = { value: T };
type Chain<T, U = T[]> = { self: T; others: U };
type F<T> = { f: T };
type G<U> = F<U[]>;
type Un<T> = T | undefined;
type In<T> = T & { x: 1 };
type Mp<T> = { [K in keyof T]: T[K] };
type Prim = string;
interface GI<T = number> { v: T }
type Cond<T = string> = T extends string ? { s: 1 } : { n: 1 };
type UnD<T = string> = T | number;
type Arr<T> = T[];
type Collapse<T> = T | string;
type Nest<T = boolean> = G<T>;
type Tup<T> = [T, T];
type OuterCond<T = string> = Cond<T>;
type ObjNoGen = { a: 1 };
type Fn<T> = (x: T) => void;
type Lit<T> = T;
interface Face { a: 1 }
class Klass { k = 1 }
type ToFace = Face;
type ToToFace = ToFace;
type ToKlass = Klass;
type ToObj = ObjNoGen;
type ToToObj = ToObj;
type ToGFace = GI<string>;
type ToGFaceDefault = GI;
type GenToFace<T> = GI<T>;
type PromAlias<T> = Promise<T>;
type P<T> = Partial<T>;
type PP<T> = P<T>;
type PFace = Partial<Face>;
type NonGenP = Partial<{ z: 1 }>;
type RO<T> = Readonly<T>;
type PU = Partial<Face | ObjNoGen>;
type PickA<T extends { a: unknown }> = Pick<T, 'a'>;
type Rec<K extends string> = Record<K, number>;
type OmitA<T> = Omit<T, 'a'>;
type MpA<T> = Mp<T>;
export function witness() { return 1; }";
    let mut failures = Vec::new();
    for (probe, checker) in [
        ("WithDefault", "WithDefault<string>"),
        ("Chain<number>", "Chain<number, number[]>"),
        ("G<boolean>", "G<boolean>"),
        ("Un<string>", "Un<string>"),
        ("In<{ y: 2 }>", "In<{ y: 2; }>"),
        ("Mp<{ a: 1 }>", "Mp<{ a: 1; }>"),
        ("Prim", "string"),
        ("GI", "GI<number>"),
        ("Cond", "{ s: 1; }"),
        ("UnD", "UnD<string>"),
        ("Arr<number>", "Arr<number>"),
        ("Collapse<string>", "string"),
        ("Nest", "Nest<boolean>"),
        ("Tup<number>", "Tup<number>"),
        ("OuterCond", "{ s: 1; }"),
        ("ObjNoGen", "ObjNoGen"),
        ("Fn<number>", "Fn<number>"),
        ("Lit<{ z: 1 }>", "{ z: 1; }"),
        ("ToFace", "Face"),
        ("ToToFace", "Face"),
        ("ToKlass", "Klass"),
        ("ToObj", "ObjNoGen"),
        ("ToToObj", "ObjNoGen"),
        ("ToGFace", "ToGFace"),
        ("ToGFaceDefault", "ToGFaceDefault"),
        ("GenToFace<boolean>", "GenToFace<boolean>"),
        ("PromAlias<number>", "PromAlias<number>"),
        ("P<{ a: 1 }>", "Partial<{ a: 1; }>"),
        ("PP<{ a: 1 }>", "Partial<{ a: 1; }>"),
        ("PFace", "Partial<Face>"),
        ("NonGenP", "Partial<{ z: 1; }>"),
        ("RO<Face>", "Readonly<Face>"),
        ("PU", "PU"),
        ("PickA<{ a: 1; b: 2 }>", "PickA<{ a: 1; b: 2; }>"),
        ("Rec<'x'>", "Rec<\"x\">"),
        ("OmitA<{ a: 1; b: 2 }>", "OmitA<{ a: 1; b: 2; }>"),
        ("MpA<{ a: 1 }>", "Mp<{ a: 1; }>"),
        ("Partial<{ a: 1 }>", "Partial<{ a: 1; }>"),
        ("Pick<{ a: 1; b: 2 }, 'a'>", "Pick<{ a: 1; b: 2; }, \"a\">"),
        ("Record<'x', number>", "Record<\"x\", number>"),
        ("Omit<{ a: 1; b: 2 }, 'a'>", "Omit<{ a: 1; b: 2; }, \"a\">"),
        ("Partial<Face | ObjNoGen>", "Partial<Face | ObjNoGen>"),
    ] {
        let row = Row {
            id: "SV_CONTROL_print_altitude",
            family: Family::SubstitutionStages,
            source: SOURCE,
            probe,
            checker,
            checker_is_any: false,
            checker_is_never: false,
            checker_display_only: false,
            diagnostic: None,
            decl_emit: "export declare function witness(): number;\n",
            verdict: Verdict::MatchesChecker,
        };
        let live = live_probe_outcome(&row);
        if !live.matched_checker {
            failures.push(format!(
                "`{probe}`: the checker prints `{checker}`, the lane measured `{}`",
                live.rendered.as_deref().unwrap_or("<no value>")
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
