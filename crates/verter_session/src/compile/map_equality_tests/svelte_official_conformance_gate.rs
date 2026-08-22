//! Full-axis gate over the Svelte requests a public/default route can
//! actually issue.
//!
//! The manifest holds twelve goldens, not twelve requests: `dev` has no
//! public spelling, so each `(fixture, generate)` pair's two `dev` arms
//! are the same request (see
//! [`the_dev_axis_has_no_public_spelling_and_only_its_implicit_value_is_reachable`](super::svelte_official_conformance_matrix::the_dev_axis_has_no_public_spelling_and_only_its_implicit_value_is_reachable)).
//! Six reachable requests (three client, three server). The six `dev1`
//! goldens are classified out via
//! [`SvelteCell::reachability`](super::svelte_official_conformance_matrix::SvelteCell::reachability)
//! and never driven.
//!
//! Per reachable client request: compile through the shipped
//! `get_virtual_file(Main)` route, hand `code`/`map` to
//! `check-candidate.mjs --authoritative`, require every axis `ran`
//! (`runtime` may be `not-applicable` for a Svelte client artifact —
//! never `skipped`), and assert the characterized current outcome.
//! Fails if a defect deepens or is silently corrected. Official
//! behaviour is in the conformance targets at the bottom — this module
//! owns no correction.
//!
//! `cargo test -p verter_session --lib --features bf2-authoritative
//! svelte_official_conformance_gate -- --test-threads=1 --nocapture`
//!
//! Without the feature this module is not compiled. Read the
//! `running N tests` line, never the exit code. One request at a time
//! so the harness's shared scratch is never concurrent.

use std::collections::BTreeSet;

use super::bf2_full_axis_gate::{check_candidate, CellReport};
use super::svelte_official_conformance_matrix::{
    pinned_svelte_domain, reachable_client_requests, reachable_server_requests,
    read_svelte_conformance_matrix, read_svelte_golden_record, svelte_client_cells,
    svelte_server_cells, unreachable_goldens, RequestReachability, SvelteCell, SvelteRouteOutcome,
    DEV_AXIS_IMPLICIT_VALUE, SVELTE_PINNED_PACKAGE_VERSION,
};
use super::*;

/// The five axes a Svelte CLIENT request must genuinely execute. `runtime` is
/// handled separately: `not-applicable` is legitimate for it and only for it.
const REQUIRED_AXES: &[&str] = &["parse", "link", "structural", "diagnostics", "mapping"];

/// Format one request's axis map for the per-request evidence table.
fn axis_table(report: &CellReport) -> String {
    let mut rows: Vec<String> = report
        .axes
        .iter()
        .map(|(name, (status, reason))| match reason {
            Some(reason) => format!("{name}={status} ({reason})"),
            None => format!("{name}={status}"),
        })
        .collect();
    rows.sort();
    rows.join(" ")
}

/// The divergence families an EMITTING cell's oracle report carries.
///
/// The empty set is UNREPRESENTABLE here by construction: the fields are private
/// to this module and the only constructors are the three below, so
/// `EmitsAndFails` cannot be written for a cell that diverges in no family. That
/// state is its own variant, [`CharacterizedOutcome::EmitsAndPasses`], and the
/// gate holds each arm to the matching oracle verdict. Before the split,
/// `EmitsAndFails { structural: false, mapping: false }` was spellable and would
/// have silently recorded a PASSING cell under a name asserting it fails — the
/// gate never compared the verdict, only the two family booleans.
mod emitted_divergences {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct EmittedDivergences {
        structural: bool,
        mapping: bool,
    }

    // The three constructors are the CLOSED set of legal divergence
    // combinations. Two are unused while no recorded cell carries a structural
    // divergence; they are retained because their absence is what would make
    // the empty set spellable again — a caller needing "structural only" would
    // otherwise reach for a raw struct literal.
    #[allow(
        dead_code,
        reason = "the closed constructor set is the mechanism that makes the empty divergence \
                  set unrepresentable; retaining every legal combination is the point"
    )]
    impl EmittedDivergences {
        /// The candidate diverges structurally from its golden only.
        pub(super) const fn structural_only() -> Self {
            Self {
                structural: true,
                mapping: false,
            }
        }

        /// The candidate's own source map is not truthful about its output, and
        /// it is otherwise structurally conformant.
        pub(super) const fn mapping_only() -> Self {
            Self {
                structural: false,
                mapping: true,
            }
        }

        /// Both families diverge.
        pub(super) const fn structural_and_mapping() -> Self {
            Self {
                structural: true,
                mapping: true,
            }
        }

        pub(super) const fn structural(self) -> bool {
            self.structural
        }

        pub(super) const fn mapping(self) -> bool {
            self.mapping
        }
    }
}

use emitted_divergences::EmittedDivergences;

/// The CURRENT outcome of one reachable client request, as this suite has
/// measured it. Keyed by the fixture, because the request — not the golden — is
/// the unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharacterizedOutcome {
    /// The route emits a candidate and the oracle reports `pass`: no structural
    /// divergence, no mapping divergence, and no other reason at all.
    EmitsAndPasses,
    /// The route emits a candidate and the oracle reports `fail`, with at least
    /// one reason mentioning each named family and NO reason mentioning any
    /// other.
    ///
    /// Currently carried by no client cell — the source-map anchor fix closed
    /// the last cell that had one (`props-events.svelte`). The variant stays
    /// for the same reason `Refuses` does: a failing-but-emitting outcome
    /// remains a legal recorded state, and it's what turns a silently
    /// reopened divergence back into a failure here rather than a pass.
    #[allow(
        dead_code,
        reason = "an emit-and-fail outcome is a legal recorded outcome; no reachable client \
                  request has one today"
    )]
    EmitsAndFails(EmittedDivergences),
    /// The route refuses the runtime surface with this typed code.
    ///
    /// Currently carried by no client cell — every reachable client request
    /// emits. The variant stays because a refusal remains a legal outcome the
    /// gate must be able to record, and because the `Emitted`-where-`Refuses`
    /// arm is what turns a silently corrected refusal into a failure rather
    /// than a pass.
    #[allow(
        dead_code,
        reason = "a refusal is a legal recorded outcome; no reachable client request has one today"
    )]
    Refuses { diagnostic_code: &'static str },
}

/// The characterized current state of each reachable client request.
///
/// This is a measurement, not a target. Every entry has a matching conformance
/// target below stating what the official compiler does instead.
fn characterized_client_outcome(fixture_path: &str) -> CharacterizedOutcome {
    match fixture_path {
        "fixtures/svelte/basic-runes.svelte" => CharacterizedOutcome::EmitsAndPasses,
        "fixtures/svelte/legacy-slots.svelte" => CharacterizedOutcome::EmitsAndPasses,
        // The component emits, its instance-script prop reads match the official
        // accessor shapes, and both of its map anchors — a script FUNCTION
        // declaration and a shorthand attribute binding — now carry authored
        // provenance, characterized by
        // `a_function_declaration_carries_its_authored_name_provenance` and
        // `a_shorthand_attribute_binding_carries_its_authored_name_provenance`
        // in the compiler crate.
        "fixtures/svelte/props-events.svelte" => CharacterizedOutcome::EmitsAndPasses,
        other => panic!("no characterized outcome for the reachable request `{other}`"),
    }
}

/// The divergence families a cell's oracle report is recorded to carry.
#[derive(Debug, Clone, Copy)]
struct RecordedDivergences {
    structural: bool,
    mapping: bool,
}

/// A short label for a cell's reachability, for the per-cell record.
fn reachability_label(reachability: &RequestReachability) -> &'static str {
    match reachability {
        RequestReachability::Reachable => "reachable",
        RequestReachability::NoPublicSpelling { .. } => "no-spelling",
    }
}

/// The divergence families recorded for one cell.
///
/// A cell with NO public spelling additionally diverges structurally because
/// its golden describes dev-instrumented output no public request can ask for
/// — the divergence is attributed to that inexpressible axis, not to the
/// compiler. Attribution is the reachability classification's job; driving the
/// cell is still mandatory.
fn recorded_divergences(
    fixture_path: &str,
    reachability: &RequestReachability,
) -> RecordedDivergences {
    let base = characterized_client_outcome(fixture_path);
    let structural = match base {
        CharacterizedOutcome::EmitsAndFails(divergences) => divergences.structural(),
        CharacterizedOutcome::EmitsAndPasses | CharacterizedOutcome::Refuses { .. } => false,
    };
    let mapping = match base {
        CharacterizedOutcome::EmitsAndFails(divergences) => divergences.mapping(),
        CharacterizedOutcome::EmitsAndPasses | CharacterizedOutcome::Refuses { .. } => false,
    };
    match reachability {
        RequestReachability::Reachable => RecordedDivergences {
            structural,
            mapping,
        },
        // The dev-instrumented golden always differs structurally from the
        // production-shaped candidate the only expressible request produces.
        RequestReachability::NoPublicSpelling { .. } => RecordedDivergences {
            structural: true,
            mapping,
        },
    }
}

/// The refusal the server arm produces today, decided at
/// `crates/verter_compiler/src/svelte/runtime/client_compile.rs:113-119`.
const SERVER_REFUSAL_CODE: &str = "svelte-runtime-unsupported-server-generate";
const SERVER_REFUSAL_MESSAGE: &str =
    "Svelte client emission does not yet support server-side rendering (`generate: 'server'`).";

// The reachable inventory this gate is defined over

/// The goldens no public/default route can request are classified out of this
/// gate, and the gate's own inventory is exactly the reachable complement.
#[test]
fn the_gate_runs_over_the_reachable_requests_and_no_others() {
    let reachable_client = reachable_client_requests();
    let reachable_server = reachable_server_requests();
    let unreachable = unreachable_goldens();

    assert_eq!(
        reachable_client.len(),
        3,
        "reachable client requests: {:?}",
        reachable_client
            .iter()
            .map(|cell| cell.golden_name.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(reachable_server.len(), 3, "reachable server requests");
    assert_eq!(
        unreachable.len(),
        6,
        "goldens with no public spelling: {:?}",
        unreachable
            .iter()
            .map(|(cell, _)| cell.golden_name.clone())
            .collect::<Vec<_>>()
    );

    // Every request this gate drives is reachable, and every fixture is
    // represented exactly once per target — so collapsing the `dev` arms lost
    // no distinct request.
    let fixtures: BTreeSet<String> = reachable_client
        .iter()
        .map(|cell| cell.fixture_path.clone())
        .collect();
    assert_eq!(
        fixtures.len(),
        reachable_client.len(),
        "two reachable client requests share a fixture, so they are not distinct requests"
    );
    for cell in reachable_client.iter().chain(reachable_server.iter()) {
        assert_eq!(
            cell.reachability(),
            RequestReachability::Reachable,
            "{}: a non-reachable golden entered the gate's inventory",
            cell.golden_name
        );
        assert_eq!(
            cell.dev, DEV_AXIS_IMPLICIT_VALUE,
            "{}: a reachable request must carry the implicit value of every inexpressible axis",
            cell.golden_name
        );
    }
    // And nothing the gate drives is in the out-of-inventory set.
    let driven: BTreeSet<String> = reachable_client
        .iter()
        .chain(reachable_server.iter())
        .map(|cell| cell.golden_name.clone())
        .collect();
    for (cell, _) in &unreachable {
        assert!(
            !driven.contains(&cell.golden_name),
            "{}: an out-of-inventory golden is being driven as a request",
            cell.golden_name
        );
    }
}

// The gate

/// Every axis genuinely runs for every reachable client request, and each
/// request's outcome is exactly the characterized one.
///
/// GREEN and discriminating in BOTH directions: a deepening defect changes the
/// reasons and fails here, and a silent correction changes them too and also
/// fails here — at which point the matching `#[ignore]`d conformance target
/// below is what proves the correction is complete.
#[test]
fn every_committed_client_cell_is_driven_and_reaches_its_recorded_outcome() {
    let cells = svelte_client_cells();
    assert_eq!(cells.len(), 6, "committed client cells");

    let mut problems = Vec::new();
    let mut summary = Vec::new();
    let mut evidence = serde_json::Map::new();

    let pinned_domain = pinned_svelte_domain();
    assert_eq!(
        pinned_domain["packageVersion"].as_str(),
        Some(SVELTE_PINNED_PACKAGE_VERSION),
        "the goldens these cells are compared against are not the pinned official version"
    );

    for cell in &cells {
        let reachability = cell.reachability();
        let expected = characterized_client_outcome(&cell.fixture_path);
        // Each cell's own record must name the pinned domain: a cell compared
        // against a differently-pinned golden is not this cell.
        assert_eq!(
            read_svelte_golden_record(&cell.golden_name).get("domain"),
            Some(&pinned_domain),
            "{}: this cell's golden names a different official domain than the pinned one",
            cell.golden_name
        );
        let outcome = cell.compile_through_shipped_route();

        // Every cell — reachable or not — is DRIVEN through the shipped route
        // and the oracle. The reachability classification below is the recorded
        // REASON a non-reachable cell's divergence is not attributed to the
        // compiler; it is never a reason to leave a cell without a result.
        let (code, source_map) = match (&outcome, expected) {
            (
                SvelteRouteOutcome::Refused {
                    diagnostic_code,
                    message,
                },
                CharacterizedOutcome::Refuses {
                    diagnostic_code: expected_code,
                },
            ) => {
                // The refusal IS this cell's outcome: request, route, profile,
                // typed refusal, and the explicit fact that no candidate exists
                // to hand the comparator.
                summary.push(format!(
                    "{:<44} {:<9} REFUSED {diagnostic_code}",
                    cell.golden_name,
                    reachability_label(&reachability),
                ));
                evidence.insert(
                    cell.golden_name.clone(),
                    json!({
                        "reachability": reachability_label(&reachability),
                        "outcome": "refused",
                        "diagnosticCode": diagnostic_code,
                        "message": message,
                        "comparatorRun": false,
                        "comparatorSkipReason":
                            "the shipped route published no candidate, so there is nothing to \
                             compare against the golden",
                    }),
                );
                if diagnostic_code != expected_code {
                    problems.push(format!(
                        "── {} ──\n  refused with `{diagnostic_code}`, recorded as \
                         `{expected_code}`",
                        cell.golden_name
                    ));
                }
                continue;
            }
            (
                SvelteRouteOutcome::Refused {
                    diagnostic_code, ..
                },
                _,
            ) => {
                problems.push(format!(
                    "── {} ──\n  the route refused (`{diagnostic_code}`) but this cell is \
                     recorded as emitting a candidate",
                    cell.golden_name
                ));
                continue;
            }
            (SvelteRouteOutcome::MissingNode, _) => {
                problems.push(format!(
                    "── {} ──\n  no `Main` node and no refusal signal",
                    cell.golden_name
                ));
                continue;
            }
            (SvelteRouteOutcome::Emitted { .. }, CharacterizedOutcome::Refuses { .. }) => {
                problems.push(format!(
                    "── {} ──\n  the route emitted a candidate but this cell is recorded as \
                     refused — if the refusal was corrected, un-ignore its conformance target",
                    cell.golden_name
                ));
                continue;
            }
            (SvelteRouteOutcome::Emitted { code, source_map }, _) => {
                (code.clone(), source_map.clone())
            }
        };

        let report = check_candidate(&cell.golden_name, &code, source_map.as_deref());

        for axis in REQUIRED_AXES {
            match report.axes.get(*axis) {
                Some((status, _)) if status == "ran" => {}
                Some((status, reason)) => problems.push(format!(
                    "── {} ──\n  axis `{axis}` reported `{status}` instead of `ran` \
                     (reason: {reason:?})",
                    cell.golden_name
                )),
                None => problems.push(format!(
                    "── {} ──\n  axis `{axis}` is absent from the report",
                    cell.golden_name
                )),
            }
        }
        match report.axes.get("runtime") {
            Some((status, _)) if status == "ran" || status == "not-applicable" => {}
            other => problems.push(format!(
                "── {} ──\n  axis `runtime` reported {other:?} instead of \
                 `ran`/`not-applicable`",
                cell.golden_name
            )),
        }

        // The recorded divergence families for THIS cell. A cell with no public
        // spelling carries the extra `dev-instrumentation` family: its golden
        // describes dev-instrumented output that no public request can ask for,
        // so its structural divergence is attributed to the inexpressible axis
        // rather than to the compiler.
        let expects = recorded_divergences(&cell.fixture_path, &reachability);
        let saw_structural = report
            .reasons
            .iter()
            .any(|reason| reason.contains("structural divergence"));
        let saw_mapping = report
            .reasons
            .iter()
            .any(|reason| reason.contains("not truthful about its own output"));
        if saw_structural != expects.structural {
            problems.push(format!(
                "── {} ──\n  structural divergence {} (recorded: {})\n  reasons={:?}",
                cell.golden_name,
                if saw_structural { "present" } else { "absent" },
                expects.structural,
                report.reasons
            ));
        }
        if saw_mapping != expects.mapping {
            problems.push(format!(
                "── {} ──\n  mapping divergence {} (recorded: {})\n  reasons={:?}",
                cell.golden_name,
                if saw_mapping { "present" } else { "absent" },
                expects.mapping,
                report.reasons
            ));
        }
        // The oracle's own VERDICT must agree with the recorded families. This
        // is what makes an "emits and passes" record mean what its name says: a
        // cell recorded with no divergence family must reach `pass`, and one
        // recorded with a family must reach `fail`. Comparing only the family
        // booleans left both readings open.
        let expected_verdict = if expects.structural || expects.mapping {
            "fail"
        } else {
            "pass"
        };
        if report.verdict != expected_verdict {
            problems.push(format!(
                "── {} ──\n  the oracle reported verdict `{}` where the recorded families \
                 (structural={}, mapping={}) demand `{expected_verdict}`\n  reasons={:?}",
                cell.golden_name,
                report.verdict,
                expects.structural,
                expects.mapping,
                report.reasons
            ));
        }
        for reason in &report.reasons {
            let known = reason.contains("structural divergence")
                || reason.contains("not truthful about its own output");
            if !known {
                problems.push(format!(
                    "── {} ──\n  an unrecorded divergence appeared: {reason}",
                    cell.golden_name
                ));
            }
        }

        summary.push(format!(
            "{:<44} {:<9} exit={:<3} verdict={:<6} {}",
            cell.golden_name,
            reachability_label(&reachability),
            report.exit_code.unwrap_or(-1),
            report.verdict,
            axis_table(&report),
        ));
        let record = read_svelte_golden_record(&cell.golden_name);
        evidence.insert(
            cell.golden_name.clone(),
            json!({
                "reachability": reachability_label(&reachability),
                "outcome": "emitted",
                "comparatorRun": true,
                "verdict": report.verdict,
                "reasons": report.reasons,
                "axes": report
                    .axes
                    .iter()
                    .map(|(name, (status, _))| (name.clone(), Value::from(status.clone())))
                    .collect::<serde_json::Map<String, Value>>(),
                "candidateCode": code,
                "candidateMap": source_map,
                "goldenCode": record.get("code"),
                "goldenMap": record.get("map"),
            }),
        );
    }

    assert_eq!(
        evidence.len(),
        cells.len(),
        "{} of {} committed client cells produced no recorded outcome",
        cells.len() - evidence.len(),
        cells.len()
    );

    // The full per-cell evidence goes to STDOUT, not to a scratch file: it is a
    // reporting convenience, no assertion reads it back, and a test that needs
    // no real file should not touch the filesystem to produce one. Capture it
    // with `-- --nocapture` and redirect.
    let evidence_json =
        serde_json::to_string_pretty(&Value::Object(evidence)).expect("the evidence serializes");

    println!(
        "Svelte official-conformance gate, {} committed client cells:\n{}\nfull per-cell \
         evidence: {}",
        cells.len(),
        summary.join("\n"),
        evidence_json
    );

    assert!(
        problems.is_empty(),
        "{} committed client cell(s) departed from their recorded outcome:\n\n{}",
        problems.len(),
        problems.join("\n\n")
    );
}

// The server arm, characterized as it stands

/// What the shipped route returns TODAY for every reachable server request.
///
/// The refusal is decided in `compile_client`'s step (0)
/// (`crates/verter_compiler/src/svelte/runtime/client_compile.rs:113-119`),
/// which returns `ClientCompileError::Unsupported(ServerGenerate)` before any
/// other pipeline stage. The carrier turns that into a product-free
/// `CarrierCompileOutcome::RuntimeSurfaceRefused` carrying the reason
/// structurally (`crates/verter_compiler/src/svelte/carrier.rs`), and the host
/// reads that typed refusal straight onto `HostError::RuntimeSurfaceRefused`
/// with its per-surface code and message
/// (`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`).
///
/// This test only RECORDS that behaviour. It adds nothing to the refusal.
#[test]
fn every_committed_server_cell_is_refused_by_the_shipped_route() {
    let requests = svelte_server_cells();
    assert_eq!(requests.len(), 6, "committed server cells");

    let pinned_domain = pinned_svelte_domain();
    let mut recorded = Vec::new();
    for cell in &requests {
        assert_eq!(
            read_svelte_golden_record(&cell.golden_name).get("domain"),
            Some(&pinned_domain),
            "{}: this cell's golden names a different official domain than the pinned one",
            cell.golden_name
        );
        let outcome = cell.compile_through_shipped_route();
        recorded.push(format!(
            "{:<44} {:<11} {outcome:?}",
            cell.golden_name,
            reachability_label(&cell.reachability())
        ));
        match outcome {
            SvelteRouteOutcome::Refused {
                diagnostic_code,
                message,
            } => {
                assert_eq!(
                    diagnostic_code, SERVER_REFUSAL_CODE,
                    "{}: the server arm's refusal code changed",
                    cell.golden_name
                );
                assert_eq!(
                    message, SERVER_REFUSAL_MESSAGE,
                    "{}: the server arm's refusal message changed",
                    cell.golden_name
                );
            }
            other => panic!(
                "{}: the server arm no longer refuses — it returned {other:?}. This test records \
                 the refusal as it stands; a change here is a real behaviour change to report, \
                 not a test to relax.",
                cell.golden_name
            ),
        }
    }
    println!(
        "Svelte server cells, shipped-route outcome:\n{}",
        recorded.join("\n")
    );
}

// Per-defect characterization — green today, discriminating in both directions

/// Compile one reachable client request and return its emitted module bytes.
fn emitted_module(fixture_suffix: &str) -> (SvelteCell, String, String) {
    let cell = reachable_client_requests()
        .into_iter()
        .find(|cell| cell.fixture_path.ends_with(fixture_suffix))
        .unwrap_or_else(|| panic!("no reachable client request for `{fixture_suffix}`"));
    match cell.compile_through_shipped_route() {
        SvelteRouteOutcome::Emitted { code, source_map } => {
            let map = source_map.unwrap_or_else(|| {
                panic!(
                    "{}: the request asked for a map and got none",
                    cell.golden_name
                )
            });
            (cell, code, map)
        }
        other => panic!(
            "{}: expected an emitted module, got {other:?}",
            cell.golden_name
        ),
    }
}

/// The flags argument of the emitted `$.each(...)` call, read STRUCTURALLY.
///
/// The emitted module is parsed with the repository's own JavaScript parser
/// (`oxc_parser`) and walked as an AST: the walk resolves the LOCAL binding of
/// the `svelte/internal/client` namespace import, finds the call whose callee is
/// a static member `each` on THAT binding, and reads its second argument as a
/// numeric literal. Nothing here reads generated text — the callee identity
/// comes from the module's own import binding, and the value from a
/// `NumericLiteral` node.
fn each_flags_argument(code: &str) -> u32 {
    use oxc_ast::ast::{Argument, Expression, ImportDeclarationSpecifier, Statement};
    use oxc_ast_visit::Visit;

    let allocator = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&allocator, code, oxc_span::SourceType::mjs()).parse();
    assert!(
        !parsed.panicked && parsed.errors.is_empty(),
        "the emitted module does not parse, so no structural reading is possible: {:?}",
        parsed.errors
    );

    // The LOCAL name the module bound the client runtime namespace to. Resolved
    // from the import declaration, so the call below is identified by binding,
    // not by a spelling that happens to appear in the text.
    let mut namespace: Option<String> = None;
    for statement in &parsed.program.body {
        let Statement::ImportDeclaration(import) = statement else {
            continue;
        };
        if import.source.value.as_str() != SVELTE_CLIENT_RUNTIME_SPECIFIER {
            continue;
        }
        for specifier in import.specifiers.iter().flatten() {
            if let ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace_specifier) =
                specifier
            {
                namespace = Some(namespace_specifier.local.name.to_string());
            }
        }
    }
    let namespace = namespace.unwrap_or_else(|| {
        panic!(
            "the emitted module binds no namespace for `{SVELTE_CLIENT_RUNTIME_SPECIFIER}`, so \
             its `each` calls cannot be identified structurally"
        )
    });

    /// Collects every `<namespace>.each(...)` call's second argument.
    struct EachFlagsCollector<'a> {
        namespace: &'a str,
        found: Vec<f64>,
    }
    impl<'a, 'ast> Visit<'ast> for EachFlagsCollector<'a> {
        fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'ast>) {
            if let Expression::StaticMemberExpression(member) = &call.callee {
                if member.property.name.as_str() == "each" {
                    if let Expression::Identifier(object) = &member.object {
                        if object.name.as_str() == self.namespace {
                            let flags = call
                                .arguments
                                .get(1)
                                .unwrap_or_else(|| panic!("an `each` call has no second argument"));
                            let Argument::NumericLiteral(literal) = flags else {
                                panic!("the `each` call's flags argument is not a numeric literal");
                            };
                            self.found.push(literal.value);
                        }
                    }
                }
            }
            oxc_ast_visit::walk::walk_call_expression(self, call);
        }
    }

    let mut collector = EachFlagsCollector {
        namespace: &namespace,
        found: Vec::new(),
    };
    collector.visit_program(&parsed.program);
    assert_eq!(
        collector.found.len(),
        1,
        "the emitted module makes {} `each` calls, so this reading is ambiguous",
        collector.found.len()
    );
    let value = collector.found[0];
    assert!(
        value.fract() == 0.0 && value >= 0.0 && value <= f64::from(u32::MAX),
        "the `each` flags argument {value} is not a non-negative integer"
    );
    value as u32
}

/// Whether an emitted module imports the Svelte client runtime, read
/// STRUCTURALLY from its own import declarations rather than from its text.
fn imports_client_runtime(code: &str) -> bool {
    use oxc_ast::ast::Statement;

    let allocator = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&allocator, code, oxc_span::SourceType::mjs()).parse();
    parsed.program.body.iter().any(|statement| {
        matches!(statement, Statement::ImportDeclaration(import)
            if import.source.value.as_str() == SVELTE_CLIENT_RUNTIME_SPECIFIER)
    })
}

/// The module specifier the Svelte client backend imports its runtime namespace
/// from (`crates/verter_compiler/src/svelte/runtime/client_imports.rs`); the
/// structural reading above resolves the call's callee through THIS import's
/// local binding rather than through any spelling in the generated text.
const SVELTE_CLIENT_RUNTIME_SPECIFIER: &str = "svelte/internal/client";

/// `EACH_ITEM_REACTIVE`, from the pinned official compiler's own constants
/// (`packages/framework-conformance-harness/.oracle-checkouts/svelte/packages/svelte/src/constants.js:1`).
const EACH_ITEM_REACTIVE: u32 = 1;
/// `EACH_IS_CONTROLLED | EACH_ITEM_IMMUTABLE` — the flags the pinned official
/// compiler emits for `basic-runes.svelte`'s keyed `{#each}` in runes mode
/// (`constants.js:3` and `:5`; the golden's own emitted call is `$.each(ul, 20, …)`).
const OFFICIAL_KEYED_RUNES_EACH_FLAGS: u32 = 20;

/// Decode a candidate map's source-bearing segments into authored
/// `(line, column)` pairs, through the crate's own validating map reader.
fn source_bearing_positions(map: &str, code: &str) -> Vec<(u32, u32)> {
    let decoded = validate_and_decode(map, code)
        .unwrap_or_else(|error| panic!("the emitted map does not decode: {error:?}"));
    decoded
        .segments
        .iter()
        .filter_map(|segment| {
            segment
                .payload
                .map(|payload| (payload.source_line, payload.source_column))
        })
        .collect()
}

/// The exact authored coordinates the emitted client map carries, per reachable
/// emitting request.
///
/// Settled by DECODING the candidate's own emitted `mappings` through
/// [`validate_and_decode`] (the crate's validating map reader), not by reading
/// a count off a report. Every source-bearing segment is enumerated; the
/// authored coordinates are 0-based `(line, column)`.
///
/// It fails if a single segment is added, removed or moved, so it holds the map
/// in BOTH directions: a lost segment is a regression, and a segment appearing
/// somewhere unaccounted for is an unreviewed provenance claim. The required
/// anchors among these are separately demanded by
/// [`the_client_source_map_covers_every_required_authored_anchor`]; this pins
/// the WHOLE set, including the coordinates no anchor requires.
#[test]
fn the_client_source_map_currently_carries_only_these_authored_coordinates() {
    let mut observed: Vec<(String, Vec<(u32, u32)>)> = Vec::new();
    for suffix in ["basic-runes.svelte", "legacy-slots.svelte"] {
        let (cell, code, map) = emitted_module(suffix);
        let mut positions = source_bearing_positions(&map, &code);
        positions.sort_unstable();
        observed.push((cell.fixture_path.clone(), positions));
    }
    println!("emitted authored coordinates: {observed:#?}");

    let expected: Vec<(String, Vec<(u32, u32)>)> = vec![
        (
            "fixtures/svelte/basic-runes.svelte".to_string(),
            // (1,6) the `count` in `let count = $state(0);`; (2,2) the `let` of
            // the `items` statement; (6,7) the `count` in `{#if count > 0}`;
            // (13,11) the `item` interpolation in the each body.
            vec![(1, 6), (2, 2), (6, 7), (13, 11)],
        ),
        (
            "fixtures/svelte/legacy-slots.svelte".to_string(),
            // (1,13) the `title` in `export let title = "Untitled";`;
            // (6,25) the `title` interpolation inside the named slot.
            vec![(1, 13), (6, 25)],
        ),
    ];
    assert_eq!(
        observed, expected,
        "the emitted client map's authored coverage moved. If coverage improved, check \
         `the_client_source_map_covers_every_required_authored_anchor`."
    );
}

// Conformance targets — the official behaviour each genuine defect must reach

/// CONFORMANCE TARGET — the `{#each}` flags argument equals the official value.
///
/// For `{#each items as item (item)}` in runes mode the pinned official
/// `svelte@5.56.10` compiler emits `$.each(ul, 20, …)` —
/// `EACH_IS_CONTROLLED | EACH_ITEM_IMMUTABLE`, with `EACH_ITEM_REACTIVE` (bit
/// `1`, `.oracle-checkouts/svelte/packages/svelte/src/constants.js:1`) CLEAR.
/// The bit is the block's reactivity/effect topology, which the
/// compiled-output conformance rule holds in contract rather than treating as
/// cosmetic, and it is coupled to the item's read form: with it clear the
/// runtime hands the render callback the raw item.
///
/// The per-axis official predicate behind it is pinned in the compiler crate by
/// `each_item_reactivity_matches_the_official_predicate_on_every_axis`.
#[test]
fn each_flags_for_a_keyed_runes_each_match_the_official_compiler() {
    let (cell, code, _map) = emitted_module("basic-runes.svelte");
    let flags = each_flags_argument(&code);
    assert_eq!(
        flags & EACH_ITEM_REACTIVE,
        0,
        "{}: EACH_ITEM_REACTIVE is set in `$.each(_, {flags}, …)`; official emits \
         {OFFICIAL_KEYED_RUNES_EACH_FLAGS}",
        cell.golden_name
    );
    assert_eq!(
        flags, OFFICIAL_KEYED_RUNES_EACH_FLAGS,
        "{}: the flags argument must equal the official value",
        cell.golden_name
    );
}

/// CONFORMANCE TARGET — a component reading its `$props()` locals from the
/// instance script publishes a runtime module.
///
/// `props-events.svelte` reads its props from a function in the instance script
/// (`ontoggle?.(!disabled)`), which the pinned official `svelte@5.56.10` compiler
/// ACCEPTS. The shipped route must therefore publish a module for it, importing
/// the client runtime — the prop-usage gate refuses WRITES only.
///
/// The emitted read SHAPES are pinned against the official output in the
/// compiler crate by
/// `instance_script_prop_reads_lower_to_the_official_accessor_shapes`; this
/// asserts the public route publishes at all.
#[test]
fn a_runes_props_read_in_the_instance_script_compiles_to_a_runtime_module() {
    let cell = reachable_client_requests()
        .into_iter()
        .find(|cell| cell.fixture_path.ends_with("props-events.svelte"))
        .expect("the reachable inventory carries the props-events request");
    match cell.compile_through_shipped_route() {
        SvelteRouteOutcome::Emitted { code, .. } => {
            assert!(
                imports_client_runtime(&code),
                "{}: the published module does not import the client runtime:\n{code}",
                cell.golden_name
            );
        }
        other => panic!(
            "{}: the official compiler accepts this component, but the shipped route returned \
             {other:?}",
            cell.golden_name
        ),
    }
}

/// CONFORMANCE TARGET — the emitted client map covers every required authored
/// anchor, and the mapping oracle reports no violation at all.
///
/// Only fragments pushed as MAPPED code accumulate segments
/// (`crates/verter_compiler/src/svelte/runtime/output.rs`), so a producer that
/// writes plain text contributes no provenance. The anchors are the harness's
/// own, defined at
/// `packages/framework-conformance-harness/src/mapping-oracle.mjs`; the oracle
/// is candidate-relative and never compares against the official map, so this is
/// a requirement about the candidate's own truthfulness.
///
/// The second half is the one that cannot be satisfied by over-claiming:
/// mapping a generated-only token to an authored position would satisfy the
/// anchor list and then fail the oracle's provenance rules, which this test also
/// demands are clean.
#[test]
fn the_client_source_map_covers_every_required_authored_anchor() {
    // The required anchors, mirrored from the harness's own definitions
    // (`src/mapping-oracle.mjs:603-661`). 0-based authored `(line, column)`.
    let required: &[(&str, &[(u32, u32)])] = &[
        ("basic-runes.svelte", &[(1, 6), (6, 7)]),
        ("legacy-slots.svelte", &[(1, 13), (6, 25)]),
    ];
    let mut missing = Vec::new();
    for (suffix, anchors) in required {
        let (cell, code, map) = emitted_module(suffix);
        let positions = source_bearing_positions(&map, &code);
        for anchor in *anchors {
            if !positions.contains(anchor) {
                missing.push(format!(
                    "{}: no segment maps to authored {anchor:?} (emitted: {positions:?})",
                    cell.golden_name
                ));
            }
        }
        // The oracle's own verdict on the same candidate must carry no mapping
        // violation either — the anchor check above is the specific half, this
        // is the whole rule set.
        let report = check_candidate(&cell.golden_name, &code, Some(&map));
        if report
            .reasons
            .iter()
            .any(|reason| reason.contains("not truthful about its own output"))
        {
            missing.push(format!(
                "{}: the mapping oracle still reports violations: {:?}",
                cell.golden_name, report.reasons
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "required authored anchors are unmapped:\n{}",
        missing.join("\n")
    );
}

// Mutation-discrimination — the oracle behind this gate is not a stub

/// The pristine baseline for the plants below: the golden's OWN recorded
/// artifact, which is the candidate proven to reach `verdict: "pass"`.
fn pristine_baseline(golden_name: &str) -> (String, String) {
    let record = read_svelte_golden_record(golden_name);
    let code = record
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{golden_name}: the record carries no `code` string"))
        .to_string();
    let map = record
        .get("map")
        .filter(|map| !map.is_null())
        .unwrap_or_else(|| panic!("{golden_name}: the record carries no `map`"))
        .to_string();
    (code, map)
}

/// Prove a plant APPLIED: the planted token must be ABSENT from the pristine
/// bytes and present EXACTLY ONCE afterwards, and the two strings must differ.
/// An exit code is never proof a mutation landed, and a search that hits a
/// pre-existing occurrence is a false positive.
#[track_caller]
fn assert_plant_applied(family: &str, pristine: &str, mutated: &str, token: &str) {
    assert_ne!(
        pristine, mutated,
        "the {family} plant did not change the candidate bytes"
    );
    assert_eq!(
        pristine.matches(token).count(),
        0,
        "the {family} plant's marker `{token}` already occurs in the pristine bytes, so finding \
         it afterwards would prove nothing"
    );
    assert_eq!(
        mutated.matches(token).count(),
        1,
        "the {family} plant's marker `{token}` is not present exactly once after planting"
    );
}

#[test]
fn the_gate_detects_a_planted_defect_on_every_applicable_axis_family() {
    // A runes client request whose golden records a populated map, so every
    // applicable axis is genuinely exercised by the same baseline every plant
    // starts from.
    let base = reachable_client_requests()
        .into_iter()
        .find(|cell| cell.runes && cell.fixture_path.ends_with("basic-runes.svelte"))
        .expect("the reachable inventory carries the basic-runes client request");
    let (pristine_code, pristine_map) = pristine_baseline(&base.golden_name);

    // Unplanted control: the pristine baseline must pass before any plant can be
    // trusted to discriminate. Every plant below is judged against THIS run.
    let control = check_candidate(&base.golden_name, &pristine_code, Some(&pristine_map));
    assert_eq!(
        control.verdict, "pass",
        "the unplanted control must pass before any plant can be trusted to discriminate: {:?}",
        control.reasons
    );

    // parse: corrupt the candidate into invalid JavaScript
    let parse_mutant = format!("{pristine_code}\nconst )(( = ;;;");
    assert_plant_applied("parse", &pristine_code, &parse_mutant, "const )(( = ;;;");
    let parse_report = check_candidate(&base.golden_name, &parse_mutant, Some(&pristine_map));
    assert_eq!(parse_report.verdict, "fail", "parse plant was not detected");
    assert!(
        parse_report
            .reasons
            .iter()
            .any(|reason| reason.contains("failed to parse")),
        "parse plant's reasons do not name parsing: {:?}",
        parse_report.reasons
    );

    // link: retarget a real import outside the pinned closure
    const NONEXISTENT: &str = "verter-gate-test-nonexistent-package-xyz";
    assert!(
        pristine_code.contains("'svelte/internal/client'"),
        "the baseline must import the client runtime for the link plant to have a target:\n{}",
        &pristine_code[..pristine_code.len().min(400)]
    );
    let link_mutant =
        pristine_code.replacen("'svelte/internal/client'", &format!("'{NONEXISTENT}'"), 1);
    assert_plant_applied("link", &pristine_code, &link_mutant, NONEXISTENT);
    let link_report = check_candidate(&base.golden_name, &link_mutant, Some(&pristine_map));
    assert_eq!(link_report.verdict, "fail", "link plant was not detected");
    assert!(
        link_report
            .reasons
            .iter()
            .any(|reason| reason.contains("unresolved imports")
                || reason.contains("outside the pinned closures")),
        "link plant's reasons do not name link: {:?}",
        link_report.reasons
    );

    // structural: rename the runtime namespace binding
    const RENAMED: &str = "$renamedByPlant";
    let structural_mutant = pristine_code.replacen(
        "import * as $ from",
        &format!("import * as {RENAMED} from"),
        1,
    );
    assert_plant_applied("structural", &pristine_code, &structural_mutant, RENAMED);
    let structural_report =
        check_candidate(&base.golden_name, &structural_mutant, Some(&pristine_map));
    assert_eq!(
        structural_report.verdict, "fail",
        "structural plant was not detected"
    );
    assert!(
        structural_report
            .reasons
            .iter()
            .any(|reason| reason.contains("structural divergence")),
        "structural plant's reasons do not name structural divergence: {:?}",
        structural_report.reasons
    );

    // ---- diagnostics: candidate claims a diagnostic the golden does not ----
    //
    // The plant lives in the candidate ENVELOPE rather than in the code or the
    // map, so its applied-proof is taken over the two envelope strings — the
    // exact bytes the CLI receives — the same way every other plant proves
    // itself.
    const DIAGNOSTIC_MARKER: &str = "GATE-TEST-PLANT";
    let pristine_diagnostics = json!([]);
    let planted_diagnostics = json!([{
        "kind": "warning",
        "code": DIAGNOSTIC_MARKER,
        "message": "planted diagnostic",
        "source": base.fixture_path,
        "start": null,
        "end": null,
    }]);
    let pristine_envelope =
        candidate_envelope(&pristine_code, &pristine_map, &pristine_diagnostics);
    let planted_envelope = candidate_envelope(&pristine_code, &pristine_map, &planted_diagnostics);
    assert_plant_applied(
        "diagnostics",
        &pristine_envelope,
        &planted_envelope,
        DIAGNOSTIC_MARKER,
    );
    let diagnostics_report = check_candidate_envelope(&base.golden_name, &planted_envelope);
    assert_eq!(
        diagnostics_report.verdict, "fail",
        "diagnostics plant was not detected"
    );
    assert!(
        diagnostics_report
            .reasons
            .iter()
            .any(|reason| reason.contains("diagnostics diverge")),
        "diagnostics plant's reasons do not name diagnostics: {:?}",
        diagnostics_report.reasons
    );

    // mapping (content integrity): corrupt sourcesContent
    const NOT_THE_FIXTURE: &str = "GATE-TEST-PLANT: this is not the authored fixture's content";
    let mut map_json: Value = serde_json::from_str(&pristine_map).expect("the map is JSON");
    let sources_content = map_json
        .get_mut("sourcesContent")
        .and_then(Value::as_array_mut)
        .expect("the map declares sourcesContent");
    assert!(
        !sources_content.is_empty(),
        "no sourcesContent row to corrupt"
    );
    sources_content[0] = json!(NOT_THE_FIXTURE);
    let mapping_mutant_map = map_json.to_string();
    assert_plant_applied(
        "mapping-content",
        &pristine_map,
        &mapping_mutant_map,
        NOT_THE_FIXTURE,
    );
    let mapping_report =
        check_candidate(&base.golden_name, &pristine_code, Some(&mapping_mutant_map));
    assert_eq!(
        mapping_report.verdict, "fail",
        "mapping content plant was not detected"
    );
    assert!(
        mapping_report
            .reasons
            .iter()
            .any(|reason| reason.contains("not truthful about its own output")),
        "mapping content plant's reasons do not name the mapping oracle: {:?}",
        mapping_report.reasons
    );
    assert!(
        mapping_report
            .axes
            .get("mapping")
            .is_some_and(|(status, _)| status == "ran"),
        "the mapping plant must still show the mapping axis genuinely ran, not skipped"
    );

    // ---- mapping (anchor coverage): remove exactly the anchor's segments ---
    //
    // The content-integrity plant above only proves the axis catches a blatantly
    // wrong `sourcesContent`; a broken or disabled anchor checker would still
    // pass it. This plant targets the anchor machinery the `legacy-slots`
    // finding rests on: it decodes the golden's OWN passing map, removes exactly
    // the segments that satisfy the `script-count-declaration` anchor (authored
    // line 1, column 6 — `src/mapping-oracle.mjs:603-613`), re-encodes, and
    // requires the mapping axis to report that anchor.
    const ANCHOR: (u32, u32) = (1, 6);
    let pristine_decoded =
        validate_and_decode(&pristine_map, &pristine_code).expect("the golden's own map decodes");
    let kept: Vec<super::InSeg> = pristine_decoded
        .segments
        .iter()
        .filter(|segment| {
            segment
                .payload
                .is_none_or(|payload| (payload.source_line, payload.source_column) != ANCHOR)
        })
        .map(|segment| {
            (
                segment.generated_line,
                segment.generated_column,
                segment.payload.map(|payload| {
                    (
                        payload.source_index,
                        payload.source_line,
                        payload.source_column,
                        payload.name_index,
                    )
                }),
            )
        })
        .collect();
    let removed = pristine_decoded.segments.len() - kept.len();
    assert!(
        removed > 0,
        "the anchor plant removed no segment, so the golden's map does not satisfy the anchor \
         it is supposed to and this plant proves nothing"
    );
    let mut anchor_map: Value = serde_json::from_str(&pristine_map).expect("the map is JSON");
    anchor_map["mappings"] = json!(encode_mappings(&kept));
    let anchor_mutant_map = anchor_map.to_string();
    // Applied-proof, in the terms this plant operates in: the mappings string
    // differs, and the anchor position is present before and absent after.
    assert_ne!(
        anchor_mutant_map, pristine_map,
        "the anchor plant did not change the map bytes"
    );
    let before: Vec<(u32, u32)> = source_bearing_positions(&pristine_map, &pristine_code);
    let after: Vec<(u32, u32)> = source_bearing_positions(&anchor_mutant_map, &pristine_code);
    assert!(
        before.contains(&ANCHOR),
        "the anchor position is absent from the pristine map, so removing it proves nothing"
    );
    assert!(
        !after.contains(&ANCHOR),
        "the anchor plant did not remove the anchor's segments"
    );
    assert_eq!(
        after.len() + removed,
        before.len(),
        "the anchor plant removed something other than exactly the anchor's segments"
    );

    let anchor_report =
        check_candidate(&base.golden_name, &pristine_code, Some(&anchor_mutant_map));
    assert_eq!(
        anchor_report.verdict, "fail",
        "anchor-coverage plant was not detected: {:?}",
        anchor_report.reasons
    );
    assert!(
        anchor_report
            .reasons
            .iter()
            .any(|reason| reason.contains("script-count-declaration")),
        "the anchor plant's reasons do not name the anchor it removed: {:?}",
        anchor_report.reasons
    );
    assert!(
        anchor_report
            .axes
            .get("mapping")
            .is_some_and(|(status, _)| status == "ran"),
        "the anchor plant must still show the mapping axis genuinely ran, not skipped"
    );

    // Control re-run: the pristine baseline is still green AFTER every plant, so
    // no plant leaked into shared harness state.
    let control_after = check_candidate(&base.golden_name, &pristine_code, Some(&pristine_map));
    assert_eq!(
        control_after.verdict, "pass",
        "the control stopped passing after the plants ran: {:?}",
        control_after.reasons
    );
}

/// The RUNTIME axis discriminates too: a candidate that MOUNTS but renders the
/// wrong markup is caught.
///
/// The plants above all run through the oracle CLI, which reports the runtime
/// axis `not-applicable` for a Svelte client golden; the runtime comparison is
/// the separate mount performed by [`compare_mounted_render`], and this proves
/// THAT comparison is not a stub. The planted defect is deliberately one the
/// official runtime accepts — the module still mounts — so what fails is the
/// rendered markup and nothing else. A plant that merely crashed the mount
/// would only prove the comparison notices a broken module.
#[test]
fn the_runtime_comparison_detects_a_planted_wrong_render() {
    let base = reachable_client_requests()
        .into_iter()
        .find(|cell| cell.runes && cell.fixture_path.ends_with("basic-runes.svelte"))
        .expect("the reachable inventory carries the basic-runes client request");
    // The same baseline every other plant starts from: the golden's OWN
    // recorded artifact.
    let (pristine_code, _) = pristine_baseline(&base.golden_name);

    // The markup this suite pins for THIS golden, so the plant can be judged
    // against the pin as well as against the golden mount.
    let pin_prefix = format!("{} => ", base.golden_name);
    let pinned_markup = CLIENT_RENDERED_MARKUP
        .lines()
        .find_map(|line| line.strip_prefix(&pin_prefix))
        .unwrap_or_else(|| {
            panic!(
                "{}: the pinned client markup records no line for this golden",
                base.golden_name
            )
        });

    // Unplanted control FIRST: the pristine artifact mounted against itself
    // must agree, on the pinned runtime, before any plant below can be trusted.
    let control = compare_mounted_render(
        &format!("{}-runtime-control", base.golden_name),
        &pristine_code,
        &pristine_code,
    );
    assert_eq!(
        control.runtime_version, SVELTE_PINNED_PACKAGE_VERSION,
        "the control did not run against the pinned runtime"
    );
    assert!(
        control.golden_ok,
        "the control's golden module did not mount: {}",
        control.golden_error
    );
    assert!(
        control.candidate_ok,
        "the control's candidate module did not mount: {}",
        control.candidate_error
    );
    assert!(
        control.divergence.is_none(),
        "the unplanted control diverged, so no plant below could be trusted to discriminate: {:?}",
        control.divergence
    );
    assert_eq!(
        control.candidate_html, pinned_markup,
        "the control rendered markup other than the markup this suite pins for it"
    );

    // runtime: render the wrong markup, but still mount
    //
    // `root_1` is the template the `alternate` branch instantiates, and `count`
    // starts at 0, so this is markup the control run above actually rendered.
    // Retemplating it is invisible to every other axis's oracle and to the
    // module's structure — only the MOUNT sees it.
    const RUNTIME_MARKER: &str = "GATE-RUNTIME-PLANT";
    const PRISTINE_TEMPLATE: &str = "<p>zero</p>";
    assert!(
        control.candidate_html.contains(PRISTINE_TEMPLATE),
        "the control did not render `{PRISTINE_TEMPLATE}`, so retemplating it would not change \
         what the candidate renders: {}",
        control.candidate_html
    );
    let planted_code =
        pristine_code.replacen(PRISTINE_TEMPLATE, &format!("<p>{RUNTIME_MARKER}</p>"), 1);
    assert_plant_applied("runtime", &pristine_code, &planted_code, RUNTIME_MARKER);

    let planted = compare_mounted_render(
        &format!("{}-runtime-plant", base.golden_name),
        &planted_code,
        &pristine_code,
    );
    assert!(
        planted.golden_ok,
        "the planted run's golden module did not mount, so it decides nothing: {}",
        planted.golden_error
    );
    // THE POINT OF THIS PLANT: the defect is a WRONG RENDER, not a crash.
    assert!(
        planted.candidate_ok,
        "the planted candidate failed to MOUNT, so this proves only that the comparison notices a \
         broken module — not that it notices a module which renders the wrong markup: {}",
        planted.candidate_error
    );
    assert!(
        planted.divergence.is_some(),
        "the runtime comparison did not detect the planted wrong render; candidate rendered {:?}, \
         golden rendered {:?}",
        planted.candidate_html,
        planted.golden_html
    );
    assert!(
        planted.candidate_html.contains(RUNTIME_MARKER),
        "the planted candidate did not render the planted marker, so the plant never reached the \
         mount: {}",
        planted.candidate_html
    );
    assert!(
        !planted.golden_html.contains(RUNTIME_MARKER),
        "the plant leaked into the golden module's render: {}",
        planted.golden_html
    );
    assert_ne!(
        planted.candidate_html, planted.golden_html,
        "the planted candidate rendered exactly what the golden rendered"
    );
    // The pinned markup is a second, independent catch of the same defect.
    assert_ne!(
        planted.candidate_html, pinned_markup,
        "the pinned client markup would not have caught the planted wrong render either"
    );

    // Control re-run: the pristine baseline still agrees AFTER the plant, so no
    // plant leaked into shared harness state.
    let control_after = compare_mounted_render(
        &format!("{}-runtime-control-after", base.golden_name),
        &pristine_code,
        &pristine_code,
    );
    assert!(
        control_after.divergence.is_none(),
        "the control stopped agreeing after the plant ran: {:?}",
        control_after.divergence
    );
    assert_eq!(
        control_after.candidate_html, pinned_markup,
        "the control's render moved after the plant ran"
    );
}

/// The exact candidate-envelope bytes the CLI receives.
fn candidate_envelope(code: &str, map: &str, diagnostics: &Value) -> String {
    json!({
        "code": code,
        "map": serde_json::from_str::<Value>(map).expect("the map is JSON"),
        "diagnostics": diagnostics,
    })
    .to_string()
}

/// Drive `check-candidate.mjs --authoritative` over a pre-built candidate
/// envelope. The shared [`check_candidate`] always sends an empty `diagnostics`
/// array; this is the one caller that needs to vary it, so it writes the
/// envelope itself rather than widening that helper's single shape.
fn check_candidate_envelope(golden_name: &str, envelope: &str) -> CellReport {
    use super::bf2_seed_matrix::{harness_root, run_bounded, TempCandidate, ORACLE_TIMEOUT};

    let candidate = TempCandidate::write(golden_name, envelope);
    let mut command = Command::new("node");
    command
        .arg(harness_root().join("bin/check-candidate.mjs"))
        .arg("--golden")
        .arg(golden_name)
        .arg("--candidate")
        .arg(&candidate.path)
        .arg("--authoritative")
        .current_dir(harness_root());
    let finished = run_bounded(&mut command, ORACLE_TIMEOUT);
    assert!(
        !finished.timed_out,
        "{golden_name}: the envelope run timed out.\nstderr:\n{}",
        finished.stderr
    );
    let report: Value = serde_json::from_str(&finished.stdout).unwrap_or_else(|error| {
        panic!(
            "{golden_name}: the envelope run produced no JSON report ({error}).\n\
             stdout:\n{}\nstderr:\n{}",
            finished.stdout, finished.stderr
        )
    });
    CellReport {
        exit_code: finished.code,
        verdict: report
            .get("verdict")
            .and_then(Value::as_str)
            .unwrap_or("<absent>")
            .to_string(),
        reasons: report
            .get("reasons")
            .and_then(Value::as_array)
            .map(|reasons| {
                reasons
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        axes: report
            .get("axes")
            .and_then(Value::as_object)
            .map(|axes| {
                axes.iter()
                    .map(|(name, state)| {
                        (
                            name.clone(),
                            (
                                state
                                    .get("status")
                                    .and_then(Value::as_str)
                                    .unwrap_or("<absent>")
                                    .to_string(),
                                state
                                    .get("reason")
                                    .and_then(Value::as_str)
                                    .map(ToString::to_string),
                            ),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

// The committed per-cell record

/// The committed, machine-checkable per-cell record
/// (`crates/verter_session/src/svelte_conformance_cell_record.json`).
///
/// FACTS ONLY: the exact request, the route it takes, the profile axes, the
/// products the route can publish, the pinned official domain the golden was
/// generated against, and the observed outcome. It records no correction owner,
/// no acceptance identifier and no disposition — those are decided elsewhere.
const COMMITTED_CELL_RECORD: &str = include_str!("../../svelte_conformance_cell_record.json");

/// Build the record from what the suite ACTUALLY observes, right now.
fn observed_cell_record() -> Value {
    let mut cells = serde_json::Map::new();
    for cell in read_svelte_conformance_matrix() {
        let record = read_svelte_golden_record(&cell.golden_name);
        let profile = cell.compile_profile();
        let outcome = match cell.compile_through_shipped_route() {
            SvelteRouteOutcome::Refused {
                diagnostic_code,
                message,
            } => json!({
                "kind": "refused",
                "diagnosticCode": diagnostic_code,
                "message": message,
                "comparatorRun": false,
                "comparatorSkipReason":
                    "the shipped route published no candidate, so there is nothing to compare \
                     against the golden",
            }),
            SvelteRouteOutcome::MissingNode => json!({ "kind": "missing-node" }),
            SvelteRouteOutcome::Emitted { code, source_map } => {
                let report = check_candidate(&cell.golden_name, &code, source_map.as_deref());
                json!({
                    "kind": "compared",
                    "verdict": report.verdict,
                    "divergences": report.reasons,
                    "axes": report
                        .axes
                        .iter()
                        .map(|(name, (status, _))| (name.clone(), Value::from(status.clone())))
                        .collect::<serde_json::Map<String, Value>>(),
                })
            }
        };
        cells.insert(
            cell.golden_name.clone(),
            json!({
                "request": {
                    "fixture": cell.fixture_path,
                    "generate": match cell.generate {
                        super::svelte_official_conformance_matrix::SvelteGenerate::Client =>
                            "client",
                        super::svelte_official_conformance_matrix::SvelteGenerate::Server =>
                            "server",
                    },
                    "runes": cell.runes,
                    "dev": cell.dev,
                },
                "reachability": reachability_label(&cell.reachability()),
                "route": "VerterHost::upsert -> get_virtual_file(Main) -> compile_entry -> \
                          CarrierCompilerRegistry::compile_bundle -> \
                          svelte::runtime::compile_client",
                "profile": {
                    "ssr": profile.ssr,
                    "sourceMap": profile.source_map,
                    "isProduction": profile.is_production,
                },
                "products": ["virtual-node.main", "virtual-node.style", "source-map", "diagnostics"],
                "officialDomain": record.get("domain"),
                "outcome": outcome,
            }),
        );
    }
    json!({
        "note": "Per-cell facts for the committed Svelte conformance goldens: the exact request, \
                 its route, its profile axes, the products the route can publish, the pinned \
                 official domain the golden was generated against, and the observed outcome. \
                 Facts only — no correction owner, no acceptance identifier, no disposition.",
        "cells": Value::Object(cells),
    })
}

/// The committed record matches what the suite observes, cell for cell.
///
/// Fails if a cell the suite drives is missing from the record, if the record
/// names a cell the suite no longer drives, or if any recorded outcome no
/// longer matches the observation.
#[test]
fn the_committed_cell_record_matches_what_the_suite_observes() {
    let observed = observed_cell_record();
    let committed: Value =
        serde_json::from_str(COMMITTED_CELL_RECORD).expect("the committed cell record is JSON");

    let observed_cells = observed["cells"].as_object().expect("observed cells");
    let committed_cells = committed["cells"].as_object().cloned().unwrap_or_default();

    // The observation is always PRINTED, so regenerating the committed record
    // after a genuine behaviour change is a copy rather than a transcription.
    // Stdout rather than a scratch file: no assertion reads it back, so the
    // test needs no real file. Run with `-- --nocapture` to see it.
    println!(
        "observed cell record:\n{}",
        serde_json::to_string_pretty(&observed).expect("the record serializes")
    );

    let observed_names: BTreeSet<&String> = observed_cells.keys().collect();
    let committed_names: BTreeSet<&String> = committed_cells.keys().collect();
    assert_eq!(
        observed_names, committed_names,
        "the committed record and the driven cells disagree on WHICH cells exist. This run's \
         observation was printed above; re-run with `-- --nocapture` to capture it"
    );

    let mut drifted = Vec::new();
    for (name, observed_cell) in observed_cells {
        let committed_cell = &committed_cells[name];
        if observed_cell != committed_cell {
            drifted.push(format!(
                "── {name} ──\n  observed:  {observed_cell}\n  committed: {committed_cell}"
            ));
        }
    }
    assert!(
        drifted.is_empty(),
        "{} recorded cell outcome(s) no longer match the observation. This run's observation \
         was printed above (re-run with `-- --nocapture`); regenerate the committed record from \
         it:\n\n{}",
        drifted.len(),
        drifted.join("\n\n")
    );
}

// The client runtime smoke

/// Mount a set of labelled client modules against the pinned official client
/// runtime and return what each rendered.
fn execute_client_modules(label: &str, modules: &[(&str, &str)]) -> Value {
    use super::bf2_seed_matrix::{harness_root, run_bounded, TempCandidate, ORACLE_TIMEOUT};

    let payload = json!({
        "modules": modules
            .iter()
            .map(|(name, code)| ((*name).to_string(), Value::from(*code)))
            .collect::<serde_json::Map<String, Value>>(),
        "props": {},
    });
    let input = TempCandidate::write(label, &payload.to_string());
    let mut command = Command::new("node");
    command
        .arg(harness_root().join("bin/execute-svelte-client.mjs"))
        .arg("--input")
        .arg(&input.path)
        .current_dir(harness_root());
    let finished = run_bounded(&mut command, ORACLE_TIMEOUT);
    assert!(
        !finished.timed_out,
        "{label}: the client runtime smoke did not finish within {ORACLE_TIMEOUT:?}.\nstderr:\n{}",
        finished.stderr
    );
    assert_eq!(
        finished.code,
        Some(0),
        "{label}: the client executor exited with {:?}.\nstdout:\n{}\nstderr:\n{}",
        finished.code,
        finished.stdout,
        finished.stderr
    );
    serde_json::from_str(&finished.stdout).unwrap_or_else(|error| {
        panic!(
            "{label}: the client executor emitted no JSON ({error}).\nstdout:\n{}\nstderr:\n{}",
            finished.stdout, finished.stderr
        )
    })
}

/// What the CANDIDATE renders once mounted, pinned exactly. Matching the
/// golden proves agreement; this proves the agreed-on markup is still the
/// markup this suite recorded.
const CLIENT_RENDERED_MARKUP: &str = include_str!("../../svelte_client_rendered_markup.txt");

/// One candidate-against-golden mount, as the runtime axis judges it.
struct MountedComparison {
    /// The svelte version the executor actually BOUND for this mount.
    runtime_version: String,
    candidate_ok: bool,
    golden_ok: bool,
    /// Each module's mount error, exactly as the executor reported it (the JSON
    /// `null` when it mounted).
    candidate_error: String,
    golden_error: String,
    /// What each module rendered.
    candidate_html: String,
    golden_html: String,
    /// `None` when the candidate mounted AND rendered exactly what the golden
    /// rendered. Otherwise the divergence, ready to report.
    divergence: Option<String>,
}

/// Mount a candidate and a golden module against the pinned official client
/// runtime and compare what they rendered.
///
/// THE runtime-axis comparison. The live gate below and the mutation-
/// discrimination test above both drive THIS function, so a planted wrong
/// render is judged by exactly the code that judges the shipped route's output
/// — not by a second copy of it that could drift into agreeing with anything.
fn compare_mounted_render(
    label: &str,
    candidate_code: &str,
    golden_code: &str,
) -> MountedComparison {
    let executed = execute_client_modules(
        label,
        &[("candidate", candidate_code), ("golden", golden_code)],
    );
    // THE CONTROL MUST BE THE PINNED RUNTIME. The compiled module reaches its
    // runtime through a BARE `svelte/internal/client` specifier, which Node
    // resolves by walking up from wherever the module is written. A different
    // copy one directory further up binds a SECOND runtime instance whose
    // `init_operations` never ran, and the mount dies inside the official
    // runtime with an opaque `undefined.call`. The executor therefore reports
    // the runtime it actually bound, and this pins it.
    let runtime = &executed["runtime"];
    assert_eq!(
        runtime["version"].as_str(),
        Some(SVELTE_PINNED_PACKAGE_VERSION),
        "{label}: the client executor bound svelte {} instead of the pinned {}; the mount would \
         be measuring a different runtime",
        runtime["version"],
        SVELTE_PINNED_PACKAGE_VERSION
    );

    let golden_run = &executed["golden"];
    let candidate_run = &executed["candidate"];

    let divergence = if candidate_run["ok"] != Value::Bool(true) {
        Some(format!(
            "── {label} ──\n  the candidate did not mount: {}",
            candidate_run["error"]
        ))
    } else if candidate_run["html"] != golden_run["html"] {
        Some(format!(
            "── {label} ──\n  rendered markup differs\n  candidate: {}\n  golden:    {}",
            candidate_run["html"], golden_run["html"]
        ))
    } else {
        None
    };

    MountedComparison {
        runtime_version: runtime["version"].as_str().unwrap_or_default().to_string(),
        candidate_ok: candidate_run["ok"] == Value::Bool(true),
        golden_ok: golden_run["ok"] == Value::Bool(true),
        candidate_error: candidate_run["error"].to_string(),
        golden_error: golden_run["error"].to_string(),
        candidate_html: rendered_html(candidate_run),
        golden_html: rendered_html(golden_run),
        divergence,
    }
}

/// The markup one executed module rendered, as a plain string.
fn rendered_html(run: &Value) -> String {
    run["html"].as_str().unwrap_or("<not a string>").to_string()
}

/// The client RUNTIME axis, driven for every reachable client request that
/// emits a module.
///
/// The harness's own applicability rule marks a Svelte CLIENT golden
/// `not-applicable` for the runtime axis
/// (`packages/framework-conformance-harness/src/check-candidate.mjs:50-56`).
/// That is a LIMITATION of the harness, not a property of the artifact: the
/// only Svelte executor it shipped was the SSR one, whose input is documented
/// as `generate:"server"` module source
/// (`packages/framework-conformance-harness/src/execute-svelte-runtime.mjs:31`).
/// A client module IS executable given a DOM, so this drives it: candidate and
/// golden are each mounted against the PINNED official client runtime and their
/// rendered markup compared.
#[test]
fn every_emitting_client_request_mounts_and_renders_what_the_golden_renders() {
    let mut compared = 0usize;
    let mut divergences = Vec::new();
    let mut rendered = Vec::new();

    for cell in reachable_client_requests() {
        let SvelteRouteOutcome::Emitted { code, .. } = cell.compile_through_shipped_route() else {
            // A refused request publishes no module to mount. That outcome is
            // recorded by the gate above; there is nothing to execute here.
            continue;
        };
        let record = read_svelte_golden_record(&cell.golden_name);
        let golden_code = record["code"]
            .as_str()
            .unwrap_or_else(|| panic!("{}: the record carries no code", cell.golden_name))
            .to_string();

        let comparison = compare_mounted_render(&cell.golden_name, &code, &golden_code);

        assert!(
            comparison.golden_ok,
            "{}: the official module did not mount, so this smoke cannot decide anything: {}",
            cell.golden_name, comparison.golden_error
        );
        if let Some(divergence) = comparison.divergence {
            divergences.push(divergence);
            if !comparison.candidate_ok {
                // A candidate that never mounted rendered no markup to pin.
                continue;
            }
        }
        rendered.push(format!(
            "{} => {}",
            cell.golden_name, comparison.candidate_html
        ));
        compared += 1;
    }

    assert!(
        compared > 0,
        "no reachable client request emitted a module, so the runtime axis executed nothing"
    );
    assert!(
        divergences.is_empty(),
        "{} of {} mounted client request(s) diverge from the official runtime result:\n\n{}",
        divergences.len(),
        compared + divergences.len(),
        divergences.join("\n\n")
    );
    // The MARKUP itself, not just "it matched": a change that alters what the
    // candidate renders fails here even if the official runtime happens to
    // produce the same change.
    rendered.sort();
    assert_eq!(
        rendered.join("\n"),
        CLIENT_RENDERED_MARKUP.trim_end().replace("\r\n", "\n"),
        "the mounted client markup moved"
    );
    println!("client runtime smoke: {compared} request(s) mounted and matched");
}
