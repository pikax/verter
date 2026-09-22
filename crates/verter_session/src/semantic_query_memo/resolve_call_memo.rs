//! ResolveCall payload storage over the modeless family memo slot.

use super::*;

pub(super) fn resolve_call_satisfied_projection() -> MaterializedSet {
    MaterializedSet::single(MaterializedPoint::new(family::point_for_slot(
        ModeSlot::Single,
        &ProjectionPath::empty(),
    )))
}

impl SemanticGraphStore {
    /// Claim the ordinary family flight for a call-resolution member
    /// computed inline. `None` means another cold owner already owns this
    /// exact full key.
    pub(crate) fn begin_inline_resolve_call_flight(
        &self,
        key: &crate::semantic_query::ResolveCallKey,
    ) -> Option<InlineMemberFlight> {
        self.begin_inline_member_flight(SemanticQueryKey::ResolveCall(Box::new(key.clone())))
    }

    pub(crate) fn get_resolve_call_result(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        key: &crate::semantic_query::ResolveCallKey,
    ) -> Option<crate::semantic_query::ResolvedCallResult> {
        let family = FamilyKey::ResolveCall {
            key: super::family_intern::InternedResolveCallKey::intern(key.clone()),
        };
        // A call-resolution entry always materialises the modeless
        // identity point, so the §3.4 gate is the modeless identity
        // point every entry records — only carrier validation can block.
        let requested = MaterializedPoint::new(family::point_for_slot(
            ModeSlot::Single,
            &ProjectionPath::empty(),
        ));
        // Miss-neutral probe: a miss falls through to the owning
        // cooperative dispatch, which records the single miss (see
        // `get_validated_value_impl`'s `record_miss` contract).
        let hit = self
            .get_validated_value_impl(&family, ModeSlot::Single, &requested, ctx, None, false)?
            .value;
        match hit {
            QueryResult::Value(SemanticQueryValue::ResolveCall(result)) => {
                Some(result.as_ref().clone())
            }
            other => unreachable!(
                "ResolveCall family entries store ResolveCall payloads only; found {other:?}"
            ),
        }
    }
}
