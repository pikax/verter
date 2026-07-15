//! Matrix slice: `app_config_proof` × `member`.
//!
//! Degenerate cell — same rationale as
//! `app_config_proof_member_presence.rs`. The producer never walks
//! the AppConfig interface body's member facts.
//!
//! Discrimination: the cold compute advances installs by exactly 1
//! and admits a non-empty fact_dep_signature whose entries are only
//! `FileWholeHash` (no `Parse` per-member-body facts).

use std::sync::Arc;

use verter_session::for_tests::app_config_no_override_proof_get_or_compute_for_tests;
use verter_session::resolver_core::FactVersionRef;

#[test]
fn app_config_proof_does_not_observe_member_body_facts() {
    let canonical = "/proj/no-app-config.ts";
    let host = super::harness::make_host(canonical, "export type Foo = { theme: string };");

    let key: verter_session::app_config_proof_db::AppConfigNoOverrideProofKey =
        (Arc::from(canonical), Arc::from("button"));

    let installs_before = super::harness::read_app_config_proof_installs(&host);
    let proof = app_config_no_override_proof_get_or_compute_for_tests(&host, &key)
        .expect("file without `interface AppConfig` must yield a published proof");
    let installs_after = super::harness::read_app_config_proof_installs(&host);

    assert_eq!(
        installs_after - installs_before,
        1,
        "cold compute must advance installs by 1"
    );

    // Producer observes ONLY the
    // `FileWholeHash` of the decl canonical, not per-member body
    // facts. Verify the signature is non-empty AND contains no
    // member-body Parse facts.
    assert!(
        !proof.fact_dep_signature.is_empty(),
        "published proof must carry a non-empty fact_dep_signature"
    );
    assert!(
        !proof
            .fact_dep_signature
            .iter()
            .any(|f| matches!(f, FactVersionRef::Parse(_))),
        "AppConfigNoOverrideProofDb producer must NOT observe member-body Parse facts"
    );
}
