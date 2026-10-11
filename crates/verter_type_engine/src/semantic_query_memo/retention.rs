//! Aggregate retention-account wiring for [`super::SemanticGraphStore`].

use std::sync::Arc;

use verter_session_query::retention::{
    RetainedFootprint, RetentionAdmission, RetentionCharge, RetentionRefusal,
    SemanticRetentionAccount, StoreAccount,
};

use super::{family::MemoEntry, SemanticGraphStore};

impl SemanticGraphStore {
    /// Construct a store wired to the host's
    /// [`EngineProvenance`](crate::engine_provenance::EngineProvenance) so the
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
        provenance: Arc<crate::engine_provenance::EngineProvenance>,
        retention_account: StoreAccount,
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
    pub fn with_account(retention_account: StoreAccount) -> Self {
        Self {
            retention_account,
            ..Default::default()
        }
    }

    /// Bind this store's producers to `task_registry`: the one cycle
    /// authority the host's composition root mints and shares with every
    /// layer whose producers may wait on this engine's. A store built
    /// without one owns a private registry.
    #[must_use]
    pub fn with_task_registry(
        mut self,
        task_registry: verter_execution::tasks::TaskRegistry,
    ) -> Self {
        self.task_registry = task_registry;
        self
    }

    /// The aggregate retained-byte account this store charges. Always
    /// present — an account-less memo store does not exist.
    pub fn retention_account(&self) -> &Arc<SemanticRetentionAccount> {
        self.retention_account.get()
    }

    /// Reserve `entry`'s estimated bytes against the store's aggregate
    /// account.
    ///
    /// `Err` means the process declined to retain this candidate: the
    /// caller must SKIP the publish and return its complete value to the
    /// caller uncached. There is no uncharged arm — every store owns an
    /// account.
    ///
    /// The candidate's receipt is reserved first: its closure lives as long
    /// as any result it costs is retained, so each receipt is charged once
    /// against the same account, for as long as it lives
    /// ([`DemandCostReceipt::reserve_retention`]).
    ///
    /// The candidate's own reservation also claims every evidence page its
    /// carrier holds that no earlier admission claimed, so a wide candidate
    /// is refused for the whole footprint it would newly retain
    /// ([`reserve_retained_with_evidence`]).
    ///
    /// [`DemandCostReceipt::reserve_retention`]: crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt::reserve_retention
    /// [`reserve_retained_with_evidence`]: verter_session_query::facts::receipt::reserve_retained_with_evidence
    pub(super) fn reserve_memo_candidate(
        &self,
        entry: &MemoEntry,
    ) -> Result<RetentionCharge, RetentionRefusal> {
        let admission = match entry
            .cost_receipt
            .reserve_retention(self.retention_account())
        {
            Ok(()) => verter_session_query::facts::receipt::reserve_retained_with_evidence(
                self.retention_account(),
                entry.retained_footprint_bytes(),
                &[&entry.read_set_signature.facts],
            ),
            Err(refusal) => RetentionAdmission::Refused(refusal),
        };
        match admission {
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
