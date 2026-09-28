//! FlowReturn storage — the payload read/write path over the family
//! memo's `FlowReturn` family.
//!
//! Storage is the family memo's [`FamilyKey::FlowReturn`] family in the
//! [`ModeSlot::Single`] slot. The stored value is the
//! [`SemanticQueryValue::FlowReturn`] payload — COMPLETE whole-function
//! results ONLY: a typed `FlowReturnFailure` (`Unsupported` / `Missing` /
//! `Budget` / `EmptyCycle` / `Unresolved` / `CallResolution`) has no
//! value-domain form and is never admitted anywhere (memo / fact /
//! reverse index). Warm reads
//! validate the self-version-rooted carrier strictly. Project-shape
//! invalidation rides `FactVersionRef::ProjectGeneration` on that
//! carrier. Retention rides the family rails (cap 8,
//! invalid-first / LRU eviction, reverse-index drains).
//!
//! Writes land through the batched SCC member publish in
//! [`super::scc_publish`] — the one store-owned admission path every
//! deferred domain rides.

use super::*;

impl SemanticGraphStore {
    /// Claim the ordinary family flight for a flow-return member computed
    /// inline. `None` means another cold owner already owns this exact
    /// full key.
    pub(crate) fn begin_inline_flow_return_flight(
        &self,
        key: &crate::semantic_query::FlowReturnKey,
    ) -> Option<InlineMemberFlight> {
        self.begin_inline_member_flight(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
    }

    /// Whether the `FlowReturn` family holds a candidate for `key` the warm
    /// read would serve — the same point, epoch and fact-rail gates — as a
    /// peek that bubbles no read, touches no LRU order and counts neither a
    /// hit nor a miss. The flow-return callee schedule reads it to leave a
    /// callee whose answer is warm to that callee's own demand. A stale
    /// candidate does not count: after an edit, the callees above it are
    /// re-evaluated by the schedule, bottom-up, rather than one nested
    /// demand per invalidated level.
    pub(crate) fn has_serving_flow_return_candidate(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        key: &crate::semantic_query::FlowReturnKey,
    ) -> bool {
        let family = FamilyKey::FlowReturn {
            key: Box::new(key.clone()),
        };
        let requested = MaterializedPoint::new(key.demand.point.clone());
        // Validated OUTSIDE the lock, as the warm read validates.
        let snapshot = self
            .entries_lock_diagnosed()
            .get(&family)
            .map(|slots| slots.snapshot_slot(ModeSlot::Single));
        snapshot.is_some_and(|list| {
            list.iter()
                .any(|entry| self.warm_candidate_serves(entry, &requested, ctx))
        })
    }

    /// The strict warm read of the `FlowReturn` family (design §3.4):
    /// the TWO-GATE hit — `cached_satisfies` over the entry's RECORDED
    /// materialised point against the key's OWN demand point (never the
    /// nominal `Single` preset), AND carrier validation. An entry carries the `FlowBody` fact rail
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
        // The §3.4 point gate is LIVE here: a flow-return family's
        // candidates record different materialised points, so a warm hit
        // must cover the caller's OWN demand point.
        let requested = MaterializedPoint::new(key.demand.point.clone());
        // Miss-neutral probe: a miss falls through to the owning
        // cooperative dispatch, which records the single miss (see
        // `get_validated_value_impl`'s `record_miss` contract).
        let hit = self
            .get_validated_value_impl(&family, ModeSlot::Single, &requested, ctx, None, false)?
            .value;
        match hit {
            QueryResult::Value(SemanticQueryValue::FlowReturn(result)) => {
                verter_debug_assert!(
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
