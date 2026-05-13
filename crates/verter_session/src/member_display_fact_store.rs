//! `MemberDisplayFactStore` — lazy member-body DISPLAY
//! fingerprints, keyed on `content_hash`.
//!
//! The parallel of [`crate::member_semantic_fact_store`]. The
//! lazy-member-body producer emits TWO fingerprints per `Member`
//! fact:
//!
//! - The SEMANTIC fingerprint (R16) lives in
//!   `MemberSemanticFactStore`, keyed on `parse_stable_hash`. A
//!   cosmetic edit keeps the same key — the cached entry survives.
//! - The DISPLAY fingerprint (R13) lives HERE, keyed on
//!   `content_hash`. A cosmetic edit re-keys the entry — the
//!   recomputed display fact may equal the original (no JSDoc
//!   change) or differ (JSDoc edit). Either way, semantic-lane
//!   consumers are not invalidated by display-lane changes.
//!
//! The split is what enforces R13's "cosmetic edits invalidate
//! display-bearing materialisations only" promise.

use std::sync::Arc;

use dashmap::DashMap;
use verter_semantic::analysis::Hash16;
use verter_semantic::facts::{Fact, FactHash, FactKey, SymbolSpace};

use crate::file_artifact_store::InternedName;

/// Key used by [`MemberDisplayFactStore`].
///
/// `content_hash` is the source-byte identity of the file at the
/// time the display fact was computed. Any cosmetic edit (whitespace,
/// comment, JSDoc) shifts `content_hash` → the store re-keys the
/// entry. Producers compute a fresh display fingerprint and admit
/// it; the recomputed value MAY equal the original (e.g. a
/// whitespace-only edit with no JSDoc).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemberDisplayFactKey {
    pub canonical: Arc<str>,
    pub content_hash: Hash16,
    pub parse_env_hash: Hash16,
    pub exporter: InternedName,
    pub member_name: InternedName,
    pub symbol_space: SymbolSpace,
}

/// Lazy member-body DISPLAY fingerprint store.
///
/// Read / write contract matches
/// [`crate::member_semantic_fact_store::MemberSemanticFactStore`]:
/// `DashMap` shards on the key; cold miss is `None`; admission
/// runs through the producer singleflight.
#[derive(Debug, Default)]
pub struct MemberDisplayFactStore {
    entries: DashMap<MemberDisplayFactKey, Arc<Fact>>,
}

impl MemberDisplayFactStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Lookup a member-body display fact by full key.
    #[must_use]
    pub fn get(&self, key: &MemberDisplayFactKey) -> Option<Arc<Fact>> {
        self.entries.get(key).map(|v| Arc::clone(&*v))
    }

    /// Admit a freshly-computed `Member` display fact.
    pub fn insert(&self, key: MemberDisplayFactKey, fact: Arc<Fact>) {
        self.entries.entry(key).or_insert(fact);
    }

    /// Number of cached entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no entries are cached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop every cached entry.
    pub fn clear(&self) {
        self.entries.clear();
    }

    /// Drop every cached entry whose `canonical` matches the supplied
    /// canonical id. Used by the project-global eviction cascade when
    /// a canonical file's content changes (per the
    /// `evict_canonical_inventory.json` contract).
    pub fn invalidate_canonical(&self, canonical: &str) {
        self.entries
            .retain(|key, _| key.canonical.as_ref() != canonical);
    }
}

/// Build a `FactKey::Member` discriminator from a
/// [`MemberDisplayFactKey`].
#[must_use]
pub fn member_display_fact_key(k: &MemberDisplayFactKey) -> FactKey {
    FactKey::Member {
        exporter: k.exporter.clone(),
        name: k.member_name.clone(),
        space: k.symbol_space,
    }
}

/// Construct a `Member` display fact from its key and a freshly-
/// computed display fingerprint.
#[must_use]
pub fn make_member_display_fact(
    key: &MemberDisplayFactKey,
    semantic_hash: FactHash,
    display_hash: FactHash,
) -> Fact {
    Fact {
        key: member_display_fact_key(key),
        semantic_hash,
        display_hash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(
        canonical: &str,
        content: u8,
        env: u8,
        exporter: &str,
        name: &str,
    ) -> MemberDisplayFactKey {
        let mut content_arr = [0u8; 16];
        content_arr[0] = content;
        let mut env_arr = [0u8; 16];
        env_arr[0] = env;
        MemberDisplayFactKey {
            canonical: Arc::from(canonical),
            content_hash: content_arr,
            parse_env_hash: env_arr,
            exporter: InternedName::from(exporter),
            member_name: InternedName::from(name),
            symbol_space: SymbolSpace::Type,
        }
    }

    fn dummy_hash(b: u8) -> FactHash {
        let mut h = [0u8; 16];
        h[0] = b;
        h
    }

    #[test]
    fn cold_miss_returns_none() {
        let store = MemberDisplayFactStore::new();
        let k = key("/a.ts", 1, 1, "Foo", "a");
        assert!(store.get(&k).is_none());
    }

    #[test]
    fn insert_and_get_round_trip() {
        let store = MemberDisplayFactStore::new();
        let k = key("/a.ts", 1, 1, "Foo", "a");
        let fact = Arc::new(make_member_display_fact(&k, dummy_hash(1), dummy_hash(2)));
        store.insert(k.clone(), Arc::clone(&fact));
        let got = store.get(&k).expect("warm hit");
        assert_eq!(got.semantic_hash[0], 1);
        assert_eq!(got.display_hash[0], 2);
    }

    #[test]
    fn cosmetic_edit_re_keys_entry() {
        // R13: cosmetic edits change `content_hash` → different
        // store key. Two entries for the same member coexist under
        // different content hashes.
        let store = MemberDisplayFactStore::new();
        let k_v1 = key("/a.ts", 1, 1, "Foo", "a");
        let k_v2 = key("/a.ts", 2, 1, "Foo", "a");
        store.insert(
            k_v1.clone(),
            Arc::new(make_member_display_fact(
                &k_v1,
                dummy_hash(1),
                dummy_hash(10),
            )),
        );
        store.insert(
            k_v2.clone(),
            Arc::new(make_member_display_fact(
                &k_v2,
                dummy_hash(1),
                dummy_hash(11),
            )),
        );
        // Two distinct entries.
        assert_eq!(store.len(), 2);
        // Old entry is preserved at the v1 content hash; new at v2.
        assert_eq!(store.get(&k_v1).unwrap().display_hash[0], 10);
        assert_eq!(store.get(&k_v2).unwrap().display_hash[0], 11);
    }

    #[test]
    fn parse_env_hash_dimension_isolates_concurrent_envs() {
        let store = MemberDisplayFactStore::new();
        let k_env_a = key("/a.ts", 5, 1, "Foo", "a");
        let k_env_b = key("/a.ts", 5, 2, "Foo", "a");
        store.insert(
            k_env_a.clone(),
            Arc::new(make_member_display_fact(
                &k_env_a,
                dummy_hash(0),
                dummy_hash(0xA),
            )),
        );
        store.insert(
            k_env_b.clone(),
            Arc::new(make_member_display_fact(
                &k_env_b,
                dummy_hash(0),
                dummy_hash(0xB),
            )),
        );
        assert_eq!(store.len(), 2);
        assert_eq!(store.get(&k_env_a).unwrap().display_hash[0], 0xA);
        assert_eq!(store.get(&k_env_b).unwrap().display_hash[0], 0xB);
    }

    #[test]
    fn insert_is_idempotent_on_identical_key() {
        let store = MemberDisplayFactStore::new();
        let k = key("/a.ts", 1, 1, "Foo", "a");
        let first = Arc::new(make_member_display_fact(&k, dummy_hash(0), dummy_hash(1)));
        store.insert(k.clone(), Arc::clone(&first));
        let second = Arc::new(make_member_display_fact(&k, dummy_hash(0), dummy_hash(2)));
        store.insert(k.clone(), second);
        let got = store.get(&k).unwrap();
        assert_eq!(got.display_hash[0], 1, "first admission wins");
    }
}
