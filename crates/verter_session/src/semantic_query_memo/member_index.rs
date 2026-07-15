//! Member-ordinal index sidecar for LARGE interned `Object` surfaces.
//!
//! The `ProjectPath` walker resolves one member per hop by name. A linear
//! scan is optimal for the small surfaces that dominate the corpus, but a
//! wide surface (a TanStack `Table` options object, a theme registry) pays
//! O(members) string compares per hop, re-paid on every path that crosses
//! it. This sidecar memoises a name → FIRST-occurrence member ordinal map
//! per interned node so repeated hops into the same wide surface resolve
//! in O(1).
//!
//! **Identity-excluded.** The index lives entirely OUTSIDE the node
//! payload: `SemanticNodeData` / `SurfaceView` identity, `Eq`, `Hash`, and
//! interning behaviour are untouched. The sidecar keys on
//! [`SemanticNodeId`] — valid forever within a store because the node
//! arena is append-only (ids are never reused) and payloads are immutable,
//! so entries never go stale and no invalidation hook is required (unlike
//! the hash-cons memos, whose values depend on cross-file walks).
//!
//! **Collision-safe hashing.** Member names are authored strings, so the
//! inner map uses the std `HashMap` default hasher (SipHash `RandomState`),
//! NOT `FxHash`. The outer `DashMap` likewise defaults to `RandomState`.
//!
//! **Bounded.** Mirrors the hash-cons memo retention contract: each build
//! pushes its node id into a FIFO sidecar deque; past
//! [`MEMBER_ORDINAL_INDEX_RETENTION_CAP`] the oldest entry is evicted.
//! Reads stay lock-free; only builds pay the FIFO lock.

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::mapref::entry::Entry;

use super::SemanticGraphStore;
use crate::semantic_query::{SemanticNodeId, SurfaceView};

/// Name → FIRST-occurrence member ordinal for one interned `Object`
/// surface. First occurrence matches the walker's linear
/// `members.iter().find(..)` semantics exactly — a duplicate name maps to
/// its earliest ordinal, and any post-lookup filtering (e.g. the public-
/// visibility gate) applies to that same member the scan would have found.
pub type MemberOrdinalIndex = HashMap<Arc<str>, u32>;

/// FIFO retention cap for the member-ordinal sidecar. Sized for the
/// working set of wide surfaces a session actually path-hops (hundreds
/// per large workspace) plus generous headroom; one entry costs roughly
/// `members × (Arc bump + 16 bytes)`.
pub(super) const MEMBER_ORDINAL_INDEX_RETENTION_CAP: usize = 16_384;

/// Linear-scan crossover: surfaces with at most this many members answer
/// name lookups faster by scanning than through the sidecar (hash + lock
/// overhead dominates below it). Callers consult the sidecar only ABOVE
/// this count.
pub const MEMBER_ORDINAL_INDEX_LINEAR_SCAN_MAX: usize = 16;

impl SemanticGraphStore {
    /// Return the lazily-built member-ordinal index for the interned
    /// `Object` node `id`, whose payload surface the caller already holds
    /// as `view`. Builds at most once per node id (first-writer-wins;
    /// concurrent builders produce identical maps because the payload is
    /// immutable).
    ///
    /// The caller is responsible for passing the `SurfaceView` that
    /// belongs to `id` — the sidecar trusts the pairing, mirroring how
    /// every walker arm already reads the view through
    /// [`Self::node_data`].
    #[must_use]
    pub fn member_ordinal_index(
        &self,
        id: SemanticNodeId,
        view: &SurfaceView,
    ) -> Arc<MemberOrdinalIndex> {
        if let Some(existing) = self.member_ordinal_index_memo.get(&id) {
            return Arc::clone(existing.value());
        }
        let mut index = MemberOrdinalIndex::with_capacity(view.members.len());
        for (ordinal, member) in view.members.iter().enumerate() {
            // First occurrence wins — `or_insert` keeps the earliest
            // ordinal for a duplicated name, matching linear-scan `find`.
            index
                .entry(Arc::clone(&member.name))
                .or_insert(ordinal as u32);
        }
        let built = Arc::new(index);
        match self.member_ordinal_index_memo.entry(id) {
            Entry::Occupied(existing) => Arc::clone(existing.get()),
            Entry::Vacant(slot) => {
                slot.insert(Arc::clone(&built));
                let mut fifo = self.member_ordinal_index_fifo.lock();
                fifo.push_back(id);
                while fifo.len() > MEMBER_ORDINAL_INDEX_RETENTION_CAP {
                    if let Some(victim) = fifo.pop_front() {
                        // Drop the FIFO lock BEFORE touching the DashMap
                        // (mirrors the hash-cons eviction ordering) so a
                        // concurrent builder is never blocked behind a
                        // shard removal; re-acquire to continue evicting.
                        drop(fifo);
                        self.member_ordinal_index_memo.remove(&victim);
                        fifo = self.member_ordinal_index_fifo.lock();
                    } else {
                        break;
                    }
                }
                built
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::{IndexSignature, PrimitiveKind, SemanticNodeData, SurfaceMember};

    fn member(name: &str, value: SemanticNodeId) -> SurfaceMember {
        SurfaceMember {
            visibility: verter_type_expr::MemberVisibility::Public,
            name: Arc::from(name),
            value,
            optional: false,
            readonly: false,
            is_method: false,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            spans: Default::default(),
            declaration_origin: None,
        }
    }

    fn object_view(members: Vec<SurfaceMember>) -> SurfaceView {
        SurfaceView {
            members: Arc::from(members.into_boxed_slice()),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }
    }

    /// Ordinals match source order, and lookups work through `&str`
    /// borrows (no needle allocation on the read side).
    #[test]
    fn member_ordinal_index_maps_names_to_source_ordinals() {
        let store = SemanticGraphStore::new();
        let value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let view = object_view(vec![
            member("alpha", value),
            member("beta", value),
            member("gamma", value),
        ]);
        let id = store.intern_node(SemanticNodeData::Object(view.clone()));

        let index = store.member_ordinal_index(id, &view);
        assert_eq!(index.get("alpha"), Some(&0));
        assert_eq!(index.get("beta"), Some(&1));
        assert_eq!(index.get("gamma"), Some(&2));
        assert_eq!(index.get("missing"), None);
    }

    /// A duplicated member name maps to its FIRST ordinal — the exact
    /// member a linear `find` would return — so post-lookup gates (e.g.
    /// the public-visibility filter) observe the same member the scan
    /// path observes.
    #[test]
    fn member_ordinal_index_first_occurrence_wins_for_duplicate_names() {
        let store = SemanticGraphStore::new();
        let value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let view = object_view(vec![
            member("dup", value),
            member("solo", value),
            member("dup", value),
        ]);
        let id = store.intern_node(SemanticNodeData::Object(view.clone()));

        let index = store.member_ordinal_index(id, &view);
        assert_eq!(
            index.get("dup"),
            Some(&0),
            "duplicate name must map to its FIRST ordinal (find semantics)"
        );
        assert_eq!(index.get("solo"), Some(&1));
    }

    /// The index is built once per node id and shared: a second read
    /// returns the SAME Arc (pointer-equal), never a rebuild.
    #[test]
    fn member_ordinal_index_is_built_once_and_shared() {
        let store = SemanticGraphStore::new();
        let value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let view = object_view(vec![member("alpha", value)]);
        let id = store.intern_node(SemanticNodeData::Object(view.clone()));

        let first = store.member_ordinal_index(id, &view);
        let second = store.member_ordinal_index(id, &view);
        assert!(
            Arc::ptr_eq(&first, &second),
            "second read must return the memoised index, not a rebuild"
        );
    }

    /// FIFO retention: entries past the cap evict oldest-first; an
    /// evicted entry rebuilds on demand (fresh Arc identity) while a
    /// retained entry keeps its memoised Arc.
    #[test]
    fn member_ordinal_index_fifo_evicts_oldest_past_cap() {
        let store = SemanticGraphStore::new();
        let value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

        // Distinct single-member views so each interns a distinct node.
        let views: Vec<(SemanticNodeId, SurfaceView)> = (0..3)
            .map(|i| {
                let view = object_view(vec![member(&format!("m{i}"), value)]);
                let id = store.intern_node(SemanticNodeData::Object(view.clone()));
                (id, view)
            })
            .collect();

        let first_arc = store.member_ordinal_index(views[0].0, &views[0].1);
        for (id, view) in &views[1..] {
            let _ = store.member_ordinal_index(*id, view);
        }
        // Simulate cap pressure by evicting through the same FIFO rail the
        // production path uses: drain until the first id is gone.
        {
            let mut fifo = store.member_ordinal_index_fifo.lock();
            while let Some(victim) = fifo.pop_front() {
                store.member_ordinal_index_memo.remove(&victim);
                if victim == views[0].0 {
                    break;
                }
            }
        }
        let rebuilt = store.member_ordinal_index(views[0].0, &views[0].1);
        assert!(
            !Arc::ptr_eq(&first_arc, &rebuilt),
            "an evicted entry rebuilds on demand with a fresh Arc"
        );
        assert_eq!(rebuilt.get("m0"), Some(&0), "rebuild is content-identical");
    }
}
