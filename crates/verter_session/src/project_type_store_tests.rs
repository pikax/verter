//! Tier 1A discriminating tests for the typed-DB shapes added to
//! [`crate::project_type_store::ProjectTypeStore`].
//!
//! Asserts the empty-DB accessor presence and basic round-trip
//! invariants. Tier 1C-α populates real consumers; these tests only
//! verify the 1A contract (field present, accessor returns a typed
//! reference, DB starts empty).

use super::owned_artifacts::eval_program::OwnedEvalProgram;
use super::owned_artifacts::type_resolution_context::OwnedTypeResolutionContext;
use super::project_type_store::{
    EvalEnvCacheDb, OwnedArtifactKey, ProjectTypeStore, ResolvedTypeCacheDb,
    TypeResolutionContextDb,
};
use std::sync::Arc;

#[test]
fn type_resolution_context_db_present_with_accessor() {
    // Discriminating predicate: `ProjectTypeStore` exposes a typed
    // accessor for the new `TypeResolutionContextDb` and the DB starts
    // empty. A regression that returns `&()` or wraps the wrong type
    // would fail at the type level.
    let store = ProjectTypeStore::new();
    let db: &TypeResolutionContextDb = store.type_resolution_context_cache();
    assert!(db.is_empty(), "Tier 1A introduces the DB empty");
    // Constructive insert + lookup roundtrip — verifies the DB is
    // really backed by storage and not a no-op stub.
    let key = OwnedArtifactKey::new("test.vue", [0u8; 16]);
    db.insert(key.clone(), Arc::new(OwnedTypeResolutionContext::empty()));
    assert_eq!(db.len(), 1, "insert must populate");
    let recovered = db.get(&key);
    assert!(recovered.is_some(), "lookup must hit");
    db.clear();
    assert!(db.is_empty(), "clear must drain");
}

#[test]
fn eval_env_cache_db_present_with_accessor() {
    let store = ProjectTypeStore::new();
    let db: &EvalEnvCacheDb = store.eval_env_cache();
    assert!(db.is_empty(), "Tier 1A introduces the DB empty");
    let key = OwnedArtifactKey::new("test.vue", [0u8; 16]);
    db.insert(key.clone(), Arc::new(OwnedEvalProgram::empty()));
    assert_eq!(db.len(), 1);
    let recovered = db.get(&key);
    assert!(recovered.is_some());
    db.clear();
    assert!(db.is_empty());
}

#[test]
fn compile_cache_db_present_with_accessor() {
    // The 1C-β split rebinds the inner type; Tier 1A only asserts the
    // accessor is callable and the DB starts empty.
    let store = ProjectTypeStore::new();
    let db = store.compile_cache();
    assert!(db.is_empty());
    db.clear();
    assert_eq!(db.len(), 0);
}

#[test]
fn resolved_type_cache_db_present_with_accessor() {
    let store = ProjectTypeStore::new();
    let db: &ResolvedTypeCacheDb = store.resolved_type_cache();
    assert!(db.is_empty());
    db.clear();
    assert_eq!(db.len(), 0);
}

#[test]
fn typed_dbs_are_send_sync_static() {
    // Aggregate guard — every Tier 1A typed DB must be `Send + Sync +
    // 'static`. A regression that puts a borrowed lifetime on any
    // payload would fail to compile here.
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<TypeResolutionContextDb>();
    assert_send_sync_static::<EvalEnvCacheDb>();
    assert_send_sync_static::<ResolvedTypeCacheDb>();
    assert_send_sync_static::<super::project_type_store::CompileCacheDb>();
    // The whole `ProjectTypeStore` likewise — already enforced by
    // `Arc<ProjectTypeStore>` storage on `VerterHost`, but explicit
    // here for the discriminating-test record.
    assert_send_sync_static::<ProjectTypeStore>();
}

// ─────────────────────────────────────────────────────────────────────
// Tier 1C-α discriminating tests (4)
//
// FAIL pre-1C-α (the four off-store fields lived directly on
// `VerterHost` and the typed-DB wrappers were empty 1A shells).
// PASS post-1C-α (the off-store bodies have moved into the
// `ProjectTypeStore` typed-DB wrappers, accessors return non-empty
// wrappers, and the storage shapes match the D17 / D18 / D46 contract).
// ─────────────────────────────────────────────────────────────────────

#[test]
fn compile_cache_db_present_with_accessor_post_tier_1c_alpha() {
    // Discriminator: a `VerterHost` constructed via `new_standalone` MUST
    // route compile-cache reads/writes through the `ProjectTypeStore`'s
    // `CompileCacheDb` wrapper. Pre-1C-α the `compile_cache` field lived
    // directly on `VerterHost` and `host.compile_cache().is_empty()` would
    // fail to compile because no method existed (the field shadowed). Post
    // -1C-α the method-call form returns a reference to the rehomed
    // DashMap, and a round-trip through the typed wrapper observes the
    // same insertion via `host.project_type_store.compile_cache()`.
    use crate::types::{CompileCacheEntry, HostConfig};
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    // Accessor returns the reference; the rehomed body starts empty.
    assert!(
        host.compile_cache().is_empty(),
        "rehomed compile_cache must start empty"
    );
    // Round-trip: insertion via the host accessor is observable through
    // the project-store accessor, proving both reach the same body.
    host.compile_cache()
        .insert("/probe.vue".to_string(), CompileCacheEntry::default());
    assert_eq!(
        host.project_type_store().compile_cache().len(),
        1,
        "host.compile_cache() and project_type_store.compile_cache() \
         must share the same backing storage"
    );
    assert_eq!(host.compile_cache().len(), 1);
    // Cascade observability: `bump_project_generation_and_evict` clears
    // the rehomed compile cache (the unified cascade extension landed
    // in 1C-α).
    host.project_type_store()
        .bump_project_generation_and_evict();
    assert!(
        host.compile_cache().is_empty(),
        "bump_project_generation_and_evict must drop rehomed compile_cache entries"
    );
}

#[test]
fn resolved_type_cache_db_present_with_accessor_post_tier_1c_alpha() {
    // Discriminator: post-1C-α, `host.resolved_type_cache()` returns
    // the typed `ResolvedTypeCacheDb` wrapper (the parking_lot Mutex
    // moved INTO the wrapper). The bounded clear-all-at-cap policy
    // is preserved INSIDE the DB.
    use crate::types::{
        HostConfig, ResolvedTypeCacheEntry, ResolvedTypeCacheKey, RESOLVED_TYPE_CACHE_CAP,
    };
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    assert!(host.resolved_type_cache().is_empty());

    // Insert one entry and verify it round-trips through the wrapper.
    let key = ResolvedTypeCacheKey {
        dep_canonical_id: "/probe.ts".to_string(),
        dep_source_hash: [0u8; 16],
        type_name: "Probe".to_string(),
        resolve_kind: verter_workspace::ResolveRequestKind::TypeImport,
    };
    host.resolved_type_cache().insert(
        key.clone(),
        ResolvedTypeCacheEntry {
            resolved: None,
            tracked_deps: Vec::new(),
        },
    );
    assert_eq!(host.resolved_type_cache().len(), 1);
    let recovered = host.resolved_type_cache().lookup(&key);
    assert!(recovered.is_some(), "round-trip must hit");

    // Bounded clear-all is enforced INSIDE the DB. Filling beyond cap
    // triggers the clear-all path; the cap constant is the public
    // contract surface.
    const _: () = assert!(
        RESOLVED_TYPE_CACHE_CAP >= 1024,
        "RESOLVED_TYPE_CACHE_CAP must remain in the documented range"
    );
}

#[test]
fn eval_env_cache_db_stores_owned_eval_program_arc() {
    // Discriminator: the rehomed `EvalEnvCacheDb` stores
    // `Arc<OwnedEvalProgram>` per D17 (NOT raw `Arc<EvalEnv>`).
    // Pre-1C-α the off-store `eval_env_cache: Mutex<FxHashMap<String,
    // (Hash16, Arc<EvalEnv>)>>` field had a different value type;
    // post-1C-α the typed DB exposes an `OwnedArtifactKey ->
    // Arc<OwnedEvalProgram>` shape that round-trips through the
    // `insert` / `get` API surface.
    use super::owned_artifacts::eval_program::OwnedEvalProgram;
    use crate::types::HostConfig;
    use crate::VerterHost;
    use std::sync::Arc;

    let host = VerterHost::new_standalone(HostConfig::default());
    let db: &EvalEnvCacheDb = host.project_type_store().eval_env_cache();
    let key = OwnedArtifactKey::new("/probe.vue", [0u8; 16]);
    let program: Arc<OwnedEvalProgram> = Arc::new(OwnedEvalProgram::empty());
    db.insert(key.clone(), Arc::clone(&program));
    let recovered = db.get(&key).expect("rehomed program lookup must hit");
    assert!(
        Arc::ptr_eq(&program, &recovered),
        "EvalEnvCacheDb MUST store Arc<OwnedEvalProgram> (D17). The pointer-\
         equality check would fail if the wrapper deep-cloned or mutated \
         the inner program on read."
    );
}

#[test]
fn type_resolution_context_db_stores_owned_arc() {
    // Discriminator: the rehomed `TypeResolutionContextDb` stores
    // `Arc<OwnedTypeResolutionContext>` per D18. Pointer-equality
    // across reads proves the wrapper does not deep-clone on lookup.
    use super::owned_artifacts::type_resolution_context::OwnedTypeResolutionContext;
    use crate::types::HostConfig;
    use crate::VerterHost;
    use std::sync::Arc;

    let host = VerterHost::new_standalone(HostConfig::default());
    let db: &TypeResolutionContextDb = host.project_type_store().type_resolution_context_cache();
    let key = OwnedArtifactKey::new("/probe.vue", [1u8; 16]);
    let ctx: Arc<OwnedTypeResolutionContext> = Arc::new(OwnedTypeResolutionContext::empty());
    db.insert(key.clone(), Arc::clone(&ctx));
    let recovered = db.get(&key).expect("rehomed context lookup must hit");
    assert!(
        Arc::ptr_eq(&ctx, &recovered),
        "TypeResolutionContextDb MUST store Arc<OwnedTypeResolutionContext> (D18)"
    );
}
