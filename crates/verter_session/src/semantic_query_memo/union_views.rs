//! Union member views and the union order of one store.

use std::sync::Arc;

use super::SemanticGraphStore;
use crate::semantic_query::semantic_context::SemanticUnionMembersKey;
use crate::semantic_query::SemanticNodeId;

impl SemanticGraphStore {
    /// The resident member view of `key`'s union, if one was built.
    pub(crate) fn union_view(
        &self,
        key: &SemanticUnionMembersKey,
    ) -> Option<Arc<[SemanticNodeId]>> {
        self.union_views.lock().get(key).map(Arc::clone)
    }

    /// Keep `view` as `key`'s union view; the first view built wins.
    pub(crate) fn keep_union_view(
        &self,
        key: SemanticUnionMembersKey,
        view: &Arc<[SemanticNodeId]>,
    ) -> Arc<[SemanticNodeId]> {
        Arc::clone(
            self.union_views
                .lock()
                .entry(key)
                .or_insert_with(|| Arc::clone(view)),
        )
    }

    /// Whether this store orders union members by descending stable key
    /// (test-only counterfactual; always `false` in production).
    #[inline]
    pub(crate) fn union_order_reversed(&self) -> bool {
        #[cfg(test)]
        {
            self.union_order_reversed_for_tests
                .load(std::sync::atomic::Ordering::Relaxed)
        }
        #[cfg(not(test))]
        {
            false
        }
    }

    /// Reverse this store's union order. Set it before the store builds any
    /// union, on a store no other test shares.
    #[cfg(test)]
    pub(crate) fn reverse_union_order_for_tests(&self) {
        self.union_order_reversed_for_tests
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
