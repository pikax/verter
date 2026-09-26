//! Union member views and the union order of one store.
//!
//! **Ownership and lifetime of `SemanticGraphStore::union_views`.** The table
//! belongs to the store whose arena the union's id indexes and dies with it;
//! it is never shared between stores, because node ids are arena-local. An
//! entry is keyed by `SemanticUnionMembersKey` (the union's id, the order
//! policy and the order domain) and holds that union's members in
//! `VerterStableV1` order, as ids of the same store.
//!
//! A view is a pure, deterministic function of the union's payload: the first
//! view built wins and is never mutated, and dropping any entry is always
//! safe, because the next read rebuilds the identical view. The only reader is
//! `semantic_union_members`, which consults the table BEFORE the union's
//! payload. The table grows with the unions read in order-sensitive positions
//! and is not bounded here; any bound or eviction policy is admissible.
//!
//! A holder that retires node payloads keeps the table consistent with them:
//! it drops every entry whose `SemanticUnionMembersKey::union` it retires (a
//! retired id must not keep answering its members), admits no view for a
//! retired id, and drops an id's entries before that id could ever name a
//! different node.

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
