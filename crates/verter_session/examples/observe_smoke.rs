//! Small host-backed capture check for pull-request CI.
//!
//! Run with `node scripts/observe-lane.mjs smoke`. The full instrumented
//! Rust suite runs separately; this example builds no test harnesses.

use std::sync::Arc;

use verter_audit::{AuditCaptureState, CaptureAvailability};
use verter_session::{capture, CompileTarget, HostConfig, UpsertRequest, VerterHost};

const CANONICAL: &str = "/instrumentation-smoke.vue";
const SOURCE: &str = r#"<script setup lang="ts">
const message = 'instrumentation smoke'
</script>
<template><div>{{ message }}</div></template>"#;

fn main() {
    assert_eq!(capture::availability(), CaptureAvailability::Available);

    let compile = |enabled| {
        let host = Arc::new(VerterHost::new_standalone(HostConfig {
            audit_enabled: enabled,
            footprint_capture: enabled,
            ..HostConfig::default()
        }));
        let registered = host
            .upsert(UpsertRequest {
                canonical_id: Some(CANONICAL.to_string()),
                input_id: CANONICAL.to_string(),
                source: Arc::from(SOURCE),
                file_language: verter_language::LanguageRegistry::global()
                    .classify_static(CANONICAL)
                    .static_resolution(),
                aliases: Vec::new(),
            })
            .expect("the smoke component must register");
        assert_eq!(registered.canonical_id, CANONICAL);

        let (result, audit) = host
            .compile_with_audit(CANONICAL, CompileTarget::BUNDLER)
            .into_parts();
        let result = result.expect("the smoke component must compile");
        assert!(
            result.errors.is_empty(),
            "the smoke component has diagnostics"
        );
        let script = result
            .script
            .expect("the compile must produce a script")
            .code;
        assert!(script.contains("instrumentation smoke"));

        let stored = host.host_audit_runtime().take_record(audit.request_id);
        if enabled {
            assert_eq!(audit.capture_state, AuditCaptureState::ActiveStored);
            assert!(audit.request_id > 0);
            assert_eq!(audit.canonical_id, CANONICAL);
            let payload = audit
                .compile_payload()
                .expect("capture must include compile data");
            assert!(
                payload.output_bytes > 0,
                "capture must describe real output"
            );
            assert_eq!(payload.num_script_blocks, 1);
            let stored = stored.expect("capture must publish the request record");
            assert_eq!(stored.request_id, audit.request_id);
            assert_eq!(stored.capture_state, AuditCaptureState::ActiveStored);
        } else {
            assert_eq!(audit.capture_state, AuditCaptureState::AuditDisabled);
            assert!(stored.is_none(), "disabled audit must publish no record");
        }
        script
    };

    assert_eq!(
        compile(true),
        compile(false),
        "capture must preserve compile output"
    );
    println!("Instrumentation smoke passed: compile output and stored audit capture verified");
}
