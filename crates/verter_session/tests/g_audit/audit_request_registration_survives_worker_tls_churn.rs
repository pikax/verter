//! Critical P0 discriminator (Slice 2.5 / plan §1.8 + §1.8.2):
//! `AuditRequestRegistration` membership in
//! `HostAuditRuntime::active_requests` MUST survive worker-thread TLS
//! churn.
//!
//! ## Why this test exists
//!
//! v4.1/v4.2 of the audit plan keyed the active-request registry
//! off `RequestContextGuard::install` / drop. That TLS guard is
//! installed/dropped once per scheduler worker stage — i.e. many
//! times per logical request. Using the TLS guard's lifecycle as the
//! registry's lifecycle causes the request to flicker out of the
//! map between worker stages, so the host-owned peak-RSS sampler
//! misses the in-flight window.
//!
//! v4.3 (this slice) keys `active_requests` off the
//! `AuditRequestRegistration` lifecycle: insert ONLY in
//! `AuditRequestRegistration::new`; remove ONLY in `finalize`
//! (idempotent) or defensive `Drop`. The TLS guard is a separate
//! concern that does NOT touch the registry.
//!
//! ## Discrimination
//!
//! Spin up N worker threads, each repeatedly installing and dropping
//! a `RequestContextGuard` for the same logical request. Sample the
//! runtime's `snapshot().contains_active_request(id)` from the main
//! thread between worker iterations. The request id must be present
//! continuously — every sampled snapshot returns `true`.
//!
//! Pre-change tree (registry keyed off TLS guard lifecycle): some
//! samples return `false` because every worker drop momentarily
//! cleared the slot. Test FAILS.
//! Post-change tree (registry keyed off
//! `AuditRequestRegistration::new` / `finalize`): every sample
//! returns `true`. Test PASSES.
//!
//! Skipped on WASM (no worker threads).

#![cfg(not(target_arch = "wasm32"))]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use verter_audit::{
    ComponentMetaPayload, RequestAuditRecord, RequestKind, RequestKindPayload, RequestMemoryAudit,
    RequestStoreAudit, RequestTimingAudit,
};
use verter_session::host_audit_runtime::AuditRequestRegistration;
use verter_session::request_context::{RequestContext, RequestContextGuard};
use verter_session::{HostConfig, VerterHost};

#[test]
fn active_request_registration_survives_worker_tls_install_drop_churn() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        audit_timing_capture: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));
    let request_id: u64 = 4242;
    let ctx = RequestContext::new(request_id, Arc::from("/churn.vue"), false, None);

    // Pre-state: registry empty.
    let snap0 = host.host_audit_runtime().snapshot();
    assert!(
        !snap0.contains_active_request(request_id),
        "pre-state must not contain the synthetic request id",
    );

    // Construct the registration. This is the SOLE event that
    // populates the active-request registry.
    let registration = AuditRequestRegistration::new(host.as_ref(), Arc::clone(&ctx));
    assert_eq!(
        registration.request_id(),
        Some(request_id),
        "filter allows ComponentMeta — registration must be Active",
    );

    // Mid-state: registry contains the id BEFORE any worker thread
    // installs a TLS guard.
    let snap_mid = host.host_audit_runtime().snapshot();
    assert!(
        snap_mid.contains_active_request(request_id),
        "registration::new must populate the active-request registry",
    );

    // Spawn N worker threads. Each worker installs and drops a
    // `RequestContextGuard` repeatedly to simulate scheduler-worker
    // stage cycling.
    const WORKER_COUNT: usize = 4;
    const ITERATIONS_PER_WORKER: u32 = 25;
    let stop_flag = Arc::new(AtomicBool::new(false));
    let workers: Vec<thread::JoinHandle<()>> = (0..WORKER_COUNT)
        .map(|_| {
            let ctx = Arc::clone(&ctx);
            let stop = Arc::clone(&stop_flag);
            thread::spawn(move || {
                for _ in 0..ITERATIONS_PER_WORKER {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let guard = RequestContextGuard::install(Arc::clone(&ctx));
                    // Hold the guard briefly so the main thread has a
                    // chance to sample mid-install — discriminating
                    // against any "removed by drop, re-inserted by
                    // install" pattern that could mask the bug.
                    thread::sleep(Duration::from_millis(2));
                    drop(guard);
                    thread::sleep(Duration::from_millis(2));
                }
            })
        })
        .collect();

    // Sample the registry continuously while the workers churn TLS.
    // The plan's discrimination requires EVERY sample during the
    // in-flight window to observe the request id present.
    let sample_deadline =
        Instant::now() + Duration::from_millis(50 * u64::from(ITERATIONS_PER_WORKER));
    let mut samples_taken = 0u32;
    let mut samples_present = 0u32;
    while Instant::now() < sample_deadline {
        let snap = host.host_audit_runtime().snapshot();
        samples_taken += 1;
        if snap.contains_active_request(request_id) {
            samples_present += 1;
        }
        thread::sleep(Duration::from_millis(3));
    }
    stop_flag.store(true, Ordering::Relaxed);
    for w in workers {
        w.join().expect("worker thread joins cleanly");
    }

    // The discriminating assertion: every sample observed the id.
    assert!(
        samples_taken >= 5,
        "test loop must take a meaningful number of samples; got \
         {samples_taken}. The deadline is generous; if this fires the \
         test scaffolding itself is broken.",
    );
    assert_eq!(
        samples_present, samples_taken,
        "registry membership flickered: {samples_present}/{samples_taken} \
         samples observed the request id during worker TLS churn. \
         Pre-change tree (registry keyed off RequestContextGuard \
         install/drop) would show samples_present < samples_taken \
         because every guard drop momentarily cleared the slot."
    );

    // Finalise the registration. Records store gets the record, the
    // active map is drained.
    let synthetic_record = RequestAuditRecord {
        request_id,
        canonical_id: "/churn.vue".to_string(),
        kind: RequestKind::ComponentMeta,
        parent_request_id: None,
        from_cache: false,
        timings: RequestTimingAudit::default(),
        memory: RequestMemoryAudit::default(),
        store: RequestStoreAudit::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::ComponentMeta(ComponentMetaPayload::default()),
        trace_id: String::new(),
    };
    assert!(
        registration.finalize(synthetic_record),
        "finalize on Active registration must return true on first call",
    );

    // Post-finalize: the id must be GONE from the registry.
    let snap_after = host.host_audit_runtime().snapshot();
    assert!(
        !snap_after.contains_active_request(request_id),
        "after finalize, the request id MUST NOT be in the registry — \
         finalize_active_request both removes the entry AND publishes \
         the record atomically through the same crate-private surface.",
    );
}
