//! Cross-crate self-test for the
//! [`verter_session::tests::audit_tls_harness`] TLS observer
//! propagation harness.
//!
//! `verter_session`'s `RequestContextGuard::install` plants the
//! audit observer into the substrate's TLS slot. This test proves
//! the wiring is visible from a DIFFERENT crate (`verter_compiler`),
//! by driving a real production audited entry-point — `compile_with_audit`
//! — and verifying that:
//! - When audit is enabled (host config flag on), the producer's
//!   `code_transform_ops` counter on the audit record is non-zero (the
//!   TLS observer reached `verter_compiler::code_transform`'s `record_event`
//!   call sites).
//! - When audit is disabled, no record is published and the harness's
//!   calling-thread observer-presence check returns `false`.
//!
//! The earlier scaffolding probe (`_audit_harness_probe`) has been
//! removed; production audit producers now drive this test directly.

use std::sync::Arc;

use verter_compiler::compile::CompileTarget;
use verter_session::tests::audit_tls_harness::assert_observer_reaches;
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

fn build_host(audit_enabled: bool) -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled,
            footprint_capture: false,
            ..HostConfig::default()
        },
        workspace,
    ));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/cross_crate_probe.vue".into()),
        input_id: "/cross_crate_probe.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\nconst greeting = 'hello';\n</script>\n\
             <template><div>{{ greeting }}</div></template>",
        ),
        file_kind: FileKind::VueSfc,
        aliases: Vec::new(),
    });
    host
}

#[test]
fn cross_crate_observer_reaches_compiler_via_compile_with_audit() {
    let host = build_host(true);
    let mut record_kind: Option<verter_audit::RequestKind> = None;
    let mut code_transform_ops: u32 = 0;
    let report = assert_observer_reaches(true, || {
        // Drive the real production audited entry-point. The producer
        // crate (`verter_compiler::code_transform`) emits
        // `record_event(CompileCodeTransformOp)` at every public op —
        // those events flow through `current_observer()` and bump the
        // per-request counter that surfaces as
        // `CompilePayload::code_transform_ops`. Non-zero ⇒ TLS observer
        // propagation reached the producer crate.
        let (_result, record) =
            host.compile_with_audit("/cross_crate_probe.vue", CompileTarget::BUNDLER);
        if let Some(rec) = record {
            record_kind = Some(rec.kind.clone());
            if let Some(payload) = rec.compile_payload() {
                code_transform_ops = payload.code_transform_ops;
            }
        }
    });

    assert!(
        record_kind.is_some(),
        "compile_with_audit must publish a record when audit_enabled=true; \
         pre-change tree (no substrate TLS plumbing in RequestContextGuard::install) \
         would leave the slot empty so the producer's instrumentation would have \
         no observer to bump and downstream record assembly would still publish a \
         record but the discriminator below would still fail. report = {report:?}",
    );
    assert!(
        code_transform_ops > 0,
        "verter_compiler::code_transform's `record_event` call sites must see Some(observer) \
         so the per-request counter goes up; got code_transform_ops={code_transform_ops}. \
         A regression that drops the TLS plumbing would leave the producer's \
         `current_observer()` returning None, the counter would stay at 0, and this \
         assertion would fail. report = {report:?}",
    );
    assert!(
        report.observer_seen_on_calling_thread,
        "harness must record the calling-thread observation as Some \
         when the audited entry-point is driven inside the harness scope: {report:?}",
    );
}

#[test]
fn cross_crate_observer_absent_when_audit_disabled() {
    let host = build_host(false);
    let mut record_kind: Option<verter_audit::RequestKind> = None;
    let report = assert_observer_reaches(false, || {
        // With `audit_enabled=false`, the registration is `Noop` and
        // `compile_with_audit` returns `record = None`. The compile
        // itself still runs but no observer is installed in TLS.
        let (_result, record) =
            host.compile_with_audit("/cross_crate_probe.vue", CompileTarget::BUNDLER);
        record_kind = record.map(|r| r.kind);
    });

    assert!(
        record_kind.is_none(),
        "compile_with_audit must NOT publish a record when audit is disabled; \
         a tautological producer that emits regardless of TLS state would still \
         publish through the registration's `Noop` path being mis-wired and \
         this assertion would fail. record_kind = {record_kind:?}",
    );
    assert!(
        !report.observer_seen_on_calling_thread,
        "harness must record the calling-thread observation as None when audit \
         is disabled: {report:?}",
    );
}
