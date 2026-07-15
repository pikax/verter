//! `RequestAuditRecord::parent_request_id`
//! discriminating coverage.
//!
//! When a sub-request is initiated while a parent context is on the
//! current thread's TLS slot (set up via
//! [`verter_session::request_context::RequestContextGuard::install`]),
//! the new [`verter_session::request_context::RequestContext`] sniffs
//! the scheduler's `current_request_id()` at construction and stores
//! it as `parent_request_id`. `AuditBuilder::finish` reads that slot
//! and stamps the record's envelope-level
//! `parent_request_id: Option<String>` field.
//!
//! A sub-request constructed under an installed parent guard
//! captures the parent's id, and the synthesized / finalised audit
//! record exposes it.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use verter_audit::AuditObserver;
use verter_session::request_context::{RequestContext, RequestContextGuard};

/// Strict TLS-only test — exercises the `RequestContext` parent
/// capture path without invoking the full scheduler / audit
/// pipeline. This is the lowest-level discriminating probe: the
/// in-flight `RequestContext::with_kind` constructor must observe
/// the scheduler-side TLS slot.
#[test]
fn request_context_constructed_under_installed_parent_records_parent_request_id() {
    // Empty TLS at start.
    assert_eq!(
        verter_scheduler::request_context::current_request_id(),
        None
    );

    let parent: Arc<RequestContext> = RequestContext::new(
        /* request_id */ 4242,
        Arc::from("/parent.vue"),
        true,
        None,
    );
    let _parent_guard = RequestContextGuard::install(Arc::clone(&parent));

    // Parent is now installed: `current_request_id()` returns
    // Some(4242).
    assert_eq!(
        verter_scheduler::request_context::current_request_id(),
        Some(4242),
        "parent guard install must populate scheduler-side TLS",
    );

    // A NEW RequestContext constructed inside the parent guard's
    // scope must record the parent's id. The captured parent must
    // equal 4242.
    let child = RequestContext::new(
        /* request_id */ 9999,
        Arc::from("/child.vue"),
        false,
        None,
    );
    assert_eq!(
        child.parent_request_id,
        Some(4242),
        "child constructed under installed parent must capture parent_request_id; \
         the parent's id must propagate"
    );

    // Negative control: a context constructed AFTER the parent
    // guard drops must NOT carry a parent (stack discipline).
    drop(_parent_guard);
    assert_eq!(
        verter_scheduler::request_context::current_request_id(),
        None
    );
    let orphan = RequestContext::new(
        /* request_id */ 7777,
        Arc::from("/orphan.vue"),
        false,
        None,
    );
    assert_eq!(
        orphan.parent_request_id, None,
        "context constructed with empty TLS must have parent_request_id == None",
    );
}

/// End-to-end discriminating test: drives an audited resolve under
/// an outer-installed parent guard, then drains the audit record
/// and asserts `record.parent_request_id == Some(parent.to_string())`.
///
/// This exercises the full wire path:
/// scheduler-side TLS at sub-request construction → captured parent
/// → audit-finalisation → envelope field.
#[test]
fn audited_sub_request_under_installed_parent_publishes_parent_request_id_in_record() {
    use verter_session::audited_request::AuditedRequest;
    use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

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
        canonical_id: Some("/child.vue".into()),
        input_id: "/child.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">defineProps<{label: string}>()</script>\
             <template><div>{{ label }}</div></template>",
        ),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });

    // Install a parent context on this thread's TLS BEFORE
    // dispatching the audited child resolve. The audited
    // entry-point will construct its own `RequestContext` for the
    // child; that constructor observes the parent on TLS and
    // captures the parent's id.
    //
    // Note: the harness installs `NestedAuditGuard` to reject
    // *re-entrant audits* on the same thread, but the parent
    // RequestContext here is a bare context (no live audit
    // registration), so the harness's nested-audit guard does not
    // trip — the child runs as a fresh audit, only the
    // `current_request_id()` TLS slot exposes the parent's id.
    const PARENT_REQ_ID: u64 = 0xCAFE_BABE;
    let parent: Arc<RequestContext> = RequestContext::new(
        /* request_id */ PARENT_REQ_ID,
        Arc::from("/parent-context.vue"),
        false,
        None,
    );
    let _parent_guard = RequestContextGuard::install(Arc::clone(&parent));

    let (_, _, record) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/child.vue")
        .expect("audited child resolve succeeded");

    assert_eq!(
        record.parent_request_id,
        Some(PARENT_REQ_ID.to_string()),
        "child record must carry parent_request_id == Some('{}'); \
         the constructor sniffs scheduler TLS, so this field captures \
         the parent's id whenever a parent is installed",
        PARENT_REQ_ID,
    );
}

/// Negative control: a top-level audited resolve with no parent
/// context installed must produce a record with
/// `parent_request_id == None`. Confirms the capture is opt-in via
/// the TLS slot, not a side-effect that fires unconditionally.
#[test]
fn audited_resolve_without_parent_context_publishes_none_parent_request_id() {
    use verter_session::audited_request::AuditedRequest;
    use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

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
        canonical_id: Some("/orphan.vue".into()),
        input_id: "/orphan.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">defineProps<{label: string}>()</script>\
             <template><div>{{ label }}</div></template>",
        ),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });

    // No parent installed on TLS.
    assert_eq!(
        verter_scheduler::request_context::current_request_id(),
        None
    );

    let (_, _, record) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/orphan.vue")
        .expect("audited orphan resolve succeeded");

    assert!(
        record.parent_request_id.is_none(),
        "top-level audited resolve must have parent_request_id == None, got {:?}",
        record.parent_request_id,
    );
}

/// Trait-method-level discriminator: confirms that the scheduler's
/// `record_scheduler_dispatch` observer hook routes through the
/// active `RequestContext` and populates the `scheduler_audit` slot
/// idempotently. The first call sets the slot; subsequent calls
/// bump only the dispatch_count.
#[test]
fn record_scheduler_dispatch_first_call_sets_slot_subsequent_bumps_count() {
    let ctx: Arc<RequestContext> =
        RequestContext::new(/* request_id */ 1, Arc::from("/c.vue"), false, None);

    // First call: slot is None → store the new audit verbatim.
    let first = verter_audit::SchedulerAudit {
        worker_thread_id: "ThreadId(1)".to_string(),
        worker_pool: verter_audit::WorkerPool::Io,
        depths: verter_audit::SchedulerDepths { inbox: 5, queue: 3 },
        queue_dwell_ms: 12.5,
        dispatch_count: 1,
    };
    ctx.record_scheduler_dispatch(first);

    let snap = ctx
        .scheduler_audit
        .lock()
        .clone()
        .expect("first dispatch wrote slot");
    assert_eq!(snap.worker_thread_id, "ThreadId(1)");
    assert!(matches!(snap.worker_pool, verter_audit::WorkerPool::Io));
    assert_eq!(snap.dispatch_count, 1);
    assert_eq!(snap.queue_dwell_ms, 12.5);

    // Second call: slot is Some → bump dispatch_count, do NOT
    // overwrite first-dispatch facts.
    let second = verter_audit::SchedulerAudit {
        worker_thread_id: "ThreadId(2)".to_string(),
        worker_pool: verter_audit::WorkerPool::Cpu,
        depths: verter_audit::SchedulerDepths { inbox: 0, queue: 0 },
        queue_dwell_ms: 99.9,
        dispatch_count: 1,
    };
    ctx.record_scheduler_dispatch(second);

    let snap = ctx
        .scheduler_audit
        .lock()
        .clone()
        .expect("second dispatch kept slot");
    assert_eq!(
        snap.worker_thread_id, "ThreadId(1)",
        "second dispatch must NOT overwrite first-dispatch worker_thread_id"
    );
    assert_eq!(
        snap.queue_dwell_ms, 12.5,
        "second dispatch must NOT overwrite first-dispatch queue_dwell_ms"
    );
    assert_eq!(
        snap.dispatch_count, 2,
        "second dispatch must increment dispatch_count to 2"
    );
}
