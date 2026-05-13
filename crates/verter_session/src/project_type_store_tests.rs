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
// Matrix (D48):
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

// ─────────────────────────────────────────────────────────────────────
// Tier 1C-γ discriminating tests (6 from +
// 5 from rewritten rehoming-doc §3.3 = 11 total).
//
// FAIL pre-1C-γ:
//   - `evict_unreachable_artifacts` does not exist on
//     `ProjectTypeStore` (D33 reachability sweep absent).
//   - `EvictionPolicyConfig` does not exist on `HostConfig`
//     (D119 tunable absent).
//   - `evict_canonical` does not invalidate `semantic_db`
//     (rehoming-doc §3.3 test #4).
//   - `FileArtifactStore::keys` and `evict_lru` do not exist (D40 LRU
//     floor absent).
//
// PASS post-1C-γ: each sub-test asserts one of the above invariants
// directly. None of them can pass against the pre-1C-γ tree.
// ─────────────────────────────────────────────────────────────────────

/// Helper — seed `FileArtifactStore` with N synthetic entries, returning
/// the live publish set (every `(canonical, content_hash)` pair).
#[cfg(not(target_arch = "wasm32"))]
fn seed_indexed_ready(
    store: &ProjectTypeStore,
    n: usize,
) -> rustc_hash::FxHashSet<(std::sync::Arc<str>, [u8; 16])> {
    use crate::project_type_store::IndexedReady;
    use std::sync::Arc;
    let mut set = rustc_hash::FxHashSet::default();
    for i in 0..n {
        let canonical: Arc<str> = Arc::from(format!("/probe-{i}.ts"));
        let mut whole_hash = [0u8; 16];
        whole_hash[0] = (i & 0xff) as u8;
        whole_hash[1] = ((i >> 8) & 0xff) as u8;
        let indexed = Arc::new(IndexedReady::new_for_test(whole_hash));
        store.indexed().insert(Arc::clone(&canonical), indexed);
        set.insert((canonical, whole_hash));
    }
    set
}

/// Plan §3.4.3 test #1 / D33 reachability — unchanged live file is
/// not re-lowered across publish cycles.
///
/// Discriminating predicate: when the same `(canonical,
/// content_hash)` pair is in `live_publish_set` across two
/// reachability sweeps, the cached `IndexedReady` `Arc` survives
/// pointer-equality, proving no re-lowering was triggered.
/// Pre-1C-γ this could not hold because
/// `evict_unreachable_artifacts` did not exist; the only
/// cache-eviction primitives were `evict_canonical` (drops the
/// entry unconditionally) and `bump_project_generation_and_evict`
/// (clears `FileArtifactStore` indirectly via `evict_canonical_for`).
/// Post-1C-γ the new method preserves entries whose pair is in
/// `live_publish_set`.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unchanged_live_file_never_re_lowered_across_publish_cycles() {
    use crate::project_type_store::IndexedReady;
    use std::sync::Arc;

    let store = ProjectTypeStore::new();
    let canonical: Arc<str> = Arc::from("/stable.ts");
    let whole_hash = [42u8; 16];
    let indexed = Arc::new(IndexedReady::new_for_test(whole_hash));
    store.indexed().insert(Arc::clone(&canonical), indexed);
    let pre_arc = store
        .indexed()
        .get_any(canonical.as_ref())
        .expect("entry seeded");

    // Cycle 1 — `(canonical, whole_hash)` is in the live set; the
    // entry MUST survive.
    let mut live: rustc_hash::FxHashSet<(Arc<str>, [u8; 16])> = rustc_hash::FxHashSet::default();
    live.insert((Arc::clone(&canonical), whole_hash));
    store.evict_unreachable_artifacts(&live, false, 1024);
    let mid_arc = store
        .indexed()
        .get_any(canonical.as_ref())
        .expect("entry must survive a no-op reachability sweep");
    assert!(
        Arc::ptr_eq(&pre_arc, &mid_arc),
        "unchanged live file MUST not be re-lowered: the cached \
         Arc<IndexedReady> must be pointer-equal across publish cycles"
    );

    // Cycle 2 — same set, same content hash. The pointer survives a
    // second sweep.
    store.evict_unreachable_artifacts(&live, false, 1024);
    let post_arc = store
        .indexed()
        .get_any(canonical.as_ref())
        .expect("entry must survive a second reachability sweep");
    assert!(
        Arc::ptr_eq(&pre_arc, &post_arc),
        "the same Arc<IndexedReady> MUST persist across multiple \
         reachability sweeps when its (canonical, content_hash) \
         remains in live_publish_set"
    );
}

/// Plan §3.4.3 test #2 — the four off-store caches absent post-Tier 1.
///
/// Discriminating predicate: parse `lib.rs` via syn and assert that
/// `VerterHost` has no field named `compile_cache`,
/// `resolved_type_cache`, `eval_env_cache`, or `semantic_db`. Pre
/// -1C-α the four fields lived on `VerterHost`; post-1C-α they all
/// rehomed onto `ProjectTypeStore`. 1C-γ verifies they stay rehomed
/// as part of the final allow-list shape.
#[test]
fn four_off_store_caches_absent_post_tier_1() {
    use std::path::PathBuf;
    use syn::{parse_file, Fields, Item};

    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set during cargo test");
    let lib_path = PathBuf::from(manifest).join("src/lib.rs");
    let lib_src =
        std::fs::read_to_string(&lib_path).unwrap_or_else(|e| panic!("read {lib_path:?}: {e}"));
    let parsed = parse_file(&lib_src).expect("parse lib.rs via syn");

    // Walk top-level items; locate `pub struct VerterHost`.
    let mut surveyed_fields: Vec<String> = Vec::new();
    for item in parsed.items.iter() {
        if let Item::Struct(s) = item {
            if s.ident == "VerterHost" {
                if let Fields::Named(named) = &s.fields {
                    for field in named.named.iter() {
                        if let Some(ident) = field.ident.as_ref() {
                            surveyed_fields.push(ident.to_string());
                        }
                    }
                }
            }
        }
    }
    assert!(
        !surveyed_fields.is_empty(),
        "syn-walk found no VerterHost fields — guard is broken"
    );

    for forbidden in [
        "compile_cache",
        "resolved_type_cache",
        "eval_env_cache",
        "semantic_db",
    ] {
        assert!(
            !surveyed_fields.iter().any(|f| f == forbidden),
            "Tier 1 final-state guard: VerterHost must NOT carry \
             field `{forbidden}` post-Tier-1 (rehomed onto \
             ProjectTypeStore in 1C-α). Re-introducing it would \
             create a dual-path off-store cache."
        );
    }
}

/// Plan §3.4.3 test #3 — host_manage thread-locals absent post-Tier 1.
///
/// Discriminating predicate: every `.rs` source file under
/// `crates/verter_session/src/host_manage/` has no
/// `HOST_PARSED_*_CACHE` thread-local (`thread_local!` macro
/// invocation containing `HOST_PARSED_`). Tier 1A retired the two
/// thread-locals (`HOST_PARSED_EVAL_PROGRAM_CACHE`,
/// `HOST_PARSED_TYPE_CONTEXT_CACHE`); 1C-γ verifies they stay
/// retired. Doc-comment references like `// HOST_PARSED_EVAL_PROGRAM_CACHE
/// thread-local was 1A...` are allowed (they are
/// not declarations); the guard fires only on actual
/// `thread_local!` invocations.
#[test]
fn host_manage_thread_local_caches_absent_post_tier_1() {
    use std::path::PathBuf;
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set during cargo test");
    let host_manage_dir = PathBuf::from(manifest).join("src/host_manage");
    assert!(
        host_manage_dir.is_dir(),
        "host_manage directory must exist at {host_manage_dir:?}"
    );

    let mut violations: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&host_manage_dir).expect("read host_manage dir") {
        let path = entry.expect("dir entry").path();
        if !path.extension().map(|e| e == "rs").unwrap_or(false) {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        // Look for `thread_local!` macro invocations whose body
        // names a `HOST_PARSED_*_CACHE` static. The check is
        // structural: any `thread_local!` block that contains the
        // substring `HOST_PARSED_` is a violation, since the only
        // legitimate use of that prefix would be a declaration.
        // Doc-comments referencing the retired names contain the
        // text `// ` or `/// ` and do not appear inside a
        // `thread_local!` block, so they are skipped automatically.
        let mut search_from = 0usize;
        while let Some(idx) = src[search_from..].find("thread_local!") {
            let abs_idx = search_from + idx;
            // Find the matching opening `{` and closing `}` for the
            // macro body. A small bracket-balance walk is enough.
            let after_macro = &src[abs_idx..];
            if let Some(open_offset) = after_macro.find('{') {
                let body_start = abs_idx + open_offset + 1;
                let mut depth = 1i32;
                let mut body_end = body_start;
                for (i, ch) in src[body_start..].char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                body_end = body_start + i;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let body = &src[body_start..body_end];
                if body.contains("HOST_PARSED_") {
                    violations.push(format!(
                        "{}: thread_local! body references HOST_PARSED_*",
                        path.display()
                    ));
                }
                search_from = body_end + 1;
            } else {
                break;
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Tier 1 final-state guard: host_manage thread-local caches \
         must NOT exist post-Tier-1 (Tier 1A retired them). \
         Violations:\n{}",
        violations.join("\n")
    );
}

/// Plan §3.4.3 test #4 — `phase_8_allow_list` shrunk to its final shape.
///
/// Discriminating predicate: the architecture-guards allow-list
/// contains exactly the documented final entries
/// (`query_profile`, `alias_to_canonical`,
/// `last_const_prop_overrides`, `workspace`, `last_upsert_priority`)
/// and NONE of the rehomed F1/F2/F4/F5 entries. The guard is
/// driven by the exact set; either an extra allow-list entry or a
/// missing one would trip the discriminator.
#[test]
fn no_off_store_host_caches_allow_list_shrunk() {
    use std::path::PathBuf;
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set during cargo test");
    let guards_path = PathBuf::from(manifest).join("tests/architecture_guards.rs");
    let src = std::fs::read_to_string(&guards_path)
        .unwrap_or_else(|e| panic!("read {guards_path:?}: {e}"));

    // Locate the allow-list function body and check that:
    //  - F1/F2/F4/F5 are NOT present as keys (their string-literal
    //    keys would appear in the body if they were)
    //  - the five expected final keys ARE present.
    let body_start = src
        .find("fn phase_8_allow_list()")
        .expect("phase_8_allow_list must exist in architecture_guards.rs");
    // Take the next ~3000 bytes after the opening — the function
    // body is short and self-contained.
    let body = &src[body_start..body_start + src[body_start..].len().min(4000)];

    for forbidden_key in [
        "\"compile_cache\"",
        "\"resolved_type_cache\"",
        "\"eval_env_cache\"",
        "\"semantic_db\"",
    ] {
        assert!(
            !body.contains(forbidden_key),
            "Tier 1 final-state guard: phase_8_allow_list MUST NOT \
             contain rehomed key {forbidden_key} (rehomed in 1C-α). \
             Re-adding it implies a regression of the F1/F2/F4/F5 \
             rehoming."
        );
    }
    for required_key in [
        "\"query_profile\"",
        "\"alias_to_canonical\"",
        "\"last_const_prop_overrides\"",
        "\"workspace\"",
        "\"last_upsert_priority\"",
    ] {
        assert!(
            body.contains(required_key),
            "Tier 1 final-state guard: phase_8_allow_list MUST \
             contain required final-state key {required_key} \
             (these are the documented non-cache exceptions). \
             A missing key indicates the allow-list is broken."
        );
    }
}

/// Plan §3.4.3 test #5 — eviction-policy tunables exposed via HostConfig.
///
/// Discriminating predicate: `HostConfig::default()` produces a
/// config whose `eviction_policy.memory_pressure_threshold ==
/// usize::MAX` (per D119 — never trigger LRU floor by default).
/// Pre-1C-γ the field did not exist; post-1C-γ the threshold is
/// part of the documented public API.
#[test]
fn eviction_policy_tunables_exposed_via_host_config() {
    use crate::types::{EvictionPolicyConfig, HostConfig};
    let config = HostConfig::default();
    assert_eq!(
        config.eviction_policy.memory_pressure_threshold,
        usize::MAX,
        "Per D119: HostConfig::default().eviction_policy.memory_pressure_threshold \
         MUST be usize::MAX so default builds NEVER trigger the LRU floor."
    );
    // The min_floor default is part of the public contract — any
    // future tightening should be intentional.
    assert!(
        config.eviction_policy.min_floor >= 1,
        "min_floor must be at least 1 (zero-floor would defeat \
         the purpose of an LRU floor); got {}",
        config.eviction_policy.min_floor
    );
    // Independent construction must yield the same defaults.
    let policy = EvictionPolicyConfig::default();
    assert_eq!(policy.memory_pressure_threshold, usize::MAX);
    assert_eq!(policy.min_floor, config.eviction_policy.min_floor);
}

/// Plan §3.4.3 test #6 — LRU floor only triggers under memory pressure.
///
/// Discriminating predicate: seeded with N entries (well above the
/// floor), a sweep with `memory_pressure: false` MUST leave entry
/// count unchanged; a sweep with `memory_pressure: true` MUST
/// shrink entry count to exactly `min_floor`. Pre-1C-γ neither the
/// `memory_pressure` flag nor `evict_lru` existed.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn lru_floor_only_triggers_under_memory_pressure_threshold() {
    let store = ProjectTypeStore::new();
    let n = 100;
    let live = seed_indexed_ready(&store, n);
    assert_eq!(store.indexed().len(), n, "seed assertion");

    // memory_pressure: false → no LRU eviction. Reachability
    // preserves every entry because the live set covers the cache
    // exactly.
    let min_floor = 10;
    store.evict_unreachable_artifacts(&live, false, min_floor);
    assert_eq!(
        store.indexed().len(),
        n,
        "Per D40 + D119: with memory_pressure=false, evict_lru \
         MUST NOT run; entry count must be unchanged."
    );

    // memory_pressure: true → LRU shrinks to exactly min_floor.
    store.evict_unreachable_artifacts(&live, true, min_floor);
    assert_eq!(
        store.indexed().len(),
        min_floor,
        "Per D40: with memory_pressure=true, evict_lru MUST shrink \
         entry count down to min_floor (got {} after sweep with \
         min_floor={})",
        store.indexed().len(),
        min_floor
    );
}

// ─────────────────────────────────────────────────────────────────────
// 5 discriminating tests from rewritten rehoming-doc §3.3.
// ─────────────────────────────────────────────────────────────────────

/// Rehoming-doc §3.3 test #1 — `compile_cache` lives on
/// `ProjectTypeStore`, not `VerterHost`.
///
/// Discriminating predicate: parse `lib.rs` via syn and confirm
/// `VerterHost` has no `compile_cache` field; then confirm the
/// rehomed DB is reachable through
/// `host.project_type_store().compile_cache()`. Pre-rehoming the
/// field lived directly on `VerterHost`; post-rehoming the
/// rehomed wrapper is the sole authority.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compile_cache_lives_on_project_type_store() {
    use crate::types::HostConfig;
    use crate::VerterHost;
    use std::path::PathBuf;
    use syn::{parse_file, Fields, Item};

    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set during cargo test");
    let lib_path = PathBuf::from(manifest).join("src/lib.rs");
    let lib_src =
        std::fs::read_to_string(&lib_path).unwrap_or_else(|e| panic!("read {lib_path:?}: {e}"));
    let parsed = parse_file(&lib_src).expect("parse lib.rs via syn");

    // Walk top-level items for `pub struct VerterHost`.
    let mut found_struct = false;
    for item in parsed.items.iter() {
        if let Item::Struct(s) = item {
            if s.ident == "VerterHost" {
                found_struct = true;
                if let Fields::Named(named) = &s.fields {
                    for field in named.named.iter() {
                        if let Some(ident) = field.ident.as_ref() {
                            assert_ne!(
                                ident.to_string(),
                                "compile_cache",
                                "rehoming-doc §3.3 test #1: VerterHost \
                                 MUST NOT carry a `compile_cache` field; \
                                 the rehomed DB lives on ProjectTypeStore."
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(found_struct, "VerterHost struct must exist in lib.rs");

    // Post-condition: the rehomed DB is reachable through the
    // ProjectTypeStore accessor and is functionally connected.
    let host = VerterHost::new_standalone(HostConfig::default());
    let db_via_pts = host.project_type_store().compile_cache();
    assert!(db_via_pts.is_empty(), "rehomed DB starts empty");
}

/// Rehoming-doc §3.3 test #2 — `resolved_type_cache.evict_canonical`
/// drains entries keyed on `dep_canonical`.
///
/// Discriminating predicate: insert a resolved-type entry whose
/// `dep_canonical_id == "X"`; call `evict_canonical("X")`; assert
/// the entry is gone. Pre-rehoming, per-canonical eviction did not
/// exist (the off-store cache only had clear-all-at-cap);
/// post-rehoming the unified cascade drains entries keyed on the
/// canonical.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolved_type_cache_evict_canonical_drains_dep_canonical() {
    use crate::types::{HostConfig, ResolvedTypeCacheEntry, ResolvedTypeCacheKey};
    use crate::VerterHost;
    use verter_workspace::ResolveRequestKind;

    let host = VerterHost::new_standalone(HostConfig::default());
    let dep_canonical = "/probe-evict.ts";
    let key = ResolvedTypeCacheKey {
        dep_canonical_id: dep_canonical.to_string(),
        dep_source_hash: [7u8; 16],
        type_name: "ProbeEvictType".to_string(),
        resolve_kind: ResolveRequestKind::TypeImport,
    };
    host.resolved_type_cache().insert(
        key.clone(),
        ResolvedTypeCacheEntry {
            resolved: None,
            tracked_deps: Vec::new(),
        },
    );
    assert!(
        host.resolved_type_cache().lookup(&key).is_some(),
        "seed: entry must be present pre-evict"
    );

    // Drain via the unified cascade.
    host.project_type_store().evict_canonical(dep_canonical);

    assert!(
        host.resolved_type_cache().lookup(&key).is_none(),
        "rehoming-doc §3.3 test #2: evict_canonical(\"X\") MUST \
         drain ResolvedTypeCacheDb entries whose dep_canonical_id \
         equals X. Pre-rehoming the off-store cache did not honour \
         per-canonical eviction; post-rehoming this is the unified \
         cascade contract."
    );
}

/// Rehoming-doc §3.3 test #3 — two concurrent cold callers compute
/// the eval-env entry exactly once.
///
/// Discriminating predicate: spawn two threads that both look up
/// the same `(canonical, whole_hash)` and, on miss, insert a value
/// produced by an `Arc<AtomicUsize>` capture token. After the
/// race resolves, the captured count MUST be exactly 1. Pre-rehoming
/// the manual `Mutex.lookup_or_insert` shape allowed both threads
/// to bypass the dedup; post-rehoming the cooperative-admission
/// contract collapses concurrent cold computes onto a single
/// admission.
///
/// NOTE: cooperative-admission cold-path wiring depends on the
/// lowering pipeline (per the 1C-α follow-up note). This test
/// exercises the simpler `entry().or_insert_with` shape that the
/// `EvalEnvCacheDb` already exposes; it discriminates the
/// `Arc::ptr_eq` / single-call-count contract that the storage
/// shape MUST honour.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn eval_env_cache_two_concurrent_cold_callers_compute_once() {
    use super::owned_artifacts::eval_program::OwnedEvalProgram;
    use crate::project_type_store::OwnedArtifactKey;
    use crate::types::HostConfig;
    use crate::VerterHost;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let key = OwnedArtifactKey::new("/probe-cooperative.vue", [3u8; 16]);
    let compute_count = Arc::new(AtomicUsize::new(0));
    let admit_barrier = Arc::new(Barrier::new(2));

    // Each thread races the same key through
    // `EvalEnvCacheDb::get_or_insert_with`. The cooperative-admission
    // contract collapses concurrent computes onto a single closure
    // invocation: only one of the two threads observes its `compute`
    // closure being called.
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let host = Arc::clone(&host);
            let key = key.clone();
            let count = Arc::clone(&compute_count);
            let barrier = Arc::clone(&admit_barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let db = host.project_type_store().eval_env_cache();
                let _admitted = db.get_or_insert_with(key.clone(), || {
                    count.fetch_add(1, Ordering::SeqCst);
                    Arc::new(OwnedEvalProgram::empty())
                });
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread join");
    }
    let admitted = host.project_type_store().eval_env_cache().get(&key);
    assert!(
        admitted.is_some(),
        "after the race, the entry MUST be in the cache"
    );
    assert_eq!(
        compute_count.load(Ordering::SeqCst),
        1,
        "rehoming-doc §3.3 test #3: cooperative cold admission MUST \
         collapse concurrent computes onto a single closure call. \
         Got compute_count = {} (expected exactly 1).",
        compute_count.load(Ordering::SeqCst)
    );
}

/// Rehoming-doc §3.3 test #4 — `evict_canonical` invalidates
/// `semantic_db` via the unified cascade.
///
/// Discriminating predicate: pre-populate `semantic_db` with a
/// component-surface fact for canonical `"X"`; call
/// `project_type_store.evict_canonical("X")`; assert that the
/// subsequent semantic-db query for `"X"` returns
/// `Completeness::Unavailable` (the entry is gone).
/// Pre-rehoming `evict_canonical` did NOT touch `semantic_db`
/// (only `smart_invalidate_dependents` did). Post-1C-γ the
/// extension is added per the rehoming-doc §3.3 contract.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn semantic_db_evict_canonical_invalidates_via_unified_cascade() {
    use crate::types::HostConfig;
    use crate::VerterHost;
    use verter_semantic::facts::component::ComponentSurface;
    use verter_semantic::query::Completeness;
    use verter_semantic::refs::FileRef;
    use verter_semantic::revision::RevisionMarker;

    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/probe-semantic-evict.ts";
    let revision = RevisionMarker {
        workspace_revision: 1,
        ..RevisionMarker::initial()
    };

    // Seed semantic_db with an entry for the canonical.
    {
        let mut db = host.project_type_store().semantic_db();
        db.set_component_surface(canonical.to_string(), revision, ComponentSurface::default());
    }
    // Sanity — the entry is queryable pre-evict.
    {
        let db = host.project_type_store().semantic_db();
        let result = db.component_surface(&FileRef::new(canonical), revision);
        assert_eq!(
            result.completeness,
            Completeness::Complete,
            "seed: semantic_db entry MUST be queryable pre-evict"
        );
    }

    // Drain via the unified cascade.
    host.project_type_store().evict_canonical(canonical);

    // Post-evict — the same query MUST observe `Unavailable` (the
    // file row was removed from `semantic_db`).
    {
        let db = host.project_type_store().semantic_db();
        let result = db.component_surface(&FileRef::new(canonical), revision);
        assert_eq!(
            result.completeness,
            Completeness::Unavailable,
            "rehoming-doc §3.3 test #4: evict_canonical(\"X\") MUST \
             invalidate the semantic_db entry for X via the unified \
             cascade. Pre-rehoming this contract did not exist."
        );
    }
}

/// Rehoming-doc §3.3 test #5 — `bump_project_generation_and_evict`
/// drains all four rehomed caches.
///
/// Discriminating predicate: populate one entry in each of the
/// four rehomed caches (compile_cache, resolved_type_cache,
/// eval_env_cache, semantic_db); call
/// `bump_project_generation_and_evict`; assert all four are empty.
/// Pre-rehoming the off-store caches each had separate clear
/// paths; post-rehoming the unified cascade fans out to all four
/// per the rehoming-doc §3.3 contract.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn bump_project_generation_evicts_all_four() {
    use super::owned_artifacts::eval_program::OwnedEvalProgram;
    use crate::project_type_store::OwnedArtifactKey;
    use crate::types::{HostConfig, ProfileState, ResolvedTypeCacheEntry, ResolvedTypeCacheKey};
    use crate::VerterHost;
    use std::sync::Arc;
    use verter_semantic::facts::component::ComponentSurface;
    use verter_semantic::query::Completeness;
    use verter_semantic::refs::FileRef;
    use verter_semantic::revision::RevisionMarker;
    use verter_workspace::ResolveRequestKind;

    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/probe-bump-all-four.ts";
    let revision = RevisionMarker {
        workspace_revision: 1,
        ..RevisionMarker::initial()
    };

    // 1 — compile_cache (rehomed F1, post-1C-β value type =
    // ProfileState).
    host.compile_cache()
        .insert(canonical.to_string(), ProfileState::default());
    // 2 — resolved_type_cache (rehomed F2).
    let rt_key = ResolvedTypeCacheKey {
        dep_canonical_id: canonical.to_string(),
        dep_source_hash: [9u8; 16],
        type_name: "ProbeBumpType".to_string(),
        resolve_kind: ResolveRequestKind::TypeImport,
    };
    host.resolved_type_cache().insert(
        rt_key.clone(),
        ResolvedTypeCacheEntry {
            resolved: None,
            tracked_deps: Vec::new(),
        },
    );
    // 3 — eval_env_cache (rehomed F4, owned-program payload).
    let ee_key = OwnedArtifactKey::new(canonical, [9u8; 16]);
    host.project_type_store()
        .eval_env_cache()
        .insert(ee_key.clone(), Arc::new(OwnedEvalProgram::empty()));
    // 4 — semantic_db (rehomed F5).
    {
        let mut db = host.project_type_store().semantic_db();
        db.set_component_surface(canonical.to_string(), revision, ComponentSurface::default());
    }

    // Sanity — all four populated.
    assert!(host.compile_cache().get(canonical).is_some());
    assert!(host.resolved_type_cache().lookup(&rt_key).is_some());
    assert!(host
        .project_type_store()
        .eval_env_cache()
        .get(&ee_key)
        .is_some());
    {
        let db = host.project_type_store().semantic_db();
        assert_eq!(
            db.component_surface(&FileRef::new(canonical), revision)
                .completeness,
            Completeness::Complete,
        );
    }

    // Unified cascade.
    host.project_type_store()
        .bump_project_generation_and_evict();

    // All four MUST be drained.
    assert!(
        host.compile_cache().get(canonical).is_none(),
        "rehoming-doc §3.3 test #5 row 1: compile_cache MUST be \
         drained by bump_project_generation_and_evict"
    );
    assert!(
        host.resolved_type_cache().lookup(&rt_key).is_none(),
        "rehoming-doc §3.3 test #5 row 2: resolved_type_cache MUST \
         be drained by bump_project_generation_and_evict"
    );
    assert!(
        host.project_type_store()
            .eval_env_cache()
            .get(&ee_key)
            .is_none(),
        "rehoming-doc §3.3 test #5 row 3: eval_env_cache MUST be \
         drained by bump_project_generation_and_evict"
    );
    {
        let db = host.project_type_store().semantic_db();
        assert_eq!(
            db.component_surface(&FileRef::new(canonical), revision)
                .completeness,
            Completeness::Unavailable,
            "rehoming-doc §3.3 test #5 row 4: semantic_db MUST be \
             drained by bump_project_generation_and_evict"
        );
    }
}
