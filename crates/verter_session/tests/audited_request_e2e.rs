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
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess, WorkspaceRead};

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
    // `RequestFootprintAudit` to the record. Without the
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
fn audited_request_resolve_produces_non_empty_vfs_reads_for_trivial_vue_sfc() {
    // Plan §3.A Commit 6.D exit criterion. Proves SessionVfsSink
    // is registered and routing events for the audit window.
    //
    // Critical fixture detail: `host.upsert` submits a scheduler
    // request with `source = Some(raw)`, so the source stage
    // never calls `workspace.read_file` for the upserted file.
    // Worse, during upsert's analysis pass the resolver EAGERLY
    // loads relative type imports via `ensure_loaded`, and those
    // reads happen BEFORE the audit window opens — the sink is
    // not yet registered.
    //
    // To guarantee ALL reads happen inside the audit window, we
    // inject BOTH files directly into the memory workspace and
    // skip `upsert` entirely. The resolver's first touch of
    // `/c.vue` now goes through `ensure_loaded` → scheduler
    // source stage → `workspace.read_file` — which fans into our
    // registered sink with `current_request_id()` set.
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file(
        "/c.vue".into(),
        Arc::from(
            "<script setup lang=\"ts\">\n\
             import type { Props } from './types';\n\
             defineProps<Props>();\n\
             </script>\n\
             <template><div>{{ label }}</div></template>\n",
        ),
    );
    workspace.inject_file(
        "/types.ts".into(),
        Arc::from("export interface Props { label: string }\n"),
    );
    let ws_access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws_access,
    ));

    let (_, resolution, record) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve("/c.vue")
        .expect("audited resolve succeeds");

    let footprint = record
        .footprint
        .as_ref()
        .expect("footprint_capture=true must populate footprint");

    // Because the run_custom closure body (outside
    // get_component_meta_with_resolution) no longer has the
    // request context installed, the SFC must be constructed so
    // that resolution itself performs at least one ws().read_file.
    // Our fixture relies on the indexer/pre-indexer touching
    // workspace files during get_component_meta's own call chain.
    //
    // If this assertion is empty, the sink path is broken. Emit a
    // diagnostic dump to make triage easier.
    if footprint.vfs_reads.is_empty() {
        eprintln!(
            "vfs_reads empty; record:\n  request_id={}\n  \
             imported_dep_entries={}\n  indexed_ready_builds.len={}\n  \
             loaded_files={:?}",
            record.request_id,
            record.store.imported_dependency_entries,
            footprint.indexed_ready_builds.len(),
            footprint.loaded_files(),
        );
    }
    assert!(
        !footprint.vfs_reads.is_empty(),
        "SessionVfsSink must route VFS read events into footprint.vfs_reads — \
         empty means the sink registration broke (plan §3.A Commit 6.D)",
    );
    for r in &footprint.vfs_reads {
        assert_eq!(
            r.request_id, resolution.request_id,
            "every routed VFS read must carry the resolution request_id",
        );
    }
}

#[test]
fn session_vfs_sink_drops_reads_outside_get_component_meta_window() {
    // Negative-scope test: a `workspace.read_file` performed in
    // the `run_custom` closure AFTER
    // `get_component_meta_with_resolution` returns is outside the
    // per-call `RequestContextGuard` scope. Scheduler TLS no
    // longer carries our request_id; the sink (already
    // deregistered when the guard dropped) sees no event.
    //
    // Protects against a reviewer proposing "just keep the sink
    // registered across the whole run_custom closure" — that
    // would attribute unrelated reads to the same audit.
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/aux.ts".into(), Arc::from("export const AUX = 1;\n"));
    workspace.inject_file(
        "/c.vue".into(),
        Arc::from(
            "<script setup lang=\"ts\">defineProps<{x:string}>();</script>\
             <template>{{x}}</template>",
        ),
    );
    let ws_access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws_access,
    ));

    let ws_read = workspace.clone();
    let (_, _resolution, record) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .run_custom(|h| {
            let res = h.get_component_meta_with_resolution("/c.vue")?;
            // By this point the per-request guard has dropped.
            // This read must NOT appear in footprint.vfs_reads.
            let _ = ws_read.read_file("/aux.ts");
            Some(res)
        })
        .expect("audited run_custom succeeds");

    let footprint = record.footprint.as_ref().expect("footprint populated");
    for r in &footprint.vfs_reads {
        assert_ne!(
            r.canonical_id.as_ref(),
            "/aux.ts",
            "reads performed after get_component_meta_with_resolution returns MUST NOT be \
             captured — the RequestContextGuard has already dropped, so scheduler TLS \
             no longer carries our request_id",
        );
    }
}

#[test]
fn concurrent_attach_to_on_same_host_16_threads_each_audit_sees_only_its_own_vfs_reads() {
    // Stress the fan-out filter: N concurrent audits on one host,
    // each registering its own sink, must NOT see each other's
    // events (the SessionVfsSink filters by request_id).
    use std::thread;
    let host = setup_host();
    let mut handles = Vec::new();
    for _ in 0..16 {
        let host = Arc::clone(&host);
        handles.push(thread::spawn(move || {
            let (_, resolution, record) = AuditedRequest::builder()
                .attach_to(host)
                .resolve("/x.vue")
                .expect("audit ok");
            let fp = record.footprint.as_ref().expect("footprint present");
            for r in &fp.vfs_reads {
                assert_eq!(
                    r.request_id, resolution.request_id,
                    "thread's audit must only see events routed to its own request_id",
                );
            }
            resolution.request_id
        }));
    }
    let ids: Vec<u64> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    // All request_ids distinct.
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        ids.len(),
        sorted.len(),
        "concurrent audits must receive distinct request_ids"
    );
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
