//! Matrix slice: `app_config_proof` × `route_surface`.
//!
//! Degenerate cell — the AppConfigNoOverrideProofDb producer
//! observes only the decl canonical's `FileWholeHash`. It does not
//! consult any RouteDb-backed effective-export surface.
//!
//! Discrimination: the published `fact_dep_signature` contains no
//! `RouteSurface(RouteSurfaceFactRef)` entries.

use std::sync::Arc;

use verter_session::for_tests::app_config_no_override_proof_get_or_compute_for_tests;
use verter_session::resolver_core::FactVersionRef;

#[test]
fn app_config_proof_does_not_observe_route_surface_facts() {
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

    assert!(
        !proof
            .fact_dep_signature
            .iter()
            .any(|f| matches!(f, FactVersionRef::RouteSurface(_))),
        "AppConfigNoOverrideProofDb producer must NOT observe RouteSurface facts"
    );
}
