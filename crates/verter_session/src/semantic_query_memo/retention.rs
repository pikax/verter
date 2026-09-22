//! Aggregate retention-account wiring for [`super::SemanticGraphStore`].

use std::sync::Arc;

use crate::semantic_retention_account::{
    ChargeClass, RetainedFootprint, RetentionAdmission, RetentionCharge, RetentionRefusal,
    SemanticRetentionAccount, StoreAccount,
};

use super::{family::MemoEntry, SemanticGraphStore};

impl SemanticGraphStore {
    /// Construct a store wired to the host's
    /// [`MetaProvenance`](crate::types::MetaProvenance) so the
    /// underlying [`NodeArena`] and `execute_cooperative` path record
    /// contention-instrumentation counters. Test-only direct
    /// constructions use [`Self::new`] / [`Self::default`]
    /// (provenance stays `None`).
    ///
    /// The constructor installs provenance via field mutation on a
    /// `Default`-built store so it stays compatible with the dispatch
    /// invariant tests that require single-owner cardinality for
    /// `arena: NodeArena` in production code.
    #[must_use]
    pub fn with_provenance(
        provenance: Arc<crate::types::MetaProvenance>,
        retention_account: Arc<SemanticRetentionAccount>,
    ) -> Self {
        let mut store = Self::with_account(retention_account);
        store.arena.provenance = Some(Arc::clone(&provenance));
        store.provenance = Some(provenance);
        store
    }

    /// Construct a store charging `retention_account` for every memo
    /// candidate it publishes. The host builds every production store
    /// through this (or [`Self::with_provenance`]); a store built without
    /// one charges the process-local account, so no live memo can grow
    /// without consuming aggregate headroom.
    #[must_use]
    pub fn with_account(retention_account: Arc<SemanticRetentionAccount>) -> Self {
        Self {
            retention_account: StoreAccount::new(retention_account),
            ..Default::default()
        }
    }

    /// The aggregate retained-byte account this store charges. Always
    /// present — an account-less memo store does not exist.
    pub(crate) fn retention_account(&self) -> &Arc<SemanticRetentionAccount> {
        self.retention_account.get()
    }

    /// Reserve `entry`'s estimated bytes against the store's aggregate
    /// account.
    ///
    /// `Err` means the process declined to retain this candidate: the
    /// caller must SKIP the publish and return its complete value to the
    /// caller uncached. There is no uncharged arm — every store owns an
    /// account.
    pub(super) fn reserve_memo_candidate(
        &self,
        entry: &MemoEntry,
    ) -> Result<RetentionCharge, RetentionRefusal> {
        match self
            .retention_account()
            .reserve(ChargeClass::Retained, entry.retained_footprint_bytes())
        {
            RetentionAdmission::Admitted(charge) => Ok(charge),
            RetentionAdmission::Refused(refusal) => {
                crate::cache_runtime::admission::propagate_non_admission(
                    refusal.non_admission_reason(),
                );
                Err(refusal)
            }
        }
    }
}
