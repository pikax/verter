//! Budget oracle: the 2nd query of the same key hits the warm
//! component-meta cache.
//!
//! Drives `get_component_meta(/owner.vue)` twice in succession with
//! no intervening mutation. Snapshots
//! `provenance_snapshot().component_meta_result_cache_hits` AND
//! `component_meta_result_cache_misses` at three points (pre, after
//! query 1, after query 2) and asserts the second query advanced
//! `cache_hits` by EXACTLY 1 while `cache_misses` stayed flat.
//!
//! ## Why this is a budget oracle
//!
//! `component_meta_result_cache_hits` is the production cache's
//! authoritative warm-return counter (see
//! `component_meta_result_db.rs::get_with_view` and the project-global
//! cache). A second identical request that
//! does NOT increment `cache_hits` is a cache-miss in disguise —
//! either the cache key drifted (cache-key fragmentation), the
//! revalidation logic rejected the warm entry (over-invalidation),
//! or the production path bypassed the final-result cache entirely
//! (fall-through to the cold resolver). All three are observable
//! regressions and the counter delta discriminates them.
//!
//! ## Discrimination contract
//!
//! Regression shape: query 1 incurs a cold miss (cache_misses += 1);
//! query 2 with NO intervening change incurs a SECOND miss because
//! a regression keyed the cache on a per-request token, or because
//! a revalidator rejected the warm entry.
//!
//! Correct shape: query 1 incurs the cold miss; query 2 hits the
//! warm cache exactly once (cache_hits += 1, cache_misses unchanged).
//!
//! ### Why the discrimination is non-trivial
//!
//! Asserting `cache_hits > 0` would not discriminate: a warm cache
//! hit during the cold resolver's internal sub-queries would satisfy
//! the predicate even if the second top-level request itself missed.
//!
//! Asserting `cache_hits >= 2` after query 2 would not discriminate
//! either: the cold resolver may bubble multiple sub-cache reads on
//! every request, so query 1's cold path could already be at
//! cache_hits == 5.
//!
//! The discriminating assertion is the DELTA between query 1 and
//! query 2: post-query-2 cache_hits == post-query-1 cache_hits + 1,
//! AND post-query-2 cache_misses == post-query-1 cache_misses. The
//! exact delta of (1 hit, 0 miss) pins the cache-substrate
//! authoritatively.

use std::sync::Arc;

use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const OWNER_VUE: &str = r#"<script setup lang="ts">
type Props = { item: string; count: number }
defineProps<Props>()
</script>
<template><div>{{ item }}: {{ count }}</div></template>
"#;

#[test]
fn second_query_of_same_key_advances_cache_hits_not_misses() {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(
        HostConfig {
            analysis_level: verter_session::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws_access,
    ));

    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/owner.vue".to_string()),
        input_id: "/owner.vue".to_string(),
        source: Arc::from(OWNER_VUE),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static("/owner.vue")
            .static_resolution(),
        aliases: Vec::new(),
    });

    // Snapshot 0: clean baseline.
    let snap0 = host.provenance_snapshot();
    let hits0 = snap0.component_meta_result_cache_hits;
    let misses0 = snap0.component_meta_result_cache_misses;

    // Query 1 — cold resolver runs end-to-end. The final-result
    // cache should publish a warm entry. The `cache_misses` counter
    // SHOULD advance (production hits the cache before resolving).
    let meta1 = host
        .get_component_meta("/owner.vue")
        .expect("query 1 must resolve");
    assert!(
        !meta1.props.is_empty(),
        "query 1 must populate at least one prop (defineProps<Props>) — \
         fixture expects 2 props (item, count). Got {} props.",
        meta1.props.len()
    );

    let snap1 = host.provenance_snapshot();
    let hits1 = snap1.component_meta_result_cache_hits;
    let misses1 = snap1.component_meta_result_cache_misses;

    // Query 2 — must hit the warm final-result cache. NO mutation
    // between query 1 and query 2; the cache key, content hash, and
    // env hashes are identical, and `HostFenceValidator` MUST
    // revalidate the entry as warm.
    let meta2 = host
        .get_component_meta("/owner.vue")
        .expect("query 2 must resolve");
    assert_eq!(
        meta2.props.len(),
        meta1.props.len(),
        "query 2 must publish the same prop count as query 1 \
         (no intervening mutation): query1={}, query2={}",
        meta1.props.len(),
        meta2.props.len(),
    );

    let snap2 = host.provenance_snapshot();
    let hits2 = snap2.component_meta_result_cache_hits;
    let misses2 = snap2.component_meta_result_cache_misses;

    let q2_hit_delta = hits2.saturating_sub(hits1);
    let q2_miss_delta = misses2.saturating_sub(misses1);

    // Discriminating assertion 1: query 2 advanced `cache_hits` by
    // EXACTLY 1. The production cache's `get_with_view` bumps the
    // counter once per validated warm return — a value other than 1
    // means either (a) the warm read fell through to the cold
    // resolver (delta=0, the regression this oracle catches), or (b)
    // the cache bumped multiple times for a single request (a
    // double-count regression that would also break audit
    // attribution).
    assert_eq!(
        q2_hit_delta, 1,
        "second query MUST advance `component_meta_result_cache_hits` \
         by EXACTLY 1 (got delta={q2_hit_delta}). \
         hits before query 1 = {hits0}, after query 1 = {hits1}, \
         after query 2 = {hits2}. \
         A delta of 0 means the second query missed the warm cache \
         (cache-key fragmentation, over-invalidation, or fall-through \
         to the cold resolver). A delta > 1 means the warm-read \
         counter bumped multiple times for a single top-level request \
         (double-count regression)."
    );

    // Discriminating assertion 2: query 2 did NOT advance
    // `cache_misses`. A second identical query that bumps misses is
    // a hidden cache-miss: the warm path either wasn't consulted or
    // was rejected by an over-eager revalidator.
    assert_eq!(
        q2_miss_delta, 0,
        "second query MUST NOT advance `component_meta_result_cache_misses` \
         (got delta={q2_miss_delta}). \
         misses before query 1 = {misses0}, after query 1 = {misses1}, \
         after query 2 = {misses2}. \
         A non-zero delta means the second query treated the cache as \
         cold — either the production path bypassed the final-result \
         cache, or `HostFenceValidator` falsely rejected the warm entry \
         (over-invalidation regression)."
    );

    // Cross-check: query 1 SHOULD have advanced `cache_misses` (the
    // cold-build path bumps `cache_misses` before resolving). A delta
    // of 0 here means the production flow bypasses the
    // final-result cache lookup entirely on the cold path — a
    // substrate regression that would invalidate this oracle's
    // premise.
    let q1_miss_delta = misses1.saturating_sub(misses0);
    assert!(
        q1_miss_delta >= 1,
        "query 1 MUST advance `component_meta_result_cache_misses` \
         by at least 1 (got delta={q1_miss_delta}). \
         misses before = {misses0}, after = {misses1}. \
         A delta of 0 means the cold resolver did NOT consult the \
         final-result cache before computing — the budget oracle's \
         premise (warm-after-cold) is broken."
    );
}
