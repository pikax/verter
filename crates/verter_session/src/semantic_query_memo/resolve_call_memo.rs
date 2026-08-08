//! ResolveCall payload storage over the modeless family memo slot.

use super::*;

pub(super) fn resolve_call_satisfied_projection() -> MaterializedSet {
    MaterializedSet::single(MaterializedPoint::new(family::point_for_slot(
        ModeSlot::Single,
        &ProjectionPath::empty(),
    )))
}

#[derive(Clone)]
pub(crate) struct InlineResolveCallFlight {
    pub(super) prepared: PreparedKeyHandle,
    pub(super) inflight: Arc<InflightEntry>,
    _owner_registration: Option<wait_cycle::ExecutionOwnerRegistration>,
}

impl std::fmt::Debug for InlineResolveCallFlight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InlineResolveCallFlight")
            .field("key", self.prepared.key())
            .finish_non_exhaustive()
    }
}

impl InlineResolveCallFlight {}

impl SemanticGraphStore {
    pub(crate) fn begin_inline_resolve_call_flight(
        &self,
        key: &crate::semantic_query::ResolveCallKey,
    ) -> Option<InlineResolveCallFlight> {
        let prepared =
            PreparedKeyHandle::prepare(SemanticQueryKey::ResolveCall(Box::new(key.clone())));
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
        Some(InlineResolveCallFlight {
            prepared,
            inflight,
            _owner_registration: owner_registration,
        })
    }

    pub(crate) fn abort_inline_resolve_call_flight(&self, flight: &InlineResolveCallFlight) {
        {
            let mut state = flight.inflight.state.lock();
            state.aborted = true;
            if state.completed.is_none() {
                state.completed = Some(QueryResult::Error(QueryError::Other(Arc::from(
                    "inline resolve-call flight abandoned",
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

    pub(crate) fn get_resolve_call_result(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        key: &crate::semantic_query::ResolveCallKey,
    ) -> Option<crate::semantic_query::ResolvedCallResult> {
        let family = FamilyKey::ResolveCall {
            key: Box::new(key.clone()),
        };
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
        {
            let mut entries = self.entries_lock_diagnosed();
            if let Some(slots) = entries.get_mut(&family) {
                slots.mark_validated_freshest(ModeSlot::Single, &hit);
            }
        }
        hit.read_set_signature.bubble(ctx);
        match hit.result {
            QueryResult::Value(SemanticQueryValue::ResolveCall(result)) => {
                Some(result.as_ref().clone())
            }
            other => unreachable!(
                "ResolveCall family entries store ResolveCall payloads only; found {other:?}"
            ),
        }
    }
}
