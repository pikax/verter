//! Aggregate retention-account wiring for [`super::SemanticGraphStore`].

use std::sync::Arc;

use crate::semantic_retention_account::{
    ChargeClass, RetainedFootprint, RetentionAdmission, RetentionCharge, RetentionRefusal,
    SemanticRetentionAccount,
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
    /// through this (or [`Self::with_provenance`]), so no live memo can
    /// grow without consuming aggregate headroom.
    #[must_use]
    pub fn with_account(retention_account: Arc<SemanticRetentionAccount>) -> Self {
        Self {
            retention_account: Some(retention_account),
            ..Default::default()
        }
    }

    /// The aggregate retained-byte account this store charges, when it
    /// has one.
    pub(super) fn retention_account(&self) -> Option<&Arc<SemanticRetentionAccount>> {
        self.retention_account.as_ref()
    }

    /// Reserve `entry`'s estimated bytes against the store's aggregate
    /// account.
    ///
    /// `Ok(None)` means the store owns no account (a fixture store) and
    /// the publish proceeds uncharged. `Err` means the process declined
    /// to retain this candidate: the caller must SKIP the publish and
    /// return its complete value to the caller uncached.
    pub(super) fn reserve_memo_candidate(
        &self,
        entry: &MemoEntry,
    ) -> Result<Option<RetentionCharge>, RetentionRefusal> {
        let Some(account) = self.retention_account.as_ref() else {
            return Ok(None);
        };
        match account.reserve(ChargeClass::Retained, entry.retained_footprint_bytes()) {
            RetentionAdmission::Admitted(charge) => Ok(Some(charge)),
            RetentionAdmission::Refused(refusal) => {
                crate::cache_runtime::admission::propagate_non_admission(
                    refusal.non_admission_reason(),
                );
                Err(refusal)
            }
        }
    }
}
