//! The lane: manifest in, summary out.
//!
//! This is the whole of the lane's orchestration, and it is deliberately
//! small — planning a selection, driving it, and folding the result. Every
//! decision it makes is the manifest's or the corpus adapter's:
//!
//! * WHICH cases run is the manifest's `smoke` slice or its complete
//!   inventory, never a filter this module invents;
//! * WHETHER the inventory is still true is checked against the checkout in
//!   BOTH directions, so a fixture added or removed upstream fails the lane
//!   rather than silently changing what it covers;
//! * WHAT an outcome means is [`crate::runner`]'s and the manifest's, never
//!   re-decided here.
//!
//! The module exists only under `external-corpus`: without a pinned checkout
//! there is no workload, and a lane that answered anyway would be reporting
//! about nothing.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::corpus::{vue_benchmarks, CorpusError};
use crate::disk;
use crate::manifest::{Framework, ManifestError, ProbeStateManifest};
use crate::request;
use crate::runner::{self, DriverCommand, PhaseDeadlines, PlannedCase, RunError};
use crate::summary::{self, FrameworkRun, Lane, ObservedCase, Summary, SummaryError};

/// Where the lane writes its one machine-readable artifact.
pub const SUMMARY_RELATIVE_PATH: &str = "target/validation-probe/summary.json";

/// Why a lane could not produce a summary.
#[derive(Debug)]
pub enum LaneError {
    /// The manifest file could not be read.
    ManifestUnreadable {
        /// The path.
        path: PathBuf,
        /// The operating system's message.
        message: String,
    },
    /// The manifest is not a valid probe-state manifest.
    Manifest(ManifestError),
    /// The corpus could not be read.
    Corpus(CorpusError),
    /// The manifest's inventory and the checkout disagree.
    InventoryDrift {
        /// Inventoried, absent from the checkout.
        missing: Vec<String>,
        /// Present in the checkout, not inventoried.
        unlisted: Vec<String>,
    },
    /// The lane could not be driven.
    Run(RunError),
    /// An observation could not be represented, or a cell could not be
    /// evaluated against it.
    Observation(String),
    /// The summary is not internally consistent.
    Summary(SummaryError),
    /// The summary artifact could not be written.
    SummaryUnwritable {
        /// The path.
        path: PathBuf,
        /// The operating system's message.
        message: String,
    },
}

impl fmt::Display for LaneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaneError::ManifestUnreadable { path, message } => {
                write!(f, "reading {}: {message}", path.display())
            }
            LaneError::Manifest(error) => write!(f, "{error}"),
            LaneError::Corpus(error) => write!(f, "{error}"),
            LaneError::InventoryDrift { missing, unlisted } => write!(
                f,
                "the manifest inventory and the pinned checkout disagree; \
                 inventoried but absent: [{}]; present but unlisted: [{}]",
                missing.join(", "),
                unlisted.join(", ")
            ),
            LaneError::Run(error) => write!(f, "{error}"),
            LaneError::Observation(message) => f.write_str(message),
            LaneError::Summary(error) => write!(f, "{error}"),
            LaneError::SummaryUnwritable { path, message } => {
                write!(f, "writing {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for LaneError {}

/// The manifest directory this crate owns.
pub fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest")
}

/// Load and validate one framework's manifest.
pub fn load_manifest(framework: Framework) -> Result<ProbeStateManifest, LaneError> {
    let file_name = format!("{}.toml", framework.as_str());
    let path = manifest_dir().join(&file_name);
    let text = disk::read_text(&path).map_err(|error| LaneError::ManifestUnreadable {
        path: error.path,
        message: error.message,
    })?;
    ProbeStateManifest::from_manifest_file(&file_name, &text).map_err(LaneError::Manifest)
}

/// Check the manifest's inventory against the pinned checkout, in both
/// directions.
pub fn check_inventory(manifest: &ProbeStateManifest) -> Result<(), LaneError> {
    let discovered: BTreeSet<String> = vue_benchmarks::discover_case_ids()
        .map_err(LaneError::Corpus)?
        .into_iter()
        .collect();
    let inventoried: BTreeSet<&str> = manifest
        .inventory
        .iter()
        .map(|case| case.case_id.as_str())
        .collect();
    let missing: Vec<String> = inventoried
        .iter()
        .filter(|case_id| !discovered.contains(**case_id))
        .map(|case_id| case_id.to_string())
        .collect();
    let unlisted: Vec<String> = discovered
        .iter()
        .filter(|case_id| !inventoried.contains(case_id.as_str()))
        .cloned()
        .collect();
    if missing.is_empty() && unlisted.is_empty() {
        Ok(())
    } else {
        Err(LaneError::InventoryDrift { missing, unlisted })
    }
}

/// The case ids `lane` selects from `manifest`.
pub fn selection(manifest: &ProbeStateManifest, lane: Lane) -> Vec<String> {
    match lane {
        Lane::Smoke => manifest.smoke.clone(),
        Lane::Main => manifest
            .inventory
            .iter()
            .map(|case| case.case_id.clone())
            .collect(),
    }
}

/// Load every selected case's bytes.
pub fn plan(selected: &[String]) -> Result<Vec<PlannedCase>, LaneError> {
    selected
        .iter()
        .map(|case_id| {
            vue_benchmarks::load_case(case_id)
                .map(|case| PlannedCase {
                    case_id: case.case_id,
                    relative_path: case.relative_path,
                    source: case.source,
                })
                .map_err(LaneError::Corpus)
        })
        .collect()
}

/// Run one lane end to end and return its summary.
pub fn run(lane: Lane, deadlines: PhaseDeadlines) -> Result<Summary, LaneError> {
    let manifest = load_manifest(Framework::Vue)?;
    check_inventory(&manifest)?;
    let selected = selection(&manifest, lane);
    let cases = plan(&selected)?;
    let driver = DriverCommand::committed(&crate::corpus::workspace_root());
    let results =
        runner::run_cases(&manifest, &driver, &cases, deadlines).map_err(LaneError::Run)?;

    let mut observed = Vec::with_capacity(results.len());
    for result in results {
        let observation = result
            .observation
            .map_err(|error| LaneError::Observation(format!("{}: {error}", result.case_id)))?;
        observed.push(ObservedCase {
            case_id: result.case_id,
            request_digest: result.request_digest,
            elapsed_ns: result.elapsed_ns,
            observation,
        });
    }

    let run = FrameworkRun {
        manifest: &manifest,
        selected,
        observed,
    };
    summary::build(lane, std::slice::from_ref(&run)).map_err(LaneError::Summary)
}

/// Write the summary artifact and return where it landed.
pub fn write_summary(summary: &Summary) -> Result<PathBuf, LaneError> {
    let path = summary_path();
    disk::write_text(&path, &summary.to_json()).map_err(|error| LaneError::SummaryUnwritable {
        path: error.path,
        message: error.message,
    })?;
    Ok(path)
}

/// Where the summary artifact lands.
pub fn summary_path() -> PathBuf {
    resolve(&crate::corpus::workspace_root(), SUMMARY_RELATIVE_PATH)
}

fn resolve(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.to_path_buf(), |path, segment| path.join(segment))
}

/// The canonical request template and its digest, for a caller recording what
/// the lane asked without re-deriving it.
pub fn request_identity() -> (&'static str, String) {
    (request::REQUEST_VUE, request::template_digest())
}
