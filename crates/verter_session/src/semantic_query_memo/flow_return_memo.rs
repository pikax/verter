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
//!
//! Writes land through the batched SCC member publish in
//! [`super::scc_publish`] — the one store-owned admission path both
//! domains ride.

use super::*;

/// Store-owned admission token for a flow-return member computed inline
/// by another obligation's transaction. Registering the token in the
/// ordinary flow-return-family flight table lets a concurrent top-level
/// request join the inline compute instead of starting duplicate cold
/// work.
#[derive(Clone)]
pub(crate) struct InlineFlowReturnFlight {
    pub(super) prepared: PreparedKeyHandle,
    pub(super) inflight: Arc<InflightEntry>,
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
    /// the TWO-GATE hit — `cached_satisfies` over the entry's RECORDED
    /// materialised point against the key's OWN demand point (never the
    /// nominal `Single` preset), AND carrier validation with the
    /// live-generation gate. An entry carries the `FlowBody` fact rail
    /// plus its consumed subquery facts and self roots; `validate(ctx)`
    /// revalidates that whole signature against the caller's live view,
    /// so a body edit or a torn fact set hard-misses. Warm validity
    /// consults the `FlowBody` rooting + the unioned consumed facts
    /// ONLY — no slice hash or selected-ID is re-derived or consulted
    /// here (the sole-rail invariant; slice identity is structurally
    /// unrepresentable in the fact rail).
    pub(crate) fn get_flow_return_result(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        key: &crate::semantic_query::FlowReturnKey,
    ) -> Option<crate::semantic_query::FlowReturnResult> {
        let family = FamilyKey::FlowReturn {
            key: Box::new(key.clone()),
        };
        let requested = MaterializedPoint::new(key.demand.point.clone());
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
            cached_satisfies(&entry.satisfied_projection, &requested)
                && entry.validated_at_generation == live_generation
                && entry.validate(ctx)
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
            QueryResult::Value(SemanticQueryValue::FlowReturn(result)) => {
                debug_assert!(
                    result.degradation().is_none(),
                    "the FlowReturn memo never stores a degraded success (ReturnOnly by contract)"
                );
                Some((*result).clone())
            }
            // Structural invariant: the flow-return authority only ever
            // stores `FlowReturn` payloads in `FlowReturn` family entries.
            other => {
                unreachable!(
                    "FlowReturn family entries store FlowReturn payloads only; found {other:?}"
                )
            }
        }
    }
}
