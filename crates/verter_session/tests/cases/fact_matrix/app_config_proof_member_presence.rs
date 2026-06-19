//! Matrix slice: `app_config_proof` × `member_presence`.
//!
//! Degenerate cell — the AppConfigNoOverrideProofDb producer
//! observes only `FileWholeHash` for the decl canonical. It does
//! NOT walk per-member presence facts
//! because the `declares_interface_app_config` IndexedReady flag
//! is the only structural input the producer's substrate consults.
//!
//! Discrimination: the cold compute advances the producer's install
//! counter exactly once; the published `fact_dep_signature` contains
//! a `FileWholeHash` observation and NO `Parse(ParseFactRef)`
//! entries (which would be member-presence facts).

use std::sync::Arc;

use verter_session::for_tests::app_config_no_override_proof_get_or_compute_for_tests;
use verter_session::resolver_core::FactVersionRef;

#[test]
fn app_config_proof_does_not_observe_member_presence_facts() {
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

    let has_parse_fact = proof
        .fact_dep_signature
        .iter()
        .any(|f| matches!(f, FactVersionRef::Parse(_)));
    assert!(
        !has_parse_fact,
        "AppConfigNoOverrideProofDb producer must NOT observe Parse-domain (member-presence) facts. \
         Got: {:?}",
        proof.fact_dep_signature,
    );

    let has_whole = proof.fact_dep_signature.iter().any(|f| {
        matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id == canonical
        )
    });
    assert!(
        has_whole,
        "producer must observe FileWholeHash for the decl canonical. Got: {:?}",
        proof.fact_dep_signature,
    );
}
