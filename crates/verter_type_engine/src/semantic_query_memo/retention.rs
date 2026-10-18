//! Aggregate retention-account wiring for [`super::SemanticGraphStore`].

use std::sync::Arc;

use verter_session_query::retention::owned::{ChargeCtx, HeldHandles, OwnedHandles};
use verter_session_query::retention::shared::CacheHold;
use verter_session_query::retention::{
    RetentionAdmission, RetentionCharge, RetentionRefusal, SemanticRetentionAccount, StoreAccount,
};

use super::{family::MemoEntry, SemanticGraphStore};
use crate::semantic_query::{QueryResult, SemanticQueryValue};

/// One memo candidate's charges.
///
/// * `own` — the candidate's cache-only bookkeeping
///   ([`MemoEntry::own_retained_footprint`]), and every evidence page
///   its carrier newly retains. Nothing here is handed to a caller, so it
///   is released the moment the candidate drains.
/// * `carrier` — the fact rail and self-root list, shared by every
///   candidate the same build published (the root, its prefix backfills,
///   an SCC's members): one registered charge each, so a sibling never
///   charges them again and the last surviving sibling still pays for
///   them.
/// * `delivered` — the cache's holds on what a read hands its caller: its
///   walker diagnostics, dependency rail and value. Each is a self-charged
///   allocation ([`verter_session_query::retention::owned`]): the delivery
///   that minted it pinned it for its caller, every clone of it keeps that
///   one charge, and the candidate's hold retains it while it lives — so a
///   caller that outlives the cache keeps the allocation charged (pinned)
///   until its own last clone drops, and a delivery the cache refuses was
///   charged all along.
#[derive(Debug)]
pub(super) struct MemoRetention {
    _own: RetentionCharge,
    _carrier: Box<[CacheHold]>,
    /// The `_carrier` slice's own backing.
    _carrier_backing: RetentionCharge,
    _delivered: HeldHandles,
}

impl MemoRetention {
    /// The candidate's own reservation.
    #[cfg(test)]
    pub(super) fn own_bytes(&self) -> usize {
        self._own.bytes()
    }

    /// How many holds the candidate keeps in each of its two hold slices:
    /// its carrier holds, and its holds on the allocations it delivers.
    #[cfg(test)]
    pub(super) fn hold_slice_lens(&self) -> (usize, usize) {
        (self._carrier.len(), self._delivered.len())
    }

    /// The candidate's registered holds on its carrier.
    #[cfg(test)]
    pub(super) fn shared_holds(&self) -> impl Iterator<Item = &CacheHold> {
        self._carrier.iter()
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
///
/// The key's owned names are charged by their own cells, once however
/// many key copies reach them; the family holds them beside its charge.
pub(super) fn family_container_footprint(
    family: &super::family::FamilyKey,
) -> verter_session_query::retention::Footprint {
    use super::family_footprint::family_key_heap_bytes;
    let mut aliases = verter_session_query::retention::allocation::AliasSet::new();
    let map_key = family_key_heap_bytes(family, &mut aliases);
    let ledger_key = family_key_heap_bytes(family, &mut aliases);
    verter_session_query::retention::Footprint {
        bytes: std::mem::size_of::<(super::family::FamilyKey, super::family::FamilySlots)>()
            + 1
            + std::mem::size_of::<(u64, super::family::FamilyKey)>()
            + map_key
            + ledger_key,
        handles: aliases.take_handles(),
    }
}

/// [`family_container_footprint`]'s own bytes: a figure for fixtures.
#[cfg(test)]
pub(super) fn family_container_bytes(family: &super::family::FamilyKey) -> usize {
    family_container_footprint(family).bytes
}

/// The owned allocations a delivered result reaches: its value's owned
/// carriers and every owned name its error holds. A delivered result keeps
/// nothing reachable outside the graph except through these, so each is
/// charged by its own cell.
pub(super) fn result_handles(
    result: &QueryResult<SemanticQueryValue>,
) -> verter_session_query::retention::owned::OwnedHandles {
    use crate::semantic_query::allocation_footprint::OwnedHeap as _;
    verter_session_query::retention::owned::OwnedHandles::of(|aliases| {
        result.owned_heap_bytes(aliases)
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
        Self::with_arena(Default::default(), retention_account)
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
        direct_edges: usize,
    ) -> Result<Arc<MemoRetention>, RetentionRefusal> {
        let reserved = match entry
            .cost_receipt
            .reserve_retention(self.retention_account())
        {
            Ok(()) => self.reserve_candidate_charges(entry, family, populated_slots, direct_edges),
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
        direct_edges: usize,
    ) -> Result<MemoRetention, RetentionRefusal> {
        let shared = self.hold_shared_allocations(SharedParts::of(entry))?;
        let own = own_reservation(entry, family, populated_slots, direct_edges);
        match verter_session_query::facts::receipt::reserve_retained_with_evidence(
            self.retention_account(),
            own.bytes,
            &[&entry.read_set_signature.facts],
        ) {
            RetentionAdmission::Admitted(charge) => {
                Ok(shared.with_own(own.handles.retained_with(charge, self.charge_ctx())?))
            }
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
        // Every delivered allocation is held in one charged slice, all or
        // nothing.
        let delivered = parts
            .lease
            .metadata_handles()
            .merge(OwnedHandles::of(|aliases| {
                parts
                    .diagnostics
                    .owned()
                    .map_or(0, |diagnostics| diagnostics.offer(aliases))
                    + parts
                        .dep_signature
                        .owned()
                        .map_or(0, |rail| rail.offer(aliases))
            }))
            .merge(result_handles(parts.result))
            .hold(self.charge_ctx())?;
        // The slice keeping the carrier holds lives as long as they do.
        let carrier = carrier.into_boxed_slice();
        let carrier_backing = match account.reserve(
            verter_session_query::retention::ChargeClass::Retained,
            verter_session_query::retention::allocation::heap_block_bytes(std::mem::size_of_val::<
                [CacheHold],
            >(&carrier)),
        ) {
            RetentionAdmission::Admitted(charge) => charge,
            RetentionAdmission::Refused(refusal) => return Err(refusal),
        };
        Ok(SharedHolds {
            carrier,
            carrier_backing,
            delivered,
        })
    }

    /// Charge the allocations a delivery hands its caller to that caller:
    /// pinned, never refused, and shared with every other holder of the
    /// same allocation — each carries its own charge, so a clone the caller
    /// keeps past the delivery keeps it charged.
    pub(super) fn pin_delivered_allocations(
        &self,
        delivered: &super::delivery_lease::DeliveredAllocations<'_>,
    ) {
        if let Some(diagnostics) = delivered.diagnostics.and_then(|list| list.owned()) {
            diagnostics.pin(self.charge_ctx());
        }
        if let Some(rail) = delivered.dep_signature.and_then(|rail| rail.owned()) {
            rail.pin(self.charge_ctx());
        }
        result_handles(delivered.result).pin(self.charge_ctx());
    }

    /// Where this store's self-charged allocations are charged: its
    /// account.
    pub(super) fn charge_ctx(&self) -> ChargeCtx<'_> {
        ChargeCtx::new(self.retention_account())
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
            Entry::Vacant(slot) => match self.reserve_family_container(family) {
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

    /// A newly keyed family's container charge, holding the key's owned
    /// names: refusable, all or nothing.
    pub(super) fn reserve_family_container(
        &self,
        family: &super::family::FamilyKey,
    ) -> RetentionAdmission {
        let footprint = family_container_footprint(family);
        match self.retention_account().reserve(
            verter_session_query::retention::ChargeClass::Retained,
            footprint.bytes,
        ) {
            RetentionAdmission::Admitted(charge) => {
                match footprint.handles.retained_with(charge, self.charge_ctx()) {
                    Ok(charge) => RetentionAdmission::Admitted(charge),
                    Err(refusal) => RetentionAdmission::Refused(refusal),
                }
            }
            refused @ RetentionAdmission::Refused(_) => refused,
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
    /// indexes' own storage, the publishes waiting to enter the family
    /// memo's index, its shared-charge registry's table and its
    /// dependency-rail interner. A required current count.
    pub fn bookkeeping_storage_charge_bytes(&self) -> usize {
        self.canonical_to_entries.storage_charge_bytes()
            + self.refusal_summaries.index_storage_charge_bytes()
            + self.reverse_index_pending.lock().storage_charge_bytes()
            + self.alias_charges.table_charged_bytes()
            + self.dep_signature_interner.storage_charge_bytes()
            + self.derivation.lock().storage_charge_bytes()
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
    pub(super) diagnostics: &'a crate::project_semantic_dispatch::walk::WalkerDiagnostics,
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
    carrier_backing: RetentionCharge,
    delivered: HeldHandles,
}

impl SharedHolds {
    /// The candidate's charges, once its own reservation is granted.
    pub(super) fn with_own(self, own: RetentionCharge) -> MemoRetention {
        MemoRetention {
            _own: own,
            _carrier: self.carrier,
            _carrier_backing: self.carrier_backing,
            _delivered: self.delivered,
        }
    }
}

/// What `entry`'s own reservation covers: its bookkeeping for
/// `populated_slots` slots and a reverse-index registration naming
/// `direct_edges` edges.
pub(super) fn own_reservation(
    entry: &MemoEntry,
    family: &super::family::FamilyKey,
    populated_slots: usize,
    direct_edges: usize,
) -> verter_session_query::retention::Footprint {
    entry.own_retained_footprint(family, populated_slots, direct_edges)
}
