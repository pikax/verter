//! Session bind of one committed [`InputBasis`] behind a [`SnapshotFence`].
//!
//! The producer (this crate's host) reads the workspace, then
//! [`RequestInputBinding::from_basis`] is the only request-level bind.
//! Consumers observe the bound basis; they do not re-read disk.

use std::sync::Arc;

pub use verter_analysis_inputs::{
    CommitError, DirectoryEntry, InputBasis, LoadWave, NegativeFact, NegativeKind, Observation,
    ObservationKind, ObserveError, RetryError, RetryOutcome, SnapshotFence, TornSnapshot,
};

/// One request's sole committed input and the fence that admits publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestInputBinding {
    basis: Arc<InputBasis>,
    fence: SnapshotFence,
}

impl RequestInputBinding {
    /// Bind `basis` and fence publication to its identity.
    #[must_use]
    pub fn from_basis(basis: InputBasis) -> Self {
        let fence = SnapshotFence::bind(&basis);
        Self {
            basis: Arc::new(basis),
            fence,
        }
    }

    /// Committed basis.
    #[must_use]
    pub fn basis(&self) -> &InputBasis {
        &self.basis
    }

    /// Fence bound at [`Self::from_basis`].
    #[must_use]
    pub fn fence(&self) -> &SnapshotFence {
        &self.fence
    }

    /// Admit publication of `basis` only when it is this binding's identity.
    pub fn admit_publication(&self, basis: &InputBasis) -> Result<(), TornSnapshot> {
        self.fence.admit(basis)
    }
}

/// Commit the single `canonical` currently visible on `workspace`.
///
/// Missing files become an explicit [`NegativeFact`]. This is the producer
/// read; after commit, consumers must [`InputBasis::observe`].
#[must_use]
pub fn commit_workspace_canonical(
    workspace: &dyn verter_workspace::WorkspaceRead,
    canonical: &str,
) -> InputBasis {
    let wave = LoadWave::from_keys([canonical]);
    match workspace.read_file(canonical) {
        Some(content) => InputBasis::commit(wave, [Observation::file(canonical, content)], [])
            .expect("single-file commit cannot mix or overlap"),
        None => InputBasis::commit(wave, [], [NegativeFact::absent(canonical)])
            .expect("single-file negative commit cannot mix or overlap"),
    }
}

/// Extend `previous` by probing only unrecorded `discovered` keys.
///
/// Already-recorded keys are not re-read. Empty discovery is
/// [`RetryOutcome::Terminal`]. Consumers still observe through the returned
/// basis; they do not read the workspace.
pub fn retry_workspace_wave<I, S>(
    workspace: &dyn verter_workspace::WorkspaceRead,
    previous: &InputBasis,
    discovered: I,
) -> Result<RetryOutcome, RetryError>
where
    I: IntoIterator<Item = S>,
    S: Into<Arc<str>>,
{
    let wave = previous.discovery_wave(discovered);
    if wave.is_empty() {
        return Ok(RetryOutcome::Terminal);
    }
    let mut observations = Vec::new();
    let mut negatives = Vec::new();
    for key in wave.keys() {
        match workspace.read_file(key.as_ref()) {
            Some(content) => observations.push(Observation::file(Arc::clone(key), content)),
            None => negatives.push(NegativeFact::absent(Arc::clone(key))),
        }
    }
    previous.retry(wave, observations, negatives)
}
