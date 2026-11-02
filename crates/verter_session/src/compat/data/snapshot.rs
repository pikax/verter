//! Loading the pinned snapshot from prepared bytes, and reading records from it.
//!
//! Resolving a snapshot verifies its digests and pin and indexes its records
//! without parsing any feature record: a feature is parsed only when asked for,
//! and one malformed record answers unknown without disturbing the others.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::support::{
    verdict_of, BrowserTarget, BrowserVersion, RawStatement, SchemaMismatch, SupportStatement,
    SupportVerdict, UnknownReason,
};

const FORMAT: &str = "verter-compat-snapshot";
const FORMAT_VERSION: u32 = 1;
const SNAPSHOT_FORMAT_KEYWORD: &str = "snapshot-format";

/// The one snapshot the session admits by default. A dataset or recipe change
/// re-mints this identity, so no other data can answer silently.
const ADMITTED_SNAPSHOT_ID: &str =
    "sha256:f7298cd7911b59e014970646d4a1b76c7cb076cd947606884bb67fd9cda5ce3d";

/// The exact snapshot identity a caller accepts. There is no floating form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) struct SnapshotPin(String);

impl SnapshotPin {
    pub(in crate::compat) fn admitted() -> Self {
        Self(ADMITTED_SNAPSHOT_ID.to_owned())
    }

    pub(in crate::compat) fn exact(snapshot_id: &str) -> Self {
        Self(snapshot_id.to_owned())
    }
}

/// What the host prepared for this session. Absence is a claim the host must
/// make; not having been told is a different state from being told there is none.
#[derive(Debug, Clone)]
pub(in crate::compat) enum SnapshotInput {
    Bytes(Arc<[u8]>),
    ProvenAbsent,
    NotSupplied,
}

/// Why supplied bytes are not an admissible snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) enum SnapshotRejection {
    MissingHeader,
    UnsupportedFormat { format: String, version: u32 },
    HeaderMismatch { detail: String },
    PayloadDigestMismatch { declared: String, actual: String },
    IdentityMismatch { declared: String, derived: String },
    UnpinnedSnapshot { pinned: String, found: String },
    Layout { detail: String },
    BrowserRecord { family: String, detail: String },
}

/// One constituent dataset of a snapshot identity.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::compat) struct DatasetIdentity {
    pub(in crate::compat) name: String,
    pub(in crate::compat) package: String,
    pub(in crate::compat) release: String,
    pub(in crate::compat) artifact_url: String,
    pub(in crate::compat) integrity: String,
    pub(in crate::compat) license: String,
    pub(in crate::compat) license_digest: String,
    pub(in crate::compat) license_file: String,
    pub(in crate::compat) attribution: String,
    pub(in crate::compat) schema_digest: String,
}

/// The ordered dataset identities plus the recipe that derived them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) struct SnapshotIdentity {
    pub(in crate::compat) snapshot_id: String,
    pub(in crate::compat) format_version: u32,
    pub(in crate::compat) datasets: Vec<DatasetIdentity>,
    pub(in crate::compat) recipe_digest: String,
    pub(in crate::compat) payload_digest: String,
}

#[derive(Deserialize)]
struct HeaderFormat {
    format: String,
    format_version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    #[allow(dead_code)]
    format: String,
    format_version: u32,
    datasets: Vec<DatasetIdentity>,
    recipe_digest: String,
    payload_digest: String,
    snapshot_id: String,
    counts: HeaderCounts,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderCounts {
    browsers: usize,
    bcd_features: usize,
    #[allow(dead_code)]
    bcd_rejected_features: usize,
    #[allow(dead_code)]
    bcd_rejected_families: usize,
    web_features: usize,
    #[allow(dead_code)]
    web_features_rejected: usize,
}

/// A browser release's lifecycle state in the data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(in crate::compat) enum ReleaseStatus {
    Retired,
    Current,
    Exclusive,
    Beta,
    Nightly,
    Esr,
    Planned,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRelease {
    status: ReleaseStatus,
    release_date: Option<String>,
    engine: Option<String>,
    engine_version: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBrowser {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    accepts_flags: Option<bool>,
    releases: BTreeMap<String, RawRelease>,
}

/// One exact release the data knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) struct BrowserRelease {
    pub(in crate::compat) version: BrowserVersion,
    pub(in crate::compat) status: ReleaseStatus,
    pub(in crate::compat) release_date: Option<String>,
    pub(in crate::compat) engine: Option<String>,
    pub(in crate::compat) engine_version: Option<String>,
}

/// A browser family with its releases in ascending version order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) struct BrowserRecord {
    pub(in crate::compat) name: String,
    pub(in crate::compat) kind: String,
    pub(in crate::compat) accepts_flags: Option<bool>,
    pub(in crate::compat) releases: Vec<BrowserRelease>,
}

impl BrowserRecord {
    fn release(&self, version: &BrowserVersion) -> Option<&BrowserRelease> {
        self.releases
            .binary_search_by(|release| release.version.cmp(version))
            .ok()
            .map(|index| &self.releases[index])
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRejection {
    instance_path: String,
    keyword: String,
}

impl From<RawRejection> for SchemaMismatch {
    fn from(raw: RawRejection) -> Self {
        Self {
            instance_path: raw.instance_path,
            keyword: raw.keyword,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawFamily {
    Statements(Vec<RawStatement>),
    Rejected { rejected: RawRejection },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFeatureStatus {
    experimental: bool,
    standard_track: bool,
    deprecated: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBcdRecord {
    #[serde(default)]
    mdn_url: Option<String>,
    #[serde(default)]
    spec_url: Vec<String>,
    #[serde(default)]
    status: Option<RawFeatureStatus>,
    support: BTreeMap<String, RawFamily>,
    #[serde(default)]
    web_features: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawBcdLine {
    Rejected { rejected: RawRejection },
    Record(RawBcdRecord),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBaseline {
    baseline: serde_json::Value,
    #[serde(default)]
    baseline_low_date: Option<String>,
    #[serde(default)]
    baseline_high_date: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    support: BTreeMap<String, String>,
}

// Parsed and consumed within one call, never stored.
#[allow(clippy::large_enum_variant)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "lowercase")]
enum RawWebFeature {
    Feature {
        name: String,
        status: RawBaseline,
        #[serde(default)]
        group: Vec<String>,
        #[serde(default)]
        compat_features: Vec<String>,
        #[serde(default)]
        discouraged: Option<Discouraged>,
    },
    Moved {
        redirect_target: String,
    },
    Split {
        redirect_targets: Vec<String>,
    },
}

// Parsed and consumed within one call, never stored.
#[allow(clippy::large_enum_variant)]
#[derive(Deserialize)]
#[serde(untagged)]
enum RawWfLine {
    Rejected { rejected: RawRejection },
    Record(RawWebFeature),
}

/// A formal recommendation against a feature, as web-features states it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::compat) struct Discouraged {
    pub(in crate::compat) according_to: Vec<String>,
    pub(in crate::compat) reason: String,
    pub(in crate::compat) reason_html: String,
    #[serde(default)]
    pub(in crate::compat) alternatives: Vec<String>,
    pub(in crate::compat) removal_date: Option<String>,
}

/// Feature-level Baseline provenance. It is never a predicate over a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) enum BaselineStatus {
    Widely {
        low_date: Option<String>,
        high_date: Option<String>,
    },
    Newly {
        low_date: Option<String>,
    },
    Limited,
    /// The dataset's record for this id is missing or failed its schema.
    Unavailable,
}

/// Baseline provenance for one web-features id carried beside an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) struct BaselineEvidence {
    pub(in crate::compat) web_feature: String,
    pub(in crate::compat) status: BaselineStatus,
}

/// Facts the support record states about the feature itself, for any target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) struct FeatureFacts {
    pub(in crate::compat) mdn_url: Option<String>,
    pub(in crate::compat) spec_urls: Vec<String>,
    /// `None` where the dataset's schema does not require a status block.
    pub(in crate::compat) experimental: Option<bool>,
    pub(in crate::compat) standard_track: Option<bool>,
    pub(in crate::compat) deprecated: Option<bool>,
}

/// The answer for one feature at one target, with its evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) struct SupportAnswer {
    /// The dataset record asked about: its key and the browser family.
    pub(in crate::compat) feature: String,
    pub(in crate::compat) target: BrowserTarget,
    pub(in crate::compat) verdict: SupportVerdict,
    /// `None` when no record was read.
    pub(in crate::compat) facts: Option<FeatureFacts>,
    /// Every statement the record carries for this family, applicable or not.
    pub(in crate::compat) statements: Vec<SupportStatement>,
    /// Which data answered; `None` only when there was no admissible snapshot.
    pub(in crate::compat) snapshot: Option<Arc<SnapshotIdentity>>,
    pub(in crate::compat) baseline: Vec<BaselineEvidence>,
}

impl SupportAnswer {
    fn unknown(
        feature: &str,
        target: &BrowserTarget,
        reason: UnknownReason,
        snapshot: Option<Arc<SnapshotIdentity>>,
    ) -> Self {
        Self {
            feature: feature.to_owned(),
            target: target.clone(),
            verdict: SupportVerdict::Unknown(reason),
            facts: None,
            statements: Vec::new(),
            snapshot,
            baseline: Vec::new(),
        }
    }
}

/// Whether a web-features id names any support record. An empty mapping is a
/// state of its own so it can never read as an empty success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) enum WebFeatureMembers {
    NoCompatMapping,
    Members(Vec<(String, SupportAnswer)>),
}

/// A web-features id looked up against one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) enum WebFeatureAnswer {
    Absent,
    Rejected(SchemaMismatch),
    Feature {
        name: String,
        groups: Vec<String>,
        baseline: BaselineStatus,
        discouraged: Option<Discouraged>,
        members: WebFeatureMembers,
    },
    Moved {
        target: String,
    },
    Split {
        targets: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy)]
struct IndexedLine {
    id_start: u32,
    id_end: u32,
    line_end: u32,
}

#[derive(Debug, Default)]
struct SectionIndex(Vec<IndexedLine>);

impl SectionIndex {
    fn find<'a>(&self, bytes: &'a [u8], id: &str) -> Option<&'a [u8]> {
        let id = id.as_bytes();
        let span = |line: &IndexedLine| &bytes[line.id_start as usize..line.id_end as usize];
        let index = self.0.binary_search_by(|line| span(line).cmp(id)).ok()?;
        let line = self.0[index];
        Some(&bytes[line.id_end as usize + 1..line.line_end as usize])
    }
}

/// An admitted snapshot: verified bytes, their identity and the record index.
#[derive(Debug)]
pub(in crate::compat) struct CompatSnapshot {
    bytes: Arc<[u8]>,
    identity: Arc<SnapshotIdentity>,
    browsers: BTreeMap<String, BrowserRecord>,
    bcd: SectionIndex,
    web_features: SectionIndex,
}

/// The session's standing with respect to compatibility data.
#[derive(Debug)]
pub(in crate::compat) enum SnapshotState {
    Ready(Box<CompatSnapshot>),
    ProvenAbsent,
    NeedInputs,
    Rejected(SnapshotRejection),
}

/// No snapshot was supplied or proven absent; the host must prepare one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::compat) struct NeedSnapshotInput;

impl SnapshotState {
    pub(in crate::compat) fn resolve(input: SnapshotInput, pin: &SnapshotPin) -> Self {
        match input {
            SnapshotInput::NotSupplied => Self::NeedInputs,
            SnapshotInput::ProvenAbsent => Self::ProvenAbsent,
            SnapshotInput::Bytes(bytes) => match CompatSnapshot::load(bytes, pin) {
                Ok(snapshot) => Self::Ready(Box::new(snapshot)),
                Err(rejection) => Self::Rejected(rejection),
            },
        }
    }

    /// The answer for one BCD feature key at one target.
    pub(in crate::compat) fn support(
        &self,
        feature: &str,
        target: &BrowserTarget,
    ) -> Result<SupportAnswer, NeedSnapshotInput> {
        match self {
            Self::Ready(snapshot) => Ok(snapshot.support(feature, target)),
            Self::ProvenAbsent => Ok(SupportAnswer::unknown(
                feature,
                target,
                UnknownReason::SnapshotAbsent,
                None,
            )),
            Self::Rejected(_) => Ok(SupportAnswer::unknown(
                feature,
                target,
                UnknownReason::SnapshotRejected,
                None,
            )),
            Self::NeedInputs => Err(NeedSnapshotInput),
        }
    }
}

fn sha256_label(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut label = String::with_capacity(7 + digest.len() * 2);
    label.push_str("sha256:");
    for byte in digest {
        label.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        label.push(char::from(b"0123456789abcdef"[usize::from(byte & 0xf)]));
    }
    label
}

/// The identity law, shared with the generator: every constituent identity, the
/// recipe and the payload digest, in order.
fn derive_snapshot_id(header: &Header) -> String {
    let format_version = header.format_version.to_string();
    let format_tag = format!("{FORMAT}-id");
    let mut parts: Vec<&str> = vec![&format_tag, &format_version];
    for dataset in &header.datasets {
        parts.extend([
            dataset.name.as_str(),
            dataset.release.as_str(),
            dataset.integrity.as_str(),
            dataset.schema_digest.as_str(),
            dataset.license_digest.as_str(),
        ]);
    }
    parts.extend([
        header.recipe_digest.as_str(),
        header.payload_digest.as_str(),
    ]);
    sha256_label(parts.join("\0").as_bytes())
}

fn layout(detail: impl Into<String>) -> SnapshotRejection {
    SnapshotRejection::Layout {
        detail: detail.into(),
    }
}

fn parse_browser(family: &str, json: &[u8]) -> Result<BrowserRecord, SnapshotRejection> {
    let rejected = |detail: String| SnapshotRejection::BrowserRecord {
        family: family.to_owned(),
        detail,
    };
    let raw: RawBrowser =
        serde_json::from_slice(json).map_err(|error| rejected(error.to_string()))?;
    let mut releases = Vec::with_capacity(raw.releases.len());
    for (spelling, release) in raw.releases {
        let version = BrowserVersion::parse(&spelling)
            .ok_or_else(|| rejected(format!("release version {spelling:?}")))?;
        releases.push(BrowserRelease {
            version,
            status: release.status,
            release_date: release.release_date,
            engine: release.engine,
            engine_version: release.engine_version,
        });
    }
    releases.sort_by(|a, b| a.version.cmp(&b.version));
    if releases
        .windows(2)
        .any(|pair| pair[0].version == pair[1].version)
    {
        return Err(rejected("two spellings of one release".to_owned()));
    }
    Ok(BrowserRecord {
        name: raw.name,
        kind: raw.kind,
        accepts_flags: raw.accepts_flags,
        releases,
    })
}

impl CompatSnapshot {
    fn load(bytes: Arc<[u8]>, pin: &SnapshotPin) -> Result<Self, SnapshotRejection> {
        let header_end = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or(SnapshotRejection::MissingHeader)?;
        let header_bytes = &bytes[..header_end];
        let format: HeaderFormat =
            serde_json::from_slice(header_bytes).map_err(|_| SnapshotRejection::MissingHeader)?;
        if format.format != FORMAT || format.format_version != FORMAT_VERSION {
            return Err(SnapshotRejection::UnsupportedFormat {
                format: format.format,
                version: format.format_version,
            });
        }
        let header: Header = serde_json::from_slice(header_bytes).map_err(|error| {
            SnapshotRejection::HeaderMismatch {
                detail: error.to_string(),
            }
        })?;
        let payload_start = header_end + 1;
        let actual = sha256_label(&bytes[payload_start..]);
        if actual != header.payload_digest {
            return Err(SnapshotRejection::PayloadDigestMismatch {
                declared: header.payload_digest,
                actual,
            });
        }
        let derived = derive_snapshot_id(&header);
        if derived != header.snapshot_id {
            return Err(SnapshotRejection::IdentityMismatch {
                declared: header.snapshot_id,
                derived,
            });
        }
        if pin.0 != header.snapshot_id {
            return Err(SnapshotRejection::UnpinnedSnapshot {
                pinned: pin.0.clone(),
                found: header.snapshot_id,
            });
        }
        let mut browsers = BTreeMap::new();
        let mut bcd = SectionIndex::default();
        let mut web_features = SectionIndex::default();
        index_records(
            &bytes,
            payload_start,
            &mut browsers,
            &mut bcd,
            &mut web_features,
        )?;
        let counts = &header.counts;
        if browsers.len() != counts.browsers
            || bcd.0.len() != counts.bcd_features
            || web_features.0.len() != counts.web_features
        {
            return Err(layout("record counts differ from the header"));
        }
        let identity = Arc::new(SnapshotIdentity {
            snapshot_id: header.snapshot_id,
            format_version: header.format_version,
            datasets: header.datasets,
            recipe_digest: header.recipe_digest,
            payload_digest: header.payload_digest,
        });
        Ok(Self {
            bytes,
            identity,
            browsers,
            bcd,
            web_features,
        })
    }

    pub(in crate::compat) fn identity(&self) -> &Arc<SnapshotIdentity> {
        &self.identity
    }

    /// The browser release data target expansion computes over.
    pub(in crate::compat) fn browser(&self, family: &str) -> Option<&BrowserRecord> {
        self.browsers.get(family)
    }

    pub(in crate::compat) fn browsers(&self) -> impl Iterator<Item = (&str, &BrowserRecord)> {
        self.browsers
            .iter()
            .map(|(family, record)| (family.as_str(), record))
    }

    /// Evaluate one feature at one exact target. Baseline is attached afterwards
    /// and cannot reach the verdict.
    pub(in crate::compat) fn support(
        &self,
        feature: &str,
        target: &BrowserTarget,
    ) -> SupportAnswer {
        let unknown =
            |reason| SupportAnswer::unknown(feature, target, reason, Some(self.identity.clone()));
        let Some(browser) = self.browsers.get(target.family()) else {
            return unknown(UnknownReason::TargetFamilyAbsent);
        };
        let Some(release) = browser.release(target.version()) else {
            return unknown(UnknownReason::TargetVersionAbsent);
        };
        let Some(json) = self.bcd.find(&self.bytes, feature) else {
            return unknown(UnknownReason::FeatureAbsent);
        };
        let record = match serde_json::from_slice::<RawBcdLine>(json) {
            Ok(RawBcdLine::Record(record)) => record,
            Ok(RawBcdLine::Rejected { rejected }) => {
                return unknown(UnknownReason::SchemaMismatch(rejected.into()));
            }
            Err(error) => return unknown(format_mismatch(feature, &error)),
        };
        let baseline = record
            .web_features
            .iter()
            .map(|id| self.baseline_evidence(id))
            .collect();
        let facts = FeatureFacts {
            mdn_url: record.mdn_url,
            spec_urls: record.spec_url,
            experimental: record.status.as_ref().map(|status| status.experimental),
            standard_track: record.status.as_ref().map(|status| status.standard_track),
            deprecated: record.status.as_ref().map(|status| status.deprecated),
        };
        let mut support = record.support;
        let family = support.remove(target.family());
        let (verdict, statements) = evaluate_family(family, target.version(), release.status);
        SupportAnswer {
            feature: feature.to_owned(),
            target: target.clone(),
            verdict,
            facts: Some(facts),
            statements,
            snapshot: Some(self.identity.clone()),
            baseline,
        }
    }

    /// Every feature record read and evaluated once, with what it came to.
    #[cfg(test)]
    pub(super) fn audit_records(&self) -> RecordAudit {
        let mut audit = RecordAudit::default();
        let probe = BrowserVersion::parse("1").expect("a version");
        for line in &self.bcd.0 {
            let json = &self.bytes[line.id_end as usize + 1..line.line_end as usize];
            audit.features += 1;
            let record = match serde_json::from_slice::<RawBcdLine>(json) {
                Ok(RawBcdLine::Record(record)) => record,
                Ok(RawBcdLine::Rejected { .. }) => {
                    audit.rejected_features += 1;
                    continue;
                }
                Err(_) => {
                    audit.format_mismatches += 1;
                    continue;
                }
            };
            for (_, family) in record.support {
                match evaluate_family(Some(family), &probe, ReleaseStatus::Current).0 {
                    SupportVerdict::Unknown(UnknownReason::SchemaMismatch(mismatch))
                        if mismatch.keyword == SNAPSHOT_FORMAT_KEYWORD =>
                    {
                        audit.format_mismatches += 1;
                    }
                    SupportVerdict::Unknown(UnknownReason::SchemaMismatch(_)) => {
                        audit.rejected_families += 1;
                    }
                    _ => {}
                }
            }
        }
        for line in &self.web_features.0 {
            let json = &self.bytes[line.id_end as usize + 1..line.line_end as usize];
            match serde_json::from_slice::<RawWfLine>(json) {
                Ok(RawWfLine::Record(_)) => audit.web_features += 1,
                Ok(RawWfLine::Rejected { .. }) | Err(_) => audit.format_mismatches += 1,
            }
        }
        audit
    }

    fn baseline_evidence(&self, web_feature: &str) -> BaselineEvidence {
        let status = self
            .web_features
            .find(&self.bytes, web_feature)
            .and_then(|json| serde_json::from_slice::<RawWfLine>(json).ok())
            .and_then(|line| match line {
                RawWfLine::Record(RawWebFeature::Feature { status, .. }) => baseline_status(status),
                RawWfLine::Record(RawWebFeature::Moved { .. } | RawWebFeature::Split { .. })
                | RawWfLine::Rejected { .. } => None,
            })
            .unwrap_or(BaselineStatus::Unavailable);
        BaselineEvidence {
            web_feature: web_feature.to_owned(),
            status,
        }
    }

    /// A web-features id and what its named support records answer at the target.
    /// A key the dataset lacks answers unknown, whatever Baseline says.
    pub(in crate::compat) fn web_feature(
        &self,
        id: &str,
        target: &BrowserTarget,
    ) -> WebFeatureAnswer {
        let Some(json) = self.web_features.find(&self.bytes, id) else {
            return WebFeatureAnswer::Absent;
        };
        let line = match serde_json::from_slice::<RawWfLine>(json) {
            Ok(line) => line,
            Err(error) => {
                return WebFeatureAnswer::Rejected(SchemaMismatch {
                    instance_path: format!("/{id}: {error}"),
                    keyword: SNAPSHOT_FORMAT_KEYWORD.to_owned(),
                });
            }
        };
        match line {
            RawWfLine::Rejected { rejected } => WebFeatureAnswer::Rejected(rejected.into()),
            RawWfLine::Record(RawWebFeature::Moved { redirect_target }) => {
                WebFeatureAnswer::Moved {
                    target: redirect_target,
                }
            }
            RawWfLine::Record(RawWebFeature::Split { redirect_targets }) => {
                WebFeatureAnswer::Split {
                    targets: redirect_targets,
                }
            }
            RawWfLine::Record(RawWebFeature::Feature {
                name,
                status,
                group,
                compat_features,
                discouraged,
            }) => {
                let members = if compat_features.is_empty() {
                    WebFeatureMembers::NoCompatMapping
                } else {
                    WebFeatureMembers::Members(
                        compat_features
                            .into_iter()
                            .map(|key| {
                                let answer = self.support(&key, target);
                                (key, answer)
                            })
                            .collect(),
                    )
                };
                WebFeatureAnswer::Feature {
                    name,
                    groups: group,
                    baseline: baseline_status(status).unwrap_or(BaselineStatus::Unavailable),
                    discouraged,
                    members,
                }
            }
        }
    }
}

/// What one browser family's statements come to at one release.
fn evaluate_family(
    family: Option<RawFamily>,
    version: &BrowserVersion,
    status: ReleaseStatus,
) -> (SupportVerdict, Vec<SupportStatement>) {
    let raw = match family {
        None => {
            return (
                SupportVerdict::Unknown(UnknownReason::NoRecordForFamily),
                Vec::new(),
            );
        }
        Some(RawFamily::Rejected { rejected }) => {
            return (
                SupportVerdict::Unknown(UnknownReason::SchemaMismatch(rejected.into())),
                Vec::new(),
            );
        }
        Some(RawFamily::Statements(raw)) => raw,
    };
    let evaluated = raw
        .into_iter()
        .map(|statement| statement.evaluate(version, status))
        .collect::<Result<Vec<_>, _>>();
    match evaluated {
        Ok(statements) => (verdict_of(&statements), statements),
        Err(mismatch) => (
            SupportVerdict::Unknown(UnknownReason::SchemaMismatch(mismatch)),
            Vec::new(),
        ),
    }
}

/// Counts from reading every record of a snapshot once.
#[cfg(test)]
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct RecordAudit {
    pub(super) features: usize,
    pub(super) rejected_features: usize,
    pub(super) rejected_families: usize,
    pub(super) web_features: usize,
    pub(super) format_mismatches: usize,
}

fn format_mismatch(feature: &str, error: &serde_json::Error) -> UnknownReason {
    UnknownReason::SchemaMismatch(SchemaMismatch {
        instance_path: format!("/{feature}: {error}"),
        keyword: SNAPSHOT_FORMAT_KEYWORD.to_owned(),
    })
}

fn baseline_status(raw: RawBaseline) -> Option<BaselineStatus> {
    match raw.baseline {
        serde_json::Value::Bool(false) => Some(BaselineStatus::Limited),
        serde_json::Value::String(level) if level == "low" => Some(BaselineStatus::Newly {
            low_date: raw.baseline_low_date,
        }),
        serde_json::Value::String(level) if level == "high" => Some(BaselineStatus::Widely {
            low_date: raw.baseline_low_date,
            high_date: raw.baseline_high_date,
        }),
        _ => None,
    }
}

fn index_records(
    bytes: &[u8],
    payload_start: usize,
    browsers: &mut BTreeMap<String, BrowserRecord>,
    bcd: &mut SectionIndex,
    web_features: &mut SectionIndex,
) -> Result<(), SnapshotRejection> {
    const SECTIONS: [&[u8]; 3] = [b"browser", b"bcd", b"wf"];
    let offset = |value: usize| u32::try_from(value).map_err(|_| layout("snapshot exceeds 4 GiB"));
    let mut position = payload_start;
    let mut section_rank = 0;
    let mut previous_id: &[u8] = &[];
    while position < bytes.len() {
        let rest = &bytes[position..];
        let line_len = rest
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or_else(|| layout("the last record is not terminated"))?;
        let line = &rest[..line_len];
        let tab = |from: usize| {
            line[from..]
                .iter()
                .position(|byte| *byte == b'\t')
                .map(|at| from + at)
                .ok_or_else(|| layout("a record lacks its section or id"))
        };
        let section_end = tab(0)?;
        let id_end = tab(section_end + 1)?;
        let section = &line[..section_end];
        let id = &line[section_end + 1..id_end];
        let rank = SECTIONS
            .iter()
            .position(|known| *known == section)
            .ok_or_else(|| layout("an unknown record section"))?;
        let ordered = match rank.cmp(&section_rank) {
            Ordering::Greater => true,
            Ordering::Equal => previous_id < id,
            Ordering::Less => false,
        };
        if !ordered || id.is_empty() {
            return Err(layout("records are out of order or duplicated"));
        }
        let json = &line[id_end + 1..];
        match rank {
            0 => {
                let family =
                    std::str::from_utf8(id).map_err(|_| layout("a non-UTF-8 record id"))?;
                browsers.insert(family.to_owned(), parse_browser(family, json)?);
            }
            _ => {
                let index = if rank == 1 {
                    &mut *bcd
                } else {
                    &mut *web_features
                };
                index.0.push(IndexedLine {
                    id_start: offset(position + section_end + 1)?,
                    id_end: offset(position + id_end)?,
                    line_end: offset(position + line_len)?,
                });
            }
        }
        section_rank = rank;
        previous_id = id;
        position += line_len + 1;
    }
    Ok(())
}
