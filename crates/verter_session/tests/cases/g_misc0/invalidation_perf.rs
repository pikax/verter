//! O(1) invalidation index regression test.
//!
//! Asserts the per-canonical secondary index drains entries in
//! O(K) (where K = entries owned by the canonical id), NOT O(N)
//! (total entries in the DB). The deterministic structural gate
//! is "entries visited equals entries actually owned by the
//! canonical id" — wall-clock timing assertions are forbidden.
//!
//! The test populates `ImportedRegistryDb` with ~10 000 entries
//! spanning 1 000 distinct canonical ids; one target canonical
//! owns exactly K entries. The capture-token counter
//! `invalidate_canonical_entries_visited` is asserted equal to K
//! (NOT N).
//!
//! `ImportedRegistryDb` is chosen as the test vehicle because its
//! `ImportedRegistryEntry { value: None, dep_signature: ... }`
//! shape can be constructed from any test crate without crate-
//! internal types. The same secondary-index contract applies
//! uniformly across every DB registered in
//! `project_type_store_dbs!` — see `invalidation_coverage.rs`
//! for the macro-coverage gate.

use std::sync::Arc;

use verter_session::component_meta_caches::{
    ImportedRegistryDb, ImportedRegistryEntry, ImportedRegistryKey,
};
use verter_session::for_tests::CaptureToken;
use verter_session::invalidation_domain::InvalidationByCanonical;

#[test]
fn invalidate_canonical_touches_only_indexed_entries() {
    let db = ImportedRegistryDb::new();
    // Populate ~10 000 entries spanning 1 000 distinct canonical ids.
    // Target canonical id "/w/target.ts" owns exactly K = 7 entries;
    // the remaining entries live on 999 other canonical ids
    // (10 entries each = 9 990).
    const K: usize = 7;
    let target = "/w/target.ts";
    let target_arc: Arc<str> = Arc::from(target);
    let mut inserted = 0usize;
    for i in 0..K {
        let key: ImportedRegistryKey = (
            Arc::clone(&target_arc),
            Arc::from(format!("name-{i}").as_str()),
        );
        let entry = Arc::new(ImportedRegistryEntry {
            value: None,
            fact_dep_signature: Arc::from(Vec::new()),
            validated_at_generation: 0,
        });
        db.insert_for_test(key, entry);
        inserted += 1;
    }
    for shard in 0..999u64 {
        let canonical = format!("/w/shard-{shard}.ts");
        let canonical_arc: Arc<str> = Arc::from(canonical.as_str());
        for slot in 0..10u64 {
            let key: ImportedRegistryKey = (
                Arc::clone(&canonical_arc),
                Arc::from(format!("name-{shard}-{slot}").as_str()),
            );
            let entry = Arc::new(ImportedRegistryEntry {
                value: None,
                fact_dep_signature: Arc::from(Vec::new()),
                validated_at_generation: 0,
            });
            db.insert_for_test(key, entry);
            inserted += 1;
        }
    }
    let total_n = inserted;
    assert!(total_n >= 9_990, "expected ≥ 9 990 entries, got {total_n}");
    let pre_count = db.live_count();
    assert_eq!(pre_count, total_n);

    // Drain via the InvalidationByCanonical impl under capture-token.
    let guard = CaptureToken::start_for_query("invalidate_canonical_perf");
    let drained = db.invalidate_canonical_for(target);
    let snap = guard.end();

    assert_eq!(
        drained, K,
        "invalidate_canonical_for must drain exactly K entries via the secondary index, \
         drained = {drained}, expected K = {K}",
    );
    assert_eq!(
        db.live_count(),
        total_n - K,
        "post-invalidation live_count must drop by K (not by N)",
    );
    let visited = snap.counter("invalidate_canonical_entries_visited");
    assert_eq!(
        visited, K as u64,
        "invalidate_canonical_entries_visited must equal K (entries owned by the canonical), \
         NOT N (total entries). visited = {visited}, K = {K}, N = {total_n}",
    );
}
