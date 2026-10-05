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
//! payload.
//!
//! **Bounded and accounted.** Every union ever read in an order-sensitive
//! position would otherwise keep its view for the life of the store — every
//! revision of an edited union among them, long after nothing reads it. The
//! table therefore keeps at most [`UNION_VIEW_CAP`] views, evicting the
//! oldest-admitted first, and each kept view holds a `Retained` charge on the
//! store's retention account for its bytes, released when the view leaves the
//! table. A view the account refuses is returned uncached.
//!
//! A holder that retires node payloads keeps the table consistent with them:
//! it drops every entry whose `SemanticUnionMembersKey::union` it retires (a
//! retired id must not keep answering its members), admits no view for a
//! retired id, and drops an id's entries before that id could ever name a
//! different node.

use std::collections::VecDeque;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::SemanticGraphStore;
use crate::semantic_query::semantic_context::SemanticUnionMembersKey;
use crate::semantic_query::SemanticNodeId;
use verter_session_query::retention::{ChargeClass, RetentionAdmission, RetentionCharge};

/// The most union views one store keeps: the bound its semantic memo keeps
/// on its families.
pub(super) const UNION_VIEW_CAP: usize = crate::bounded_query_retention::DEFAULT_BUDGET_CAP;

/// The kept views, each with its retention charge, and their keys in
/// admission order.
#[derive(Default)]
pub(super) struct UnionViews {
    views: FxHashMap<SemanticUnionMembersKey, (Arc<[SemanticNodeId]>, RetentionCharge)>,
    admitted: VecDeque<SemanticUnionMembersKey>,
}

impl UnionViews {
    /// Views kept (retention observability).
    pub(super) fn len(&self) -> usize {
        self.views.len()
    }

    /// Take every kept view `retired` selects out of the table, with its
    /// charge and its place in the admission order. The caller drops what
    /// is returned after releasing the table's lock, as an eviction does
    /// (the account a charge returns to takes its own lock).
    pub(super) fn take_where(
        &mut self,
        mut retired: impl FnMut(&SemanticUnionMembersKey, &[SemanticNodeId]) -> bool,
    ) -> Vec<(Arc<[SemanticNodeId]>, RetentionCharge)> {
        let keys: Vec<SemanticUnionMembersKey> = self
            .views
            .iter()
            .filter(|(key, (view, _))| retired(key, view))
            .map(|(key, _)| *key)
            .collect();
        if keys.is_empty() {
            return Vec::new();
        }
        let taken = keys
            .iter()
            .filter_map(|key| self.views.remove(key))
            .collect();
        let views = &self.views;
        self.admitted.retain(|key| views.contains_key(key));
        taken
    }
}

impl std::fmt::Debug for UnionViews {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnionViews")
            .field("views", &self.views.len())
            .finish()
    }
}

/// The bytes a kept view retains: its members and its entry.
fn retained_view_bytes(view: &[SemanticNodeId]) -> usize {
    std::mem::size_of_val(view)
        + std::mem::size_of::<SemanticUnionMembersKey>()
        + std::mem::size_of::<(Arc<[SemanticNodeId]>, RetentionCharge)>()
}

impl SemanticGraphStore {
    /// The resident member view of `key`'s union, if one was built.
    pub(crate) fn union_view(
        &self,
        key: &SemanticUnionMembersKey,
    ) -> Option<Arc<[SemanticNodeId]>> {
        self.union_views
            .lock()
            .views
            .get(key)
            .map(|(view, _)| Arc::clone(view))
    }

    /// Keep `view` as `key`'s union view; the first view built wins. The
    /// view is charged to the store's retention account and, past
    /// [`UNION_VIEW_CAP`], the oldest-admitted view leaves the table; a view
    /// the account refuses is returned without being kept. A view of a union
    /// a document close already released is served but not kept either: a
    /// late reader of a released id builds the placeholder's one-element
    /// view, and keeping it would leave a residue no later release can find
    /// (the id is never re-minted).
    pub(crate) fn keep_union_view(
        &self,
        key: SemanticUnionMembersKey,
        view: &Arc<[SemanticNodeId]>,
    ) -> Arc<[SemanticNodeId]> {
        if !self.arena.is_live(key.union()) {
            return Arc::clone(view);
        }
        if let Some(kept) = self.union_view(&key) {
            return kept;
        }
        // Reserve outside the table's lock: the account takes its own.
        let charge = match self
            .retention_account()
            .reserve(ChargeClass::Retained, retained_view_bytes(view))
        {
            RetentionAdmission::Admitted(charge) => charge,
            RetentionAdmission::Refused(_) => return Arc::clone(view),
        };
        let mut table = self.union_views.lock();
        if let Some((kept, _)) = table.views.get(&key) {
            // Another reader kept the same view first; this charge drops.
            return Arc::clone(kept);
        }
        table.views.insert(key, (Arc::clone(view), charge));
        table.admitted.push_back(key);
        let evicted = if table.admitted.len() > UNION_VIEW_CAP {
            table
                .admitted
                .pop_front()
                .and_then(|oldest| table.views.remove(&oldest))
        } else {
            None
        };
        drop(table);
        // The evicted view and its charge are released outside the lock.
        drop(evicted);
        Arc::clone(view)
    }

    /// Test-only: the number of kept union views.
    #[cfg(test)]
    pub(crate) fn union_view_count_for_tests(&self) -> usize {
        self.union_views.lock().views.len()
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::SemanticGraphStore;
    use crate::semantic_query::composite::{CompositeList, UnionKind};
    use crate::semantic_query::semantic_context::SemanticContext;
    use crate::semantic_query::stable_key::semantic_union_members;
    use crate::semantic_query::{LiteralValue, SemanticNodeData, SemanticNodeId};
    use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount};

    /// The most views the store may keep: the bound its memo keeps on its
    /// families.
    const BOUND: usize = crate::bounded_query_retention::DEFAULT_BUDGET_CAP;

    /// An edited union (`type U = "fixed" | "revision-N"`) is a new union
    /// every revision, and each revision's ordered member view is built once
    /// it is read. The store keeps a bounded number of those views, charged
    /// to its retention account while kept and released with the store; a
    /// view that left the table rebuilds identically.
    #[test]
    fn union_views_stay_bounded_and_charged_across_revisions() {
        let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
        let store = SemanticGraphStore::with_account(
            verter_session_query::retention::StoreAccount::new(Arc::clone(&account)),
        );
        let ctx = SemanticContext::production();
        let literal =
            |text: String| store.intern_node(SemanticNodeData::Literal(LiteralValue::String(text)));
        let fixed = literal("fixed".to_owned());
        let mut first: Option<(SemanticNodeId, Arc<[SemanticNodeId]>)> = None;
        for revision in 0..BOUND + 64 {
            let revised = literal(format!("revision-{revision}"));
            let union = store.intern_node(SemanticNodeData::Union(
                CompositeList::<UnionKind>::authored_shell(Arc::from([fixed, revised])),
            ));
            let view = semantic_union_members(&store, union, &ctx);
            first.get_or_insert((union, view));
        }
        let kept = store.union_view_count_for_tests();
        assert!(kept > 0, "premise: the revisions kept views");
        assert_eq!(
            store.union_view_count(),
            kept,
            "the public count reads the same table"
        );
        assert!(
            kept <= BOUND,
            "{kept} union views kept after {} revisions, more than {BOUND}",
            BOUND + 64
        );
        assert!(
            account.snapshot().retained_bytes > 0,
            "kept union views are charged to the store's retention account"
        );
        let (union, view) = first.expect("a first revision");
        assert_eq!(
            semantic_union_members(&store, union, &ctx),
            view,
            "a view that left the table rebuilds identically"
        );
        drop(store);
        assert_eq!(
            account.snapshot().retained_bytes,
            0,
            "every union view's charge is released with its store"
        );
    }
}
