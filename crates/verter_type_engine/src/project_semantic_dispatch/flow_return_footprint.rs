//! Allocation footprints of the independent flow-return proofs.

#[cfg(test)]
use std::sync::Arc;

use verter_session_query::retention::allocation::AliasSet;

use super::{ExactMemberValue, ExactReturnKeyDomain};
use crate::semantic_query::allocation_footprint::{copy_inline, OwnedHeap};
#[cfg(test)]
use crate::semantic_query::{FlowReturnKey, SemanticNodeId};
#[cfg(test)]
use crate::semantic_query_memo::ObservedGraphSelfRoot;

impl OwnedHeap for ExactMemberValue {
    /// The proven key and the self-root list with its canonicals. The
    /// member node is the arena's.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Self { key, node, roots } = self;
        key.owned_heap_bytes(aliases) + copy_inline(node) + roots.owned_heap_bytes(aliases)
    }
}

impl OwnedHeap for ExactReturnKeyDomain {
    /// The proven key, the key-name list with its text, and the self-root
    /// list with its canonicals.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Self { key, keys, roots } = self;
        key.owned_heap_bytes(aliases)
            + keys.owned_heap_bytes(aliases)
            + roots.owned_heap_bytes(aliases)
    }
}

#[cfg(test)]
impl ExactMemberValue {
    /// A proof with the given self-roots, for footprint tests.
    pub(crate) fn for_footprint_tests(
        key: FlowReturnKey,
        node: SemanticNodeId,
        roots: Arc<[ObservedGraphSelfRoot]>,
    ) -> Self {
        Self { key, node, roots }
    }
}

#[cfg(test)]
impl ExactReturnKeyDomain {
    /// A key domain with the given keys and self-roots, for footprint tests.
    pub(crate) fn for_footprint_tests(
        key: FlowReturnKey,
        keys: Arc<[Arc<str>]>,
        roots: Arc<[ObservedGraphSelfRoot]>,
    ) -> Self {
        Self { key, keys, roots }
    }
}
