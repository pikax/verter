//! Compile-fail fixture: an extracted composite payload cannot be
//! REPLAYED into the other composite kind. The payloads are kind-bound
//! (`CompositeList<UnionKind>` vs `CompositeList<IntersectionKind>`), so
//! extracting the canonical payload of `string | number` and
//! reconstructing it as `Intersection(payload)` — which would forge
//! `Intersection([string, number])` verbatim, bypassing the canonical
//! algebra that reduces it to `never` — is a TYPE error, not a runtime
//! concern. Same-kind reconstruction merely reproduces the identical
//! node value (the member list is unforgeable), so no NEW derived
//! composite is mintable from a read: the canonical builder stays the
//! sole derived-composite mint even against a payload replay.
//!
//! If the two variants ever shared one payload type again, this fixture
//! would COMPILE and trybuild would turn red.

use verter_session::semantic_query::SemanticNodeData;

fn replay(data: SemanticNodeData) -> SemanticNodeData {
    match data {
        // Extract the (opaque, unforgeable) payload of a Union node...
        SemanticNodeData::Union(payload) => {
            // ...and replay it as an Intersection: kind-bound payloads
            // make this a type mismatch.
            SemanticNodeData::Intersection(payload)
        }
        other => other,
    }
}

fn main() {
    let _ = replay;
}
