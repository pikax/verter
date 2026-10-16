//! Aggregate retention-account wiring for [`super::SemanticGraphStore`].

use std::sync::Arc;

use verter_session_query::retention::shared::{CacheHold, CallerHold};
use verter_session_query::retention::{
    RetentionAdmission, RetentionCharge, RetentionRefusal, SemanticRetentionAccount, StoreAccount,
};

use super::{family::MemoEntry, SemanticGraphStore};
use crate::semantic_query::allocation_footprint::OwnedHeap as _;
use crate::semantic_query::{QueryResult, SemanticQueryValue};

/// Allocations a delivery aliases from the candidate that retains them: its
/// walker diagnostics, its dependency rail and its value.
pub(super) const DELIVERED_ALLOCATIONS: usize = 3;

/// One memo candidate's charges.
///
/// * `own` — the candidate's cache-only bookkeeping
///   ([`MemoEntry::own_retained_footprint_bytes`]), and every evidence page
///   its carrier newly retains. Nothing here is handed to a caller, so it
///   is released the moment the candidate drains.
/// * `carrier` — the fact rail and self-root list, shared by every
///   candidate the same build published (the root, its prefix backfills,
///   an SCC's members): one registered charge each, so a sibling never
///   charges them again and the last surviving sibling still pays for
///   them.
/// * `delivered` — the walker diagnostics, dependency rail and value a
///   read hands its caller: one registered charge each, whose caller holds
///   ride every delivery of the candidate, so a caller that outlives the
///   cache keeps the allocation charged (pinned) instead of uncharged.
///
/// An empty shared slice is only its counters: those are charged with the
/// candidate's own bookkeeping instead of a registration that would cost
/// more than the allocation.
#[derive(Debug)]
pub(super) struct MemoRetention {
    _own: RetentionCharge,
    _carrier: Box<[CacheHold]>,
    delivered: Box<[(usize, CacheHold)]>,
}

impl MemoRetention {
    /// Caller holds for every delivered allocation.
    pub(super) fn callers(&self) -> Box<[CallerHold]> {
        self.delivered
            .iter()
            .map(|(_, hold)| hold.caller())
            .collect()
    }

    /// Caller holds for the delivered allocations among `allocations` — a
    /// cold producer's own delivery aliases only the ones it shares with
    /// the candidate.
    pub(super) fn callers_for(&self, allocations: &[usize]) -> Box<[CallerHold]> {
        self.delivered
            .iter()
            .filter(|(allocation, _)| allocations.contains(allocation))
            .map(|(_, hold)| hold.caller())
            .collect()
    }

    /// The candidate's own reservation.
    #[cfg(test)]
    pub(super) fn own_bytes(&self) -> usize {
        self._own.bytes()
    }

    /// The candidate's shared holds, carrier first.
    #[cfg(test)]
    pub(super) fn shared_holds(&self) -> impl Iterator<Item = &CacheHold> {
        self._carrier
            .iter()
            .chain(self.delivered.iter().map(|(_, hold)| hold))
    }
}

/// The address an `Arc` names: the key of its registered charge.
pub(super) fn allocation_key<T: ?Sized>(allocation: &Arc<T>) -> usize {
    Arc::as_ptr(allocation).cast::<()>() as usize
}

/// The value allocation a result keeps reachable outside the graph, and its
/// key, when it has one.
pub(super) fn value_allocation_key(result: &QueryResult<SemanticQueryValue>) -> Option<usize> {
    let QueryResult::Value(value) = result else {
        return None;
    };
    match value {
        SemanticQueryValue::ObjectProjection(formula) => {
            Some(allocation_key(formula.alternatives_arc()))
        }
        SemanticQueryValue::DeclarationAnalysis(value) => Some(allocation_key(&value.contributors)),
        SemanticQueryValue::OverloadSet(signatures) => Some(allocation_key(signatures)),
        SemanticQueryValue::Relation(payload) => Some(allocation_key(&payload.bindings)),
        SemanticQueryValue::BroadRuntime(classification) => {
            Some(allocation_key(classification.kinds_arc()))
        }
        SemanticQueryValue::FlowReturn(result) => Some(allocation_key(result)),
        SemanticQueryValue::ResolveCall(result) => Some(allocation_key(result)),
        SemanticQueryValue::SignatureSet(value) => Some(allocation_key(&value.nodes)),
        SemanticQueryValue::TypeNode(_)
        | SemanticQueryValue::ProgramAnalysis(_)
        | SemanticQueryValue::MaterializationCycleGate(_)
        | SemanticQueryValue::TruthinessDomain(_)
        | SemanticQueryValue::SignatureResult(_)
        | SemanticQueryValue::DiagnosticAnalysis(_) => None,
    }
}

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
        Self::with_provenance_and_ownership(provenance, retention_account, Default::default())
    }

    /// [`Self::with_provenance`] in `ownership`'s reclamation regime.
    #[must_use]
    pub(crate) fn with_provenance_and_ownership(
        provenance: Arc<crate::engine_provenance::EngineProvenance>,
        retention_account: StoreAccount,
        ownership: super::GraphOwnership,
    ) -> Self {
        let mut store = Self::with_account_and_ownership(retention_account, ownership);
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
        Self::with_account_and_ownership(retention_account, Default::default())
    }

    /// [`Self::with_account`] in `ownership`'s reclamation regime.
    #[must_use]
    pub(crate) fn with_account_and_ownership(
        retention_account: StoreAccount,
        ownership: super::GraphOwnership,
    ) -> Self {
        Self {
            retention_account,
            ..Self::with_ownership(ownership)
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

    /// Reserve `entry`'s charges against the store's aggregate account, for
    /// a publication that may populate `populated_slots` slots.
    ///
    /// `Err` means the process declined to retain this candidate: the
    /// caller must SKIP the publish and return its complete value to the
    /// caller uncached. There is no uncharged arm — every store owns an
    /// account — and a refusal releases whatever part was already granted.
    ///
    /// The candidate's receipt is reserved first: its closure lives as long
    /// as any result it costs is retained, so each receipt is charged once
    /// against the same account, for as long as it lives
    /// ([`DemandCostReceipt::reserve_retention`]).
    ///
    /// The candidate's own reservation also claims every evidence page its
    /// carrier holds that no earlier admission claimed, so a wide candidate
    /// is refused for the whole footprint it would newly retain
    /// ([`reserve_retained_with_evidence`]). Its carrier and delivered
    /// allocations are held through the store's registry, so an allocation
    /// a sibling candidate already charges is shared, not charged again.
    ///
    /// [`DemandCostReceipt::reserve_retention`]: crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt::reserve_retention
    /// [`reserve_retained_with_evidence`]: verter_session_query::facts::receipt::reserve_retained_with_evidence
    pub(super) fn reserve_memo_candidate(
        &self,
        entry: &MemoEntry,
        populated_slots: usize,
    ) -> Result<Arc<MemoRetention>, RetentionRefusal> {
        let reserved = match entry
            .cost_receipt
            .reserve_retention(self.retention_account())
        {
            Ok(()) => self.reserve_candidate_charges(entry, populated_slots),
            Err(refusal) => Err(refusal),
        };
        reserved.map(Arc::new).inspect_err(|refusal| {
            crate::cache_runtime::admission::propagate_non_admission(
                refusal.non_admission_reason(),
            );
        })
    }

    /// The charges of one candidate after its receipt is reserved. The
    /// shared allocations are held first, so a refused own reservation
    /// leaves every page pinned exactly as it found it.
    fn reserve_candidate_charges(
        &self,
        entry: &MemoEntry,
        populated_slots: usize,
    ) -> Result<MemoRetention, RetentionRefusal> {
        let shared = self.hold_shared_allocations(SharedParts::of(entry))?;
        let own_bytes = own_reservation_bytes(entry, populated_slots);
        match verter_session_query::facts::receipt::reserve_retained_with_evidence(
            self.retention_account(),
            own_bytes,
            &[&entry.read_set_signature.facts],
        ) {
            RetentionAdmission::Admitted(own) => Ok(shared.with_own(own)),
            RetentionAdmission::Refused(refusal) => Err(refusal),
        }
    }

    /// Hold a retained result's carrier and delivered allocations.
    pub(super) fn hold_shared_allocations(
        &self,
        parts: SharedParts<'_>,
    ) -> Result<SharedHolds, RetentionRefusal> {
        use verter_session_query::retention::allocation::{
            arc_str_list_bytes, fact_slice_bytes, AliasSet,
        };
        let account = self.retention_account();
        let mut carrier = Vec::with_capacity(2);
        if !parts.facts.is_empty() {
            carrier.push(self.alias_charges.hold(account, parts.facts, || {
                fact_slice_bytes(parts.facts, &mut AliasSet::new())
            })?);
        }
        if !parts.self_roots.is_empty() {
            carrier.push(self.alias_charges.hold(account, parts.self_roots, || {
                arc_str_list_bytes(parts.self_roots, &mut AliasSet::new())
            })?);
        }
        let mut delivered = Vec::with_capacity(DELIVERED_ALLOCATIONS);
        if !parts.diagnostics.is_empty() {
            delivered.push((
                allocation_key(parts.diagnostics),
                self.alias_charges.hold(account, parts.diagnostics, || {
                    parts.diagnostics.owned_heap_bytes(&mut AliasSet::new())
                })?,
            ));
        }
        if !parts.dep_signature.is_empty() {
            delivered.push((
                allocation_key(parts.dep_signature),
                self.alias_charges.hold(account, parts.dep_signature, || {
                    parts.dep_signature.owned_heap_bytes(&mut AliasSet::new())
                })?,
            ));
        }
        if let Some(value) = self.hold_value_allocation(parts.result)? {
            delivered.push(value);
        }
        Ok(SharedHolds {
            carrier: carrier.into_boxed_slice(),
            delivered: delivered.into_boxed_slice(),
        })
    }

    /// Hold the allocation a result's value keeps reachable outside the
    /// graph, keyed by the value's outermost shared allocation.
    fn hold_value_allocation(
        &self,
        result: &QueryResult<SemanticQueryValue>,
    ) -> Result<Option<(usize, CacheHold)>, RetentionRefusal> {
        use verter_session_query::retention::allocation::AliasSet;
        let QueryResult::Value(value) = result else {
            return Ok(None);
        };
        let account = self.retention_account();
        let bytes = || value.owned_heap_bytes(&mut AliasSet::new());
        let hold = match value {
            SemanticQueryValue::ObjectProjection(formula) => {
                self.alias_charges
                    .hold(account, formula.alternatives_arc(), bytes)?
            }
            SemanticQueryValue::DeclarationAnalysis(analysis) => {
                self.alias_charges
                    .hold(account, &analysis.contributors, bytes)?
            }
            SemanticQueryValue::OverloadSet(signatures) => {
                self.alias_charges.hold(account, signatures, bytes)?
            }
            SemanticQueryValue::Relation(payload) => {
                self.alias_charges.hold(account, &payload.bindings, bytes)?
            }
            SemanticQueryValue::BroadRuntime(classification) => {
                self.alias_charges
                    .hold(account, classification.kinds_arc(), bytes)?
            }
            SemanticQueryValue::FlowReturn(flow) => {
                self.alias_charges.hold(account, flow, bytes)?
            }
            SemanticQueryValue::ResolveCall(call) => {
                self.alias_charges.hold(account, call, bytes)?
            }
            SemanticQueryValue::SignatureSet(set) => {
                self.alias_charges.hold(account, &set.nodes, bytes)?
            }
            SemanticQueryValue::TypeNode(_)
            | SemanticQueryValue::ProgramAnalysis(_)
            | SemanticQueryValue::MaterializationCycleGate(_)
            | SemanticQueryValue::TruthinessDomain(_)
            | SemanticQueryValue::SignatureResult(_)
            | SemanticQueryValue::DiagnosticAnalysis(_) => return Ok(None),
        };
        Ok(value_allocation_key(result).map(|key| (key, hold)))
    }

    /// The registry of shared allocation charges this store's candidates
    /// hold. Live registrations: one per allocation a candidate or a caller
    /// still charges.
    #[cfg(any(test, feature = "test-support"))]
    pub fn shared_allocation_registrations_for_tests(&self) -> usize {
        self.alias_charges.len()
    }
}

/// The shared allocations one retained result keeps reachable: its fact
/// carrier and self-root list, and the diagnostics, dependency rail and
/// value its reads deliver.
pub(super) struct SharedParts<'a> {
    pub(super) facts: &'a Arc<[verter_session_query::facts::fact_cache::FactVersionRef]>,
    pub(super) self_roots: &'a Arc<[Arc<str>]>,
    pub(super) diagnostics: &'a Arc<[crate::project_semantic_dispatch::walk::ShallowDiagnostic]>,
    pub(super) dep_signature: &'a crate::semantic_query::DepSignature,
    pub(super) result: &'a QueryResult<SemanticQueryValue>,
}

impl<'a> SharedParts<'a> {
    /// A memo candidate's shared allocations.
    pub(super) fn of(entry: &'a MemoEntry) -> Self {
        Self {
            facts: &entry.read_set_signature.facts,
            self_roots: &entry.self_root_canonicals,
            diagnostics: &entry.walker_diagnostics,
            dep_signature: &entry.dispatch_dep_signature,
            result: &entry.result,
        }
    }

    /// The counters of the empty shared slices, charged with the owner's
    /// own bookkeeping (see [`MemoRetention`]).
    pub(super) fn empty_slice_bytes(&self) -> usize {
        use verter_session_query::retention::allocation::arc_block_bytes;
        [
            self.facts.is_empty(),
            self.self_roots.is_empty(),
            self.diagnostics.is_empty(),
            self.dep_signature.is_empty(),
        ]
        .into_iter()
        .filter(|empty| *empty)
        .count()
            * arc_block_bytes(0)
    }
}

/// A candidate's shared holds, before its own reservation is granted.
pub(super) struct SharedHolds {
    carrier: Box<[CacheHold]>,
    delivered: Box<[(usize, CacheHold)]>,
}

impl SharedHolds {
    /// The candidate's charges, once its own reservation is granted.
    pub(super) fn with_own(self, own: RetentionCharge) -> MemoRetention {
        MemoRetention {
            _own: own,
            _carrier: self.carrier,
            delivered: self.delivered,
        }
    }
}

/// What `entry`'s own reservation covers: its bookkeeping for
/// `populated_slots` slots and the counters of its empty shared slices.
pub(super) fn own_reservation_bytes(entry: &MemoEntry, populated_slots: usize) -> usize {
    entry.own_retained_footprint_bytes(populated_slots) + SharedParts::of(entry).empty_slice_bytes()
}
