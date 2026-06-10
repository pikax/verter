//! Discriminating tests for the validation-snapshot
//! foundation: the [`crate::resolver_store::StoreViewManager`], the
//! complete [`crate::resolver_store::StoreViewValidationToken`], the
//! Arc-shared `StoreViewSnapshot`, the no-torn-return build, and the
//! distinct-overlay-token contract.
//!
//! Each token-advance test asserts the token CHANGES for a
//! validation-affecting write source; the negative test asserts an
//! unrelated op does NOT change it. The Arc-reuse test asserts
//! pointer-identity sharing on a token-stable hit and a fresh snapshot
//! on a token change. All FAIL against the pre-change tree, where
//! `HostStoreView` had no Arc-shared snapshot and `from_host` rebuilt
//! the full workspace sweep on every call (no token-keyed cache, no
//! pointer identity to assert, no `validation_token` to compare).

use std::sync::Arc;

use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};

fn host_with_one_file() -> (Arc<VerterHost>, String) {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let canonical = "/proj/a.ts".to_string();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.clone(),
            source: Arc::from("export interface A { x: number }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert a.ts must succeed");
    (host, canonical)
}

// ── Token-advance tests (one per major write source) ──────────────────

#[test]
fn token_advances_on_source_content_upsert() {
    let (host, canonical) = host_with_one_file();
    let before = host.current_validation_token();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.clone(),
            source: Arc::from("export interface A { x: number; y: string }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("re-upsert with changed content must succeed");
    let after = host.current_validation_token();
    assert_ne!(
        before, after,
        "a content upsert MUST advance the StoreViewValidationToken"
    );
}

#[test]
fn token_advances_on_evict() {
    let (host, canonical) = host_with_one_file();
    let before = host.current_validation_token();
    host.evict(&canonical);
    let after = host.current_validation_token();
    assert_ne!(
        before, after,
        "an evict MUST advance the StoreViewValidationToken"
    );
}

#[test]
fn token_advances_on_clear_compile_cache() {
    let (host, _canonical) = host_with_one_file();
    let before = host.current_validation_token();
    host.clear_compile_cache();
    let after = host.current_validation_token();
    assert_ne!(
        before, after,
        "clear_compile_cache MUST advance the StoreViewValidationToken"
    );
}

#[test]
fn token_advances_on_configure_projects_project_generation() {
    let (host, _canonical) = host_with_one_file();
    let before = host.current_validation_token();
    // `configure_projects` runs `bump_project_generation_and_evict` —
    // the project-shape dimension of the token.
    host.configure_projects(Vec::new());
    let after = host.current_validation_token();
    assert_ne!(
        before, after,
        "a project-generation change (configure_projects) MUST advance the token"
    );
    assert_ne!(
        before.project_generation, after.project_generation,
        "the project_generation token dimension specifically must change"
    );
}

#[test]
fn token_advances_on_set_import_dependencies() {
    let (host, canonical) = host_with_one_file();
    let before = host.current_validation_token();
    // `set_import_dependencies` records import routes + the known-miss
    // generation sidecar and bumps `store_view_epoch`.
    host.set_import_dependencies(
        &canonical,
        vec![crate::types::DependencyResolution {
            specifier: "./b".to_string(),
            resolved_canonical_id: Some("/proj/b.ts".to_string()),
            possible_canonical_ids: vec!["/proj/b.ts".to_string()],
        }],
    );
    let after = host.current_validation_token();
    assert_ne!(
        before, after,
        "set_import_dependencies MUST advance the StoreViewValidationToken"
    );
}

#[test]
fn token_advances_on_close() {
    let (host, _canonical) = host_with_one_file();
    let before = host.current_validation_token();
    host.close();
    let after = host.current_validation_token();
    assert_ne!(
        before, after,
        "close MUST advance the StoreViewValidationToken"
    );
}

#[test]
fn token_advances_on_lazy_indexed_artifact_publication() {
    // A lazy `ensure_indexed_ready` publication advances the
    // FileArtifactStore `artifact_generation`, hence the FULL reuse
    // token — even though it does NOT bump `store_view_epoch`. This is
    // the by-value-snapshot-dimension coverage for indexed-artifact
    // publication: without it a manager-cached base view would go stale
    // after the publication and warm-hit validation would false-miss.
    let (host, _canonical) = host_with_one_file();
    // Upsert a second file so it is tracked but not yet materialised
    // into FileArtifactStore. (The upsert itself bumps the epoch; we
    // capture `before` AFTER it so the artifact-generation dimension is
    // isolated from the epoch dimension.)
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/proj/b.ts".to_string(),
            source: Arc::from("export interface B { y: number }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert b.ts");

    let before = host.current_validation_token();
    let before_epoch = before.store_view_epoch;

    // `ensure_indexed_ready` publishes the IndexedReady into
    // FileArtifactStore → bumps `artifact_generation` WITHOUT bumping
    // `store_view_epoch`.
    let _ = host.ensure_indexed_ready("/proj/b.ts");

    let after = host.current_validation_token();
    assert_ne!(
        before.artifact_generation, after.artifact_generation,
        "a lazy indexed-artifact publication MUST advance the artifact_generation \
         token dimension (by-value snapshot coverage)"
    );
    assert_eq!(
        before_epoch, after.store_view_epoch,
        "a lazy indexed-artifact publication MUST NOT bump store_view_epoch — \
         the artifact_generation dimension is what covers it (proving the token \
         is a strict superset of the epoch)"
    );
    assert_ne!(
        before, after,
        "the full reuse token MUST change after a lazy artifact publication"
    );
}

#[test]
fn token_advances_on_route_owned_shallow_publish() {
    // `RouteOwnedShallowDb::publish` bumps the route-owned generation,
    // which the token folds via `route_owned_generation`. The base view
    // snapshots the route-owned `Route` derived-hash fallback by value,
    // so a publish MUST advance the token or a manager-cached base view
    // goes stale.
    let (host, _canonical) = host_with_one_file();
    let before = host.current_validation_token();
    let before_route_owned = before.route_owned_generation;

    let entry = Arc::new(
        crate::project_type_store::RouteOwnedShallowEntry::test_stub(Arc::from("/proj/r.ts")),
    );
    host.project_type_store()
        .route_owned_shallow()
        .publish(Arc::from("/proj/r.ts"), entry);

    let after = host.current_validation_token();
    assert_ne!(
        before_route_owned, after.route_owned_generation,
        "a route-owned-shallow publish MUST advance the route_owned_generation \
         token dimension"
    );
    assert_ne!(
        before, after,
        "the full reuse token MUST change after a route-owned-shallow publish"
    );
}

#[test]
fn token_advances_on_route_owned_shallow_schema_eviction() {
    // A schema-version reconciliation sweep that drains route-owned
    // entries through `evict_if_schema_mismatch` mutates the SAME
    // `route_owned_shallow` derived-hash source the base view snapshots
    // by value. It MUST advance `route_owned_generation` exactly like
    // `publish` / `remove` / `clear_all` do — else a `HostStoreView`
    // snapshotted before the sweep keeps validating against route-owned
    // `Route` derived hashes the store can no longer reproduce.
    use crate::cache_schema::{CacheSchemaVersioned, CACHE_CLUSTER_SCHEMA_VERSION};

    let (host, _canonical) = host_with_one_file();

    // Seed one route-owned entry so the schema sweep has a row to drain;
    // the publish itself advances the generation, so capture the snapshot
    // token AFTER seeding (the view a request would hold over the sweep).
    let entry = Arc::new(
        crate::project_type_store::RouteOwnedShallowEntry::test_stub(Arc::from("/proj/r.ts")),
    );
    host.project_type_store()
        .route_owned_shallow()
        .publish(Arc::from("/proj/r.ts"), entry);

    let snapshotted = host.current_validation_token();
    let snapshotted_route_owned = snapshotted.route_owned_generation;

    // Force the schema-mismatch eviction branch on the live production
    // store (built at `CACHE_CLUSTER_SCHEMA_VERSION`) by reconciling
    // against a HIGHER cluster version — the exact shape of a cluster
    // schema bump landing past the version this store was constructed
    // under. The one seeded row is drained.
    let evicted = host
        .project_type_store()
        .route_owned_shallow()
        .evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION + 1);
    assert_eq!(
        evicted, 1,
        "the schema-mismatch sweep MUST drain the one seeded route-owned entry"
    );

    let after = host.current_validation_token();
    assert_ne!(
        snapshotted_route_owned, after.route_owned_generation,
        "a route-owned-shallow schema eviction that drains entries MUST advance \
         the route_owned_generation token dimension (else a pre-sweep snapshot \
         keeps validating against drained route-owned derived hashes)"
    );
    assert_ne!(
        snapshotted, after,
        "the previously-snapshotted token MUST be stale after a route-owned \
         schema eviction"
    );
}

#[test]
fn token_stable_on_route_owned_shallow_schema_eviction_noop() {
    // Bump-iff-CHANGED: a schema-mismatch reconciliation that drains
    // NOTHING (the store is already empty) must NOT advance
    // `route_owned_generation`. Mirrors the no-op augmenter-reinsert
    // gating — a no-op eviction is not a token-relevant mutation.
    use crate::cache_schema::{CacheSchemaVersioned, CACHE_CLUSTER_SCHEMA_VERSION};

    let (host, _canonical) = host_with_one_file();

    // No route-owned entry seeded — the store is empty.
    let before = host.current_validation_token();
    let before_route_owned = before.route_owned_generation;

    let evicted = host
        .project_type_store()
        .route_owned_shallow()
        .evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION + 1);
    assert_eq!(
        evicted, 0,
        "an empty store drains nothing on schema reconciliation"
    );

    let after = host.current_validation_token();
    assert_eq!(
        before_route_owned, after.route_owned_generation,
        "a no-op schema eviction (nothing drained) MUST NOT advance the \
         route_owned_generation token dimension (bump-iff-changed)"
    );
    assert_eq!(
        before, after,
        "the token MUST stay stable across a no-op schema eviction"
    );
}

#[test]
fn token_advances_on_env_hash_change() {
    // The token folds the four R21 env hashes directly (a self-contained
    // defence even if a future workspace mutator changed env without
    // bumping a generation). Changing the workspace resolve extensions
    // changes `resolve_env_hash`, hence the folded `env_hash_fold`.
    let (host, _canonical) = host_with_one_file();
    let before = host.current_validation_token();
    let before_fold = before.env_hash_fold;

    // `set_default_resolve_extensions` is ADDITIVE (it merges into the
    // static `probe_extensions()` set), so a guaranteed-novel extension
    // changes the merged list and hence `resolve_env_hash`.
    host.ws()
        .set_default_resolve_extensions(vec![".zzzcustomext".to_string()]);

    let after = host.current_validation_token();
    assert_ne!(
        before_fold, after.env_hash_fold,
        "a resolve-extensions change MUST change the env_hash_fold token dimension"
    );
    assert_ne!(
        before, after,
        "the full reuse token MUST change after an env-hash change"
    );
}

#[test]
fn token_carries_project_identity_dimension() {
    // The token folds `project_identity` directly. The
    // workspace-default identity is a constant for a given workspace, so
    // we discriminate the DIMENSION (not a runtime mutation) by asserting
    // that two otherwise-identical tokens that differ ONLY in
    // `project_identity` compare unequal — i.e. the field participates in
    // the derived `PartialEq`/`Hash`. A regression that dropped the field
    // from the token would make these compare equal.
    let (host, _canonical) = host_with_one_file();
    let base = host.current_validation_token();
    let mut other = base;
    other.project_identity = crate::file_artifact_store::ProjectIdentity([0x5Au8; 16]);
    assert_ne!(
        base.project_identity, other.project_identity,
        "the two tokens must differ in the project_identity dimension"
    );
    assert_ne!(
        base, other,
        "the project_identity dimension MUST participate in token identity \
         (a token differing only in project_identity must not compare equal)"
    );
}

#[test]
fn token_advances_on_augmentation_index_populate() {
    // The augmentation-index populate path bumps `artifact_generation`
    // (the base view snapshots `route_surface_index_fingerprints` by
    // value). A populate MUST advance the token.
    use crate::file_artifact_store::{
        AugmentationPopulation, AugmentationTargetKey, AugmentationTargetKind, AugmenterSet,
        ProjectIdentity,
    };
    use smallvec::SmallVec;

    let (host, _canonical) = host_with_one_file();
    let before = host.current_validation_token();
    let before_artifact = before.artifact_generation;

    let key = AugmentationTargetKey {
        project_identity: ProjectIdentity([0u8; 16]),
        resolve_env_hash: [0u8; 16],
        lib_env_hash: [0u8; 16],
        population: AugmentationPopulation::Base,
        target: AugmentationTargetKind::GlobalAugmentation,
    };
    let set = Arc::new(AugmenterSet {
        entries: SmallVec::new(),
        fingerprint: [7u8; 16],
    });
    host.project_type_store()
        .indexed()
        .populate_augmenter_set(key, set);

    let after = host.current_validation_token();
    assert_ne!(
        before_artifact, after.artifact_generation,
        "an augmentation-index populate MUST advance the artifact_generation \
         token dimension (route_surface_index_fingerprints is snapshotted by value)"
    );
    assert_ne!(
        before, after,
        "the full reuse token MUST change after an augmentation-index populate"
    );
}

#[test]
fn token_advances_on_first_time_additive_load_via_load_generation() {
    // SOUNDNESS: a first-time additive
    // `ensure_loaded` adds a scheduler node + `derived_raw_cache` state a
    // base view snapshots BY VALUE but does NOT publish into
    // `FileArtifactStore`, so it advances the DEDICATED `load_generation`
    // dimension. The full reuse token MUST change (invalidating a
    // manager-cached base view), but the load MUST NOT count as an
    // EXTERNAL supersession — else a scalar/batch cold compute that loads
    // a dependency would self-fence its own result promotion.
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};
    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(
        "/proj/dep.ts".to_string(),
        Arc::from("export interface D { d: number }\n"),
    );
    let ws_access: Arc<dyn WorkspaceAccess> = ws;
    let host = VerterHost::new(HostConfig::default(), ws_access);

    let before = host.current_validation_token();
    let before_load = before.load_generation;

    assert!(
        host.ensure_loaded("/proj/dep.ts"),
        "first-time ensure_loaded must succeed"
    );

    let after = host.current_validation_token();
    assert_ne!(
        before_load, after.load_generation,
        "a first-time additive load MUST advance the load_generation token dimension"
    );
    assert_ne!(
        before, after,
        "the full reuse token MUST change after a first-time additive load \
         (so a manager-cached base view is invalidated)"
    );
    // The load did NOT externally-supersede: epoch / project-gen / env /
    // identity are unchanged, so a cold compute's own dependency loads do
    // not self-fence the publish fence.
    assert!(
        !before.externally_superseded_by(&after),
        "a first-time additive load (own-work) MUST NOT count as an EXTERNAL \
         supersession — the publish fence must not self-fence on dependency loads"
    );
    // And specifically: the epoch did NOT move (the dedicated
    // load_generation dimension is what covers it).
    assert_eq!(
        before.store_view_epoch, after.store_view_epoch,
        "a first-time additive load MUST NOT bump store_view_epoch — the dedicated \
         load_generation dimension covers it (proving load-gen is a strict addition \
         the publish fence can exclude)"
    );
}

#[test]
fn token_advances_on_keyed_artifact_removal_gc() {
    // SOUNDNESS: keyed artifact removal / reachability GC drops by-value
    // snapshot dimensions a base view holds, so it MUST advance
    // `artifact_generation`. `FileArtifactStore::remove_artifacts` (the
    // method the GC routes every unreachable version through) must bump
    // the generation; otherwise a manager-cached base view would persist
    // stale across a GC.
    let (host, _canonical) = host_with_one_file();
    // Materialise an indexed artifact for a second file so the GC has a
    // removable artifact. Capture `before` AFTER the materialise so the
    // removal dimension is isolated from the publish dimension.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/proj/gc.ts".to_string(),
            source: Arc::from("export interface G { z: number }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert gc.ts");
    let _ = host.ensure_indexed_ready("/proj/gc.ts");

    let before = host.current_validation_token();
    let before_artifact = before.artifact_generation;

    // Reachability GC with an EMPTY live set → every artifact is
    // unreachable and removed through `remove_artifacts`.
    host.project_type_store().evict_unreachable_artifacts(
        &rustc_hash::FxHashSet::default(),
        false,
        0,
    );

    let after = host.current_validation_token();
    assert_ne!(
        before_artifact, after.artifact_generation,
        "a keyed artifact removal / reachability GC MUST advance the \
         artifact_generation token dimension — else a manager-cached base view \
         survives stale across a GC"
    );
    assert_ne!(
        before, after,
        "the full reuse token MUST change after a keyed artifact removal / GC"
    );
}

// ── Negative test: an unrelated op does NOT advance the token ─────────

#[test]
fn token_does_not_advance_on_pure_read() {
    let (host, _canonical) = host_with_one_file();
    let before = host.current_validation_token();
    // Pure reads: snapshotting the view, re-reading the token. None of
    // these is a validation-affecting write source.
    let _view1 = host.resolver_store_view_read().into_owned_view();
    let _view2 = host.resolver_store_view_read().into_owned_view();
    let _token = host.current_validation_token();
    let after = host.current_validation_token();
    assert_eq!(
        before, after,
        "a pure read (resolver_store_view / token capture) MUST NOT advance the token"
    );
}

#[test]
fn token_does_not_advance_on_byte_identical_reupsert() {
    let (host, canonical) = host_with_one_file();
    let before = host.current_validation_token();
    // R1: a byte-identical re-upsert is a true cache no-op and must not
    // bump the epoch, hence must not advance the token.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.clone(),
            source: Arc::from("export interface A { x: number }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("byte-identical re-upsert must succeed");
    let after = host.current_validation_token();
    assert_eq!(
        before, after,
        "a byte-identical re-upsert MUST NOT advance the token (R1 no-op)"
    );
}

#[test]
fn token_does_not_advance_on_noop_reachability_gc() {
    // Negative counterpart to `token_advances_on_keyed_artifact_removal_gc`:
    // a reachability GC whose live set COVERS every artifact removes
    // nothing, so no keyed removal fires and the token MUST stay stable.
    // This proves the GC bump is gated on an actual removal, not emitted
    // unconditionally on every sweep.
    let (host, _canonical) = host_with_one_file();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/proj/keep.ts".to_string(),
            source: Arc::from("export interface K { k: number }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert keep.ts");
    let indexed = host
        .ensure_indexed_ready("/proj/keep.ts")
        .expect("keep.ts must have an IndexedReady");

    let before = host.current_validation_token();

    // Live set covers the single artifact's (canonical, whole_hash)
    // projection → nothing is unreachable → no removal.
    let mut live: rustc_hash::FxHashSet<(Arc<str>, crate::types::Hash16)> =
        rustc_hash::FxHashSet::default();
    live.insert((Arc::from("/proj/keep.ts"), indexed.whole_hash));
    host.project_type_store()
        .evict_unreachable_artifacts(&live, false, 0);

    let after = host.current_validation_token();
    assert_eq!(
        before, after,
        "a reachability GC that removes nothing MUST NOT advance the token"
    );
}

#[test]
fn token_does_not_advance_on_redundant_ensure_loaded() {
    // Negative counterpart to the first-time-load bump
    // (`meta_tests::ensure_loaded_first_time_advances_the_validation_token`):
    // a SECOND `ensure_loaded` of an already-loaded file is a fast-path
    // no-op (the scheduler already has the source and the file is not
    // evicted), so it MUST NOT advance the token. This proves the bump is
    // gated on an actual load, not emitted on every `ensure_loaded` call.
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};
    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(
        "/proj/loaded.ts".to_string(),
        Arc::from("export interface L { y: number }\n"),
    );
    let ws_access: Arc<dyn WorkspaceAccess> = ws;
    let host = VerterHost::new(HostConfig::default(), ws_access);

    // First load — additive, advances the token (covered positively
    // elsewhere). Capture `before` AFTER it so the redundant call is
    // isolated.
    assert!(
        host.ensure_loaded("/proj/loaded.ts"),
        "first ensure_loaded must succeed"
    );
    let before = host.current_validation_token();

    // Second load — fast-path no-op (already loaded, not evicted).
    assert!(
        host.ensure_loaded("/proj/loaded.ts"),
        "redundant ensure_loaded of a loaded file must succeed"
    );
    let after = host.current_validation_token();
    assert_eq!(
        before, after,
        "a redundant ensure_loaded of an already-loaded file MUST NOT advance \
         the token (no actual additive load occurred)"
    );
}

// ── from_host reuses the cached Arc snapshot when the token is stable ──

#[test]
fn from_host_reuses_cached_arc_when_token_stable() {
    let (host, _canonical) = host_with_one_file();

    // First call builds and caches the base view.
    let view1 = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.store_view_manager().is_populated(),
        "the first resolver_store_view call must populate the manager cache"
    );
    let ptr1 = view1.snapshot_ptr_for_tests();

    // Second call, token unchanged: must hand back the SAME shared
    // `Arc<StoreViewSnapshot>` (pointer identity), not a fresh sweep.
    let view2 = host.resolver_store_view_read().into_owned_view();
    let ptr2 = view2.snapshot_ptr_for_tests();
    assert_eq!(
        ptr1, ptr2,
        "token-stable resolver_store_view MUST reuse the cached Arc snapshot \
         (pointer identity), not rebuild the workspace sweep"
    );
    assert_eq!(
        view1.validation_token_for_tests(),
        view2.validation_token_for_tests(),
        "two token-stable views must report the same validation token"
    );
}

#[test]
fn from_host_rebuilds_after_token_change() {
    let (host, canonical) = host_with_one_file();

    let view1 = host.resolver_store_view_read().into_owned_view();
    let ptr1 = view1.snapshot_ptr_for_tests();
    let token1 = view1.validation_token_for_tests();

    // Mutate: a content upsert advances the token.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.clone(),
            source: Arc::from("export interface A { x: number; z: boolean }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("re-upsert with changed content must succeed");

    let view2 = host.resolver_store_view_read().into_owned_view();
    let ptr2 = view2.snapshot_ptr_for_tests();
    let token2 = view2.validation_token_for_tests();

    assert_ne!(
        token1, token2,
        "the token must change across a content upsert"
    );
    assert_ne!(
        ptr1, ptr2,
        "a token change MUST rebuild the snapshot (distinct Arc), not reuse the stale cache"
    );
    assert_eq!(
        host.store_view_manager().cached_token(),
        Some(token2),
        "the manager must republish the freshly-built snapshot's token"
    );
}

// ── No-torn-return: build_coherent caches a coherent (token-matching) view ──

#[test]
fn manager_caches_coherent_view_matching_live_token() {
    let (host, _canonical) = host_with_one_file();
    // A quiescent host (no concurrent mutation) always produces a
    // coherent build; the cached token must equal the live token, and
    // the view's own `validation_token()` must equal the cached token.
    let view = host.resolver_store_view_read().into_owned_view();
    let cached = host
        .store_view_manager()
        .cached_token()
        .expect("manager must be populated after resolver_store_view");
    assert_eq!(
        cached,
        host.current_validation_token(),
        "the cached coherent view's token must equal the live host token \
         (no torn / superseded view published)"
    );
    assert_eq!(
        view.validation_token_for_tests(),
        cached,
        "the returned view's own token must equal the cached token"
    );
}

#[test]
fn build_coherent_reports_superseded_under_mid_build_mutation() {
    let (host, _canonical) = host_with_one_file();
    // Force a mid-build mutation on every retry attempt: the builder
    // must exhaust its retries and report `Superseded`, NOT publish a
    // (potentially torn) view. This discriminates against the retired
    // "retry 3× then return-anyway" behaviour.
    assert!(
        crate::resolver_store::HostStoreView::build_coherent_is_superseded_for_tests(&host),
        "a build that observes a mutation on every attempt MUST report Superseded, \
         never publish a torn view"
    );
    // The manager still hands back a coherent view afterwards (the knob
    // is reset; a quiescent build succeeds), proving Superseded is a
    // transient signal, not a permanent failure.
    let view = host.resolver_store_view_read().into_owned_view();
    assert_eq!(
        view.validation_token_for_tests(),
        host.current_validation_token(),
        "after a transient supersession the manager must still yield a coherent view"
    );
}

// ── Distinct completion/session overlays → distinct token identities ──

#[test]
fn distinct_overlays_yield_distinct_token_identities() {
    use crate::session_view::OverlaidView;
    use rustc_hash::FxHashMap;

    let (host, canonical) = host_with_one_file();
    let env = host.host_view_env_hashes();

    // Two session views over the SAME base host but DIFFERENT overlay
    // content for the same canonical. Their views must carry distinct
    // validation-token identities: a later block's
    // proof memo must never cross an overlay boundary.
    let mut overlays_a: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays_a.insert(
        canonical.clone(),
        Arc::from("export interface A { v: 1 }\n"),
    );
    let mut hashes_a: FxHashMap<String, crate::types::Hash16> = FxHashMap::default();
    hashes_a.insert(canonical.clone(), [0xAAu8; 16]);
    let view_a = OverlaidView::with_overlay_hashes(
        Arc::clone(&host),
        Arc::new(overlays_a),
        Arc::new(hashes_a),
        env,
    );

    let mut overlays_b: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays_b.insert(
        canonical.clone(),
        Arc::from("export interface A { v: 2 }\n"),
    );
    let mut hashes_b: FxHashMap<String, crate::types::Hash16> = FxHashMap::default();
    hashes_b.insert(canonical.clone(), [0xBBu8; 16]);
    let view_b = OverlaidView::with_overlay_hashes(
        Arc::clone(&host),
        Arc::new(overlays_b),
        Arc::new(hashes_b),
        env,
    );

    let store_view_a = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view_a);
    let store_view_b = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view_b);

    let token_a = store_view_a.validation_token_for_tests();
    let token_b = store_view_b.validation_token_for_tests();

    assert_ne!(
        token_a, token_b,
        "two session views with different overlay content for the same canonical \
         MUST carry distinct StoreViewValidationToken identities"
    );

    // And both differ from the base (no-overlay) token.
    let base_token = host
        .resolver_store_view_read()
        .into_owned_view()
        .validation_token_for_tests();
    assert_ne!(
        token_a, base_token,
        "a session-overlaid view's token must differ from the base token"
    );
    assert!(
        token_a.overlay_identity.is_some(),
        "a session-overlaid view's token must carry an overlay identity"
    );
    assert!(
        base_token.overlay_identity.is_none(),
        "a base (non-overlay) view's token must carry no overlay identity"
    );
}

#[test]
fn base_view_overlay_does_not_mutate_shared_snapshot() {
    use crate::session_view::OverlaidView;
    use rustc_hash::FxHashMap;

    let (host, canonical) = host_with_one_file();
    let env = host.host_view_env_hashes();

    // Build a base view and capture its shared snapshot pointer.
    let base = host.resolver_store_view_read().into_owned_view();
    let base_ptr = base.snapshot_ptr_for_tests();

    // A session overlay re-roots one canonical. Copy-on-write: the
    // overlaid view's snapshot must be a DISTINCT Arc (the base shared
    // snapshot is never mutated in place), while a SECOND base view
    // still shares the original cached snapshot.
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(
        canonical.clone(),
        Arc::from("export interface A { w: 9 }\n"),
    );
    let mut hashes: FxHashMap<String, crate::types::Hash16> = FxHashMap::default();
    hashes.insert(canonical.clone(), [0xCCu8; 16]);
    let view = OverlaidView::with_overlay_hashes(
        Arc::clone(&host),
        Arc::new(overlays),
        Arc::new(hashes),
        env,
    );
    let overlaid = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view);
    assert_ne!(
        base_ptr,
        overlaid.snapshot_ptr_for_tests(),
        "a session overlay that re-roots a canonical MUST copy-on-write \
         (distinct Arc), never mutate the shared base snapshot in place"
    );

    // The manager's cached base snapshot is still the original.
    let base_again = host.resolver_store_view_read().into_owned_view();
    assert_eq!(
        base_ptr,
        base_again.snapshot_ptr_for_tests(),
        "the shared base snapshot must stay pristine after a copy-on-write overlay"
    );
}

/// A cold component-meta compute publishes indexed artifacts (advancing
/// `artifact_generation`), so the FULL reuse token changes across the
/// cold window — but the EXTERNALLY-driven dimensions
/// (`store_view_epoch` / `project_generation` / env / identity) do NOT.
/// The publish fence keys off `externally_superseded_by`, so the cold
/// result IS promoted and the second identical query is a warm hit.
/// Discriminates against keying the publish fence off the full token
/// (which would self-fence every cold compute that loads a dependency).
#[test]
fn cold_compute_artifact_publication_does_not_self_supersede_publish() {
    use crate::types::HostConfig;
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};
    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(
        "/A.vue".into(),
        Arc::from(
            "<script setup lang=\"ts\">\nimport type { BProps } from './B'\ndefineProps<BProps>()\n</script>\n<template><div /></template>\n",
        ),
    );
    ws.inject_file(
        "/B.ts".into(),
        Arc::from("export interface BProps { foo: string }\n"),
    );
    let ws_access: Arc<dyn WorkspaceAccess> = ws;
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws_access));

    let before = host.current_validation_token();
    let _ = host.get_component_meta("/A.vue");
    let after = host.current_validation_token();

    // The cold compute published indexed artifacts → the FULL token's
    // artifact generation advanced.
    assert_ne!(
        before.artifact_generation, after.artifact_generation,
        "a cold compute that loads a dep MUST advance the artifact generation \
         (it publishes IndexedReady)"
    );
    // But it did NOT externally-supersede: epoch / generation / env /
    // identity are unchanged, so the publish fence promoted the result.
    assert!(
        !before.externally_superseded_by(&after),
        "a cold compute's OWN artifact publication MUST NOT count as an external \
         supersession — else the publish fence would self-fence every cold compute"
    );

    // The result-db entry is present → the publish fence promoted it.
    let whole_hash = host
        .ensure_indexed_ready("/A.vue")
        .map(|ir| ir.whole_hash)
        .expect("owner must have an IndexedReady");
    let key = crate::component_meta_result_db::ComponentMetaResultKey {
        owner_canonical: Arc::from("/A.vue"),
        options_fingerprint: crate::host_manage::component_meta_options_fingerprint(
            &crate::host_manage::ComponentMetaOptions::default(),
        ),
    };
    assert!(
        host.project_type_store()
            .component_meta_results()
            .get(&key, whole_hash)
            .is_some(),
        "a cold compute whose own artifact publication is the only token change \
         MUST still be promoted (publish fence keys off external supersession)"
    );
}

// ── Singleflight: concurrent token-miss callers share ONE cold sweep ──

#[test]
fn cold_store_view_build_is_singleflighted_across_concurrent_callers() {
    // PERF SOUNDNESS: on a token miss the manager must NOT let
    // N concurrent callers each run `build_coherent` (N full-workspace
    // sweeps). Exactly one caller sweeps; the rest WAIT on the condvar and
    // clone the winner's `Arc<StoreViewSnapshot>`.
    //
    // Discrimination: spin N threads that all request the base view after
    // a fresh token (the cache is cold). With singleflight only ONE thread
    // sweeps; the rest WAIT on the condvar and clone the winner's Arc.
    //
    // The sweep count is measured PER-THREAD (each spawned thread reports
    // its `COHERENT_BUILD_SWEEPS_THIS_THREAD`, summed by the test) rather
    // than via the process-wide counter. The process-wide counter is
    // inflated by unrelated parallel tests that build store views; the
    // thread-local counter captures ONLY the sweeps these N threads ran,
    // so the assertion is robust against cross-test contamination. With
    // singleflight the sum is a tiny number (1 plus at most a couple of
    // supersession retries); without it every thread would sweep, so the
    // sum would be ~N. We also assert all threads observe the SAME shared
    // snapshot pointer.
    use crate::resolver_store::COHERENT_BUILD_SWEEPS_THIS_THREAD;
    use std::sync::Barrier;

    const THREADS: usize = 16;

    let (host, _canonical) = host_with_one_file();
    // Warm the manager once, then advance the token so the next wave is a
    // guaranteed cold miss for every thread simultaneously.
    let _ = host.resolver_store_view_read().into_owned_view();
    host.bump_store_view_epoch();

    let barrier = Arc::new(Barrier::new(THREADS));
    let mut handles = Vec::with_capacity(THREADS);
    for _ in 0..THREADS {
        let host = Arc::clone(&host);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            // Each spawned thread starts with a fresh (0) thread-local
            // sweep counter; reset defensively in case the worker thread is
            // reused across tests.
            COHERENT_BUILD_SWEEPS_THIS_THREAD.with(|c| c.set(0));
            barrier.wait();
            let ptr = host
                .resolver_store_view_read()
                .into_owned_view()
                .snapshot_ptr_for_tests() as usize;
            let sweeps = COHERENT_BUILD_SWEEPS_THIS_THREAD.with(std::cell::Cell::get);
            (ptr, sweeps)
        }));
    }
    let results: Vec<(usize, u64)> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // Sum the per-thread sweeps these N threads ran. Robust against
    // concurrent parallel-test build activity (which never touches THESE
    // threads' thread-locals).
    let sweeps: u64 = results.iter().map(|(_ptr, s)| *s).sum();
    // Strictly far below the thread count — without singleflight this
    // would be ~THREADS. Allow a small slack for `Superseded` retries; the
    // separation between "singleflight" (≈1) and "N parallel sweeps"
    // (=16) is wide enough that THREADS / 2 is an unambiguous gate.
    assert!(
        sweeps < (THREADS as u64) / 2,
        "concurrent token-miss callers MUST singleflight the cold sweep: \
         expected far fewer than {THREADS} sweeps across the wave, observed {sweeps}"
    );

    // Every thread observed the same shared snapshot pointer — they
    // cloned one winner's Arc, not N independent builds.
    let first = results[0].0;
    assert!(
        results.iter().all(|(p, _s)| *p == first),
        "all concurrent token-miss callers MUST share one Arc<StoreViewSnapshot> \
         (singleflight winner), observed distinct pointers: {results:?}"
    );
}

// ── Singleflight: a builder PANIC must not strand the build claim ─────

#[test]
fn builder_panic_does_not_permanently_hang_subsequent_callers() {
    // SOUNDNESS: the cold-build claim (`building = true`) is
    // cleared and joiners woken by a RAII guard whose `Drop` runs on
    // EVERY exit path. parking_lot mutexes do NOT poison, so a builder
    // that panics mid-build must STILL release the claim — otherwise
    // `building` stays `true` forever and every current joiner AND every
    // future caller blocks permanently on the `built` condvar.
    //
    // Discrimination: arm the one-shot mid-build panic knob, drive a cold
    // build (token miss) and catch the unwind, then assert a FRESH cold
    // build still RETURNS. The fresh call runs on a watchdog thread joined
    // with a timeout — a regression (stranded claim) makes the watchdog
    // time out and the test FAIL rather than hang CI. Without the RAII
    // guard (plain `state.building = false` statements that the panic
    // unwinds past) the claim would stay set and the watchdog would time
    // out; the guard clears it on unwind and the watchdog returns.
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::mpsc;
    use std::time::Duration;

    let (host, _canonical) = host_with_one_file();

    // Warm the manager once, then advance the token so the next request is
    // a guaranteed cold miss (forces the claim-then-build path, not a warm
    // Arc-clone hit).
    let _ = host.resolver_store_view_read().into_owned_view();
    host.bump_store_view_epoch();

    // Arm the one-shot panic and drive the cold build. The panic unwinds
    // out of `build_coherent`; `catch_unwind` swallows it. With the RAII
    // guard the claim is released during the unwind.
    crate::resolver_store::HostStoreView::arm_build_panic_for_tests();
    let panicked = catch_unwind(AssertUnwindSafe(|| {
        let _ = host.resolver_store_view_read().into_owned_view();
    }))
    .is_err();
    assert!(
        panicked,
        "the armed mid-build panic knob MUST cause the cold build to panic \
         (otherwise the regression is not being exercised)"
    );

    // A FRESH cold build must now RETURN. Run it on a watchdog thread so a
    // stranded claim (the regression) surfaces as a TIMEOUT/FAILURE, never
    // a hung CI process.
    let token_after_panic = host.current_validation_token();
    let host_for_watchdog = Arc::clone(&host);
    let (tx, rx) = mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        // The knob is one-shot and already disarmed, so this build does
        // not panic.
        let view = host_for_watchdog
            .resolver_store_view_read()
            .into_owned_view();
        // Sanity: the returned view is coherent against the live token.
        assert_eq!(
            view.validation_token_for_tests(),
            token_after_panic,
            "the recovered build must yield a coherent view"
        );
        let _ = tx.send(());
    });

    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(()) => {
            watchdog
                .join()
                .expect("watchdog thread must not itself panic");
        }
        Err(_) => panic!(
            "REGRESSION: a builder panic stranded the singleflight build claim — a \
             subsequent resolver_store_view() call blocked forever on the `built` \
             condvar (RAII claim guard missing / claim not cleared on panic unwind)"
        ),
    }
}

// ── compat_token folds the EXTERNAL-supersession dimensions (coalescing-lane == promotion oracle) ──

#[test]
fn compat_token_lane_oracle_ignores_additive_generations() {
    // SOUNDNESS: the singleflight / stability coalescing lane keys on
    // `compat_token`, and a follower in `run_stable_request` receives the
    // leader's stable result WITHOUT revalidating it against its own view.
    // The lane oracle MUST therefore be EXACTLY as strict as the promotion
    // fence (`is_stable`), which gates on `external_supersession_fingerprint`
    // and DELIBERATELY EXCLUDES the additive `artifact_generation` /
    // `route_owned_generation` / `load_generation`: a cold compute advances
    // those generations as its OWN work (publishing artifacts, loading
    // dependencies). The leader promotes a result computed while those
    // generations advanced; if the lane oracle were STRICTER than the
    // promotion oracle (folding the additive generations), then two
    // concurrent identical cold requests that snapshot at different points in
    // the load sweep would fork into distinct lanes and each compute its own
    // result — multiple cold winners instead of one leader + N-1 dedup-
    // joining followers. So a lazy `ensure_indexed_ready` artifact publication
    // (which bumps ONLY `artifact_generation`, no epoch) MUST leave
    // `compat_token` UNCHANGED.
    //
    // Discrimination: a `compat_token` that folded the COMPLETE token (the
    // over-strict pre-fix behaviour) would change `validity_fingerprint`
    // after the artifact publication and fork the lane — exactly the
    // multiple-cold-winner regression this proves closed. The companion test
    // `compat_token_changes_on_external_supersession_without_epoch` proves the
    // oracle still discriminates a REAL external mutation.
    use crate::resolver_core::StoreView;

    let (host, _canonical) = host_with_one_file();
    // Upsert a second file (tracked, not yet materialised). Capture the
    // compat token AFTER the upsert so the epoch dimension is isolated.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/proj/lazy.ts".to_string(),
            source: Arc::from("export interface Z { z: number }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert lazy.ts");

    let before_view = host.resolver_store_view_read().into_owned_view();
    let before_token = before_view.compat_token();
    let before_epoch = host.current_store_view_epoch();

    // Lazy indexed-artifact publication → bumps `artifact_generation`
    // WITHOUT bumping `store_view_epoch`. This is the compute's OWN work,
    // not an external supersession.
    let _ = host.ensure_indexed_ready("/proj/lazy.ts");

    // The epoch did NOT move (precondition for this discrimination).
    assert_eq!(
        before_epoch,
        host.current_store_view_epoch(),
        "a lazy artifact publication must NOT bump store_view_epoch (precondition \
         for this discrimination)"
    );

    let after_token = host
        .resolver_store_view_read()
        .into_owned_view()
        .compat_token();
    assert_eq!(
        before_token.epoch, after_token.epoch,
        "the epoch dimension of compat_token must be unchanged (the artifact \
         publication did not touch store_view_epoch)"
    );
    assert_eq!(
        before_token, after_token,
        "compat_token MUST be UNCHANGED after an additive artifact publication \
         that advances only `artifact_generation` — the coalescing-lane identity \
         is the SAME oracle the promotion fence applies, and that oracle excludes \
         the additive generations a cold compute advances as its own work. Folding \
         them would fork identical concurrent cold requests onto distinct lanes \
         (multiple cold winners)."
    );
    assert_eq!(
        before_token.validity_fingerprint, after_token.validity_fingerprint,
        "the validity_fingerprint dimension specifically must be UNCHANGED (it folds \
         the EXTERNAL-supersession dimensions only, NOT artifact_generation)"
    );
}

#[test]
fn compat_token_changes_on_external_supersession_without_epoch() {
    // SOUNDNESS companion to `compat_token_lane_oracle_ignores_additive_
    // generations`: the lane oracle must STILL discriminate a REAL external
    // supersession — a validity-affecting change in a dimension the promotion
    // fence DOES gate on (env-hash / project-identity / overlay), even when
    // it does NOT move `store_view_epoch`. Two views that would externally-
    // supersede each other MUST get distinct lanes, or a follower could
    // receive a leader's result computed under an externally-stale view.
    //
    // Discrimination: an `{ epoch, session }`-only `compat_token` would leave
    // an env-hash-only change IDENTICAL (its epoch did not move) and the two
    // views would wrongly coalesce. The folded `validity_fingerprint`
    // (external-supersession dims) differs, keeping the lanes distinct.
    use crate::resolver_core::StoreView;

    let (host, _canonical) = host_with_one_file();

    let before_token = host
        .resolver_store_view_read()
        .into_owned_view()
        .compat_token();
    let before_epoch = host.current_store_view_epoch();

    // Shift the resolve-extension env-hash. `set_default_resolve_extensions`
    // is ADDITIVE (merges into the static probe set), so a guaranteed-novel
    // extension changes `resolve_env_hash` → `env_hash_fold` (an EXTERNAL-
    // supersession dimension) WITHOUT bumping `store_view_epoch`.
    host.ws()
        .set_default_resolve_extensions(vec![".zzzcustomext".to_string()]);

    assert_eq!(
        before_epoch,
        host.current_store_view_epoch(),
        "an env-hash shift must NOT bump store_view_epoch (precondition for this \
         discrimination)"
    );

    let after_token = host
        .resolver_store_view_read()
        .into_owned_view()
        .compat_token();
    assert_eq!(
        before_token.epoch, after_token.epoch,
        "the epoch dimension is unchanged (the env-hash shift did not touch \
         store_view_epoch)"
    );
    assert_ne!(
        before_token, after_token,
        "compat_token MUST change after an EXTERNAL-supersession change (env-hash) \
         that does NOT move the epoch — the lane oracle gates on the same external \
         dimensions the promotion fence does"
    );
    assert_ne!(
        before_token.validity_fingerprint, after_token.validity_fingerprint,
        "the validity_fingerprint dimension specifically must change (it folds the \
         external-supersession dimensions, incl. env_hash_fold)"
    );
}

#[test]
fn compat_token_validity_fingerprint_matches_external_supersession_fingerprint() {
    // The base view's `compat_token.validity_fingerprint` MUST equal the
    // view's `external_supersession_fingerprint` — proving the production
    // `HostStoreView` wires the SAME external oracle into the coalescing-lane
    // identity that the promotion fence (`is_stable`) compares, not a partial
    // or constant fingerprint and not the over-strict complete-token fold.
    use crate::resolver_core::StoreView;

    let (host, _canonical) = host_with_one_file();
    let view = host.resolver_store_view_read().into_owned_view();
    let token = view.validation_token_for_tests();
    let expected = token.lane_fingerprint();
    assert_eq!(
        view.compat_token().validity_fingerprint,
        expected,
        "compat_token.validity_fingerprint must fold the external-supersession \
         dimensions (== lane_fingerprint)"
    );
    assert_eq!(
        token.lane_fingerprint(),
        token.external_supersession_fingerprint(),
        "lane_fingerprint must be identical to external_supersession_fingerprint — \
         the coalescing-lane oracle and the promotion oracle are the SAME oracle"
    );
    assert_ne!(
        view.compat_token().validity_fingerprint,
        0,
        "a real base view must carry a non-zero external-supersession fingerprint"
    );
}

// ── A positive import-route cache write advances the token ──

#[test]
fn token_advances_on_positive_import_route_cache_write() {
    // SOUNDNESS: `cache_positive_import_route_result`
    // mutates `DerivedRawState.import_routes`, which the base store view
    // snapshots BY VALUE (the `ImportRoute` derived-hash domain, via
    // `generation_current_import_route_hash`'s `DerivedRawState`
    // fallback). If this write moved NO token dimension a
    // `StoreViewManager`-cached base view built before the write would be
    // served unchanged and warm validation would compare against the
    // STALE `ImportRoute` hash forever. The write advances the
    // dedicated `load_generation` dimension, so the full reuse token
    // changes (rebuild on the next request) WITHOUT counting as an
    // external supersession (a cold compute's own route resolution must
    // not self-fence its own result promotion).
    let (host, canonical) = host_with_one_file();
    let before = host.current_validation_token();
    let before_load = before.load_generation;
    let before_epoch = before.store_view_epoch;

    // Drive the exact positive-route point producer (the single writer of
    // `DerivedRawState.import_routes` outside the snapshot writer + the two
    // lifecycle resets).
    host.cache_positive_import_route_result_for_tests(&canonical, "./dep", "/proj/dep.ts");

    let after = host.current_validation_token();
    assert_ne!(
        before_load, after.load_generation,
        "a positive import-route cache write MUST advance the load_generation \
         token dimension (it mutates DerivedRawState.import_routes, snapshotted \
         by value as the ImportRoute derived-hash domain)"
    );
    assert_ne!(
        before, after,
        "the full reuse token MUST change after a positive import-route cache write \
         (so a manager-cached base view is invalidated and never serves a stale \
         ImportRoute hash)"
    );
    // It is additive own-work, not an external mutation: the publish fence
    // must NOT self-fence on it.
    assert!(
        !before.externally_superseded_by(&after),
        "a positive import-route cache write (own-work) MUST NOT count as an \
         EXTERNAL supersession — else a cold compute that resolves its own import \
         would self-fence its result promotion"
    );
    assert_eq!(
        before_epoch, after.store_view_epoch,
        "a positive import-route cache write MUST NOT bump store_view_epoch — the \
         dedicated load_generation dimension covers it"
    );
}

#[test]
fn idempotent_positive_route_readmit_does_not_advance_token_or_rebuild_snapshot() {
    // OVER-BUMP GUARD: a positive route re-admission that resolves
    // the SAME `(owner, specifier)` to the SAME canonical (repeated
    // blocker hydration / concurrent resolution) is a SEMANTIC NO-OP. It
    // must advance NO token dimension — over-bumping needlessly
    // invalidates the `StoreViewManager` base snapshot (forcing a rebuild
    // sweep) and forks singleflight lanes.
    //
    // DISCRIMINATION: an unconditional `bump_load_generation()` in
    // `cache_positive_import_route_result` would make the SECOND
    // (no-op) admission advance `load_generation` → the full token
    // changes → the manager-cached base snapshot is INVALIDATED and the
    // next `resolver_store_view()` rebuilds a fresh Arc. With the no-op
    // admission bumping nothing → token unchanged → the manager-cached
    // token is preserved and the next view is the SAME warm Arc (no
    // rebuild, no lane fork). Asserted per-host (manager `cached_token` +
    // snapshot Arc identity) so the test is deterministic under parallel
    // execution.
    let (host, canonical) = host_with_one_file();

    // FIRST admission is a genuine transition (establishes the route +
    // dependency edge) — it legitimately advances the token. Warm the
    // manager cache against the post-first-admission token and pin the
    // served Arc's identity.
    host.cache_positive_import_route_result_for_tests(&canonical, "./dep", "/proj/dep.ts");
    let warm = host.resolver_store_view_read().into_owned_view();
    let warm_ptr = warm.snapshot_ptr_for_tests();
    let cached_before = host
        .store_view_manager()
        .cached_token()
        .expect("manager must have a warm base view after the first request");
    let before = host.current_validation_token();

    // SECOND admission: byte-identical `(owner, specifier, resolved)` — a
    // pure re-admit of the already-stored route + already-present
    // dependency edge. This is the no-op the fix must not bump on.
    host.cache_positive_import_route_result_for_tests(&canonical, "./dep", "/proj/dep.ts");

    let after = host.current_validation_token();
    assert_eq!(
        before, after,
        "an idempotent positive-route re-admission (same owner/specifier/resolved, \
         dependency edge already present) MUST NOT advance ANY token dimension — \
         over-bumping invalidates the manager snapshot and forks singleflight lanes"
    );
    assert_eq!(
        before.load_generation, after.load_generation,
        "the no-op re-admit specifically must NOT bump load_generation"
    );

    // The manager-cached token is unchanged (the no-op did not invalidate
    // the base snapshot) and the next base-view request is the SAME Arc.
    assert_eq!(
        Some(cached_before),
        host.store_view_manager().cached_token(),
        "a no-op positive-route re-admit MUST NOT invalidate the manager-cached base \
         view token"
    );
    let after_view = host.resolver_store_view_read().into_owned_view();
    assert!(
        std::ptr::eq(warm_ptr, after_view.snapshot_ptr_for_tests()),
        "a no-op positive-route re-admit MUST NOT force a base-snapshot rebuild — the \
         manager hands back the SAME Arc because the token did not move"
    );
}

#[test]
fn distinct_positive_route_readmit_does_advance_token() {
    // Positive counterpart to the no-op guard: re-admitting the SAME
    // specifier resolved to a DIFFERENT canonical IS a genuine transition
    // (the snapshotted `ImportRoute` derived hash changes), so it MUST
    // advance the token. Proves the compare-before-insert gates on an
    // actual value change, not blanket suppression.
    let (host, canonical) = host_with_one_file();
    host.cache_positive_import_route_result_for_tests(&canonical, "./dep", "/proj/dep.ts");
    let _warm = host.resolver_store_view_read().into_owned_view();

    let before = host.current_validation_token();
    // Same specifier, DIFFERENT resolved canonical → route map value
    // changes → genuine transition.
    host.cache_positive_import_route_result_for_tests(&canonical, "./dep", "/proj/other.ts");
    let after = host.current_validation_token();

    assert_ne!(
        before.load_generation, after.load_generation,
        "re-resolving the same specifier to a DIFFERENT canonical IS a genuine \
         route transition and MUST advance load_generation"
    );
    // And it is own-work (additive), not an external supersession.
    assert!(
        !before.externally_superseded_by(&after),
        "a route value change is the compute's own additive work — it must NOT \
         count as an external supersession"
    );
}

#[test]
fn token_does_not_advance_on_unrelated_import_route_read() {
    // Negative counterpart to the positive-route write: reading the
    // import-route surface (the `ImportRoute` derived-hash domain via the
    // base-view snapshot) MUST NOT advance the token. This proves the
    // load_generation bump is gated on an actual WRITE, not emitted on the
    // read path that snapshots the routes.
    let (host, canonical) = host_with_one_file();
    // Seed a positive route once (the write that legitimately advances the
    // token), then capture `before` AFTER it so the read is isolated.
    host.cache_positive_import_route_result_for_tests(&canonical, "./dep", "/proj/dep.ts");

    let before = host.current_validation_token();
    // Pure reads of the import-route surface: snapshot the base view
    // (captures the ImportRoute derived hash) and re-read the token. No
    // route map is mutated.
    let _view = host.resolver_store_view_read().into_owned_view();
    let _hash = host.generation_current_import_route_hash(&canonical);
    let _view2 = host.resolver_store_view_read().into_owned_view();
    let after = host.current_validation_token();
    assert_eq!(
        before, after,
        "reading the import-route surface (snapshot / generation_current_import_route_hash) \
         MUST NOT advance the token — only a positive-route WRITE does"
    );
}

// ── A waiter re-captures the live token after waking ──

#[test]
fn woken_waiter_recaptures_live_token_and_warm_hits_instead_of_resweeping() {
    // SOUNDNESS: a waiter parks in `base_view` with a
    // token `current` captured BEFORE it slept. If a host mutation advances
    // the live token while it sleeps, on wake it must NOT re-probe the cache
    // against the now-stale `current`. A waiter that looped the INNER loop
    // and re-probed against its stale `current` would either return a
    // winner keyed on the OLD token (a superseded view validated against
    // already-invalidated state) or false-miss a winner keyed on the NEW
    // token and redundantly RE-SWEEP (reintroducing O(N) sweeps). The
    // waiter restarts the OUTER loop after `built.wait` to RE-CAPTURE the
    // live token.
    //
    // Deterministic interleave via the process-global build gate:
    //   1. Warm the manager, then bump the token → next request is a cold
    //      miss at token T0.
    //   2. Arm the build gate and spawn ONE builder. It claims the build,
    //      captures `pre` = T0, then PARKS at the gate holding the claim.
    //   3. Spawn N waiter threads. Each captures `current` = T0, sees
    //      `building == true`, and parks on `built.wait`. Poll
    //      `parked_waiters()` until all N have parked (deterministic).
    //   4. Advance the live token to T1 (a content upsert) WHILE the
    //      builder is gated and the waiters are parked.
    //   5. Reset the sweep counter, then release the builder. Its T0 attempt
    //      is now superseded (live == T1) → it retries, captures T1, builds
    //      a coherent T1 view, publishes it, and wakes the waiters.
    //   6. Each woken waiter restarts the outer loop and re-reads the live
    //      token inside the lock, observing T1, so it warm-hits the published
    //      T1 view (NO sweep). A waiter that re-probed a token captured before
    //      it slept (T0) against the cached T1 would false-miss and re-sweep
    //      → ~N extra sweeps; the in-lock re-read is what prevents that.
    //
    // Discriminators: (a) the post-release sweep count stays far below N
    // (the woken waiters warm-hit, they do not re-sweep); (b) every returned
    // view validates against the FINAL live token T1 (no superseded view
    // escapes); (c) all waiters share ONE snapshot pointer (singleflight
    // preserved across the re-capture). A watchdog bounds every blocking
    // wait so a regression that hangs FAILS rather than stalling CI.
    //
    // The WAITERS' sweep count is measured PER-THREAD (each waiter reports
    // its own `COHERENT_BUILD_SWEEPS_THIS_THREAD`) so the assertion is
    // robust against concurrent build activity from unrelated parallel
    // tests (which never touches THESE threads' thread-locals).
    use crate::resolver_store::{COHERENT_BUILD_SWEEPS_THIS_THREAD, TEST_BUILD_GATE};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    const WAITERS: usize = 8;

    let (host, canonical) = host_with_one_file();
    // Warm the manager, then advance the token so the next request is a cold
    // miss at T0.
    let _ = host.resolver_store_view_read().into_owned_view();
    host.bump_store_view_epoch();

    // Arm the gate and spawn the builder. It enrolls itself so ONLY this
    // thread parks at the gate (unrelated parallel tests are never
    // captured), then claims + captures T0 + parks.
    TEST_BUILD_GATE.arm();
    let builder = {
        let host = Arc::clone(&host);
        std::thread::spawn(move || {
            TEST_BUILD_GATE.enroll_current_thread();
            let view = host.resolver_store_view_read().into_owned_view();
            (
                view.validation_token_for_tests(),
                view.snapshot_ptr_for_tests() as usize,
            )
        })
    };
    // Wait (bounded) for the builder to reach + park at the gate.
    assert!(
        TEST_BUILD_GATE.wait_for_builder_parked(Duration::from_secs(20)),
        "the gated builder must reach the build gate within the watchdog window"
    );

    // Spawn the waiters. Each captures T0, sees `building`, parks on
    // `built.wait`. Each reports its OWN per-thread sweep count so the
    // re-sweep discrimination is contamination-free.
    let mut waiters = Vec::with_capacity(WAITERS);
    for _ in 0..WAITERS {
        let host = Arc::clone(&host);
        waiters.push(std::thread::spawn(move || {
            COHERENT_BUILD_SWEEPS_THIS_THREAD.with(|c| c.set(0));
            let view = host.resolver_store_view_read().into_owned_view();
            let sweeps = COHERENT_BUILD_SWEEPS_THIS_THREAD.with(std::cell::Cell::get);
            (
                view.validation_token_for_tests(),
                view.snapshot_ptr_for_tests() as usize,
                sweeps,
            )
        }));
    }
    // Poll (bounded) until all N waiters have parked on `built.wait`.
    let deadline = Instant::now() + Duration::from_secs(20);
    while host.store_view_manager().parked_waiters() < WAITERS {
        assert!(
            Instant::now() < deadline,
            "all {WAITERS} waiters must park on built.wait within the watchdog window \
             (observed {} parked)",
            host.store_view_manager().parked_waiters()
        );
        std::thread::yield_now();
    }

    // Advance the live token to T1 WHILE the builder is gated and the
    // waiters are parked.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.clone(),
            source: Arc::from("export interface A { x: number; t1: boolean }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("mid-flight content upsert must succeed");
    let final_token = host.current_validation_token();

    // Release the gated builder.
    TEST_BUILD_GATE.release();

    // Collect all results on a watchdog thread so a regression that hangs a
    // woken waiter FAILS via timeout rather than stalling CI.
    let (tx, rx) = std::sync::mpsc::channel();
    let collector = std::thread::spawn(move || {
        let builder_result = builder.join().unwrap();
        let waiter_results: Vec<(crate::resolver_store::StoreViewValidationToken, usize, u64)> =
            waiters.into_iter().map(|h| h.join().unwrap()).collect();
        let _ = tx.send((builder_result, waiter_results));
    });
    let (builder_result, waiter_results) = match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(r) => {
            collector.join().unwrap();
            r
        }
        Err(_) => panic!(
            "REGRESSION: a woken waiter blocked / lost a wakeup — base_view did not \
             re-capture the live token and make progress after built.wait"
        ),
    };

    // (a) WAITER re-sweep count: the woken waiters warm-hit the published T1
    // and do NOT re-sweep (per-thread sweeps ≈ 0). A woken waiter that
    // re-probed its STALE `current` (T0) against the cached T1 would
    // false-miss and re-sweep → ~N waiter sweeps. The separation (≈0 vs ≈N) makes
    // WAITERS/2 an unambiguous gate. Measured per-thread, so unrelated
    // parallel tests cannot perturb it.
    let waiter_sweeps: u64 = waiter_results.iter().map(|(_t, _p, s)| *s).sum();
    assert!(
        waiter_sweeps < (WAITERS as u64) / 2,
        "woken waiters MUST re-capture the live token and WARM-HIT the published \
         view, not re-sweep: expected far fewer than {WAITERS} waiter sweeps, \
         observed {waiter_sweeps}"
    );

    // (b) No superseded view escaped: every returned view validates against
    // the FINAL live token T1.
    assert_eq!(
        builder_result.0, final_token,
        "the builder's retried view must be coherent against the final token T1"
    );
    for (token, _ptr, _sweeps) in &waiter_results {
        assert_eq!(
            *token, final_token,
            "a woken waiter returned a SUPERSEDED view (token {token:?} != final live \
             token {final_token:?}) — base_view must re-capture the live token after \
             built.wait and never hand back a view keyed on a stale token"
        );
    }

    // (c) All waiters + the builder share ONE snapshot pointer (singleflight
    // preserved across the re-capture).
    let builder_ptr = builder_result.1;
    for (_token, ptr, _sweeps) in &waiter_results {
        assert_eq!(
            *ptr, builder_ptr,
            "all woken waiters MUST share the builder's published Arc<StoreViewSnapshot> \
             (one coherent T1 view), not independent re-builds"
        );
    }
}

// ── A mid-build non-epoch dimension change forces a retry, not a torn view ──

#[test]
fn mid_build_env_change_without_epoch_forces_retry_not_torn_view() {
    // SOUNDNESS: `build` reads the token-relevant
    // env-hash / project-identity / project-generation dimensions. If it
    // read env LATE (after the per-canonical snapshot maps were already
    // populated), a mid-build env mutation that advances `resolve_env_hash`
    // WITHOUT bumping `store_view_epoch` would leave the view's reconstructed
    // token reflecting the NEW env while the snapshot maps were captured
    // under the OLD env, and the post-build coherence check (comparing a
    // token reconstructed from the SAME late reads against the live token)
    // would accept the TORN view as coherent. `build` stamps every token
    // dimension from a single PRE-build capture, so the view's token reflects
    // the OLD env (coherent with its OLD-env snapshot maps) and DIFFERS from
    // the live (NEW-env) token → `build_coherent` rejects the attempt.
    //
    // Discrimination part A — single attempt: drive ONE `build` with a
    // mid-build env bump injected after the snapshot maps are populated. The
    // produced view's own token MUST equal the PRE-build token (OLD env) and
    // MUST differ from the post-build live token (NEW env). A late-env-read
    // build would equate the view's token with the live token (both NEW env)
    // — the torn view would look coherent.
    let (host, _canonical) = host_with_one_file();

    let (view, pre_token, live_token) =
        crate::resolver_store::HostStoreView::build_one_attempt_with_mid_build_env_bump_for_tests(
            &host,
        );
    assert_ne!(
        pre_token.env_hash_fold, live_token.env_hash_fold,
        "the injected mid-build env bump MUST advance the env_hash_fold (precondition \
         for the discrimination)"
    );
    assert_eq!(
        pre_token.store_view_epoch, live_token.store_view_epoch,
        "the mid-build env bump MUST NOT move store_view_epoch — that is what makes \
         this a non-epoch dimension change the coarse epoch cannot catch"
    );
    assert_eq!(
        view.validation_token_for_tests(),
        pre_token,
        "the built view's token MUST equal the PRE-build capture (OLD env), \
         coherent with the snapshot maps that were captured under the OLD env — NOT \
         the live NEW-env token (which would mean a torn view stamped with late env)"
    );
    assert_ne!(
        view.validation_token_for_tests(),
        live_token,
        "the built view's token MUST DIFFER from the live (post-mutation) token, so \
         build_coherent's post-build comparison rejects this attempt and retries \
         rather than publishing a torn view"
    );

    // Discrimination part B — the full no-torn-return path: a quiescent
    // rebuild (the env mutation has settled) yields a fully coherent view
    // whose token matches the live token. Proves the retry path converges on
    // a coherent view, not a permanent failure.
    let coherent = host.resolver_store_view_read().into_owned_view();
    assert_eq!(
        coherent.validation_token_for_tests(),
        host.current_validation_token(),
        "after the mid-build env change settles, the manager MUST yield a fully \
         coherent view (token == live token), proving Superseded is transient"
    );
}

// ── Bounded liveness + typed currentness under sustained token churn ──

#[test]
fn base_view_under_sustained_token_churn_returns_return_only_never_current() {
    // LIVENESS + SOUNDNESS: `StoreViewManager::base_view` runs a cooperative
    // outer loop (warm-hit / join-the-flight / claim-and-build). On a
    // `Superseded` build it re-claims a fresh build. Under sustained
    // validation-token churn — a host whose token advances on EVERY snapshot
    // attempt — every claimed build is superseded forever, so an UNBOUNDED
    // outer loop would re-claim a never-coherent build indefinitely (a
    // hang). The validation-snapshot path routes `from_host` /
    // `resolver_store_view_read` through `base_view`, so without the bound a
    // caller would block FOREVER instead of returning in bounded time.
    //
    // SOUNDNESS discrimination: the read must come back as a typed
    // `StoreViewRead::ReturnOnly` — NOT a validation-capable
    // `StoreViewRead::Current`. The freshest built view under churn is
    // KNOWN-STALE (its token is the stale pre-build capture); a manager that
    // handed it back as `Current` would let a warm validator validate a
    // cache entry against an already-superseded snapshot. This assertion
    // FAILS against a tree that returns the freshest stale view as a plain
    // validation-capable view: `.current()` would be `Some(_)` rather than
    // `None`.
    //
    // It ALSO asserts (a) the read carries a `RetryBudgetExhausted`-class
    // reason (`Superseded`), (b) the manager did NOT cache the incoherent
    // view, and (c) a quiescent call after the churn is disarmed rebuilds a
    // COHERENT `Current` read and warms a canonical cached entry. Without
    // the bound these arms are unreachable (the call never returns).
    use std::sync::mpsc;
    use std::time::Duration;

    let (host, _canonical) = host_with_one_file();

    // Force a guaranteed cold miss so the very first request claims a build
    // (not a warm Arc-clone hit).
    let _ = host.resolver_store_view_read();
    host.bump_store_view_epoch();

    let host_for_watchdog = Arc::clone(&host);
    let (tx, rx) = mpsc::channel::<bool>();
    let watchdog = std::thread::spawn(move || {
        // Arm the churn ON THE BUILDER THREAD: `build_coherent` runs on this
        // same thread, so the thread-local knob bumps the epoch mid-build on
        // every attempt.
        crate::resolver_store::HostStoreView::arm_supersede_always_for_tests();
        // Without the bound this call would NEVER return (infinite re-claim
        // loop) and the watchdog would time out. With the bounded loop it
        // terminates and returns the freshest built view as `ReturnOnly`.
        let read = host_for_watchdog.resolver_store_view_read();
        // The defining soundness assertion: under sustained churn the read
        // is NEVER `Current`. A validation-capable stale view here is the
        // false-positive class this fix closes.
        let is_return_only = !read.is_current_for_tests();
        let returned_view_token = read.view_for_tests().validation_token_for_tests();
        // Disarm the churn so the manager's own internal probe is quiescent.
        crate::resolver_store::HostStoreView::disarm_supersede_always_for_tests();
        // The manager must NOT hold the incoherent view as a warm-hit
        // candidate matching the live token: a fresh quiescent call rebuilds
        // a coherent `Current` view rather than warm-hitting the
        // return-only one.
        let stable_read = host_for_watchdog.resolver_store_view_read();
        let stable_is_current = stable_read.is_current_for_tests();
        let cached_canonical = host_for_watchdog
            .store_view_manager()
            .cached_token()
            .expect("a quiescent stable call must populate the manager")
            == host_for_watchdog.current_validation_token();
        // The return-only view's token must NOT be the canonical cached
        // token — it was never promoted — and the quiescent rebuild's view
        // IS the live canonical token.
        let return_only_not_canonical = returned_view_token
            != host_for_watchdog.current_validation_token()
            && stable_read.view_for_tests().validation_token_for_tests()
                == host_for_watchdog.current_validation_token();
        let _ = tx.send(
            is_return_only && stable_is_current && cached_canonical && return_only_not_canonical,
        );
    });

    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(invariants_hold) => {
            watchdog
                .join()
                .expect("watchdog thread must not itself panic");
            assert!(
                invariants_hold,
                "under sustained validation-token churn the manager must (a) hand back \
                 a typed ReturnOnly read (NEVER a validation-capable Current view), \
                 (b) NOT cache the incoherent view, and (c) rebuild a coherent Current \
                 read warming a canonical cached entry on the next quiescent call"
            );
        }
        Err(_) => panic!(
            "REGRESSION: base_view spun forever under sustained validation-token \
             churn — resolver_store_view_read() never returned within the watchdog \
             window (the bounded-retry cap is missing / the outer loop re-claims a \
             never-coherent build indefinitely)"
        ),
    }
}

// ── Manager contract: retry-exhausted reads are typed ReturnOnly ──────

#[test]
fn retry_exhausted_read_is_return_only_and_recovers_on_quiescence() {
    // MANAGER CONTRACT: the current-view accessor
    // (`resolver_store_view_read`) must NEVER advertise a known-stale view
    // as current. When the bounded cooperative retry budget is exhausted
    // under sustained churn, the read is a typed `StoreViewRead::ReturnOnly`
    // carrying a `Superseded` reason — NOT a validation-capable
    // `StoreViewRead::Current`. A `Current` here would let a warm validator
    // false-positive a stale cache entry against an already-superseded
    // snapshot (the soundness hole this fix closes).
    //
    // This is the focused accessor-contract counterpart to the watchdog
    // liveness test above: it asserts (a) the typed arm + classified
    // reason, (b) the stale view is not cached, and (c) a quiescent call
    // after the churn is disarmed rebuilds a coherent `Current` view and
    // warms a canonical cached entry.
    //
    // Discrimination: against a tree whose accessor returns the freshest
    // stale view as a plain validation-capable view, `.is_current_for_tests()`
    // would be `true` and `.return_only_reason_for_tests()` would be `None`
    // — both assertions FAIL.
    use crate::resolver_store::StoreViewReturnOnlyReason;
    use std::sync::mpsc;
    use std::time::Duration;

    let (host, _canonical) = host_with_one_file();

    // Force a guaranteed cold miss so the request claims a build.
    let _ = host.resolver_store_view_read();
    host.bump_store_view_epoch();

    let host_for_watchdog = Arc::clone(&host);
    let (tx, rx) = mpsc::channel::<(bool, bool, bool, bool)>();
    let watchdog = std::thread::spawn(move || {
        crate::resolver_store::HostStoreView::arm_supersede_always_for_tests();
        let read = host_for_watchdog.resolver_store_view_read();
        let is_return_only = !read.is_current_for_tests();
        let reason_is_superseded =
            read.return_only_reason_for_tests() == Some(StoreViewReturnOnlyReason::Superseded);
        crate::resolver_store::HostStoreView::disarm_supersede_always_for_tests();
        // The stale view was never promoted into the manager cache.
        let not_cached_stale = read.view_for_tests().validation_token_for_tests()
            != host_for_watchdog.current_validation_token();
        // A quiescent call now rebuilds a coherent Current read.
        let quiescent = host_for_watchdog.resolver_store_view_read();
        let recovered_current = quiescent.is_current_for_tests()
            && host_for_watchdog
                .store_view_manager()
                .cached_token()
                .expect("a quiescent call must populate the manager")
                == host_for_watchdog.current_validation_token();
        let _ = tx.send((
            is_return_only,
            reason_is_superseded,
            not_cached_stale,
            recovered_current,
        ));
    });

    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok((is_return_only, reason_is_superseded, not_cached_stale, recovered_current)) => {
            watchdog.join().expect("watchdog thread must not panic");
            assert!(
                is_return_only,
                "REGRESSION: the retry-exhausted read was Current — a known-stale view \
                 was advertised as validation-capable"
            );
            assert!(
                reason_is_superseded,
                "the ReturnOnly read must carry the Superseded reason under sustained \
                 mid-build churn"
            );
            assert!(
                not_cached_stale,
                "the stale view must not be promoted as the canonical cached entry"
            );
            assert!(
                recovered_current,
                "a quiescent call after disarming the churn must rebuild a coherent \
                 Current view and warm a canonical cached entry"
            );
        }
        Err(_) => panic!(
            "REGRESSION: resolver_store_view_read() never returned under sustained \
             token churn (bounded-retry cap missing)"
        ),
    }
}

// ── close() must DROP the StoreViewManager's cached snapshot Arc ──────

#[test]
fn close_drops_store_view_manager_cached_snapshot_arc() {
    // MEMORY: `close()` bumps the validation token, which invalidates the
    // cached base view as a warm-hit candidate — but a token bump ALONE
    // keeps the cached `Arc<StoreViewSnapshot>` (and its per-file maps /
    // fact `Arc`s) strongly held until a LATER store-view request rebuilds
    // and replaces the entry. A closed-not-reused host (the NAPI
    // finalisation case) never issues that next request, so without an
    // explicit clear the snapshot stays resident — regressing close()'s
    // memory-release contract.
    //
    // Discrimination: populate the manager via `resolver_store_view()`, hold
    // a `Weak` to the cached view's inner snapshot `Arc`, DROP the test's own
    // strong refs, then `close()`. `close()` drops the manager's cached
    // `Arc` (the only remaining strong ref), so the `Weak` fails to upgrade
    // AND the manager cache is empty. A token-bump-only close would leave
    // the manager still holding the `Arc`, so the `Weak` would upgrade and
    // the cache would stay populated → FAIL.
    let (host, _canonical) = host_with_one_file();

    // Populate the manager and capture a Weak to the cached snapshot.
    let view = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.store_view_manager().is_populated(),
        "resolver_store_view must populate the manager cache (precondition)"
    );
    let weak = view.snapshot_weak_for_tests();
    // Drop the test's own strong ref so the ONLY remaining strong holder is
    // the manager's cached entry. The Weak must still upgrade now (the
    // manager holds the Arc).
    drop(view);
    assert!(
        weak.upgrade().is_some(),
        "before close() the manager still holds the cached snapshot Arc (precondition)"
    );

    host.close();

    assert!(
        weak.upgrade().is_none(),
        "REGRESSION: close() left the StoreViewManager's cached snapshot Arc alive — a \
         token bump alone keeps it strongly held until the next store-view request, so \
         a closed-not-reused host never releases the snapshot memory"
    );
    assert!(
        !host.store_view_manager().is_populated(),
        "close() must clear the StoreViewManager cache (cached entry dropped)"
    );
}

// ── Snapshot publish/return invariant: token re-validated after every lock
//    transition; a clear/reset blocks any in-flight builder from publishing
//    a pre-reset snapshot (the in-lock re-read and the reset fence are two
//    symptoms of ONE invariant). ──

#[test]
fn warm_hit_revalidates_live_token_after_acquiring_lock() {
    // IN-LOCK RE-READ SOUNDNESS: the warm probe must compare the cached
    // entry against a token re-read AFTER acquiring the manager lock, not
    // one captured before it. A host mutation that lands between a pre-lock
    // capture and the comparison (e.g. while this thread waited for `state`)
    // bumps the live token; the cached entry would still match the STALE
    // captured token and the manager would return a view the host has
    // already superseded. Direct callers (`try_component_meta_cache_hit`)
    // use this view for immediate fact validation, so a stale warm hit can
    // validate an old cache entry against already-invalidated state.
    //
    // Discrimination: warm the manager at T0, then arm the one-shot knob
    // that advances `store_view_epoch` INSIDE the lock immediately before
    // the warm-probe token re-read. With the in-lock re-read, it observes
    // T1, the cached T0 entry false-misses, and a rebuild yields a view at
    // the LIVE token. A manager that compared the pre-lock-captured T0
    // against the cached T0 would match and return the STALE view (token ==
    // T0 != live) — the assertion that the returned view's token equals the
    // live token discriminates against that flaw.
    let (host, _canonical) = host_with_one_file();

    // Warm the manager so a cached entry exists at the current token.
    let _ = host.resolver_store_view_read().into_owned_view();
    let cached_before = host
        .store_view_manager()
        .cached_token()
        .expect("manager must be warm (precondition)");

    // Arm the one-shot in-lock token bump and issue a request.
    crate::resolver_store::HostStoreView::arm_warm_probe_token_bump_for_tests();
    let view = host.resolver_store_view_read().into_owned_view();

    let live = host.current_validation_token();
    assert_ne!(
        cached_before, live,
        "the in-lock token bump must have advanced the live token past the warm entry \
         (otherwise the in-lock re-read window is not being exercised)"
    );
    assert_eq!(
        view.validation_token_for_tests(),
        live,
        "REGRESSION (in-lock re-read): the warm probe returned a view whose token does \
         not match the live host token — it warm-hit a STALE cached entry instead of \
         re-reading the live token after acquiring the manager lock"
    );
}

#[test]
fn clear_blocks_in_flight_builder_from_republishing_pre_reset_snapshot() {
    // RESET-FENCE SOUNDNESS: a host-lifecycle reset (`close` /
    // `set_workspace` /
    // `configure_projects`) calls `StoreViewManager::clear()` to RELEASE the
    // cached snapshot `Arc`. If an in-flight builder that claimed its build
    // BEFORE the reset is allowed to write its (now pre-reset) snapshot back
    // into `cached` AFTER `clear()` ran, the reset's `Arc`-release intent is
    // defeated and the cache is re-warmed with a stale base view.
    //
    // `clear()` advances the manager's reset generation; the builder records
    // the reset generation it observed at claim time and refuses to publish
    // when it advanced mid-build.
    //
    // Discrimination: gate a builder inside `build_coherent` (holding the
    // singleflight claim, having captured `pre`), then `clear()` on the main
    // thread WHILE the builder is parked, then release. The builder detects
    // the reset (its claim-time reset generation advanced) and `publish_coherent`
    // declines its PRE-RESET snapshot. The declined view drives a bounded
    // re-loop (the unified publish/return contract: a declined view is never
    // returned to a warm validator), so the builder rebuilds and may publish
    // a FRESH post-reset snapshot — but it NEVER republishes the specific
    // pre-reset snapshot `Arc` the reset released. A manager that republished
    // the builder's pre-reset snapshot directly into `cached` would re-warm
    // the cache with the stale base view, making the cached `Arc` BE the
    // pre-reset one — which the assertion below rejects.
    //
    // The gate is one-shot, so only the FIRST build attempt parks. The
    // post-reset re-loop rebuild proceeds without parking.
    //
    // A watchdog bounds the builder join so a regression FAILS rather than
    // hangs CI.
    use std::sync::mpsc;
    use std::time::Duration;

    use crate::resolver_store::TEST_BUILD_GATE;

    let (host, _canonical) = host_with_one_file();

    // Clear any reset-decline recording leaked from a prior test.
    crate::resolver_store::clear_reset_declined_snapshot_for_tests();

    // Warm once, then advance the token so the builder's request is a
    // guaranteed cold miss (claim-then-build, not a warm Arc-clone hit).
    let _ = host.resolver_store_view_read().into_owned_view();
    host.bump_store_view_epoch();

    // Spawn the gated builder. It enrolls (so ONLY this thread parks at the
    // gate), claims the build, captures `pre`, then parks at the gate.
    TEST_BUILD_GATE.arm();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let builder = {
        let host = Arc::clone(&host);
        std::thread::spawn(move || {
            TEST_BUILD_GATE.enroll_current_thread();
            let _view = host.resolver_store_view_read().into_owned_view();
            let _ = done_tx.send(());
        })
    };

    // Wait until the builder has parked inside `build_coherent` (holding the
    // claim, before publishing).
    assert!(
        TEST_BUILD_GATE.wait_for_builder_parked(Duration::from_secs(10)),
        "the gated builder must reach the build gate within the watchdog window"
    );

    // Reset WHILE the builder is parked. This bumps the manager reset
    // generation and drops the (already token-stale) cached entry. It does
    // NOT bump the host epoch, so the builder will still produce a Coherent
    // outcome and reach the publish path — where the reset fence (Gate 1)
    // declines its PRE-RESET snapshot.
    host.store_view_manager().clear();
    assert!(
        !host.store_view_manager().is_populated(),
        "clear() must drop the cached entry (precondition)"
    );

    // Release the builder. Its FIRST (pre-reset) build reaches
    // publish_coherent; the reset fence DECLINES it (records the declined
    // snapshot), and the declined view drives a bounded re-loop that rebuilds
    // a FRESH post-reset snapshot instead of returning / republishing the
    // stale one.
    TEST_BUILD_GATE.release();

    match done_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(()) => builder.join().expect("builder thread must not panic"),
        Err(_) => panic!(
            "REGRESSION: the gated builder never completed within the watchdog window \
             (clear()/publish deadlock)"
        ),
    }

    // The reset fence MUST have fired: the builder's pre-reset build
    // straddled the reset, so `publish_coherent` declined it. A manager
    // WITHOUT the reset fence would have published the pre-reset snapshot
    // directly and never recorded a decline.
    let declined_pre_reset = crate::resolver_store::reset_declined_snapshot_for_tests();
    assert!(
        crate::resolver_store::reset_fence_fired_for_tests(),
        "REGRESSION (reset fence): the reset fence did NOT fire — an in-flight builder's \
         pre-reset snapshot was published without being declined, defeating clear()'s \
         Arc-release intent"
    );

    // The unified publish/return contract re-loops a declined view rather
    // than returning it stale, so the cache may legitimately re-warm with a
    // FRESH post-reset snapshot — but it must NEVER be the SPECIFIC
    // pre-reset snapshot the reset fence declined.
    if let (Some(declined), Some((_token, cached_snapshot))) = (
        declined_pre_reset,
        host.store_view_manager().cached_entry_for_tests(),
    ) {
        assert!(
            !std::sync::Arc::ptr_eq(&cached_snapshot, &declined),
            "REGRESSION (reset fence): the manager re-warmed `cached` with the SPECIFIC \
             pre-reset snapshot the reset fence declined — the reset's Arc-release intent \
             was defeated"
        );
    }
}

// ── Publish-decline must NOT return a known-stale view to a warm validator.
//    A declined publish (live token moved past the build's token, or a reset
//    raced the build) means the freshly-built view is stale; it must drive a
//    bounded re-loop, never be handed back. Direct warm-cache validators
//    (e.g. `try_component_meta_cache_hit`) validate a cached entry's facts
//    against the returned view with NO outer freshness fence, so a returned
//    stale view → unsound warm validation. ──

#[test]
fn publish_decline_reloops_instead_of_returning_stale_view() {
    // PUBLISH-DECLINE SOUNDNESS: when `publish_coherent` declines a
    // freshly-built view because the live token moved past the build's
    // token between build completion and publish, `base_view` must REBUILD
    // against the now-current token and return a LIVE view — never hand back
    // the declined (stale) view.
    //
    // Discrimination: warm the manager, advance the epoch so the next
    // request is a guaranteed cold build (claim → build → publish), and arm
    // the one-shot publish-decline knob. Inside `publish_coherent`, just
    // before the live-token fence, the knob advances `store_view_epoch`, so
    // the build's token no longer matches the live token → Gate 2 declines.
    //
    // The declined view becomes the return-only fallback and `base_view`
    // re-loops within the bound; the next iteration claims a fresh build
    // against the now-current token, publishes it, and returns it. The
    // returned view's token EQUALS the live host token. A manager that
    // returned the declined view directly would yield a result whose token
    // is the pre-decline (stale) token, DIFFERING from the live token — the
    // assertion that the returned view's token equals the live token
    // discriminates against that flaw. A warm-cache validator handed such a
    // stale view would validate a cached entry against already-superseded
    // host state.
    let (host, _canonical) = host_with_one_file();

    // Warm once so the manager has a cached entry, then advance the epoch so
    // the next request misses the warm entry and takes the cold build path
    // (the path that reaches `publish_coherent`).
    let _ = host.resolver_store_view_read().into_owned_view();
    host.bump_store_view_epoch();

    // Arm the one-shot publish-decline knob. The next `publish_coherent` on
    // this thread bumps the epoch immediately before its live-token fence,
    // declining the just-built view.
    crate::resolver_store::HostStoreView::arm_publish_decline_once_for_tests();

    let view = host.resolver_store_view_read().into_owned_view();
    let live = host.current_validation_token();

    assert_eq!(
        view.validation_token_for_tests(),
        live,
        "REGRESSION (publish-decline): the manager returned a view whose token does not \
         match the live host token — `publish_coherent` declined the build as stale and \
         `base_view` handed the KNOWN-STALE view to the caller instead of re-looping to \
         rebuild against the now-current token. A warm-cache validator would validate a \
         cached entry against already-superseded state."
    );

    // The published cache entry (if any) must also be at the live token —
    // the manager must never warm the cache with the stale declined view.
    if let Some(cached) = host.store_view_manager().cached_token() {
        assert_eq!(
            cached, live,
            "REGRESSION (publish-decline): the manager cached a view at a stale \
             (declined) token"
        );
    }
}

// ── Final fallback stays singleflighted: exhausted no-claim waiters must NOT
//    each launch an unclaimed parallel sweep under sustained churn. ──

#[test]
fn final_fallback_under_churn_does_not_run_parallel_unclaimed_sweeps() {
    // PERF SOUNDNESS: `base_view`'s bounded retry loop
    // can leave a waiter parked behind successively-superseded builders for
    // EVERY round, so it exhausts its budget WITHOUT ever claiming a build
    // (`fallback == None`). The retired tail then ran the final
    // `build_coherent` UNCLAIMED — so under churn N such waiters each swept
    // the full workspace in parallel, defeating the StoreViewManager
    // singleflight guarantee. The fix routes the final build through a
    // claim-or-rejoin lane so at most ONE sweep runs at a time.
    //
    // Discrimination: N threads, EACH with the persistent supersede knob
    // armed (so no build ever publishes and the cache stays cold), all hammer
    // `resolver_store_view_read` after a forced cold miss. Each thread ENROLLS
    // in the concurrent-sweep gauge so the global peak counts ONLY these
    // threads' concurrency (an unrelated parallel store-view test's sweeper
    // threads never enroll, so they cannot perturb it). With per-sweep overlap
    // held open, the PEAK number of concurrent full-workspace sweeps is
    // measured. Post-fix every build runs under the singleflight claim
    // (`building`), which admits exactly ONE sweeper at a time → peak == 1.
    // Pre-fix the exhausted no-claim waiters each run an UNCLAIMED
    // `build_coherent` that overlaps the others → peak rises to >= 2 (up to
    // N). The watchdog bounds the whole run so a regression that hangs FAILS
    // rather than stalling CI.
    use crate::resolver_store::{
        arm_store_view_sweep_overlap_hold, enroll_concurrent_sweep_gauge,
        reset_store_view_peak_concurrent_sweeps, store_view_peak_concurrent_sweeps,
    };
    use std::sync::mpsc;
    use std::sync::Barrier;
    use std::time::Duration;

    const THREADS: usize = 8;

    let (host, _canonical) = host_with_one_file();
    // Warm once, then bump the token so the next wave is a guaranteed cold
    // miss for every thread simultaneously.
    let _ = host.resolver_store_view_read();
    host.bump_store_view_epoch();

    // Hold each sweep open briefly so genuinely-parallel UNCLAIMED sweeps
    // reliably overlap (the gauge then records the true peak). Under the
    // singleflight claim only one sweep is ever live, so the hold merely
    // serialises — the peak stays 1.
    reset_store_view_peak_concurrent_sweeps();
    arm_store_view_sweep_overlap_hold(true);

    let barrier = Arc::new(Barrier::new(THREADS));
    let host_for_watchdog = Arc::clone(&host);
    let (tx, rx) = mpsc::channel::<u64>();
    let watchdog = std::thread::spawn(move || {
        let mut handles = Vec::with_capacity(THREADS);
        for _ in 0..THREADS {
            let host = Arc::clone(&host_for_watchdog);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                // Enroll THIS thread in the concurrent-sweep gauge so its
                // sweeps (and only sweeps from these N threads) count toward
                // the global peak.
                enroll_concurrent_sweep_gauge();
                // Arm sustained churn ON THIS THREAD so every build_coherent
                // attempt this thread runs supersedes (never publishes), and
                // — crucially for the race — a thread that parks behind other
                // perpetually-superseding builders exhausts its budget with
                // no claim of its own, reaching the final fallback.
                crate::resolver_store::HostStoreView::arm_supersede_always_for_tests();
                barrier.wait();
                // A few rounds to maximise the chance several waiters reach
                // the final fallback simultaneously under contention.
                for _ in 0..3 {
                    let _ = host.resolver_store_view_read();
                }
                crate::resolver_store::HostStoreView::disarm_supersede_always_for_tests();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let _ = tx.send(store_view_peak_concurrent_sweeps());
    });

    let peak = match rx.recv_timeout(Duration::from_secs(60)) {
        Ok(peak) => {
            watchdog.join().expect("watchdog thread must not panic");
            peak
        }
        Err(_) => {
            arm_store_view_sweep_overlap_hold(false);
            panic!(
                "REGRESSION: the store-view path hung under sustained churn + contention \
                 (final-fallback claim/rejoin must terminate within the bounded budget)"
            );
        }
    };
    // Disarm the hold so it cannot leak into later tests.
    arm_store_view_sweep_overlap_hold(false);

    assert!(
        peak <= 1,
        "REGRESSION (final-fallback unclaimed sweep): the peak number of CONCURRENT \
         full-workspace sweeps was {peak} (> 1) under sustained churn — exhausted \
         no-claim waiters each ran an UNCLAIMED build_coherent in parallel instead of \
         rejoining the singleflight lane. With the claim-or-rejoin fallback exactly \
         one sweep runs at a time (peak == 1)."
    );
}

// ── content_generation token dimension (edge-currency × snapshot cache) ──
//
// The RouteDb stale-serve fixes made store-view freshness
// edge-currency-dependent on the LIVE
// workspace `content_generation`: the three edge-currency gates
// (`route_surface_is_edge_current` in the base build, the overlay
// re-root, and the completion overlay) are evaluated DURING the snapshot
// build. The manager then caches that snapshot under a
// `StoreViewValidationToken`. Without a `content_generation` dimension in
// the token, a cached snapshot whose gates were evaluated PRE-mutation
// keeps validating warm entries after (a) a watcher recovery
// (`DirectoryTreeDirty`) and (b) an edge-staleness transition (a
// dependency appeared / retargeted while the owner's content stayed put)
// — silently reopening the watcher-recovery and edge-stale wildcard
// stale-serve holes for the snapshot's lifetime. Both tests below FAIL
// against a token without the
// `content_generation` dimension (no other token dimension moves in
// either scenario) and PASS once the token folds it.

/// DISCRIMINATING (watcher-recovery stale-serve × manager cache): an
/// OS-watcher recovery (`DirectoryTreeDirty`) advances ONLY the workspace
/// `content_generation` (the `apply_changes`/`bump_content_generation`
/// wiring — no host-side epoch
/// or artifact generation moves, because no host mutation API runs). The
/// manager-cached base view MUST miss: a fresh read returns a REBUILT
/// snapshot under a NEW token, and the old token is EXTERNALLY
/// superseded so a result computed under the stale snapshot can never be
/// promoted to the shared cache.
#[test]
fn manager_cached_view_misses_after_directory_tree_dirty_watcher_recovery() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/proj/a.ts".to_string(),
        Arc::from("export interface A { x: number }\n"),
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    // Build + cache the manager base view.
    let view1 = host.resolver_store_view_read().into_owned_view();
    let ptr1 = view1.snapshot_ptr_for_tests();
    let token1 = view1.validation_token_for_tests();

    // Watcher recovery: the OS watcher lost events under `/proj`, the
    // recovery path marks the tree dirty. Files may have appeared or
    // disappeared; every baked dependency-set-derived edge is suspect.
    let _ = ws.apply_changes(vec![
        verter_workspace::WorkspaceChange::DirectoryTreeDirty {
            prefix: "/proj".to_string(),
        },
    ]);

    let view2 = host.resolver_store_view_read().into_owned_view();
    let token2 = view2.validation_token_for_tests();
    assert_ne!(
        token1, token2,
        "a DirectoryTreeDirty watcher recovery MUST advance the \
         StoreViewValidationToken — content_generation is the ONLY dimension \
         that moves, so a token without it keeps validating the stale snapshot"
    );
    assert_ne!(
        ptr1,
        view2.snapshot_ptr_for_tests(),
        "the manager MUST rebuild the snapshot after a watcher recovery (the \
         edge-currency gates must re-evaluate against the recovered file set), \
         not hand back the pre-recovery Arc"
    );
    assert!(
        token1.externally_superseded_by(&token2),
        "a watcher recovery is an EXTERNAL mutation: a result computed under \
         the pre-recovery snapshot MUST NOT be promoted to the shared cache"
    );
}

/// DISCRIMINATING (edge-stale wildcard stale-serve × manager cache): the edge-stale
/// wildcard scenario — a dependency file APPEARS while the wildcard
/// barrel's own content stays put — replayed against a MANAGER-CACHED
/// view rather than a per-request build. The file-set change advances
/// ONLY `content_generation` (the file lands in the workspace without
/// any host upsert, exactly like an on-disk file appearing under a
/// watched root). The per-request edge-currency regression tests prove a FRESH
/// build re-gates the barrel's baked wildcard edges; this test pins that
/// the manager does not keep serving the PRE-transition snapshot whose
/// gates evaluated against the old file set.
#[test]
fn manager_cached_view_misses_after_edge_stale_wildcard_file_set_change() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    // A wildcard barrel whose `export *` edge is unresolvable at first:
    // its baked edge set is derived from the CURRENT dependency file set.
    ws.inject_file(
        "/proj/barrel.ts".to_string(),
        Arc::from("export * from \"./dep\";\n"),
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    host.ensure_indexed_ready("/proj/barrel.ts")
        .expect("barrel must index (unresolvable wildcard is a valid surface)");

    // Build + cache the manager base view: the barrel's edge-currency
    // gates evaluate against the dep-less file set.
    let view1 = host.resolver_store_view_read().into_owned_view();
    let ptr1 = view1.snapshot_ptr_for_tests();
    let token1 = view1.validation_token_for_tests();

    // The wildcard target APPEARS — a file-set mutation with NO host
    // upsert and NO content change to the barrel: only
    // `content_generation` advances, and the barrel's baked wildcard
    // edge set is now stale.
    ws.inject_file(
        "/proj/dep.ts".to_string(),
        Arc::from("export type DepThing = { id: string };\n"),
    );

    let view2 = host.resolver_store_view_read().into_owned_view();
    let token2 = view2.validation_token_for_tests();
    assert_ne!(
        token1, token2,
        "a dependency-set change that edge-stales a wildcard surface MUST \
         advance the StoreViewValidationToken — content_generation is the \
         ONLY dimension that moves, so a token without it keeps validating \
         warm entries rooted on the stale baked wildcard edge"
    );
    assert_ne!(
        ptr1,
        view2.snapshot_ptr_for_tests(),
        "the manager MUST rebuild the snapshot after the wildcard target \
         appears — the build-time edge-currency gate must re-run so the \
         barrel's stale Route/ImportRoute derived hashes are suppressed"
    );
    assert!(
        token1.externally_superseded_by(&token2),
        "an edge-staleness transition is an EXTERNAL mutation: a result \
         computed under the pre-transition snapshot MUST NOT be promoted"
    );
}

/// No-self-fencing guard for the `content_generation` dimension: a cold
/// compute's OWN reads (`ensure_indexed_ready`, store-view builds) never
/// advance `content_generation`, so adding the dimension to the token /
/// external-supersession oracle cannot make a compute fence its own
/// promotion. (Only real file-set mutations — inject / apply_changes /
/// watcher recovery — move it.)
#[test]
fn content_generation_dimension_does_not_self_fence_reads() {
    let (host, canonical) = host_with_one_file();
    let token1 = host.current_validation_token();
    // Reads that a cold compute performs as its own work:
    host.ensure_indexed_ready(&canonical)
        .expect("indexing an upserted file must succeed");
    let _view = host.resolver_store_view_read().into_owned_view();
    let token2 = host.current_validation_token();
    assert!(
        !token1.externally_superseded_by(&token2),
        "a compute's own reads must NOT externally supersede its token — \
         the content_generation dimension only moves on real file-set mutations"
    );
}
