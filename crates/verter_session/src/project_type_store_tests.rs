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
    use crate::types::{HostConfig, ProfileState};
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    // Accessor returns the reference; the rehomed body starts empty.
    assert!(
        host.compile_cache().is_empty(),
        "rehomed compile_cache must start empty"
    );
    // Round-trip: insertion via the host accessor is observable through
    // the project-store accessor, proving both reach the same body. Tier
    // 1C-β shrunk the value type from `CompileCacheEntry` to
    // `ProfileState` (D48 split — option (b) in the rehoming doc); the
    // test exercises the new shape.
    host.compile_cache()
        .insert("/probe.vue".to_string(), ProfileState::default());
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

// ─────────────────────────────────────────────────────────────────────
// Tier 1C-β discriminating tests (4) — D48 invalidation matrix.
//
// FAIL pre-1C-β: `CompileCacheEntry` was a single super-shape stored in
// `CompileCacheDb`, so per-domain invalidation could not be observed
// independently — every trigger either dropped the whole entry or
// preserved it. The matrix-asymmetric assertions below could not hold
// because the three sub-states shared eviction lifetime.
//
// PASS post-1C-β: `CompileCacheEntry` is split into three independent
// sub-state types (`ProfileState`, `DerivedRawState`, `DependencyState`)
// stored in three independent DBs (`CompileCacheDb`,
// `DerivedRawCacheDb`, `DependencyCacheDb`). Each trigger fires per
// domain per the §3.4.2 matrix; the asymmetric drops/preserves below
// are observable.
//
// Matrix (D48 plan §3.4.2):
//
// | Trigger                                  | ProfileState | DerivedRawState | DependencyState |
// |------------------------------------------|--------------|-----------------|-----------------|
// | Source content change for owner          | preserve     | invalidate      | invalidate      |
// | Profile-flag change                      | invalidate   | preserve        | preserve        |
// | Dep transitive close changed             | preserve     | preserve        | invalidate      |
// | bump_project_generation_and_evict        | invalidate   | invalidate      | invalidate      |
// ─────────────────────────────────────────────────────────────────────

/// Helper: seed all three sub-state entries for a canonical so each
/// discriminating test can observe per-domain eviction asymmetrically.
/// Each sub-state has at least one observable field set so a `get(..)`
/// after the trigger discriminates "entry survived" vs "entry dropped".
#[cfg(not(target_arch = "wasm32"))]
fn seed_all_three_sub_states(host: &super::VerterHost, canonical: &str) {
    use super::types::{
        CompileSlot, DependencyResolution, DependencyState, DerivedRawState, ProfileState,
    };
    use std::collections::BTreeSet;

    // ProfileState — populate compile_slots so the test can observe
    // whether the per-profile entry survives.
    {
        let mut profile = host
            .compile_cache()
            .entry(canonical.to_string())
            .or_default();
        let p: &mut ProfileState = profile.value_mut();
        p.compile_slots.insert(
            42,
            CompileSlot {
                semantic_hash: [0u8; 16],
                style_override_hash: 0,
                content_override_hash: 0,
                outputs: Default::default(),
                diagnostics: Default::default(),
                last_good_outputs: None,
                last_access_tick: 0,
                tsx: None,
                template_analysis: None,
            },
        );
    }
    // DerivedRawState — populate import_routes so the test can observe
    // whether the source-content-domain entry survives.
    {
        let mut derived = host
            .derived_raw_cache()
            .entry(canonical.to_string())
            .or_default();
        let d: &mut DerivedRawState = derived.value_mut();
        d.import_routes.insert(
            "./probe-dep".to_string(),
            DependencyResolution {
                specifier: "./probe-dep".to_string(),
                resolved_canonical_id: Some("/probe-dep.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        );
    }
    // DependencyState — populate dependencies so the test can observe
    // whether the dep-closure-domain entry survives.
    {
        let mut dep = host
            .dependency_cache()
            .entry(canonical.to_string())
            .or_default();
        let s: &mut DependencyState = dep.value_mut();
        s.dependencies = BTreeSet::from(["/probe-dep.ts".to_string()]);
    }
}

/// D48 row 1: source content change for owner — preserve ProfileState,
/// invalidate DerivedRawState + DependencyState.
///
/// Discriminating predicate: after `evict_canonical(canonical)` (the
/// per-canonical source-content trigger), the ProfileState entry MUST
/// still be queryable, and BOTH DerivedRawState AND DependencyState
/// entries MUST be gone. Pre-1C-β this could not hold because all three
/// "fields" lived in one struct that drops together; post-1C-β each DB
/// fans the trigger only into its own domain per the matrix.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn source_content_change_preserves_profile_state() {
    use crate::types::HostConfig;
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/source-content-trigger.vue";
    seed_all_three_sub_states(&host, canonical);

    // Sanity — all three present pre-trigger.
    assert!(host.compile_cache().get(canonical).is_some());
    assert!(host.derived_raw_cache().get(canonical).is_some());
    assert!(host.dependency_cache().get(canonical).is_some());

    // Source-content trigger — `evict_for_source_content_change` is
    // the per-canonical D48 matrix row-1 primitive (drops
    // source-content + dep-closure domain entries; preserves
    // profile-domain). The host-level upsert flow calls this before
    // re-populating the dropped entries with freshly-computed state.
    host.project_type_store()
        .evict_for_source_content_change(canonical);

    // ProfileState SURVIVES (matrix row 1 column 1: preserve).
    assert!(
        host.compile_cache().get(canonical).is_some(),
        "ProfileState entry MUST survive a source-content trigger per D48 matrix row 1"
    );
    let profile = host.compile_cache().get(canonical).unwrap();
    assert!(
        !profile.compile_slots.is_empty(),
        "ProfileState body MUST be untouched (compile_slots preserved)"
    );

    // DerivedRawState DROPS (matrix row 1 column 2: invalidate).
    assert!(
        host.derived_raw_cache().get(canonical).is_none(),
        "DerivedRawState entry MUST drop on a source-content trigger per D48 matrix row 1"
    );
    // DependencyState DROPS (matrix row 1 column 3: invalidate).
    assert!(
        host.dependency_cache().get(canonical).is_none(),
        "DependencyState entry MUST drop on a source-content trigger per D48 matrix row 1"
    );
}

/// D48 row 2: profile-flag change — invalidate ProfileState, preserve
/// DerivedRawState + DependencyState.
///
/// Discriminating predicate: after a profile-domain flush
/// (`compile_cache().clear()` modeling a workspace-wide profile-flag
/// rotation), the ProfileState entry MUST be gone, and BOTH
/// DerivedRawState AND DependencyState entries MUST survive. Pre-1C-β
/// this could not hold because the three "fields" lived in one entry
/// that the same `clear()` would drop wholesale; post-1C-β the
/// per-domain DB clear surgically targets only ProfileState.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn profile_flag_change_preserves_raw_and_dep_state() {
    use crate::types::HostConfig;
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/profile-flag-trigger.vue";
    seed_all_three_sub_states(&host, canonical);

    // Sanity — all three present pre-trigger.
    assert!(host.compile_cache().get(canonical).is_some());
    assert!(host.derived_raw_cache().get(canonical).is_some());
    assert!(host.dependency_cache().get(canonical).is_some());

    // Profile-flag trigger — `compile_cache().clear()` is the
    // profile-domain flush. (A real profile-flag rotation propagates
    // through the host's per-host query-profile mutator, which calls
    // exactly this DB-clear; the discriminator exercises the same
    // primitive directly so the matrix predicate is mechanically
    // checkable.)
    host.project_type_store().compile_cache().clear();

    // ProfileState DROPS (matrix row 2 column 1: invalidate).
    assert!(
        host.compile_cache().get(canonical).is_none(),
        "ProfileState entry MUST drop on a profile-flag trigger per D48 matrix row 2"
    );
    // DerivedRawState SURVIVES (matrix row 2 column 2: preserve).
    assert!(
        host.derived_raw_cache().get(canonical).is_some(),
        "DerivedRawState entry MUST survive a profile-flag trigger per D48 matrix row 2"
    );
    let derived = host.derived_raw_cache().get(canonical).unwrap();
    assert!(
        !derived.import_routes.is_empty(),
        "DerivedRawState body MUST be untouched (import_routes preserved)"
    );
    // DependencyState SURVIVES (matrix row 2 column 3: preserve).
    assert!(
        host.dependency_cache().get(canonical).is_some(),
        "DependencyState entry MUST survive a profile-flag trigger per D48 matrix row 2"
    );
    let dep = host.dependency_cache().get(canonical).unwrap();
    assert!(
        !dep.dependencies.is_empty(),
        "DependencyState body MUST be untouched (dependencies preserved)"
    );
}

/// D48 row 3: dep transitive close changed — preserve ProfileState +
/// DerivedRawState, invalidate DependencyState.
///
/// Discriminating predicate: after a dep-domain flush
/// (`dependency_cache().clear()` modeling a transitive-closure
/// recomputation that observed a delta), the DependencyState entry
/// MUST be gone, and BOTH ProfileState AND DerivedRawState entries
/// MUST survive. Pre-1C-β this could not hold because the three fields
/// shared an entry that drops together; post-1C-β the dep-domain DB
/// clear surgically targets only DependencyState.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dep_transitive_close_change_preserves_profile_and_raw() {
    use crate::types::HostConfig;
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/dep-closure-trigger.vue";
    seed_all_three_sub_states(&host, canonical);

    // Sanity — all three present pre-trigger.
    assert!(host.compile_cache().get(canonical).is_some());
    assert!(host.derived_raw_cache().get(canonical).is_some());
    assert!(host.dependency_cache().get(canonical).is_some());

    // Dep transitive-close trigger — `dependency_cache().clear()` is
    // the dep-domain flush. (A real transitive-closure delta routes
    // through `set_import_dependencies` / `sync_transitive_macro_type_dependencies`
    // which mutate exactly this DB; the discriminator exercises the
    // same primitive directly so the matrix predicate is mechanically
    // checkable.)
    host.project_type_store().dependency_cache().clear();

    // ProfileState SURVIVES (matrix row 3 column 1: preserve).
    assert!(
        host.compile_cache().get(canonical).is_some(),
        "ProfileState entry MUST survive a dep-closure trigger per D48 matrix row 3"
    );
    let profile = host.compile_cache().get(canonical).unwrap();
    assert!(
        !profile.compile_slots.is_empty(),
        "ProfileState body MUST be untouched (compile_slots preserved)"
    );
    // DerivedRawState SURVIVES (matrix row 3 column 2: preserve).
    assert!(
        host.derived_raw_cache().get(canonical).is_some(),
        "DerivedRawState entry MUST survive a dep-closure trigger per D48 matrix row 3"
    );
    let derived = host.derived_raw_cache().get(canonical).unwrap();
    assert!(
        !derived.import_routes.is_empty(),
        "DerivedRawState body MUST be untouched (import_routes preserved)"
    );
    // DependencyState DROPS (matrix row 3 column 3: invalidate).
    assert!(
        host.dependency_cache().get(canonical).is_none(),
        "DependencyState entry MUST drop on a dep-closure trigger per D48 matrix row 3"
    );
}

/// D48 row 4: `bump_project_generation_and_evict` — invalidate ALL
/// THREE sub-state DBs.
///
/// Discriminating predicate: after `bump_project_generation_and_evict`,
/// every per-canonical entry across all three D48 sub-state DBs MUST
/// be gone. Pre-1C-β the cascade extension only cleared the unified
/// `compile_cache_db`; post-1C-β it MUST fan into the three sibling
/// DBs (per the unified-cascade rehoming-doc rule). Failure to extend
/// the cascade to BOTH new DBs would leak DerivedRawState +
/// DependencyState entries past a project-generation rotation, which
/// the matrix forbids.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn bump_project_generation_evicts_all_three_sub_shapes() {
    use crate::types::HostConfig;
    use crate::VerterHost;

    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/project-gen-trigger.vue";
    seed_all_three_sub_states(&host, canonical);

    // Sanity — all three present pre-trigger.
    assert!(host.compile_cache().get(canonical).is_some());
    assert!(host.derived_raw_cache().get(canonical).is_some());
    assert!(host.dependency_cache().get(canonical).is_some());

    // Project-generation trigger — the unified
    // `bump_project_generation_and_evict` cascade.
    host.project_type_store()
        .bump_project_generation_and_evict();

    // ALL THREE DBs DROP (matrix row 4: invalidate × 3).
    assert!(
        host.compile_cache().get(canonical).is_none(),
        "ProfileState entry MUST drop on bump_project_generation_and_evict per D48 matrix row 4"
    );
    assert!(
        host.derived_raw_cache().get(canonical).is_none(),
        "DerivedRawState entry MUST drop on bump_project_generation_and_evict per D48 matrix row 4"
    );
    assert!(
        host.dependency_cache().get(canonical).is_none(),
        "DependencyState entry MUST drop on bump_project_generation_and_evict per D48 matrix row 4"
    );
    // Aggregate length assertions — closes the "stale entry leaks past
    // the cascade in another canonical" trapdoor.
    assert!(
        host.compile_cache().is_empty(),
        "compile_cache_db MUST be empty after bump_project_generation_and_evict"
    );
    assert!(
        host.derived_raw_cache().is_empty(),
        "derived_raw_cache_db MUST be empty after bump_project_generation_and_evict"
    );
    assert!(
        host.dependency_cache().is_empty(),
        "dependency_cache_db MUST be empty after bump_project_generation_and_evict"
    );
}
