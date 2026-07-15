//! Architecture guard (multi-file version-gating): a result whose touched provider
//! file or source-map changed MID-FLIGHT is DROPPED — and the gate is MULTI-FILE
//! aware via a PROJECT-BOUND capture (rename / find-references / definition return
//! spans across many files; a change to ANY project surface, not just the queried
//! one, drops the whole result). The engine/project epoch is captured BEFORE and
//! AFTER each request; an unchanged project surface set keeps the result.
//!
//! This is the project-bound external-TS-engine guard
//! `stale_generation_result_dropped`. It exercises the production project-bound
//! epoch capture + returned-path validation
//! ([`verter_lsp::external_ts_sync::RequestEpoch`]) over the single
//! [`ProviderSurfaceStore`]. Discriminating self-checks: a gate that captured only
//! the queried file (not the project), or that did not validate engine-discovered
//! returned paths, would fail the multi-file / returned-path tests.

use std::sync::Arc;

use verter_lsp::carrier_cache::{EngineRecheckState, RegenKey};
use verter_lsp::external_ts_sync::RequestEpoch;
use verter_lsp::provider_surface_store::{
    ProviderSurfaceKind, ProviderSurfaceStore, RecordSurface,
};

const TSCONFIG: &str = "/proj/tsconfig.json";

fn regen() -> RegenKey {
    RegenKey {
        source_content_hash: [1u8; 16],
        parse_env_hash: [2u8; 16],
        compile_profile_hash: 7,
        file_language_row_hash: [3u8; 16],
        helper_runtime_version: 1,
    }
}

fn recheck() -> EngineRecheckState {
    EngineRecheckState {
        import_signature_hash: [9u8; 16],
        closure_generation: 5,
        project_recheck_generation: 1,
    }
}

fn record(
    store: &ProviderSurfaceStore,
    provider_path: &str,
    source: &str,
    content: &str,
    map_hash: [u8; 16],
) {
    store.record(RecordSurface {
        provider_path: provider_path.to_string(),
        kind: ProviderSurfaceKind::CarrierIde,
        source_canonical: source.to_string(),
        provider_content: Arc::from(content),
        source_map: None,
        carrier_source: Arc::from("<source>"),
        map_hash,
        project_owner: Some(Arc::from(TSCONFIG)),
        regen_key: Some(regen()),
        engine_recheck: Some(recheck()),
    });
}

#[test]
fn generation_advance_midflight_drops_result() {
    let store = ProviderSurfaceStore::new();
    let companion = "/proj/src/App.vue.tsx";
    record(&store, companion, "/proj/src/App.vue", "v1\n", [0x42u8; 16]);

    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    record(&store, companion, "/proj/src/App.vue", "v2\n", [0x42u8; 16]);
    let after = RequestEpoch::capture_project(&store, TSCONFIG);

    assert!(
        !RequestEpoch::result_is_fresh(&before, &after),
        "a generation advance between before/after must DROP the result"
    );
}

#[test]
fn unchanged_epoch_keeps_result() {
    let store = ProviderSurfaceStore::new();
    record(
        &store,
        "/proj/src/App.vue.tsx",
        "/proj/src/App.vue",
        "v1\n",
        [0x42u8; 16],
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    let after = RequestEpoch::capture_project(&store, TSCONFIG);
    assert!(
        RequestEpoch::result_is_fresh(&before, &after),
        "an unchanged project surface set keeps the result (no spurious drop)"
    );
}

#[test]
fn multi_file_drop_when_any_non_queried_project_surface_changes() {
    // The DISCRIMINATING case: a result spans A (queried), B, and C (Svelte). Only
    // C — NOT the queried A — changes mid-flight. A gate capturing only the queried
    // file would WRONGLY keep the result; the PROJECT-BOUND capture drops it.
    let store = ProviderSurfaceStore::new();
    record(
        &store,
        "/proj/src/A.vue.tsx",
        "/proj/src/A.vue",
        "a\n",
        [0x42u8; 16],
    );
    record(
        &store,
        "/proj/src/B.vue.tsx",
        "/proj/src/B.vue",
        "b\n",
        [0x42u8; 16],
    );
    record(
        &store,
        "/proj/src/C.svelte.tsx",
        "/proj/src/C.svelte",
        "c\n",
        [0x42u8; 16],
    );

    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    assert_eq!(
        before.captured_len(),
        3,
        "project capture covers every owned surface"
    );
    record(
        &store,
        "/proj/src/C.svelte.tsx",
        "/proj/src/C.svelte",
        "c2\n",
        [0x42u8; 16],
    );
    let after = RequestEpoch::capture_project(&store, TSCONFIG);

    assert!(
        !RequestEpoch::result_is_fresh(&before, &after),
        "a multi-file result must be dropped when ANY project surface (not just the queried \
         one) changed mid-flight"
    );
}

#[test]
fn map_hash_change_midflight_drops_result() {
    let store = ProviderSurfaceStore::new();
    let companion = "/proj/src/App.vue.tsx";
    record(
        &store,
        companion,
        "/proj/src/App.vue",
        "same\n",
        [0x11u8; 16],
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    record(
        &store,
        companion,
        "/proj/src/App.vue",
        "same\n",
        [0x22u8; 16],
    );
    let after = RequestEpoch::capture_project(&store, TSCONFIG);
    assert!(
        !RequestEpoch::result_is_fresh(&before, &after),
        "a map_hash change mid-flight must drop the result (never remap through a new map)"
    );
}

#[test]
fn surface_closed_midflight_drops_result() {
    let store = ProviderSurfaceStore::new();
    let companion = "/proj/src/App.vue.tsx";
    record(&store, companion, "/proj/src/App.vue", "v1\n", [0x42u8; 16]);
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    let _token = store.forget(companion);
    let after = RequestEpoch::capture_project(&store, TSCONFIG);
    assert!(
        !RequestEpoch::result_is_fresh(&before, &after),
        "a project surface closed mid-flight must drop the result"
    );
}

#[test]
fn surface_appearing_midflight_drops_result() {
    let store = ProviderSurfaceStore::new();
    record(
        &store,
        "/proj/src/A.vue.tsx",
        "/proj/src/A.vue",
        "a\n",
        [0x42u8; 16],
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    record(
        &store,
        "/proj/src/New.vue.tsx",
        "/proj/src/New.vue",
        "n\n",
        [0x42u8; 16],
    );
    let after = RequestEpoch::capture_project(&store, TSCONFIG);
    assert!(
        !RequestEpoch::result_is_fresh(&before, &after),
        "a project surface appearing mid-flight must drop the result"
    );
}

// ── Returned-path validation (engine-discovered touched set) ──────────────────

#[test]
fn returned_companion_path_absent_from_before_fails_closed() {
    // The realistic broken-integration case: a result's touched set is discovered
    // from the engine response. A returned COMPANION path that was not captured
    // before the request (a surface that appeared mid-flight) must fail closed.
    let store = ProviderSurfaceStore::new();
    record(
        &store,
        "/proj/src/A.vue.tsx",
        "/proj/src/A.vue",
        "a\n",
        [0x42u8; 16],
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    record(
        &store,
        "/proj/src/B.vue.tsx",
        "/proj/src/B.vue",
        "b\n",
        [0x42u8; 16],
    );

    let returned: Vec<Arc<str>> = vec![
        Arc::from("/proj/src/A.vue.tsx"),
        Arc::from("/proj/src/B.vue.tsx"),
    ];
    assert!(
        !before.returned_paths_all_fresh(&store, &returned),
        "a returned companion path absent from the before-capture must fail closed"
    );
}

#[test]
fn returned_carrier_api_companion_absent_from_before_fails_closed() {
    // A `.vue.verter.ts` CarrierApi companion that a `.vue.tsx`/`.svelte.tsx` suffix
    // heuristic would MISCLASSIFY as external. The STORE authority classifies it as
    // a companion, so it fails closed.
    let store = ProviderSurfaceStore::new();
    record(
        &store,
        "/proj/src/A.vue.tsx",
        "/proj/src/A.vue",
        "a\n",
        [0x42u8; 16],
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    // A CarrierApi surface appears mid-flight (recorded with the CarrierApi kind).
    store.record(RecordSurface {
        provider_path: "/proj/src/B.vue.verter.ts".to_string(),
        kind: ProviderSurfaceKind::CarrierApi,
        source_canonical: "/proj/src/B.vue".to_string(),
        provider_content: Arc::from("b\n"),
        source_map: None,
        carrier_source: Arc::from("<source>"),
        map_hash: [0x42u8; 16],
        project_owner: Some(Arc::from(TSCONFIG)),
        regen_key: Some(regen()),
        engine_recheck: Some(recheck()),
    });
    let returned: Vec<Arc<str>> = vec![
        Arc::from("/proj/src/A.vue.tsx"),
        Arc::from("/proj/src/B.vue.verter.ts"),
    ];
    assert!(
        !before.returned_paths_all_fresh(&store, &returned),
        "a returned .vue.verter.ts CarrierApi companion absent from the before-capture must \
         fail closed (the store is the companion authority, not a suffix check)"
    );
}

#[test]
fn returned_external_path_never_synced_is_fresh() {
    let store = ProviderSurfaceStore::new();
    record(
        &store,
        "/proj/src/A.vue.tsx",
        "/proj/src/A.vue",
        "a\n",
        [0x42u8; 16],
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    // The engine returns a span in A (captured) and a real util.ts the store NEVER
    // synced (a genuine external file).
    let returned: Vec<Arc<str>> = vec![
        Arc::from("/proj/src/A.vue.tsx"),
        Arc::from("/proj/src/util.ts"),
    ];
    assert!(
        before.returned_paths_all_fresh(&store, &returned),
        "a returned external .ts the store never synced is fresh (no project epoch)"
    );
}

#[test]
fn returned_captured_path_changed_fails_closed() {
    let store = ProviderSurfaceStore::new();
    record(
        &store,
        "/proj/src/A.vue.tsx",
        "/proj/src/A.vue",
        "a\n",
        [0x42u8; 16],
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    record(
        &store,
        "/proj/src/A.vue.tsx",
        "/proj/src/A.vue",
        "a2\n",
        [0x42u8; 16],
    );
    let returned: Vec<Arc<str>> = vec![Arc::from("/proj/src/A.vue.tsx")];
    assert!(
        !before.returned_paths_all_fresh(&store, &returned),
        "a returned captured companion whose epoch advanced must fail closed"
    );
}
