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
    EvalEnvCacheDb, OwnedArtifactKey, ProjectTypeStore, ResolvedTypeCacheDb, TypeResolutionContextDb,
};
use std::sync::Arc;
use verter_semantic::analysis::Hash16;

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
    let key = OwnedArtifactKey::new("test.vue", Hash16::default());
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
    let key = OwnedArtifactKey::new("test.vue", Hash16::default());
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
    assert_send_sync_static::<
        super::project_type_store::CompileCacheDb,
    >();
    // The whole `ProjectTypeStore` likewise — already enforced by
    // `Arc<ProjectTypeStore>` storage on `VerterHost`, but explicit
    // here for the discriminating-test record.
    assert_send_sync_static::<ProjectTypeStore>();
}
