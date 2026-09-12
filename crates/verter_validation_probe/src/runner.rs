//! The table-driven workload runner.
//!
//! There is no per-fixture test and no per-case CI job: the runner reads a
//! validated probe-state manifest, asks its corpus adapter for the listed
//! cases, drives them through the ONE public compile route, and folds what it
//! saw into a [`CaseObservation`] per case. `framework` is a case attribute
//! throughout — never a runner variant — so a second corpus plugs in by
//! registering an adapter and a reference producer, not by forking this file.
//!
//! Three properties the rest of the lane depends on:
//!
//! * **Classification reads structure, never message text.** A failure is
//!   classified by its `kind`, its diagnostics' severities, and the shape of
//!   the product it did or did not produce. No branch matches on a message.
//! * **Frames are authenticated before they are believed.** A frame is paired
//!   with its probe by id and checked for cardinality and per-position
//!   identity before anything is classified, so a dropped, duplicated, or
//!   reordered entry can never attach one case's product or reference to
//!   another case.
//! * **Termination is attributable.** The driver announces each phase, so a
//!   death with no line is attributed to the step that was running: an addon
//!   that fails to load is a harness failure, a compiler that hangs is a
//!   timeout, and a reference producer that dies costs only the Structural
//!   cell.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::manifest::{Comparison, Framework, ProbeStateManifest};
use crate::outcome::{
    CaseObservation, Dimension, DimensionInput, Evidence, EvidenceSource, InvalidObservation,
    NotApplicableReason, ProbeOutcomeClass,
};
use crate::request;

/// The comparator's reason cap. Fixed in the crate: a lane that tuned it per
/// run would report a different number of reasons for the same difference.
pub const MAX_REASONS: usize = 32;

/// How long each phase may take before the driver is killed.
///
/// Per phase, not per process: a compiler that hangs is a `timeout` and a
/// reference producer that hangs is a `reference_failure`, and neither can be
/// mistaken for the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhaseDeadlines {
    /// Addon load and driver start.
    pub load: Duration,
    /// The bracketed `compileRequests` call.
    pub compile: Duration,
    /// Reference production.
    pub reference: Duration,
}

impl Default for PhaseDeadlines {
    fn default() -> Self {
        PhaseDeadlines {
            load: Duration::from_secs(120),
            compile: Duration::from_secs(120),
            reference: Duration::from_secs(120),
        }
    }
}

/// The step the driver announced it was entering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Loading the addon.
    Load,
    /// Inside the bracketed compile call.
    Compile,
    /// Producing the reference product.
    Reference,
}

impl Phase {
    /// The serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Phase::Load => "load",
            Phase::Compile => "compile",
            Phase::Reference => "reference",
        }
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A raw execution event, before any per-dimension folding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionEvent {
    /// The driver process could not be started.
    SpawnFailed,
    /// A phase exceeded its deadline.
    TimedOut {
        /// The last phase the driver announced.
        phase: Option<Phase>,
    },
    /// The process died by signal.
    Signaled {
        /// The last phase the driver announced.
        phase: Option<Phase>,
    },
    /// The process exited.
    Exited {
        /// The last phase the driver announced.
        phase: Option<Phase>,
        /// The exit code.
        code: i32,
    },
}

/// Classify one raw execution event.
///
/// The phase is what makes the three failures distinct rather than one
/// undifferentiated "the driver died":
///
/// * before any marker, or in `load` — the driver or the addon never got
///   started, which is the harness's own failure and says nothing about the
///   compiler;
/// * in `compile` — the only code running is inside the native call, because
///   every JavaScript step around it reports as a line rather than an exit, so
///   this is genuinely the compiler crashing or hanging;
/// * in `reference` — the compiler already answered and its frame is already
///   ingested, so the loss is confined to the comparison input.
///
/// A clean exit is not an event any probe is classified from: it finalizes the
/// protocol per probe, which [`ProbeRun`] does from the frames it did and did
/// not receive.
pub fn classify_execution(event: ExecutionEvent) -> ProbeOutcomeClass {
    match event {
        ExecutionEvent::SpawnFailed => ProbeOutcomeClass::HarnessFailure,
        ExecutionEvent::TimedOut { phase } => match phase {
            None | Some(Phase::Load) => ProbeOutcomeClass::HarnessFailure,
            Some(Phase::Compile) => ProbeOutcomeClass::Timeout,
            Some(Phase::Reference) => ProbeOutcomeClass::ReferenceFailure,
        },
        ExecutionEvent::Signaled { phase } => match phase {
            None | Some(Phase::Load) => ProbeOutcomeClass::HarnessFailure,
            Some(Phase::Compile) => ProbeOutcomeClass::Crash,
            Some(Phase::Reference) => ProbeOutcomeClass::ReferenceFailure,
        },
        ExecutionEvent::Exited { phase, code } => {
            if code == 0 {
                // A clean exit decides nothing on its own: the protocol is
                // finalized per probe from the frames that did and did not
                // arrive.
                ProbeOutcomeClass::Pass
            } else {
                match phase {
                    None | Some(Phase::Load) => ProbeOutcomeClass::HarnessFailure,
                    Some(Phase::Compile) => ProbeOutcomeClass::Crash,
                    Some(Phase::Reference) => ProbeOutcomeClass::ReferenceFailure,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Driver wire shapes
// ---------------------------------------------------------------------------

/// One line the driver wrote. The arms are distinguished structurally, by the
/// fields a line carries, never by inspecting a message.
#[derive(Clone, Debug, PartialEq)]
pub enum DriverLine {
    /// A phase marker.
    Phase {
        /// The probe it belongs to.
        probe_id: String,
        /// The phase being entered.
        phase: Phase,
    },
    /// The compile frame.
    Compile {
        /// The probe it belongs to.
        probe_id: String,
        /// Nanoseconds bracketing exactly the compile call.
        elapsed_ns: u64,
        /// The route's answer, one entry per requested entry, in order.
        entries: Vec<RouteEntry>,
    },
    /// The reference frame.
    Reference {
        /// The probe it belongs to.
        probe_id: String,
        /// One reference result per requested entry, in order.
        reference: Vec<ReferenceResult>,
    },
    /// A driver-level exception.
    Error {
        /// The probe it belongs to, when the driver could attribute one.
        probe_id: Option<String>,
        /// The message, verbatim; retained as evidence, never classified from.
        error: String,
    },
}

/// One `compileRequests` answer, carried whole.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct RouteEntry {
    /// The id the route answered for this position.
    #[serde(rename = "canonicalId")]
    pub canonical_id: String,
    /// The success arm.
    #[serde(default)]
    pub response: Option<RouteResponse>,
    /// The typed failure arm.
    #[serde(default)]
    pub failure: Option<RouteFailure>,
}

/// A typed compile response.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct RouteResponse {
    /// The diagnostics snapshot, intact.
    pub diagnostics: DiagnosticsSnapshot,
    /// The products, intact.
    pub products: Vec<RouteProduct>,
}

/// One product row.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct RouteProduct {
    /// The product's wire tag.
    pub kind: String,
    /// The separately-addressed outputs, when this product has them.
    #[serde(default)]
    pub nodes: Option<Vec<RouteNode>>,
}

/// One addressed output of a runtime product.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct RouteNode {
    /// Which addressed output this is.
    pub node: VirtualNodeKind,
    /// The emitted module.
    pub code: String,
}

/// The route's virtual-node kind, carried as the route spells it.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct VirtualNodeKind {
    /// The kind's wire tag.
    pub kind: String,
}

/// A typed terminal failure.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct RouteFailure {
    /// The failure's wire tag — the field classification reads.
    pub kind: String,
    /// The message, retained as evidence only.
    #[serde(default)]
    pub message: String,
    /// The diagnostics the failure carries, intact.
    pub diagnostics: DiagnosticsSnapshot,
}

/// The diagnostics snapshot both arms carry.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct DiagnosticsSnapshot {
    /// The diagnostics, in the order the compiler reported them.
    #[serde(default)]
    pub diagnostics: Vec<RouteDiagnostic>,
}

/// One diagnostic.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct RouteDiagnostic {
    /// The severity, the field classification reads.
    pub severity: String,
    /// The diagnostic code.
    #[serde(default)]
    pub code: String,
    /// The message, retained as evidence only.
    #[serde(default)]
    pub message: String,
}

impl DiagnosticsSnapshot {
    fn has_error(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == "error")
    }

    /// The comparator's severity-ordered row projection.
    fn rows(&self) -> Vec<verter_vue_conformance::compare::DiagnosticRow> {
        let mut rows: Vec<_> = self
            .diagnostics
            .iter()
            .map(
                |diagnostic| verter_vue_conformance::compare::DiagnosticRow {
                    kind: diagnostic.severity.clone(),
                    code: (!diagnostic.code.is_empty()).then(|| diagnostic.code.clone()),
                    message: diagnostic.message.clone(),
                },
            )
            .collect();
        rows.sort_by(|left, right| left.kind.cmp(&right.kind));
        rows
    }

    fn evidence(&self) -> Vec<Evidence> {
        self.diagnostics
            .iter()
            .map(|diagnostic| Evidence {
                source: EvidenceSource::AddonDiagnostic,
                message: format!(
                    "{}[{}]: {}",
                    diagnostic.severity, diagnostic.code, diagnostic.message
                ),
            })
            .collect()
    }
}

/// One reference result: a product, a failure, or no producer at all.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ReferenceResult {
    /// The reference compiler produced a module.
    Produced {
        /// The emitted module.
        code: String,
    },
    /// The reference compiler failed.
    Failed {
        /// Its message, retained as evidence only.
        error: String,
    },
    /// No reference producer is registered for this framework.
    Inapplicable {
        /// The framework with no producer.
        inapplicable: String,
    },
}

/// Parse one driver line. A line whose shape is not one of the four is not
/// resolved into a plausible arm — it is rejected, and the caller classifies
/// the probe a harness failure.
pub fn parse_line(line: &str) -> Result<DriverLine, String> {
    let value: serde_json::Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    let object = value.as_object().ok_or("a driver line must be an object")?;
    let probe_id = object
        .get("probe_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    if object.contains_key("error") {
        return Ok(DriverLine::Error {
            probe_id,
            error: object
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
        });
    }
    let probe_id = probe_id.ok_or("a driver line must carry a probe_id")?;
    if let Some(phase) = object.get("phase").and_then(serde_json::Value::as_str) {
        let phase = match phase {
            "load" => Phase::Load,
            "compile" => Phase::Compile,
            "reference" => Phase::Reference,
            other => return Err(format!("unknown phase `{other}`")),
        };
        return Ok(DriverLine::Phase { probe_id, phase });
    }
    match object.get("frame").and_then(serde_json::Value::as_str) {
        Some("compile") => {
            let elapsed_ns = object
                .get("elapsed_ns")
                .and_then(serde_json::Value::as_u64)
                .ok_or("a compile frame must carry elapsed_ns")?;
            let entries: Vec<RouteEntry> = serde_json::from_value(
                object
                    .get("entries")
                    .cloned()
                    .ok_or("a compile frame must carry entries")?,
            )
            .map_err(|error| error.to_string())?;
            Ok(DriverLine::Compile {
                probe_id,
                elapsed_ns,
                entries,
            })
        }
        Some("reference") => {
            let reference: Vec<ReferenceResult> = serde_json::from_value(
                object
                    .get("reference")
                    .cloned()
                    .ok_or("a reference frame must carry a reference array")?,
            )
            .map_err(|error| error.to_string())?;
            Ok(DriverLine::Reference {
                probe_id,
                reference,
            })
        }
        Some(other) => Err(format!("unknown frame `{other}`")),
        None => Err("a driver line must carry a phase, a frame, or an error".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Streaming probe state
// ---------------------------------------------------------------------------

/// What one probe asked for, and what has arrived so far.
#[derive(Clone, Debug)]
pub struct ProbeRun {
    probe_id: String,
    requested: Vec<RequestedEntry>,
    phase: Option<Phase>,
    compile: Option<CompileFrame>,
    reference: Option<Vec<ReferenceResult>>,
    harness: Vec<String>,
}

/// One entry the runner asked the driver to compile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestedEntry {
    /// The case id, which is also the entry's `canonicalId`.
    pub canonical_id: String,
    /// The component text handed to the driver.
    pub source: String,
    /// The case's own request digest.
    pub request_digest: String,
}

#[derive(Clone, Debug)]
struct CompileFrame {
    elapsed_ns: u64,
    entries: Vec<RouteEntry>,
}

/// Why a frame was refused before anything was classified from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameViolation {
    /// A frame for a probe the runner never requested.
    UnknownProbe {
        /// The id the frame claimed.
        probe_id: String,
    },
    /// A second compile frame for one probe.
    DuplicateCompileFrame {
        /// The probe.
        probe_id: String,
    },
    /// A reference frame whose compile frame never arrived.
    ReferenceWithoutCompile {
        /// The probe.
        probe_id: String,
    },
    /// A frame that answers a different number of entries than were asked.
    CountMismatch {
        /// The probe.
        probe_id: String,
        /// What was asked.
        requested: usize,
        /// What arrived.
        answered: usize,
    },
    /// A frame that answers a different id at a position than was asked.
    IdentityMismatch {
        /// The probe.
        probe_id: String,
        /// The position.
        position: usize,
        /// What was asked for there.
        requested: String,
        /// What arrived there.
        answered: String,
    },
    /// One id answered twice in the same frame.
    DuplicateEntry {
        /// The probe.
        probe_id: String,
        /// The repeated id.
        canonical_id: String,
    },
}

impl fmt::Display for FrameViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameViolation::UnknownProbe { probe_id } => {
                write!(f, "frame for unrequested probe `{probe_id}`")
            }
            FrameViolation::DuplicateCompileFrame { probe_id } => {
                write!(f, "`{probe_id}`: a second compile frame arrived")
            }
            FrameViolation::ReferenceWithoutCompile { probe_id } => {
                write!(
                    f,
                    "`{probe_id}`: a reference frame arrived with no compile frame"
                )
            }
            FrameViolation::CountMismatch {
                probe_id,
                requested,
                answered,
            } => write!(
                f,
                "`{probe_id}`: {answered} answered entries for {requested} requested"
            ),
            FrameViolation::IdentityMismatch {
                probe_id,
                position,
                requested,
                answered,
            } => write!(
                f,
                "`{probe_id}`: position {position} answered `{answered}`, requested `{requested}`"
            ),
            FrameViolation::DuplicateEntry {
                probe_id,
                canonical_id,
            } => write!(f, "`{probe_id}`: `{canonical_id}` answered twice"),
        }
    }
}

impl ProbeRun {
    /// Begin streaming state for one probe.
    pub fn new(probe_id: impl Into<String>, requested: Vec<RequestedEntry>) -> Self {
        ProbeRun {
            probe_id: probe_id.into(),
            requested,
            phase: None,
            compile: None,
            reference: None,
            harness: Vec::new(),
        }
    }

    /// The probe's id.
    pub fn probe_id(&self) -> &str {
        &self.probe_id
    }

    /// The last phase the driver announced for this probe.
    pub fn phase(&self) -> Option<Phase> {
        self.phase
    }

    /// Whether the compile frame has been ingested.
    pub fn has_compile_frame(&self) -> bool {
        self.compile.is_some()
    }

    /// Record a harness-side failure against this probe.
    pub fn record_harness_failure(&mut self, message: impl Into<String>) {
        self.harness.push(message.into());
    }

    /// Ingest one driver line.
    ///
    /// A frame is validated against the entry set this probe holds BEFORE it
    /// is retained: the count must match, each position's id must be the one
    /// requested there, and no id may repeat. A violation records a harness
    /// failure and the frame is dropped whole — nothing is paired positionally
    /// from a frame that failed its own identity check.
    pub fn ingest_frame(&mut self, line: DriverLine) -> Result<(), FrameViolation> {
        match line {
            DriverLine::Phase { phase, .. } => {
                self.phase = Some(phase);
                Ok(())
            }
            DriverLine::Error { error, .. } => {
                self.harness.push(error);
                Ok(())
            }
            DriverLine::Compile {
                elapsed_ns,
                entries,
                ..
            } => {
                if self.compile.is_some() {
                    let violation = FrameViolation::DuplicateCompileFrame {
                        probe_id: self.probe_id.clone(),
                    };
                    self.harness.push(violation.to_string());
                    return Err(violation);
                }
                self.check_cardinality(entries.len())?;
                let mut seen = std::collections::BTreeSet::new();
                for (position, entry) in entries.iter().enumerate() {
                    let requested = &self.requested[position].canonical_id;
                    if &entry.canonical_id != requested {
                        let violation = FrameViolation::IdentityMismatch {
                            probe_id: self.probe_id.clone(),
                            position,
                            requested: requested.clone(),
                            answered: entry.canonical_id.clone(),
                        };
                        self.harness.push(violation.to_string());
                        return Err(violation);
                    }
                    if !seen.insert(entry.canonical_id.clone()) {
                        let violation = FrameViolation::DuplicateEntry {
                            probe_id: self.probe_id.clone(),
                            canonical_id: entry.canonical_id.clone(),
                        };
                        self.harness.push(violation.to_string());
                        return Err(violation);
                    }
                }
                self.compile = Some(CompileFrame {
                    elapsed_ns,
                    entries,
                });
                Ok(())
            }
            DriverLine::Reference { reference, .. } => {
                if self.compile.is_none() {
                    let violation = FrameViolation::ReferenceWithoutCompile {
                        probe_id: self.probe_id.clone(),
                    };
                    self.harness.push(violation.to_string());
                    return Err(violation);
                }
                self.check_cardinality(reference.len())?;
                self.reference = Some(reference);
                Ok(())
            }
        }
    }

    fn check_cardinality(&mut self, answered: usize) -> Result<(), FrameViolation> {
        if answered == self.requested.len() {
            return Ok(());
        }
        let violation = FrameViolation::CountMismatch {
            probe_id: self.probe_id.clone(),
            requested: self.requested.len(),
            answered,
        };
        self.harness.push(violation.to_string());
        Err(violation)
    }

    /// Fold this probe's state into one observation per requested entry.
    ///
    /// `terminated` is the process outcome when the driver died before the
    /// protocol completed, and `None` when it ran to a clean exit.
    pub fn finish(
        &self,
        manifest: &ProbeStateManifest,
        terminated: Option<ExecutionEvent>,
    ) -> Vec<Result<CaseObservation, InvalidObservation>> {
        let terminal = terminated.map(classify_execution);
        self.requested
            .iter()
            .enumerate()
            .map(|(position, requested)| self.observe(manifest, position, requested, terminal))
            .collect()
    }

    fn observe(
        &self,
        manifest: &ProbeStateManifest,
        position: usize,
        requested: &RequestedEntry,
        terminal: Option<ProbeOutcomeClass>,
    ) -> Result<CaseObservation, InvalidObservation> {
        let harness_evidence: Vec<Evidence> = self
            .harness
            .iter()
            .map(|message| Evidence {
                source: EvidenceSource::Driver,
                message: message.clone(),
            })
            .collect();

        // The harness failing to observe outranks anything it might have
        // observed: a probe whose protocol broke reports that, not a verdict
        // about the compiler it could not watch.
        if !self.harness.is_empty() {
            return self.all_dimensions(
                manifest,
                requested,
                ProbeOutcomeClass::HarnessFailure,
                harness_evidence,
            );
        }

        let Some(compile) = self.compile.as_ref() else {
            // No compile frame: either the process died mid-compile, or it
            // exited cleanly without completing its protocol. Both leave the
            // Route dimension unanswered.
            let class = terminal.unwrap_or(ProbeOutcomeClass::HarnessFailure);
            let class = if class == ProbeOutcomeClass::Pass {
                ProbeOutcomeClass::HarnessFailure
            } else {
                class
            };
            return self.all_dimensions(manifest, requested, class, harness_evidence);
        };

        let entry = &compile.entries[position];
        let mut route = Vec::new();
        let mut compile_classes = Vec::new();
        let mut evidence = Vec::new();

        let diagnostics = match (&entry.response, &entry.failure) {
            (Some(response), _) => &response.diagnostics,
            (None, Some(failure)) => &failure.diagnostics,
            (None, None) => {
                // Neither arm: the route answered nothing the classifier can
                // read. Failing closed keeps an unexplained envelope from
                // reading as a pass.
                return self.all_dimensions(
                    manifest,
                    requested,
                    ProbeOutcomeClass::HarnessFailure,
                    vec![Evidence {
                        source: EvidenceSource::Driver,
                        message: "route entry carries neither a response nor a failure".to_string(),
                    }],
                );
            }
        };
        evidence.extend(diagnostics.evidence());
        // (i) An error-severity diagnostic is a Compile outcome whichever arm
        // carried it: the route ANSWERED, so Route is a pass.
        if diagnostics.has_error() {
            compile_classes.push(ProbeOutcomeClass::VerterDiagnostic);
        }

        if let Some(failure) = &entry.failure {
            evidence.push(Evidence {
                source: EvidenceSource::Driver,
                message: format!("failure.kind={}: {}", failure.kind, failure.message),
            });
            match failure.kind.as_str() {
                // The route's typed refusal of a binding or a framework: a
                // public-boundary outcome, never hidden as a harness defect.
                "binding" | "frameworkMismatch" => {
                    route.push(ProbeOutcomeClass::RequestRefused);
                }
                "host" => route.push(ProbeOutcomeClass::HostFailure),
                "runtimeSurfaceRefused" | "unsupportedProduct" => {
                    compile_classes.push(ProbeOutcomeClass::Unsupported);
                }
                "productNotProduced" => compile_classes.push(ProbeOutcomeClass::ProductNotProduced),
                // `refused` contributes nothing of its own: the live route
                // constructs it whenever compile diagnostics carry errors, so
                // it folds to `verter_diagnostic` through rule (i). A
                // `refused` with no diagnostic is unexplained, and fails
                // closed rather than being read as a product outcome.
                "refused" => {
                    if !diagnostics.has_error() {
                        compile_classes.push(ProbeOutcomeClass::HarnessFailure);
                    }
                }
                _ => compile_classes.push(ProbeOutcomeClass::HarnessFailure),
            }
        }

        let mut product_code: Option<&str> = None;
        if let Some(response) = &entry.response {
            match extract_main(response) {
                Ok(code) => product_code = Some(code),
                Err(class) => compile_classes.push(class),
            }
        }

        if route.is_empty() {
            route.push(ProbeOutcomeClass::Pass);
        }
        let route_failed = route.iter().any(|class| class.is_failure());
        // A Route failure is contributed to Compile as well, not left to
        // propagate. Propagation only fills a dimension that observed nothing,
        // so a host failure or a typed refusal that ALSO carried an error
        // diagnostic would otherwise be replaced at Compile by the lower-
        // precedence diagnostic — and a host failure would silently satisfy a
        // diagnostic expectation. Contributing it here makes the Compile fold
        // apply the taxonomy's own precedence and retain the diagnostic as
        // secondary evidence.
        if route_failed {
            compile_classes.extend(route.iter().copied().filter(|class| class.is_failure()));
        }

        // The product's own validity check: the same canonicalizer the
        // comparator runs, so a module that cannot be compared is reported as
        // malformed here rather than as a comparator error later.
        let authored = verter_vue_conformance::authored_identifiers(&requested.source);
        if let Some(code) = product_code {
            if let Err(message) =
                verter_vue_conformance::canon::canonicalize_module(code, &authored)
            {
                compile_classes.push(ProbeOutcomeClass::ProductMalformed);
                evidence.push(Evidence {
                    source: EvidenceSource::Parser,
                    message,
                });
                product_code = None;
            }
        }

        if compile_classes.is_empty() && !route_failed {
            compile_classes.push(ProbeOutcomeClass::Pass);
        }

        let mut inputs = BTreeMap::new();
        inputs.insert(
            Dimension::Route,
            DimensionInput::Observed {
                classes: route.clone(),
                evidence: evidence.clone(),
            },
        );
        inputs.insert(
            Dimension::Compile,
            if compile_classes.is_empty() {
                DimensionInput::Unreached
            } else {
                DimensionInput::Observed {
                    classes: compile_classes.clone(),
                    evidence: evidence.clone(),
                }
            },
        );
        inputs.insert(
            Dimension::Structural,
            self.structural(manifest, position, product_code, &authored, terminal),
        );
        inputs.insert(
            Dimension::Runtime,
            not_applicable_or_unreached(manifest, Dimension::Runtime),
        );
        inputs.insert(
            Dimension::Map,
            not_applicable_or_unreached(manifest, Dimension::Map),
        );
        inputs.insert(
            Dimension::Performance,
            DimensionInput::Observed {
                classes: vec![ProbeOutcomeClass::Pass],
                evidence: vec![Evidence {
                    source: EvidenceSource::Driver,
                    message: format!("elapsed_ns={}", compile.elapsed_ns),
                }],
            },
        );
        CaseObservation::fold(requested.canonical_id.clone(), inputs)
    }

    fn structural(
        &self,
        manifest: &ProbeStateManifest,
        position: usize,
        product_code: Option<&str>,
        authored: &std::collections::BTreeSet<String>,
        terminal: Option<ProbeOutcomeClass>,
    ) -> DimensionInput {
        if manifest.comparison == Comparison::None {
            return DimensionInput::NotApplicable(NotApplicableReason::ComparatorAbsent);
        }
        let reference_failure =
            |message: String, source: EvidenceSource| DimensionInput::Observed {
                classes: vec![ProbeOutcomeClass::ReferenceFailure],
                evidence: vec![Evidence { source, message }],
            };
        let Some(reference) = self.reference.as_ref() else {
            // The compile frame arrived and the reference frame did not. The
            // reference frame is REQUIRED for every probe of every framework —
            // an unregistered producer still answers `inapplicable` — so its
            // absence is the protocol breaking, not a comparator that does not
            // exist.
            let class = match terminal {
                Some(ProbeOutcomeClass::ReferenceFailure) => ProbeOutcomeClass::ReferenceFailure,
                Some(class) if class.is_failure() => class,
                _ => ProbeOutcomeClass::HarnessFailure,
            };
            return DimensionInput::Observed {
                classes: vec![class],
                evidence: vec![Evidence {
                    source: EvidenceSource::Driver,
                    message: "the reference frame never arrived".to_string(),
                }],
            };
        };
        match &reference[position] {
            ReferenceResult::Failed { error } => {
                reference_failure(error.clone(), EvidenceSource::Reference)
            }
            ReferenceResult::Inapplicable { inapplicable } => reference_failure(
                format!(
                    "no reference producer is registered for `{inapplicable}`, but this \
                     framework's manifest declares comparison = structural"
                ),
                EvidenceSource::Reference,
            ),
            ReferenceResult::Produced { code } => {
                let Some(product) = product_code else {
                    // No comparable product: the Compile dimension already
                    // says why, and it propagates here. Emitting a comparison
                    // class would claim a comparison that never ran.
                    return DimensionInput::Unreached;
                };
                // The reference side is prevalidated with the SAME check the
                // Compile cell applies to the product, so `compare_modules`
                // is only ever called on two modules the comparator can
                // canonicalize — which is what makes a `CompareError` below
                // an internal comparator failure rather than an input it was
                // never able to read.
                if let Err(message) =
                    verter_vue_conformance::canon::canonicalize_module(code, authored)
                {
                    return reference_failure(
                        format!(
                            "the reference produced a module the structural comparator does \
                             not canonicalize: {message}"
                        ),
                        EvidenceSource::Reference,
                    );
                }
                let verter = verter_vue_conformance::compare::ModuleInput {
                    code: product.to_string(),
                    diagnostics: self.diagnostic_rows(position),
                };
                let golden = verter_vue_conformance::compare::ModuleInput {
                    code: code.clone(),
                    diagnostics: Vec::new(),
                };
                match verter_vue_conformance::compare::compare_modules(
                    &verter,
                    &golden,
                    authored,
                    MAX_REASONS,
                ) {
                    // Both inputs prevalidated, so a comparator error here is
                    // the comparator's own failure, not either module's.
                    Err(error) => DimensionInput::Observed {
                        classes: vec![ProbeOutcomeClass::HarnessFailure],
                        evidence: vec![Evidence {
                            source: EvidenceSource::Comparator,
                            message: error.to_string(),
                        }],
                    },
                    Ok(comparison) if comparison.passed() => DimensionInput::Observed {
                        classes: vec![ProbeOutcomeClass::Pass],
                        evidence: Vec::new(),
                    },
                    Ok(comparison) => DimensionInput::Observed {
                        classes: vec![ProbeOutcomeClass::SemanticMismatch],
                        evidence: comparison
                            .reasons
                            .iter()
                            .map(|reason| Evidence {
                                source: EvidenceSource::Comparator,
                                message: reason.summary(),
                            })
                            .chain(std::iter::once(Evidence {
                                source: EvidenceSource::Comparator,
                                message: format!("{} in-contract differences", comparison.total),
                            }))
                            .collect(),
                    },
                }
            }
        }
    }

    fn diagnostic_rows(
        &self,
        position: usize,
    ) -> Vec<verter_vue_conformance::compare::DiagnosticRow> {
        self.compile
            .as_ref()
            .and_then(|frame| frame.entries.get(position))
            .and_then(|entry| entry.response.as_ref())
            .map(|response| response.diagnostics.rows())
            .unwrap_or_default()
    }

    fn all_dimensions(
        &self,
        manifest: &ProbeStateManifest,
        requested: &RequestedEntry,
        class: ProbeOutcomeClass,
        evidence: Vec<Evidence>,
    ) -> Result<CaseObservation, InvalidObservation> {
        let mut inputs = BTreeMap::new();
        for dimension in Dimension::ALL {
            let input = match manifest.not_applicable_reason(dimension) {
                Some(reason) => DimensionInput::NotApplicable(reason),
                None if dimension == Dimension::Route => DimensionInput::Observed {
                    classes: vec![class],
                    evidence: evidence.clone(),
                },
                None => DimensionInput::Unreached,
            };
            inputs.insert(dimension, input);
        }
        CaseObservation::fold(requested.canonical_id.clone(), inputs)
    }
}

fn not_applicable_or_unreached(
    manifest: &ProbeStateManifest,
    dimension: Dimension,
) -> DimensionInput {
    match manifest.not_applicable_reason(dimension) {
        Some(reason) => DimensionInput::NotApplicable(reason),
        None => DimensionInput::Unreached,
    }
}

/// Extract the one `main` node of the one `runtimeClient` product.
///
/// The request asks for exactly one product, so the answer's shape is exact:
/// anything other than one `runtimeClient` product holding exactly one `main`
/// node is reported as absent or malformed rather than searched through for
/// something usable.
fn extract_main(response: &RouteResponse) -> Result<&str, ProbeOutcomeClass> {
    if response.products.is_empty() {
        return Err(ProbeOutcomeClass::ProductNotProduced);
    }
    if response.products.len() > 1 {
        return Err(ProbeOutcomeClass::ProductMalformed);
    }
    let product = &response.products[0];
    if product.kind != "runtimeClient" {
        return Err(ProbeOutcomeClass::ProductMalformed);
    }
    let nodes = product.nodes.as_deref().unwrap_or_default();
    // The `main` node is the emitted module; `script` and `template` are the
    // route's separately-addressed halves of it, and comparing one of those
    // against the reference's whole module would compare two different things.
    let mains: Vec<&RouteNode> = nodes
        .iter()
        .filter(|node| node.node.kind == "main")
        .collect();
    match mains.len() {
        0 => Err(ProbeOutcomeClass::ProductNotProduced),
        1 => Ok(mains[0].code.as_str()),
        _ => Err(ProbeOutcomeClass::ProductMalformed),
    }
}

// ---------------------------------------------------------------------------
// Driving the lane
// ---------------------------------------------------------------------------

/// One case's evaluated outcome.
#[derive(Clone, Debug)]
pub struct CaseResult {
    /// The case id.
    pub case_id: String,
    /// The case's own request digest.
    pub request_digest: String,
    /// Nanoseconds the bracketed compile call took, when it was observed.
    pub elapsed_ns: Option<u64>,
    /// The folded observation, or why one could not be represented.
    pub observation: Result<CaseObservation, InvalidObservation>,
}

/// Why a lane could not run at all. A lane that cannot execute reports that;
/// it never publishes a summary of zero cases.
#[derive(Clone, Debug)]
pub enum RunError {
    /// The selection is empty.
    EmptySelection,
    /// A selected case is not in the manifest inventory.
    UnselectableCase {
        /// The case id.
        case_id: String,
    },
    /// The driver could not be spawned.
    SpawnFailed {
        /// The operating system's message.
        message: String,
    },
    /// The driver's stdin or stdout could not be used.
    Io {
        /// The operating system's message.
        message: String,
    },
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::EmptySelection => f.write_str("the lane selected no case"),
            RunError::UnselectableCase { case_id } => {
                write!(f, "case `{case_id}` is not in the manifest inventory")
            }
            RunError::SpawnFailed { message } => write!(f, "spawning the driver: {message}"),
            RunError::Io { message } => write!(f, "driving the probe: {message}"),
        }
    }
}

impl std::error::Error for RunError {}

/// One case the runner will drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedCase {
    /// The stable case id.
    pub case_id: String,
    /// The corpus-relative path, which the request carries as its filename.
    pub relative_path: String,
    /// The component text.
    pub source: String,
}

/// Where the driver and its interpreter live.
#[derive(Clone, Debug)]
pub struct DriverCommand {
    /// The interpreter.
    pub program: PathBuf,
    /// The driver script.
    pub script: PathBuf,
}

impl DriverCommand {
    /// The committed driver, run by `node`.
    pub fn committed(repo_root: &Path) -> Self {
        DriverCommand {
            program: PathBuf::from("node"),
            script: repo_root
                .join("crates")
                .join("verter_validation_probe")
                .join("driver")
                .join("probe-driver.mjs"),
        }
    }
}

/// Drive every planned case through one driver process and fold the results.
///
/// One process serves the whole lane, and each case is its own probe, so a
/// per-case failure is attributed to that case while the rest keep running.
pub fn run_cases(
    manifest: &ProbeStateManifest,
    driver: &DriverCommand,
    cases: &[PlannedCase],
    deadlines: PhaseDeadlines,
) -> Result<Vec<CaseResult>, RunError> {
    if cases.is_empty() {
        return Err(RunError::EmptySelection);
    }
    let inventory: std::collections::BTreeSet<&str> = manifest
        .inventory
        .iter()
        .map(|case| case.case_id.as_str())
        .collect();
    for case in cases {
        if !inventory.contains(case.case_id.as_str()) {
            return Err(RunError::UnselectableCase {
                case_id: case.case_id.clone(),
            });
        }
    }

    let mut child = Command::new(&driver.program)
        .arg(&driver.script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| RunError::SpawnFailed {
            message: error.to_string(),
        })?;
    let stdout = child.stdout.take().ok_or_else(|| RunError::Io {
        message: "the driver exposes no stdout".to_string(),
    })?;
    let lines = spawn_reader(stdout);

    let mut results = Vec::with_capacity(cases.len());
    let mut terminated: Option<ExecutionEvent> = None;
    for case in cases {
        let requested = vec![RequestedEntry {
            canonical_id: case.case_id.clone(),
            source: case.source.clone(),
            request_digest: request::request_digest(&case.relative_path),
        }];
        let mut run = ProbeRun::new(case.case_id.clone(), requested);
        if terminated.is_none() {
            match write_probe(&mut child, case) {
                Ok(()) => drive_probe(&mut run, &lines, deadlines, &mut terminated, &mut child),
                Err(message) => {
                    run.record_harness_failure(message);
                }
            }
        } else {
            run.record_harness_failure("the driver terminated before this probe was sent");
        }
        let elapsed_ns = run.compile.as_ref().map(|frame| frame.elapsed_ns);
        let observations = run.finish(manifest, terminated);
        results.push(CaseResult {
            case_id: case.case_id.clone(),
            request_digest: request::request_digest(&case.relative_path),
            elapsed_ns,
            observation: observations
                .into_iter()
                .next()
                .expect("one requested entry yields one observation"),
        });
    }
    drop(child.stdin.take());
    let _ = child.wait();
    Ok(results)
}

fn write_probe(child: &mut Child, case: &PlannedCase) -> Result<(), String> {
    let request: serde_json::Value =
        serde_json::from_str(&request::substitute(&case.relative_path))
            .map_err(|error| format!("the canonical request is not JSON: {error}"))?;
    let probe = serde_json::json!({
        "probe_id": case.case_id,
        "entries": [{
            "canonicalId": case.case_id,
            "source": case.source,
            "request": request,
        }],
    });
    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| "the driver exposes no stdin".to_string())?;
    writeln!(stdin, "{probe}").map_err(|error| error.to_string())?;
    stdin.flush().map_err(|error| error.to_string())
}

fn drive_probe(
    run: &mut ProbeRun,
    lines: &Receiver<Result<String, String>>,
    deadlines: PhaseDeadlines,
    terminated: &mut Option<ExecutionEvent>,
    child: &mut Child,
) {
    let mut deadline = Instant::now() + deadlines.load;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match lines.recv_timeout(remaining) {
            Ok(Ok(line)) => match parse_line(&line) {
                Ok(parsed) => {
                    if let DriverLine::Phase { phase, .. } = &parsed {
                        deadline = Instant::now()
                            + match phase {
                                Phase::Load => deadlines.load,
                                Phase::Compile => deadlines.compile,
                                Phase::Reference => deadlines.reference,
                            };
                    }
                    let done = matches!(parsed, DriverLine::Reference { .. });
                    let _ = run.ingest_frame(parsed);
                    if done {
                        return;
                    }
                }
                Err(message) => {
                    run.record_harness_failure(format!("unparseable driver line: {message}"));
                    return;
                }
            },
            Ok(Err(message)) => {
                run.record_harness_failure(format!("reading the driver: {message}"));
                *terminated = Some(ExecutionEvent::Exited {
                    phase: run.phase(),
                    code: -1,
                });
                return;
            }
            Err(RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                *terminated = Some(ExecutionEvent::TimedOut { phase: run.phase() });
                return;
            }
            Err(RecvTimeoutError::Disconnected) => {
                *terminated = Some(match child.wait() {
                    Ok(status) => match status.code() {
                        Some(code) => ExecutionEvent::Exited {
                            phase: run.phase(),
                            code,
                        },
                        None => ExecutionEvent::Signaled { phase: run.phase() },
                    },
                    Err(_) => ExecutionEvent::Signaled { phase: run.phase() },
                });
                return;
            }
        }
    }
}

fn spawn_reader(stdout: ChildStdout) -> Receiver<Result<String, String>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let message = line.map_err(|error| error.to_string());
            if sender.send(message).is_err() {
                return;
            }
        }
    });
    receiver
}

/// The framework a case id names, for a runner that never branches on one.
pub fn framework_of(case_id: &str) -> Option<Framework> {
    match case_id.split('/').next() {
        Some("vue") => Some(Framework::Vue),
        Some("svelte") => Some(Framework::Svelte),
        _ => None,
    }
}
