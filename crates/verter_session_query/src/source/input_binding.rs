//! A request's committed input basis and the publication fence bound to it.
use verter_analysis_inputs::InputBasis;
use verter_analysis_inputs::SnapshotFence;
use verter_analysis_inputs::TornSnapshot;

use std::sync::Arc;

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
