//! The probe-state manifest: one TOML file per framework at
//! `manifest/<framework>.toml`, the only manifest input any validator reads.
//!
//! A manifest pins one external corpus revision, lists the complete ratified
//! case inventory with its representative strata, and declares one cell per
//! `{ probe_id, dimension }` with an explicit expected state. Every cell
//! cites the `{ authority, atom }` that owns (or will own) its behaviour;
//! whether that citation is admissible for the cell's framework, dimension
//! and state is decided by the authority validator against the
//! validation-authority catalog and the implementation ledger.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::authority::Authority;
use crate::outcome::{
    CaseObservation, Dimension, InvalidObservation, NotApplicableReason, ProbeOutcomeClass,
    Terminal,
};

/// The largest smoke slice a pull-request lane accepts.
///
/// The bound is STRUCTURAL, not a wall clock: a lane whose size is a time
/// budget grows silently as machines get faster and shrinks as they get
/// loaded, and neither tells a reviewer what the required job covers.
pub const MAX_SMOKE_CASES: usize = 24;

/// A first-class Verter target framework. Always a case attribute, never a
/// runner variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Framework {
    /// Vue single-file components.
    Vue,
    /// Svelte components.
    Svelte,
}

impl Framework {
    /// The serialized name, also the manifest file stem and case-id prefix.
    pub const fn as_str(self) -> &'static str {
        match self {
            Framework::Vue => "vue",
            Framework::Svelte => "svelte",
        }
    }
}

impl fmt::Display for Framework {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether a framework's `Structural` cells run a bound comparator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Comparison {
    /// A structural comparator is bound; the manifest names it exactly.
    Structural,
    /// No comparator is bound; every `Structural` cell is an owned `skip`.
    None,
}

/// The exact structural comparator a framework's product authority supplies.
/// Must equal the authority catalog's comparator identity verbatim.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparatorIdentity {
    /// The crate that owns the comparator.
    #[serde(rename = "crate")]
    pub krate: String,
    /// The comparator's source path inside that crate.
    pub path: String,
    /// The comparator function.
    pub function: String,
    /// The durable atom of the product authority that owns the comparison.
    pub atom: String,
}

/// Whether a dimension's surface is bound for a framework.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Applicability {
    /// The surface is bound and the dimension is exercised.
    Applicable,
    /// No surface is bound; every cell of the dimension is an owned `skip`.
    Inapplicable,
}

/// Which optional dimensions a framework's probes exercise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicabilityFlags {
    /// The `Runtime` dimension.
    pub runtime: Applicability,
    /// The `Map` dimension.
    pub map: Applicability,
}

/// A full commit id: forty lowercase hexadecimal digits.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha40(String);

/// A SHA-256 digest: sixty-four lowercase hexadecimal digits.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256(String);

fn lowercase_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl TryFrom<String> for Sha40 {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if lowercase_hex(&value, 40) {
            Ok(Sha40(value))
        } else {
            Err(format!(
                "`{value}` is not a full commit id of forty lowercase hexadecimal digits"
            ))
        }
    }
}

impl From<Sha40> for String {
    fn from(value: Sha40) -> Self {
        value.0
    }
}

impl Sha40 {
    /// The hex digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Sha256 {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if lowercase_hex(&value, 64) {
            Ok(Sha256(value))
        } else {
            Err(format!(
                "`{value}` is not a SHA-256 digest of sixty-four lowercase hexadecimal digits"
            ))
        }
    }
}

impl From<Sha256> for String {
    fn from(value: Sha256) -> Self {
        value.0
    }
}

impl Sha256 {
    /// The hex digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A reviewed representative family of cases with its minimum coverage.
///
/// `pattern` is a `*`-glob over a case's file name (the last path segment).
/// Each case belongs to the first stratum, in declaration order, whose
/// pattern matches it, so a trailing `*` stratum collects the remainder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stratum {
    /// Durable lower-kebab id.
    pub id: String,
    /// `*`-glob over the case file name.
    pub pattern: String,
    /// The minimum number of inventory cases the stratum must hold.
    pub min_cases: u32,
}

/// One ratified case of the pinned corpus.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// `<framework>/<relative-path-in-corpus>`.
    pub case_id: String,
    /// Content digest, for a corpus that is generated rather than committed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<Sha256>,
}

/// A cell's declared expectation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExpectedState {
    /// Blocks on anything but its exact expected class. Admitted only through
    /// an implemented authority whose cited atom lists that class.
    Gate,
    /// Non-blocking, owned, with an exact expected class: a known failure
    /// class, or `pass` for a currently passing cell whose authority is not
    /// yet implemented.
    Canary,
    /// A non-blocking owned failure; an unexpected pass is an XPASS promotion
    /// candidate, never an automatic gate.
    KnownFail,
    /// Execution is meaningless, impossible, or unstable. Owned and reasoned.
    Skip,
}

impl ExpectedState {
    /// The serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            ExpectedState::Gate => "gate",
            ExpectedState::Canary => "canary",
            ExpectedState::KnownFail => "known-fail",
            ExpectedState::Skip => "skip",
        }
    }
}

/// One manifest cell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeEntry {
    /// The case: `<framework>/<case>`.
    pub probe_id: String,
    /// The case's framework.
    pub framework: Framework,
    /// The case's relative path inside the corpus.
    pub case: String,
    /// The one dimension this cell expects.
    pub dimension: Dimension,
    /// The declared expectation.
    pub expected_state: ExpectedState,
    /// The exact expected terminal class; required for every state but
    /// `skip`, forbidden for `skip`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_class: Option<ProbeOutcomeClass>,
    /// The owning authority; required for every state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<Authority>,
    /// The durable atom of the owning authority; required for every state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atom: Option<String>,
    /// A terse reason; required for `skip`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The pinned corpus revision, when the cell restates it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_revision: Option<Sha40>,
}

impl ProbeEntry {
    /// The cell's canonical identity.
    pub fn cell(&self) -> CellId {
        CellId {
            probe_id: self.probe_id.clone(),
            dimension: self.dimension,
        }
    }
}

/// The canonical cell identity, unique per manifest and the key every
/// totality and deduplication check uses.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellId {
    /// The case.
    pub probe_id: String,
    /// The dimension.
    pub dimension: Dimension,
}

impl fmt::Display for CellId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]", self.probe_id, self.dimension)
    }
}

/// The machine-readable probe-state manifest of one framework.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeStateManifest {
    /// The framework every case and cell belongs to.
    pub framework: Framework,
    /// Whether `Structural` cells run a bound comparator.
    pub comparison: Comparison,
    /// The bound comparator; present exactly when `comparison = structural`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparator: Option<ComparatorIdentity>,
    /// Which optional dimensions are bound.
    pub applicability: ApplicabilityFlags,
    /// The full pinned corpus commit.
    pub external_revision: Sha40,
    /// Reviewed representative families with their minimum coverage.
    #[serde(default)]
    pub strata: Vec<Stratum>,
    /// The complete ratified case list.
    #[serde(default)]
    pub inventory: Vec<Case>,
    /// The deterministic pull-request smoke slice, by case id.
    ///
    /// Listed rather than computed at run time, and then checked to EQUAL its
    /// own derivation — the lexicographic first `min_cases` of each stratum in
    /// declaration order. A reviewer reads exactly what a pull request runs,
    /// and a hand-edited, reordered, emptied, or padded slice fails
    /// [`ProbeStateManifest::validate`] instead of quietly changing what the
    /// required lane covers.
    #[serde(default)]
    pub smoke: Vec<String>,
    /// One cell per `{ probe_id, dimension }` for every inventory case.
    #[serde(default)]
    pub entries: Vec<ProbeEntry>,
}

/// One reason a manifest is invalid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestViolation {
    /// The manifest file name differs from its framework.
    FileNameMismatch {
        /// The file name found.
        file_name: String,
    },
    /// No case is inventoried.
    EmptyInventory,
    /// A case id is not `<framework>/<relative-path>`.
    InvalidCaseId {
        /// The case id.
        case_id: String,
    },
    /// A case is inventoried twice.
    DuplicateCase {
        /// The case id.
        case_id: String,
    },
    /// A stratum id is not durable lower-kebab, its pattern is not a file-name
    /// glob, or it requires zero cases.
    InvalidStratum {
        /// The stratum id.
        id: String,
    },
    /// Two strata share an id.
    DuplicateStratum {
        /// The stratum id.
        id: String,
    },
    /// A stratum holds fewer inventory cases than its minimum.
    UnderfilledStratum {
        /// The stratum id.
        id: String,
        /// The declared minimum.
        min_cases: u32,
        /// The cases the stratum holds.
        matched: usize,
    },
    /// `comparison = structural` without a comparator identity.
    ComparatorMissing,
    /// A comparator identity under `comparison = none`.
    ComparatorForbidden,
    /// The comparator's atom is not a durable id.
    ComparatorAtomNotDurable {
        /// The atom.
        atom: String,
    },
    /// A cell of another framework.
    FrameworkMismatch {
        /// The cell.
        cell: CellId,
    },
    /// A cell whose `probe_id` is not `<framework>/<case>`.
    ProbeIdMismatch {
        /// The cell.
        cell: CellId,
    },
    /// A cell for a case the inventory does not list.
    UnknownCase {
        /// The cell.
        cell: CellId,
    },
    /// Two cells share one `{ probe_id, dimension }`.
    DuplicateCell {
        /// The cell.
        cell: CellId,
    },
    /// An inventoried case lacks the cell for one dimension.
    MissingCell {
        /// The absent cell.
        cell: CellId,
    },
    /// A cell restates a corpus revision other than the pinned one.
    RevisionMismatch {
        /// The cell.
        cell: CellId,
    },
    /// A cell without its `{ authority, atom }` owner citation.
    MissingCitation {
        /// The cell.
        cell: CellId,
    },
    /// A cited atom that is not a durable lower-kebab id.
    AtomNotDurable {
        /// The cell.
        cell: CellId,
        /// The atom.
        atom: String,
    },
    /// A gate, canary, or known-fail without an exact expected class.
    MissingClass {
        /// The cell.
        cell: CellId,
    },
    /// A skip that names an expected class.
    ClassOnSkip {
        /// The cell.
        cell: CellId,
    },
    /// A known-fail expecting `pass`, which would suppress XPASS.
    KnownFailExpectsPass {
        /// The cell.
        cell: CellId,
    },
    /// An expected class the cell's dimension does not admit.
    InadmissibleClass {
        /// The cell.
        cell: CellId,
        /// The class.
        class: ProbeOutcomeClass,
    },
    /// A skip without a reason.
    SkipWithoutReason {
        /// The cell.
        cell: CellId,
    },
    /// A cell of a dimension the manifest declares inapplicable that is not
    /// an owned skip.
    InapplicableCellNotSkipped {
        /// The cell.
        cell: CellId,
    },
    /// The smoke slice selects no case, so the required lane would publish a
    /// green summary having run nothing.
    EmptySmokeSlice,
    /// The smoke slice is larger than the bound a pull-request lane accepts.
    SmokeSliceTooLarge {
        /// The cases listed.
        listed: usize,
    },
    /// The smoke slice is not the deterministic derivation of its own strata.
    SmokeSliceNotDerived {
        /// The lexicographic first-`min_cases`-per-stratum slice.
        expected: Vec<String>,
        /// What the manifest lists.
        listed: Vec<String>,
    },
}

impl fmt::Display for ManifestViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestViolation::FileNameMismatch { file_name } => {
                write!(
                    f,
                    "manifest file `{file_name}` does not match its framework"
                )
            }
            ManifestViolation::EmptyInventory => f.write_str("the inventory lists no case"),
            ManifestViolation::InvalidCaseId { case_id } => {
                write!(
                    f,
                    "case id `{case_id}` is not `<framework>/<relative-path>`"
                )
            }
            ManifestViolation::DuplicateCase { case_id } => {
                write!(f, "case `{case_id}` is inventoried twice")
            }
            ManifestViolation::InvalidStratum { id } => write!(
                f,
                "stratum `{id}` needs a durable id, a file-name glob, and min_cases >= 1"
            ),
            ManifestViolation::DuplicateStratum { id } => {
                write!(f, "stratum `{id}` is declared twice")
            }
            ManifestViolation::UnderfilledStratum {
                id,
                min_cases,
                matched,
            } => write!(
                f,
                "stratum `{id}` holds {matched} case(s), fewer than {min_cases}"
            ),
            ManifestViolation::ComparatorMissing => {
                f.write_str("comparison = structural requires a comparator identity")
            }
            ManifestViolation::ComparatorForbidden => {
                f.write_str("comparison = none forbids a comparator identity")
            }
            ManifestViolation::ComparatorAtomNotDurable { atom } => {
                write!(f, "comparator atom `{atom}` is not a durable id")
            }
            ManifestViolation::FrameworkMismatch { cell } => {
                write!(
                    f,
                    "{cell}: cell framework differs from the manifest framework"
                )
            }
            ManifestViolation::ProbeIdMismatch { cell } => {
                write!(f, "{cell}: probe_id is not `<framework>/<case>`")
            }
            ManifestViolation::UnknownCase { cell } => {
                write!(f, "{cell}: case is not in the inventory")
            }
            ManifestViolation::DuplicateCell { cell } => write!(f, "{cell}: cell declared twice"),
            ManifestViolation::MissingCell { cell } => write!(f, "{cell}: cell missing"),
            ManifestViolation::RevisionMismatch { cell } => {
                write!(
                    f,
                    "{cell}: external_revision differs from the pinned revision"
                )
            }
            ManifestViolation::MissingCitation { cell } => {
                write!(f, "{cell}: no {{ authority, atom }} owner citation")
            }
            ManifestViolation::AtomNotDurable { cell, atom } => {
                write!(f, "{cell}: atom `{atom}` is not a durable lower-kebab id")
            }
            ManifestViolation::MissingClass { cell } => {
                write!(f, "{cell}: expected state requires an exact expected_class")
            }
            ManifestViolation::ClassOnSkip { cell } => {
                write!(f, "{cell}: a skip carries no expected_class")
            }
            ManifestViolation::KnownFailExpectsPass { cell } => {
                write!(f, "{cell}: a known-fail cannot expect pass")
            }
            ManifestViolation::InadmissibleClass { cell, class } => {
                write!(
                    f,
                    "{cell}: class {class} is not admissible at this dimension"
                )
            }
            ManifestViolation::SkipWithoutReason { cell } => {
                write!(f, "{cell}: a skip requires a reason")
            }
            ManifestViolation::InapplicableCellNotSkipped { cell } => {
                write!(
                    f,
                    "{cell}: the dimension is inapplicable, so the cell must be a skip"
                )
            }
            ManifestViolation::EmptySmokeSlice => f.write_str("the smoke slice selects no case"),
            ManifestViolation::SmokeSliceTooLarge { listed } => write!(
                f,
                "the smoke slice lists {listed} cases, above the {MAX_SMOKE_CASES} a \
                 pull-request lane accepts"
            ),
            ManifestViolation::SmokeSliceNotDerived { expected, listed } => write!(
                f,
                "the smoke slice is not the lexicographic first-min_cases-per-stratum \
                 derivation; expected [{}], found [{}]",
                expected.join(", "),
                listed.join(", ")
            ),
        }
    }
}

/// Why a manifest could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// The text is not a manifest (malformed TOML, unknown field, or a value
    /// outside a closed vocabulary such as an outcome alias).
    Parse(String),
    /// The manifest parsed but violates the contract.
    Invalid(Vec<ManifestViolation>),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::Parse(message) => write!(f, "manifest malformed: {message}"),
            ManifestError::Invalid(violations) => {
                f.write_str("manifest invalid:")?;
                for violation in violations {
                    write!(f, "\n  {violation}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ManifestError {}

/// Whether `value` is a durable lower-kebab id: `[a-z][a-z0-9]*(-[a-z0-9]+)*`.
/// Upper-case, node-shaped, acceptance-id-shaped and section-path values are
/// all outside this form.
pub fn is_durable_id(value: &str) -> bool {
    let mut segments = value.split('-');
    let Some(first) = segments.next() else {
        return false;
    };
    let lower_alnum = |segment: &str| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    };
    lower_alnum(first) && first.as_bytes()[0].is_ascii_lowercase() && segments.all(lower_alnum)
}

fn valid_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn valid_pattern(pattern: &str) -> bool {
    !pattern.is_empty()
        && pattern
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'*' | b'.' | b'_' | b'-'))
}

/// `*`-glob match; `*` matches any run of characters.
fn glob_matches(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or("");
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let tail: Vec<&str> = parts.collect();
    let Some((last, middle)) = tail.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

impl ProbeStateManifest {
    /// Parse and validate manifest TOML.
    pub fn from_toml_str(text: &str) -> Result<Self, ManifestError> {
        let manifest: ProbeStateManifest =
            toml::from_str(text).map_err(|error| ManifestError::Parse(error.to_string()))?;
        manifest.validate().map_err(ManifestError::Invalid)?;
        Ok(manifest)
    }

    /// Parse and validate the text of the manifest file `file_name`
    /// (`<framework>.toml`), including that the file name names the
    /// manifest's framework. The caller reads the file.
    pub fn from_manifest_file(file_name: &str, text: &str) -> Result<Self, ManifestError> {
        let manifest: ProbeStateManifest =
            toml::from_str(text).map_err(|error| ManifestError::Parse(error.to_string()))?;
        let mut violations = manifest.validate().err().unwrap_or_default();
        if file_name != format!("{}.toml", manifest.framework.as_str()) {
            violations.insert(
                0,
                ManifestViolation::FileNameMismatch {
                    file_name: file_name.to_string(),
                },
            );
        }
        if violations.is_empty() {
            Ok(manifest)
        } else {
            Err(ManifestError::Invalid(violations))
        }
    }

    /// The not-applicable reason this manifest declares for `dimension`, if
    /// its comparison or applicability header makes the dimension
    /// inapplicable. Such a dimension's cells must be owned skips.
    pub fn not_applicable_reason(&self, dimension: Dimension) -> Option<NotApplicableReason> {
        let (inapplicable, reason) = match dimension {
            Dimension::Structural => (
                self.comparison == Comparison::None,
                NotApplicableReason::ComparatorAbsent,
            ),
            Dimension::Runtime => (
                self.applicability.runtime == Applicability::Inapplicable,
                NotApplicableReason::RuntimeExecutorAbsent,
            ),
            Dimension::Map => (
                self.applicability.map == Applicability::Inapplicable,
                NotApplicableReason::MapValidatorAbsent,
            ),
            Dimension::Route | Dimension::Compile | Dimension::Performance => return None,
        };
        inapplicable.then_some(reason)
    }

    /// Check that an observation reports [`Terminal::NotApplicable`] exactly
    /// where this manifest declares a dimension inapplicable. A driver that
    /// reports inapplicability for a declared-applicable dimension has failed
    /// that dimension; it must be classified as a failure, never accepted as
    /// not applicable, so a missing producer, executor, or validator cannot
    /// fail open.
    pub fn check_applicability(
        &self,
        observation: &CaseObservation,
    ) -> Result<(), InvalidObservation> {
        for dimension in Dimension::ALL {
            let declared = self.not_applicable_reason(dimension);
            let observed = match observation.terminal(dimension) {
                Terminal::NotApplicable { reason } => Some(*reason),
                Terminal::Class { .. } | Terminal::NotRun { .. } => None,
            };
            if declared != observed {
                return Err(InvalidObservation::ApplicabilityMismatch {
                    dimension,
                    declared,
                    observed,
                });
            }
        }
        Ok(())
    }

    /// Check every contract rule; returns every violation, in a
    /// deterministic order.
    pub fn validate(&self) -> Result<(), Vec<ManifestViolation>> {
        let mut violations = Vec::new();
        self.validate_header(&mut violations);
        let inventory = self.validate_inventory(&mut violations);
        self.validate_strata(&mut violations);
        self.validate_smoke(&mut violations);
        self.validate_cells(&inventory, &mut violations);
        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }

    fn validate_header(&self, violations: &mut Vec<ManifestViolation>) {
        match (&self.comparison, &self.comparator) {
            (Comparison::Structural, None) => violations.push(ManifestViolation::ComparatorMissing),
            (Comparison::None, Some(_)) => violations.push(ManifestViolation::ComparatorForbidden),
            (Comparison::Structural, Some(comparator)) if !is_durable_id(&comparator.atom) => {
                violations.push(ManifestViolation::ComparatorAtomNotDurable {
                    atom: comparator.atom.clone(),
                })
            }
            _ => {}
        }
    }

    fn validate_inventory(&self, violations: &mut Vec<ManifestViolation>) -> BTreeSet<String> {
        if self.inventory.is_empty() {
            violations.push(ManifestViolation::EmptyInventory);
        }
        let prefix = format!("{}/", self.framework.as_str());
        let mut seen = BTreeSet::new();
        for case in &self.inventory {
            let relative = case.case_id.strip_prefix(&prefix);
            if !relative.is_some_and(valid_relative_path) {
                violations.push(ManifestViolation::InvalidCaseId {
                    case_id: case.case_id.clone(),
                });
            }
            if !seen.insert(case.case_id.clone()) {
                violations.push(ManifestViolation::DuplicateCase {
                    case_id: case.case_id.clone(),
                });
            }
        }
        seen
    }

    fn validate_strata(&self, violations: &mut Vec<ManifestViolation>) {
        let mut ids = BTreeSet::new();
        for stratum in &self.strata {
            if !is_durable_id(&stratum.id)
                || !valid_pattern(&stratum.pattern)
                || stratum.min_cases == 0
            {
                violations.push(ManifestViolation::InvalidStratum {
                    id: stratum.id.clone(),
                });
            }
            if !ids.insert(stratum.id.as_str()) {
                violations.push(ManifestViolation::DuplicateStratum {
                    id: stratum.id.clone(),
                });
            }
        }
        let mut matched = vec![0usize; self.strata.len()];
        for case in &self.inventory {
            let file_name = case.case_id.rsplit('/').next().unwrap_or_default();
            if let Some(index) = self
                .strata
                .iter()
                .position(|stratum| glob_matches(&stratum.pattern, file_name))
            {
                matched[index] += 1;
            }
        }
        for (stratum, matched) in self.strata.iter().zip(matched) {
            if stratum.min_cases > 0 && matched < stratum.min_cases as usize {
                violations.push(ManifestViolation::UnderfilledStratum {
                    id: stratum.id.clone(),
                    min_cases: stratum.min_cases,
                    matched,
                });
            }
        }
    }

    /// The deterministic smoke slice this manifest's strata and inventory
    /// derive: within each stratum, in declaration order, the lexicographic
    /// first `min_cases` inventory cases that fall into it.
    ///
    /// A case belongs to the FIRST stratum whose pattern matches its file
    /// name, exactly as the stratum-coverage check assigns it, so the two can
    /// never disagree about which family a case counts towards.
    pub fn derived_smoke_slice(&self) -> Vec<String> {
        let mut buckets: Vec<Vec<&str>> = vec![Vec::new(); self.strata.len()];
        for case in &self.inventory {
            let file_name = case.case_id.rsplit('/').next().unwrap_or_default();
            if let Some(index) = self
                .strata
                .iter()
                .position(|stratum| glob_matches(&stratum.pattern, file_name))
            {
                buckets[index].push(case.case_id.as_str());
            }
        }
        let mut slice = Vec::new();
        for (stratum, bucket) in self.strata.iter().zip(buckets.iter_mut()) {
            bucket.sort_unstable();
            slice.extend(
                bucket
                    .iter()
                    .take(stratum.min_cases as usize)
                    .map(|case_id| case_id.to_string()),
            );
        }
        slice
    }

    fn validate_smoke(&self, violations: &mut Vec<ManifestViolation>) {
        if self.smoke.is_empty() {
            violations.push(ManifestViolation::EmptySmokeSlice);
            return;
        }
        if self.smoke.len() > MAX_SMOKE_CASES {
            violations.push(ManifestViolation::SmokeSliceTooLarge {
                listed: self.smoke.len(),
            });
        }
        let expected = self.derived_smoke_slice();
        if expected != self.smoke {
            violations.push(ManifestViolation::SmokeSliceNotDerived {
                expected,
                listed: self.smoke.clone(),
            });
        }
    }

    fn validate_cells(
        &self,
        inventory: &BTreeSet<String>,
        violations: &mut Vec<ManifestViolation>,
    ) {
        let mut cells: BTreeMap<CellId, usize> = BTreeMap::new();
        for entry in &self.entries {
            let cell = entry.cell();
            if entry.framework != self.framework {
                violations.push(ManifestViolation::FrameworkMismatch { cell: cell.clone() });
            }
            if entry.probe_id != format!("{}/{}", entry.framework.as_str(), entry.case) {
                violations.push(ManifestViolation::ProbeIdMismatch { cell: cell.clone() });
            }
            if !inventory.contains(&entry.probe_id) {
                violations.push(ManifestViolation::UnknownCase { cell: cell.clone() });
            }
            if entry
                .external_revision
                .as_ref()
                .is_some_and(|revision| *revision != self.external_revision)
            {
                violations.push(ManifestViolation::RevisionMismatch { cell: cell.clone() });
            }
            self.validate_expectation(entry, &cell, violations);
            let count = cells.entry(cell.clone()).or_insert(0);
            *count += 1;
            if *count == 2 {
                violations.push(ManifestViolation::DuplicateCell { cell });
            }
        }
        for case_id in inventory {
            for dimension in Dimension::ALL {
                let cell = CellId {
                    probe_id: case_id.clone(),
                    dimension,
                };
                if !cells.contains_key(&cell) {
                    violations.push(ManifestViolation::MissingCell { cell });
                }
            }
        }
    }

    fn validate_expectation(
        &self,
        entry: &ProbeEntry,
        cell: &CellId,
        violations: &mut Vec<ManifestViolation>,
    ) {
        match (&entry.authority, &entry.atom) {
            (Some(_), Some(atom)) if !is_durable_id(atom) => {
                violations.push(ManifestViolation::AtomNotDurable {
                    cell: cell.clone(),
                    atom: atom.clone(),
                })
            }
            (Some(_), Some(_)) => {}
            _ => violations.push(ManifestViolation::MissingCitation { cell: cell.clone() }),
        }
        match (entry.expected_state, entry.expected_class) {
            (ExpectedState::Skip, Some(_)) => {
                violations.push(ManifestViolation::ClassOnSkip { cell: cell.clone() })
            }
            (ExpectedState::Skip, None) => {}
            (_, None) => violations.push(ManifestViolation::MissingClass { cell: cell.clone() }),
            (state, Some(class)) => {
                if !entry.dimension.admits(class) {
                    violations.push(ManifestViolation::InadmissibleClass {
                        cell: cell.clone(),
                        class,
                    });
                }
                if state == ExpectedState::KnownFail && class == ProbeOutcomeClass::Pass {
                    violations.push(ManifestViolation::KnownFailExpectsPass { cell: cell.clone() });
                }
            }
        }
        if entry.expected_state == ExpectedState::Skip
            && entry
                .reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
        {
            violations.push(ManifestViolation::SkipWithoutReason { cell: cell.clone() });
        }
        if self.not_applicable_reason(entry.dimension).is_some()
            && entry.expected_state != ExpectedState::Skip
        {
            violations.push(ManifestViolation::InapplicableCellNotSkipped { cell: cell.clone() });
        }
    }
}
