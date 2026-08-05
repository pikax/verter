//! The ONE batched SCC member publish — the store-owned admission path
//! every deferred component member rides, across BOTH domains.
//!
//! A relation or flow-return SCC defers its non-root members: each one
//! claims the ordinary family flight, computes inline on the root's
//! transaction, and batches its publish for the root to drain. The root
//! publishes first through the family singleflight; the members then
//! ride the root's SCC-union carrier.
//!
//! Two properties make that safe, and both are enforced HERE rather than
//! by call-site discipline:
//!
//! 1. **The root-witness fence.** The root's published candidate is
//!    identified by its `admission_seq`, and the batch refuses unless
//!    that exact candidate is still in the root family's slot AT THE
//!    MOMENT of publication — checked under the SAME `entries` lock the
//!    publish will use. A member drained onto a root an invalidation has
//!    already swept would otherwise publish anyway and serve a live warm
//!    read from a component whose root no longer exists; the invalidation
//!    abort sweep cannot cover it, because a deferred, never-published
//!    member's flight holds no reverse-index registration. The witness is
//!    family-agnostic and MANDATORY: a flow root and a relation root are
//!    both expressible, so no drain site can opt out.
//!
//! 2. **Atomicity.** Every member is written under ONE `entries` hold.
//!    Publishing members one at a time lets an invalidation land BETWEEN
//!    two of them and leave a partially-published component; a superseded
//!    root must release the ENTIRE component with ZERO member
//!    publication.
//!
//! Only decided, non-degraded results publish. A degraded flow success is
//! `ReturnOnly` by contract and a non-binary relation outcome has no
//! value-domain form — either one refuses the WHOLE batch here rather
//! than admitting a torn component.

use super::inflight::InflightState;
use super::relation_memo::relation_satisfied_projection;
use super::*;

/// The root candidate a batched SCC member publish is fenced on.
///
/// Family-agnostic by construction: a relation SCC root and a
/// flow-return SCC root are both expressible, which is exactly why the
/// drain sites can no longer pass "no root".
#[derive(Clone)]
pub(crate) struct SccRootWitness {
    family: FamilyKey,
    admission_seq: u64,
}

impl SccRootWitness {
    /// The witness of a relation SCC root's published candidate.
    pub(crate) fn relate(key: crate::semantic_query::RelateMemoKey, admission_seq: u64) -> Self {
        Self {
            family: FamilyKey::Relate { key: Box::new(key) },
            admission_seq,
        }
    }

    /// The witness of a flow-return SCC root's published candidate.
    pub(crate) fn flow_return(
        key: crate::semantic_query::FlowReturnKey,
        admission_seq: u64,
    ) -> Self {
        Self {
            family: FamilyKey::FlowReturn { key: Box::new(key) },
            admission_seq,
        }
    }
}

/// One relation member queued for the batched publish.
pub(crate) struct PendingRelationMember {
    /// The member's full relation identity.
    pub(crate) key: crate::semantic_query::RelateMemoKey,
    /// The decided binary payload its inline compute produced.
    pub(crate) payload: crate::semantic_query::RelationPayload,
    /// The ordinary family flight the member claimed.
    pub(crate) flight: InlineRelationFlight,
}

/// One flow-return member queued for the batched publish.
pub(crate) struct PendingFlowReturnMember {
    /// The member's full flow-return identity.
    pub(crate) key: crate::semantic_query::FlowReturnKey,
    /// The complete whole-function result its inline compute produced.
    pub(crate) result: crate::semantic_query::FlowReturnResult,
    /// The point set that compute ACTUALLY materialised (§3.4) —
    /// recorded by the compute, never re-derived from the nominal key.
    pub(crate) materialized: MaterializedSet,
    /// The ordinary family flight the member claimed.
    pub(crate) flight: InlineFlowReturnFlight,
}

/// A member's claimed flight, in whichever domain it belongs to.
enum StagedFlight {
    Relation(InlineRelationFlight),
    Flow(InlineFlowReturnFlight),
}

/// One member prepared for publication: everything computed OUTSIDE the
/// `entries` lock, so the batch's single hold does no planning work.
struct StagedMember {
    family: FamilyKey,
    entry: MemoEntry,
    eviction: family::EvictionVictim,
    /// The value a waiting joiner receives when the flight completes.
    completed: QueryResult<SemanticQueryValue>,
    admission_seq: u64,
    flight: StagedFlight,
}

impl SemanticGraphStore {
    /// Publish a component's deferred members onto the root's published
    /// carrier — the SOLE member-admission entry for both domains.
    ///
    /// Returns whether the batch published. `false` means NOTHING was
    /// written and every flight was released: the root witness no longer
    /// matches a live candidate, the caller was cancelled, an
    /// invalidation aborted a flight, or a member's result is not
    /// admissible. Waiting joiners wake on the abort sentinel and retry
    /// admission.
    pub(crate) fn publish_scc_members_fenced(
        &self,
        ctx: Option<&dyn crate::resolver_core::ResolverContext>,
        required_root: &SccRootWitness,
        carrier: &crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: &Arc<[Arc<str>]>,
        validated_at_generation: u64,
        relation_members: Vec<PendingRelationMember>,
        flow_members: Vec<PendingFlowReturnMember>,
    ) -> bool {
        if relation_members.is_empty() && flow_members.is_empty() {
            return true;
        }
        let dispatch_dep_signature = self.dep_signature_interner.intern(&empty_signature());

        // A degraded flow success is a usable value but ReturnOnly by
        // contract — no memo entry, no fact signature, no reverse-index
        // metadata. Under a BATCHED publish that refusal is necessarily
        // whole-component: a component cannot be half-admitted. The
        // caller already holds every value.
        if flow_members
            .iter()
            .any(|member| member.result.degradation.is_some())
        {
            for member in relation_members {
                self.abort_inline_relation_flight(&member.flight);
            }
            for member in flow_members {
                self.abort_inline_flow_return_flight(&member.flight);
            }
            return false;
        }

        let mut staged: Vec<StagedMember> =
            Vec::with_capacity(relation_members.len() + flow_members.len());
        for member in relation_members {
            debug_assert!(
                matches!(
                    member.payload.outcome,
                    crate::semantic_query::RelationOutcome::Assignable
                        | crate::semantic_query::RelationOutcome::NotAssignable
                ),
                "only decided binary relation payloads publish; {:?} must route ReturnOnly",
                member.payload
            );
            debug_assert!(
                member.flight.prepared.key() == &member.key.to_query_key(),
                "an inline relation flight must publish its own exact full key"
            );
            let family = FamilyKey::Relate {
                key: Box::new(member.key),
            };
            let entry = self.stage_entry(
                SemanticQueryValue::Relation(member.payload),
                relation_satisfied_projection(),
                carrier,
                self_root_canonicals,
                &dispatch_dep_signature,
                validated_at_generation,
            );
            staged.push(self.stage_member(
                family,
                entry,
                StagedFlight::Relation(member.flight),
                ctx,
            ));
        }
        for member in flow_members {
            debug_assert!(
                matches!(member.flight.prepared.key(), SemanticQueryKey::FlowReturn(k) if **k == member.key),
                "an inline flow-return flight must publish its own exact full key"
            );
            let family = FamilyKey::FlowReturn {
                key: Box::new(member.key),
            };
            let entry = self.stage_entry(
                SemanticQueryValue::FlowReturn(Arc::new(member.result)),
                member.materialized,
                carrier,
                self_root_canonicals,
                &dispatch_dep_signature,
                validated_at_generation,
            );
            staged.push(self.stage_member(family, entry, StagedFlight::Flow(member.flight), ctx));
        }

        // Test-only injection point — parked immediately before the
        // batch's single `entries` acquisition, so a race test can
        // invalidate the SCC root deterministically inside the
        // publication window. `None` (the production default) is a no-op.
        #[cfg(any(test, feature = "test-support"))]
        {
            let gate = self.relation_member_pre_entries_gate.lock().clone();
            if let Some(gate) = gate {
                gate.wait();
                gate.wait();
            }
        }

        let mut entries = self.entries_lock_diagnosed();
        let root_is_published = entries.get(&required_root.family).is_some_and(|slots| {
            slots
                .snapshot_slot(ModeSlot::Single)
                .iter()
                .any(|entry| entry.admission_seq == required_root.admission_seq)
        });
        let cancelled = self.force_cold_abort_sweep.load(Ordering::Relaxed)
            || ctx.is_some_and(|ctx| ctx.is_cancelled());
        if cancelled {
            for member in &staged {
                member.flight.mark_aborted();
            }
        }
        let any_aborted = staged.iter().any(|member| member.flight.is_aborted());
        if !root_is_published || any_aborted {
            drop(entries);
            record_cold_abort_swept(&self.stats);
            for member in staged {
                self.abort_staged_flight(&member.flight);
            }
            return false;
        }

        let mut to_complete: Vec<(StagedFlight, QueryResult<SemanticQueryValue>)> =
            Vec::with_capacity(staged.len());
        for member in staged {
            self.publish_staged_locked(
                &mut entries,
                &member.family,
                member.entry,
                member.eviction,
                carrier,
                &dispatch_dep_signature,
                member.admission_seq,
            );
            to_complete.push((member.flight, member.completed));
        }
        drop(entries);

        for (flight, completed) in to_complete {
            {
                let state_lock = flight.state();
                let mut state = state_lock.lock();
                if state.aborted {
                    drop(state);
                    self.abort_staged_flight(&flight);
                    continue;
                }
                state.completed = Some(completed);
                state.dep_signature = Some(empty_signature());
                state.graph_carrier = Some(Box::new(carrier.clone()));
                state.self_root_canonicals = Arc::clone(self_root_canonicals);
                state.walker_diagnostics = Some(Arc::from([]));
                state.cache_suppress = false;
                state.result_is_partial = false;
            }
            flight.notify_and_retire(self);
        }
        true
    }

    /// Build one candidate for the batch. Pure construction — no lock,
    /// no eviction planning.
    fn stage_entry(
        &self,
        value: SemanticQueryValue,
        satisfied_projection: MaterializedSet,
        carrier: &crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: &Arc<[Arc<str>]>,
        dispatch_dep_signature: &DepSignature,
        validated_at_generation: u64,
    ) -> MemoEntry {
        MemoEntry {
            result: QueryResult::Value(value),
            read_set_signature: carrier.clone(),
            dispatch_dep_signature: Arc::clone(dispatch_dep_signature),
            self_root_canonicals: Arc::clone(self_root_canonicals),
            walker_diagnostics: Arc::from([]),
            satisfied_projection,
            validated_at_generation,
            admission_seq: 0,
        }
    }

    /// Finish staging one member: allocate its admission token and plan
    /// its per-family bounded-retention eviction against the publishing
    /// caller's stable store view. Both run OUTSIDE the batch's `entries`
    /// hold — `plan_family_slot_eviction` takes the lock itself.
    fn stage_member(
        &self,
        family: FamilyKey,
        mut entry: MemoEntry,
        flight: StagedFlight,
        ctx: Option<&dyn crate::resolver_core::ResolverContext>,
    ) -> StagedMember {
        let admission_seq = self.alloc_candidate_admission_seq();
        entry.admission_seq = admission_seq;
        let QueryResult::Value(value) = &entry.result else {
            unreachable!("a staged SCC member always carries a decided value")
        };
        let completed = QueryResult::Value(value.clone());
        let cap = family.candidate_cap();
        let eviction = match ctx {
            Some(ctx) => family::plan_family_slot_eviction(
                &self.entries,
                &family,
                ModeSlot::Single,
                &entry,
                cap,
                ctx,
            ),
            None => family::EvictionVictim::LruFront,
        };
        StagedMember {
            family,
            entry,
            eviction,
            completed,
            admission_seq,
            flight,
        }
    }

    /// Write one prepared candidate into the family memo and land the
    /// `(entries, memo_budget, reverse-index)` consistency cluster. The
    /// caller holds the batch's single `entries` lock.
    fn publish_staged_locked(
        &self,
        entries: &mut FxHashMap<FamilyKey, FamilySlots>,
        family: &FamilyKey,
        entry: MemoEntry,
        eviction: family::EvictionVictim,
        carrier: &crate::fact_signature_helpers::ReadSetSignature,
        dispatch_dep_signature: &DepSignature,
        admission_seq: u64,
    ) {
        let cap = family.candidate_cap();
        let family_was_new = !entries.contains_key(family);
        let outcome = entries.entry(family.clone()).or_default().publish(
            ModeSlot::Single,
            entry,
            &ProjectionPath::empty(),
            cap,
            eviction,
        );
        let populated_slots = outcome.populated;
        for (displaced_slot, displaced_entry) in &outcome.displaced {
            reverse_index::drain_candidate_reverse_index_registrations(
                &self.canonical_to_entries,
                family,
                *displaced_slot,
                displaced_entry,
            );
        }
        if family_was_new && !populated_slots.is_empty() {
            self.record_family_admission_locked(entries, family);
        }
        reverse_index::register_reverse_index(
            &self.canonical_to_entries,
            family,
            &populated_slots,
            carrier,
            dispatch_dep_signature,
            admission_seq,
        );
    }

    fn abort_staged_flight(&self, flight: &StagedFlight) {
        match flight {
            StagedFlight::Relation(flight) => self.abort_inline_relation_flight(flight),
            StagedFlight::Flow(flight) => self.abort_inline_flow_return_flight(flight),
        }
    }

    /// Test-support seed seam: publish ONE decided candidate directly,
    /// with no flight and no root fence. Backs the relation fixture
    /// seams, which seed a ROOT rather than drain a member.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn publish_unfenced_candidate_for_tests(
        &self,
        ctx: Option<&dyn crate::resolver_core::ResolverContext>,
        family: FamilyKey,
        value: SemanticQueryValue,
        satisfied_projection: MaterializedSet,
        carrier: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        validated_at_generation: u64,
    ) {
        let dispatch_dep_signature = self.dep_signature_interner.intern(&empty_signature());
        let entry = self.stage_entry(
            value,
            satisfied_projection,
            &carrier,
            &self_root_canonicals,
            &dispatch_dep_signature,
            validated_at_generation,
        );
        let admission_seq = self.alloc_candidate_admission_seq();
        let cap = family.candidate_cap();
        let eviction = match ctx {
            Some(ctx) => family::plan_family_slot_eviction(
                &self.entries,
                &family,
                ModeSlot::Single,
                &entry,
                cap,
                ctx,
            ),
            None => family::EvictionVictim::LruFront,
        };
        let mut entry = entry;
        entry.admission_seq = admission_seq;
        let mut entries = self.entries_lock_diagnosed();
        self.publish_staged_locked(
            &mut entries,
            &family,
            entry,
            eviction,
            &carrier,
            &dispatch_dep_signature,
            admission_seq,
        );
    }
}

impl StagedFlight {
    fn state(&self) -> &parking_lot::Mutex<InflightState> {
        match self {
            StagedFlight::Relation(flight) => &flight.inflight.state,
            StagedFlight::Flow(flight) => &flight.inflight.state,
        }
    }

    fn mark_aborted(&self) {
        self.state().lock().aborted = true;
    }

    fn is_aborted(&self) -> bool {
        self.state().lock().aborted
    }

    /// Wake every joiner and retire the flight from the ORDINARY table —
    /// the table `begin_inline_*_flight` claimed in.
    fn notify_and_retire(self, store: &SemanticGraphStore) {
        match self {
            StagedFlight::Relation(flight) => {
                flight.inflight.ready.notify_all();
                store.retire_inflight(&flight.prepared, &flight.inflight, false);
            }
            StagedFlight::Flow(flight) => {
                flight.inflight.ready.notify_all();
                store.retire_inflight(&flight.prepared, &flight.inflight, false);
            }
        }
    }
}
