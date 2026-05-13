//! Stage 10 — eviction policy discrimination tests.
//!
//! Binds R22 (eviction is memory-bound, not correctness-bound) +
//! the Stage 10 extension to `EvictionPolicyConfig` adding
//! `per_canonical_content_hash_retention` (default 3) and
//! `promote_threshold` (default 2). Each test FAILS pre-change
//! against the Stage 0 substrate and PASSES post-change against
//! the Stage 10 substrate — both fields ARE new in Stage 10.
//!
//! Hermeticity: in-process `FileArtifactStore` + synthetic
//! `IndexedReady` payloads. No third-party fixture.

use std::sync::Arc;

use verter_session::file_artifact_store::{FileArtifactKey, FileArtifactStore};
use verter_session::project_type_store::IndexedReady;
use verter_session::EvictionPolicyConfig;

fn synthetic_indexed(content_hash_seed: u8) -> Arc<IndexedReady> {
    let mut h = [0u8; 16];
    h[0] = content_hash_seed;
    Arc::new(IndexedReady::new_for_test(h))
}

fn legacy_key(canonical: &str, content_hash_seed: u8) -> FileArtifactKey {
    let mut h = [0u8; 16];
    h[0] = content_hash_seed;
    FileArtifactKey::legacy_for_test(Arc::from(canonical), h)
}

/// Stage 10 discrimination: per-canonical retention caps the number
/// of distinct `content_hash` variants kept per canonical. With
/// retention = 3, admitting 5 variants for the same canonical and
/// running the retention sweep MUST leave exactly 3.
#[test]
fn per_canonical_retention_evicts_oldest_variants_first() {
    let store = FileArtifactStore::new();
    // Admit 5 variants for the same canonical via the
    // content-addressed `insert_artifacts` API (so multiple
    // versions coexist; the legacy `insert` always drained prior
    // versions and would defeat the test).
    for seed in 0..5u8 {
        let key = legacy_key("/x.ts", seed);
        let indexed = synthetic_indexed(seed);
        let artifacts =
            Arc::new(verter_session::file_artifact_store::FileArtifacts::with_indexed(indexed));
        store.insert_artifacts(key, artifacts);
    }
    assert_eq!(store.len(), 5, "seed assertion: 5 variants admitted");

    // Run retention with cap = 3.
    store.enforce_per_canonical_retention(3);
    assert_eq!(
        store.len(),
        3,
        "retention=3 must shrink the entry count to exactly 3 \
         distinct variants; got {}",
        store.len()
    );

    // The surviving variants are the 3 highest-numbered ones
    // (deterministic content_hash sort — lower seeds drop first).
    let surviving_seeds: Vec<u8> = store
        .artifact_keys()
        .into_iter()
        .map(|k| k.content_hash[0])
        .collect();
    let mut surviving = surviving_seeds.clone();
    surviving.sort();
    assert_eq!(
        surviving,
        vec![2u8, 3u8, 4u8],
        "expected the 3 highest-seed variants to survive; got {:?}",
        surviving_seeds
    );
}

/// Stage 10 discrimination: per-canonical retention with cap = 1
/// drops every variant except the highest-numbered one.
#[test]
fn per_canonical_retention_one_keeps_only_top_variant() {
    let store = FileArtifactStore::new();
    for seed in 0..3u8 {
        let key = legacy_key("/x.ts", seed);
        let indexed = synthetic_indexed(seed);
        let artifacts =
            Arc::new(verter_session::file_artifact_store::FileArtifacts::with_indexed(indexed));
        store.insert_artifacts(key, artifacts);
    }
    assert_eq!(store.len(), 3);
    store.enforce_per_canonical_retention(1);
    assert_eq!(store.len(), 1, "retention=1 leaves exactly one variant");
    let surviving_seed = store
        .artifact_keys()
        .into_iter()
        .next()
        .map(|k| k.content_hash[0])
        .expect("at least one variant survives");
    assert_eq!(
        surviving_seed, 2u8,
        "retention=1 keeps the highest-content_hash variant (deterministic sort)"
    );
}

/// Stage 10 discrimination: `usize::MAX` retention disables the
/// per-canonical cap — every variant survives.
#[test]
fn per_canonical_retention_max_disables_cap() {
    let store = FileArtifactStore::new();
    for seed in 0..10u8 {
        let key = legacy_key("/x.ts", seed);
        let indexed = synthetic_indexed(seed);
        let artifacts =
            Arc::new(verter_session::file_artifact_store::FileArtifacts::with_indexed(indexed));
        store.insert_artifacts(key, artifacts);
    }
    assert_eq!(store.len(), 10);
    store.enforce_per_canonical_retention(usize::MAX);
    assert_eq!(
        store.len(),
        10,
        "retention=usize::MAX is a no-op; got {} entries (expected 10)",
        store.len()
    );
}

/// Stage 10 discrimination: promotion-aware LRU floor preserves hot
/// entries (hit count >= promote_threshold) when only cold
/// candidates are over the floor. Pre-Stage-10 the LRU was pure
/// recency; the promotion split is new.
#[test]
fn promote_threshold_retains_hot_entries() {
    let store = FileArtifactStore::new();
    // Admit 10 distinct canonicals (10 distinct keys / 10 distinct
    // canonicals).
    for i in 0..10u8 {
        let canonical = format!("/c{i}.ts");
        let key = FileArtifactKey::legacy_for_test(Arc::from(canonical.as_str()), [i; 16]);
        let indexed = Arc::new(IndexedReady::new_for_test([i; 16]));
        let artifacts =
            Arc::new(verter_session::file_artifact_store::FileArtifacts::with_indexed(indexed));
        store.insert_artifacts(key, artifacts);
    }
    assert_eq!(store.len(), 10);

    // Promote entries 0..3 to "hot" by hitting them 3 times each
    // (above the threshold of 2). Entries 3..10 remain "cold".
    for i in 0..3u8 {
        let canonical = format!("/c{i}.ts");
        let key = FileArtifactKey::legacy_for_test(Arc::from(canonical.as_str()), [i; 16]);
        for _ in 0..3 {
            let _ = store.get_artifacts(&key);
        }
        // Sanity: hit_count is now >= 2.
        assert!(
            store.hit_count(&key) >= 2,
            "/c{}.ts must be hot; got hit_count = {}",
            i,
            store.hit_count(&key)
        );
    }
    // Cold entries: zero hits.
    for i in 3..10u8 {
        let canonical = format!("/c{i}.ts");
        let key = FileArtifactKey::legacy_for_test(Arc::from(canonical.as_str()), [i; 16]);
        assert_eq!(
            store.hit_count(&key),
            0,
            "/c{}.ts must be cold; got hit_count = {}",
            i,
            store.hit_count(&key)
        );
    }

    // Run the promotion-aware LRU floor at min_floor=3 with
    // promote_threshold=2. The 3 hot entries survive; cold
    // entries age out to bring total down to 3.
    store.evict_lru_promoted(3, 2);
    assert_eq!(
        store.len(),
        3,
        "LRU floor at min_floor=3 must leave exactly 3 entries; got {}",
        store.len()
    );
    // The surviving entries are the 3 hot ones (canonicals
    // /c0.ts, /c1.ts, /c2.ts).
    let surviving_canonicals: Vec<String> = store
        .artifact_keys()
        .into_iter()
        .map(|k| k.canonical.as_ref().to_owned())
        .collect();
    for i in 0..3u8 {
        let expected = format!("/c{i}.ts");
        assert!(
            surviving_canonicals.contains(&expected),
            "hot entry {} must survive the LRU floor; surviving = {:?}",
            expected,
            surviving_canonicals
        );
    }
    for i in 3..10u8 {
        let expected = format!("/c{i}.ts");
        assert!(
            !surviving_canonicals.contains(&expected),
            "cold entry {} must be evicted before hot entries; surviving = {:?}",
            expected,
            surviving_canonicals
        );
    }
}

/// Stage 10 discrimination: with `promote_threshold = 0` (every
/// entry is hot — promotion disabled), the floor falls back to
/// pure recency. This characterises the legacy `evict_lru`
/// behaviour and proves the new method subsumes it.
#[test]
fn promote_threshold_zero_falls_back_to_pure_recency() {
    let store = FileArtifactStore::new();
    for i in 0..10u8 {
        let canonical = format!("/c{i}.ts");
        let key = FileArtifactKey::legacy_for_test(Arc::from(canonical.as_str()), [i; 16]);
        let indexed = Arc::new(IndexedReady::new_for_test([i; 16]));
        let artifacts =
            Arc::new(verter_session::file_artifact_store::FileArtifacts::with_indexed(indexed));
        store.insert_artifacts(key, artifacts);
    }
    // Bump access for entry 9 so it has the freshest tick.
    let bump_key = FileArtifactKey::legacy_for_test(Arc::from("/c9.ts"), [9u8; 16]);
    let _ = store.get_artifacts(&bump_key);

    store.evict_lru_promoted(3, 0);
    assert_eq!(store.len(), 3);
    let surviving_canonicals: Vec<String> = store
        .artifact_keys()
        .into_iter()
        .map(|k| k.canonical.as_ref().to_owned())
        .collect();
    assert!(
        surviving_canonicals.contains(&"/c9.ts".to_owned()),
        "freshest-tick entry /c9.ts must survive pure-recency LRU; \
         surviving = {:?}",
        surviving_canonicals
    );
}

/// Stage 10 discrimination: `EvictionPolicyConfig::default()`
/// exposes the new Stage 10 fields with documented defaults.
#[test]
fn eviction_policy_config_defaults_carry_stage10_tunables() {
    let policy = EvictionPolicyConfig::default();
    assert_eq!(
        policy.per_canonical_content_hash_retention, 3,
        "Stage 10 contract: default retention is 3"
    );
    assert_eq!(
        policy.promote_threshold, 2,
        "Stage 10 contract: default promote threshold is 2"
    );
    // Existing defaults remain.
    assert_eq!(policy.memory_pressure_threshold, usize::MAX);
    assert!(policy.min_floor >= 1);
}
