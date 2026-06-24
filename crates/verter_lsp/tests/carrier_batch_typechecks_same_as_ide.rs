//! Architecture guard (§2.1 / §2.7): `carrier_batch_typechecks_same_as_ide`.
//!
//! The leaf batch carrier and `CarrierIde` MUST produce the SAME diagnostic set,
//! so the cold TSC path can never silently drop a type-observable
//! template-expression construct. This gate is asserted where the batch carrier is
//! decided/stored and is RE-RUN on the perf corpus later.
//!
//! The measurement decided to MERGE `CarrierBatch` into `CarrierIde` (no distinct
//! codegen profile — see `crates/verter_compiler/tests/carrier_batch_measurement.rs`,
//! which proves on a rich SFC that the diagnostic-bearing surface is byte-identical
//! across the only batch-relevant toggle and that no valid diagnostic-preserving
//! minimal profile exists today). Under MERGE the gate holds "trivially (one
//! surface)" — but trivial is NOT a no-op: the invariant this guard pins is that
//! there is NO distinct `CarrierBatch` storage slot, and a `CarrierBatch` request
//! RESOLVES TO the single `CarrierIde` surface, so the cold batch path and the
//! interactive IDE path read the SAME stored carrier and CANNOT diverge in
//! diagnostics.
//!
//! This guard proves the REAL merged path through the live registry: ONE store
//! holding ONE `CarrierIde` snapshot, and a `CarrierBatch` lookup that returns
//! that exact snapshot's content + identity. It would FAIL if the registry could
//! not serve the merged role, or served different content. The codegen-side
//! same-diagnostic-set proof on real TSX is the companion `verter_compiler`
//! measurement cross-referenced above.

use std::sync::Arc;

use verter_lsp::carrier_cache::{EngineRecheckState, RegenKey};
use verter_lsp::carrier_registry::{CarrierPathResolver, StoreBackedCarrierRegistry};
use verter_lsp::provider_surface_store::{
    ProviderSurfaceKind, ProviderSurfaceStore, RecordSurface,
};
use verter_session::external_ts::CarrierRole;

const SOURCE: &str = "/src/Widget.vue";
// Under MERGE the batch carrier reads the IDE carrier path/identity.
const IDE_PATH: &str = "/src/Widget.vue.tsx";
// The single shared carrier surface text (one surface, read by both paths).
const CARRIER_TSX: &str = "/* one merged carrier surface */\nexport default {} as any;\n";

fn shared_regen() -> RegenKey {
    RegenKey {
        source_content_hash: [0xAB; 16],
        parse_env_hash: [0x10; 16],
        compile_profile_hash: 7,
        file_language_row_hash: [0x20; 16],
        helper_runtime_version: 1,
    }
}

fn recheck() -> EngineRecheckState {
    EngineRecheckState {
        import_signature_hash: [0x55; 16],
        closure_generation: 5,
        project_recheck_generation: 42,
    }
}

/// Record exactly ONE `CarrierIde` surface for the source (the merged carrier).
fn record_single_ide_surface(store: &ProviderSurfaceStore) {
    store.record(RecordSurface {
        provider_path: IDE_PATH.to_string(),
        kind: ProviderSurfaceKind::CarrierIde,
        source_canonical: SOURCE.to_string(),
        provider_content: Arc::from(CARRIER_TSX),
        source_map: None,
        carrier_source: Arc::from("<source>\n"),
        map_hash: [0x42; 16],
        project_owner: Some(Arc::from("/tsconfig.json")),
        regen_key: Some(shared_regen()),
        engine_recheck: Some(recheck()),
    });
}

/// A descriptor-style resolver: the IDE role resolves to the IDE companion path.
/// (The registry's merge alias maps a `CarrierBatch` request to the `CarrierIde`
/// slot, so the resolver is asked for the effective IDE role and returns the IDE
/// path — proving batch and IDE land on the SAME surface.)
fn resolver() -> impl CarrierPathResolver {
    |source: &str, role: CarrierRole| -> Option<String> {
        if source != SOURCE {
            return None;
        }
        match role {
            CarrierRole::CarrierIde => Some(IDE_PATH.to_string()),
            _ => None,
        }
    }
}

#[test]
fn batch_request_resolves_to_the_single_ide_surface() {
    // ONE store, ONE CarrierIde snapshot. A CarrierBatch request must resolve to
    // that exact surface (merge: no distinct slot) — proving the cold batch path
    // reads precisely what the interactive IDE path reads, so they cannot produce
    // different diagnostics.
    let store = ProviderSurfaceStore::new();
    record_single_ide_surface(&store);

    // There is NO distinct CarrierBatch slot in the store.
    assert!(
        store.current_snapshot(IDE_PATH).is_some(),
        "the single CarrierIde surface is stored"
    );

    let registry = StoreBackedCarrierRegistry::new(store, resolver());

    let ide = registry
        .carrier_for_role(SOURCE, CarrierRole::CarrierIde)
        .expect("the IDE carrier resolves");
    let batch = registry
        .carrier_for_role(SOURCE, CarrierRole::CarrierBatch)
        .expect("the batch carrier resolves to the merged IDE surface");

    // Byte-identical served content (the cold TSC path reads exactly the IDE
    // surface) and identical content/map identity — they are ONE surface.
    assert_eq!(
        &*ide.content, &*batch.content,
        "the merged batch carrier must serve byte-identical content to the IDE \
         carrier — it can never type-check a different surface"
    );
    assert_eq!(
        ide.content_hash, batch.content_hash,
        "shared content identity"
    );
    assert_eq!(ide.map_hash, batch.map_hash, "shared map identity");
    assert_eq!(
        ide.version, batch.version,
        "both read the SAME stored snapshot generation"
    );
    assert_eq!(
        &*ide.provider_uri, IDE_PATH,
        "the IDE artifact is served from the IDE companion path"
    );
    assert_eq!(
        &*batch.provider_uri, IDE_PATH,
        "the batch artifact is served from the SAME IDE companion path (merge)"
    );
}

#[test]
fn batch_reports_its_requested_role_over_the_merged_surface() {
    // The artifact reports the REQUESTED role (the caller asked for batch) while
    // reading the IDE-stored content — they are one surface, but the caller's
    // intent is preserved for downstream profile selection.
    let store = ProviderSurfaceStore::new();
    record_single_ide_surface(&store);
    let registry = StoreBackedCarrierRegistry::new(store, resolver());

    let batch = registry
        .carrier_for_role(SOURCE, CarrierRole::CarrierBatch)
        .expect("batch resolves");
    assert_eq!(
        batch.role,
        CarrierRole::CarrierBatch,
        "the artifact reports the requested batch role over the merged IDE surface"
    );
    let ide = registry
        .carrier_for_role(SOURCE, CarrierRole::CarrierIde)
        .expect("ide resolves");
    assert_eq!(ide.role, CarrierRole::CarrierIde);
}
