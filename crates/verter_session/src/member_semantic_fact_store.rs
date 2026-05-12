//! `MemberSemanticFactStore` — lazy member-body semantic
//! fingerprints, keyed on `parse_stable_hash`.
//!
//! The lazy-member-body half of the fact-emission contract (R28):
//! `Member(exporter, name, space).semantic_hash` is computed on the
//! FIRST member-access query that needs it, then interned here so
//! every subsequent consumer reuses the same canonical body
//! fingerprint.
//!
//! **Keyed on `parse_stable_hash`** so cosmetic edits (whitespace,
//! comments, JSDoc, generic param rename, declaration reorder)
//! produce the same key — the cache entry survives across
//! cosmetic-only re-upserts. This is the architectural pair with
//! [`MemberDisplayFactStore`], which keys on `content_hash` so
//! cosmetic edits DO recompute display facts.
//!
//! See `/type-cache-architecture` skill for the full R28 contract.

use std::sync::Arc;

use dashmap::DashMap;
use verter_semantic::analysis::Hash16;
use verter_semantic::facts::{Fact, FactHash, FactKey, SymbolSpace};

use crate::file_artifact_store::InternedName;

/// Key used by [`MemberSemanticFactStore`].
///
/// `parse_stable_hash` is the cosmetic-invariant identity of the
/// file at the time the fact was computed. Cosmetic edits that don't
/// shift the post-shallow-analysis decl skeleton keep the same key
/// → the store returns the existing entry without recomputation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemberSemanticFactKey {
    pub canonical: Arc<str>,
    pub parse_stable_hash: Hash16,
    pub parse_env_hash: Hash16,
    pub exporter: InternedName,
    pub member_name: InternedName,
    pub symbol_space: SymbolSpace,
}

/// Lazy member-body semantic fingerprint store.
///
/// **Lookup contract.** A cold miss returns `None`; the caller (the
/// resolver / materialiser producer) computes the fingerprint via
/// `compute_semantic_hash` and admits the entry via
/// [`MemberSemanticFactStore::insert`]. A warm hit returns the
/// canonical `Member` fact without re-walking the body.
///
/// **Concurrency.** `DashMap` shards on the key; concurrent readers
/// for different keys are wait-free. Same-key admissions go through
/// the producer's `StoreViewCompatToken`-keyed singleflight.
#[derive(Debug, Default)]
pub struct MemberSemanticFactStore {
    entries: DashMap<MemberSemanticFactKey, Arc<Fact>>,
}

impl MemberSemanticFactStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Lookup a member-body semantic fact by full key. `None` is a
    /// cold miss — the caller computes the fingerprint and admits
    /// it via [`Self::insert`].
    #[must_use]
    pub fn get(&self, key: &MemberSemanticFactKey) -> Option<Arc<Fact>> {
        self.entries.get(key).map(|v| Arc::clone(&*v))
    }

    /// Admit a freshly-computed `Member` semantic fact. If an entry
    /// already exists at the same key, the existing entry is
    /// preserved (write contention on identical inputs reduces to a
    /// no-op).
    pub fn insert(&self, key: MemberSemanticFactKey, fact: Arc<Fact>) {
        // Insert-only-if-absent semantics: an identical key MUST be a
        // deterministic recomputation, so we keep the first-admitted
        // entry to preserve `Arc` identity for shared consumers.
        self.entries.entry(key).or_insert(fact);
    }

    /// Number of cached entries. Used by tests + diagnostics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no entries are cached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop every cached entry. Used by GC sweeps and test setup.
    pub fn clear(&self) {
        self.entries.clear();
    }
}

/// Build a `FactKey::Member` discriminator from a
/// [`MemberSemanticFactKey`]. Used by consumers that need the
/// underlying `FactKey` for `fact_dep_signature` recording.
#[must_use]
pub fn member_fact_key(k: &MemberSemanticFactKey) -> FactKey {
    FactKey::Member {
        exporter: k.exporter.clone(),
        name: k.member_name.clone(),
        space: k.symbol_space,
    }
}

/// Construct a `Member` fact from its key and a freshly-computed
/// body fingerprint. Used by member-body producers.
#[must_use]
pub fn make_member_fact(key: &MemberSemanticFactKey, semantic_hash: FactHash) -> Fact {
    Fact {
        key: member_fact_key(key),
        semantic_hash,
        // Semantic-only — display fact lives in the parallel
        // store keyed on content_hash. The producer fills the
        // display fact separately.
        display_hash: semantic_hash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(canonical: &str, psh: u8, env: u8, exporter: &str, name: &str) -> MemberSemanticFactKey {
        let mut psh_arr = [0u8; 16];
        psh_arr[0] = psh;
        let mut env_arr = [0u8; 16];
        env_arr[0] = env;
        MemberSemanticFactKey {
            canonical: Arc::from(canonical),
            parse_stable_hash: psh_arr,
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
        let store = MemberSemanticFactStore::new();
        let k = key("/a.ts", 1, 1, "Foo", "a");
        assert!(store.get(&k).is_none());
        assert!(store.is_empty());
    }

    #[test]
    fn insert_and_get_round_trip() {
        let store = MemberSemanticFactStore::new();
        let k = key("/a.ts", 1, 1, "Foo", "a");
        let fact = Arc::new(make_member_fact(&k, dummy_hash(7)));
        store.insert(k.clone(), Arc::clone(&fact));
        let got = store.get(&k).expect("warm hit");
        assert_eq!(got.semantic_hash[0], 7);
        assert!(Arc::ptr_eq(&got, &fact), "same Arc identity preserved");
    }

    #[test]
    fn two_parse_stable_hashes_coexist_for_same_member() {
        // R28: when the file's decl skeleton changes
        // (`parse_stable_hash` shifts), the new and old entries
        // coexist in the store. Cosmetic edits keep the same key
        // (covered by `cosmetic_edit_keeps_same_key`); structural
        // edits introduce a NEW key (covered here).
        let store = MemberSemanticFactStore::new();
        let k_v1 = key("/a.ts", 1, 1, "Foo", "a");
        let k_v2 = key("/a.ts", 2, 1, "Foo", "a");
        store.insert(
            k_v1.clone(),
            Arc::new(make_member_fact(&k_v1, dummy_hash(1))),
        );
        store.insert(
            k_v2.clone(),
            Arc::new(make_member_fact(&k_v2, dummy_hash(2))),
        );
        assert_eq!(store.len(), 2);
        assert_eq!(store.get(&k_v1).unwrap().semantic_hash[0], 1);
        assert_eq!(store.get(&k_v2).unwrap().semantic_hash[0], 2);
    }

    #[test]
    fn cosmetic_edit_keeps_same_key() {
        // R13 / R28: a cosmetic edit produces the same
        // `parse_stable_hash` (parse-time structural invariant), so
        // the key is unchanged and the store returns the cached fact.
        let store = MemberSemanticFactStore::new();
        let k = key("/a.ts", 5, 7, "Foo", "a");
        let cached_fact = Arc::new(make_member_fact(&k, dummy_hash(42)));
        store.insert(k.clone(), Arc::clone(&cached_fact));
        // Caller hits the cache with the SAME parse_stable_hash —
        // gets the cached entry without recomputation.
        let warm = store.get(&k).expect("warm hit");
        assert!(
            Arc::ptr_eq(&warm, &cached_fact),
            "cosmetic edits keep parse_stable_hash → same key → cached Arc"
        );
    }

    #[test]
    fn parse_env_hash_dimension_isolates_concurrent_envs() {
        // R21 / R6: two project envs reading the same canonical at
        // the same parse_stable_hash but DIFFERENT parse_env_hash
        // coexist in the store under different keys.
        let store = MemberSemanticFactStore::new();
        let k_env_a = key("/a.ts", 5, 1, "Foo", "a");
        let k_env_b = key("/a.ts", 5, 2, "Foo", "a");
        store.insert(
            k_env_a.clone(),
            Arc::new(make_member_fact(&k_env_a, dummy_hash(0xA))),
        );
        store.insert(
            k_env_b.clone(),
            Arc::new(make_member_fact(&k_env_b, dummy_hash(0xB))),
        );
        assert_eq!(store.len(), 2);
        assert_eq!(store.get(&k_env_a).unwrap().semantic_hash[0], 0xA);
        assert_eq!(store.get(&k_env_b).unwrap().semantic_hash[0], 0xB);
    }

    #[test]
    fn member_fact_key_carries_the_three_fact_key_dimensions() {
        let k = key("/a.ts", 1, 1, "Foo", "a");
        let fk = member_fact_key(&k);
        match fk {
            FactKey::Member {
                exporter,
                name,
                space,
            } => {
                assert_eq!(exporter.as_ref(), "Foo");
                assert_eq!(name.as_ref(), "a");
                assert_eq!(space, SymbolSpace::Type);
            }
            other => panic!("expected Member, got {other:?}"),
        }
    }

    #[test]
    fn insert_is_idempotent_on_identical_key() {
        // Insert-only-if-absent: the second call with the same key
        // keeps the first-admitted entry.
        let store = MemberSemanticFactStore::new();
        let k = key("/a.ts", 1, 1, "Foo", "a");
        let first = Arc::new(make_member_fact(&k, dummy_hash(1)));
        store.insert(k.clone(), Arc::clone(&first));
        let second = Arc::new(make_member_fact(&k, dummy_hash(2)));
        store.insert(k.clone(), second);
        let got = store.get(&k).unwrap();
        // First insert wins.
        assert_eq!(got.semantic_hash[0], 1);
        assert!(Arc::ptr_eq(&got, &first));
    }
}
