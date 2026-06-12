//! R1 / R2 — byte-identical `host.upsert(...)` is a true cache-state no-op.
//!
//! Architectural rules bound: **R1, R2**.
//!
//! The `evict_canonical_inventory.json` documents every DB that
//! `host.upsert(...)` could mutate on the byte-identical fast path
//! (the `co_evicted_outside_project_type_store` block plus the
//! ProjectTypeStore drains). This test cross-references that inventory:
//! construct a host, perform N=10 byte-identical re-upserts,
//! and assert every observable DB-level dimension is preserved.
//!
//! Verify-bullet correspondence:
//! - **Verify #1**: "After N byte-identical re-upserts: all DB
//!   entries from `evict_canonical_inventory.json` unchanged."
//!   — `inventory_dbs_unchanged_after_n_byte_identical_re_upserts`.
//! - **Verify #2**: "`bump_store_view_epoch` counter unchanged
//!   when quintuple is unchanged." —
//!   `store_view_epoch_unchanged_after_n_byte_identical_re_upserts`.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

const SFC_SOURCE: &str = r#"<script setup lang="ts">
defineProps<{ alpha: string }>()
</script>
<template><div>{{ alpha }}</div></template>
"#;

const CANONICAL: &str = "/probe-no-op.vue";

fn build_host_and_seed() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(SFC_SOURCE),
            file_kind: FileKind::from_path(CANONICAL),
            aliases: Vec::new(),
        })
        .expect("initial seed upsert succeeds");
    // Drive component-meta to populate downstream caches so the
    // inventory probe sees non-empty DBs.
    let _ = host.get_component_meta_with_resolution(CANONICAL);
    host
}

fn re_upsert_byte_identical(host: &VerterHost) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(SFC_SOURCE),
            file_kind: FileKind::from_path(CANONICAL),
            aliases: Vec::new(),
        })
        .expect("re-upsert succeeds");
}

/// Snapshot every DB the inventory tracks so a post-upsert
/// comparison can prove byte-identical re-upsert is a true no-op.
#[derive(Debug, PartialEq, Eq)]
struct InventorySnapshot {
    indexed_len: usize,
    compile_cache_len: usize,
    derived_raw_cache_len: usize,
    dependency_cache_len: usize,
    resolved_type_cache_len: usize,
    store_view_epoch: u64,
}

fn snapshot(host: &VerterHost) -> InventorySnapshot {
    let pts = host.project_type_store();
    InventorySnapshot {
        indexed_len: pts.indexed().len(),
        compile_cache_len: pts.compile_cache().len(),
        derived_raw_cache_len: pts.derived_raw_cache().len(),
        dependency_cache_len: pts.dependency_cache().len(),
        resolved_type_cache_len: pts.resolved_type_cache().len(),
        store_view_epoch: host.store_view_epoch(),
    }
}

/// Verify-bullet #1: every DB on
/// `evict_canonical_inventory.json` is unchanged after N=10
/// byte-identical re-upserts.
///
/// Discriminating predicate: this test runs all the upserts after taking
/// the baseline snapshot. A fast path that called `clear()` on
/// `resolved_type_cache_db` AND bumped
/// `store_view_epoch` would fail it. The snapshot comparison fails if any
/// DB shrank, any DB grew, or the epoch advanced.
#[test]
fn inventory_dbs_unchanged_after_n_byte_identical_re_upserts() {
    let host = build_host_and_seed();
    let baseline = snapshot(&host);

    for _ in 0..10 {
        re_upsert_byte_identical(&host);
    }

    let after = snapshot(&host);
    assert_eq!(
        after, baseline,
        "R1: every DB on the evict_canonical inventory MUST be \
         unchanged after byte-identical re-upserts. A fast \
         path that bumped store_view_epoch and called \
         resolved_type_cache().clear() would \
         diverge on at least one of those dimensions."
    );
}

/// Artifact/env preservation probe (the successor of the retired
/// env-cache dimension on this suite): the retained `IndexedReady` — and
/// its `Arc<EvalEnv>` — survives byte-identical re-upserts BY IDENTITY.
/// A fast path that evicted/rebuilt the artifact (or rebuilt only the
/// env) would break `Arc::ptr_eq` even though every inventory LENGTH
/// stays equal, so this discriminates what the length snapshot cannot.
#[test]
fn indexed_artifact_and_env_preserved_across_byte_identical_re_upserts() {
    let host = build_host_and_seed();
    // Materialise the canonical artifact through a public read.
    let _ = host.get_component_meta(CANONICAL);
    let before = host
        .project_type_store()
        .indexed()
        .get_any(CANONICAL)
        .expect("the meta read must have materialised an IndexedReady");

    for _ in 0..10 {
        re_upsert_byte_identical(&host);
    }

    let after = host
        .project_type_store()
        .indexed()
        .get_any(CANONICAL)
        .expect("the artifact must survive byte-identical re-upserts");
    assert!(
        Arc::ptr_eq(&before, &after),
        "byte-identical re-upserts must preserve the retained \
         IndexedReady by identity (no evict/rebuild churn)",
    );
    assert!(
        Arc::ptr_eq(before.eval_env(), after.eval_env()),
        "the artifact's EvalEnv must be the same allocation (no \
         env-only rebuild)",
    );
}

/// Verify-bullet #2: `bump_store_view_epoch` counter is unchanged when
/// the quintuple `(canonical, content_hash, parse_env_hash,
/// resolve_env_hash, lib_env_hash)` is unchanged.
///
/// Today the quintuple maps 1:1 to "byte-identical source" because
/// `configure_projects` / `set_workspace` are the only paths that
/// change `parse_env_hash` / `resolve_env_hash` / `lib_env_hash`, and
/// each of those paths fully invalidates the host before any further
/// upsert can hit the fast path.
#[test]
fn store_view_epoch_unchanged_after_n_byte_identical_re_upserts() {
    let host = build_host_and_seed();
    let epoch_before = host.store_view_epoch();

    for _ in 0..10 {
        re_upsert_byte_identical(&host);
    }

    let epoch_after = host.store_view_epoch();
    assert_eq!(
        epoch_after, epoch_before,
        "R1: byte-identical re-upsert MUST NOT bump store_view_epoch. \
         A fast path that bumped unconditionally would diverge — \
         epoch_before = {epoch_before}, epoch_after = {epoch_after}. The \
         fast path skips the bump because no cache state was touched."
    );
}

/// Verify-bullet #3 (discrimination): `parse_env_hash`-equivalent state
/// (a real content change) DOES bump `store_view_epoch`.
///
/// Today the host's `parse_env_hash` is fixed at construction time — it
/// doesn't change without going through `configure_projects`, which
/// resets the host wholesale and bumps the epoch via a different code
/// path. We discriminate the byte-identical no-op against a structural
/// content change instead, which is the canonical "quintuple changes"
/// signal callers actually exercise. The structural-change path runs
/// the full upsert flow and MUST bump.
#[test]
fn structural_change_does_bump_store_view_epoch() {
    let host = build_host_and_seed();
    let epoch_before = host.store_view_epoch();

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(
                r#"<script setup lang="ts">
defineProps<{ beta: number }>()
</script>
<template><div>{{ beta }}</div></template>
"#,
            ),
            file_kind: FileKind::from_path(CANONICAL),
            aliases: Vec::new(),
        })
        .expect("structural-change upsert succeeds");

    let epoch_after = host.store_view_epoch();
    assert!(
        epoch_after > epoch_before,
        "R1 discrimination: a structural source change MUST bump the \
         store_view_epoch (epoch_before={epoch_before}, \
         epoch_after={epoch_after}). If this assertion fails, the \
         byte-identical no-op tests above are uninformative because \
         every upsert would be a no-op."
    );
}

/// Verify-bullet #1 / #4 — the inventory snapshot is a strict equality
/// check. Asserting the byte-identical re-upsert does not change a
/// repeat snapshot more than once.
#[test]
fn inventory_dbs_stable_across_many_byte_identical_re_upserts() {
    let host = build_host_and_seed();
    let baseline = snapshot(&host);

    // Run upserts in batches and snapshot after each batch. Every
    // intermediate snapshot must equal baseline.
    for batch in 1..=5 {
        for _ in 0..2 {
            re_upsert_byte_identical(&host);
        }
        let mid = snapshot(&host);
        assert_eq!(
            mid, baseline,
            "R1: inventory snapshot must stay identical across {batch} \
             batches of byte-identical re-upserts. The fact that batches \
             accumulate (rather than reset) catches a regression that \
             grows or shrinks any DB after the first re-upsert."
        );
    }
}
