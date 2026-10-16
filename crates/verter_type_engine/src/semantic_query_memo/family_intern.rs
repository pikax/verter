//! Interned handles for large family-key payloads.
//!
//! `RelateMemoKey` (144B) and `ResolveCallKey` intern behind a compact
//! handle so [`super::family::FamilyKey`] stays at the 136B bound with
//! margin instead of embedding those payloads by value. Identity is exact
//! equality of the interned value; the handle `Deref`s to the payload.
//!
//! Each handle OWNS its record (one pointer, see
//! [`super::intern_table`]); the tables beside them are weak
//! deduplication indexes only. Memo eviction dropping the last handle
//! frees the payload and forgets its index entry in the same step, so a
//! drained family memo leaves neither records nor index capacity behind.
//! The records retain no child handle of their own kind.

use std::ops::Deref;

use super::intern_table::{intern_domain, Interned};
use crate::semantic_query::{RelateMemoKey, ResolveCallKey};

type IssuingArena = std::sync::Arc<super::arena::NodeArenaCore>;

/// Interned `RelateMemoKey` handle.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct InternedRelateKey(
    Interned<RelateMemoKey>,
    super::node_roots::IdentityNodeRoots,
);

/// Interned `ResolveCallKey` handle.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct InternedResolveCallKey(
    Interned<ResolveCallKey>,
    super::node_roots::IdentityNodeRoots,
);

intern_domain!(RelateMemoKey);
intern_domain!(ResolveCallKey);

impl InternedRelateKey {
    /// Intern `key`; its operands are owned in `arena`, the graph that
    /// issued them, or not at all when `None` (a lookup witness).
    pub(super) fn intern(arena: Option<&IssuingArena>, key: RelateMemoKey) -> Self {
        let mut handle = Self(Interned::new_scoped(key), Default::default());
        let mut nodes = Vec::new();
        super::family::FamilyKey::Relate {
            key: handle.clone(),
        }
        .for_each_node_id(|node| nodes.push(node));
        handle.1 = arena.map_or_else(Default::default, |arena| {
            super::node_roots::IdentityNodeRoots::in_arena(arena, nodes)
        });
        handle
    }
}

impl InternedResolveCallKey {
    /// Intern `key`; its operands are owned in `arena`, the graph that
    /// issued them, or not at all when `None` (a lookup witness).
    pub(super) fn intern(arena: Option<&IssuingArena>, key: ResolveCallKey) -> Self {
        let mut handle = Self(Interned::new_scoped(key), Default::default());
        let mut nodes = Vec::new();
        super::family::FamilyKey::ResolveCall {
            key: handle.clone(),
        }
        .for_each_node_id(|node| nodes.push(node));
        handle.1 = arena.map_or_else(Default::default, |arena| {
            super::node_roots::IdentityNodeRoots::in_arena(arena, nodes)
        });
        handle
    }
}

impl Deref for InternedRelateKey {
    type Target = RelateMemoKey;
    fn deref(&self) -> &RelateMemoKey {
        &self.0
    }
}
impl std::fmt::Debug for InternedRelateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("InternedRelateKey").field(&*self.0).finish()
    }
}

impl Deref for InternedResolveCallKey {
    type Target = ResolveCallKey;
    fn deref(&self) -> &ResolveCallKey {
        &self.0
    }
}
impl std::fmt::Debug for InternedResolveCallKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("InternedResolveCallKey")
            .field(&*self.0)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::{RelateMemoKey, RelationContext, SemanticNodeId};
    use crate::semantic_query_memo::intern_table::InternDomain;

    fn unique_key(tag: u64) -> RelateMemoKey {
        RelateMemoKey::assignable(
            SemanticNodeId(0xF1A1_0000_0000_0000 | tag),
            SemanticNodeId(0xF1A1_0000_0000_0001 | tag << 8),
            RelationContext::default(),
        )
    }

    #[test]
    fn interned_relate_keys_reclaim_when_handles_drop() {
        let key = unique_key(0x51);
        let handle = InternedRelateKey::intern(None, key.clone());
        assert!(
            RelateMemoKey::index().get(&key).is_some(),
            "a live handle keeps the payload"
        );
        assert!(InternedRelateKey::intern(None, key.clone()) == handle);
        drop(handle);
        assert!(
            RelateMemoKey::index().get(&key).is_none(),
            "memo eviction dropping the last handle frees the payload and its index entry"
        );
        let again = InternedRelateKey::intern(None, key);
        assert_eq!(again.source, unique_key(0x51).source);
    }
}
