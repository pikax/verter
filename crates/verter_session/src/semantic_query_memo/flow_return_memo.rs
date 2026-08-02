//! FlowReturn storage — the payload read/write path over the family
//! memo's `FlowReturn` family.
//!
//! Storage is the family memo's [`FamilyKey::FlowReturn`] family in the
//! [`ModeSlot::Single`] slot. The stored value is the
//! [`SemanticQueryValue::FlowReturn`] payload — COMPLETE whole-function
//! results ONLY: a typed `FlowReturnFailure` (`Unsupported` / `Missing` /
//! `Budget` / `EmptyCycle` / `Unresolved`) has no value-domain form and is
//! never admitted anywhere (memo / fact / reverse index). Warm reads
//! validate the self-version-rooted carrier strictly AND hard-miss on a
//! `validated_at_generation` mismatch (the family carries the
//! live-generation gate). Retention rides the family rails (cap 8,
//! invalid-first / LRU eviction, reverse-index drains).

use super::*;

/// The `satisfied_projection` every flow-return entry carries: the
/// modeless [`ModeSlot::Single`] identity point at the empty path (same
/// modeless-family treatment as the relation family).
fn flow_return_satisfied_projection() -> MaterializedSet {
    MaterializedSet::single(MaterializedPoint::new(family::point_for_slot(
        ModeSlot::Single,
        &ProjectionPath::empty(),
    )))
}

/// Store-owned admission token for a flow-return member computed inline
/// by another obligation's transaction. Registering the token in the
/// ordinary flow-return-family flight table lets a concurrent top-level
/// request join the inline compute instead of starting duplicate cold
/// work.
#[derive(Clone)]
pub(crate) struct InlineFlowReturnFlight {
    prepared: PreparedKeyHandle,
    inflight: Arc<InflightEntry>,
    /// Present only when an inline flight starts outside an existing
    /// semantic execution stack. Production nested evaluations reuse the
    /// active owner; direct callers hold this detached RAII lease.
    _owner_registration: Option<wait_cycle::ExecutionOwnerRegistration>,
}

impl std::fmt::Debug for InlineFlowReturnFlight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InlineFlowReturnFlight")
            .field("key", self.prepared.key())
            .finish_non_exhaustive()
    }
}

impl InlineFlowReturnFlight {
    /// The exact key this flight admitted for (debug assertions only).
    pub(crate) fn prepared_key(&self) -> &SemanticQueryKey {
        self.prepared.key()
    }
}

impl SemanticGraphStore {
    /// Claim the ordinary family flight for a flow-return member computed
    /// inline. `None` means another cold owner already owns this exact
    /// full key.
    pub(crate) fn begin_inline_flow_return_flight(
        &self,
        key: &crate::semantic_query::FlowReturnKey,
    ) -> Option<InlineFlowReturnFlight> {
        let prepared =
            PreparedKeyHandle::prepare(SemanticQueryKey::FlowReturn(Box::new(key.clone())));
        let inflight = Arc::new(InflightEntry::new());
        let (owner, owner_registration) =
            if let Some(owner) = wait_cycle::ExecutionOwnerScope::current(&self.wait_for_graph) {
                (owner, None)
            } else {
                let registration = self.wait_for_graph.register_owner();
                (registration.owner(), Some(registration))
            };
        {
            let mut state = inflight.state.lock();
            state.claimed = true;
            state.owner = Some(owner);
        }
        let mut table = self.inflight.lock();
        if table.contains_key(&prepared) {
            return None;
        }
        table.insert(prepared.clone(), Arc::clone(&inflight));
        Some(InlineFlowReturnFlight {
            prepared,
            inflight,
            _owner_registration: owner_registration,
        })
    }

    /// Release an inline flow flight that cannot publish a decided
    /// member. Waiting top-level callers wake on the abort sentinel and
    /// retry admission.
    pub(crate) fn abort_inline_flow_return_flight(&self, flight: &InlineFlowReturnFlight) {
        {
            let mut state = flight.inflight.state.lock();
            state.aborted = true;
            if state.completed.is_none() {
                state.completed = Some(QueryResult::Error(QueryError::Other(Arc::from(
                    "inline flow-return flight abandoned",
                ))));
                state.dep_signature = Some(empty_signature());
            }
            state.graph_carrier = None;
            state.walker_diagnostics = None;
            state.cache_suppress = true;
            state.result_is_partial = true;
        }
        flight.inflight.ready.notify_all();
        self.retire_inflight(&flight.prepared, &flight.inflight, false);
    }

    /// The strict warm read of the `FlowReturn` family (design §3.4):
    /// carrier validation AND the live-generation gate. An entry carries
    /// the `FlowBody` fact rail plus its consumed subquery facts and self
    /// roots; `validate(ctx)` revalidates that whole signature against
    /// the caller's live view, so a body edit or a torn fact set hard-misses.
    pub(crate) fn get_flow_return_result(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        key: &crate::semantic_query::FlowReturnKey,
    ) -> Option<crate::semantic_query::FlowReturnResult> {
        let family = FamilyKey::FlowReturn {
            key: Box::new(key.clone()),
        };
        // Snapshot the candidate list under the `entries` lock, then
        // validate OUTSIDE the lock (the family warm-read discipline:
        // validation may re-enter the memo through the resolver view).
        let snapshot: CandidateList = {
            let entries = self.entries_lock_diagnosed();
            entries
                .get(&family)
                .map(|slots| slots.snapshot_slot(ModeSlot::Single))?
        };
        let live_generation = ctx.project_type_store().current_project_generation();
        let hit = snapshot.into_iter().find(|entry| {
            entry.validated_at_generation == live_generation && entry.validate(ctx)
        })?;
        // Brief LRU bookkeeping — promote the hit candidate in the slot's
        // recency order (a concurrent invalidation makes this a no-op).
        {
            let mut entries = self.entries_lock_diagnosed();
            if let Some(slots) = entries.get_mut(&family) {
                slots.mark_validated_freshest(ModeSlot::Single, &hit);
            }
        }
        hit.read_set_signature.bubble(ctx);
        match hit.result {
            QueryResult::Value(SemanticQueryValue::FlowReturn(result)) => Some((*result).clone()),
            // Structural invariant: the flow-return authority only ever
            // stores `FlowReturn` payloads in `FlowReturn` family entries.
            other => {
                unreachable!(
                    "FlowReturn family entries store FlowReturn payloads only; found {other:?}"
                )
            }
        }
    }

    /// Publish ONE decided flow-return member at its SCC's batched close,
    /// riding the root's union carrier (read-set signature, self roots,
    /// generation stamp). Fenced exactly like the relation member
    /// publish: only `Complete` results publish, and only through the
    /// store-owned flight.
    pub(crate) fn publish_flow_return_member_fenced(
        &self,
        ctx: Option<&dyn crate::resolver_core::ResolverContext>,
        key: crate::semantic_query::FlowReturnKey,
        result: crate::semantic_query::FlowReturnResult,
        carrier: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        validated_at_generation: u64,
        flight: Option<InlineFlowReturnFlight>,
    ) -> bool {
        debug_assert!(
            flight.as_ref().is_none_or(|flight| {
                matches!(flight.prepared_key(), SemanticQueryKey::FlowReturn(k) if **k == key)
            }),
            "an inline flow-return flight must publish its own exact full key"
        );
        let completed =
            QueryResult::Value(SemanticQueryValue::FlowReturn(Arc::new(result.clone())));
        let family = FamilyKey::FlowReturn { key: Box::new(key) };
        let slot = ModeSlot::Single;
        let admission_seq = self.alloc_candidate_admission_seq();
        let dispatch_dep_signature = self.dep_signature_interner.intern(&empty_signature());
        let entry = MemoEntry {
            result: QueryResult::Value(SemanticQueryValue::FlowReturn(Arc::new(result))),
            read_set_signature: carrier.clone(),
            dispatch_dep_signature: Arc::clone(&dispatch_dep_signature),
            self_root_canonicals: Arc::clone(&self_root_canonicals),
            walker_diagnostics: Arc::from([]),
            satisfied_projection: flow_return_satisfied_projection(),
            validated_at_generation,
            admission_seq,
        };
        let cap = family.candidate_cap();
        let eviction = match ctx {
            Some(ctx) => {
                family::plan_family_slot_eviction(&self.entries, &family, slot, &entry, cap, ctx)
            }
            None => family::EvictionVictim::LruFront,
        };
        let mut entries = self.entries_lock_diagnosed();
        if let Some(flight) = flight.as_ref() {
            if self.force_cold_abort_sweep.load(Ordering::Relaxed) {
                flight.inflight.state.lock().aborted = true;
            }
            if ctx.is_some_and(|ctx| ctx.is_cancelled()) {
                flight.inflight.state.lock().aborted = true;
            }
            if flight.inflight.state.lock().aborted {
                drop(entries);
                record_cold_abort_swept(&self.stats);
                self.abort_inline_flow_return_flight(flight);
                return false;
            }
        }
        let family_was_new = !entries.contains_key(&family);
        let outcome = entries.entry(family.clone()).or_default().publish(
            slot,
            entry,
            &ProjectionPath::empty(),
            cap,
            eviction,
        );
        let populated_slots = outcome.populated;
        for (displaced_slot, displaced_entry) in &outcome.displaced {
            reverse_index::drain_candidate_reverse_index_registrations(
                &self.canonical_to_entries,
                &family,
                *displaced_slot,
                displaced_entry,
            );
        }
        if family_was_new && !populated_slots.is_empty() {
            self.record_family_admission_locked(&mut entries, &family);
        }
        reverse_index::register_reverse_index(
            &self.canonical_to_entries,
            &family,
            &populated_slots,
            &carrier,
            &dispatch_dep_signature,
            admission_seq,
        );
        drop(entries);
        if let Some(flight) = flight {
            {
                let mut state = flight.inflight.state.lock();
                if state.aborted {
                    drop(state);
                    self.abort_inline_flow_return_flight(&flight);
                    return false;
                }
                state.completed = Some(completed);
                state.dep_signature = Some(empty_signature());
                state.graph_carrier = Some(Box::new(carrier));
                state.self_root_canonicals = self_root_canonicals;
                state.walker_diagnostics = Some(Arc::from([]));
                state.cache_suppress = false;
                state.result_is_partial = false;
            }
            flight.inflight.ready.notify_all();
            self.retire_inflight(&flight.prepared, &flight.inflight, true);
        }
        true
    }
}
