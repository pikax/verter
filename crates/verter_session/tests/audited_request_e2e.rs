//! End-to-end coverage for `AuditedRequest` — confirms the audit
//! record is published, the outer request_id is preserved through
//! `resolve_component_meta_with_view`, and `take_audit_record` drains
//! a concrete record.
//!
//! Protects against two regressions found in the post-F5 review:
//!
//! 1. `emit_audit_trace` used to just stderr the record and drop it
//!    without inserting into the host's `AuditRecordsStore`, so
//!    `take_audit_record` always returned `None`.
//! 2. `resolve_component_meta_with_view` used its own global static
//!    `next_component_meta_audit_request_id` counter, producing a
//!    different id from the one stamped onto
//!    `ResolvedComponentMetaState.request_id` by
//!    `get_component_meta_with_resolution` — the record was stored
//!    under the inner id while callers looked up with the outer id.

use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

fn setup_host() -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        workspace,
    ));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/x.vue".into()),
        input_id: "/x.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">defineProps<{label: string}>()</script>\
             <template><div>{{ label }}</div></template>",
        ),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });
    host
}

#[test]
fn audited_request_attach_to_returns_triple_with_matching_request_id() {
    let host = setup_host();
    let (_, resolution, record) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve("/x.vue")
        .expect("audited resolve should succeed");

    assert!(
        resolution.request_id > 0,
        "resolution must carry a non-zero request_id"
    );
    assert_eq!(
        record.request_id, resolution.request_id,
        "audit record's request_id must match resolution.request_id so \
         `take_audit_record(resolution.request_id)` drains the right record",
    );
    assert_eq!(record.canonical_id, "/x.vue");
}

#[test]
fn audited_request_attach_to_take_audit_record_drains_after_resolve() {
    let host = setup_host();
    let (_, resolution, _record) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve("/x.vue")
        .expect("first audited resolve succeeds");

    // The harness already drained the record; a second take by the same
    // id must return None (strict insert-then-take semantics).
    assert!(
        host.take_audit_record(resolution.request_id).is_none(),
        "record must have been drained by the harness on return",
    );
}

#[test]
fn concurrent_audits_on_same_host_each_see_their_own_record() {
    use std::thread;

    let host = setup_host();
    // Two concurrent audited-request threads on the same host, each
    // resolving the same canonical. Distinct request_ids must be
    // assigned (host.next_request_id is thread-safe) and each
    // thread's `take_audit_record(resolution.request_id)` must drain
    // that thread's own record, not the other's.
    let h1 = {
        let host = Arc::clone(&host);
        thread::spawn(move || {
            AuditedRequest::builder()
                .attach_to(host)
                .resolve("/x.vue")
                .map(|(_, r, rec)| (r.request_id, rec.request_id))
        })
    };
    let h2 = {
        let host = Arc::clone(&host);
        thread::spawn(move || {
            AuditedRequest::builder()
                .attach_to(host)
                .resolve("/x.vue")
                .map(|(_, r, rec)| (r.request_id, rec.request_id))
        })
    };
    let r1 = h1.join().unwrap().expect("thread 1 audit ok");
    let r2 = h2.join().unwrap().expect("thread 2 audit ok");
    assert_eq!(r1.0, r1.1, "thread 1 resolution/record ids match");
    assert_eq!(r2.0, r2.1, "thread 2 resolution/record ids match");
    assert_ne!(
        r1.0, r2.0,
        "concurrent audits on the same host must get distinct request_ids",
    );
}

#[test]
fn audited_request_record_carries_populated_footprint_when_capture_enabled() {
    // Plan §3 Commit 4 wire-up: when `footprint_capture` is enabled,
    // the request path mines the per-request accumulator and attaches a
    // `RustSemanticFootprintAudit` to the record. Without the
    // `mine_footprint` call inserted in `meta_resolve.rs`, the
    // `record.footprint` field would always be `None`.
    let host = setup_host();
    let (_, _resolution, record) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve("/x.vue")
        .expect("audited resolve should succeed");

    let footprint = record
        .footprint
        .as_ref()
        .expect("footprint_capture=true must populate record.footprint");
    // Cache counters are populated from the request's own atomics
    // (plan §1.4 — kills `is_approximate`). We exercise the read path
    // here; exact counts depend on resolver call shape and are pinned
    // by the Commit 7 corpus snapshots.
    let _ = footprint.cache_outcomes.cold_builds
        + footprint.cache_outcomes.warm_hits
        + footprint.cache_outcomes.joined_waits
        + footprint.cache_outcomes.sentinels;
    // The mined subgraph + indexed_ready_builds vectors must be
    // present (even if empty for this trivial fixture).
    let _ = footprint.derivation_subgraph.nodes.len();
    let _ = footprint.indexed_ready_builds.len();
}

#[test]
fn direct_resolve_without_audit_context_still_publishes_via_static_counter() {
    // Without AuditedRequest wrapping, the outer request_id counter is
    // not installed; audit must still publish via the legacy static
    // counter (fallback path).
    let host = setup_host();
    let (_analysis, resolution) = host
        .get_component_meta_with_resolution("/x.vue")
        .expect("resolution must succeed");
    // resolution.request_id here is host.next_request_id() (non-zero).
    // Because RequestContext was installed by get_component_meta_with_resolution,
    // the audit_builder sees it and stamps the record with the same id.
    assert!(resolution.request_id > 0);
    let record = host
        .take_audit_record(resolution.request_id)
        .expect("record must be published under the outer request_id");
    assert_eq!(record.request_id, resolution.request_id);
    assert_eq!(record.canonical_id, "/x.vue");
}
