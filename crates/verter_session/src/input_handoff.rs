//! Asynchronous input acquisition to committed-snapshot handoff.
//!
//! A browser host acquires file bytes asynchronously (OPFS, the File
//! System Access API, or a worker message) OUTSIDE any semantic
//! callback, then hands the acquired rows here in one synchronous call.
//! This seam commits ONE immutable [`InputBasis`] — positives plus
//! explicit negatives — through the canonical F1 constructor and binds
//! it behind a [`RequestInputBinding`] fence. Every later synchronous
//! semantic callback observes committed rows only.
//!
//! Two rules make the boundary honest:
//!
//! - **No acquisition capability lives here.** The handoff type holds
//!   no workspace handle, no loader, and no callback: a requested key
//!   that was never acquired surfaces as typed [`HandoffObserve::NeedInputs`]
//!   naming the missing canonical, never as a synchronous fetch attempt
//!   inside a resolver. The caller runs the NEXT asynchronous
//!   acquisition wave and commits the successor basis through the
//!   existing retry machinery.
//! - **Missing means probed-and-absent.** A key the acquisition wave
//!   probed and did not find is committed as a [`NegativeFact`] and
//!   observes as [`HandoffObserve::Absent`] — a complete-negative
//!   answer, distinct from `NeedInputs` the same way partial, pending
//!   and failed are distinct from complete-empty.

use std::sync::Arc;

use crate::input_basis::{InputBasis, LoadWave, NegativeFact, Observation, RequestInputBinding};

/// One asynchronously acquired file row handed to
/// [`CommittedInputHandoff::commit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquiredFile {
    /// Canonical identity of the acquired file.
    pub canonical: Arc<str>,
    /// File bytes captured by the asynchronous acquisition wave.
    pub content: Arc<str>,
}

/// One immutable committed handoff: the basis, its publication fence,
/// and nothing else.
#[derive(Debug, Clone)]
pub struct CommittedInputHandoff {
    binding: RequestInputBinding,
}

/// Typed observation outcome over a committed handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffObserve {
    /// Committed file bytes.
    File {
        /// Canonical identity.
        canonical: Arc<str>,
        /// Captured content.
        content: Arc<str>,
    },
    /// The acquisition wave probed this key and recorded it absent.
    Absent {
        /// Canonical identity of the probed-missing input.
        canonical: Arc<str>,
    },
    /// The key was never acquired: the caller must run the next
    /// asynchronous acquisition wave. Not an error, not an absence,
    /// and never resolved by a read from here.
    NeedInputs {
        /// Canonical identity of the unacquired input.
        canonical: Arc<str>,
    },
}

impl HandoffObserve {
    /// The canonical this outcome answers for.
    #[must_use]
    pub fn canonical(&self) -> &str {
        match self {
            Self::File { canonical, .. }
            | Self::Absent { canonical }
            | Self::NeedInputs { canonical } => canonical,
        }
    }
}

impl CommittedInputHandoff {
    /// Commit one handoff from an acquisition wave's file rows and
    /// probed-missing keys.
    ///
    /// Duplicate identical rows collapse; a canonical both acquired and
    /// probed-missing, or acquired with two different contents, is
    /// refused closed by the canonical constructor.
    pub fn commit(
        files: impl IntoIterator<Item = AcquiredFile>,
        missing: impl IntoIterator<Item = Arc<str>>,
    ) -> Result<Self, crate::input_basis::CommitError> {
        let files: Vec<AcquiredFile> = files.into_iter().collect();
        let missing: Vec<Arc<str>> = missing.into_iter().collect();
        // The wave is the union of the rows themselves, so
        // `InputBasis::commit` sees one coherent wave covering every
        // positive and negative it is handed.
        let keys: Vec<Arc<str>> = files
            .iter()
            .map(|file| Arc::clone(&file.canonical))
            .chain(missing.iter().map(Arc::clone))
            .collect();
        let observations = files
            .into_iter()
            .map(|file| Observation::file(file.canonical, file.content));
        let negatives = missing.into_iter().map(NegativeFact::absent);
        let basis = InputBasis::commit(LoadWave::from_keys(keys), observations, negatives)?;
        Ok(Self {
            binding: RequestInputBinding::from_basis(basis),
        })
    }

    /// The bound request binding: the sole committed input and the
    /// fence that admits publication of this basis.
    #[must_use]
    pub fn binding(&self) -> &RequestInputBinding {
        &self.binding
    }

    /// The committed basis.
    #[must_use]
    pub fn basis(&self) -> &InputBasis {
        self.binding.basis()
    }

    /// Observe one key through the committed basis. Pure lookup: this
    /// type has no acquisition path, so an unrecorded key can only be
    /// the typed [`HandoffObserve::NeedInputs`] demand for the next
    /// asynchronous wave.
    #[must_use]
    pub fn observe(&self, canonical: &str) -> HandoffObserve {
        match self.binding.basis().observe(canonical) {
            Ok(observation) => match observation.kind() {
                crate::input_basis::ObservationKind::File { content } => HandoffObserve::File {
                    canonical: Arc::from(canonical),
                    content: Arc::clone(content),
                },
                crate::input_basis::ObservationKind::Directory { .. } => {
                    // File waves are the handoff's vocabulary; a
                    // directory row cannot arrive through
                    // `AcquiredFile`, so reaching here means a
                    // non-handoff basis was projected through this
                    // seam. Fail loudly rather than silently equating a
                    // directory listing with a missing input.
                    panic!("committed handoff projected a directory observation as a file wave")
                }
            },
            Err(crate::input_basis::ObserveError::Negative(_)) => HandoffObserve::Absent {
                canonical: Arc::from(canonical),
            },
            Err(crate::input_basis::ObserveError::Unrecorded) => HandoffObserve::NeedInputs {
                canonical: Arc::from(canonical),
            },
        }
    }

    /// Unrecorded keys among `keys`, sorted and deduplicated — the
    /// exact demand set for the next asynchronous acquisition wave.
    /// Already-committed positives and negatives are not re-demanded.
    #[must_use]
    pub fn next_acquisition_wave(&self, keys: impl IntoIterator<Item = Arc<str>>) -> LoadWave {
        self.binding.basis().discovery_wave(keys)
    }
}

#[cfg(test)]
#[path = "input_handoff_tests.rs"]
mod input_handoff_tests;
