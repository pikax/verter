//! Audit-cleanliness regression for `getComponentMeta` over a component
//! with non-trivial fallthrough.
//!
//! Codifies the audit-nesting invariant for fallthrough / root-inheritance
//! synthesis paths: the entire `getComponentMeta` request — including the
//! inheritance / fallthrough resolver chain — emits exactly ONE
//! `RequestAuditRecord` from the audited entry-point, and ZERO additional
//! records from any nested host method.
//!
//! Concrete drift modes this regression characterises:
//!
//! * A future commit that wired `host.resolve_type_with_audit(...)`,
//!   `host.evaluate_type_expression_with_audit(...)`, or
//!   `host.resolve_named_symbol_with_audit(...)` into the
//!   fallthrough / root-inheritance synthesis path. Each of those
//!   public host methods plants its own `AuditRequestRegistration`,
//!   so calling them from inside an outer audited request would
//!   produce nested records.
//! * A future commit that introduced a record-emitting helper from
//!   inside `host_manage/component_meta_methods.rs`,
//!   `host_manage/component_meta_extract.rs`,
//!   `host_manage/component_meta_entry.rs`, or
//!   `host_manage/fallthrough.rs` synthesis paths.
//!
//! Discriminating: this test would FAIL if any synthesis path
//! re-entered a public `_with_audit` host method (records-store
//! count would jump above one), and would PASS only when the entire
//! component-meta synthesis stays on the raw
//! `dispatch.execute_read(...) + accumulate_dispatch_dep_signature(...)`
//! substrate.
//!
//! The fallthrough surface for this fixture is non-trivial: a single
//! native root with declared props (`label`) consumes `label` and
//! exposes the remaining intrinsic surface for fallthrough, exercising
//! the inheritance resolver's cache-write path. A trivial fixture
//! that bypasses the resolver would not characterise the invariant.

use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// Build a host with audit recording fully enabled. The records store
/// snapshot must observe every published `RequestAuditRecord`; if
/// audit were disabled the post-condition would be silently true.
fn audit_enabled_host() -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        workspace,
    ))
}

/// Seed a small SFC that exercises the fallthrough resolver's cache
/// path. A single native root + a declared prop forces the inheritance
/// resolver to compute and publish a fallthrough surface, which is
/// the path most likely to drift toward an internal `_with_audit`
/// re-entrance during refactors.
fn seed_fallthrough_sfc(host: &VerterHost) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/component.vue".into()),
        input_id: "/component.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             defineProps<{label: string}>()\
             </script>\
             <template><button>{{ label }}</button></template>",
        ),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });
}

/// Regression: `getComponentMeta` for a component with a non-trivial
/// fallthrough surface emits exactly one `RequestAuditRecord`. The
/// inheritance resolver and any synthesis helper it invokes must use
/// raw `dispatch.execute_read(...)` rather than the audited public
/// host methods.
///
/// FAILS on any future commit that wires `resolve_type_with_audit`,
/// `evaluate_type_expression_with_audit`, or
/// `resolve_named_symbol_with_audit` into the fallthrough / inheritance
/// synthesis path — nested records would push the records-store size
/// above one.
#[test]
fn getcomponentmeta_fallthrough_emits_no_nested_records() {
    let host = audit_enabled_host();
    seed_fallthrough_sfc(&host);

    // Pre-condition: the host has not emitted any records yet. A
    // drift commit that started leaking records during host setup
    // would already invalidate the regression target.
    let pre = host.host_audit_runtime().snapshot();
    assert_eq!(
        pre.records_store_size, 0,
        "fresh host must start with zero audit records, got {pre:?}",
    );

    // Drive the audited entry-point. This is the production path that
    // synthesises the full `ComponentMetaAnalysis` including the
    // fallthrough / root-reachability fields.
    //
    // Note: `AuditedRequest::resolve_component_meta` drains the
    // outer record via `take_audit_record` before returning. The
    // discriminating signal therefore lives in:
    //   (a) `record` itself — the SINGLE drained record, scoped to
    //       the outer request id; and
    //   (b) the records-store size AFTER the drain, which must be
    //       exactly 0 — any nested `_with_audit` re-entrance would
    //       have left an additional record under a different
    //       request id that the harness does not drain.
    let (_, resolution, record) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/component.vue")
        .expect("audited resolve_component_meta must succeed for the fallthrough fixture");

    // Discriminating side-check: the resolution carries a non-zero
    // request id, and the audit record's request id matches. A
    // drift commit that detached the inner record's request id from
    // the outer resolution's request id would surface here.
    assert!(
        resolution.request_id > 0,
        "resolution must carry a non-zero request_id; got {}",
        resolution.request_id
    );
    assert_eq!(
        record.request_id, resolution.request_id,
        "outer record's request_id must match the resolution's request_id",
    );

    // Post-condition 1: after the harness drained the outer record,
    // the records store is EMPTY. A nested `_with_audit` call from
    // inside fallthrough / inheritance / component-meta synthesis
    // would have published a second record under a different
    // request id; that record would survive the harness drain.
    let post = host.host_audit_runtime().snapshot();
    assert_eq!(
        post.records_store_size, 0,
        "harness drained the outer record; any leftover records prove \
         a nested `_with_audit` re-entrance from inside synthesis. got {post:?}",
    );

    // Post-condition 2: no active request remains tracked. A drift
    // commit that planted an inner registration without finalising
    // it would leak entries into the active-request registry.
    assert_eq!(
        post.active_request_count, 0,
        "no active request registration may leak after the audited resolve completes; \
         got {post:?}",
    );

    // Post-condition 3: the published record really is the
    // ComponentMeta record for the audited canonical, not a
    // synthesis-path child record that happens to share the request
    // id. This catches the (less likely but possible) drift where a
    // nested record stomps on the outer record under the same id.
    assert_eq!(record.canonical_id, "/component.vue");
    assert_eq!(record.kind, verter_audit::RequestKind::ComponentMeta);
}

/// Reaffirmation companion: even when the request id allocator
/// deliberately advances between two consecutive audited resolves,
/// each call must publish exactly one record. This protects against
/// a regression where a synthesis cache writes a side-effect record
/// during the SECOND call (e.g. on warm-cache promotion) but not the
/// first.
///
/// Discriminating: a future commit that pushed a nested record only
/// on the warm-cache path would let the first assertion pass but
/// trip the second.
#[test]
fn getcomponentmeta_fallthrough_warm_path_emits_no_nested_records() {
    let host = audit_enabled_host();
    seed_fallthrough_sfc(&host);

    // Cold call. The harness drains the outer record on return, so
    // the records-store size after the call must be 0 — the same
    // discriminating signal as the cold-path test.
    let (_, resolution_cold, record_cold) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/component.vue")
        .expect("cold audited resolve must succeed");
    assert_eq!(record_cold.request_id, resolution_cold.request_id);
    let post_cold = host.host_audit_runtime().snapshot();
    assert_eq!(
        post_cold.records_store_size, 0,
        "cold call: nested `_with_audit` re-entrance would leave \
         leftover records after the harness drains the outer record; \
         got {post_cold:?}",
    );

    // Warm call. The component-meta result cache should hit, but the
    // audited entry-point still produces exactly one outer record.
    let (_, resolution_warm, record_warm) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/component.vue")
        .expect("warm audited resolve must succeed");
    assert_ne!(
        resolution_cold.request_id, resolution_warm.request_id,
        "successive audited requests must carry distinct request_ids; \
         got cold={} warm={}",
        resolution_cold.request_id, resolution_warm.request_id,
    );
    assert_eq!(record_warm.request_id, resolution_warm.request_id);

    let post_warm = host.host_audit_runtime().snapshot();
    assert_eq!(
        post_warm.records_store_size, 0,
        "warm call: nested `_with_audit` re-entrance from inside the \
         warm-cache promotion path would leave leftover records after \
         the harness drains; got {post_warm:?}",
    );
    assert_eq!(
        post_warm.active_request_count, 0,
        "warm-path registration must not leak; got {post_warm:?}",
    );
}
