//! Aggregate retention-account wiring for [`super::SemanticGraphStore`].

use std::sync::Arc;

use verter_session_query::retention::shared::{CacheHold, CallerHold};
use verter_session_query::retention::{
    RetentionAdmission, RetentionCharge, RetentionRefusal, SemanticRetentionAccount, StoreAccount,
};

use super::{family::MemoEntry, SemanticGraphStore};
use crate::semantic_query::allocation_footprint::OwnedHeap as _;
use crate::semantic_query::{QueryResult, SemanticQueryValue};

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
///   read hands its caller: one registered charge each. Every delivery
///   holds the same charges as a caller from the moment it is minted
///   ([`super::delivery_lease`]), so a caller that outlives the cache keeps
///   the allocation charged (pinned), and a delivery the cache refuses was
///   charged all along.
#[derive(Debug)]
pub(super) struct MemoRetention {
    _own: RetentionCharge,
    _carrier: Box<[CacheHold]>,
    _delivered: Box<[CacheHold]>,
}

impl MemoRetention {
    /// The candidate's own reservation.
    #[cfg(test)]
    pub(super) fn own_bytes(&self) -> usize {
        self._own.bytes()
    }

    /// The candidate's shared holds, carrier first.
    #[cfg(test)]
    pub(super) fn shared_holds(&self) -> impl Iterator<Item = &CacheHold> {
        self._carrier.iter().chain(self._delivered.iter())
    }
}

/// The bytes one memo family keeps beside its candidates: its key and slot
/// lists in the family map (with the map's control byte), its record in the
/// retention budget's ledger, and the heap each of those two key copies
/// keeps reachable. The map's key counts the whole key heap; the ledger
/// holds a clone, which shares every `Arc` the map's key already counted
/// and owns fresh copies of its boxes — exactly what measuring the key
/// again under the same aliases counts. The candidates themselves are
/// charged with each candidate.
pub(super) fn family_container_bytes(family: &super::family::FamilyKey) -> usize {
    use super::family_footprint::family_key_heap_bytes;
    let mut aliases = verter_session_query::retention::allocation::AliasSet::new();
    let map_key = family_key_heap_bytes(family, &mut aliases);
    let ledger_key = family_key_heap_bytes(family, &mut aliases);
    std::mem::size_of::<(super::family::FamilyKey, super::family::FamilySlots)>()
        + 1
        + std::mem::size_of::<(u64, super::family::FamilyKey)>()
        + map_key
        + ledger_key
}

/// Receives the one allocation a result's value keeps reachable outside the
/// graph — its outermost shared allocation, which owns the value's heap.
pub(super) trait ValueAllocation {
    type Output;
    fn allocation<T: ?Sized + Send + Sync + 'static>(self, allocation: &Arc<T>) -> Self::Output;
}

/// Offer `result`'s value allocation to `visit`; `None` for a value that
/// keeps nothing reachable outside the graph.
pub(super) fn with_value_allocation<V: ValueAllocation>(
    result: &QueryResult<SemanticQueryValue>,
    visit: V,
) -> Option<V::Output> {
    let QueryResult::Value(value) = result else {
        return None;
    };
    Some(match value {
        SemanticQueryValue::ObjectProjection(formula) => {
            visit.allocation(formula.alternatives_arc())
        }
        SemanticQueryValue::DeclarationAnalysis(analysis) => {
            visit.allocation(&analysis.contributors)
        }
        SemanticQueryValue::OverloadSet(signatures) => visit.allocation(signatures),
        SemanticQueryValue::Relation(payload) => visit.allocation(&payload.bindings),
        SemanticQueryValue::BroadRuntime(classification) => {
            visit.allocation(classification.kinds_arc())
        }
        SemanticQueryValue::FlowReturn(flow) => visit.allocation(flow),
        SemanticQueryValue::ResolveCall(call) => visit.allocation(call),
        SemanticQueryValue::SignatureSet(set) => visit.allocation(&set.nodes),
        SemanticQueryValue::TypeNode(_)
        | SemanticQueryValue::ProgramAnalysis(_)
        | SemanticQueryValue::MaterializationCycleGate(_)
        | SemanticQueryValue::TruthinessDomain(_)
        | SemanticQueryValue::SignatureResult(_)
        | SemanticQueryValue::DiagnosticAnalysis(_) => return None,
    })
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
        Self::with_ownership_and_account(ownership, retention_account)
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
        family: &super::family::FamilyKey,
        populated_slots: usize,
        registered_canonicals: usize,
    ) -> Result<Arc<MemoRetention>, RetentionRefusal> {
        let reserved = match entry
            .cost_receipt
            .reserve_retention(self.retention_account())
        {
            Ok(()) => self.reserve_candidate_charges(
                entry,
                family,
                populated_slots,
                registered_canonicals,
            ),
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
        family: &super::family::FamilyKey,
        populated_slots: usize,
        registered_canonicals: usize,
    ) -> Result<MemoRetention, RetentionRefusal> {
        let shared = self.hold_shared_allocations(SharedParts::of(entry))?;
        let own_bytes =
            own_reservation_bytes(entry, family, populated_slots, registered_canonicals);
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
        use verter_session_query::retention::allocation::{arc_str_list_bytes, fact_slice_bytes};
        let account = self.retention_account();
        let mut carrier = vec![
            self.alias_charges.hold(account, parts.facts, |aliases| {
                fact_slice_bytes(parts.facts, aliases)
            })?,
            self.alias_charges
                .hold(account, parts.self_roots, |aliases| {
                    arc_str_list_bytes(parts.self_roots, aliases)
                })?,
        ];
        // The delivery the retained result's warm reads share: its metadata
        // is retained with the result.
        carrier.extend(parts.lease.adopt_metadata()?);
        let mut delivered = Vec::with_capacity(3);
        if !super::is_static_empty(parts.diagnostics) {
            delivered.push(
                self.alias_charges
                    .hold(account, parts.diagnostics, |aliases| {
                        parts.diagnostics.owned_heap_bytes(aliases)
                    })?,
            );
        }
        if !super::is_static_empty(parts.dep_signature) {
            delivered.push(
                self.alias_charges
                    .hold(account, parts.dep_signature, |aliases| {
                        dep_rail_heap_bytes(parts.dep_signature, aliases)
                    })?,
            );
        }
        if let Some(value) = self.hold_value_allocation(parts.result) {
            delivered.push(value?);
        }
        Ok(SharedHolds {
            carrier: carrier.into_boxed_slice(),
            delivered: delivered.into_boxed_slice(),
        })
    }

    /// A cache hold on the allocation a result's value keeps reachable
    /// outside the graph.
    fn hold_value_allocation(
        &self,
        result: &QueryResult<SemanticQueryValue>,
    ) -> Option<Result<CacheHold, RetentionRefusal>> {
        struct Hold<'a>(&'a SemanticGraphStore, &'a SemanticQueryValue);
        impl ValueAllocation for Hold<'_> {
            type Output = Result<CacheHold, RetentionRefusal>;
            fn allocation<T: ?Sized + Send + Sync + 'static>(
                self,
                allocation: &Arc<T>,
            ) -> Self::Output {
                let Self(store, value) = self;
                store
                    .alias_charges
                    .hold(store.retention_account(), allocation, |aliases| {
                        value.owned_heap_bytes(aliases)
                    })
            }
        }
        let QueryResult::Value(value) = result else {
            return None;
        };
        with_value_allocation(result, Hold(self, value))
    }

    /// A delivery's holds on the allocations it hands its caller, taken as
    /// the caller's: pinned, never refused, and shared with any holder of
    /// the same allocation.
    pub(super) fn hold_delivered_allocations(
        &self,
        delivered: &super::delivery_lease::DeliveredAllocations<'_>,
    ) -> Box<[CallerHold]> {
        struct Pin<'a>(&'a SemanticGraphStore, &'a SemanticQueryValue);
        impl ValueAllocation for Pin<'_> {
            type Output = CallerHold;
            fn allocation<T: ?Sized + Send + Sync + 'static>(
                self,
                allocation: &Arc<T>,
            ) -> Self::Output {
                let Self(store, value) = self;
                store
                    .alias_charges
                    .pin_caller(store.retention_account(), allocation, |aliases| {
                        value.owned_heap_bytes(aliases)
                    })
            }
        }
        let account = self.retention_account();
        let mut holds = Vec::with_capacity(3);
        if let Some(diagnostics) = delivered
            .diagnostics
            .filter(|diagnostics| !super::is_static_empty(diagnostics))
        {
            holds.push(
                self.alias_charges
                    .pin_caller(account, diagnostics, |aliases| {
                        diagnostics.owned_heap_bytes(aliases)
                    }),
            );
        }
        if let Some(dep_signature) = delivered
            .dep_signature
            .filter(|dep_signature| !super::is_static_empty(dep_signature))
        {
            holds.push(
                self.alias_charges
                    .pin_caller(account, dep_signature, |aliases| {
                        dep_rail_heap_bytes(dep_signature, aliases)
                    }),
            );
        }
        if let QueryResult::Value(value) = delivered.result {
            holds.extend(with_value_allocation(delivered.result, Pin(self, value)));
        }
        holds.into_boxed_slice()
    }

    /// The slots of `family`, keyed under the held `entries` lock: an
    /// existing family's, or a newly keyed family's empty slots holding its
    /// container charge. `None` when the account refuses to key a new
    /// family: the publish is skipped and its complete value returned
    /// uncached. The `bool` is whether the family was newly keyed.
    pub(super) fn family_slots_locked<'a>(
        &self,
        entries: &'a mut rustc_hash::FxHashMap<
            super::family::FamilyKey,
            super::family::FamilySlots,
        >,
        family: &super::family::FamilyKey,
    ) -> Option<(&'a mut super::family::FamilySlots, bool)> {
        self.family_slots_prepaid_locked(entries, family, None)
    }

    /// [`Self::family_slots_locked`] with a container charge `prepaid`
    /// before the lock — taken by a batch that must not be refused halfway.
    /// An already keyed family releases the unused prepayment.
    pub(super) fn family_slots_prepaid_locked<'a>(
        &self,
        entries: &'a mut rustc_hash::FxHashMap<
            super::family::FamilyKey,
            super::family::FamilySlots,
        >,
        family: &super::family::FamilyKey,
        prepaid: Option<RetentionCharge>,
    ) -> Option<(&'a mut super::family::FamilySlots, bool)> {
        use std::collections::hash_map::Entry;
        match entries.entry(family.clone()) {
            Entry::Occupied(slots) => Some((slots.into_mut(), false)),
            Entry::Vacant(slot) if prepaid.is_some() => Some((
                slot.insert(super::family::FamilySlots::charged(
                    prepaid.expect("matched a prepayment"),
                )),
                true,
            )),
            Entry::Vacant(slot) => match self.retention_account().reserve(
                verter_session_query::retention::ChargeClass::Retained,
                family_container_bytes(family),
            ) {
                RetentionAdmission::Admitted(charge) => Some((
                    slot.insert(super::family::FamilySlots::charged(charge)),
                    true,
                )),
                RetentionAdmission::Refused(refusal) => {
                    crate::cache_runtime::admission::propagate_non_admission(
                        refusal.non_admission_reason(),
                    );
                    None
                }
            },
        }
    }

    /// Bytes the store's graph storage — its arena and its current
    /// signature epoch — is pinned for against the retention account. A
    /// required current count: a retired epoch a reader still holds is
    /// charged until it drops but is not counted here.
    pub fn graph_storage_charge_bytes(&self) -> usize {
        self.arena.core.storage_charge_bytes() + self.signatures.current_epoch_storage_charge()
    }

    /// Bytes the store's arena storage is pinned for.
    #[cfg(any(test, feature = "test-support"))]
    pub fn graph_storage_charge_for_tests(&self) -> usize {
        self.arena.core.storage_charge_bytes()
    }

    /// Bytes the store's memo bookkeeping is pinned for: its reverse
    /// index's shards, its shared-charge registry's table and its
    /// dependency-rail interner. A required current count.
    pub fn bookkeeping_storage_charge_bytes(&self) -> usize {
        self.canonical_to_entries.storage_charge_bytes()
            + self.alias_charges.table_charged_bytes()
            + self.dep_signature_interner.storage_charge_bytes()
    }

    /// Bytes the store's own storage is pinned for: its graph storage and
    /// its memo bookkeeping. Everything else a store's account pins is a
    /// caller's: the allocations live deliveries hold and their metadata.
    #[cfg(any(test, feature = "test-support"))]
    pub fn store_storage_pins_for_tests(&self) -> usize {
        self.graph_storage_charge_bytes() + self.bookkeeping_storage_charge_bytes()
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
    pub(super) lease: &'a super::delivery_lease::SemanticDeliveryLease,
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
            lease: &entry.lease,
            facts: &entry.read_set_signature.facts,
            self_roots: &entry.self_root_canonicals,
            diagnostics: &entry.walker_diagnostics,
            dep_signature: &entry.dispatch_dep_signature,
            result: &entry.result,
        }
    }
}

/// A candidate's shared holds, before its own reservation is granted.
pub(super) struct SharedHolds {
    carrier: Box<[CacheHold]>,
    delivered: Box<[CacheHold]>,
}

impl SharedHolds {
    /// The candidate's charges, once its own reservation is granted.
    pub(super) fn with_own(self, own: RetentionCharge) -> MemoRetention {
        MemoRetention {
            _own: own,
            _carrier: self.carrier,
            _delivered: self.delivered,
        }
    }
}

/// What `entry`'s own reservation covers: its bookkeeping for
/// `populated_slots` slots registered under `registered_canonicals`
/// canonicals each.
pub(super) fn own_reservation_bytes(
    entry: &MemoEntry,
    family: &super::family::FamilyKey,
    populated_slots: usize,
    registered_canonicals: usize,
) -> usize {
    entry.own_retained_footprint_bytes(family, populated_slots, registered_canonicals)
}

/// The heap a holder of a dependency rail pays for: the whole rail, or —
/// for an interned rail, whose block its interner keeps allocated and
/// charges — only the names its elements hold.
fn dep_rail_heap_bytes(
    rail: &crate::semantic_query::DepSignature,
    aliases: &mut verter_session_query::retention::allocation::AliasSet,
) -> usize {
    if super::interner::is_interned_rail(rail) {
        super::interner::interned_rail_heap_bytes(rail, aliases)
    } else {
        rail.owned_heap_bytes(aliases)
    }
}
