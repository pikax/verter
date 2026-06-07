//! Matrix slice: `app_config_proof` × `import_ref`.
//!
//! Degenerate cell — the AppConfigNoOverrideProofDb producer
//! observes only the decl canonical's `FileWholeHash`. It does not
//! walk import declarations or resolve any cross-file edge.
//!
//! Discrimination: the published `fact_dep_signature` contains a
//! single `FileWholeHash` for the decl canonical and no
//! `ResolveImports(ResolveImportsFactRef)` entries.

use std::sync::Arc;

use verter_session::for_tests::app_config_no_override_proof_get_or_compute_for_tests;
use verter_session::resolver_core::FactVersionRef;

#[test]
fn app_config_proof_does_not_observe_import_ref_facts() {
    let canonical = "/proj/no-app-config.ts";
    let host = super::harness::make_host(
        canonical,
        "import { something } from '/proj/dep';\nexport type Foo = { theme: string };",
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

    assert!(
        !proof
            .fact_dep_signature
            .iter()
            .any(|f| matches!(f, FactVersionRef::ResolveImports(_))),
        "AppConfigNoOverrideProofDb producer must NOT observe ResolveImports facts"
    );
}
