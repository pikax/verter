//! Matrix slice: `app_config_proof` × `module_augmentation_index_shape`.
//!
//! Degenerate cell — the AppConfigNoOverrideProofDb producer
//! observes only the decl canonical's `FileWholeHash`. Module
//! augmentation index facts are not part of its substrate contract.
//!
//! Discrimination: the published `fact_dep_signature` is a single
//! `FileWholeHash` entry without any augmentation-index references.

use std::sync::Arc;

use verter_session::for_tests::app_config_no_override_proof_get_or_compute_for_tests;
use verter_session::resolver_core::FactVersionRef;

#[test]
fn app_config_proof_does_not_observe_module_augmentation_index_facts() {
    let canonical = "/proj/no-app-config.ts";
    let host = super::harness::make_host(
        canonical,
        "declare module '@nuxt/schema' { interface Foo {} }\nexport type X = string;",
    );

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

    // Module augmentation index facts surface as either Parse-domain
    // facts (per-file augmentation declarations) or DerivedFactHash
    // facts (route-domain). The producer must observe neither.
    for fact in proof.fact_dep_signature.iter() {
        assert!(
            matches!(fact, FactVersionRef::FileWholeHash { .. }),
            "AppConfigNoOverrideProofDb producer must observe only FileWholeHash facts. \
             Got non-WholeHash fact: {fact:?}",
        );
    }
}
