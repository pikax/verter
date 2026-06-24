//! Architecture guard (the provisional-role measurement): `carrier_batch_role_stored_and_invalidated`.
//!
//! The provisional `CarrierBatch` role was MEASURED and the decision is MERGE into
//! `CarrierIde` (no distinct `compile_profile` / cache key — the measurement
//! showed no material cold-perf gain, and no valid diagnostic-preserving minimal
//! codegen profile exists today; see
//! `crates/verter_compiler/tests/carrier_batch_measurement.rs`). Per the spec this
//! guard "applies only if the measurement keeps `CarrierBatch` as a distinct role;
//! trivially satisfied if merged".
//!
//! "Trivially satisfied if merged" is NOT a no-op assertion. The honest, real form
//! of the guard under MERGE is: there is NO distinct `CarrierBatch` storage slot,
//! and a `CarrierBatch` request RESOLVES TO the single `CarrierIde` entry — so the
//! invalidation decisions (regeneration skip + dependency-driven engine re-check)
//! the cold batch path makes are read from the SHARED `CarrierIde` entry, never a
//! separate batch slot. This guard exercises exactly that through the live
//! registry: a `CarrierBatch` lookup reads the `CarrierIde` snapshot, and the
//! store's split-cache invalidation on that one shared entry is what both paths
//! observe.

use std::sync::Arc;

use verter_lsp::carrier_cache::{EngineRecheckState, RegenKey};
use verter_lsp::carrier_registry::{CarrierPathResolver, StoreBackedCarrierRegistry};
use verter_lsp::provider_surface_store::{
    ProviderSurfaceKind, ProviderSurfaceStore, RecordSurface,
};
use verter_session::external_ts::CarrierRole;

const SOURCE: &str = "/src/Widget.vue";
const IDE_PATH: &str = "/src/Widget.vue.tsx";
// A path the resolver would map a DISTINCT batch slot to, IF one existed. It must
// stay empty under MERGE — proving no separate batch slot is created/used.
const HYPOTHETICAL_BATCH_PATH: &str = "/src/Widget.vue.batch.tsx";

fn regen(source: u8) -> RegenKey {
    RegenKey {
        source_content_hash: [source; 16],
        parse_env_hash: [0x10; 16],
        compile_profile_hash: 7,
        file_language_row_hash: [0x20; 16],
        helper_runtime_version: 1,
    }
}

fn recheck(import_sig: u8, closure_gen: u64, project_gen: u64) -> EngineRecheckState {
    EngineRecheckState {
        import_signature_hash: [import_sig; 16],
        closure_generation: closure_gen,
        project_recheck_generation: project_gen,
    }
}

fn record_ide(store: &ProviderSurfaceStore, regen: RegenKey, recheck: EngineRecheckState) {
    store.record(RecordSurface {
        provider_path: IDE_PATH.to_string(),
        kind: ProviderSurfaceKind::CarrierIde,
        source_canonical: SOURCE.to_string(),
        provider_content: Arc::from("ide tsx\n"),
        source_map: None,
        carrier_source: Arc::from("<source>\n"),
        map_hash: [0x42; 16],
        project_owner: Some(Arc::from("/tsconfig.json")),
        regen_key: Some(regen),
        engine_recheck: Some(recheck),
    });
}

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
fn no_distinct_carrier_batch_slot_under_merge() {
    // MERGE: only the CarrierIde surface is stored; there is NO separate batch
    // slot. A CarrierBatch request reads the single IDE entry via the registry.
    let store = ProviderSurfaceStore::new();
    record_ide(&store, regen(0xAA), recheck(0x55, 5, 1));

    // No distinct batch slot exists in the store.
    assert!(
        store.current_snapshot(HYPOTHETICAL_BATCH_PATH).is_none(),
        "MERGE means NO distinct CarrierBatch storage slot is created"
    );

    let registry = StoreBackedCarrierRegistry::new(store, resolver());
    let batch = registry
        .carrier_for_role(SOURCE, CarrierRole::CarrierBatch)
        .expect("a CarrierBatch request resolves to the merged CarrierIde entry");
    assert_eq!(
        &*batch.provider_uri, IDE_PATH,
        "the batch carrier is served from the shared CarrierIde entry, not a \
         separate batch slot"
    );
}

#[test]
fn batch_invalidation_reads_the_shared_ide_entry() {
    // The split-cache invalidation the cold batch path observes is read from the
    // SINGLE shared CarrierIde entry (merge) — there is no separate batch slot to
    // invalidate. We assert the store's invalidation decisions on the IDE entry,
    // which is exactly what a batch request resolves to.
    let store = ProviderSurfaceStore::new();
    let r = regen(0xAA);
    record_ide(&store, r, recheck(0x55, 10, 1));

    // (a) Regeneration freshness on the shared IDE entry.
    assert!(
        store.carrier_regeneration_is_fresh(IDE_PATH, &r),
        "an unchanged self-content key on the shared entry is regeneration-fresh"
    );
    assert!(
        !store.carrier_regeneration_is_fresh(IDE_PATH, &regen(0xBB)),
        "a source-content change on the shared entry forces re-codegen"
    );

    // (b) Dependency-driven engine re-check on the shared IDE entry: a dependency
    // change re-checks; an unchanged closure does not.
    assert!(
        store.carrier_needs_engine_recheck(IDE_PATH, &recheck(0x55, 11, 1)),
        "a dependency change on the shared entry re-checks (cold batch path observes this)"
    );
    assert!(
        !store.carrier_needs_engine_recheck(IDE_PATH, &recheck(0x55, 10, 1)),
        "an unchanged dependency closure + project gen does not re-check"
    );
    // A project/env (lib/tsconfig) change ALSO re-checks the shared entry.
    assert!(
        store.carrier_needs_engine_recheck(IDE_PATH, &recheck(0x55, 10, 2)),
        "a project/env change on the shared entry re-checks (the lib/tsconfig rail)"
    );
}
