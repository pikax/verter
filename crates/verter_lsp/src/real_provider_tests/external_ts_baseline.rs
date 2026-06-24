//! RED baseline fixtures for the project-bound external-TS engine.
//!
//! These real-provider tests encode the configured-project-correct behaviour the
//! OLD config-less inferred overlay FAILS: a carrier placed in an inferred
//! project never sees the real `tsconfig.json` (`paths`/`baseUrl`/`types`/`lib`/
//! `jsx`/`moduleResolution`/project references), so it emits false `TS2307`
//! (unresolved `@/`-aliased import) and `TS2304` (unknown ambient global).
//!
//! Every test here is `#[ignore]`d so the canonical gate stays GREEN: they are
//! the RED baseline that goes green when the tsserver backend lands and makes
//! the carrier a real member of the configured project. Un-ignoring them is that
//! backend's green gate. They are REAL fixtures + REAL assertions (not stubs):
//! the assertions describe the correct configured-project outcome, so they FAIL
//! against today's inferred path and PASS once the contract is wired live.
//!
//! The hermetic skip is intrinsic: `TestSessionBuilder::build()` returns `None`
//! (and the test returns early) when the provider binary / `node_modules` is
//! absent, so a developer machine without the toolchain never sees a spurious
//! failure even if the test is run with `--ignored`.

use tower_lsp_server::ls_types::Diagnostic;

use crate::test_harness::{RealProviderTestSession, TestProviderKind, TestSessionBuilder};

const FIXTURE: &str = "external-ts-engine";

/// Does `diags` contain a diagnostic with the given numeric TS code?
fn has_code(diags: &[Diagnostic], code: &str) -> bool {
    diags.iter().any(|d| match &d.code {
        Some(tower_lsp_server::ls_types::NumberOrString::Number(n)) => n.to_string() == code,
        Some(tower_lsp_server::ls_types::NumberOrString::String(s)) => s == code,
        None => false,
    })
}

/// Assert the configured-project-correct diagnostics for a carrier: the
/// `@/`-aliased import resolves (no `TS2307`) and the ambient global resolves
/// (no `TS2304`). This is the behaviour the inferred path cannot produce.
async fn assert_carrier_resolves_configured(session: &RealProviderTestSession, relative: &str) {
    let uri = session.open_fixture_file(relative).await;
    let diags = session.merged_diagnostics(&uri).await;

    assert!(
        !has_code(&diags, "2307"),
        "{relative}: a path-aliased `@/` import must resolve under the configured \
         project — no false TS2307 (the carrier is a real project member). Diags: {diags:?}"
    );
    assert!(
        !has_code(&diags, "2304"),
        "{relative}: the ambient global reached via tsconfig `types`/`typeRoots` must \
         be in scope — no TS2304. Diags: {diags:?}"
    );
}

// ── paths / baseUrl / types / lib / jsx / moduleResolution (one cross-section) ──

#[tokio::test(flavor = "multi_thread")]
#[ignore = "RED baseline for the project-bound external-TS engine; goes green once the tsserver backend makes the carrier a configured-project member"]
async fn vue_carrier_resolves_aliased_import_and_ambient_under_configured_project_tsserver() {
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture(FIXTURE)
        .build()
        .await
    else {
        return;
    };
    assert_carrier_resolves_configured(&session, "src/AliasConsumer.vue").await;
    session.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "RED baseline for the project-bound external-TS engine; goes green once the tsserver backend makes the carrier a configured-project member"]
async fn vue_carrier_resolves_aliased_import_and_ambient_under_configured_project_tsgo() {
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsgo)
        .fixture(FIXTURE)
        .build()
        .await
    else {
        return;
    };
    assert_carrier_resolves_configured(&session, "src/AliasConsumer.vue").await;
    session.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "RED baseline for the project-bound external-TS engine; goes green once the tsserver backend makes the carrier a configured-project member"]
async fn svelte_carrier_resolves_aliased_import_and_ambient_under_configured_project_tsserver() {
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture(FIXTURE)
        .build()
        .await
    else {
        return;
    };
    assert_carrier_resolves_configured(&session, "src/AliasConsumer.svelte").await;
    session.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "RED baseline for the project-bound external-TS engine; goes green once the tsserver backend makes the carrier a configured-project member"]
async fn svelte_carrier_resolves_aliased_import_and_ambient_under_configured_project_tsgo() {
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsgo)
        .fixture(FIXTURE)
        .build()
        .await
    else {
        return;
    };
    assert_carrier_resolves_configured(&session, "src/AliasConsumer.svelte").await;
    session.shutdown().await;
}

// ── project references / solution-style (existing fixtures) ──

#[tokio::test(flavor = "multi_thread")]
#[ignore = "RED baseline for the project-bound external-TS engine; goes green once the tsserver backend makes the carrier a configured-project member"]
async fn carrier_under_solution_style_leaf_resolves_tsserver() {
    // `tsconfig-references` is a solution-style root (`files: [], references: […]`)
    // whose leaf `tsconfig.app.json` owns the `.vue`. Under the inferred path the
    // carrier is not a leaf-project member; under the configured backend it is.
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture("tsconfig-references")
        .build()
        .await
    else {
        return;
    };
    let uri = session.open_fixture_file("src/App.vue").await;
    let diags = session.merged_diagnostics(&uri).await;
    assert!(
        !has_code(&diags, "2307"),
        "solution-style leaf must own the carrier so its imports resolve — no false \
         TS2307. Diags: {diags:?}"
    );
    session.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "RED baseline for the project-bound external-TS engine; goes green once the tsserver backend makes the carrier a configured-project member"]
async fn carrier_in_multiroot_monorepo_resolves_cross_package_tsserver() {
    // `monorepo` has two referenced leaves (`packages/app`, `packages/shared`).
    // A cross-package import from the app carrier resolves only when the carrier
    // is a member of the configured app project with its references wired.
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture("monorepo")
        .build()
        .await
    else {
        return;
    };
    let uri = session.open_fixture_file("packages/app/src/App.vue").await;
    let diags = session.merged_diagnostics(&uri).await;
    assert!(
        !has_code(&diags, "2307"),
        "a multi-root monorepo carrier must resolve its cross-package import under \
         the configured project — no false TS2307. Diags: {diags:?}"
    );
    session.shutdown().await;
}
