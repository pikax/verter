//! A closed session must refuse the AUDITED component-meta lane before any
//! host operation runs: no resolution, no request id, no audit publication
//! under a session that no longer owns a lifetime.

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::HostConfig;

fn audited_host_with_component() -> ComponentMetaHost {
    let host = ComponentMetaHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    });
    host.upsert_base("/ClosedAudited.vue", SFC)
        .expect("the base upsert succeeds");
    host
}

const SFC: &str = r#"<script setup lang="ts">
defineProps<{ label: string }>();
</script>

<template><p>{{ label }}</p></template>
"#;

#[test]
fn a_closed_session_refuses_the_audited_lane_before_resolution_or_publication() {
    let host = audited_host_with_component();
    let session = host.open_session().expect("the host is live");

    // Control: while the session is live the audited lane resolves and
    // publishes a REAL record (non-zero request id, capture stored).
    let live = session
        .get_component_meta_with_audit("/ClosedAudited.vue")
        .into_parts();
    let (output, live_record) = live;
    let output = output.expect("the live audited query resolves");
    assert!(output.is_some(), "the component resolves while live");
    assert!(
        live_record.request_id != 0,
        "the live query published a real audit record"
    );

    // The session's terminal close revokes the audited lane too: the query
    // refuses with the closed-session error and carries the cheap
    // default-filled record — request id 0, nothing captured, so nothing was
    // resolved or published under the dead session's identity.
    session.close();
    let (outcome, record) = session
        .get_component_meta_with_audit("/ClosedAudited.vue")
        .into_parts();
    let error = outcome.expect_err("a closed session refuses the audited lane");
    assert!(
        error.to_string().contains("session is closed"),
        "the refusal names the closed session: {error}"
    );
    assert_eq!(
        record.request_id, 0,
        "no audit record was published by the refusal"
    );
    assert_eq!(record.canonical_id, "/ClosedAudited.vue");
    assert!(record.files.is_empty(), "no resolution ran for the refusal");
}

/// The refusal record's capture state reports the HOST's audit config, not a
/// fixed label: on a host with audit disabled the closed-session refusal is
/// marked `AuditDisabled` (capture is off at host config), never
/// `FilteredNoop` (which claims audit is enabled and a filter rejected the
/// kind). On an audited host the same refusal stays `FilteredNoop`.
#[test]
fn a_closed_session_refusal_reports_the_hosts_capture_state() {
    for (audit_enabled, expected) in [
        (false, verter_audit::AuditCaptureState::AuditDisabled),
        (true, verter_audit::AuditCaptureState::FilteredNoop),
    ] {
        let host = ComponentMetaHost::new_standalone(HostConfig {
            audit_enabled,
            footprint_capture: audit_enabled,
            ..HostConfig::default()
        });
        host.upsert_base("/ClosedAudited.vue", SFC)
            .expect("the base upsert succeeds");
        let session = host.open_session().expect("the host is live");
        session.close();

        let (outcome, record) = session
            .get_component_meta_with_audit("/ClosedAudited.vue")
            .into_parts();
        let error = outcome.expect_err("a closed session refuses the audited lane");
        assert!(
            error.to_string().contains("session is closed"),
            "the closed-session refusal wins over the audit-config refusal \
             (audit_enabled={audit_enabled}): {error}"
        );
        assert_eq!(
            record.capture_state, expected,
            "the refusal record reports the host's capture state \
             (audit_enabled={audit_enabled})"
        );
    }
}
