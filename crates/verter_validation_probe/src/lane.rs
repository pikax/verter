//! The lane: manifest in, summary out.
//!
//! This is the whole of the lane's orchestration, and it is deliberately
//! small — planning a selection, driving it, and folding the result. Every
//! decision it makes is the manifest's or the corpus adapter's:
//!
//! * WHICH cases run is the manifest's `smoke` slice or its complete
//!   inventory, never a filter this module invents;
//! * WHETHER the checkout is the pinned one is read from its own Git state, so
//!   a summary can never record a revision its cases did not come from;
//! * WHETHER the inventory is still true is checked against the checkout in
//!   BOTH directions, so a fixture added or removed upstream fails the lane
//!   rather than silently changing what it covers;
//! * WHAT an outcome means is [`crate::runner`]'s and the manifest's, never
//!   re-decided here;
//! * WHICH frameworks run is [`Framework::ALL`], the closed target set, and
//!   all of them fold into ONE summary. `framework` is a case attribute, so a
//!   second corpus is a second manifest and a second [`Corpus`] constant, not
//!   a second lane.
//!
//! The module exists only under `external-corpus`: without a pinned checkout
//! there is no workload, and a lane that answered anyway would be reporting
//! about nothing.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::corpus::{Corpus, CorpusError};
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
    Corpus {
        /// Whose corpus.
        framework: Framework,
        /// What went wrong.
        error: CorpusError,
    },
    /// The checkout is at a commit other than the pinned one.
    RevisionDrift {
        /// Whose corpus.
        framework: Framework,
        /// What the manifest pins.
        pinned: String,
        /// What the checkout is at.
        checked_out: String,
    },
    /// The manifest's inventory and the checkout disagree.
    InventoryDrift {
        /// Whose corpus.
        framework: Framework,
        /// Inventoried, absent from the checkout.
        missing: Vec<String>,
        /// Present in the checkout, not inventoried.
        unlisted: Vec<String>,
    },
    /// An inventoried case's bytes are not the ones the manifest digests.
    ///
    /// A GENERATED corpus has no committed bytes to pin, so the digest is the
    /// only thing standing between "the generator produced what was ratified"
    /// and "the generator produced something else and the lane classified it
    /// anyway".
    CaseDigestDrift {
        /// Whose corpus.
        framework: Framework,
        /// The case.
        case_id: String,
        /// What the manifest records.
        recorded: String,
        /// What the checkout holds.
        found: String,
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
            LaneError::Corpus { framework, error } => write!(f, "{framework}: {error}"),
            LaneError::RevisionDrift {
                framework,
                pinned,
                checked_out,
            } => write!(
                f,
                "{framework}: the manifest pins `{pinned}` but the checkout is at \
                 `{checked_out}`; every classification would be recorded against a revision \
                 its cases did not come from"
            ),
            LaneError::InventoryDrift {
                framework,
                missing,
                unlisted,
            } => write!(
                f,
                "{framework}: the manifest inventory and the pinned checkout disagree; \
                 inventoried but absent: [{}]; present but unlisted: [{}]",
                missing.join(", "),
                unlisted.join(", ")
            ),
            LaneError::CaseDigestDrift {
                framework,
                case_id,
                recorded,
                found,
            } => write!(
                f,
                "{framework}: `{case_id}` digests to `{found}`, but the manifest ratified \
                 `{recorded}`; the corpus generator no longer produces the bytes this \
                 inventory was reviewed against"
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

/// Load and validate one framework's committed manifest.
pub fn load_manifest(framework: Framework) -> Result<ProbeStateManifest, LaneError> {
    load_manifest_from(&manifest_dir(), framework)
}

/// Load and validate one framework's manifest from `dir`.
///
/// The directory is a PARAMETER so the lane can be driven over a planted
/// manifest without overwriting the committed one: a lane whose zero-selection
/// refusal could only be proven by editing the file it ships is a refusal
/// nobody can test.
pub fn load_manifest_from(
    dir: &Path,
    framework: Framework,
) -> Result<ProbeStateManifest, LaneError> {
    let file_name = format!("{}.toml", framework.as_str());
    let path = dir.join(&file_name);
    let text = disk::read_text(&path).map_err(|error| LaneError::ManifestUnreadable {
        path: error.path,
        message: error.message,
    })?;
    ProbeStateManifest::from_manifest_file(&file_name, &text).map_err(LaneError::Manifest)
}

/// Check the checkout is at the commit the manifest pins.
pub fn check_revision(manifest: &ProbeStateManifest) -> Result<(), LaneError> {
    let framework = manifest.framework;
    let checked_out = Corpus::for_framework(framework)
        .checkout_revision()
        .map_err(|error| LaneError::Corpus { framework, error })?;
    if checked_out == manifest.external_revision.as_str() {
        Ok(())
    } else {
        Err(LaneError::RevisionDrift {
            framework,
            pinned: manifest.external_revision.as_str().to_string(),
            checked_out,
        })
    }
}

/// Check the manifest's inventory against the pinned checkout, in both
/// directions.
pub fn check_inventory(manifest: &ProbeStateManifest) -> Result<(), LaneError> {
    let framework = manifest.framework;
    let discovered: BTreeSet<String> = Corpus::for_framework(framework)
        .discover_case_ids()
        .map_err(|error| LaneError::Corpus { framework, error })?
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
        Err(LaneError::InventoryDrift {
            framework,
            missing,
            unlisted,
        })
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

/// Load every selected case's bytes, checking each against the digest the
/// manifest ratified for it.
///
/// A corpus whose cases are COMMITTED upstream is already pinned by the
/// revision check; one whose cases are GENERATED at that revision is not, so a
/// per-case digest is the manifest's own record of the bytes it was reviewed
/// against. A case the manifest digests must match; a case it does not is
/// carried as read, which is what keeps the committed-corpus adapter from
/// having to invent digests it has no need for.
pub fn plan(
    manifest: &ProbeStateManifest,
    selected: &[String],
) -> Result<Vec<PlannedCase>, LaneError> {
    let framework = manifest.framework;
    let corpus = Corpus::for_framework(framework);
    let digests: std::collections::BTreeMap<&str, &str> = manifest
        .inventory
        .iter()
        .filter_map(|case| {
            case.digest
                .as_ref()
                .map(|digest| (case.case_id.as_str(), digest.as_str()))
        })
        .collect();
    selected
        .iter()
        .map(|case_id| {
            let case = corpus
                .load_case(case_id)
                .map_err(|error| LaneError::Corpus { framework, error })?;
            if let Some(recorded) = digests.get(case_id.as_str()) {
                let found = request::sha256_hex(case.source.as_bytes());
                if found != *recorded {
                    return Err(LaneError::CaseDigestDrift {
                        framework,
                        case_id: case_id.clone(),
                        recorded: (*recorded).to_string(),
                        found,
                    });
                }
            }
            Ok(PlannedCase {
                case_id: case.case_id,
                relative_path: case.relative_path,
                source: case.source,
            })
        })
        .collect()
}

/// Run one lane end to end and return its summary.
pub fn run(lane: Lane, deadlines: PhaseDeadlines) -> Result<Summary, LaneError> {
    run_with_manifest_dir(lane, deadlines, &manifest_dir())
}

/// Run one lane end to end over the manifests in `manifest_dir`.
///
/// EVERY framework runs, through the one runner, into the one summary. The
/// set is [`Framework::ALL`], not a list this module keeps: a framework that
/// exists and is not covered here would publish a summary that reported on one
/// corpus and said nothing about the other, and the summary's own coverage
/// check refuses exactly that.
pub fn run_with_manifest_dir(
    lane: Lane,
    deadlines: PhaseDeadlines,
    manifest_dir: &Path,
) -> Result<Summary, LaneError> {
    let driver = DriverCommand::committed(&crate::corpus::workspace_root());
    let mut manifests = Vec::with_capacity(Framework::ALL.len());
    let mut slices: Vec<(Vec<String>, Vec<ObservedCase>)> =
        Vec::with_capacity(Framework::ALL.len());
    for framework in Framework::ALL {
        let manifest = load_manifest_from(manifest_dir, framework)?;
        check_revision(&manifest)?;
        check_inventory(&manifest)?;
        let selected = selection(&manifest, lane);
        let cases = plan(&manifest, &selected)?;
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
        manifests.push(manifest);
        slices.push((selected, observed));
    }

    let runs: Vec<FrameworkRun<'_>> = manifests
        .iter()
        .zip(slices)
        .map(|(manifest, (selected, observed))| FrameworkRun {
            manifest,
            request_template: request::template_for(manifest.framework),
            selected,
            observed,
        })
        .collect();
    let summary = summary::build(lane, &runs).map_err(LaneError::Summary)?;
    summary
        .require_every_framework()
        .map_err(LaneError::Summary)?;
    Ok(summary)
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

/// One framework's canonical request template and its digest, for a caller
/// recording what the lane asked without re-deriving it.
pub fn request_identity(framework: Framework) -> (&'static str, String) {
    (
        request::template_for(framework),
        request::template_digest(framework),
    )
}
