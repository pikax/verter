//! RED baseline fixtures for the project-bound external-TS engine.
//!
//! These real-provider tests encode the configured-project-correct behaviour the
//! OLD config-less inferred overlay FAILS: a carrier placed in an inferred
//! project never sees the real `tsconfig.json` (`paths`/`baseUrl`/`types`/`lib`/
//! `jsx`/`moduleResolution`/project references), so it emits false `TS2307`
//! (unresolved `@/`-aliased import) and `TS2304` (unknown ambient global).
//!
//! The `*_tsserver` tests are LIVE: the tsserver carrier-publish path makes the
//! carrier a real member of its configured project (the LSP publishes the carrier
//! companions into the on-disk store the `@verter/typescript-plugin` reads, and
//! the plugin advertises them via `getExternalFiles` under the `extraFileExtensions`
//! the LSP configures), so the aliased import + ambient global resolve. The
//! `*_tsgo` tests stay `#[ignore]`d until the tgo engine is migrated onto the same
//! contract. They are REAL fixtures + REAL assertions (not stubs): the assertions
//! describe the correct configured-project outcome.
//!
//! The hermetic skip is intrinsic: `TestSessionBuilder::build()` returns `None`
//! (and the test returns early) when the provider binary / `node_modules` is
//! absent, so a developer machine without the toolchain never sees a spurious
//! failure even if the test is run with `--ignored`.
//!
//! The two solution-style / multi-root `*_tsserver` tests below characterize the
//! LEAF-binding cases (a leaf config not named `tsconfig.json`, and a cross-package
//! dependency carrier). Their consumers are VUE-FREE (mirroring
//! `external-ts-engine/AliasConsumer.vue`) so the ONLY resolution that can produce
//! a TS2307 is the leaf-binding mechanism, not a missing `vue` dependency. They
//! resolve TODAY under the diagnostics cold-membership recovery (the `reloadProjects`
//! lever forces the referenced leaf projects to load and re-advertise their
//! `getExternalFiles`), so they PASS when run with `--ignored`; they are kept
//! `#[ignore]`d only pending confirmation that gating on that lever is intended.

use tower_lsp_server::ls_types::Diagnostic;

use crate::provider_sync::{ProviderOwnerBinding, ProviderPathKind, ProviderSyncState};
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

// ── POSITIVE carrier-diagnostic PRESENCE (not absence) ──
//
// The `*_resolves_*` baselines above are ABSENCE checks (no false TS2307/TS2304)
// — they pass vacuously even if the carrier yields ZERO diagnostics. This test
// is the PRESENCE counterpart: a `.vue` carrier with a DELIBERATE TS2322 (a
// string assigned to a `number` in `<script setup>`) must surface that exact
// diagnostic on the carrier's `.vue` SOURCE. It is the end-to-end proof that
// carrier diagnostics positively flow under the project-bound engine WITHOUT a
// contentful companion open: the LSP publishes the carrier, registers it with
// its owning configured project (`projectFileName`) + a contentless
// project-load open, and `semanticDiagnosticsSync` reports TS2322 mapped back
// through the carrier source map. Reverting the project-targeting / membership
// signal makes `semanticDiagnosticsSync` return empty (or `No Project`) and this
// PRESENCE assertion fails — discriminating.
#[tokio::test(flavor = "multi_thread")]
async fn vue_carrier_surfaces_semantic_type_error_on_source_tsserver() {
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture(FIXTURE)
        .build()
        .await
    else {
        return;
    };
    let uri = session.open_fixture_file("src/TypeErrorCarrier.vue").await;
    let diags = session.merged_diagnostics(&uri).await;

    // TS2322 = "Type 'string' is not assignable to type 'number'". Assert by code
    // (stable across tsserver/TSGO), falling back to the message text.
    assert!(
        has_code(&diags, "2322") || diags.iter().any(|d| d.message.contains("not assignable")),
        "a carrier `<script setup>` type error must surface TS2322 on the `.vue` source \
         (carrier diagnostics positively flow); got: {diags:?}"
    );

    // The diagnostic must land on the `typedNumber` declaration in the `.vue`
    // SOURCE — proving the carrier's TSX diagnostic was mapped back through the
    // source map, not left in companion coordinates. The decl is on the
    // `const typedNumber: number = ...` line.
    let decl = session.find_position(&uri, "typedNumber", 0);
    let on_decl_line = diags.iter().any(|d| {
        (has_code(std::slice::from_ref(d), "2322") || d.message.contains("not assignable"))
            && d.range.start.line <= decl.line
            && d.range.end.line >= decl.line
    });
    assert!(
        on_decl_line,
        "the TS2322 diagnostic must map back to the `typedNumber` decl line {} in the \
         `.vue` source; got: {diags:?}",
        decl.line
    );

    session.shutdown().await;
}

// ── The TS2322 PRESENCE proof TIED to the provider-sync ownership backbone ──
//
// The presence proof above proves carrier diagnostics flow; this companion proves
// WHY they flow — through `provider_sync.rs`, the provider-neutral ownership-state
// backbone (`ProviderSyncState`/`ProviderPathKind`/`ProviderOwnerBinding`) that the
// tsserver carrier-membership path (`external_ts/carrier_sync.rs`) commits when a
// carrier becomes a configured-project member. The carrier-sync gateway, on an
// `Advertised` membership, marks the carrier's `ProviderSyncState` `Owned` +
// background-loaded and commits it; that committed state is the precondition for the
// `.vue`'s TS2322 to surface. This test asserts BOTH halves on the SAME carrier:
//
//  * POSITIVE: the deliberate `string` → `number` error surfaces TS2322 on the
//    `.vue` SOURCE (carrier diagnostics positively flow), AND the carrier's
//    committed `ProviderSyncState` is `Owned` and IDE-background-loaded — the
//    membership the diagnostic flow rides.
//  * NEGATIVE: a CLEAN carrier (`AliasConsumer.vue`, no type error) does NOT report
//    TS2322 — the presence is the error carrier's real type error, not blanket noise;
//    and the error carrier's committed state is NOT the `Unresolved` (config-less /
//    no-owner) binding — proving the diagnostic did not arrive via an inferred
//    no-owner placement.
//
// Because the assertions reference the `provider_sync.rs` ownership types directly,
// deleting that backbone breaks this test at compile time — it is the positive proof
// that pins `provider_sync.rs` as a retained shared dependency of the tsserver path.
#[tokio::test(flavor = "multi_thread")]
async fn vue_carrier_semantic_type_error_rides_owned_provider_sync_membership_tsserver() {
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture(FIXTURE)
        .build()
        .await
    else {
        return;
    };

    // The carrier with the DELIBERATE TS2322 (`string` assigned to `number`).
    let err_uri = session.open_fixture_file("src/TypeErrorCarrier.vue").await;
    // A CLEAN carrier (resolves under the configured project; NO type error) — the
    // negative control proving TS2322 presence is the error carrier's real error.
    let clean_uri = session.open_fixture_file("src/AliasConsumer.vue").await;

    let err_diags = session.merged_diagnostics(&err_uri).await;

    // ── POSITIVE (a): the type error surfaces TS2322 on the `.vue` source. ──
    assert!(
        has_code(&err_diags, "2322")
            || err_diags
                .iter()
                .any(|d| d.message.contains("not assignable")),
        "the carrier `<script setup>` type error must surface TS2322 on the `.vue` \
         source (carrier diagnostics positively flow through the project-bound \
         engine); got: {err_diags:?}"
    );

    // ── POSITIVE (b): the carrier rode an OWNED, background-loaded provider-sync ──
    // membership — the `provider_sync.rs` backbone state the gateway commits when the
    // carrier joins its configured project. This is the GENUINE mechanism: the
    // tsserver carrier-sync gateway sets `Owned` + background-loaded on `Advertised`
    // and the diagnostic above can only flow once that membership is committed.
    let err_state: ProviderSyncState = session.provider_sync_state(&err_uri).expect(
        "a published `.vue` carrier whose diagnostics flow must have a committed \
         ProviderSyncState in the provider-sync ownership backbone",
    );
    assert!(
        matches!(err_state.owner_binding, ProviderOwnerBinding::Owned(_)),
        "the carrier's committed provider-sync state must be OWNED by its configured \
         project (the membership the TS2322 flow rides); got: {:?}",
        err_state.owner_binding
    );
    assert!(
        err_state.background_loaded_for_kind(ProviderPathKind::Ide),
        "the carrier's IDE companion must be background-loaded (store-resident) in the \
         committed provider-sync state — the published-member state the gateway \
         commits on `Advertised`; got: {err_state:?}"
    );

    // ── NEGATIVE (a): the OWNED state is NOT the config-less `Unresolved` binding. ──
    // A diagnostic arriving via an inferred no-owner placement would leave the state
    // `Unresolved`; this asserts the membership is a real owner, discriminating
    // against that failure mode.
    assert!(
        !err_state.is_unresolved(),
        "the carrier must NOT be in the Unresolved (no-owner / inferred) binding — the \
         TS2322 must ride a real configured-project membership; got: {err_state:?}"
    );

    // ── NEGATIVE (b): a CLEAN carrier does not report TS2322. ──
    let clean_diags = session.merged_diagnostics(&clean_uri).await;
    assert!(
        !has_code(&clean_diags, "2322"),
        "a clean carrier with no type error must NOT report TS2322 — the presence on \
         the error carrier is its real type error, not blanket diagnostic noise; \
         got: {clean_diags:?}"
    );

    session.shutdown().await;
}

// The SAME positive TS2322 PRESENCE proof, but built THROUGH the production
// `ResilientProvider` wrapper (`.resilient(true)`) — the seam the LSP binary
// actually installs (`try_spawn_tsserver`). The wrapper previously did NOT
// override `register_carrier_member`, so a published carrier registration fell
// through to the trait no-op and NEVER reached the inner tsserver provider: the
// carrier never joined its configured project and `semanticDiagnosticsSync`
// returned empty. The raw-provider baseline above could not catch this because it
// builds the bare `TsserverTypeProvider` (which has the real `register` impl). This
// test closes that coverage hole: it fails (no TS2322) on the pre-fix wrapper and
// passes once the wrapper forwards the registration to the inner provider.
#[tokio::test(flavor = "multi_thread")]
async fn vue_carrier_surfaces_semantic_type_error_through_resilient_provider_tsserver() {
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture(FIXTURE)
        .resilient(true)
        .build()
        .await
    else {
        return;
    };
    let uri = session.open_fixture_file("src/TypeErrorCarrier.vue").await;
    let diags = session.merged_diagnostics(&uri).await;

    assert!(
        has_code(&diags, "2322") || diags.iter().any(|d| d.message.contains("not assignable")),
        "a carrier `<script setup>` type error must surface TS2322 on the `.vue` source \
         THROUGH the ResilientProvider wrapper — the carrier registration must FORWARD to \
         the inner provider, not be swallowed by the trait no-op; got: {diags:?}"
    );

    session.shutdown().await;
}

// The OWNED one-instance dual-surface tsgo provider makes the carrier a member of
// its CONFIGURED project: diagnostics ride the attached `--api` checker holding
// the real `tsconfig.json` (the `paths` / `types` / `typeRoots` apply), so the
// `@/`-aliased import + ambient global resolve. The old inferred `rootUri`-only
// init could not apply the real tsconfig. (NON-vacuity is proven by the PRESENCE
// counterpart `vue_carrier_surfaces_semantic_type_error_on_source_tsgo` below —
// these absence checks alone would pass on an empty diagnostic set.)
#[tokio::test(flavor = "multi_thread")]
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

// PRESENCE counterpart for the OWNED tsgo provider (mirrors the tsserver presence
// proof above): a `.vue` carrier with a DELIBERATE TS2322 (`string` → `number` in
// `<script setup>`) MUST surface that exact diagnostic on the `.vue` SOURCE,
// proving carrier diagnostics POSITIVELY flow through the attached `--api` checker
// and map back through the carrier source map. This is the NON-VACUITY guard for
// the absence-only baselines: if the OWNED provider produced an empty diagnostic
// set (no program / wrong project), this PRESENCE assertion fails. Reverting the
// `--api` attach (the inferred-project path) makes `getSemanticDiagnostics` empty
// or mis-targeted and this test fails — discriminating.
#[tokio::test(flavor = "multi_thread")]
async fn vue_carrier_surfaces_semantic_type_error_on_source_tsgo() {
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsgo)
        .fixture(FIXTURE)
        .build()
        .await
    else {
        return;
    };
    let uri = session.open_fixture_file("src/TypeErrorCarrier.vue").await;
    let diags = session.merged_diagnostics(&uri).await;

    // TS2322 = "Type 'string' is not assignable to type 'number'". Assert by code
    // (stable across engines), falling back to the message text.
    assert!(
        has_code(&diags, "2322") || diags.iter().any(|d| d.message.contains("not assignable")),
        "a carrier `<script setup>` type error must surface TS2322 on the `.vue` source \
         through the OWNED tsgo `--api` checker (carrier diagnostics positively flow); \
         got: {diags:?}"
    );

    // The diagnostic must land on the `typedNumber` decl line in the `.vue` SOURCE,
    // proving the carrier's TSX diagnostic was mapped back through the source map.
    let decl = session.find_position(&uri, "typedNumber", 0);
    let on_decl_line = diags.iter().any(|d| {
        (has_code(std::slice::from_ref(d), "2322") || d.message.contains("not assignable"))
            && d.range.start.line <= decl.line
            && d.range.end.line >= decl.line
    });
    assert!(
        on_decl_line,
        "the TS2322 diagnostic must map back to the `typedNumber` decl line {} in the \
         `.vue` source; got: {diags:?}",
        decl.line
    );

    session.shutdown().await;
}

// ── project references / solution-style (existing fixtures) ──

// A solution-style root (`files: [], references: […]`) reaches its leaf
// `tsconfig.app.json` (a config NOT named `tsconfig.json`) for an opened carrier
// only once a configured-project load is forced. The diagnostics cold-membership
// recovery forces that load (the `reloadProjects` lever re-evaluates project
// structure so the leaf owns the companion), so the carrier's aliased import +
// ambient global resolve under the leaf with no false TS2307.
#[tokio::test(flavor = "multi_thread")]
async fn carrier_under_solution_style_leaf_resolves_tsserver() {
    // `tsconfig-references` is a solution-style root (`files: [], references: […]`)
    // whose leaf `tsconfig.app.json` owns the `.vue` and its `paths` / `types` /
    // `typeRoots`. The carrier consumer (`AliasConsumer.vue`) is VUE-FREE (mirrors
    // `external-ts-engine/AliasConsumer.vue`): a `@/`-aliased import plus an
    // ambient global, no `import "vue"` / `defineProps`, so the ONLY resolution
    // that can produce a TS2307 is the leaf-binding mechanism under test.
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture("tsconfig-references")
        .build()
        .await
    else {
        return;
    };
    let uri = session.open_fixture_file("src/AliasConsumer.vue").await;
    let diags = session.merged_diagnostics(&uri).await;
    assert!(
        !has_code(&diags, "2307"),
        "solution-style leaf must own the carrier so its imports resolve — no false \
         TS2307. Diags: {diags:?}"
    );
    session.shutdown().await;
}

// Two referenced leaves (`packages/app`, `packages/shared`). The app carrier's
// cross-package import + ambient global resolve once the referenced leaf
// configured project is loaded with its sibling-package `paths` wired. The
// diagnostics cold-membership recovery forces that leaf load (the `reloadProjects`
// lever), so the cross-package import resolves with no false TS2307.
#[tokio::test(flavor = "multi_thread")]
async fn carrier_in_multiroot_monorepo_resolves_cross_package_tsserver() {
    // `monorepo` has two referenced leaves (`packages/app`, `packages/shared`).
    // The app carrier consumer (`AliasConsumer.vue`) is VUE-FREE (mirrors
    // `external-ts-engine/AliasConsumer.vue`): a cross-package TS import plus an
    // ambient global, no `import "vue"` / `defineProps`, so the ONLY resolution
    // that can produce a TS2307 is the cross-package leaf-binding mechanism under
    // test.
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture("monorepo")
        .build()
        .await
    else {
        return;
    };
    let uri = session
        .open_fixture_file("packages/app/src/AliasConsumer.vue")
        .await;
    let diags = session.merged_diagnostics(&uri).await;
    assert!(
        !has_code(&diags, "2307"),
        "a multi-root monorepo carrier must resolve its cross-package import under \
         the configured project — no false TS2307. Diags: {diags:?}"
    );
    session.shutdown().await;
}
