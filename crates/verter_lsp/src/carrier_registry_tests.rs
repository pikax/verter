//! Tests for the store-backed `CarrierRegistry` wiring (the contract-layer deferred wiring):
//! the contract's registry seam resolves against the SINGLE
//! [`ProviderSurfaceStore`], with no second store.

use std::sync::Arc;

use verter_session::external_ts::{CarrierRegistry, CarrierRole};

use super::*;
use crate::carrier_cache::{EngineRecheckState, RegenKey};
use crate::provider_surface_store::{ProviderSurfaceKind, RecordSurface};

const SOURCE: &str = "/src/Widget.vue";
const IDE_PATH: &str = "/src/Widget.vue.tsx";
const API_PATH: &str = "/src/Widget.vue.verter.ts";

fn regen_key() -> RegenKey {
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

/// Record a published surface of `kind` at `provider_path` directly into the
/// store (no host needed) with the full extended cache columns.
fn record(
    store: &ProviderSurfaceStore,
    provider_path: &str,
    kind: ProviderSurfaceKind,
    content: &str,
) {
    store.record(RecordSurface {
        provider_path: provider_path.to_string(),
        kind,
        source_canonical: SOURCE.to_string(),
        provider_content: Arc::from(content),
        source_map: None,
        carrier_source: Arc::from("<source>"),
        map_hash: [0x42u8; 16],
        project_owner: Some(Arc::from("/tsconfig.json")),
        regen_key: Some(regen_key()),
        engine_recheck: Some(recheck()),
    });
}

/// A descriptor-style path resolver mapping the source to its companion paths.
fn resolver() -> impl CarrierPathResolver {
    |source: &str, role: CarrierRole| -> Option<String> {
        if source != SOURCE {
            return None;
        }
        match role {
            CarrierRole::CarrierIde => Some(IDE_PATH.to_string()),
            CarrierRole::CarrierApi => Some(API_PATH.to_string()),
            _ => None,
        }
    }
}

#[test]
fn carrier_for_reads_ide_surface_from_store() {
    let store = ProviderSurfaceStore::new();
    record(
        &store,
        IDE_PATH,
        ProviderSurfaceKind::CarrierIde,
        "ide tsx\n",
    );

    let registry = StoreBackedCarrierRegistry::new(store, resolver());
    let artifact = registry
        .carrier_for(SOURCE)
        .expect("the IDE carrier must be served from the store");

    assert_eq!(&*artifact.provider_uri, IDE_PATH);
    assert_eq!(artifact.role, CarrierRole::CarrierIde);
    assert_eq!(&*artifact.content, "ide tsx\n");
    assert_eq!(artifact.map_hash, [0x42u8; 16]);
    // The artifact's content hash is the surface's 16-byte content identity.
    assert_ne!(artifact.content_hash, [0u8; 16]);
}

#[test]
fn carrier_for_role_reads_specific_role() {
    let store = ProviderSurfaceStore::new();
    record(&store, IDE_PATH, ProviderSurfaceKind::CarrierIde, "ide\n");
    record(&store, API_PATH, ProviderSurfaceKind::CarrierApi, "api\n");

    let registry = StoreBackedCarrierRegistry::new(store, resolver());

    let ide = registry
        .carrier_for_role(SOURCE, CarrierRole::CarrierIde)
        .expect("IDE role present");
    assert_eq!(&*ide.content, "ide\n");

    let api = registry
        .carrier_for_role(SOURCE, CarrierRole::CarrierApi)
        .expect("API role present");
    assert_eq!(&*api.content, "api\n");
    assert_eq!(&*api.provider_uri, API_PATH);
}

#[test]
fn unknown_source_has_no_carrier() {
    let store = ProviderSurfaceStore::new();
    record(&store, IDE_PATH, ProviderSurfaceKind::CarrierIde, "ide\n");
    let registry = StoreBackedCarrierRegistry::new(store, resolver());
    assert!(
        registry.carrier_for("/src/Other.vue").is_none(),
        "a source with no companion path resolves to no carrier"
    );
}

#[test]
fn role_kind_mismatch_fails_closed() {
    // The store has a CarrierApi surface at the IDE path (a mismatch). Asking for
    // the IDE role at that path must fail closed, never serve the wrong-role
    // surface.
    let store = ProviderSurfaceStore::new();
    record(
        &store,
        IDE_PATH,
        ProviderSurfaceKind::CarrierApi,
        "api at ide path\n",
    );
    let registry = StoreBackedCarrierRegistry::new(store, resolver());
    assert!(
        registry
            .carrier_for_role(SOURCE, CarrierRole::CarrierIde)
            .is_none(),
        "a role/kind mismatch must fail closed (single store is the authority)"
    );
}

#[test]
fn version_is_the_snapshot_generation() {
    let store = ProviderSurfaceStore::new();
    record(&store, IDE_PATH, ProviderSurfaceKind::CarrierIde, "v1\n");
    record(&store, IDE_PATH, ProviderSurfaceKind::CarrierIde, "v2\n");
    let registry = StoreBackedCarrierRegistry::new(store, resolver());
    let artifact = registry.carrier_for(SOURCE).expect("present");
    assert_eq!(&*artifact.content, "v2\n", "the current snapshot wins");
    assert!(artifact.version >= 1, "version is the monotonic generation");
}

/// Record a surface with a custom source_canonical + project_owner (for the
/// fail-closed gate tests).
fn record_custom(
    store: &ProviderSurfaceStore,
    provider_path: &str,
    kind: ProviderSurfaceKind,
    source_canonical: &str,
    project_owner: Option<&str>,
) {
    store.record(RecordSurface {
        provider_path: provider_path.to_string(),
        kind,
        source_canonical: source_canonical.to_string(),
        provider_content: Arc::from("content\n"),
        source_map: None,
        carrier_source: Arc::from("<source>"),
        map_hash: [0x42u8; 16],
        project_owner: project_owner.map(Arc::from),
        regen_key: Some(regen_key()),
        engine_recheck: Some(recheck()),
    });
}

#[test]
fn carrier_batch_request_resolves_to_the_merged_ide_surface() {
    // MERGE: a CarrierBatch request reads the CarrierIde slot (no distinct batch
    // slot). The resolver only knows the IDE path; the registry's merge alias maps
    // the batch request onto it.
    let store = ProviderSurfaceStore::new();
    record(&store, IDE_PATH, ProviderSurfaceKind::CarrierIde, "ide\n");
    let registry = StoreBackedCarrierRegistry::new(store, resolver());

    let batch = registry
        .carrier_for_role(SOURCE, CarrierRole::CarrierBatch)
        .expect("a CarrierBatch request resolves to the merged CarrierIde surface");
    assert_eq!(
        &*batch.provider_uri, IDE_PATH,
        "served from the IDE slot (merge)"
    );
    assert_eq!(&*batch.content, "ide\n");
    assert_eq!(
        batch.role,
        CarrierRole::CarrierBatch,
        "the artifact reports the requested batch role over the merged IDE surface"
    );
}

#[test]
fn source_canonical_mismatch_fails_closed() {
    // A resolver/path bug points the IDE companion at a surface recorded for a
    // DIFFERENT source. The registry must NOT serve another source's carrier.
    let store = ProviderSurfaceStore::new();
    record_custom(
        &store,
        IDE_PATH,
        ProviderSurfaceKind::CarrierIde,
        "/src/OtherSource.vue", // belongs to a different source
        Some("/tsconfig.json"),
    );
    let registry = StoreBackedCarrierRegistry::new(store, resolver());
    assert!(
        registry.carrier_for(SOURCE).is_none(),
        "a surface whose source_canonical does not match the requested source must \
         fail closed (never serve another source's carrier)"
    );
}

#[test]
fn missing_project_owner_fails_closed() {
    // A surface recorded WITHOUT a project owner (e.g. a legacy rename-mapping
    // record) must NOT leak into the project-bound contract.
    let store = ProviderSurfaceStore::new();
    record_custom(
        &store,
        IDE_PATH,
        ProviderSurfaceKind::CarrierIde,
        SOURCE,
        None, // no project owner
    );
    let registry = StoreBackedCarrierRegistry::new(store, resolver());
    assert!(
        registry.carrier_for(SOURCE).is_none(),
        "a surface with no project owner must fail closed in the project-bound \
         contract (a legacy record must not leak in)"
    );
}
