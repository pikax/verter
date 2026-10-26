//! The aggregate semantic-retention account wiring for the session's
//! resolution state. The account itself and its charge primitives live in
//! [`verter_retention`]; this module re-exports them under their established
//! path and adds the fact-aware pieces.

pub use verter_retention::*;

pub mod allocation;
pub mod heap_size_foreign;
pub(crate) mod resident;
pub mod resolution_charge;

use std::sync::Arc;

/// The aggregate account as the workspace's resident resolution state sees
/// it: each overlay-lane answer and each interned overlay value version is
/// a [`ChargeClass::Retained`] charge, refused under pressure like any
/// other warm cache entry (the workspace then serves the answer uncached).
///
/// The bound account is private and the only constructors are
/// [`Self::process_local`] and [`Default`], so this adapter always charges
/// the one process-local account. That closes only
/// THIS adapter's construction: the resolution-account trait it implements
/// is an open provider interface, and any crate may install its own
/// implementation of it. The single-account policy for resolution state is
/// therefore upheld by the host installing this adapter, not by the trait.
pub struct ResolutionRetention(Arc<SemanticRetentionAccount>);

impl ResolutionRetention {
    /// Bind resolution publications to an aggregate retention account.
    #[must_use]
    pub fn new(account: Arc<SemanticRetentionAccount>) -> Self {
        Self(account)
    }
    /// The adapter over the process-local account.
    #[must_use]
    pub fn process_local() -> Self {
        Self(SemanticRetentionAccount::process_local())
    }
}

impl Default for ResolutionRetention {
    fn default() -> Self {
        Self::process_local()
    }
}

impl crate::retention::resolution_charge::ResolutionRetentionAccount for ResolutionRetention {
    fn reserve_retained_with_evidence(
        &self,
        bytes: usize,
        facts: crate::facts::fact_set::FactsRef<'_>,
    ) -> Option<crate::retention::resolution_charge::ResolutionRetentionCharge> {
        crate::facts::receipt::reserve_retained_with_evidence(&self.0, bytes, &[&facts])
            .admitted()
            .map(crate::retention::resolution_charge::ResolutionRetentionCharge::new)
    }
    fn reserve_retained(
        &self,
        bytes: usize,
    ) -> Option<crate::retention::resolution_charge::ResolutionRetentionCharge> {
        self.0
            .reserve(ChargeClass::Retained, bytes)
            .admitted()
            .map(crate::retention::resolution_charge::ResolutionRetentionCharge::new)
    }
}
