//! A host's audit records live in ONE key space, minted by the host.
//!
//! Component-meta reads that run outside an audited entry-point (no
//! `AuditRequestRegistration` on the request context) still publish a
//! record into the host's records store. That record must be keyed by an
//! id the HOST minted, the same authority every audited entry-point
//! draws from:
//!
//! * a second, process-wide id counter keys records into a per-host
//!   store it does not own, so an unregistered record can land on an id
//!   the host also hands out and silently replace that record; and
//! * the same work on two fresh hosts would yield different ids, because
//!   the process-wide counter carries state from every other host.
//!
//! Discrimination: under a process-wide counter the second host's
//! unregistered record id differs from the first host's (the counter
//! only advances), so the per-host id sets differ; an id collision would
//! instead drop a record and shrink the store below two.

use std::sync::Arc;

use verter_audit::batch::AuditRecordSource;
use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::semantic_query::ProjectionMode;

const CANONICAL: &str = "/KeySpace.vue";
const SFC: &str = r#"<script setup lang="ts">
defineProps<{ label: string }>()
</script>
<template><div>{{ label }}</div></template>
"#;

/// Drive one unregistered read and one audited read on a fresh host.
/// Returns the sorted record ids left in the host's store and the id the
/// audited entry-point reported.
fn run_on_fresh_host() -> (Vec<u64>, u64) {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(CANONICAL.to_string()),
        input_id: CANONICAL.to_string(),
        source: Arc::from(SFC),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });

    // No registration is installed on this path: the record is published
    // straight into the host's store.
    host.resolve_component_meta(CANONICAL, ProjectionMode::Expanded)
        .expect("the fixture resolves to a component");
    let (_analysis, resolution) = host
        .get_component_meta_with_resolution(CANONICAL)
        .expect("the audited entry-point resolves the fixture");

    let mut ids = Vec::new();
    host.host_audit_runtime()
        .audit_records_store()
        .for_each_record(&mut |_inserted_at, record| ids.push(record.request_id));
    ids.sort_unstable();
    (ids, resolution.request_id)
}

#[test]
fn unregistered_and_audited_records_share_one_host_minted_key_space() {
    let (ids_a, audited_a) = run_on_fresh_host();
    let (ids_b, audited_b) = run_on_fresh_host();

    assert_eq!(
        ids_a.len(),
        2,
        "the unregistered and the audited read each keep their own record; got ids {ids_a:?}"
    );
    assert!(
        ids_a.contains(&audited_a),
        "the audited entry-point's id {audited_a} keys a stored record; got ids {ids_a:?}"
    );
    assert_eq!(
        (ids_a, audited_a),
        (ids_b, audited_b),
        "the same work on two fresh hosts mints the same record ids: the host is the only \
         id authority for its records store"
    );
}
