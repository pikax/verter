//! One committed [`InputBasis`] and a [`SnapshotFence`].
//!
//! This crate never reads disk. A producer (the session host) hands already-read
//! observations and explicit [`NegativeFact`]s to [`InputBasis::commit`]. After
//! commit, consumers observe only those rows. An unrecorded key is
//! [`ObserveError::Unrecorded`], never a filesystem fallback.

use std::collections::BTreeMap;
use std::sync::Arc;

use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{ContentId, InputBasisId};

/// Deterministic, sorted, deduplicated set of load keys for one wave.
///
/// F1 owns the wave identity and ordering. Retry that extends a basis is a
/// successor (F2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LoadWave {
    keys: Vec<Arc<str>>,
}

impl LoadWave {
    /// Sort and deduplicate `keys`. Empty is a valid zero wave.
    #[must_use]
    pub fn from_keys<I, S>(keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Arc<str>>,
    {
        let mut keys: Vec<Arc<str>> = keys.into_iter().map(Into::into).collect();
        keys.sort();
        keys.dedup();
        Self { keys }
    }

    /// Ordered keys.
    #[must_use]
    pub fn keys(&self) -> &[Arc<str>] {
        &self.keys
    }
}

/// Why a probed input is absent from the committed basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NegativeKind {
    /// The producer probed the key and recorded that it does not exist.
    Absent,
}

/// Explicit missing-input fact. Never recovered by a later consumer-local read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NegativeFact {
    canonical: Arc<str>,
    kind: NegativeKind,
}

impl NegativeFact {
    /// Record that `canonical` was probed and is absent.
    #[must_use]
    pub fn absent(canonical: impl Into<Arc<str>>) -> Self {
        Self {
            canonical: canonical.into(),
            kind: NegativeKind::Absent,
        }
    }

    /// Canonical identity of the missing input.
    #[must_use]
    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    /// Absence class.
    #[must_use]
    pub fn kind(&self) -> NegativeKind {
        self.kind
    }
}

/// One directory child captured in a committed directory observation.
///
/// Derived `Ord` includes `is_dir`, so identical path + opposite classification
/// are distinct rows. [`Observation::directory`] refuses that pair.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DirectoryEntry {
    path: Arc<str>,
    is_dir: bool,
}

impl DirectoryEntry {
    /// Directory child at `path`.
    #[must_use]
    pub fn new(path: impl Into<Arc<str>>, is_dir: bool) -> Self {
        Self {
            path: path.into(),
            is_dir,
        }
    }

    /// Child path as captured.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Whether the child is a directory.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }
}

/// Kind of a positive observation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ObservationKind {
    /// File bytes captured at commit.
    File { content: Arc<str> },
    /// Directory listing captured at commit.
    Directory { entries: Vec<DirectoryEntry> },
}

/// One positive observation in a committed basis.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Observation {
    canonical: Arc<str>,
    revision: [u8; 32],
    kind: ObservationKind,
}

impl Observation {
    /// File observation. Revision is the content digest, not a live path.
    #[must_use]
    pub fn file(canonical: impl Into<Arc<str>>, content: impl Into<Arc<str>>) -> Self {
        let content = content.into();
        let revision = content_revision(content.as_bytes());
        Self {
            canonical: canonical.into(),
            revision,
            kind: ObservationKind::File { content },
        }
    }

    /// Directory observation. Revision is the digest of the sorted listing.
    ///
    /// Identical duplicates collapse. The same child path with opposite
    /// `is_dir` values is refused: `DirectoryEntry` ordering includes the
    /// flag, so sort+dedup would otherwise keep both rows.
    pub fn directory(
        canonical: impl Into<Arc<str>>,
        mut entries: Vec<DirectoryEntry>,
    ) -> Result<Self, CommitError> {
        let canonical = canonical.into();
        entries.sort();
        for pair in entries.windows(2) {
            if pair[0].path == pair[1].path && pair[0].is_dir != pair[1].is_dir {
                return Err(CommitError::ConflictingDirectoryEntry {
                    directory: Arc::clone(&canonical),
                    path: Arc::clone(&pair[0].path),
                });
            }
        }
        entries.dedup();
        let revision = directory_revision(&entries);
        Ok(Self {
            canonical,
            revision,
            kind: ObservationKind::Directory { entries },
        })
    }

    /// Canonical identity.
    #[must_use]
    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    /// Content or listing digest bound at commit.
    #[must_use]
    pub fn revision(&self) -> &[u8; 32] {
        &self.revision
    }

    /// Captured kind.
    #[must_use]
    pub fn kind(&self) -> &ObservationKind {
        &self.kind
    }

    /// File bytes when this observation is a file.
    #[must_use]
    pub fn file_content(&self) -> Option<&str> {
        match &self.kind {
            ObservationKind::File { content } => Some(content.as_ref()),
            ObservationKind::Directory { .. } => None,
        }
    }

    /// Directory listing when this observation is a directory.
    #[must_use]
    pub fn directory_entries(&self) -> Option<&[DirectoryEntry]> {
        match &self.kind {
            ObservationKind::Directory { entries } => Some(entries),
            ObservationKind::File { .. } => None,
        }
    }
}

/// Why [`InputBasis::commit`] refused a candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitError {
    /// The same canonical arrived with two different revisions or kinds.
    MixedRevision { canonical: Arc<str> },
    /// A canonical is both a positive observation and a negative fact.
    OverlappingPositiveAndNegative { canonical: Arc<str> },
    /// One directory listing classified the same child as both file and directory.
    ConflictingDirectoryEntry { directory: Arc<str>, path: Arc<str> },
    /// A load-wave key has no positive observation and no negative fact.
    IncompleteWave { canonical: Arc<str> },
    /// An observation or negative fact is not in the load wave.
    ExtraneousRow { canonical: Arc<str> },
}

/// Why [`InputBasis::observe`] cannot return a positive row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveError<'a> {
    /// The producer recorded this key as missing.
    Negative(&'a NegativeFact),
    /// The key was never committed. Not a license to read disk.
    Unrecorded,
}

/// One immutable committed input. Identity is [`InputBasisId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputBasis {
    id: InputBasisId,
    wave: LoadWave,
    observations: BTreeMap<Arc<str>, Observation>,
    negatives: BTreeMap<Arc<str>, NegativeFact>,
}

impl InputBasis {
    /// Commit one basis from a wave, positive observations, and negative facts.
    ///
    /// Duplicate identical observations collapse. Distinct revisions of one
    /// canonical, or a canonical that is both present and absent, fail closed.
    pub fn commit(
        wave: LoadWave,
        observations: impl IntoIterator<Item = Observation>,
        negatives: impl IntoIterator<Item = NegativeFact>,
    ) -> Result<Self, CommitError> {
        let mut observation_map = BTreeMap::<Arc<str>, Observation>::new();
        for observation in observations {
            let canonical = Arc::clone(&observation.canonical);
            if let Some(existing) = observation_map.get(&canonical) {
                if existing.revision != observation.revision || existing.kind != observation.kind {
                    return Err(CommitError::MixedRevision { canonical });
                }
                continue;
            }
            observation_map.insert(canonical, observation);
        }

        let mut negative_map = BTreeMap::<Arc<str>, NegativeFact>::new();
        for fact in negatives {
            let canonical = Arc::clone(&fact.canonical);
            if observation_map.contains_key(&canonical) {
                return Err(CommitError::OverlappingPositiveAndNegative { canonical });
            }
            negative_map.insert(canonical, fact);
        }

        for key in wave.keys() {
            if !observation_map.contains_key(key) && !negative_map.contains_key(key) {
                return Err(CommitError::IncompleteWave {
                    canonical: Arc::clone(key),
                });
            }
        }
        for canonical in observation_map.keys().chain(negative_map.keys()) {
            if wave.keys.binary_search(canonical).is_err() {
                return Err(CommitError::ExtraneousRow {
                    canonical: Arc::clone(canonical),
                });
            }
        }

        let id = InputBasisId::from_canonical(&InputBasisDescriptor {
            wave: &wave,
            observations: &observation_map,
            negatives: &negative_map,
        });
        Ok(Self {
            id,
            wave,
            observations: observation_map,
            negatives: negative_map,
        })
    }

    /// Committed identity.
    #[must_use]
    pub fn id(&self) -> &InputBasisId {
        &self.id
    }

    /// Wave that produced this basis.
    #[must_use]
    pub fn wave(&self) -> &LoadWave {
        &self.wave
    }

    /// Positive observations, ordered by canonical.
    pub fn observations(&self) -> impl Iterator<Item = &Observation> {
        self.observations.values()
    }

    /// Negative facts, ordered by canonical.
    pub fn negatives(&self) -> impl Iterator<Item = &NegativeFact> {
        self.negatives.values()
    }

    /// Observe a committed key. Never reads disk.
    pub fn observe(&self, canonical: &str) -> Result<&Observation, ObserveError<'_>> {
        if let Some(observation) = self.observations.get(canonical) {
            return Ok(observation);
        }
        if let Some(fact) = self.negatives.get(canonical) {
            return Err(ObserveError::Negative(fact));
        }
        Err(ObserveError::Unrecorded)
    }
}

/// Publication fence bound to one [`InputBasisId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotFence {
    basis_id: InputBasisId,
}

impl SnapshotFence {
    /// Bind a fence to `basis`. Later admission must present the same id.
    #[must_use]
    pub fn bind(basis: &InputBasis) -> Self {
        Self {
            basis_id: basis.id().clone(),
        }
    }

    /// Bound basis identity.
    #[must_use]
    pub fn basis_id(&self) -> &InputBasisId {
        &self.basis_id
    }

    /// Admit publication of `basis` only when it is the bound identity.
    pub fn admit(&self, basis: &InputBasis) -> Result<(), TornSnapshot> {
        if basis.id() == &self.basis_id {
            Ok(())
        } else {
            Err(TornSnapshot::BasisMismatch)
        }
    }
}

/// Torn snapshot: publication mixed a different committed basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TornSnapshot {
    /// Presented basis is not the fence's bound identity.
    BasisMismatch,
}

struct InputBasisDescriptor<'a> {
    wave: &'a LoadWave,
    observations: &'a BTreeMap<Arc<str>, Observation>,
    negatives: &'a BTreeMap<Arc<str>, NegativeFact>,
}

impl CanonicalEncode for InputBasisDescriptor<'_> {
    const DOMAIN_TAG: &'static str = "verter.analysis_inputs.input_basis.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_sorted_set(1, self.wave.keys.iter().map(|key| key.as_bytes()));
        let observation_entries = self.observations.iter().map(|(canonical, observation)| {
            let mut value = Vec::with_capacity(1 + 32);
            value.push(kind_tag(&observation.kind));
            value.extend_from_slice(&observation.revision);
            (canonical.as_bytes(), value)
        });
        let _ = encoder.field_sorted_map(2, observation_entries);
        encoder.field_sorted_set(
            3,
            self.negatives.values().map(|fact| {
                let mut bytes = Vec::with_capacity(1 + fact.canonical.len());
                bytes.push(match fact.kind {
                    NegativeKind::Absent => 0,
                });
                bytes.extend_from_slice(fact.canonical.as_bytes());
                bytes
            }),
        );
    }
}

fn kind_tag(kind: &ObservationKind) -> u8 {
    match kind {
        ObservationKind::File { .. } => 0,
        ObservationKind::Directory { .. } => 1,
    }
}

fn content_revision(bytes: &[u8]) -> [u8; 32] {
    *ContentId::from_content_bytes(bytes).digest().as_bytes()
}

fn directory_revision(entries: &[DirectoryEntry]) -> [u8; 32] {
    let mut payload = Vec::new();
    payload.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    for entry in entries {
        payload.extend_from_slice(&(entry.path.len() as u64).to_le_bytes());
        payload.extend_from_slice(entry.path.as_bytes());
        payload.push(u8::from(entry.is_dir));
    }
    content_revision(&payload)
}
