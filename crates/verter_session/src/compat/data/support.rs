//! Support facts and their evaluation for one exact target.
//!
//! Each statement's interval is decided first and its conditions second, so a
//! flag on a statement that does not apply never colours the answer, and a
//! flag on one that does is never dropped. An indeterminate boundary cannot
//! prove support at a particular version; it stays uncertain rather than
//! resolving either way.

use std::cmp::Ordering;

use serde::Deserialize;

use super::snapshot::ReleaseStatus;

/// One browser version, compared numerically segment by segment.
#[derive(Debug, Clone)]
pub(in crate::compat) struct BrowserVersion {
    spelling: Box<str>,
    segments: Box<[u32]>,
}

impl BrowserVersion {
    /// Parse a dotted-numeric spelling; anything else is not a version.
    pub(in crate::compat) fn parse(spelling: &str) -> Option<Self> {
        let segments = spelling
            .split('.')
            .map(|segment| {
                let digits = !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit());
                digits.then(|| segment.parse::<u32>().ok()).flatten()
            })
            .collect::<Option<Box<[u32]>>>()?;
        Some(Self {
            spelling: spelling.into(),
            segments,
        })
    }

    pub(in crate::compat) fn as_str(&self) -> &str {
        &self.spelling
    }
}

impl Ord for BrowserVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let width = self.segments.len().max(other.segments.len());
        let segment = |version: &Self, index: usize| version.segments.get(index).copied();
        (0..width)
            .map(|index| {
                segment(self, index)
                    .unwrap_or(0)
                    .cmp(&segment(other, index).unwrap_or(0))
            })
            .find(|ordering| ordering.is_ne())
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for BrowserVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for BrowserVersion {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for BrowserVersion {}

/// The spelling offered as a target was not one exact version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) struct InvalidTargetSpelling {
    pub(in crate::compat) spelling: String,
}

/// One exact browser release. A range, a floating tag or a bound never
/// constructs one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) struct BrowserTarget {
    family: String,
    version: BrowserVersion,
}

impl BrowserTarget {
    pub(in crate::compat) fn exact(
        family: &str,
        version: &str,
    ) -> Result<Self, InvalidTargetSpelling> {
        let invalid = || InvalidTargetSpelling {
            spelling: format!("{family} {version}"),
        };
        if family.is_empty() {
            return Err(invalid());
        }
        Ok(Self {
            family: family.to_owned(),
            version: BrowserVersion::parse(version).ok_or_else(invalid)?,
        })
    }

    pub(in crate::compat) fn family(&self) -> &str {
        &self.family
    }

    pub(in crate::compat) fn version(&self) -> &BrowserVersion {
        &self.version
    }
}

/// `version_added` as a distinct fact, never flattened to a boolean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) enum VersionAdded {
    Exact(BrowserVersion),
    AtOrBefore(BrowserVersion),
    Preview,
    /// `true`: supported since a version the data does not name.
    UnknownVersion,
    /// `false`: an explicit negative record.
    NotSupported,
    /// `null`: the data says support itself is unknown.
    UnknownSupport,
}

/// `version_removed`; removal is an exclusive upper bound on support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) enum VersionRemoved {
    NotRemoved,
    Exact(BrowserVersion),
    AtOrBefore(BrowserVersion),
    Preview,
    /// `true`: removed in a version the data does not name.
    UnknownVersion,
}

/// A runtime flag exactly as the data states it. The data does not say whether
/// a flag is on by default, so no such field exists here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::compat) struct FlagCondition {
    #[serde(rename = "type")]
    pub(in crate::compat) flag_type: String,
    pub(in crate::compat) name: String,
    pub(in crate::compat) value_to_set: Option<String>,
}

/// A condition a statement attaches to its support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) enum SupportCondition {
    Partial { notes: Vec<String> },
    Flagged(FlagCondition),
    Prefixed(String),
    Aliased(String),
}

/// The kind of a condition, in the order answers report them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::compat) enum ConditionKind {
    Partial,
    Flagged,
    Prefixed,
    Aliased,
}

impl SupportCondition {
    fn kind(&self) -> ConditionKind {
        match self {
            Self::Partial { .. } => ConditionKind::Partial,
            Self::Flagged(_) => ConditionKind::Flagged,
            Self::Prefixed(_) => ConditionKind::Prefixed,
            Self::Aliased(_) => ConditionKind::Aliased,
        }
    }
}

/// Why a statement's interval could not be decided at the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::compat) enum IndeterminateBoundary {
    AdditionVersionUnknown,
    AdditionAtOrBefore,
    AdditionInPreview,
    SupportUnknown,
    RemovalVersionUnknown,
    RemovalAtOrBefore,
    RemovalInPreview,
}

/// Where the target falls against one statement's interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::compat) enum IntervalStatus {
    InInterval,
    BeforeAddition,
    AtOrAfterRemoval,
    ExplicitNegative,
    Indeterminate(IndeterminateBoundary),
}

/// One retained support statement and how it evaluated at the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) struct SupportStatement {
    pub(in crate::compat) added: VersionAdded,
    pub(in crate::compat) removed: VersionRemoved,
    /// The notes this statement carries, whether or not it is partial.
    pub(in crate::compat) notes: Vec<String>,
    pub(in crate::compat) conditions: Vec<SupportCondition>,
    pub(in crate::compat) interval: IntervalStatus,
}

/// Lower bound carried by unconditional support, when the data knows one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) enum SinceBound {
    Exact(BrowserVersion),
    AtOrBefore(BrowserVersion),
}

/// The upstream schema failure, or the snapshot record failing its own format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) struct SchemaMismatch {
    pub(in crate::compat) instance_path: String,
    pub(in crate::compat) keyword: String,
}

/// Why no decidable record answers this feature at this target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) enum UnknownReason {
    SnapshotAbsent,
    SnapshotRejected,
    FeatureAbsent,
    TargetFamilyAbsent,
    TargetVersionAbsent,
    NoRecordForFamily,
    SchemaMismatch(SchemaMismatch),
    Indeterminate,
}

/// The per-target answer. `Conditional` never implies general support: the exact
/// flag, prefix, name or limits are on the statements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::compat) enum SupportVerdict {
    Supported { since: SinceBound },
    Conditional { kinds: Vec<ConditionKind> },
    Unsupported,
    Unknown(UnknownReason),
}

/// One support statement as stored in the snapshot.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawStatement {
    version_added: serde_json::Value,
    #[serde(default)]
    version_removed: Option<serde_json::Value>,
    // Derived upstream from `version_removed`; validated for shape, never consulted.
    #[serde(default)]
    #[allow(dead_code)]
    version_last: Option<serde_json::Value>,
    #[serde(default)]
    prefix: Option<String>,
    #[serde(default)]
    alternative_name: Option<String>,
    #[serde(default)]
    flags: Vec<FlagCondition>,
    #[serde(default)]
    partial_implementation: Option<bool>,
    #[serde(default)]
    notes: Vec<String>,
}

enum Bound {
    Exact(BrowserVersion),
    AtOrBefore(BrowserVersion),
    Preview,
}

fn parse_bound(spelling: &str) -> Option<Bound> {
    if spelling == "preview" {
        return Some(Bound::Preview);
    }
    match spelling.strip_prefix('≤') {
        Some(rest) => BrowserVersion::parse(rest).map(Bound::AtOrBefore),
        None => BrowserVersion::parse(spelling).map(Bound::Exact),
    }
}

fn parse_added(value: &serde_json::Value) -> Option<VersionAdded> {
    use serde_json::Value;
    Some(match value {
        Value::Bool(true) => VersionAdded::UnknownVersion,
        Value::Bool(false) => VersionAdded::NotSupported,
        Value::Null => VersionAdded::UnknownSupport,
        Value::String(spelling) => match parse_bound(spelling)? {
            Bound::Exact(version) => VersionAdded::Exact(version),
            Bound::AtOrBefore(version) => VersionAdded::AtOrBefore(version),
            Bound::Preview => VersionAdded::Preview,
        },
        _ => return None,
    })
}

fn parse_removed(value: Option<&serde_json::Value>) -> Option<VersionRemoved> {
    use serde_json::Value;
    Some(match value {
        None => VersionRemoved::NotRemoved,
        Some(Value::Bool(true)) => VersionRemoved::UnknownVersion,
        Some(Value::String(spelling)) => match parse_bound(spelling)? {
            Bound::Exact(version) => VersionRemoved::Exact(version),
            Bound::AtOrBefore(version) => VersionRemoved::AtOrBefore(version),
            Bound::Preview => VersionRemoved::Preview,
        },
        Some(_) => return None,
    })
}

fn is_prerelease(status: ReleaseStatus) -> bool {
    matches!(
        status,
        ReleaseStatus::Beta | ReleaseStatus::Nightly | ReleaseStatus::Planned
    )
}

/// A definite bound decides; an indeterminate one only applies if no definite
/// bound already places the target outside the interval.
fn evaluate_interval(
    added: &VersionAdded,
    removed: &VersionRemoved,
    target: &BrowserVersion,
    status: ReleaseStatus,
) -> IntervalStatus {
    use IndeterminateBoundary as Boundary;
    if *added == VersionAdded::NotSupported {
        return IntervalStatus::ExplicitNegative;
    }
    let removal = match removed {
        VersionRemoved::NotRemoved => None,
        VersionRemoved::Exact(version) | VersionRemoved::AtOrBefore(version)
            if target >= version =>
        {
            return IntervalStatus::AtOrAfterRemoval;
        }
        VersionRemoved::Exact(_) => None,
        VersionRemoved::AtOrBefore(_) => Some(Boundary::RemovalAtOrBefore),
        VersionRemoved::Preview => is_prerelease(status).then_some(Boundary::RemovalInPreview),
        VersionRemoved::UnknownVersion => Some(Boundary::RemovalVersionUnknown),
    };
    let addition = match added {
        VersionAdded::Exact(version) if target < version => {
            return IntervalStatus::BeforeAddition;
        }
        VersionAdded::Exact(_) => None,
        VersionAdded::AtOrBefore(version) => {
            (target < version).then_some(Boundary::AdditionAtOrBefore)
        }
        VersionAdded::Preview if is_prerelease(status) => Some(Boundary::AdditionInPreview),
        VersionAdded::Preview => return IntervalStatus::BeforeAddition,
        VersionAdded::UnknownVersion => Some(Boundary::AdditionVersionUnknown),
        VersionAdded::UnknownSupport => Some(Boundary::SupportUnknown),
        VersionAdded::NotSupported => unreachable!("an explicit negative returned above"),
    };
    match addition.or(removal) {
        Some(boundary) => IntervalStatus::Indeterminate(boundary),
        None => IntervalStatus::InInterval,
    }
}

impl RawStatement {
    /// Evaluate against the target; a value outside the format is a mismatch.
    pub(super) fn evaluate(
        self,
        target: &BrowserVersion,
        status: ReleaseStatus,
    ) -> Result<SupportStatement, SchemaMismatch> {
        let mismatch = |field: &str| SchemaMismatch {
            instance_path: field.to_owned(),
            keyword: "snapshot-format".to_owned(),
        };
        let added = parse_added(&self.version_added).ok_or_else(|| mismatch("version_added"))?;
        let removed = parse_removed(self.version_removed.as_ref())
            .ok_or_else(|| mismatch("version_removed"))?;
        let mut conditions = Vec::new();
        match self.partial_implementation {
            None | Some(false) => {}
            Some(true) => conditions.push(SupportCondition::Partial {
                notes: self.notes.clone(),
            }),
        }
        conditions.extend(self.flags.into_iter().map(SupportCondition::Flagged));
        conditions.extend(self.prefix.map(SupportCondition::Prefixed));
        conditions.extend(self.alternative_name.map(SupportCondition::Aliased));
        let interval = evaluate_interval(&added, &removed, target, status);
        Ok(SupportStatement {
            added,
            removed,
            notes: self.notes,
            conditions,
            interval,
        })
    }
}

/// Aggregate evaluated statements. Unconditional support needs one unconditional
/// statement in interval; a retained uncertain statement never upgrades or
/// erases the others.
pub(super) fn verdict_of(statements: &[SupportStatement]) -> SupportVerdict {
    let applicable = || {
        statements
            .iter()
            .filter(|statement| statement.interval == IntervalStatus::InInterval)
    };
    for statement in applicable() {
        if statement.conditions.is_empty() {
            match &statement.added {
                VersionAdded::Exact(version) => {
                    return SupportVerdict::Supported {
                        since: SinceBound::Exact(version.clone()),
                    };
                }
                VersionAdded::AtOrBefore(version) => {
                    return SupportVerdict::Supported {
                        since: SinceBound::AtOrBefore(version.clone()),
                    };
                }
                VersionAdded::Preview
                | VersionAdded::UnknownVersion
                | VersionAdded::NotSupported
                | VersionAdded::UnknownSupport => {}
            }
        }
    }
    let mut kinds: Vec<ConditionKind> = applicable()
        .flat_map(|statement| statement.conditions.iter().map(SupportCondition::kind))
        .collect();
    if !kinds.is_empty() {
        kinds.sort_unstable();
        kinds.dedup();
        return SupportVerdict::Conditional { kinds };
    }
    if statements.is_empty() {
        return SupportVerdict::Unknown(UnknownReason::NoRecordForFamily);
    }
    let uncertain = statements
        .iter()
        .any(|statement| matches!(statement.interval, IntervalStatus::Indeterminate(_)));
    if uncertain {
        SupportVerdict::Unknown(UnknownReason::Indeterminate)
    } else {
        SupportVerdict::Unsupported
    }
}
