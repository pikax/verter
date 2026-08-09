//! The ONE batched SCC member publish — the store-owned admission path
//! every deferred component member rides, across ALL THREE domains.
//!
//! A relation, flow-return, or call-resolution SCC defers its non-root
//! members: each one
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
//! 3. **Retention self-exemption.** The batch's own admissions drive the
//!    shared global family-retention FIFO, so under cap pressure a
//!    member's admission can select a victim — and the two victims that
//!    would tear the component are the batch's OWN witnessed root and the
//!    batch's OWN already-written members. A per-member admission cannot
//!    see that: it would publish a proper SUFFIX of the component onto a
//!    root it had just evicted, and *which* suffix survived would depend
//!    on the order the drain happened to iterate its members. So the
//!    whole batch is ONE budget step whose victim selection exempts the
//!    root plus every member family
//!    ([`SemanticGraphStore::record_family_admissions_locked`]), and a
//!    component whose resident footprint cannot fit the budget cap
//!    REFUSES WHOLE — an exemption wider than the cap would pin the
//!    ledger above its bound. The per-member write no longer touches the
//!    budget at all: it hands back a `#[must_use]` [`NewlyKeyedFamily`]
//!    the batch accumulates, so adding a member cannot reintroduce a
//!    per-member admission without deleting that contract outright.
//!
//! Only decided, non-degraded results publish. A degraded flow success is
//! `ReturnOnly` by contract and a non-binary relation outcome has no
//! value-domain form — either one refuses the WHOLE batch here rather
//! than admitting a torn component.

use super::inflight::InflightState;
use super::relation_memo::relation_satisfied_projection;
use super::resolve_call_memo::resolve_call_satisfied_projection;
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

    /// The witness of a call-resolution SCC root's published candidate.
    pub(crate) fn resolve_call(
        key: crate::semantic_query::ResolveCallKey,
        admission_seq: u64,
    ) -> Self {
        Self {
            family: FamilyKey::ResolveCall { key: Box::new(key) },
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

/// One call-resolution member queued for the batched publish.
pub(crate) struct PendingResolveCallMember {
    /// The member's full call-resolution identity.
    pub(crate) key: crate::semantic_query::ResolveCallKey,
    /// The admitted result its inline compute produced — the
    /// admissibility seal already excludes a rootless winner, so the
    /// batch needs no further result-kind gate for this domain.
    pub(crate) result: crate::semantic_query::AdmissibleCallResult,
    /// The ordinary family flight the member claimed.
    pub(crate) flight: InlineResolveCallFlight,
}

/// Whether a member's `entries` write made its family NEWLY resident,
/// and therefore still owes the retention budget a ledger record.
///
/// `#[must_use]` on purpose: the per-member write deliberately does NOT
/// record the admission itself (that is what let a member evict its own
/// component's root), so dropping this answer would strand the family
/// outside the budget and let the memo grow past its cap. The batch
/// collects every newly-keyed family and settles them in ONE exempting
/// budget step.
#[must_use]
struct NewlyKeyedFamily(bool);

/// A member's claimed flight, in whichever domain it belongs to.
enum StagedFlight {
    Relation(InlineRelationFlight),
    Flow(InlineFlowReturnFlight),
    ResolveCall(InlineResolveCallFlight),
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
    /// invalidation aborted a flight, a member's result is not
    /// admissible, or the component cannot fit the retention budget.
    /// Waiting joiners wake on the abort sentinel and retry admission.
    pub(crate) fn publish_scc_members_fenced(
        &self,
        ctx: Option<&dyn crate::resolver_core::ResolverContext>,
        required_root: &SccRootWitness,
        carrier: &crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: &Arc<[Arc<str>]>,
        validated_at_generation: u64,
        relation_members: Vec<PendingRelationMember>,
        flow_members: Vec<PendingFlowReturnMember>,
        call_members: Vec<PendingResolveCallMember>,
    ) -> bool {
        if relation_members.is_empty() && flow_members.is_empty() && call_members.is_empty() {
            return true;
        }
        let dispatch_dep_signature = self.dep_signature_interner.intern(&empty_signature());

        // WHOLE-BATCH ADMISSIBILITY. Every gate here is release-active
        // and every one is evaluated BEFORE any staging: a batched
        // publish cannot be half-refused, so a single inadmissible member
        // takes the entire component to `ReturnOnly`. The caller already
        // holds every value; refusing only withholds warmth.
        //
        // 1. RETENTION FOOTPRINT. A component is coherent only while its
        //    root AND every member stay resident, so publishing commits
        //    the global family budget to `1 + members` co-resident
        //    families. A component that cannot fit is not warmable at
        //    all: exempting a set wider than the cap from FIFO victim
        //    selection would pin the ledger permanently above its bound,
        //    and publishing without the exemption would tear the
        //    component. The count is a deliberate UPPER BOUND (`1 + len`,
        //    not the distinct-family count) — a component's member list
        //    is duplicate-free by construction, so the bound is exact in
        //    practice and costs no set allocation on the publish path.
        //
        // 2. DEGRADED FLOW SUCCESS. A usable value, but `ReturnOnly` by
        //    contract — no memo entry, no fact signature, no
        //    reverse-index metadata. Under a MIXED component this is not
        //    free: a relation machinery root's verdict is binary and
        //    carries no degradation channel, so one degraded flow member
        //    costs the clean relation siblings their warmth too. That is
        //    a cold-recompute cost, never a wrong value — the alternative
        //    (admitting the clean siblings) is the torn component.
        //
        // 3. NON-BINARY RELATION OUTCOME. `Unknown` / `BudgetExceeded`
        //    have no value-domain form and must never enter the memo
        //    (the decided-only admission contract). The authority maps
        //    them to `ReturnOnly` before ever reaching here, so this is a
        //    backstop — but a backstop spelled as a `debug_assert!` is
        //    absent from the builds that ship, where the payload would
        //    proceed to publication. It refuses instead.
        let footprint = 1 + relation_members.len() + flow_members.len() + call_members.len();
        let inadmissible = footprint > self.memo_budget.cap()
            || flow_members
                .iter()
                .any(|member| member.result.degradation().is_some())
            || relation_members.iter().any(|member| {
                !matches!(
                    member.payload.outcome,
                    crate::semantic_query::RelationOutcome::Assignable
                        | crate::semantic_query::RelationOutcome::NotAssignable
                )
            });
        if inadmissible {
            for member in relation_members {
                self.abort_inline_relation_flight(&member.flight);
            }
            for member in flow_members {
                self.abort_inline_flow_return_flight(&member.flight);
            }
            for member in call_members {
                self.abort_inline_resolve_call_flight(&member.flight);
            }
            return false;
        }

        let mut staged: Vec<StagedMember> =
            Vec::with_capacity(relation_members.len() + flow_members.len() + call_members.len());
        for member in relation_members {
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
        for member in call_members {
            debug_assert!(
                matches!(member.flight.prepared.key(), SemanticQueryKey::ResolveCall(k) if **k == member.key),
                "an inline ResolveCall flight must publish its own exact full key"
            );
            let family = FamilyKey::ResolveCall {
                key: Box::new(member.key),
            };
            let entry = self.stage_entry(
                SemanticQueryValue::ResolveCall(Arc::new(member.result.into_inner())),
                resolve_call_satisfied_projection(),
                carrier,
                self_root_canonicals,
                &dispatch_dep_signature,
                validated_at_generation,
            );
            staged.push(self.stage_member(
                family,
                entry,
                StagedFlight::ResolveCall(member.flight),
                ctx,
            ));
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
        // Every member family the batch touches, whether or not its write
        // newly keyed it — an ALREADY-resident member family still carries
        // an older ledger record the batch's own admissions must not
        // select. `newly_keyed` is the narrower subset that owes a fresh
        // record.
        //
        // HASHED, not a linear scan: the exemption is consulted once per
        // ledger record the trim walks, so a linear membership test would
        // make the step quadratic in component size while the `entries`
        // lock is held.
        let mut component: rustc_hash::FxHashSet<FamilyKey> =
            rustc_hash::FxHashSet::with_capacity_and_hasher(
                staged.len() + 1,
                rustc_hash::FxBuildHasher,
            );
        component.insert(required_root.family.clone());
        let mut newly_keyed: Vec<FamilyKey> = Vec::with_capacity(staged.len());
        for member in staged {
            let NewlyKeyedFamily(is_new) = self.publish_staged_locked(
                &mut entries,
                &member.family,
                member.entry,
                member.eviction,
                carrier,
                &dispatch_dep_signature,
                member.admission_seq,
            );
            if is_new {
                newly_keyed.push(member.family.clone());
            }
            component.insert(member.family);
            to_complete.push((member.flight, member.completed));
        }
        // ONE budget step for the whole component, with the root and every
        // member family exempt from victim selection. The footprint gate
        // above guarantees the exempt set fits the cap, so the trim still
        // settles the ledger at exactly `cap`.
        self.record_family_admissions_locked(&mut entries, &newly_keyed, &|family| {
            component.contains(family)
        });
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
    /// `entries` + reverse-index halves of the consistency cluster. The
    /// caller holds the batch's single `entries` lock.
    ///
    /// The retention-budget half is deliberately NOT landed here — see
    /// [`NewlyKeyedFamily`]. A per-member budget admission is exactly the
    /// step that could select the batch's own root as its FIFO victim, so
    /// the answer is returned and the caller settles every newly-keyed
    /// family in one exempting step.
    fn publish_staged_locked(
        &self,
        entries: &mut FxHashMap<FamilyKey, FamilySlots>,
        family: &FamilyKey,
        entry: MemoEntry,
        eviction: family::EvictionVictim,
        carrier: &crate::fact_signature_helpers::ReadSetSignature,
        dispatch_dep_signature: &DepSignature,
        admission_seq: u64,
    ) -> NewlyKeyedFamily {
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
        reverse_index::register_reverse_index(
            &self.canonical_to_entries,
            family,
            &populated_slots,
            carrier,
            dispatch_dep_signature,
            admission_seq,
        );
        NewlyKeyedFamily(family_was_new && !populated_slots.is_empty())
    }

    fn abort_staged_flight(&self, flight: &StagedFlight) {
        match flight {
            StagedFlight::Relation(flight) => self.abort_inline_relation_flight(flight),
            StagedFlight::Flow(flight) => self.abort_inline_flow_return_flight(flight),
            StagedFlight::ResolveCall(flight) => self.abort_inline_resolve_call_flight(flight),
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
        let NewlyKeyedFamily(is_new) = self.publish_staged_locked(
            &mut entries,
            &family,
            entry,
            eviction,
            &carrier,
            &dispatch_dep_signature,
            admission_seq,
        );
        // A seeded ROOT is a lone family, not a component: it takes the
        // ordinary unexempted admission, exactly like `warm_publish_one`.
        if is_new {
            self.record_family_admission_locked(&mut entries, &family);
        }
    }
}

impl StagedFlight {
    fn state(&self) -> &parking_lot::Mutex<InflightState> {
        match self {
            StagedFlight::Relation(flight) => &flight.inflight.state,
            StagedFlight::Flow(flight) => &flight.inflight.state,
            StagedFlight::ResolveCall(flight) => &flight.inflight.state,
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
            StagedFlight::ResolveCall(flight) => {
                flight.inflight.ready.notify_all();
                store.retire_inflight(&flight.prepared, &flight.inflight, false);
            }
        }
    }
}
