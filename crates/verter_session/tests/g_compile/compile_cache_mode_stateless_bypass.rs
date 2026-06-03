//! `Stateless` mode bypasses both typed cache nodes.
//!
//! A `requested_mode == Stateless` compile produces fresh output every
//! call, never reads or writes the session slot or the content-addressed
//! node, and never finalises a fact signature.
//!
//! Observable signals (no direct lookup counter exists, so indirect):
//!   - the response reports `actual_mode == Stateless`,
//!   - the session slot is never published (`compile_slot_fact_dep_signature`
//!     stays `None`, and `compile_slot_is_warm` stays `false`),
//!   - the content-addressed node's entry count stays 0.
//!
//! Discrimination: before the `requested_mode` field on `CompileProfile`,
//! the `actual_mode` response field, the Stateless routing, and the
//! `compile_output_pure_content_entry_count` accessor existed, this test
//! would not compile. The `fact_dep_signature == None` + `entry_count
//! == 0` assertions additionally fail against any implementation that
//! routes Stateless through the session cache.

use verter_session::{
    CompileCacheMode, CompileProfile, FileKind, HostConfig, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn upsert_vue(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

fn stateless_profile() -> CompileProfile {
    CompileProfile {
        requested_mode: CompileCacheMode::Stateless,
        ..CompileProfile::default()
    }
}

const SIMPLE: &str =
    "<script setup lang=\"ts\">const n = 1</script><template><div>{{ n }}</div></template>";

#[test]
fn stateless_compile_produces_output_and_writes_no_cache() {
    let host = host();
    upsert_vue(&host, "/A.vue", SIMPLE);
    let profile = stateless_profile();

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/A.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })
        .expect("stateless compile produces output");

    // The compile ran and returned real output.
    assert!(
        !response.code.is_empty(),
        "stateless compile must emit code"
    );

    // The runtime ran under Stateless and recorded no downgrade.
    assert_eq!(response.requested_mode, CompileCacheMode::Stateless);
    assert_eq!(response.actual_mode, CompileCacheMode::Stateless);
    assert!(response.downgrade_reason.is_none());

    // NO session slot was published: the fact-signature accessor returns
    // None (a Session route would have published a slot here).
    assert!(
        host.compile_slot_fact_dep_signature("/A.vue", &profile)
            .is_none(),
        "Stateless mode must NOT publish a session compile slot"
    );
    assert!(
        !host.compile_slot_is_warm("/A.vue", &profile),
        "Stateless mode must never warm the session slot"
    );

    // NO content-addressed entry was published.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "Stateless mode must NOT publish a content-addressed entry"
    );
}

#[test]
fn stateless_repeated_compiles_never_warm() {
    // Several Stateless compiles in a row: each is fresh, none warms a
    // cache. The content-addressed store stays empty throughout.
    let host = host();
    upsert_vue(&host, "/B.vue", SIMPLE);
    let profile = stateless_profile();

    for _ in 0..3 {
        let r = host
            .get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some("/B.vue".to_string()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile.clone(),
            })
            .expect("stateless compile");
        assert_eq!(r.actual_mode, CompileCacheMode::Stateless);
        assert!(!host.compile_slot_is_warm("/B.vue", &profile));
        assert_eq!(host.compile_output_pure_content_entry_count(), 0);
    }
}
