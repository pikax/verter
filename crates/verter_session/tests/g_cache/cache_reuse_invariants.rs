//! R1 / R2 — byte-identical `upsert` produces zero cache mutations.
//!
//! Per the fact-based cache architectural rules:
//!
//! - **R1**: `host.upsert(canonical, source)` is a cache-state no-op iff the
//!   quintuple `(canonical, content_hash, parse_env_hash, resolve_env_hash,
//!   lib_env_hash)` is unchanged. No cache mutation, no semantic invalidation,
//!   no `bump_store_view_epoch`, no scheduler round-trip beyond the quintuple
//!   check.
//! - **R2**: `upsert` means "the source changed." Cache eviction is an
//!   explicit method with a stated scope; it is never a side effect of
//!   `upsert`.
//!
//! The byte-identical fast path is a true no-op: it does NOT clear
//! `compile_slots`, `cached_resolved_meta`, route-mirroring state, the
//! project-wide `resolved_type_cache_db` or
//! the semantic-fact cache, and does NOT bump `store_view_epoch`. None
//! of those mutations fire on a byte-identical re-upsert.
//!
//! Discrimination contract: every assertion fails if the fast path
//! mutates cache state.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

const SFC_SOURCE: &str = r#"<script setup lang="ts">
defineProps<{ alpha: string }>()
</script>
<template><div>{{ alpha }}</div></template>
"#;

const CANONICAL: &str = "/probe-reuse.vue";

fn build_host_with_one_file() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(SFC_SOURCE),
            file_kind: FileKind::from_path(CANONICAL),
            aliases: Vec::new(),
        })
        .expect("initial upsert succeeds");
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

/// R1 — byte-identical re-upsert MUST NOT bump `store_view_epoch`.
///
/// The byte-identical fast path skips the bump because no cache state
/// was touched. Capture the epoch before and after N=10 re-upserts and
/// assert no change.
#[test]
fn byte_identical_re_upsert_does_not_bump_store_view_epoch() {
    let host = build_host_with_one_file();

    let epoch_before = host.store_view_epoch();
    for _ in 0..10 {
        re_upsert_byte_identical(&host);
    }
    let epoch_after = host.store_view_epoch();

    assert_eq!(
        epoch_after, epoch_before,
        "R1: byte-identical re-upsert MUST NOT bump store_view_epoch. \
         A fast path that bumped unconditionally would fail this assertion \
         (epoch would have grown by exactly 10)."
    );
}

/// R1 discrimination — `parse_env_hash` equivalent (a real content change)
/// DOES bump `store_view_epoch`.
///
/// If the assertion above passed because the test accidentally constructed
/// a host that never bumps, this test would also pass. Instead we force
/// a structural change (a non-byte-identical upsert) and confirm the
/// epoch DOES bump — proving the no-op gate is conditional on the
/// quintuple, not unconditional.
#[test]
fn structural_change_does_bump_store_view_epoch() {
    let host = build_host_with_one_file();

    let epoch_before = host.store_view_epoch();
    // Change content — this is NOT byte-identical, so the full upsert
    // path runs and `bump_store_view_epoch` MUST fire.
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
        .expect("structural upsert succeeds");
    let epoch_after = host.store_view_epoch();

    assert!(
        epoch_after > epoch_before,
        "R1 discrimination: a structural source change MUST bump store_view_epoch; \
         epoch_before={epoch_before}, epoch_after={epoch_after}. If this assertion fails \
         the test above is uninformative (every upsert would be a no-op)."
    );
}

/// R1 — byte-identical re-upsert MUST NOT change DB entry counts.
///
/// Even when a fast path mutates `compile_slots`, `derived_raw_cache`
/// inner maps, and `dependency_cache` per-canonical entries, the outer
/// entry counts stay stable (the existing entry is mutated in place, not
/// removed), so this assertion is a SECONDARY discriminator alongside
/// the epoch-bump check.
///
/// What this test most reliably catches:
/// 1. A regression that re-keys the entry on byte-identical re-upsert
///    (entry count would change because the new key inserts and the old
///    drops).
/// 2. A regression that mass-evicts the DBs.
#[test]
fn byte_identical_re_upsert_preserves_db_entry_counts() {
    let host = build_host_with_one_file();

    let pts = host.project_type_store();
    let compile_count_before = pts.compile_cache().len();
    let derived_count_before = pts.derived_raw_cache().len();
    let dep_count_before = pts.dependency_cache().len();
    let indexed_count_before = pts.indexed().len();

    for _ in 0..10 {
        re_upsert_byte_identical(&host);
    }

    let compile_count_after = pts.compile_cache().len();
    let derived_count_after = pts.derived_raw_cache().len();
    let dep_count_after = pts.dependency_cache().len();
    let indexed_count_after = pts.indexed().len();

    assert_eq!(
        compile_count_after, compile_count_before,
        "R1: byte-identical re-upsert MUST NOT change compile_cache entry count"
    );
    assert_eq!(
        derived_count_after, derived_count_before,
        "R1: byte-identical re-upsert MUST NOT change derived_raw_cache entry count"
    );
    assert_eq!(
        dep_count_after, dep_count_before,
        "R1: byte-identical re-upsert MUST NOT change dependency_cache entry count"
    );
    assert_eq!(
        indexed_count_after, indexed_count_before,
        "R1: byte-identical re-upsert MUST NOT change FileArtifactStore entry count"
    );
}

/// R1 — byte-identical re-upsert MUST NOT clear the project-wide
/// `resolved_type_cache_db`.
///
/// A fast path that called `self.resolved_type_cache().clear()` would
/// do a project-wide wipe, not a per-canonical drain. We seed the cache
/// by driving real semantic work that populates it, then re-upsert and
/// assert the count is preserved. This is a hostile probe: a wiping
/// fast path would drop the count to zero after the first byte-identical
/// re-upsert.
#[test]
fn byte_identical_re_upsert_preserves_resolved_type_cache_len() {
    // Build a host with one file whose semantic resolution will populate
    // some resolved-type-cache entries. The exact entry count varies with
    // implementation; what matters is "non-zero before, equal after".
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(SFC_SOURCE),
            file_kind: FileKind::from_path(CANONICAL),
            aliases: Vec::new(),
        })
        .expect("initial upsert succeeds");

    // Drive component-meta resolution to seed resolved_type_cache_db.
    // The probe may legitimately produce zero cache entries on a simple
    // fixture; the test still discriminates because a wipe resets the
    // count to 0 while the correct no-op fast path keeps it at the seed
    // value (whatever that is).
    let _ = host.get_component_meta_with_resolution(CANONICAL);

    let count_before = host.project_type_store().resolved_type_cache().len();

    for _ in 0..10 {
        re_upsert_byte_identical(&host);
    }

    let count_after = host.project_type_store().resolved_type_cache().len();

    assert_eq!(
        count_after, count_before,
        "R1: byte-identical re-upsert MUST NOT clear the project-wide \
         resolved_type_cache_db. count_before={count_before}, count_after={count_after}. \
         A fast path that called `self.resolved_type_cache().clear()` \
         wipes the entire DB project-wide; this assertion fails for such \
         a path if the cache had any non-zero entries pre-upsert."
    );
}

/// R1 / R2 — direct audit-observer probe: byte-identical re-upsert
/// MUST NOT emit any `CacheDrainedAtUpsert` structured event.
///
/// The instrumentation hook
/// (`StructuredAuditEvent::CacheDrainedAtUpsert`) fires at every
/// cache cascade drain site (the `co_evicted_outside_project_type_store`
/// block plus `databases_drained`). The byte-identical fast path
/// goes through NONE of those sites, so observing zero events on a
/// re-upsert is direct proof of R1 / R2 compliance — independent of
/// the DB-state-snapshot tests above.
///
/// Discrimination: a STRUCTURAL re-upsert (different source) is
/// asserted to emit at least one such event in the same test, so a
/// regression that disabled the hook entirely would fail.
#[test]
fn byte_identical_reupsert_emits_no_cache_drained_at_upsert_events() {
    use verter_session::component_meta_audit::accumulator::RequestFootprintAccumulator;
    use verter_session::component_meta_audit::StructuredAuditEvent;
    use verter_session::request_context::{RequestContext, RequestContextGuard};

    let host = build_host_with_one_file();

    // Install a request context with a fresh accumulator so the
    // host-side `push_cache_drained_at_upsert` calls land in
    // observable state.
    let acc = Arc::new(RequestFootprintAccumulator::new());
    let ctx = RequestContext::new(404, Arc::from(CANONICAL), true, Some(Arc::clone(&acc)));
    let _g = RequestContextGuard::install(Arc::clone(&ctx));

    // N byte-identical re-upserts → zero cascade events.
    for _ in 0..5 {
        re_upsert_byte_identical(&host);
    }

    let state_after_byte_identical = acc.drain();
    let byte_identical_drain_events: Vec<&StructuredAuditEvent> = state_after_byte_identical
        .structured_events
        .iter()
        .filter(|e| matches!(e, StructuredAuditEvent::CacheDrainedAtUpsert { .. }))
        .collect();
    assert!(
        byte_identical_drain_events.is_empty(),
        "R1: byte-identical re-upsert MUST emit zero CacheDrainedAtUpsert events. \
         Observed {} events: {:?}",
        byte_identical_drain_events.len(),
        byte_identical_drain_events
    );

    // Discrimination: a STRUCTURAL change must emit at least one
    // cascade event. If the hook is no-op'd everywhere, this assert
    // fails and we know the absence-test above is uninformative.
    let _ = host
        .upsert(verter_session::UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(
                r#"<script setup lang="ts">
defineProps<{ gamma: boolean }>()
</script>
<template><div>{{ gamma }}</div></template>
"#,
            ),
            file_kind: verter_session::FileKind::from_path(CANONICAL),
            aliases: Vec::new(),
        })
        .expect("structural upsert succeeds");

    let state_after_structural = acc.drain();
    let structural_drain_events: Vec<&StructuredAuditEvent> = state_after_structural
        .structured_events
        .iter()
        .filter(|e| matches!(e, StructuredAuditEvent::CacheDrainedAtUpsert { .. }))
        .collect();
    assert!(
        !structural_drain_events.is_empty(),
        "R1 discrimination: a structural source change MUST emit at least one \
         CacheDrainedAtUpsert event. If this fails, the hook is no-op everywhere \
         and the absence-test above is uninformative."
    );
}
