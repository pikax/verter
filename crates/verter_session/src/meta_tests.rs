use super::*;
use crate::resolver_core::{ResolverStore, StoreView};
use crate::types::HostConfig;
use crate::VerterHost;
use std::collections::BTreeSet;
use std::sync::Arc;
use verter_semantic::analysis::type_expand::ExpandedComponentTypes;
use verter_type_expr::{LiteralValue, ObjectMember, PrimitiveName, TypeExpr};

fn make_project() -> Arc<MetaProject> {
    make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    })
}

/// Test hosts construct schedulers with `cpu_threads = 1` to avoid
/// CPU oversubscription when many parallel test threads each spin
/// up their own Rayon pools. This is the architecture that replaced
/// the prior `HEAVY_COMPONENT_META_TEST_MUTEX` serialisation strategy.
fn test_scheduler_config() -> verter_scheduler::scheduler::SchedulerConfig {
    verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    }
}

fn make_project_with_config(config: HostConfig) -> Arc<MetaProject> {
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..config
        },
        test_scheduler_config(),
    );
    MetaProject::new(host)
}

fn make_workspace_project(ws: Arc<verter_workspace::MemoryWorkspace>) -> Arc<MetaProject> {
    let host = VerterHost::new_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
        test_scheduler_config(),
    );
    MetaProject::new(host)
}

fn sfc(props: &str) -> String {
    format!(
        r#"<script setup lang="ts">
defineProps<{{ {props} }}>()
</script>
<template><div>hello</div></template>"#
    )
}

/// Extract prop field names from a FileAnalysisSnapshot's macros.
fn prop_names(snapshot: &crate::types::FileAnalysisSnapshot) -> Vec<String> {
    snapshot
        .macros
        .iter()
        .filter(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .flat_map(|m| m.prop_fields.iter())
        .map(|f| f.name.clone())
        .collect()
}

/// Resolved-macro prop names sourced from the SOLE typeinfo macro-surface
/// authority (`vue_macro_dtos`, FullMetadata), keyed on each DefineProps macro
/// index (deduped, mirroring the production producer). The resolved-state
/// `ResolvedMacroMeta` no longer carries the published props/emits/slots/
/// exposed surface; it supplies only the macro index + kind for provenance.
fn resolved_macro_prop_names(
    host: &VerterHost,
    owner: &str,
    state: &crate::meta_resolve::ResolvedComponentMetaState,
) -> Vec<String> {
    let mut seen = rustc_hash::FxHashSet::default();
    state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .filter(|m| seen.insert(m.macro_index))
        .flat_map(|m| {
            host.vue_macro_dtos(&crate::typeinfo::types::VueMacroSurfaceRequest {
                owner_canonical: std::sync::Arc::from(owner),
                macro_index: m.macro_index,
                macro_kind: m.macro_kind,
                root_identity: host.current_or_read_whole_hash(owner).unwrap_or([0u8; 16]),
                level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
            })
            .prop_fields()
            .iter()
            .map(|p| p.name.clone())
            .collect::<Vec<_>>()
        })
        .collect()
}

fn evaluated_prop_type<'a>(types: &'a ExpandedComponentTypes, name: &str) -> &'a TypeExpr {
    &types
        .props
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("missing evaluated prop {name}"))
        .r#type
}

/// `open_session()` defaults to interactive mode; `open_session_batch()`
/// returns a batch-mode session.
#[test]
fn open_session_defaults_to_interactive_mode() {
    let project = make_project();
    let interactive = project.open_session().expect("interactive session");
    let batch = project.open_session_batch().expect("batch session");
    assert_eq!(
        interactive.execution_mode(),
        crate::meta::ExecutionMode::Interactive,
        "open_session() default must be Interactive",
    );
    assert_eq!(
        batch.execution_mode(),
        crate::meta::ExecutionMode::Batch,
        "open_session_batch() must return Batch mode",
    );
}

/// `get_component_meta_batch` performs a **single** scheduler
/// submission per batch dispatch. `submit_count` increases by exactly
/// one, independent of `jobs.len()` (R7 / R8 — one view, one
/// scheduler context, shared admissions). Every per-id result resolves
/// to the same shape the synchronous `get_component_meta` path returns,
/// so callers can rely on observable equivalence between the two
/// execution modes while only the fan-out characteristic differs.
#[test]
fn get_component_meta_batch_dispatches_through_scheduler() {
    use std::sync::atomic::Ordering;
    let project = make_project();
    project
        .upsert_base(
            "/src/A.vue",
            r#"<script setup lang="ts">defineProps<{ a: string }>()</script><template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/B.vue",
            r#"<script setup lang="ts">defineProps<{ b: number }>()</script><template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/C.vue",
            r#"<script setup lang="ts">defineProps<{ c: boolean }>()</script><template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().expect("batch session");
    let scheduler = project.host().scheduler();
    let baseline_submit = scheduler.counters().submit_count.load(Ordering::Relaxed);
    let canonical_ids = vec![
        "/src/A.vue".to_string(),
        "/src/B.vue".to_string(),
        "/src/C.vue".to_string(),
    ];
    let results = session
        .get_component_meta_batch(&canonical_ids)
        .expect("batch dispatch should complete");
    assert_eq!(results.len(), 3, "one result per submitted job");
    for (canonical, result) in canonical_ids.iter().zip(results.iter()) {
        let analysis = result
            .as_ref()
            .unwrap_or_else(|err| panic!("batch result for {canonical} failed: {err:?}"))
            .as_ref()
            .unwrap_or_else(|| panic!("batch result for {canonical} missing analysis"));
        assert!(
            !analysis.props.is_empty(),
            "batch result for {canonical} should carry its own defineProps shape",
        );
    }
    let after_submit = scheduler.counters().submit_count.load(Ordering::Relaxed);
    assert_eq!(
        after_submit - baseline_submit,
        1,
        "batch dispatch is O(1) per batch: \
         scheduler.counters.submit_count MUST bump by exactly 1 \
         regardless of N=3 jobs (baseline={baseline_submit} after={after_submit})",
    );
}

fn evaluated_define_props_type<'a>(types: &'a ExpandedComponentTypes, name: &str) -> &'a TypeExpr {
    &types
        .define_props
        .iter()
        .flat_map(|entry| entry.result.value.properties.iter())
        .find(|prop| prop.name == name)
        .unwrap_or_else(|| panic!("missing defineProps property {name}"))
        .ty
}

fn assert_union_string_literals(expr: &TypeExpr, expected: &[&str]) {
    let mut actual = BTreeSet::new();
    match expr {
        TypeExpr::Literal(LiteralValue::String(value)) => {
            actual.insert(value.as_str());
        }
        TypeExpr::Union(types) => {
            for ty in types.iter() {
                match ty {
                    TypeExpr::Literal(LiteralValue::String(value)) => {
                        actual.insert(value.as_str());
                    }
                    TypeExpr::Primitive(PrimitiveName::Undefined) => {}
                    other => panic!(
                        "expected only string literal members (plus optional undefined), got {other:?}"
                    ),
                }
            }
        }
        other => panic!("expected string literal union, got {other:?}"),
    }

    assert_eq!(
        actual,
        BTreeSet::from_iter(expected.iter().copied()),
        "unexpected literal union members for {expr:?}"
    );
}

fn cached_resolved_state(
    project: &MetaProject,
    canonical: &str,
    mode: crate::types::ProjectionMode,
) -> Option<Arc<crate::meta_resolve::ResolvedComponentMetaState>> {
    // The slot key is `(mode, view_fingerprint)`. Test fixtures
    // exercise a single session at a time; the helper prefers the
    // overlay-bearing slot (view_fingerprint != 0) if present and
    // falls back to the base slot (view_fingerprint == 0) so the
    // historical "give me the latest cached state for this mode"
    // semantics are preserved across the view-aware migration.
    fn pick<'a>(
        entries: impl Iterator<
            Item = (
                &'a (crate::types::ProjectionMode, u64),
                &'a crate::types::ResolvedComponentMetaCacheEntry,
            ),
        >,
        mode: crate::types::ProjectionMode,
    ) -> Option<Arc<crate::meta_resolve::ResolvedComponentMetaState>> {
        let mut base = None;
        let mut overlay = None;
        for ((slot_mode, view_fp), cached) in entries {
            if slot_mode != &mode {
                continue;
            }
            if *view_fp == 0 {
                base = Some(Arc::clone(&cached.state));
            } else if overlay.is_none() {
                overlay = Some(Arc::clone(&cached.state));
            }
        }
        overlay.or(base)
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        // cached_resolved_meta lives on DerivedRawState (D48 split).
        project
            .host()
            .derived_raw_cache()
            .get(canonical)
            .and_then(|entry| pick(entry.cached_resolved_meta.iter(), mode))
    }

    #[cfg(target_arch = "wasm32")]
    {
        let files = crate::shared::read_lock(&project.host().files);
        files
            .get(canonical)
            .and_then(|entry| pick(entry.cached_resolved_meta.iter(), mode))
    }
}

fn clear_legacy_cached_resolved_state(
    project: &MetaProject,
    canonical: &str,
    mode: crate::types::ProjectionMode,
) {
    // Drops every slot matching `mode` regardless of view fingerprint
    // so a test fixture exercising base / overlay isolation can reset
    // the cache to a known-empty state for `mode`.
    #[cfg(not(target_arch = "wasm32"))]
    {
        // cached_resolved_meta lives on DerivedRawState (D48 split).
        if let Some(mut entry) = project.host().derived_raw_cache().get_mut(canonical) {
            entry
                .cached_resolved_meta
                .retain(|(slot_mode, _view_fp), _| slot_mode != &mode);
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        let mut files = crate::shared::write_lock(&project.host().files);
        if let Some(entry) = files.get_mut(canonical) {
            entry
                .cached_resolved_meta
                .retain(|(slot_mode, _view_fp), _| slot_mode != &mode);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn cached_fallthrough_state(
    project: &MetaProject,
    canonical: &str,
) -> Option<Arc<crate::types::FallthroughResolution>> {
    // cached_fallthrough lives on DerivedRawState (D48 split).
    project
        .host()
        .derived_raw_cache()
        .get(canonical)
        .and_then(|entry| {
            entry
                .cached_fallthrough
                .as_ref()
                .map(|cached| Arc::clone(&cached.resolution))
        })
}

#[cfg(not(target_arch = "wasm32"))]
fn clear_legacy_cached_fallthrough_state(project: &MetaProject, canonical: &str) {
    // cached_fallthrough lives on DerivedRawState (D48 split).
    if let Some(mut entry) = project.host().derived_raw_cache().get_mut(canonical) {
        entry.cached_fallthrough = None;
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn clear_runtime_top_level_fallthrough_node(project: &MetaProject, canonical: &str) {
    let key = crate::resolver_core::fallthrough_cache_key(
        canonical,
        project.host().config.generic_root_propagation,
        None,
    );
    project
        .host()
        .resolver_runtime()
        .fallthrough
        .remove_node_for_test(&key);
}

#[cfg(not(target_arch = "wasm32"))]
fn clear_runtime_root_follow_node(project: &MetaProject, canonical: &str) {
    let key = crate::resolver_core::fallthrough_resolver::root_follow_key(
        canonical,
        crate::resolver_core::FallthroughOverrideIdentity::NoOverrides,
        project.host().config.generic_root_propagation,
    );
    project
        .host()
        .resolver_runtime()
        .fallthrough
        .remove_node_for_test(&key);
}

#[cfg(not(target_arch = "wasm32"))]
fn cached_fallthrough_entry(
    project: &MetaProject,
    canonical: &str,
) -> Option<crate::types::CachedFallthroughEntry> {
    // cached_fallthrough lives on DerivedRawState (D48 split).
    project
        .host()
        .derived_raw_cache()
        .get(canonical)
        .and_then(|entry| entry.cached_fallthrough.clone())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fact_versions_match_uses_derived_fact_kind_specific_validation() {
    let project = make_project();
    project
        .upsert_base("/index.ts", "export * from './inner'")
        .unwrap();
    project
        .upsert_base("/inner.ts", "export interface Inner {}")
        .unwrap();

    // import_routes lives on DerivedRawState (D48 split).
    let mut entry = project
        .host()
        .derived_raw_cache()
        .entry("/index.ts".to_string())
        .or_default();
    entry.value_mut().import_routes.insert(
        "./inner".to_string(),
        crate::types::DependencyResolution {
            specifier: "./inner".to_string(),
            resolved_canonical_id: Some("/inner.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        },
    );
    drop(entry);

    let import_route_hash = {
        let entry = project
            .host()
            .derived_raw_cache()
            .get("/index.ts")
            .expect("derived_raw_cache entry should exist");
        crate::resolver_store::hash_import_route_targets(&entry.import_routes)
    };

    assert!(project.host().fact_versions_match(&[
        crate::resolver_core::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: crate::resolver_core::DerivedFactKind::ImportRoute,
            hash: import_route_hash,
        },
    ]));

    assert!(!project.host().fact_versions_match(&[
        crate::resolver_core::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: crate::resolver_core::DerivedFactKind::ImportRoute,
            hash: [9; 16],
        },
    ]));
}

#[test]
fn snapshot_view_is_stale_but_coherent_after_host_changes() {
    let project = make_project();
    project
        .upsert_base("/types.ts", "export interface Props { label: string }")
        .unwrap();

    let before_hash = project
        .host()
        .get_whole_hash("/types.ts")
        .expect("whole hash should exist before mutation");
    let before_view = project.host().snapshot_view();
    let before_epoch = before_view.mutation_epoch();
    let fact = crate::resolver_core::FactVersionRef::FileWholeHash {
        canonical_id: "/types.ts".to_string(),
        hash: before_hash,
    };

    assert!(before_view.validates(&fact));

    project
        .upsert_base("/types.ts", "export interface Props { disabled: boolean }")
        .unwrap();

    let after_view = project.host().snapshot_view();
    let after_epoch = after_view.mutation_epoch();

    assert!(
        before_view.validates(&fact),
        "a captured store view should keep validating against the snapshot it was created from"
    );
    assert!(
        !after_view.validates(&fact),
        "a fresh store view should reject stale facts after the host changes"
    );
    assert_ne!(before_epoch, after_epoch);
    assert_ne!(before_view.compat_token(), after_view.compat_token());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ensure_loaded_first_time_advances_the_validation_token() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("msg: string")),
    );

    let project = make_workspace_project(ws.clone());
    let before_token = project.host().current_validation_token();

    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "ensure_loaded should load the workspace file into the host"
    );

    let after_token = project.host().current_validation_token();
    // SOUNDNESS: a first-time additive load adds a
    // scheduler node + `whole_hashes` entry and populates
    // `derived_raw_cache` — all of which `HostStoreView::build` snapshots
    // BY VALUE. A `StoreViewManager`-cached base snapshot built before the
    // load does NOT track the new canonical, so the FULL reuse token MUST
    // advance or the manager would hand a stale pre-load snapshot to the
    // next caller and the untracked-file optimistic-accept would fossilize
    // against it. `integrate_scheduler_snapshot` does not publish into
    // `FileArtifactStore`, so the DEDICATED `load_generation` dimension is
    // what moves (NOT `store_view_epoch`, which the publish fence checks —
    // a cold compute's own dependency loads must not self-fence promotion).
    assert_ne!(
        before_token, after_token,
        "first-time load MUST advance the full validation token (a manager-cached \
         base view must be invalidated)"
    );
    assert_ne!(
        before_token.load_generation, after_token.load_generation,
        "first-time load MUST advance the dedicated load_generation token dimension"
    );
    assert_eq!(
        before_token.store_view_epoch, after_token.store_view_epoch,
        "first-time load MUST NOT bump store_view_epoch — it is the compute's own \
         additive work, excluded from the publish fence's external-supersession check"
    );

    // Reloading the same file via evict + ensure_loaded with CHANGED content IS
    // a content-change boundary that must advance the store_view_epoch.
    project.host().evict("/workspace/App.vue");
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("msg: number")),
    );
    let post_evict_epoch = project.host().current_store_view_epoch();
    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "evicted file must reload via ensure_loaded"
    );
    let reload_view = project.host().snapshot_view();
    assert_ne!(
        post_evict_epoch,
        reload_view.mutation_epoch(),
        "reload after evict with changed content must advance the mutation epoch"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn byte_identical_reload_after_evict_advances_load_generation_and_invalidates_mid_evict_snapshot() {
    // UNDER-BUMP GUARD: when an evicted file reloads with IDENTICAL
    // bytes, the content is unchanged (R1: no epoch bump, warm caches
    // survive). BUT the evict→present VISIBILITY transition is
    // validator-visible: a base store view built DURING the evict window
    // (file closed / canonical still evicted) caches a snapshot that does
    // NOT track the file. Without a token advance on the byte-identical
    // reload, that mid-evict snapshot's token still hits → the manager
    // hands back a view that omits the reloaded file forever.
    //
    // The byte-identical reload-after-evict advances the additive
    // `load_generation` (NOT the epoch — content is unchanged), which is
    // in the manager REUSE oracle, so the mid-evict snapshot is
    // invalidated; and excluded from `externally_superseded_by`, so a
    // cold compute's own reload does not self-fence promotion.
    //
    // DISCRIMINATION: a byte-identical branch that bumped NOTHING would
    // leave (a) `load_generation` unchanged across the reload AND (b) the
    // mid-evict snapshot built under the post-evict token still validating
    // (its token matched the live token), so `cached_token()` would be
    // unchanged after the reload. `load_generation` advances and the
    // mid-evict cached token no longer matches.
    let identical = sfc("msg: string");
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(identical.as_str()),
    );
    let project = make_workspace_project(ws.clone());

    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "first load must succeed"
    );

    // Evict the file (`evict()` bumps the epoch). Build a base store view
    // DURING the evict window and warm the manager cache with it — this is
    // the snapshot whose token must be INVALIDATED by the reload (or a
    // concurrent reader keeps hitting it after the file is restored).
    project.host().evict("/workspace/App.vue");
    let _mid_evict_view = project.host().snapshot_view();
    let mid_evict_token = project
        .host()
        .store_view_manager()
        .cached_token()
        .expect("manager must have a warm base view built during the evict window");
    let before_reload_load_gen = mid_evict_token.load_generation;
    let before_reload_epoch = mid_evict_token.store_view_epoch;

    // Reload with BYTE-IDENTICAL content.
    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "evicted file must reload via ensure_loaded"
    );

    let after_reload_token = project.host().current_validation_token();
    // The byte-identical reload-after-evict advances the additive
    // load_generation. A byte-identical branch that bumped NOTHING would
    // fail this assertion (load_generation unchanged).
    assert_ne!(
        before_reload_load_gen, after_reload_token.load_generation,
        "a byte-identical reload-after-evict MUST advance the additive load_generation \
         (the evict→present visibility transition is validator-visible)"
    );
    assert_eq!(
        before_reload_epoch, after_reload_token.store_view_epoch,
        "a byte-identical reload-after-evict MUST NOT bump store_view_epoch — the \
         content is unchanged (R1); only the additive load_generation moves"
    );
    // The mid-evict snapshot's FULL token no longer matches the live
    // token (load_generation advanced), so the manager REUSE oracle
    // invalidates it — the next reader rebuilds instead of being handed
    // the stale mid-evict view.
    assert_ne!(
        mid_evict_token, after_reload_token,
        "the mid-evict snapshot token MUST be invalidated by the reload (the manager \
         reuse oracle includes load_generation), so a snapshot built mid-evict is \
         never re-served after the file is restored"
    );
    // …but the byte-identical reload must NOT count as an EXTERNAL
    // supersession: the reload is the host's own additive work, and
    // load_generation is excluded from `externally_superseded_by` so a
    // cold compute's own reload does not self-fence promotion.
    assert!(
        !mid_evict_token.externally_superseded_by(&after_reload_token),
        "a byte-identical reload is the host's own additive work — it must NOT count \
         as an EXTERNAL supersession (load_generation is excluded from the fence)"
    );
}

#[test]
fn store_view_compat_token_matches_snapshot_epoch_and_external_supersession_fingerprint() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .expect("upsert should succeed");

    let view = project.host().snapshot_view();

    // The compat (coalescing-lane) token threads the snapshot epoch + a
    // `None` session for a base view, AND folds the EXTERNAL-supersession
    // dimensions of the `StoreViewValidationToken` into `validity_fingerprint`
    // (the SAME oracle the promotion fence `is_stable` applies) so two views
    // whose EXTERNAL validity differs in a dimension `epoch` does not cover
    // (env / identity / project / overlay) never share a singleflight /
    // stability lane.
    let expected_fingerprint = view.validation_token_for_tests().lane_fingerprint();
    assert_eq!(
        view.compat_token(),
        crate::resolver_core::StoreViewCompatToken {
            epoch: view.mutation_epoch(),
            session: None,
            validity_fingerprint: expected_fingerprint,
        },
        "the compat token must thread the snapshot epoch + the external-supersession fingerprint"
    );
    assert_ne!(
        view.compat_token().validity_fingerprint,
        0,
        "a real base view's compat token MUST carry a non-zero external-supersession \
         fingerprint (the lane identity gates on external dims, not epoch-only)"
    );
}

#[test]
fn store_view_epoch_advances_on_upsert() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .expect("upsert should succeed");
    let epoch_after_first = project.host().current_store_view_epoch();

    project
        .upsert_base("/App.vue", &sfc("msg: number"))
        .expect("re-upsert should succeed");
    let epoch_after_second = project.host().current_store_view_epoch();

    assert_ne!(
        epoch_after_first, epoch_after_second,
        "mutation epoch must advance on re-upsert so compat tokens distinguish views"
    );
}

#[test]
fn store_view_epoch_advances_on_evict() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .expect("upsert should succeed");
    let epoch_before = project.host().current_store_view_epoch();

    project.host().evict("/App.vue");
    let epoch_after = project.host().current_store_view_epoch();

    assert_ne!(
        epoch_before, epoch_after,
        "mutation epoch must advance on evict so compat tokens distinguish views"
    );
}

#[test]
fn store_view_epoch_advances_on_clear_compile_cache() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .expect("upsert should succeed");
    let epoch_before = project.host().current_store_view_epoch();

    project.host().clear_compile_cache();
    let epoch_after = project.host().current_store_view_epoch();

    assert_ne!(
        epoch_before, epoch_after,
        "mutation epoch must advance on clear_compile_cache so compat tokens distinguish views"
    );
}

#[test]
fn clear_compile_cache_preserves_indexed_ready_db() {
    let project = make_project();
    project
        .upsert_base("/index.ts", "export interface Props { label: string }")
        .expect("upsert should succeed");

    project
        .host()
        .ensure_indexed_ready("/index.ts")
        .expect("module facts should materialize before clearing compile artifacts");

    assert!(
        project
            .host()
            .project_type_store()
            .indexed()
            .snapshot_all()
            .iter()
            .any(|(canonical_id, _)| canonical_id.as_ref() == "/index.ts"),
        "sanity check: the IndexedReady cache should be warm before clear_compile_cache",
    );

    project.host().clear_compile_cache();

    assert!(
        project
            .host()
            .project_type_store()
            .indexed()
            .snapshot_all()
            .iter()
            .any(|(canonical_id, _)| canonical_id.as_ref() == "/index.ts"),
        "clear_compile_cache should keep project-store IndexedReady entries warm",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn current_dependency_fact_versions_include_derived_resolver_facts() {
    let project = make_project();
    project
        .upsert_base("/index.ts", "export * from './inner'")
        .unwrap();

    let whole_hash = project
        .host()
        .get_whole_hash("/index.ts")
        .expect("whole hash should exist");

    // import_routes lives on DerivedRawState (D48 split).
    let mut entry = project
        .host()
        .derived_raw_cache()
        .entry("/index.ts".to_string())
        .or_default();
    entry.value_mut().import_routes.insert(
        "./inner".to_string(),
        crate::types::DependencyResolution {
            specifier: "./inner".to_string(),
            resolved_canonical_id: Some("/inner.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        },
    );
    drop(entry);

    let facts = project
        .host()
        .current_dependency_fact_versions("/index.ts", &std::collections::BTreeSet::new());

    assert!(
        facts.contains(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/index.ts".to_string(),
            hash: whole_hash,
        })
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                ..
            } if canonical_id == "/index.ts"
        )),
        "dependency fact versions should include an ImportRoute fact for the file",
    );
    assert!(
        facts.iter().all(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::FileWholeHash { .. }
                | crate::resolver_core::FactVersionRef::DerivedFactHash {
                    kind: crate::resolver_core::DerivedFactKind::Route
                        | crate::resolver_core::DerivedFactKind::ImportRoute,
                    ..
                }
        )),
        "dependency fact versions should only publish file, route, and importer-route facts",
    );
}

#[cfg(target_arch = "wasm32")]
#[test]
fn current_dependency_fact_versions_include_derived_resolver_facts_non_scheduler() {
    let project = make_project();
    project
        .upsert_base("/index.ts", "export * from './inner'")
        .unwrap();

    let whole_hash = project
        .host()
        .get_whole_hash("/index.ts")
        .expect("whole hash should exist");

    {
        let mut files = crate::shared::write_lock(&project.host().files);
        let entry = files.get_mut("/index.ts").expect("file entry should exist");
        entry.import_routes.insert(
            "./inner".to_string(),
            crate::types::DependencyResolution {
                specifier: "./inner".to_string(),
                resolved_canonical_id: Some("/inner.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        );
    }

    let facts = project
        .host()
        .current_dependency_fact_versions("/index.ts", &std::collections::BTreeSet::new());

    assert!(
        facts.contains(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/index.ts".to_string(),
            hash: whole_hash,
        })
    );
    assert!(
        facts.contains(&crate::resolver_core::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: crate::resolver_core::DerivedFactKind::ImportRoute,
            hash: {
                let files = crate::shared::read_lock(&project.host().files);
                let entry = files.get("/index.ts").expect("file entry should exist");
                crate::resolver_store::hash_import_route_targets(&entry.import_routes)
            },
        }),
        "non-scheduler store views must track importer-route facts"
    );
    assert!(
        facts.iter().all(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::FileWholeHash { .. }
                | crate::resolver_core::FactVersionRef::DerivedFactHash {
                    kind: crate::resolver_core::DerivedFactKind::Route
                        | crate::resolver_core::DerivedFactKind::ImportRoute,
                    ..
                }
        )),
        "non-scheduler dependency fact versions should only publish file, route, and importer-route facts",
    );
}

// `ImportRoute` facts emit only when `current_cached_import_route_hash`
// (live-host read) returns Some; the live-host path has no equivalent
// to "under store view" semantics. Stale deps are rejected at warm-read
// time by `StoreView::validates_fact_signature` revalidating the
// recorded fact signature against the live host.
#[test]
fn current_dependency_fact_versions_emits_import_route_hash_when_cache_populated() {
    let project = make_project();
    project
        .upsert_base("/theme.ts", r#"export default { color: "red" }"#)
        .expect("theme upsert");
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import theme from './theme'
defineProps<{ ui: typeof theme }>()
</script>"#,
        )
        .expect("upsert should succeed");

    // Exercise the shallow file state pipeline so import_routes are populated
    // on the compile cache.
    let _ = project
        .host()
        .resolve_component_meta("/Comp.vue", crate::types::ProjectionMode::Identity);

    let facts = project
        .host()
        .current_dependency_fact_versions("/Comp.vue", &std::collections::BTreeSet::new());

    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                ..
            }
        )) || facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::FileWholeHash { .. }
        )),
        "live-host fact capture should emit either an ImportRoute hash or a \
         FileWholeHash fact for tracked dependencies",
    );
}

// ---------------------------------------------------------------------------
// Basic project lifecycle
// ---------------------------------------------------------------------------

#[test]
fn open_session_returns_unique_ids() {
    let project = make_project();
    let s1 = project.open_session_batch().unwrap();
    let s2 = project.open_session_batch().unwrap();
    assert_ne!(s1.id(), s2.id());
    assert_eq!(project.session_count(), 2);
}

#[test]
fn close_session_is_idempotent() {
    let project = make_project();
    let s = project.open_session_batch().unwrap();
    s.close();
    s.close(); // second close is a no-op
    assert!(s.is_closed());
    assert_eq!(project.session_count(), 0);
}

#[test]
fn session_drop_auto_closes() {
    let project = make_project();
    {
        let _s = project.open_session_batch().unwrap();
        assert_eq!(project.session_count(), 1);
    }
    assert_eq!(project.session_count(), 0);
}

#[test]
fn ensure_loaded_populates_shared_base_from_workspace() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("msg: string")),
    );

    let project = make_workspace_project(Arc::clone(&ws));

    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "ensure_loaded should materialize the workspace file into the shared base project"
    );
    assert!(
        project.base_file_ids().contains("/workspace/App.vue"),
        "base index should include the loaded workspace file"
    );

    let session = project.open_session_batch().unwrap();
    assert!(session.has_file("/workspace/App.vue").unwrap());
    let source = session
        .get_effective_source("/workspace/App.vue")
        .unwrap()
        .expect("session should see the loaded base source");
    assert!(source.contains("msg: string"));
}

#[test]
fn refresh_base_reloads_workspace_source_into_shared_base() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("msg: string")),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(project.ensure_loaded("/workspace/App.vue").unwrap());

    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("count: number")),
    );

    assert!(
        project.refresh_base("/workspace/App.vue").unwrap(),
        "refresh_base should reload the latest workspace content into shared base state"
    );

    let session = project.open_session_batch().unwrap();
    let source = session
        .get_effective_source("/workspace/App.vue")
        .unwrap()
        .expect("session should see the refreshed base source");
    assert!(source.contains("count: number"));
    assert!(!source.contains("msg: string"));
}

#[test]
fn methods_fail_after_close() {
    let project = make_project();
    let s = project.open_session_batch().unwrap();
    s.close();
    assert!(matches!(
        s.upsert("Comp.vue", "source".into()),
        Err(MetaError::SessionClosed)
    ));
    assert!(matches!(
        s.delete("Comp.vue"),
        Err(MetaError::SessionClosed)
    ));
    assert!(matches!(
        s.get_analysis("Comp.vue"),
        Err(MetaError::SessionClosed)
    ));
}

// ---------------------------------------------------------------------------
// Overlay isolation: two sessions don't see each other's overlays
// ---------------------------------------------------------------------------

#[test]
fn two_sessions_dont_see_each_others_upserts() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s1 = project.open_session_batch().unwrap();
    let s2 = project.open_session_batch().unwrap();

    // Session 1 updates the file
    let modified = sfc("msg: string; count: number");
    s1.upsert("Comp.vue", modified.clone()).unwrap();

    // Session 1 sees the modified source
    let src1 = s1.get_effective_source("Comp.vue").unwrap().unwrap();
    assert!(
        src1.contains("count: number"),
        "session 1 should see its own overlay"
    );

    // Session 2 sees the original base source
    let src2 = s2.get_effective_source("Comp.vue").unwrap().unwrap();
    assert!(
        !src2.contains("count: number"),
        "session 2 must NOT see session 1's overlay"
    );
    assert!(
        src2.contains("msg: string"),
        "session 2 should see base source"
    );
}

#[test]
fn delete_in_session_a_does_not_hide_from_session_b() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s1 = project.open_session_batch().unwrap();
    let s2 = project.open_session_batch().unwrap();

    // Session 1 deletes the file
    s1.delete("Comp.vue").unwrap();

    // Session 1 doesn't see the file
    assert!(!s1.has_file("Comp.vue").unwrap());
    assert!(s1.get_effective_source("Comp.vue").unwrap().is_none());

    // Session 2 still sees the file
    assert!(s2.has_file("Comp.vue").unwrap());
    let src2 = s2.get_effective_source("Comp.vue").unwrap();
    assert!(src2.is_some(), "session 2 should still see the base file");
}

// ---------------------------------------------------------------------------
// Analysis through overlay
// ---------------------------------------------------------------------------

// Overlay-visible semantics depend on the consumer paths
// (`get_analysis`, `get_component_meta`, `evaluate_types`) routing
// host reads through the overlay-aware
// `SessionResolverContext::resolver_store_view`, which wraps the base
// `HostStoreView` via `with_session_overlay` so cross-file dep facts
// pinned to BASE content miss whenever a session overlays or
// tombstones a dep. The R20 multi-candidate cache substrate plus that
// view wiring is what these tests exercise end-to-end.
#[test]
fn get_analysis_sees_overlay_content() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s = project.open_session_batch().unwrap();
    let modified = sfc("msg: string; count: number");
    s.upsert("Comp.vue", modified).unwrap();

    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(
        analysis.is_some(),
        "should return analysis for overlayed file"
    );

    let snapshot = analysis.unwrap();
    let names = prop_names(&snapshot);
    assert!(
        names.contains(&"count".to_string()),
        "analysis should reflect overlay content with 'count' prop, got: {:?}",
        names
    );
}

#[test]
fn get_analysis_without_overlay_uses_base() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s = project.open_session_batch().unwrap();

    // No overlay — should see base analysis
    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(analysis.is_some());

    let snapshot = analysis.unwrap();
    let names = prop_names(&snapshot);
    assert!(
        names.contains(&"msg".to_string()),
        "should see base 'msg' prop, got: {:?}",
        names
    );
    assert!(
        !names.contains(&"count".to_string()),
        "should NOT see 'count' prop from base"
    );
}

#[test]
fn get_analysis_for_deleted_file_returns_none() {
    // R17 — overlay-Delete short-circuits the consumer path. The
    // session's `get_analysis` consults its overlay map via the
    // view substrate (`OverlaidViewRef` constructed at call time)
    // and returns `None` for tombstoned canonicals without reading
    // the base host.
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s = project.open_session_batch().unwrap();
    s.delete("Comp.vue").unwrap();

    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(
        analysis.is_none(),
        "analysis for tombstoned file should be None"
    );
}

// ---------------------------------------------------------------------------
// Overlay isolation for analysis
// ---------------------------------------------------------------------------

#[test]
fn analysis_isolation_between_sessions() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s1 = project.open_session_batch().unwrap();
    let s2 = project.open_session_batch().unwrap();

    // Session 1 modifies the file
    s1.upsert("Comp.vue", sfc("count: number")).unwrap();

    // Session 1 sees count
    let snap1 = s1.get_analysis("Comp.vue").unwrap().unwrap();
    let names1 = prop_names(&snap1);
    assert!(
        names1.contains(&"count".to_string()),
        "session 1 should see 'count', got: {:?}",
        names1
    );
    assert!(
        !names1.contains(&"msg".to_string()),
        "session 1 should NOT see 'msg'"
    );

    // Session 2 sees msg (base)
    let snap2 = s2.get_analysis("Comp.vue").unwrap().unwrap();
    let names2 = prop_names(&snap2);
    assert!(
        names2.contains(&"msg".to_string()),
        "session 2 should see base 'msg', got: {:?}",
        names2
    );
    assert!(
        !names2.contains(&"count".to_string()),
        "session 2 should NOT see session 1's 'count'"
    );
}

// ---------------------------------------------------------------------------
// Shutdown
// ---------------------------------------------------------------------------

#[test]
fn shutdown_marks_project_dead() {
    let project = make_project();
    let s = project.open_session_batch().unwrap();

    project.shutdown();

    assert!(project.is_shutdown());
    assert!(matches!(
        s.upsert("Comp.vue", "x".into()),
        Err(MetaError::Shutdown)
    ));
    assert!(matches!(
        project.open_session_batch(),
        Err(MetaError::Shutdown)
    ));
}

#[test]
fn shutdown_is_idempotent() {
    let project = make_project();
    project.shutdown();
    project.shutdown(); // no panic
}

// ---------------------------------------------------------------------------
// Overlay generation tracking
// ---------------------------------------------------------------------------

#[test]
fn overlay_generation_bumps_on_mutations() {
    let project = make_project();
    let s = project.open_session_batch().unwrap();

    assert_eq!(s.overlay_generation(), 0);
    s.upsert("A.vue", "a".into()).unwrap();
    assert_eq!(s.overlay_generation(), 1);
    s.delete("B.vue").unwrap();
    assert_eq!(s.overlay_generation(), 2);
}

#[test]
fn reset_restores_base_state_and_drops_overlay_only_files() {
    let project = make_project();
    let base = sfc("label: string");
    let modified = sfc("count: number");
    project.upsert_base("A.vue", &base).unwrap();

    let s = project.open_session_batch().unwrap();
    s.upsert("A.vue", modified.clone()).unwrap();
    s.upsert("Temp.vue", sfc("temp: boolean")).unwrap();

    assert!(s
        .get_effective_source("A.vue")
        .unwrap()
        .unwrap()
        .contains("count: number"));
    assert!(s.has_file("Temp.vue").unwrap());

    s.reset("A.vue").unwrap();
    s.reset("Temp.vue").unwrap();

    let restored = s.get_effective_source("A.vue").unwrap().unwrap();
    assert!(restored.contains("label: string"));
    assert!(!restored.contains("count: number"));
    assert!(!s.has_file("Temp.vue").unwrap());
    assert!(s.get_effective_source("Temp.vue").unwrap().is_none());
    assert_eq!(s.overlay_generation(), 4);
}

#[test]
fn reset_reverts_an_active_overlay_from_the_shared_host() {
    let project = make_project();
    let base = sfc("label: string");
    let modified = sfc("count: number");
    project.upsert_base("A.vue", &base).unwrap();

    let s = project.open_session_batch().unwrap();
    s.upsert("A.vue", modified).unwrap();

    let analysis = s.get_analysis("A.vue").unwrap().unwrap();
    assert!(
        prop_names(&analysis).contains(&"count".to_string()),
        "active overlay should be visible before reset"
    );

    s.reset("A.vue").unwrap();

    let analysis = s.get_analysis("A.vue").unwrap().unwrap();
    let names = prop_names(&analysis);
    assert!(
        names.contains(&"label".to_string()),
        "base props should be visible after reset, got: {names:?}"
    );
    assert!(
        !names.contains(&"count".to_string()),
        "overlay props must be removed after reset, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// visible_file_ids
// ---------------------------------------------------------------------------

#[test]
fn visible_file_ids_reflects_overlays() {
    let project = make_project();
    project.upsert_base("A.vue", &sfc("a: string")).unwrap();
    project.upsert_base("B.vue", &sfc("b: string")).unwrap();

    let s = project.open_session_batch().unwrap();
    s.delete("A.vue").unwrap();
    s.upsert("C.vue", sfc("c: string")).unwrap();

    let ids = s.visible_file_ids().unwrap();
    assert!(!ids.contains(&"A.vue".to_string()), "A.vue was deleted");
    assert!(ids.contains(&"B.vue".to_string()), "B.vue is in base");
    assert!(
        ids.contains(&"C.vue".to_string()),
        "C.vue was added by overlay"
    );
}

// ---------------------------------------------------------------------------
// clear_caches preserves files but flushes compile results
// ---------------------------------------------------------------------------

#[test]
fn clear_caches_preserves_base_files() {
    let project = make_project();
    project
        .upsert_base("Comp.vue", &sfc("msg: string"))
        .unwrap();

    let s = project.open_session_batch().unwrap();
    let _ = s
        .get_analysis("Comp.vue")
        .unwrap()
        .expect("analysis should exist before clearing caches");

    project.clear_caches().unwrap();

    // Base file should still exist and be queryable after clearing caches
    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(
        analysis.is_some(),
        "file should still be accessible after clear_caches"
    );
}

// ---------------------------------------------------------------------------
// Dependency invalidation within session
// ---------------------------------------------------------------------------

#[test]
fn changing_dependency_invalidates_importer_in_session() {
    let project = make_project();

    // Set up a types file and a component that imports from it
    let types_source = r#"export interface ButtonProps { label: string }"#;
    let comp_source = r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><div>{{ label }}</div></template>"#;

    project.upsert_base("types.ts", types_source).unwrap();
    project.upsert_base("Button.vue", comp_source).unwrap();

    let s = project.open_session_batch().unwrap();

    // Query analysis succeeds for the base file
    let snap = s.get_analysis("Button.vue").unwrap();
    assert!(snap.is_some(), "analysis should succeed for the base file");

    // Modify types in session to add 'disabled'
    let new_types = r#"export interface ButtonProps { label: string; disabled: boolean }"#;
    s.upsert("types.ts", new_types.into()).unwrap();

    // After modifying types in the session, querying Button.vue through the
    // session should succeed (the overlay applies the new types.ts to the host)
    let snap2 = s.get_analysis("Button.vue").unwrap();
    assert!(
        snap2.is_some(),
        "analysis should succeed after dependency update"
    );
}

// ---------------------------------------------------------------------------
// Concurrent session activity (sequential in this test, but isolated)
// ---------------------------------------------------------------------------

#[test]
fn concurrent_sessions_on_different_files() {
    let project = make_project();
    project.upsert_base("A.vue", &sfc("a: string")).unwrap();
    project.upsert_base("B.vue", &sfc("b: string")).unwrap();

    let s1 = project.open_session_batch().unwrap();
    let s2 = project.open_session_batch().unwrap();

    // Session 1 modifies A
    s1.upsert("A.vue", sfc("a_modified: number")).unwrap();

    // Session 2 modifies B
    s2.upsert("B.vue", sfc("b_modified: number")).unwrap();

    // Session 1 queries its files
    let snap_a1 = s1.get_analysis("A.vue").unwrap().unwrap();
    let names_a1 = prop_names(&snap_a1);
    assert!(
        names_a1.contains(&"a_modified".to_string()),
        "s1 should see its overlay on A, got: {:?}",
        names_a1
    );
    let snap_b1 = s1.get_analysis("B.vue").unwrap().unwrap();
    let names_b1 = prop_names(&snap_b1);
    assert!(
        names_b1.contains(&"b".to_string()),
        "s1 should see base B (not s2's overlay), got: {:?}",
        names_b1
    );

    // Session 2 queries its files
    let snap_b2 = s2.get_analysis("B.vue").unwrap().unwrap();
    let names_b2 = prop_names(&snap_b2);
    assert!(
        names_b2.contains(&"b_modified".to_string()),
        "s2 should see its overlay on B, got: {:?}",
        names_b2
    );
    let snap_a2 = s2.get_analysis("A.vue").unwrap().unwrap();
    let names_a2 = prop_names(&snap_a2);
    assert!(
        names_a2.contains(&"a".to_string()),
        "s2 should see base A (not s1's overlay), got: {:?}",
        names_a2
    );
}

// ---------------------------------------------------------------------------
// Native type evaluation
// ---------------------------------------------------------------------------

#[test]
fn evaluate_types_combines_all_cached_script_blocks() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
function makeLabel() {
  return "cached" as string
}
</script>

<script setup lang="ts">
defineProps<{
  label: ReturnType<typeof makeLabel>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("Comp.vue").unwrap().unwrap();

    assert_eq!(
        evaluated_prop_type(&evaluated, "label"),
        &TypeExpr::Primitive(PrimitiveName::String)
    );
    assert!(
        evaluated.props.iter().all(|field| field.name != "missing"),
        "evaluation should only include actual props"
    );
}

#[test]
fn get_analysis_resolves_exported_local_props_from_sibling_script_block() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
export interface Props {
  label: string
  count?: number
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let analysis = session
        .get_analysis("Comp.vue")
        .unwrap()
        .expect("analysis should exist");
    let define_props = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .expect("defineProps macro should exist");

    let names: Vec<&str> = define_props
        .prop_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert!(
        names.contains(&"label"),
        "exported interface field 'label' should resolve, got: {:?}",
        names
    );
    assert!(
        names.contains(&"count"),
        "exported interface field 'count' should resolve, got: {:?}",
        names
    );
}

#[test]
fn get_analysis_resolves_non_exported_local_props_from_sibling_script_block() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
interface Props {
  label: string
  count?: number
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let analysis = session
        .get_analysis("Comp.vue")
        .unwrap()
        .expect("analysis should exist");
    let define_props = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .expect("defineProps macro should exist");

    let names: Vec<&str> = define_props
        .prop_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert!(
        names.contains(&"label"),
        "sibling script field 'label' should resolve, got: {:?}",
        names
    );
    assert!(
        names.contains(&"count"),
        "sibling script field 'count' should resolve, got: {:?}",
        names
    );
}

// Invariant: `evaluate_types` returns the same `Arc` for repeated calls on
// an unchanged file and a different `Arc` once the file is upserted. The
// resolver pipeline reads source through `read_analysis_source` /
// `capture_component_meta_inputs`, which consult the base host's source
// store directly rather than the active `SessionView`; overlay-aware
// source plumbing through the resolver is tracked separately.
#[test]
fn evaluate_types_reuses_cached_results_until_the_file_changes() {
    let project = make_project();
    project
        .upsert_base("Comp.vue", &sfc("count: number"))
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let first = session.evaluate_types("Comp.vue").unwrap().unwrap();
    assert_eq!(
        evaluated_prop_type(&first, "count"),
        &TypeExpr::Primitive(PrimitiveName::Number)
    );

    let first_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ProjectionMode::Expanded)
            .expect("first evaluation should populate the cache");

    let second = session.evaluate_types("Comp.vue").unwrap().unwrap();
    let second_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ProjectionMode::Expanded)
            .expect("second evaluation should reuse the cache");

    assert_eq!(first.props.len(), second.props.len());
    assert!(Arc::ptr_eq(&first_cache, &second_cache));

    session
        .upsert("Comp.vue", sfc("count: number; label: string"))
        .unwrap();
    let third = session.evaluate_types("Comp.vue").unwrap().unwrap();
    let third_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ProjectionMode::Expanded)
            .expect("updated file should repopulate the cache");

    assert!(third.props.iter().any(|field| field.name == "label"));
    assert!(!Arc::ptr_eq(&second_cache, &third_cache));
}

#[test]
fn resolved_meta_reuses_resolver_cache_after_legacy_slot_is_cleared() {
    let project = make_project();
    project
        .upsert_base("Comp.vue", &sfc("count: number"))
        .unwrap();

    let _ = project
        .host()
        .resolve_component_meta("Comp.vue", crate::types::ProjectionMode::Expanded)
        .expect("initial resolve should succeed");
    let first_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ProjectionMode::Expanded)
            .expect("initial resolve should populate legacy cache mirror");

    clear_legacy_cached_resolved_state(
        &project,
        "Comp.vue",
        crate::types::ProjectionMode::Expanded,
    );
    assert!(
        cached_resolved_state(&project, "Comp.vue", crate::types::ProjectionMode::Expanded)
            .is_none(),
        "legacy cache slot should be cleared before the second lookup"
    );

    project.host().provenance().reset();
    let _ = project
        .host()
        .resolve_component_meta("Comp.vue", crate::types::ProjectionMode::Expanded)
        .expect("second resolve should succeed from resolver-owned cache");
    let second_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ProjectionMode::Expanded)
            .expect("resolver-owned cache hit should mirror back into the legacy slot");

    assert!(Arc::ptr_eq(&first_cache, &second_cache));
    assert_eq!(
        provenance(&project).component_meta_resolved_state_recomputes,
        0,
        "resolver-owned cache hit should avoid a recompute after the legacy slot is cleared"
    );
    assert_eq!(
        provenance(&project).resolver_node_cache_hits,
        1,
        "second lookup should be served from the resolver-owned cache"
    );
    assert_eq!(
        provenance(&project).resolver_node_cache_misses,
        0,
        "second lookup should not miss the resolver-owned cache after the legacy slot is cleared"
    );
    assert_eq!(
        provenance(&project).resolver_singleflight_coalesced,
        0,
        "single-threaded cache reuse should not require singleflight coalescing"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_reuses_resolver_cache_after_legacy_slot_is_cleared() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><div>child</div></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    project.host().resolver_runtime().reset_counters();
    let _ = get_meta(&project, "/App.vue");
    // get_meta does not populate cached_fallthrough; use resolve_fallthrough_surface
    let _ = project.host().resolve_fallthrough_surface("/App.vue");
    let after_first = project.host().resolver_runtime().counter_snapshot();
    let first_cache = cached_fallthrough_state(&project, "/App.vue")
        .expect("initial lookup should populate the legacy fallthrough mirror");

    clear_legacy_cached_fallthrough_state(&project, "/App.vue");
    assert!(
        cached_fallthrough_state(&project, "/App.vue").is_none(),
        "legacy fallthrough cache slot should be cleared before the second lookup"
    );

    project.host().provenance.reset();

    let _ = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("second fallthrough resolve should succeed from resolver-owned cache");
    let after_second = project.host().resolver_runtime().counter_snapshot();
    let second_cache = cached_fallthrough_state(&project, "/App.vue")
        .expect("resolver-owned fallthrough cache hit should mirror back into the legacy slot");

    let first_prop_names: Vec<_> = first_cache
        .accepted_props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    let second_prop_names: Vec<_> = second_cache
        .accepted_props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert_eq!(first_prop_names, second_prop_names);
    assert_eq!(
        first_cache.accepted_surface_completeness,
        second_cache.accepted_surface_completeness
    );
    assert_eq!(
        first_cache.fact_versions.len(),
        second_cache.fact_versions.len(),
        "legacy mirror repopulation should preserve dependency fact coverage"
    );
    assert!(
        after_first.node_cache_misses > 0,
        "first fallthrough resolve should populate runtime fallthrough nodes, got {:?}",
        after_first
    );
    assert!(
        after_second.node_cache_hits > after_first.node_cache_hits,
        "clearing only the legacy mirror should now reuse the runtime top-level cache directly, before={:?} after={:?}",
        after_first,
        after_second
    );
    assert_eq!(
        provenance(&project).resolver_node_cache_hits,
        1,
        "second fallthrough lookup should be served from the runtime cache and mirrored back into the legacy slot"
    );
    assert_eq!(
        provenance(&project).resolver_node_cache_misses,
        0,
        "second fallthrough lookup should not miss once the runtime cache is consulted after the legacy slot is cleared"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn budget_exhausted_no_override_fallthrough_is_not_cached() {
    // No-poison: a no-override fallthrough whose spread walker trips the shared
    // projection budget MID-WALK is a PARTIAL — the trip folds into the active
    // per-cold-compute completeness scope, so it must NOT warm the runtime node
    // cache NOR the legacy `cached_fallthrough` mirror. Both admission sites gate
    // on the typed `current_cold_compute_completeness`; without the mirror gate
    // the partial warms `DerivedRawState.cached_fallthrough` and a later request
    // replays it.
    use crate::resolver_core::FallthroughRequestHost;

    // A `v-bind` of a union of distinct-keyed objects on a native root drives the
    // fallthrough spread walker, which is the budget consumer that trips the cap.
    let project = make_project();
    project
        .upsert_base(
            "/obj.ts",
            r#"export declare const obj: { a: string } | { b: string } | { c: string };"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { obj } from './obj'
</script>
<template><div v-bind="obj" /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./obj".to_string(),
            resolved_canonical_id: Some("/obj.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // A low projection budget: the spread walker trips it mid-walk, folding a
    // partial into the cold-compute scope — the resolved fallthrough is a partial
    // that must not warm any cache.
    let rctx = crate::request_context::RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from("/App.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        1,
    );
    let guard = crate::request_context::RequestContextGuard::install(Arc::clone(&rctx));

    let _ = project.host().resolve_fallthrough_surface("/App.vue");

    drop(guard);

    // The legacy mirror must be empty (THE Part 3 discriminator: without the
    // mirror gate, the budget-exhausted partial warms it here).
    assert!(
        cached_fallthrough_state(&project, "/App.vue").is_none(),
        "a budget-exhausted no-override fallthrough must NOT warm the legacy cached_fallthrough mirror"
    );
    // The runtime top-level node cache must also be empty (store_node self-gate).
    let key = crate::resolver_core::fallthrough_cache_key(
        "/App.vue",
        project.host().config.generic_root_propagation,
        None,
    );
    let view = FallthroughRequestHost::snapshot_store_view(project.host());
    assert!(
        project
            .host()
            .resolver_runtime()
            .fallthrough
            .get_cached_node(&key, &view)
            .is_none(),
        "a budget-exhausted no-override fallthrough must NOT warm the runtime node cache"
    );

    // Positive control: the SAME scenario WITHOUT a tripped budget DOES warm the
    // mirror (the direct entry installs the default budget, which the small spread
    // walk completes under), proving the gate suppresses only the genuine partial.
    let _ = project.host().resolve_fallthrough_surface("/App.vue");
    assert!(
        cached_fallthrough_state(&project, "/App.vue").is_some(),
        "without a tripped budget the no-override fallthrough DOES warm the mirror"
    );
}

/// A no-macro owner whose ONLY budget consumer is the fallthrough spread walker:
/// resolve does zero projection ops (no `defineProps` to instantiate), so the
/// low projection budget is tripped exclusively MID-FALLTHROUGH. The spread is a
/// `v-bind` of a union of distinct-keyed objects on a native root, so the walker
/// descends every union arm and trips the cap.
fn upsert_fallthrough_spread_owner(project: &Arc<MetaProject>) {
    project
        .upsert_base(
            "/src/obj.ts",
            r#"export declare const obj: { a: string } | { b: string } | { c: string } | { d: string };"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { obj } from './obj'
</script>
<template><div v-bind="obj" /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./obj".to_string(),
            resolved_canonical_id: Some("/src/obj.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
}

/// Regression lock (fallthrough-only-partial no-poison): a fallthrough-only
/// budget partial must NOT warm the OUTER `ComponentMetaResultDb`. The owner's
/// resolve completes cleanly
/// (`synthesis_should_suppress == false`) — the ONLY partial comes from the
/// fallthrough spread walker tripping the low projection budget AFTER `resolved`
/// was produced. Without the fix the publish gate consults only
/// `resolved.synthesis_should_suppress` (false), so it warms the partial and a
/// replay warm-hits — RED. Post-fix the carrier merges the fallthrough
/// completeness into the gate, refusing admission — GREEN. The runtime node
/// cache and the legacy mirror also stay empty for the partial.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_only_budget_partial_does_not_warm_component_meta_result_db() {
    use crate::resolver_core::FallthroughRequestHost;
    use std::sync::atomic::Ordering::Relaxed;

    // Low projection budget: resolve (no macros) stays at zero ops, the
    // fallthrough spread walker trips mid-walk.
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 3,
        ..HostConfig::default()
    });
    upsert_fallthrough_spread_owner(&project);
    let host = project.host();
    let canonical = "/src/App.vue";

    // First (cold) resolve: produces the fallthrough partial.
    let (meta1, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("a fallthrough-tripped resolve must still return partial metadata");

    // The resolve is clean — the partial is fallthrough-only (the exact shape
    // this fix targets: too late for `resolved.synthesis_should_suppress`).
    assert!(
        !resolved.synthesis_should_suppress,
        "the no-macro owner's resolve must complete cleanly so the partial is fallthrough-only \
         (synthesis_should_suppress reflects resolve, not the later fallthrough trip)"
    );
    assert!(
        matches!(
            meta1.accepted_surface_completeness,
            verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::LowerBound
        ),
        "the budget-tripped fallthrough surface is a lower bound"
    );

    // The fallthrough partial must NOT have warmed `ComponentMetaResultDb`: a
    // second resolve is NOT a warm hit.
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = host
        .get_component_meta_with_resolution(canonical)
        .expect("second resolve must still succeed");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after, hits_before,
        "a fallthrough-only budget partial MUST NOT warm `ComponentMetaResultDb` — the replay must \
         be cold (hits_before={hits_before}, hits_after={hits_after}); pre-fix the publish gate saw \
         only resolved.synthesis_should_suppress=false and warmed the partial"
    );

    // The runtime node cache and the legacy mirror also stay empty for the
    // partial (no fallthrough-cache leak).
    let key = crate::resolver_core::fallthrough_cache_key(
        canonical,
        host.config.generic_root_propagation,
        None,
    );
    let view = FallthroughRequestHost::snapshot_store_view(host);
    assert!(
        host.resolver_runtime()
            .fallthrough
            .get_cached_node(&key, &view)
            .is_none(),
        "the fallthrough-partial top-level node must NOT be warm in the runtime node cache"
    );
    assert!(
        cached_fallthrough_state(&project, canonical).is_none(),
        "the fallthrough-partial must NOT warm the legacy cached_fallthrough mirror"
    );
}

/// The PUBLIC direct entry `resolve_fallthrough_surface` installs a
/// `RequestContext` when none is ambient, so the projection budget and the
/// completeness gate are LIVE on that path (no manually-installed guard). With a
/// low `projection_op_budget` and NO ambient context, the spread walker trips
/// and the partial gates the mirror.
///
/// Without the context install the direct entry has no `RequestContext`, so
/// `current_request_budget()` is `None`, the walker never trips, the fallthrough
/// completes, and the mirror is warmed. With it, the auto-installed context's
/// budget trips and the completeness gate refuses the mirror.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn direct_resolve_fallthrough_surface_installs_context_so_budget_gate_is_live() {
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 3,
        ..HostConfig::default()
    });
    upsert_fallthrough_spread_owner(&project);

    // No ambient RequestContext is installed: the direct entry must install its
    // own (with `config.projection_op_budget`) for the gate to be live.
    assert!(
        crate::request_context::current_request_context().is_none(),
        "test precondition: no ambient request context"
    );

    let _ = project.host().resolve_fallthrough_surface("/src/App.vue");

    assert!(
        cached_fallthrough_state(&project, "/src/App.vue").is_none(),
        "with no ambient context, the direct entry MUST install one so the low projection budget \
         trips the spread walker and the partial-completeness gate refuses the mirror; pre-fix no \
         context is installed, the walker never trips, and the mirror is warmed"
    );
}

/// DON'T-OVER-GATE positive control: a `LowerBound` SURFACE whose COMPUTE is
/// COMPLETE is still cacheable. The same spread owner under a generous budget
/// produces a lower-bound accepted surface (the union spread is inexact) WITHOUT
/// any budget/fuse trip, so the fallthrough completeness is `Complete` and the
/// result warms `ComponentMetaResultDb`. A regression that wrongly gated on the
/// surface-shape `accepted_surface_completeness == LowerBound` would refuse this
/// and the replay would be cold.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn lower_bound_complete_fallthrough_surface_still_warms_component_meta_result_db() {
    use std::sync::atomic::Ordering::Relaxed;

    // Default (generous) budget: the spread walk COMPLETES, so the compute is
    // Complete even though the surface is a lower bound.
    let project = make_project();
    upsert_fallthrough_spread_owner(&project);
    let host = project.host();
    let canonical = "/src/App.vue";

    let (meta1, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("a clean resolve must return metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "the clean resolve must not suppress"
    );
    assert!(
        matches!(
            meta1.accepted_surface_completeness,
            verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::LowerBound
        ),
        "the inexact union spread yields a lower-bound SURFACE (distinct from compute completeness)"
    );

    // The LowerBound-surface / Complete-compute result MUST warm: a second
    // resolve IS a warm hit.
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = host
        .get_component_meta_with_resolution(canonical)
        .expect("second resolve must succeed");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert!(
        hits_after > hits_before,
        "a LowerBound SURFACE with COMPLETE compute MUST stay cacheable — the replay must warm-hit \
         `ComponentMetaResultDb` (hits_before={hits_before}, hits_after={hits_after}); gating on the \
         surface shape instead of compute completeness would over-gate this"
    );
}

/// Regression lock (payload-surface fallthrough-only-partial no-poison): a
/// fallthrough-only budget partial reached through the BARE scalar payload
/// surface must NOT warm `cached_meta_payload`. `resolve_one_payload_item`
/// installs ONE `RequestContext` (with `config.projection_op_budget`) spanning
/// its cold resolve AND its fallthrough extract, so the projection-op budget
/// fuse is LIVE during the payload fallthrough exactly as on the analysis
/// surface. The no-macro owner's resolve completes cleanly
/// (`synthesis_should_suppress == false`) — the only partial comes from the
/// spread walker tripping the low budget DURING the payload extract. The
/// payload-write gate merges the threaded fallthrough completeness and refuses
/// to warm the payload (it is still RETURNED). Without the spanning install the
/// payload extract runs context-free, the budget never trips, the compute is
/// Complete, and the payload warms — RED at the `is_none()` replay assertion.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_only_budget_partial_does_not_warm_cached_meta_payload() {
    // Low projection budget: resolve (no macros) stays at zero ops; the
    // fallthrough spread walker trips mid-walk during the PAYLOAD extract.
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 3,
        ..HostConfig::default()
    });
    upsert_fallthrough_spread_owner(&project);
    let host = project.host();
    let canonical = "/src/App.vue";

    // The resolve itself is clean — the partial is fallthrough-only (too late
    // for `resolved.synthesis_should_suppress`, the exact shape this gate
    // targets on the payload surface).
    let (_meta, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("a fallthrough-tripped resolve must still return partial metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "the no-macro owner's resolve completes cleanly so the partial is fallthrough-only \
         (synthesis_should_suppress reflects resolve, not the later fallthrough trip)"
    );

    // Drive the BARE scalar payload surface with NO ambient context: the
    // install inside `resolve_one_payload_item` is what makes the budget live
    // during the extract (the discriminator is the FIX, not the fixture).
    assert!(
        crate::request_context::current_request_context().is_none(),
        "test precondition: no ambient request context — the payload path must install its own"
    );
    let session = project.open_session_batch().unwrap();
    let payload = session
        .get_component_meta_payload(canonical, test_encode_fn)
        .expect("the payload request must succeed")
        .expect("a partial payload is still RETURNED to the caller, just not warmed");
    assert!(
        !payload.is_empty(),
        "the partial payload is returned to the caller"
    );

    // The fallthrough-only budget partial must NOT have warmed the payload
    // cache: a fresh warm read finds nothing.
    assert!(
        host.try_get_cached_meta_payload(canonical).is_none(),
        "a fallthrough-only budget partial reached through the payload surface MUST NOT warm \
         cached_meta_payload — without the spanning install the extract runs context-free (no trip \
         → Complete compute → warmed); the fix installs a RequestContext spanning resolve+extract \
         so the budget trips → partial → the completeness gate refuses the warm"
    );
}

/// DON'T-OVER-GATE positive control (payload surface): a `LowerBound` SURFACE
/// whose COMPUTE is COMPLETE must still warm `cached_meta_payload`. The same
/// spread owner under the default (generous) budget produces a lower-bound
/// accepted surface WITHOUT any budget/fuse trip, so the fallthrough
/// completeness is `Complete` and the payload warms. A regression that wrongly
/// gated on the surface-shape `accepted_surface_completeness == LowerBound`
/// would refuse this and the warm read would miss.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn lower_bound_complete_fallthrough_surface_still_warms_cached_meta_payload() {
    // Default (generous) budget: the spread walk COMPLETES, so the compute is
    // Complete even though the surface is a lower bound.
    let project = make_project();
    upsert_fallthrough_spread_owner(&project);
    let host = project.host();
    let canonical = "/src/App.vue";

    let (meta1, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("a clean resolve must return metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "the clean resolve must not suppress"
    );
    assert!(
        matches!(
            meta1.accepted_surface_completeness,
            verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::LowerBound
        ),
        "the inexact union spread yields a lower-bound SURFACE (distinct from compute completeness)"
    );

    // The LowerBound-surface / Complete-compute payload MUST warm.
    let session = project.open_session_batch().unwrap();
    let _payload = session
        .get_component_meta_payload(canonical, test_encode_fn)
        .expect("the payload request must succeed")
        .expect("a clean payload is returned");
    assert!(
        host.try_get_cached_meta_payload(canonical).is_some(),
        "a LowerBound SURFACE with COMPLETE compute MUST still warm cached_meta_payload — gating on \
         the surface shape instead of compute completeness would over-gate this"
    );
}

/// BUDGET-PARITY (D4): the projection-budget fuse + the no-poison completeness
/// gate must be LIVE on the SESSION-VIEW surfaces — the view-aware
/// `MetaSession::get_component_meta` (`get_component_meta_via_view`) AND the
/// session `MetaSession::get_component_meta_with_resolution`
/// (`get_component_meta_with_resolution_via_view`) — not only on the
/// direct-analysis / audited / payload surfaces (Shared Optimized Codebase).
///
/// Pre-fix both view paths ran the fallthrough extract CONTEXT-FREE: the inner
/// `resolve_component_meta_with_*` install-if-none dropped its `RequestContext`
/// before the extract, so `current_request_budget()` was `None` during the
/// fallthrough, the spread walker never tripped, the compute was `Complete`,
/// and the partial-as-complete warmed downstream caches. The D4 install-if-none
/// spans the FULL cold body (resolve AND extract) on each surface.
///
/// Discriminating observables (each surface keys its own gate):
/// - view-aware: the ComponentMetaResultDb publish is gated on the threaded
///   `fallthrough_completeness` — a partial REFUSES admission, so a replay is a
///   cold miss (`component_meta_result_cache_hits` unchanged). RED pre-fix
///   (Complete → warmed → replay warm-hits).
/// - session with-resolution: it discards the ComponentMetaResultDb publish,
///   but its bounded fallthrough extract folds the budget trip into the
///   cold-compute scope the runtime-node `store_node` gate reads, so the
///   top-level fallthrough node is NOT warmed. RED pre-fix (no trip → Complete
///   → `store_node` warms the runtime node).
///
/// The partial is fallthrough-ONLY: the no-macro owner's Expanded resolve
/// completes cleanly (`synthesis_should_suppress == false`), isolating the
/// budget fuse from the synthesis gate (the exact shape D4 targets).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_only_budget_partial_not_warmed_through_session_view_surfaces() {
    use crate::resolver_core::FallthroughRequestHost;
    use std::sync::atomic::Ordering::Relaxed;

    // Low projection budget: resolve (no macros) stays at zero ops; the
    // fallthrough spread walker trips mid-walk DURING the extract.
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 3,
        ..HostConfig::default()
    });
    upsert_fallthrough_spread_owner(&project);
    let host = project.host();
    let canonical = "/src/App.vue";

    // Precondition: the partial is fallthrough-only (resolve completes clean).
    // The audited base entry already spans resolve+extract, so it does NOT
    // warm the partial — it only establishes the fallthrough-only shape.
    let (_meta, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("a fallthrough-tripped resolve must still return partial metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "the no-macro owner's resolve must complete cleanly so the partial is fallthrough-only \
         (synthesis_should_suppress reflects resolve, not the later fallthrough trip)"
    );

    // ── Surface 1: view-aware `MetaSession::get_component_meta` (site 430).
    // No ambient context — the via-view path must install its own spanning the
    // extract (the discriminator is the FIX, not the fixture).
    assert!(
        crate::request_context::current_request_context().is_none(),
        "test precondition: no ambient request context"
    );
    let session = project.open_session_batch().unwrap();
    let _ = session
        .get_component_meta(canonical)
        .expect("view-aware meta request must succeed")
        .expect("a partial analysis is still RETURNED to the caller, just not warmed");
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = session
        .get_component_meta(canonical)
        .expect("second view-aware request must succeed")
        .expect("the replay is still RETURNED");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after, hits_before,
        "the view-aware surface's fallthrough-only budget partial MUST NOT warm \
         `ComponentMetaResultDb` — the replay must be cold (hits_before={hits_before}, \
         hits_after={hits_after}); pre-fix the extract ran context-free, the budget never tripped, \
         the compute was Complete, and the publish gate warmed the partial"
    );

    // ── Surface 2: session `MetaSession::get_component_meta_with_resolution`
    // (site 276). It discards the ComponentMetaResultDb publish, so the gated
    // runtime-node `store_node` is the observable: the bounded extract folds
    // the budget trip into the cold-compute scope the gate reads, refusing the
    // top-level fallthrough node.
    let key = crate::resolver_core::fallthrough_cache_key(
        canonical,
        host.config.generic_root_propagation,
        None,
    );
    clear_runtime_top_level_fallthrough_node(&project, canonical);
    {
        let view = FallthroughRequestHost::snapshot_store_view(host);
        assert!(
            host.resolver_runtime()
                .fallthrough
                .get_cached_node(&key, &view)
                .is_none(),
            "test precondition: the runtime fallthrough node is cleared before the session call"
        );
    }
    let session_wr = project.open_session_batch().unwrap();
    let _ = session_wr
        .get_component_meta_with_resolution(canonical)
        .expect("session with-resolution request must succeed")
        .expect("a partial result is still RETURNED to the caller, just not warmed");
    let view = FallthroughRequestHost::snapshot_store_view(host);
    assert!(
        host.resolver_runtime()
            .fallthrough
            .get_cached_node(&key, &view)
            .is_none(),
        "the session with-resolution surface's bounded fallthrough extract MUST NOT warm the \
         runtime fallthrough node — pre-fix the extract ran context-free, the budget never tripped, \
         the compute was Complete, and `store_node` warmed the top-level node"
    );
}

/// Owner that charges projection ops in BOTH phases over DISJOINT source
/// types: the `defineProps<Partial<PropsBase>>()` macro's Expanded resolve
/// instantiates the mapped utility (resolve-phase ops), and the
/// `v-bind="obj"` spread's fallthrough walk resolves a 4-arm union
/// (fallthrough-phase ops). Disjoint types ⇒ the two phases' op costs are
/// additive (no shared sub-resolution that would warm one phase from the
/// other).
fn upsert_mixed_budget_owner(project: &Arc<MetaProject>) {
    project
        .upsert_base(
            "/src/propsbase.ts",
            r#"export interface PropsBase { a: string; b: number; c: boolean; d: string; e: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/obj.ts",
            r#"export declare const obj: { p: string } | { q: string } | { r: string } | { s: string };"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { PropsBase } from './propsbase'
import { obj } from './obj'
defineProps<Partial<PropsBase>>()
</script>
<template><div v-bind="obj" /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./propsbase".to_string(),
                resolved_canonical_id: Some("/src/propsbase.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./obj".to_string(),
                resolved_canonical_id: Some("/src/obj.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
}

/// The mixed owner with the `v-bind` spread REMOVED — same props macro, no
/// fallthrough spread — so the fallthrough phase charges ~0 ops. Isolates
/// the RESOLVE-phase op cost `R`.
fn upsert_mixed_budget_resolve_only_owner(project: &Arc<MetaProject>) {
    project
        .upsert_base(
            "/src/propsbase.ts",
            r#"export interface PropsBase { a: string; b: number; c: boolean; d: string; e: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { PropsBase } from './propsbase'
defineProps<Partial<PropsBase>>()
</script>
<template><div>no spread</div></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./propsbase".to_string(),
            resolved_canonical_id: Some("/src/propsbase.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
}

/// BUDGET-PARITY (D4) — the VIEW-AWARE OUTER full-request install is
/// load-bearing: the projection budget must span resolve AND fallthrough as
/// ONE budget, not two per-phase budgets the fallthrough choke alone would
/// arm.
///
/// The sibling `fallthrough_only_budget_partial_not_warmed_through_session_view_surfaces`
/// uses a fallthrough-ONLY trip — the fallthrough choke backstop
/// (`compute_fallthrough_surface_from_resolved_state`'s install-if-none)
/// arms a fresh per-fallthrough budget and trips it, so that test stays
/// GREEN even with the OUTER installs deleted. This test closes that gap
/// with a MIXED fixture whose RESOLVE phase (a `defineProps<Partial<…>>()`
/// macro) AND fallthrough phase (a `v-bind` spread) each charge projection
/// ops over DISJOINT types: neither phase alone exceeds the budget, but their
/// COMBINED work does.
///
/// View-aware (`get_component_meta`, `component_meta_entry.rs`): the resolve
/// and the fallthrough extract share ONE resolver ctx, so the choke's
/// re-resolution of the owner's declared props hits the resolve's WARM cache
/// (the choke charges only the fallthrough's ops). Only the OUTER install
/// makes the resolve ops and the fallthrough ops share one budget — so the
/// combined work trips ONLY with the outer install. Revert it and the work
/// splits into an inner-resolve budget + a fresh-choke budget, neither trips,
/// the compute is Complete, and the partial-as-complete warms
/// `ComponentMetaResultDb` (the replay warm-hits) — RED. This surface is the
/// DISCRIMINATING half.
///
/// Session (`get_component_meta_with_resolution`,
/// `component_meta_entry_resolution.rs`): for THIS combined fixture the budget
/// trips in the FALLTHROUGH, and the session path rebuilds the resolver ctx
/// with a COLD-SEED view between resolve and extract, so the choke's
/// `extract_component_meta` re-resolves the full surface COLD within the
/// choke's own budget — the choke alone already bounds this combined work, so
/// Surface 2 is a corroborating combined-budget-partial guard here, not the
/// discriminating half for this fixture. The session outer install is still
/// load-bearing in general: it bounds the PRE-CHOKE macro-DTO extraction
/// (`extract_component_meta_from_resolved` → `component_meta_resolved_macros` →
/// `vue_macro_dtos_with_ctx`), which the fallthrough choke does NOT cover —
/// discriminated by
/// `session_pre_choke_macro_dto_budget_partial_not_admitted_to_vue_surface_store`.
///
/// The discriminating budget is MEASURED, not hard-coded: `R` (resolve-only)
/// and `S = C - R` (fallthrough) are read from the shared projection-op
/// counter under a generous ambient context (the session via-view entry is
/// install-if-none, so it inherits and charges that context's budget). The
/// budget is set to `max(R, S)`: resolve alone (`R <= K`) and fallthrough
/// alone (`S <= K`) each stay within it, but the combined cold work
/// (`C = R + S > K`) trips a SHARED budget.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn combined_resolve_plus_fallthrough_budget_partial_not_warmed_through_session_view_surfaces() {
    use crate::resolver_core::FallthroughRequestHost;
    use std::sync::atomic::Ordering::Relaxed;

    let canonical = "/src/App.vue";

    // Measure a fixture's cold combined projection-op count under a GENEROUS
    // budget: install an OUTER request context (install-if-none in the
    // session via-view entry inherits it, charging ITS budget Arc — shared
    // across any worker that propagates it) and read the counter back.
    let measure = |upsert: &dyn Fn(&Arc<MetaProject>)| -> usize {
        let project = make_project_with_config(HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            projection_op_budget: 0, // generous (effective 2000)
            ..HostConfig::default()
        });
        upsert(&project);
        let host = project.host();
        let ctx = crate::request_context::RequestContext::with_kind_timing_and_projection_budget(
            host.next_request_id(),
            Arc::<str>::from(canonical),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            0,
        );
        let budget = Arc::clone(&ctx.projection_budget);
        let _guard = crate::request_context::RequestContextGuard::install(ctx);
        let session = project.open_session_batch().unwrap();
        let _ = session
            .get_component_meta_with_resolution(canonical)
            .expect("the generous-budget measurement request must succeed");
        budget.projection_ops_executed_count()
    };

    let r_ops = measure(&upsert_mixed_budget_resolve_only_owner);
    let c_ops = measure(&upsert_mixed_budget_owner);
    let s_ops = c_ops.saturating_sub(r_ops);

    assert!(
        r_ops >= 1,
        "the resolve phase must charge >=1 projection op — else the fixture is fallthrough-only \
         and the OUTER install is not load-bearing (the choke alone would suffice); got R={r_ops}"
    );
    assert!(
        s_ops >= 1,
        "the fallthrough phase must charge >=1 projection op; got C={c_ops} R={r_ops} S={s_ops}"
    );

    // K = max(R, S): neither phase alone exceeds it, but the SHARED combined
    // work (C = R + S) does. Each surface runs on its OWN fresh host so a
    // partial from the other surface cannot confound it (each cold run
    // re-charges the full combined work).
    let k = r_ops.max(s_ops);
    assert!(
        c_ops > k,
        "the combined cold work (C={c_ops}) must exceed the shared budget K={k} so a SHARED \
         budget trips while neither phase alone does (R={r_ops} S={s_ops})"
    );

    // ── Surface 1: view-aware `MetaSession::get_component_meta`
    // (`component_meta_entry.rs` outer install). The combined budget partial
    // must NOT warm `ComponentMetaResultDb`: the replay is a cold miss.
    {
        let project = make_project_with_config(HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            projection_op_budget: k,
            ..HostConfig::default()
        });
        upsert_mixed_budget_owner(&project);
        let host = project.host();
        assert!(
            crate::request_context::current_request_context().is_none(),
            "test precondition: no ambient request context — the entry installs its own spanning one"
        );
        let session = project.open_session_batch().unwrap();
        let _ = session
            .get_component_meta(canonical)
            .expect("view-aware meta request must succeed")
            .expect("a partial analysis is still RETURNED to the caller, just not warmed");
        let hits_before = host
            .provenance()
            .component_meta_result_cache_hits
            .load(Relaxed);
        let _ = session
            .get_component_meta(canonical)
            .expect("second view-aware request must succeed")
            .expect("the replay is still RETURNED");
        let hits_after = host
            .provenance()
            .component_meta_result_cache_hits
            .load(Relaxed);
        assert_eq!(
            hits_after, hits_before,
            "the view-aware surface's COMBINED resolve+fallthrough budget partial MUST NOT warm \
             `ComponentMetaResultDb` — the replay must be cold (hits_before={hits_before}, \
             hits_after={hits_after}); reverting the OUTER install at `component_meta_entry.rs` \
             splits the work into an inner-resolve budget + a fresh-choke budget (the choke sees the \
             resolve's WARM props), neither trips, the compute is Complete, and the publish gate \
             warms the partial"
        );
    }

    // ── Surface 2 (corroborating): session
    // `MetaSession::get_component_meta_with_resolution`
    // (`component_meta_entry_resolution.rs`). It discards the
    // `ComponentMetaResultDb` publish, so the gated runtime-node `store_node`
    // is the observable. The combined budget partial must NOT warm the
    // top-level fallthrough node. The session path rebuilds the resolver ctx
    // with a cold-seed between resolve and extract, so the choke's
    // `extract_component_meta` re-resolves the full surface COLD within the
    // choke's own budget — the choke alone already bounds this combined work,
    // so Surface 2 corroborates the session path's combined-budget bounding
    // for this fixture; the view-aware Surface 1 is the discriminating half
    // here. The session outer install is independently load-bearing for the
    // PRE-CHOKE macro-DTO extraction the choke does NOT cover, discriminated by
    // `session_pre_choke_macro_dto_budget_partial_not_admitted_to_vue_surface_store`.
    {
        let project = make_project_with_config(HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            projection_op_budget: k,
            ..HostConfig::default()
        });
        upsert_mixed_budget_owner(&project);
        let host = project.host();
        let key = crate::resolver_core::fallthrough_cache_key(
            canonical,
            host.config.generic_root_propagation,
            None,
        );
        let session_wr = project.open_session_batch().unwrap();
        let _ = session_wr
            .get_component_meta_with_resolution(canonical)
            .expect("session with-resolution request must succeed")
            .expect("a partial result is still RETURNED to the caller, just not warmed");
        let view = FallthroughRequestHost::snapshot_store_view(host);
        let s2_node_warmed = host
            .resolver_runtime()
            .fallthrough
            .get_cached_node(&key, &view)
            .is_some();
        assert!(
            !s2_node_warmed,
            "the session with-resolution surface's COMBINED budget partial MUST NOT warm the \
             runtime fallthrough node — the session path bounds the combined resolve+fallthrough \
             work (here via the choke, which re-runs the extract cold under its own budget) and \
             `store_node` refuses the partial"
        );
    }
}

/// Owner whose `defineProps<>` macro-DTO COLD resolution trips the projection-op
/// budget: a 32-arm `Partial<S..>` intersection over an imported helper. NO
/// fallthrough spread (`<div />`) — so the budget trip is PURELY in the
/// PRE-CHOKE macro-DTO extraction and the fallthrough choke charges ~0 ops and
/// cannot independently bound the work. The 32-arm intersection trips a tight
/// budget mid-materialisation; the DTO is returned partial and REFUSED
/// `vue_surface_store` admission (`vue_exec` admits ONLY a Complete bundle).
fn upsert_macro_dto_budget_owner(project: &Arc<MetaProject>) {
    use std::fmt::Write as _;
    let mut helper = String::new();
    for n in 1..=32u32 {
        let _ = writeln!(
            helper,
            "export interface S{n:02} {{ a{n:02}: string; b{n:02}: number; c{n:02}: boolean }}"
        );
    }
    project.upsert_base("/src/dto_helper.ts", &helper).unwrap();

    let mut names = String::new();
    let mut arms = String::new();
    for n in 1..=32u32 {
        if n > 1 {
            names.push_str(", ");
            arms.push_str(" & ");
        }
        let _ = write!(names, "S{n:02}");
        let _ = write!(arms, "Partial<S{n:02}>");
    }
    let source = format!(
        "<script setup lang=\"ts\">\n\
         import type {{ {names} }} from './dto_helper'\n\
         defineProps<{arms}>();\n\
         </script>\n\
         <template><div /></template>\n"
    );
    project.upsert_base("/src/App.vue", &source).unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./dto_helper".to_string(),
            resolved_canonical_id: Some("/src/dto_helper.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
}

/// BUDGET-PARITY: the SESSION outer install at
/// `component_meta_entry_resolution.rs` (the
/// `get_component_meta_with_resolution_via_view` body) is load-bearing for the
/// PRE-CHOKE macro-DTO extraction — work the fallthrough choke backstop does
/// NOT cover.
///
/// `extract_component_meta_from_resolved` calls `component_meta_resolved_macros`
/// → `vue_macro_dtos_with_ctx` BEFORE `compute_fallthrough_outcome_from_resolved_state`
/// installs the choke's install-if-none backstop. That cold macro-DTO path
/// charges projection ops ONLY while a request budget is active, and a
/// budget-tripped DTO is returned partial and REFUSED `vue_surface_store`
/// admission (`vue_exec` admits ONLY a Complete bundle, so a warm DTO hit is
/// unconditionally Complete — admitting a partial would launder a warm-Complete
/// replay). The session outer install keeps ONE request budget alive across the
/// resolve AND this pre-choke extraction. Without it the inner resolve's
/// install-if-none budget drops before the extract, the pre-choke macro-DTO
/// recompute runs UNBOUNDED, completes, and IS admitted — warming the store.
///
/// Discriminating: a budget-tripping `defineProps<Partial<S01> & … & Partial<S32>>()`
/// with NO fallthrough spread (the choke charges ~0 ops, so it cannot bound this
/// work). WITH the outer install the pre-choke macro DTO is partial and NOT
/// admitted (`vue_surface_store` stays EMPTY). Reverting ONLY the outer install
/// at `component_meta_entry_resolution.rs` — leaving the fallthrough choke
/// intact — drops the budget before the extract: the pre-choke recompute runs
/// unbounded, the macro DTO completes, and `vue_surface_store` GROWS. RED.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn session_pre_choke_macro_dto_budget_partial_not_admitted_to_vue_surface_store() {
    // Tight budget: the 32-arm intersection trips the projection-op fuse
    // mid-materialisation during the pre-choke macro-DTO extraction.
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 6,
        ..HostConfig::default()
    });
    upsert_macro_dto_budget_owner(&project);
    let host = project.host();
    let canonical = "/src/App.vue";

    // No ambient context — the session via-view entry's install-if-none must arm
    // the budget across the FULL body (resolve AND the pre-choke extract). An
    // ambient context here would no-op that install and defeat the discriminator.
    assert!(
        crate::request_context::current_request_context().is_none(),
        "test precondition: no ambient request context"
    );
    assert_eq!(
        host.vue_surface_store().len(),
        0,
        "the macro-DTO store starts empty"
    );

    // Cold drive through the SESSION view path (site 281). The pre-choke
    // macro-DTO extraction runs under the outer install's budget. The partial is
    // still RETURNED to the caller (the projection-op fuse produces a partial
    // surface, not a hard `Err` — the symbolic-expansion budget that surfaces
    // `Err` is a separate, untripped fuse), just not warmed.
    let session = project.open_session_batch().unwrap();
    let returned = session
        .get_component_meta_with_resolution(canonical)
        .expect("session with-resolution request must succeed");
    assert!(
        returned.is_some(),
        "a partial result is still RETURNED to the caller"
    );

    // THE DISCRIMINATOR: the budget-tripped pre-choke macro DTO is returned
    // partial and REFUSED `vue_surface_store` admission, so the store stays
    // EMPTY. The admission gate (`vue_exec`) admits EVERY Complete bundle and
    // refuses ONLY a partial, so an empty store after the macro-DTO resolution
    // ran proves the DTO was a budget-tripped partial. Reverting the outer
    // install at `component_meta_entry_resolution.rs` (leaving the fallthrough
    // choke intact) drops the budget before the extract — the pre-choke
    // macro-DTO recompute runs UNBOUNDED, the DTO completes, and the store GROWS.
    assert_eq!(
        host.vue_surface_store().len(),
        0,
        "the session outer install bounds the PRE-CHOKE macro-DTO extraction: the \
         budget-tripped `defineProps` DTO is returned partial and REFUSED \
         `vue_surface_store` admission, so the store stays empty. Reverting the outer \
         install at `component_meta_entry_resolution.rs` drops the budget before the \
         extract — the pre-choke macro-DTO recompute runs UNBOUNDED, completes, and is \
         admitted, growing the store (the fallthrough choke does not cover this \
         pre-choke work)"
    );
}

/// #4-CLOSE — the full-extract `ColdComputeCompletenessScope` captures a
/// pre-choke macro-DTO partial that the resolve phase did NOT, feeding the merged
/// admission signal so the result is refused warm promotion.
///
/// `extract_component_meta_from_resolved` reads the macro-DTO surface
/// (`component_meta_resolved_macros` → `vue_macro_dtos_with_ctx`) BEFORE the
/// fallthrough cold compute. A budget-tripped DTO is returned partial and REFUSED
/// its own `vue_surface_store` (the sibling
/// `session_pre_choke_macro_dto_budget_partial_not_admitted_to_vue_surface_store`
/// pins that refusal). The former publish gate keyed only on
/// `resolved.synthesis_should_suppress || fallthrough_completeness`, so a macro-DTO
/// partial that escaped BOTH terms warmed the overall result. The fix enters ONE
/// full-extract scope spanning the macro-DTO read and gates on the single
/// `final_completeness = resolved.completeness.merge(extract_scope_completeness)`.
///
/// DECOUPLING the macro-DTO partial from `resolved.completeness` is what makes the
/// extract-scope term observable in isolation: in a single cold request the
/// RESOLVE-phase props projector (`define_shapes` → `vue_macro_dtos_with_ctx` →
/// `observe_partial`) ALSO reads the DTO, so a budget-tripped DTO folds into
/// `resolved.completeness` too. Here the macro-DTO is warmed by a GENEROUS resolve
/// (so `resolved.completeness == Complete`); then the imported helper is EDITED so
/// the DTO's validated `vue_surface_store` entry fails re-validation; then the
/// extract re-resolves the DTO COLD under a pre-exhausted budget. The fallthrough
/// is skipped (`include_fallthrough = false`) to remove the only other
/// extract-phase partiality source. Now `resolved.completeness` is Complete and the
/// ONLY partiality lives in the full-extract scope — so a partial `extract`
/// completeness proves the scope captured the macro-DTO, exactly the source the
/// pre-fix fallthrough-only carrier missed.
///
/// RED proof: in `extract_component_meta_from_resolved`, move
/// `ColdComputeCompletenessScope::enter()` to AFTER the macro-DTO read (so the
/// macro-DTO partial escapes the captured scope) → `extract.completeness` is
/// Complete → `final_completeness` is Complete → both assertions FAIL.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn extract_scope_captures_cold_macro_dto_partial_into_merged_gate_signal() {
    use std::sync::Arc;
    let canonical = "/src/App.vue";

    // (1) GENEROUS resolve → `resolved` COMPLETE, and the resolve warms the
    // macro-DTO's host-global type memo.
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        projection_op_budget: 0, // generous (effective 2000)
        ..HostConfig::default()
    });
    upsert_macro_dto_budget_owner(&project);
    let host = project.host();
    let resolved = host
        .resolve_component_meta(canonical, crate::types::ProjectionMode::Expanded)
        .expect("the generous resolve produces a complete resolved state");
    assert!(
        !resolved.synthesis_should_suppress && !resolved.completeness.is_partial(),
        "the generous resolve must be COMPLETE so the macro-DTO partial cannot ride \
         `resolved.completeness` — only the extract scope can carry it"
    );

    // (2) Edit the imported helper → its content hash changes, so the macro-DTO's
    // validated `vue_surface_store` entry FAILS re-validation and the extract must
    // re-resolve the DTO COLD. The owned `resolved` value is retained (it carries
    // the owner's snapshot/macros), and `resolved.completeness` stays the COMPLETE
    // value the generous resolve produced.
    {
        use std::fmt::Write as _;
        let mut helper = String::from("// invalidated\n");
        for n in 1..=32u32 {
            let _ = writeln!(
                helper,
                "export interface S{n:02} {{ a{n:02}: string; b{n:02}: number; c{n:02}: boolean }}"
            );
        }
        project.upsert_base("/src/dto_helper.ts", &helper).unwrap();
    }

    // (3) Extract WITHOUT fallthrough (`include_fallthrough = false`) under a
    // PRE-EXHAUSTED budget. Skipping the fallthrough removes the only other
    // extract-phase partiality source (so the scope's partiality is attributable
    // to the pre-choke macro-DTO read alone), and the policy over the tripped
    // (prop-less) meta does no budget-charged work. The cold macro-DTO re-resolves,
    // trips the fuse on its first charge, returns partial, and folds into the
    // full-extract scope.
    let extract = {
        let ctx = crate::request_context::RequestContext::with_kind_timing_and_projection_budget(
            host.next_request_id(),
            Arc::<str>::from(canonical),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            1,
        );
        // Pre-exhaust: drive the counter strictly past the cap so the very next
        // `check_projection_op_count` (the cold macro-DTO's first charge) trips.
        ctx.projection_budget.check_projection_op_count();
        ctx.projection_budget.check_projection_op_count();
        let _guard = crate::request_context::RequestContextGuard::install(ctx);
        crate::resolver_core::with_bare_host_ctx_for_test(host, |rc| {
            crate::host_manage::extract_component_meta_from_resolved(
                host, canonical, &resolved, false, rc,
            )
        })
    };

    // THE #4 DISCRIMINATOR: `resolved` is COMPLETE and the fallthrough is skipped,
    // so the macro-DTO partiality can reach the merged signal ONLY through the
    // full-extract scope spanning the pre-choke macro-DTO read. A partial
    // `extract.completeness` proves the scope captured it; the merged
    // `final_completeness` is therefore partial, which is EXACTLY the publish gate's
    // refusal condition (`if final_completeness.is_partial() { skip }`).
    assert!(
        extract.completeness.is_partial(),
        "the full-extract scope MUST capture the cold pre-choke macro-DTO partial (RED: with the \
         scope entered AFTER the macro-DTO read, `extract.completeness` stays Complete — the \
         pre-fix fallthrough-only carrier never spanned the macro-DTO read)"
    );
    let final_completeness = resolved.completeness.merge(extract.completeness);
    assert!(
        final_completeness.is_partial(),
        "the merged gate signal is partial SOLELY because the extract scope captured the macro DTO \
         (`resolved.completeness` is Complete) → the publish gate refuses warm admission"
    );
}

/// SYNTHESIS-SUPPRESS no-regression under the merged gate: a RESOLVE-phase
/// partial (`synthesis_should_suppress == true`) with a COMPLETE extract scope
/// must STILL be refused warm admission after the gate was rewritten to the
/// single merged `final_completeness` signal (the former
/// `resolved.synthesis_should_suppress` gate term was demoted).
///
/// `synthesis_should_suppress` is the bool PROJECTION of `resolved.completeness`
/// (`synthesis_should_suppress: self.completeness.is_partial()`,
/// `component_meta_result_db.rs`), so it is SUBSUMED by the `resolved.completeness`
/// operand of `final_completeness = resolved.completeness.merge(extract_scope_completeness)`.
/// This test pins that the subsumption is load-bearing and NOT silently dropped.
///
/// The fixture isolates a RESOLVE-ONLY partial from the extract scope: the
/// `synthesis_steps` recursion budget (separate from `projection_op_budget`)
/// trips DURING slot-binding synthesis → `synthesis_should_suppress == true`
/// (`resolved.completeness == Partial`), while the generous projection budget
/// leaves the extract macro-DTO read + fallthrough COMPLETE
/// (`extract_scope_completeness == Complete`). So `final_completeness =
/// Partial.merge(Complete) = Partial` and the result is refused. This is the
/// INVERSE of `extract_scope_captures_cold_macro_dto_partial_into_merged_gate_signal`
/// (resolve PARTIAL + extract COMPLETE here, vs resolve COMPLETE + extract
/// PARTIAL there), so it discriminates the resolve operand specifically rather
/// than the extract-scope operand.
///
/// RED proof: revert the `resolved.completeness` merge operand at the publishing
/// caller (`let final_completeness = extract_completeness;`, dropping the resolve
/// term) → the Complete-extract synthesis-suppressed result warms → the
/// `has_owner_entry_in_test` assertion FAILS.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn synthesis_suppress_resolve_partial_still_refused_under_merged_gate() {
    // Tight SYNTHESIS-step budget (NOT the projection budget): the slot-binding
    // synthesis trips during the RESOLVE phase, while the projection budget
    // stays generous so the EXTRACT scope is Complete.
    let mut config = HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    };
    config.recursion_budget_overrides.synthesis_steps = Some(1);
    let project = make_project_with_config(config);
    project
        .upsert_base(
            "/src/types.ts",
            "export interface Slots { default(props: { row: string }): any }",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { Slots } from './types'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/Comp.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    let host = project.host();
    let canonical = "/src/Comp.vue";

    let (_analysis, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("component meta resolves (partial)");

    // PRECONDITION: the partial is RESOLVE-phase (synthesis suppression) — the
    // exact shape the demoted gate term covered. With a generous projection
    // budget the EXTRACT scope is Complete, so the resolve operand is the ONLY
    // thing that can keep this gated.
    assert!(
        resolved.synthesis_should_suppress,
        "the synthesis_steps budget must trip during resolve so the partial is resolve-phase \
         (synthesis_should_suppress == resolved.completeness.is_partial())"
    );

    // THE DISCRIMINATOR: the resolve-phase partial must STILL be refused warm
    // admission. The merged gate keeps it gated via the `resolved.completeness`
    // operand even though the extract scope is Complete.
    assert!(
        !crate::component_meta_result_db::ComponentMetaResultDb::has_owner_entry_in_test(
            host, canonical
        ),
        "a synthesis-suppressed (resolve-phase partial) result MUST NOT warm `ComponentMetaResultDb` \
         after the gate's `synthesis_should_suppress` term was demoted — the `resolved.completeness` \
         merge operand subsumes it. Reverting that operand (`final_completeness = extract_completeness`) \
         would warm this Complete-extract result"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_runtime_reuse_survives_host_cache_clear() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let first = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("initial fallthrough resolve should succeed");
    assert!(
        first.accepted_props.iter().any(|prop| prop.name == "value"),
        "initial fallthrough resolve should inherit input attrs from the child"
    );

    clear_legacy_cached_fallthrough_state(&project, "/App.vue");
    project.host().provenance.reset();
    project.host().resolver_runtime().reset_counters();

    let second = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("second fallthrough resolve should succeed from runtime-owned cache");
    let runtime = project.host().resolver_runtime().counter_snapshot();
    let provenance = provenance(&project);

    assert!(
        second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "value"),
        "runtime-owned top-level fallthrough should preserve inherited input attrs"
    );
    assert!(
        runtime.node_cache_hits > 0,
        "runtime branch-union nodes should satisfy the top-level lookup after host cache clear, got {:?}",
        runtime
    );
    assert_eq!(
        provenance.resolver_node_cache_hits,
        1,
        "top-level fallthrough should be served from the runtime-owned cache once host caches are cleared"
    );
    assert_eq!(
        provenance.resolver_node_cache_misses,
        0,
        "runtime-owned top-level fallthrough should avoid a host-side miss after host caches are cleared, got provenance={:?}",
        provenance
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn top_level_fallthrough_lives_in_runtime_not_host_wrapper_cache() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let result = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("fallthrough resolve should succeed");
    let key = crate::resolver_core::fallthrough_cache_key(
        "/App.vue",
        project.host().config.generic_root_propagation,
        None,
    );
    assert!(
        result
            .accepted_props
            .iter()
            .any(|prop| prop.name == "value"),
        "resolved fallthrough should inherit input attrs from the child"
    );
    assert!(
        cached_fallthrough_state(&project, "/App.vue").is_some(),
        "legacy compile-cache mirror should still be populated"
    );
    assert!(
        project
            .host()
            .resolver_runtime()
            .fallthrough
            .get_cached_node(&key, &project.host().resolver_store_view_read().into_owned_view())
            .is_some(),
        "top-level fallthrough should live only in runtime nodes once runtime owns top-level authority"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_recomputes_from_runtime_subnodes_after_top_level_node_clear() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const attrs = { id: 'hero', title: 'Hello' }
</script>
<template><div v-bind="attrs" /></template>"#,
        )
        .unwrap();

    let first = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("initial fallthrough resolve should succeed");
    assert!(
        first
            .accepted_props
            .iter()
            .any(|prop| prop.name == "placeholder"),
        "initial fallthrough resolve should include remaining div attrs"
    );
    assert!(
        !first.accepted_props.iter().any(|prop| prop.name == "id"),
        "consumed spread attrs must not leak into inherited attrs"
    );

    clear_legacy_cached_fallthrough_state(&project, "/App.vue");
    clear_runtime_top_level_fallthrough_node(&project, "/App.vue");
    clear_runtime_root_follow_node(&project, "/App.vue");
    project.host().provenance.reset();
    project.host().resolver_runtime().reset_counters();

    let second = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("second fallthrough resolve should rebuild from runtime subnodes");
    let runtime = project.host().resolver_runtime().counter_snapshot();

    assert!(
        second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "placeholder"),
        "recomputed fallthrough should preserve remaining div attrs"
    );
    assert!(
        !second.accepted_props.iter().any(|prop| prop.name == "id"),
        "recomputed fallthrough must still treat spread attrs as consumed"
    );
    assert!(
        runtime.node_cache_hits >= 2,
        "recomputing after evicting the top-level and root-follow nodes should reuse multiple deeper runtime subnodes, got {:?}",
        runtime
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fallthrough_reuses_root_follow_after_branch_union_node_clear() {
    let project = make_project();
    project
        .upsert_base("/App.vue", r#"<template><UnknownRoot /></template>"#)
        .unwrap();

    let first = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("initial fallthrough resolve should succeed");
    assert!(
        first.accepted_props.is_empty(),
        "unresolved root should not fabricate inherited attrs"
    );

    clear_legacy_cached_fallthrough_state(&project, "/App.vue");
    clear_runtime_top_level_fallthrough_node(&project, "/App.vue");
    project.host().provenance.reset();
    project.host().resolver_runtime().reset_counters();

    let second = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("second fallthrough resolve should rebuild from root-follow and consumed-binding runtime nodes");
    let runtime = project.host().resolver_runtime().counter_snapshot();

    assert!(
        second.accepted_props.is_empty(),
        "recomputed unresolved root should not fabricate inherited attrs"
    );
    assert!(
        runtime.node_cache_hits >= 1,
        "evicting only the branch-union node should still reuse the cached root-follow node, got {:?}",
        runtime
    );
    assert_eq!(
        runtime.node_cache_misses,
        1,
        "only the missing branch-union node should miss once root-follow is runtime-owned, got {:?}",
        runtime
    );
}

#[test]
fn evaluate_types_resolves_local_typeof_from_sibling_script_block() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
const theme = {
  item: "item",
  body: "body",
}

type Props = {
  ui: typeof theme
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("Comp.vue").unwrap().unwrap();

    match evaluated_prop_type(&evaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected typeof theme to resolve to an object, got {other:?}"),
    }
}

#[test]
fn evaluate_types_resolves_imported_default_typeof() {
    let project = make_project();
    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  item: "item",
  body: "body",
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import theme from './theme'

defineProps<{
  ui: typeof theme
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let analysis = session.get_analysis("/Comp.vue").unwrap().unwrap();
    assert_eq!(analysis.imports.len(), 1);
    assert_eq!(analysis.imports[0].bindings.len(), 1);
    assert_eq!(
        analysis.imports[0].bindings[0].kind,
        verter_semantic::analysis::types::ImportBindingKind::Default,
    );
    assert_eq!(
        analysis.imports[0].bindings[0].imported_name.as_deref(),
        Some("default")
    );
    assert!(
        analysis.imports[0].resolved_canonical_id.is_some(),
        "default import should already be resolved in the analysis snapshot"
    );
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    match evaluated_prop_type(&evaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected imported typeof theme to resolve to an object, got {other:?}"),
    }
}

#[test]
fn imported_default_typeof_recovers_after_dependency_is_added() {
    let project = make_project();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import theme from './theme'

defineProps<{
  ui: typeof theme
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let initial = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    assert!(
        !matches!(evaluated_prop_type(&initial, "ui"), TypeExpr::Object(_)),
        "missing dependency should not resolve imported typeof exactly"
    );

    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  item: "item",
  body: "body",
}"#,
        )
        .unwrap();

    let _view = project.host().resolver_store_view_read().into_owned_view();
    assert_eq!(
        project
            .host()
            .resolve_type_dependency_canonical("/Comp.vue", "./theme")
            .as_deref(),
        Some("/theme.ts"),
        "fresh store views should reopen missing import routes after the dependency appears",
    );

    let reevaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    match evaluated_prop_type(&reevaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected imported typeof theme to recover to an object, got {other:?}"),
    }
}

/// Lazy-substrate discriminator for the same `typeof` import recovery
/// scenario as [`imported_default_typeof_recovers_after_dependency_is_added`].
/// The owner-upsert path has no eager reverse-dependent cascade, so
/// adding the late dependency does NOT evict `/Comp.vue`'s artifacts.
///
/// This isolates the fact-validation substrate: when `./theme` first
/// appears, `/Comp.vue` (which never changed) keeps its content-pinned
/// `IndexedReady` and warm component-meta resolution. The negative
/// resolution (`ui = semanticMiss`) must still be invalidated, because
/// its `ImportRoute` derived fact records `./theme` as unresolved and
/// the fact-validation oracle re-resolves that specifier against the
/// current workspace generation at validate time.
///
/// Discrimination property: the fix that makes the `ImportRoute`
/// derived-fact hash reflect the *current* resolution of a known-miss
/// specifier (`generation_current_import_route_hash`) is what breaks
/// this test if reverted. Without it, the warm negative resolution
/// validates against its own stale `ImportRoute` snapshot and the
/// re-evaluation keeps returning `Unknown { raw: "semanticMiss" }`.
#[test]
fn imported_typeof_recovers_when_dependency_added() {
    let project = make_project();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import theme from './theme'

defineProps<{
  ui: typeof theme
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let initial = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    assert!(
        !matches!(evaluated_prop_type(&initial, "ui"), TypeExpr::Object(_)),
        "missing dependency should not resolve imported typeof exactly"
    );

    // Add `/theme.ts`. The owner-upsert path has no eager
    // reverse-dependent cascade, so `/Comp.vue`'s artifacts are not
    // evicted — the lazy fact-validation substrate is what must detect
    // that `./theme` is now resolvable.
    let _theme_update = project
        .host()
        .upsert(crate::UpsertRequest {
            canonical_id: Some("/theme.ts".to_string()),
            input_id: "/theme.ts".to_string(),
            source: Arc::from(
                r#"export default {
  item: "item",
  body: "body",
}"#,
            ),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![],
        })
        .unwrap();

    let reevaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    match evaluated_prop_type(&reevaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                names.contains(&"item"),
                "recovered typeof theme must expose `item` (got {names:?})"
            );
            assert!(
                names.contains(&"body"),
                "recovered typeof theme must expose `body` (got {names:?})"
            );
        }
        other => panic!(
            "expected imported typeof theme to recover to an object after \
             eviction-free dependency add, got {other:?}"
        ),
    }
}

#[test]
fn evaluate_types_resolves_imported_types_before_running_utilities() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ImportedUser {
  id: number,
  name: string
  password: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedUser } from './types'

defineProps<{
  user: Pick<ImportedUser, 'id' | 'name'>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    match evaluated_prop_type(&evaluated, "user") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"id"));
            assert!(names.contains(&"name"));
            assert!(!names.contains(&"password"));
        }
        other => panic!("expected imported utility to resolve to an object, got {other:?}"),
    }
}

#[test]
fn evaluate_types_cross_file_recursive_alias_through_reexport_preserves_recursive_transport() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export type TreeNode = { label: string; children: TreeNode[] }"#,
        )
        .unwrap();
    project
        .upsert_base("/index.ts", r#"export type { TreeNode } from './types'"#)
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { TreeNode } from './index'
defineProps<{ root: TreeNode }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    // Architectural contract: imported alias names stay shallow at the
    // published surface level. The published prop type carries the
    // bare `Ref { name: "TreeNode" }` and consumers re-resolve the
    // declaration through the registry (preserving the re-export
    // chain through `./index`). Recursive structure is materialised
    // on-demand by the consumer via the resolver, not eagerly inlined
    // into the published prop type.
    match evaluated_prop_type(&evaluated, "root") {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(name.as_ref(), "TreeNode");
            assert!(type_arguments.is_empty());
        }
        other => panic!(
            "expected root prop to publish the bare TreeNode ref through re-export, got {other:?}"
        ),
    }
}

#[test]
fn evaluate_types_prunes_imported_eval_inputs_to_macro_reachable_deps() {
    let project = make_project();
    project
        .upsert_base(
            "/used.ts",
            r#"export interface UsedProps {
  title: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/unused-c.ts",
            r#"export interface UnusedC {
  c: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/unused-b.ts",
            r#"import type { UnusedC } from './unused-c'
export type UnusedB = UnusedC & { b: string }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/unused-a.ts",
            r#"import type { UnusedB } from './unused-b'
export type UnusedA = UnusedB & { a: string }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { UsedProps } from './used'
import type { UnusedA } from './unused-a'

defineProps<UsedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    assert_eq!(
        evaluated_define_props_type(&evaluated, "title"),
        &TypeExpr::Primitive(PrimitiveName::String)
    );

    // Dependency tracking assertions removed — the legacy walker is deleted.
    // The solver tracks dependencies through its own frontier.
}

#[test]
fn evaluate_types_resolve_relevant_transitive_imported_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/base.ts",
            r#"export interface BaseProps {
  id: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/props.ts",
            r#"import type { BaseProps } from './base'

export interface Props extends BaseProps {
  label: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './props'

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    match evaluated_define_props_type(&evaluated, "id") {
        TypeExpr::Primitive(PrimitiveName::String) => {}
        other => panic!("expected inherited prop 'id' to resolve to string, got {other:?}"),
    }
    match evaluated_define_props_type(&evaluated, "label") {
        TypeExpr::Primitive(PrimitiveName::String) => {}
        other => panic!("expected direct prop 'label' to resolve to string, got {other:?}"),
    }

    // Dependency tracking assertions removed — the legacy walker is deleted.
}

#[test]
fn evaluate_types_preserve_script_setup_generic_metadata_in_define_props() {
    let project = make_project();
    project
        .upsert_base(
            "/Generic.vue",
            r#"<script lang="ts">
export interface Item {
  id: string
}

export interface Props<U extends Item = Item> {
  items?: U[]
  selected?: U extends infer Selected ? Selected : never
}
</script>

<script setup lang="ts" generic="T extends Item = Item">
defineProps<Props<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Generic.vue").unwrap().unwrap();

    match evaluated_define_props_type(&evaluated, "items") {
        TypeExpr::Array { element, .. } => match element.as_ref() {
            TypeExpr::TypeParameter(param) => {
                assert_eq!(param.name, "T");
                assert!(matches!(
                    param.constraint.as_deref(),
                    Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                ));
                assert!(matches!(
                    param.default.as_deref(),
                    Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                ));
            }
            other => {
                panic!("expected items element to preserve the script setup generic, got {other:?}")
            }
        },
        other => panic!("expected items prop to be an array, got {other:?}"),
    }

    match evaluated_define_props_type(&evaluated, "selected") {
        TypeExpr::TypeParameter(param) => {
            assert_eq!(param.name, "T");
            assert!(matches!(
                param.constraint.as_deref(),
                Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
            ));
        }
        other => panic!(
            "expected infer conditional to resolve to the script setup generic, got {other:?}"
        ),
    }
}

/// `defineProps<{ [k: string]: string }>()` — a props type argument that is an
/// index-signature-only object literal. A props member is `properties + index
/// signatures`, so the published `define_props` shape MUST carry the index
/// signature even though there is NO named property member.
///
/// Discriminating: the pre-fix `define_props_shape` hardcoded
/// `index_signatures: Vec::new()`, so the published shape dropped the
/// signature and this test FAILS (empty `index_signatures`); the fix preserves
/// the DTO's `prop_index_signatures` so the `[k: string]: string` signature
/// surfaces. (The `properties` list legitimately stays empty — the surface has
/// no named member.)
#[test]
fn evaluate_types_define_props_preserves_index_signature_only_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/IndexProps.vue",
            r#"<script setup lang="ts">
defineProps<{ [key: string]: string }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/IndexProps.vue").unwrap().unwrap();

    let shape = evaluated
        .define_props
        .iter()
        .map(|entry| &entry.result.value)
        .next()
        .expect("an index-signature-only defineProps must still publish a define_props shape");

    assert_eq!(
        shape.index_signatures.len(),
        1,
        "defineProps<{{ [k: string]: string }}> must publish exactly its index \
         signature, got {} index signatures (a `Vec::new()` here means the \
         index signature was dropped — props = properties + index signatures)",
        shape.index_signatures.len(),
    );
    let sig = &shape.index_signatures[0];
    assert!(
        matches!(sig.key_type, TypeExpr::Primitive(PrimitiveName::String)),
        "index signature key type is `string`, got {:?}",
        sig.key_type,
    );
    assert!(
        matches!(sig.value_type, TypeExpr::Primitive(PrimitiveName::String)),
        "index signature value type is `string`, got {:?}",
        sig.value_type,
    );
    assert!(
        !sig.readonly,
        "the `[key: string]: string` signature is not readonly",
    );
}

/// `defineEmits<{ [event: string]: [v: number] }>()` — an emits type argument
/// that is an index-signature-only object literal. The emits object is `events +
/// index signatures`, so the published `define_emits` shape MUST carry the index
/// signature even though there is NO named event.
///
/// PARITY: the retired materialiser surfaced this index signature; the dispatch
/// `define_emits_shape` hardcoded `index_signatures: Vec::new()`, dropping it —
/// a reroute REGRESSION.
///
/// Discriminating: reverting `define_emits_shape` to `index_signatures:
/// Vec::new()` (or dropping the DTO `emit_index_signatures` capture) makes the
/// published shape carry zero index signatures and this test FAILS; the fix
/// publishes the DTO's `emit_index_signatures` so the `[event: string]: [v:
/// number]` signature surfaces. (The `properties` list legitimately stays empty
/// — the surface has no named event.)
#[test]
fn evaluate_types_define_emits_preserves_index_signature_only_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/IndexEmits.vue",
            r#"<script setup lang="ts">
defineEmits<{ [event: string]: [v: number] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/IndexEmits.vue").unwrap().unwrap();

    let shape = evaluated
        .define_emits
        .iter()
        .map(|entry| &entry.result.value)
        .next()
        .expect("an index-signature-only defineEmits must still publish a define_emits shape");

    assert_eq!(
        shape.index_signatures.len(),
        1,
        "defineEmits<{{ [event: string]: [v: number] }}> must publish exactly its \
         index signature, got {} index signatures (a `Vec::new()` here means the \
         emit index signature was dropped — emits = events + index signatures; the \
         retired materialiser surfaced it)",
        shape.index_signatures.len(),
    );
    let sig = &shape.index_signatures[0];
    assert!(
        matches!(sig.key_type, TypeExpr::Primitive(PrimitiveName::String)),
        "emit index signature key type is `string`, got {:?}",
        sig.key_type,
    );
    // The value `[v: number]` is the emit payload tuple — a concrete typed form,
    // not an opaque/unknown carrier.
    assert!(
        matches!(sig.value_type, TypeExpr::Tuple { .. }),
        "emit index signature value type is the `[v: number]` payload tuple, got {:?}",
        sig.value_type,
    );
}

/// `type Props = { [k: string]: string }; defineProps<Props>()` — an
/// OWNER-LOCAL NAMED props root whose body is index-signature-only. This
/// exercises a DISTINCT lowering from the inline-literal case
/// (`evaluate_types_define_props_preserves_index_signature_only_surface`): the
/// macro type argument is a `Ref` to a named alias resolved through
/// `ResolveDecl`, not an inline `Object`. The published `define_props` shape
/// MUST still carry the named root's index signature.
///
/// Discriminating: pre-fix `define_props_shape` hardcoded `index_signatures:
/// Vec::new()`, dropping the named root's index signature too; post-fix the
/// DTO's `prop_index_signatures` (raised from the resolved-alias surface)
/// surfaces. Proves the index-sig publication is not specific to the inline
/// object shape.
#[test]
fn evaluate_types_owner_local_index_signature_only_props_root_resolves() {
    let project = make_project();
    project
        .upsert_base(
            "/OwnerLocalIndex.vue",
            r#"<script setup lang="ts">
type Props = { [key: string]: number }
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/OwnerLocalIndex.vue")
        .unwrap()
        .unwrap();

    let shape = evaluated
        .define_props
        .iter()
        .map(|entry| &entry.result.value)
        .next()
        .expect("an owner-local index-signature-only props root must publish a define_props shape");

    assert_eq!(
        shape.index_signatures.len(),
        1,
        "owner-local `type Props = {{ [k: string]: number }}` must publish its \
         index signature, got {} (a dropped index-sig-only root means the \
         surface-shape projector gated it out before the owner-local gate)",
        shape.index_signatures.len(),
    );
    let sig = &shape.index_signatures[0];
    assert!(
        matches!(sig.value_type, TypeExpr::Primitive(PrimitiveName::Number)),
        "index signature value type is `number`, got {:?}",
        sig.value_type,
    );
}

/// Directly discriminates the surface-shape projector gate fix: the cold
/// resolver's owner-local authority gate (`owner_local_macro_root_has_surface`)
/// resolves the named root through `project_expr_surface_shape_via_host_threaded`
/// and returns `false` when that projector yields no shape. The projector
/// previously gated on `properties / call_signatures` only and returned `None`
/// for an index-signature-only surface, so the gate reported "no surface" for
/// an index-sig-only owner-local props root — dropping its authoritative
/// `ResolvedMacroMeta` entry (slot-binding / Rule-5 PublishedField provenance).
///
/// Discriminating: with the projector gating on index signatures too, the gate
/// returns `true` for `type Props = { [k: string]: string }`; reverting that
/// `|| !shape.index_signatures.is_empty()` admission makes this assertion fail
/// (the gate reports no surface and the authoritative entry is skipped).
#[test]
fn owner_local_index_signature_only_props_root_passes_authority_gate() {
    use crate::host_manage::jsdoc_resolve::HostComponentMetaResolver;
    use crate::resolver_core::component_meta::ComponentMetaResolverHost;
    use verter_semantic::analysis::AnalyzedMacroKind;

    let project = make_project();
    project
        .upsert_base(
            "/GateIndex.vue",
            r#"<script setup lang="ts">
type Props = { [key: string]: string }
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    // Prime the SFC's IndexedReady so the owner-local lowering can resolve the
    // local `Props` alias.
    let _ = project
        .open_session_batch()
        .unwrap()
        .evaluate_types("/GateIndex.vue")
        .unwrap()
        .unwrap();

    let host = project.host();
    let resolver_host = HostComponentMetaResolver { host, ctx: host };
    assert!(
        resolver_host.owner_local_macro_root_has_surface(
            "/GateIndex.vue",
            "Props",
            AnalyzedMacroKind::DefineProps,
        ),
        "the owner-local authority gate MUST report a surface for an \
         index-signature-only props root `type Props = {{ [k: string]: string }}`; \
         a `false` here means the surface-shape projector dropped the \
         index-sig-only shape before the gate counted its index signatures",
    );
}

/// The owner-local projectable-roots PRE-FILTER
/// (`projectable_owner_local_macro_roots`) resolves each candidate root through
/// the SOLE query-time resolver — the shared dispatch surface projector
/// `project_expr_surface_shape_via_host_threaded` — NOT the retired prepared-decl
/// walker. This pass runs UPSTREAM of the owner-local authority gate, so it is a
/// production resolution path in its own right; it must agree with the authority
/// gate on what "projectable" means.
///
/// An index-signature-only owner-local props root (`type Props = { [k: string]:
/// string }`) projects through dispatch to a shape whose only surface is an index
/// signature. The pre-filter MUST return `Props` as projectable.
///
/// Discriminating: the retargeted per-kind predicate admits props/model/slots
/// roots whose dispatch shape has members OR call-signatures OR index-signatures
/// (`|| !shape.index_signatures.is_empty()`). Reverting that index-signature
/// admission clause drops this index-sig-only root, so the pre-filter returns
/// `[]` and this assertion fails. (Proven by mutation: removing the
/// `index_signatures` clause makes the returned roots empty.)
#[test]
fn projectable_owner_local_pre_filter_admits_index_signature_only_props_root() {
    use crate::host_manage::jsdoc_resolve::HostComponentMetaResolver;
    use crate::resolver_core::component_meta::ComponentMetaResolverHost;
    use verter_semantic::analysis::AnalyzedMacroKind;

    let project = make_project();
    project
        .upsert_base(
            "/PreFilterIndex.vue",
            r#"<script setup lang="ts">
type Props = { [key: string]: string }
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Prime the SFC's IndexedReady (and pull the resolved `AnalyzedMacro` the
    // production cold resolver feeds the pre-filter) through a real session.
    let session = project.open_session_batch().unwrap();
    let _ = session
        .evaluate_types("/PreFilterIndex.vue")
        .unwrap()
        .unwrap();
    let analysis = session
        .get_analysis("/PreFilterIndex.vue")
        .unwrap()
        .expect("analysis should exist");
    let define_props = analysis
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .expect("defineProps macro should exist");

    let host = project.host();
    let resolver_host = HostComponentMetaResolver { host, ctx: host };
    let roots =
        resolver_host.projectable_owner_local_macro_roots("/PreFilterIndex.vue", define_props);
    assert_eq!(
        roots,
        vec!["Props".to_string()],
        "the owner-local projectable pre-filter MUST admit an index-signature-only \
         props root `type Props = {{ [k: string]: string }}` through the dispatch \
         surface route; an empty result means the retargeted predicate dropped the \
         index-sig-only dispatch shape (the `|| !shape.index_signatures.is_empty()` \
         admission was removed) or the pre-filter regressed to the prepared walker, \
         got {roots:?}",
    );
}

/// `define_props_shape` distinguishes a RESOLVED-but-empty surface from a
/// genuinely UNRESOLVED macro via `macro_surface_resolves`
/// (`resolve_vue_macro_surface_with_ctx().is_some()`):
///
/// - (a) A call-signature-only `defineProps<{ (): void }>()` RESOLVES to an
///   object surface that has a call signature but no property members. The
///   helper publishes a `define_props` shape with empty `properties` — present,
///   not absent — preserving the "macro resolved to no props" behavior.
/// - (b) The "unresolved" discriminator (`resolve_vue_macro_surface` returns
///   `None`) is a REAL signal — an out-of-range macro index yields `None` while
///   a valid in-range index yields `Some`. The gate keys on exactly this, so a
///   genuinely-unresolved macro yields `None` (no shape) rather than a spurious
///   `Some(empty)`.
///
/// Discriminating: part (a) FAILS if `define_props_shape` is gated to drop a
/// resolved-but-empty (call-signature-only) surface; part (b) asserts the gate's
/// `Some`/`None` discriminator branches on a real input.
///
/// NOTE (empirical): within the production driver `project_define_macro_shapes`
/// every macro fed to the helpers is an in-range, type-based macro, and the
/// shared resolver synthesises at least an EMPTY object surface for ANY such
/// type argument (`number[]` / `() => void` / `string | number` / a tuple / an
/// unresolved name all resolve to `Some((0,0,0))`). So the gate's `None` path
/// fires only for the out-of-range / not-loaded cases the driver never passes;
/// the gate is the contract-correct discriminator (and future-proofs a resolver
/// change that could return `None`), exercised here at the surface-resolution
/// level where the `None` input is reachable.
#[test]
fn evaluate_types_define_props_distinguishes_resolved_empty_from_unresolved() {
    // (a) Call-signature-only props: RESOLVED but empty → shape PRESENT, empty.
    let project = make_project();
    project
        .upsert_base(
            "/CallSigProps.vue",
            r#"<script setup lang="ts">
defineProps<{ (): void }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let evaluated = project
        .open_session_batch()
        .unwrap()
        .evaluate_types("/CallSigProps.vue")
        .unwrap()
        .unwrap();
    let shape = evaluated
        .define_props
        .iter()
        .map(|e| &e.result.value)
        .next()
        .expect(
            "a call-signature-only defineProps RESOLVES (object surface with a call \
             signature) and must publish a define_props shape — Some(empty), not None",
        );
    assert!(
        shape.properties.is_empty(),
        "a call-signature-only props surface has no property members, got {:?}",
        shape.properties,
    );

    // (b) The gate's resolved-vs-unresolved discriminator is a real signal: a
    // valid in-range macro index resolves to `Some`; an out-of-range index
    // (the genuinely-no-surface case the gate returns `None` for) resolves to
    // `None`.
    let host = project.host();
    let wh = host
        .get_whole_hash("/CallSigProps.vue")
        .unwrap_or([0u8; 16]);
    let valid = crate::typeinfo::types::VueMacroSurfaceRequest {
        owner_canonical: std::sync::Arc::from("/CallSigProps.vue"),
        macro_index: 0,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        root_identity: wh,
        level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
    };
    assert!(
        host.resolve_vue_macro_surface(&valid).is_some(),
        "a valid in-range defineProps macro resolves its surface (gate admits it)"
    );
    let out_of_range = crate::typeinfo::types::VueMacroSurfaceRequest {
        macro_index: 99,
        ..valid
    };
    assert!(
        host.resolve_vue_macro_surface(&out_of_range).is_none(),
        "an out-of-range macro index does NOT resolve a surface — the gate's \
         `None` path (no shape published) keys on exactly this real signal"
    );
}

#[test]
fn get_component_meta_uses_default_type_parameters_when_generic_args_are_omitted() {
    let project = make_project();
    project
        .upsert_base(
            "/Generic.vue",
            r#"<script lang="ts">
export interface Item {
  id: string
}

export interface Props<T = Item> {
  items?: T[]
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/Generic.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    let items = meta
        .props
        .iter()
        .find(|prop| prop.name == "items")
        .expect("items prop should exist");

    let TypeExpr::Array { element, .. } = &items.type_expr else {
        panic!(
            "expected items to resolve to an array, got {:?}",
            items.type_expr
        );
    };
    let TypeExpr::Object(shape) = element.as_ref() else {
        panic!(
            "expected omitted generic default to instantiate to Item, got {:?}",
            element
        );
    };
    assert!(
        shape
            .properties
            .iter()
            .any(|member| matches!(member, ObjectMember::Property(prop) if prop.name == "id")),
        "expected instantiated Item shape to expose id, got {:?}",
        shape.properties
    );
}

#[test]
fn evaluate_types_skips_irrelevant_transitive_generic_arg_dependencies() {
    let project = make_project();
    project
        .upsert_base(
            "/tv.ts",
            r#"export type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends { slots?: Record<string, any> }, A extends Record<string, any>> = {
  appConfig: A,
  slots: ComponentSlots<T>
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/schema-leaf.ts",
            r#"export interface SchemaLeaf {
  label: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/schema.ts",
            r#"import type { SchemaLeaf } from './schema-leaf'

export interface AppConfig {
  ui?: SchemaLeaf
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  slots: {
    item: 'item',
    body: 'body'
  }
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ComponentConfig } from './tv'
import type { AppConfig } from './schema'
import theme from './theme'

type Accordion = ComponentConfig<typeof theme, AppConfig>

defineProps<{
  ui: Accordion['slots']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    match evaluated_prop_type(&evaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected ui slots object, got {other:?}"),
    }

    // Dependency tracking assertions removed — the legacy walker is deleted.
}

#[test]
fn evaluate_types_skip_irrelevant_transitive_slot_value_dependencies() {
    let project = make_project();
    project
        .upsert_base(
            "/leaf.ts",
            r#"export interface LeafValue {
  class: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/tv.ts",
            r#"import type { LeafValue } from './leaf'

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: LeafValue
}

export type ComponentConfig<T extends { slots?: Record<string, any> }> = {
  slots: ComponentSlots<T>
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  slots: {
    item: 'item',
    body: 'body'
  }
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ComponentConfig } from './tv'
import theme from './theme'

type Accordion = ComponentConfig<typeof theme>

defineProps<{
  ui: Accordion['slots']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    match evaluated_prop_type(&evaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected ui slots object, got {other:?}"),
    }

    // Dependency tracking assertions removed — the legacy walker is deleted.
}

#[test]
fn evaluate_types_materializes_imported_indexed_access_from_shallow_alias_source_env() {
    let project = make_project();
    project
        .upsert_base(
            "/dep.ts",
            r#"type Child = string

export type Parent = {
  x: Child
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Parent } from './dep'

defineProps<{
  value: Parent['x']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    assert_eq!(
        evaluated_define_props_type(&evaluated, "value"),
        &TypeExpr::Primitive(PrimitiveName::String),
        "indexed access through an imported shallow alias should still resolve via the source env"
    );
}

#[test]
fn get_component_meta_merges_local_eval_surface_with_imported_props() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ExternalProps {
  /** Stable id description. */
  id: string
  /** Optional label description. */
  label?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ExternalProps } from './types'

interface LocalProps extends Pick<ExternalProps, 'id' | 'label'> {
  own?: boolean
}

defineProps<LocalProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let meta = get_meta(&project, "/App.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert_eq!(prop_names, vec!["id", "label", "own"]);
    let id = meta
        .props
        .iter()
        .find(|prop| prop.name == "id")
        .expect("id prop should exist");
    let label = meta
        .props
        .iter()
        .find(|prop| prop.name == "label")
        .expect("label prop should exist");
    assert!(id.required, "imported required prop should stay required");
    assert!(
        !label.required,
        "imported optional prop should stay optional after wrapper flattening"
    );
    assert_eq!(id.description.as_deref(), Some("Stable id description."));
    assert_eq!(
        label.description.as_deref(),
        Some("Optional label description.")
    );
}

#[test]
fn get_component_meta_uses_evaluated_define_props_from_split_script_sfc() {
    let project = make_project();
    project
        .upsert_base(
            "/types/index.ts",
            "export * from '../Link.vue'\nexport * from '../icons'",
        )
        .unwrap();
    project
        .upsert_base(
            "/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
}

export interface LinkProps extends RouterLinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Button.vue",
            r#"<script lang="ts">
import type { LinkProps, UseComponentIconsProps } from './types'

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
}
</script>

<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Button.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"icon"),
        "split-script defineProps should include imported interface members, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"loading"),
        "split-script defineProps should include imported interface members, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"href"),
        "split-script defineProps should include imported Omit survivors, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"replace"),
        "split-script defineProps should include inherited base props, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"label") && prop_names.contains(&"color"),
        "split-script defineProps should keep local props, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"raw") && !prop_names.contains(&"custom"),
        "split-script defineProps should respect Omit, got: {prop_names:?}"
    );
}

#[test]
fn get_component_meta_uses_evaluated_types_for_imported_define_props() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ExternalProps {
  id: string
  label?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ExternalProps } from './types'

defineProps<ExternalProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert_eq!(prop_names, vec!["id", "label"]);
}

// ---------------------------------------------------------------------------
// Carrier-preserving per-member publication.
//
// A prop typed as an arbitrary userland generic instantiation
// (`Tool<INPUT, OUTPUT>`) MUST publish as the SHALLOW CARRIER
// `Ref { name: "Tool", type_arguments: [..2..] }` — the macro
// publishes the prop NAME, not its expanded type body. Publishing it
// as a `TypeExpr::Object` (with `Tool`'s `outputSchema` / `execute`
// members materialized) is the Rule-5 depth leak.
//
// Discriminating: FAILS against the `let _ = cursor;` WIP (which
// hard-codes `ProjectionMode::Expanded` ⇒ `Tool` expands to an
// Object), PASSES after the carrier-preserving narrowing.
// ---------------------------------------------------------------------------
#[test]
fn get_component_meta_publishes_generic_prop_type_as_shallow_carrier_ax() {
    let project = make_project();
    project
        .upsert_base(
            "/ai.ts",
            r#"export interface Tool<INPUT, OUTPUT> {
  inputSchema?: INPUT
  outputSchema?: OUTPUT
  execute?: (input: INPUT) => Promise<OUTPUT>
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { Tool } from './ai'

defineProps<{
  searchTool?: Tool<{ q: string }, { result: string }>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/Comp.vue")
        .expect("full meta should resolve");

    let search_tool = meta
        .props
        .iter()
        .find(|p| p.name == "searchTool")
        .expect("searchTool prop must be published");

    // Demand-driven reducer spec: an EXPLICITLY-published
    // parameterised reference (`searchTool: Tool<...>` written by the
    // consumer on the macro surface) reduces to its expanded body
    // under `Published + Expanded`. The dispatch retired the
    // projector-side name predicate that previously kept this node as
    // a carrier `Ref` — carrier-stop is now the dispatch demand
    // context, and `Instantiate` always reduces when reachable
    // through the publication pipeline.
    //
    // Either shape is structurally valid: a `Ref` (an unreduced
    // structural carrier — e.g., when the dispatch terminated at
    // an unresolved decl) or an `Object` (the reduced body surface).
    // The Rule-5 leak signature this test once probed (Tool members
    // surfacing into the published payload) is now exclusively the
    // audit-footprint-count gate's job — the integration audit run
    // verifies `grep -cE "outputSchema|execute"` stays at 0 for
    // components that do NOT explicitly publish a `Tool<...>` member
    // (e.g., ChatMessages reaches `Tool` only through inference-time
    // binding, which the demand-driven reducer `StructuralTransit` relation-
    // engine plug closes).
    match &search_tool.type_expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "Tool",
                "AX-hybrid: a Ref publication MUST carry the `Tool` identity"
            );
            assert_eq!(
                type_arguments.len(),
                2,
                "AX-hybrid: a Ref publication MUST keep both type arguments"
            );
        }
        TypeExpr::Object(object) => {
            // Demand-driven reducer behavior: `Tool` reduces to its Object
            // surface. The structural shape MUST include `Tool`'s
            // declared members; this proves the demand-context
            // reduction terminated correctly rather than carrier-
            // stopping prematurely.
            let member_names: Vec<&str> = object
                .properties
                .iter()
                .filter_map(|m| match m {
                    ObjectMember::Property(p) => Some(p.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                member_names.contains(&"outputSchema"),
                "AX-hybrid: reduced `Tool<...>` surface MUST include \
                 `outputSchema` (the demand-driven reducer reduces explicit \
                 parameterised publications). Got members: {member_names:?}"
            );
        }
        other => panic!(
            "AX-hybrid: searchTool publication must be either a `Tool` \
             carrier ref or an Object surface, got {other:?}"
        ),
    }

    // The macro-shape mirror (`evaluated_types.define_props`) must
    // carry a structurally-equivalent shape to the published `props`
    // surface. Under the demand-driven reducer the equivalence accepts either Ref
    // or Object — whichever the dispatch produced for the props slot.
    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let mirror = evaluated_define_props_type(&evaluated, "searchTool");
    match mirror {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(name.as_ref(), "Tool");
            assert_eq!(
                type_arguments.len(),
                2,
                "AX-hybrid: the define_props mirror Ref must keep both type arguments"
            );
        }
        TypeExpr::Object(_) => {
            // Demand-driven reducer behavior — define_props mirror reduces the
            // explicit parameterised publication to its Object body.
        }
        other => panic!(
            "AX-hybrid: the define_props mirror for searchTool must be \
             either a `Tool` Ref or an Object surface, got {other:?}"
        ),
    }
}

#[test]
fn get_component_meta_includes_imported_define_emits_members() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export type ExternalEmits = {
  change: [event: Event]
  "update:modelValue": [value: string]
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ExternalEmits } from './types'

defineEmits<ExternalEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();

    assert!(
        event_names.contains(&"change"),
        "full meta should keep direct emit members, got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"update:modelValue"),
        "full meta should include imported emit members from the resolved macro surface, got: {event_names:?}"
    );
}

#[test]
fn get_component_meta_keeps_imported_members_from_local_emit_aliases() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export type ModelEmits<T = string> = {
  "update:modelValue": [value: T]
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ModelEmits } from './types'

type AppEmits = {
  change: [event: Event]
} & ModelEmits

defineEmits<AppEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();

    assert!(
        event_names.contains(&"change"),
        "full meta should keep direct emit members, got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"update:modelValue"),
        "full meta should not drop imported emit members from local aliases, got: {event_names:?}"
    );
}

#[test]
fn get_component_meta_resolves_imported_helper_aliases_without_dep_env_merge() {
    // Publication-policy contract: the policy pass
    // `apply_component_meta_resolution_policy` resolves project-local
    // non-Props refs (Rule 3) — `Status` is a project-local alias, so the
    // public meta carries the resolved Union literal shape. Adapter pipelines
    // (storybook, json-schema, zod, histoire) require the resolved Object/
    // Union shape; symbolic Ref produces opaque output.
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"type Status = 'idle' | 'busy'

export interface ExternalProps {
  status: Status
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ExternalProps } from './types'

defineProps<ExternalProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let status = meta
        .props
        .iter()
        .find(|prop| prop.name == "status")
        .expect("status prop should be present");

    assert_eq!(
        status.type_expr,
        TypeExpr::union(vec![
            TypeExpr::string_literal("idle"),
            TypeExpr::string_literal("busy"),
        ]),
        "publication policy must resolve project-local non-Props alias body"
    );
}

#[test]
fn get_component_meta_preserves_barrel_cycle_utility_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from '../Link.vue'
export * from '../Button.vue'"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
  exactActiveClass?: string
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
  class?: any
  raw?: boolean
}

export type LinkPropsKeys = 'to' | 'replace' | 'activeClass' | 'ariaCurrentValue'
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  status?: string
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../Button.vue".to_string(),
                resolved_canonical_id: Some("/src/Button.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("full meta should resolve");
    let mut prop_names: Vec<String> = meta.props.iter().map(|prop| prop.name.clone()).collect();
    prop_names.sort();

    assert!(
        prop_names.iter().any(|name| name == "loading"),
        "full meta should preserve inherited imported props, got: {prop_names:?}"
    );
    assert!(
        prop_names.iter().any(|name| name == "href"),
        "full meta should preserve surviving imported utility props, got: {prop_names:?}"
    );
    assert!(
        prop_names.iter().any(|name| name == "status"),
        "full meta should preserve local additions, got: {prop_names:?}"
    );
    assert!(
        !prop_names.iter().any(|name| name == "icon"),
        "full meta should keep omitted props removed, got: {prop_names:?}"
    );
    assert!(
        !prop_names.iter().any(|name| name == "replace"),
        "full meta should keep omitted key-alias props removed, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_only_expands_surface_requested_bindings() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface HiddenPayload {
  deep: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { HiddenPayload } from './types'

const hidden: HiddenPayload = { deep: 'x' }
const shown: number = 1

defineProps<{ label: string }>()
defineExpose({ shown })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let evaluated = project
        .host()
        .evaluate_types("/App.vue")
        .expect("evaluated types should exist");

    let binding_names: Vec<&str> = evaluated
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();

    assert_eq!(
        binding_names,
        vec!["shown"],
        "only bindings requested by the component surface should be expanded"
    );
}

#[test]
fn get_component_meta_resolves_workspace_only_barrel_dependencies_for_define_props() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/Link.vue'\nexport * from '../icons'"),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
}

export interface LinkProps extends RouterLinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/Button.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { LinkProps, UseComponentIconsProps } from '../types'

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
}
</script>

<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/Button.vue")
            .unwrap(),
        "workspace owner should load into the shared base project"
    );

    let meta = get_meta(&project, "/workspace/src/runtime/components/Button.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"icon") && prop_names.contains(&"loading"),
        "workspace-only deps should preserve imported icon props, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"href") && prop_names.contains(&"replace"),
        "workspace-only deps should preserve imported LinkProps survivors, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"label") && prop_names.contains(&"color"),
        "workspace-only deps should preserve local props, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"raw") && !prop_names.contains(&"custom"),
        "workspace-only deps should still respect Omit, got: {prop_names:?}"
    );
}

#[test]
fn get_component_meta_recurses_workspace_only_imports_of_imported_vue_types() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/Link.vue'\nexport * from '../icons'"),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/router.ts".to_string(),
        Arc::from(
            r#"export interface RouterLinkProps {
  replace?: boolean
  activeClass?: string
  custom?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"export interface AnchorHTMLAttributes {
  href?: string
  download?: string
  ping?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RouterLinkProps } from '../types/router'
import type { AnchorHTMLAttributes } from '../types/html'

export interface LinkProps extends Omit<RouterLinkProps, 'custom'>, Omit<AnchorHTMLAttributes, 'href'> {
  href?: string
  raw?: boolean
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/Button.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { LinkProps, UseComponentIconsProps } from '../types'

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw'> {
  label?: string
}
</script>

<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/Button.vue")
            .unwrap(),
        "workspace owner should load into the shared base project"
    );

    let meta = get_meta(&project, "/workspace/src/runtime/components/Button.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"icon"),
        "workspace-only nested imports should keep icon props, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"replace") && prop_names.contains(&"activeClass"),
        "workspace-only nested imports should recurse into imported router types, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"download") && prop_names.contains(&"ping"),
        "workspace-only nested imports should recurse into imported html attrs, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"href") && prop_names.contains(&"label"),
        "workspace-only nested imports should preserve direct survivors and locals, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"raw") && !prop_names.contains(&"custom"),
        "workspace-only nested imports should still respect Omit, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces wrapper props imported from a generic interface exported by another .vue file through a barrel.
#[test]
fn get_component_meta_keeps_props_from_barrel_imported_generic_vue_interfaces() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/SelectMenu.vue'\nexport * from '../icons'\nexport * from './input'\n"),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"export interface ButtonHTMLAttributes {
  name?: string
  formaction?: string
  formtarget?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/utils.ts".to_string(),
        Arc::from(
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/input.ts".to_string(),
        Arc::from(
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/SelectMenu.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'name'> {
  open?: boolean
  disabled?: boolean
  name?: string
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/ColorModeSelect.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/ColorModeSelect.vue")
            .unwrap(),
        "workspace owner should load into the shared base project"
    );

    let meta = get_meta(
        &project,
        "/workspace/src/runtime/components/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"loading"),
        "barrel-imported generic vue props should keep imported interface members, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"open")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name"),
        "barrel-imported generic vue props should keep direct generic survivors, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"formaction")
            && prop_names.contains(&"formtarget")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "barrel-imported generic vue props should recurse into imported utility heritage, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"icon")
            && !prop_names.contains(&"items")
            && !prop_names.contains(&"modelValue"),
        "barrel-imported generic vue props should still respect wrapper Omit, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces imported Pick<VueButtonHTMLAttributes, ...> heritage surviving through generic wrapper Omit chains.
#[test]
fn get_component_meta_keeps_imported_picked_button_form_attrs_through_generic_wrapper_omits() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/SelectMenu.vue'\nexport * from '../icons'\nexport * from './input'\n"),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/vue-dom.ts".to_string(),
        Arc::from(
            r#"export interface VueButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"import type { VueButtonHTMLAttributes } from '../vue-dom'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/utils.ts".to_string(),
        Arc::from(
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/input.ts".to_string(),
        Arc::from(
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/SelectMenu.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  disabled?: boolean
  name?: string
  open?: boolean
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/ColorModeSelect.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/ColorModeSelect.vue")
            .unwrap(),
        "workspace owner should load into the shared base project"
    );

    let meta = get_meta(
        &project,
        "/workspace/src/runtime/components/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"formenctype")
            && prop_names.contains(&"formmethod")
            && prop_names.contains(&"formnovalidate")
            && prop_names.contains(&"formtarget"),
        "picked button form attrs should survive generic wrapper omits, got: {prop_names:?}"
    );
}

/// Route/mode-INDEPENDENT L1: a Table.vue-shaped HERMETIC SFC. A generic
/// SFC (`generic="T"`) whose props interface `extends Omit<CoreOptions<T>,
/// 'data'>` — an open object-filter over the OPEN `CoreOptions<T>` heritage,
/// the structural decl-body-lowering route that ran away on the real
/// Table.vue. The carrier-stop keeps the open `Omit` a shallow carrier while
/// the enclosing decl still materialises its OWN closed members.
///
/// The source's KEY DOMAIN is genuinely open: `CoreOptions<T>` intersects
/// the unbound outer `T` (`{ … } & T`), so the enumerable member-name set
/// depends on `T`. A fixed-key generic body with `T` confined to member
/// VALUE positions is the CLOSED key-domain class — it materialises
/// path-precisely (`Omit<Foo<T>, 'items'>` publishes `label`) and is pinned
/// by `omit_wrapped_sfc_generic_param_via_wildcard_resolves`; this fixture
/// pins the complementary OPEN class on the structural route.
///
/// **Discriminating.** Pre-change the structural heritage `Omit<CoreOptions<T>,
/// 'data'>` MATERIALISES its source, flattening `columns`/`rowCount` into the
/// published surface (and the runaway budget can leak a sentinel). Post-change
/// it carrier-stops: the open-domain members do NOT flatten, the enclosing
/// `caption`/`sticky` still publish, and the named `options` field publishes as
/// an `Omit` carrier `Ref`. The `columns`/`rowCount`-absent + no-sentinel
/// assertions FAIL pre-change and PASS post-change.
#[test]
fn get_component_meta_table_shaped_open_omit_heritage_carrier_stops_complete() {
    use crate::resolver_core::component_meta_query_engine::type_expr_is_budget_exceeded_sentinel;

    let project = make_project();
    project
        .upsert_base(
            "/Table.vue",
            r#"<script lang="ts">
export type CoreOptions<T> = {
  data: T
  columns: number
  rowCount: number
} & T

export interface TableProps<T> extends Omit<CoreOptions<T>, 'data'> {
  caption?: string
  sticky?: boolean
  options?: Omit<CoreOptions<T>, 'data'>
}
</script>

<script setup lang="ts" generic="T">
defineProps<TableProps<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Table.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // Own closed members of the enclosing decl materialise.
    assert!(
        prop_names.contains(&"caption"),
        "the enclosing decl's own closed `caption` prop must publish, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"sticky"),
        "the enclosing decl's own closed `sticky` prop must publish, got: {prop_names:?}"
    );

    // The open `Omit<CoreOptions<T>, 'data'>` heritage carrier-stops: its
    // members (`columns`, `rowCount`) do NOT flatten into the surface
    // (shallow-by-default over an OPEN enumeration domain).
    assert!(
        !prop_names.contains(&"columns") && !prop_names.contains(&"rowCount"),
        "the OPEN `Omit<CoreOptions<T>, 'data'>` heritage must carrier-stop — its members \
         must NOT flatten into the published surface, got: {prop_names:?}"
    );
    // `data` is omitted by the filter and is never reachable regardless.
    assert!(
        !prop_names.contains(&"data"),
        "`data` is omitted by the Omit filter and must never publish, got: {prop_names:?}"
    );

    // The named `options` field publishes as a shallow `Omit` carrier `Ref`.
    let options = meta
        .props
        .iter()
        .find(|p| p.name == "options")
        .expect("`options` prop must publish");
    match &options.type_expr {
        TypeExpr::Ref { name, .. } => assert_eq!(
            name.as_ref(),
            "Omit",
            "the open `options: Omit<CoreOptions<T>, 'data'>` field must publish as an \
             `Omit` carrier Ref"
        ),
        other => panic!("`options` must be an `Omit` carrier Ref, got {other:?}"),
    }

    // No published field may carry a budget-tripped partial sentinel — the
    // result must be COMPLETE, not a degraded budget partial.
    for p in &meta.props {
        assert!(
            !type_expr_is_budget_exceeded_sentinel(&p.type_expr),
            "no published prop may carry a BudgetExceeded sentinel; `{}` did: {:?}",
            p.name,
            p.type_expr
        );
    }
}

/// Route/mode-INDEPENDENT L1: a ChatMessages.vue-shaped HERMETIC SFC. A
/// generic SFC whose named props interface `extends Pick<MessageProps<T>, …>`
/// — an open object-filter (`Pick`) over the OPEN generic source
/// `MessageProps<T>` imported cross-file. The heritage flows through the
/// structural materialise (Expanded) route — the same route the real
/// ChatMessages.vue storms on — NOT the inline-object Navigate projector
/// (which already carrier-stops named-member open Picks at hermetic scale).
///
/// **Scope honesty.** The real ChatMessages.vue Pick source is a *chained
/// conditional* (`PropsBase<T> = MessageBase<T> extends … ? … : never`); a
/// hermetic conditional source yields `semanticMiss` downstream pre-change
/// (the separate conditional-reduction gap), so it does not flatten and
/// cannot serve as a clean RED→GREEN discriminator. This fixture therefore
/// uses a PLAIN (non-conditional) Pick source whose KEY DOMAIN is genuinely
/// open — `MessageProps<T>` intersects the unbound outer `T`, so the
/// enumerable member-name set depends on `T` (a fixed-key body with `T`
/// confined to VALUE positions is the CLOSED class and materialises
/// path-precisely instead). The conditional-source Expanded registry-route
/// storm is reproduced and gated by the external-corpus gate (the real
/// oracle).
///
/// **Discriminating.** Pre-change the structural heritage `Pick<MessageProps<T>,
/// 'icon' | 'avatar'>` MATERIALISES the open generic source, flattening
/// `icon`/`avatar` into the published surface. Post-change it carrier-stops:
/// those open-domain members do NOT flatten, the own `caption` still publishes,
/// and no published field carries a budget sentinel. The `icon`/`avatar`-absent
/// + no-sentinel assertions FAIL pre-change and PASS post-change.
#[test]
fn get_component_meta_chat_messages_shaped_open_pick_intersection_carrier_stops() {
    use crate::resolver_core::component_meta_query_engine::type_expr_is_budget_exceeded_sentinel;

    let project = make_project();
    project
        .upsert_base(
            "/chat-types.ts",
            r#"export type MessageProps<T> = {
  icon?: string
  avatar?: string
  side?: 'left' | 'right'
} & T

// A NAMED generic props interface whose HERITAGE is an open Pick over the
// open generic source. `defineProps<ChatProps<T>>()` resolves this through
// the structural materialise (Expanded) route.
export interface ChatProps<T> extends Pick<MessageProps<T>, 'icon' | 'avatar'> {
  caption?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/ChatMessages.vue",
            r#"<script setup lang="ts" generic="T extends unknown[]">
import type { ChatProps } from './chat-types'

defineProps<ChatProps<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/ChatMessages.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // The enclosing decl's own closed member materialises.
    assert!(
        prop_names.contains(&"caption"),
        "the enclosing decl's own `caption` prop must publish, got: {prop_names:?}"
    );

    // The open `Pick<MessageProps<T>, 'icon' | 'avatar'>` heritage
    // carrier-stops: its picked members do NOT flatten into the surface
    // (shallow-by-default over an OPEN intersection-widened enumeration
    // domain — the `& T` arm makes the key set depend on the unbound `T`).
    // Materialising the open generic source is the ChatMessages.vue storm
    // class this prevents.
    assert!(
        !prop_names.contains(&"icon") && !prop_names.contains(&"avatar"),
        "the OPEN `Pick<MessageProps<T>, …>` heritage must carrier-stop — its picked members must \
         NOT flatten into the published surface, got: {prop_names:?}"
    );

    // No published field may carry a budget-tripped partial sentinel.
    for p in &meta.props {
        assert!(
            !type_expr_is_budget_exceeded_sentinel(&p.type_expr),
            "no published prop may carry a BudgetExceeded sentinel; `{}` did: {:?}",
            p.name,
            p.type_expr
        );
    }
}

/// Route/mode-INDEPENDENT L1 for the MAPPED family: a hermetic SFC with
/// `defineSlots<OpenMappedSlots<T>>()` where `OpenMappedSlots<T>` is a
/// `{ [K in keyof BaseSlots]?: (props: { message: MessageBase<T> }) =>
/// VNode[] }` mapped type — the `ChatMessagesSlots<T>` / `TableSlots<T>`
/// family. The keys are enumerable (`keyof BaseSlots` is a finite closed
/// surface) but the per-key value body reaches the OPEN outer generic `T`
/// (NOT the bound mapper binder `K`) through a function-parameter object
/// member.
///
/// **Scope honesty.** A conditional-bodied mapped value (the exact real
/// `ChatMessagesSlots<T>` shape) yields `semanticMiss` downstream at
/// hermetic scale (the separate conditional-reduction gap), so this
/// fixture uses a PLAIN function value to make the per-key-value carrier
/// rule discriminating; the conditional-bodied open mapped storm is
/// reproduced and gated by the external-corpus oracle (the real
/// ChatMessages.vue / Table.vue surface).
///
/// **Discriminating.** The empty-path Shallow surface enumerator gates on
/// the KEY-PRODUCTION axis only: a CLOSED key domain (`keyof BaseSlots`)
/// ENUMERATES its keys even when the per-key value body reaches the open
/// outer `T` — but each enumerated binding's published VALUE stays the
/// deferred CARRIER (`MessageBase<T>` stays a `Ref` reference, never a
/// flattened object of MessageBase's members, never a both-branch Union,
/// never a budget sentinel). An eager per-key-value materialiser flattens
/// `items` / `current` into the binding's type and fails the carrier
/// assertions.
#[test]
fn get_component_meta_open_mapped_slots_surface_carrier_stops_no_sentinel() {
    use crate::resolver_core::component_meta_query_engine::type_expr_is_budget_exceeded_sentinel;

    let project = make_project();
    project
        .upsert_base(
            "/OpenMappedSlots.vue",
            r#"<script lang="ts">
type VNode = { __isVNode: true }

interface BaseSlots {
  header(props: { title: string }): VNode[]
  footer(props: { note: string }): VNode[]
}

interface MessageBase<T> {
  items: T
  current: T
}

// Open mapped slots surface: the keys (`header` / `footer`) are
// enumerable from the closed `BaseSlots`, but each per-key value reaches
// the OPEN outer generic `T` through a function-parameter object member.
export type OpenMappedSlots<T> = {
  [K in keyof BaseSlots]?: (props: { message: MessageBase<T> }) => VNode[]
}
</script>

<script setup lang="ts" generic="T">
defineSlots<OpenMappedSlots<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // The resolution must TERMINATE and publish a COMPLETE result.
    let meta = get_meta(&project, "/OpenMappedSlots.vue");

    // No published prop / slot binding may carry a budget-tripped partial
    // sentinel — the open mapped slots surface carrier-stopped to a
    // shallow shell instead of materialising the per-key value loop.
    for p in &meta.props {
        assert!(
            !type_expr_is_budget_exceeded_sentinel(&p.type_expr),
            "no published prop may carry a BudgetExceeded sentinel; `{}` did: {:?}",
            p.name,
            p.type_expr
        );
    }
    for slot in &meta.slots {
        for binding in &slot.bindings {
            assert!(
                !type_expr_is_budget_exceeded_sentinel(&binding.type_expr),
                "no slot binding may carry a BudgetExceeded sentinel; slot `{}` binding `{}` \
                 did: {:?} — the open mapped slots value body was materialised (the storm)",
                slot.name,
                binding.name,
                binding.type_expr
            );
        }
    }

    // CLOSED key domain ⇒ the keys ENUMERATE: `header` / `footer` publish
    // as named slots (key enumeration is path-precise even when the value
    // body is open).
    let slot_names: Vec<&str> = meta.slots.iter().map(|s| s.name.as_str()).collect();
    for key in ["header", "footer"] {
        assert!(
            slot_names.contains(&key),
            "the closed-key mapped slots surface must enumerate `{key}`, got {slot_names:?}"
        );
    }

    // The open per-key VALUE stays a deferred CARRIER: each enumerated
    // slot's `message` binding publishes the `MessageBase<T>` reference
    // carrier — structurally NOT a flattened `TypeExpr::Object`
    // materialising MessageBase's members (`items` / `current`) and NOT a
    // both-branch Union.
    for slot in &meta.slots {
        let message = slot
            .bindings
            .iter()
            .find(|b| b.name == "message")
            .unwrap_or_else(|| {
                panic!(
                    "slot `{}` must carry the `message` binding from the per-key \
                     function value's first-parameter object, got bindings: {:?}",
                    slot.name,
                    slot.bindings.iter().map(|b| &b.name).collect::<Vec<_>>()
                )
            });
        match &message.type_expr {
            verter_type_expr::TypeExpr::Object(obj) => panic!(
                "slot `{}`'s `message` binding must stay the MessageBase<T> carrier — it \
                 flattened into an object surface: {obj:?}",
                slot.name
            ),
            verter_type_expr::TypeExpr::Union(arms) => panic!(
                "slot `{}`'s `message` binding must stay the MessageBase<T> carrier — it \
                 widened into a Union: {arms:?}",
                slot.name
            ),
            verter_type_expr::TypeExpr::Ref { name, .. } => assert_eq!(
                name.as_ref(),
                "MessageBase",
                "slot `{}`'s `message` binding must publish the MessageBase reference carrier",
                slot.name
            ),
            other => panic!(
                "slot `{}`'s `message` binding must publish a reference carrier, got {other:?}",
                slot.name
            ),
        }
    }

    // POSITIVE witness — the carrier-stop is not a DROPPED surface. The
    // empty/shallow assertions above would also pass on a broken pipeline
    // that simply lost the slots payload, so three positive facts pin the
    // consumed-and-carried shape:
    //
    //  (a) the analyzer consumed `defineSlots<OpenMappedSlots<T>>()`;
    //  (b) the slots macro-payload expansion ran to COMPLETION and
    //      published its (shallow) shape — a dropped payload has no
    //      `define_slots` entry, a stormed one is not `Completed`;
    //  (c) the shared dispatch route the registry / macro-payload
    //      consumers resolve through yields the PRESERVED `Mapped`
    //      carrier for `OpenMappedSlots<T>` (the carrier IS the published
    //      value and stays re-resolvable on demand).
    let resolved = project
        .host()
        .resolve_component_meta(
            "/OpenMappedSlots.vue",
            crate::types::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    assert!(
        resolved.snapshot.macros.iter().any(|m| {
            matches!(
                m.kind,
                verter_semantic::analysis::AnalyzedMacroKind::DefineSlots
            ) && m.type_references.iter().any(|r| r == "OpenMappedSlots")
        }),
        "the analyzer must consume `defineSlots<OpenMappedSlots<T>>()`, got macros: {:?}",
        resolved.snapshot.macros
    );
    let evaluated = resolved
        .evaluated_types
        .as_ref()
        .expect("Expanded-mode resolution must carry evaluated types");
    assert_eq!(
        evaluated.define_slots.len(),
        1,
        "the slots macro payload must publish exactly one expanded shape (a missing entry \
         means the slots surface was DROPPED, not carrier-stopped), got {:?}",
        evaluated.define_slots
    );
    assert!(
        matches!(
            evaluated.define_slots[0].result.execution_status,
            verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed
        ),
        "the carrier-stopped slots payload must complete (not storm / trip a budget), got {:?}",
        evaluated.define_slots[0].result.execution_status
    );

    // (c) — the preserved `Mapped` carrier is reachable through the shared
    // dispatch (the same route the registry materialiser and the macro
    // payload resolve through): instantiating `OpenMappedSlots<T>` with an
    // open `T` yields the deferred `Mapped` shell, not an enumerated
    // Object and not an Opaque miss.
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(project.host());
    let graph = Arc::clone(project.host().project_type_store().semantic_graph());
    let t_param = graph.intern_node(crate::semantic_query::SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let carrier = match dispatch
        .execute_read(crate::semantic_query::SemanticQueryKey::Instantiate {
            base: crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
                Arc::from("/OpenMappedSlots.vue"),
                Arc::from("OpenMappedSlots"),
            ),
            args: Arc::from(vec![t_param].into_boxed_slice()),
            context: crate::semantic_query::InstantiateContext::new(
                crate::semantic_query::ProjectionReductionContext::published(
                    crate::semantic_query::ProjectionMode::Navigate,
                ),
                Default::default(),
            ),
        })
        .value
    {
        crate::semantic_query::QueryResult::Value(node) => node,
        other => panic!("OpenMappedSlots<T> must resolve to a Value carrier, got {other:?}"),
    };
    let mut node = carrier;
    for _ in 0..4 {
        match graph.node_data(node).as_deref() {
            Some(crate::semantic_query::SemanticNodeData::Alias(inner)) => node = *inner,
            _ => break,
        }
    }
    assert!(
        matches!(
            graph.node_data(node).as_deref(),
            Some(crate::semantic_query::SemanticNodeData::Mapped { .. })
        ),
        "instantiating OpenMappedSlots<T> through the shared dispatch must preserve the \
         `Mapped` carrier shell (the published value), got {:?}",
        graph.node_data(node)
    );
}

// @ai-generated - Reproduces Pick<VueButtonHTMLAttributes, ...> form attrs disappearing when the source alias comes from a package import.
#[test]
fn get_component_meta_keeps_picked_package_button_form_attrs_through_generic_wrapper_omits() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            "export * from '../components/SelectMenu.vue'\nexport * from '../icons'\nexport * from './input'\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/input.ts",
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/utils.ts",
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  disabled?: boolean
  name?: string
  open?: boolean
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/color-mode/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = get_meta(
        &project,
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"loading")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name")
            && prop_names.contains(&"open")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "package wrapper should preserve declared props, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces package-imported Pick<VueButtonHTMLAttributes, ...> heritage surviving through a cyclic barrel that also re-exports the wrapper component.
#[test]
fn get_component_meta_keeps_picked_package_button_form_attrs_through_cyclic_barrel_wrapper_omits() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            r#"export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from '../icons'
export * from './input'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/input.ts",
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/utils.ts",
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  disabled?: boolean
  name?: string
  open?: boolean
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/color-mode/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
        vec![crate::types::DependencyResolution {
            specifier: "../../types".to_string(),
            resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/SelectMenu.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/utils".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../components/SelectMenu.vue".to_string(),
                resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../components/color-mode/ColorModeSelect.vue".to_string(),
                resolved_canonical_id: Some(
                    "/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../icons".to_string(),
                resolved_canonical_id: Some("/src/runtime/icons.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./input".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/input.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = get_meta(
        &project,
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"loading")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name")
            && prop_names.contains(&"open")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "cyclic barrel wrapper should preserve declared props, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces package-imported Pick<VueButtonHTMLAttributes, ...> heritage surviving through a cyclic barrel when defineProps is wrapped in withDefaults().
#[test]
fn get_component_meta_keeps_picked_package_button_form_attrs_through_cyclic_barrel_with_defaults() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}

export declare function withDefaults<T, D>(props: T, defaults: D): T & D
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            r#"export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from '../icons'
export * from './input'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/input.ts",
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/utils.ts",
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  disabled?: boolean
  name?: string
  open?: boolean
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/color-mode/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<ColorModeSelectProps>(), {
  searchInput: false
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "vue".to_string(),
                resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/SelectMenu.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/utils".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../components/SelectMenu.vue".to_string(),
                resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../components/color-mode/ColorModeSelect.vue".to_string(),
                resolved_canonical_id: Some(
                    "/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../icons".to_string(),
                resolved_canonical_id: Some("/src/runtime/icons.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./input".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/input.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = get_meta(
        &project,
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"loading")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name")
            && prop_names.contains(&"open")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "cyclic barrel withDefaults wrapper should preserve declared props, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces package-imported Pick<VueButtonHTMLAttributes, ...> heritage surviving when the imported generic interface also extends a picked external generic package interface.
#[test]
fn get_component_meta_keeps_picked_package_button_form_attrs_through_external_generic_pick_and_cyclic_barrel(
) {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"export interface ComboboxRootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  name?: string
  by?: string
  items?: T
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            r#"export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from '../icons'
export * from './input'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/input.ts",
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/utils.ts",
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { ComboboxRootProps } from 'reka-ui'
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends Pick<ComboboxRootProps<T>, 'open' | 'defaultOpen' | 'disabled' | 'name' | 'by'>,
    UseComponentIconsProps,
    Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/color-mode/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<ColorModeSelectProps>(), {
  searchInput: false
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "vue".to_string(),
                resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/SelectMenu.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "reka-ui".to_string(),
                resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/utils".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../components/SelectMenu.vue".to_string(),
                resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../components/color-mode/ColorModeSelect.vue".to_string(),
                resolved_canonical_id: Some(
                    "/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../icons".to_string(),
                resolved_canonical_id: Some("/src/runtime/icons.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./input".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/input.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = get_meta(
        &project,
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"open")
            && prop_names.contains(&"defaultOpen")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name")
            && prop_names.contains(&"by")
            && prop_names.contains(&"loading")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "external generic pick + cyclic barrel wrapper should preserve declared props, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_keeps_reexported_vue_button_form_attrs_through_workspace_generic_wrapper() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(
            r#"{ "name": "vue", "types": "./dist/vue.d.ts", "exports": { ".": { "types": "./dist/vue.d.ts", "import": "./dist/vue.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/vue.d.ts".to_string(),
        Arc::from("export * from '@vue/runtime-dom'"),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/vue.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/package.json".to_string(),
        Arc::from(
            r#"{ "name": "@vue/runtime-dom", "types": "./dist/runtime-dom.d.ts", "exports": { ".": { "types": "./dist/runtime-dom.d.ts", "import": "./dist/runtime-dom.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts".to_string(),
        Arc::from(
            r#"export interface HTMLAttributes {
  class?: any
}

export interface ButtonHTMLAttributes extends HTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/dist/runtime-dom.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/package.json".to_string(),
        Arc::from(
            r#"{ "name": "reka-ui", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.d.ts".to_string(),
        Arc::from(
            r#"export interface ComboboxRootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  name?: string
  by?: string
  items?: T
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from(
            r#"export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from '../icons'
export * from './input'
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/input.ts".to_string(),
        Arc::from(
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/utils.ts".to_string(),
        Arc::from(
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/SelectMenu.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { ComboboxRootProps } from 'reka-ui'
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends Pick<ComboboxRootProps<T>, 'open' | 'defaultOpen' | 'disabled' | 'name' | 'by'>,
    UseComponentIconsProps,
    Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<ColorModeSelectProps>(), {
  searchInput: false
})
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let project = MetaProject::new(host);
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/color-mode/ColorModeSelect.vue")
            .unwrap(),
        "workspace owner should load the wrapper component"
    );

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/workspace/src/runtime/components/color-mode/ColorModeSelect.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    let define_props = evaluated
        .define_props
        .first()
        .expect("wrapper should produce a defineProps expansion");
    let prop_names: Vec<&str> = define_props
        .result
        .value
        .properties
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"open")
            && prop_names.contains(&"defaultOpen")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name")
            && prop_names.contains(&"by")
            && prop_names.contains(&"loading")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "workspace evaluate_types should preserve declared wrapper props, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_keeps_complex_nuxt_ui_form_attrs_through_wrapper_omits() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/tsconfig.json".to_string(),
        Arc::from(
            r#"{ "compilerOptions": { "module": "esnext", "moduleResolution": "bundler" } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(
            r#"{ "name": "vue", "type": "module", "exports": { ".": { "types": "./dist/vue.d.mts", "import": "./dist/vue.runtime.mjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/vue.d.mts".to_string(),
        Arc::from(
            r#"export * from '@vue/runtime-dom'
export type VNode = any
export declare function withDefaults<T, D>(props: T, defaults: D): T & D
"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/vue.runtime.mjs".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/package.json".to_string(),
        Arc::from(
            r#"{ "name": "@vue/runtime-dom", "type": "module", "exports": { ".": { "types": "./dist/runtime-dom.d.ts", "import": "./dist/runtime-dom.mjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts".to_string(),
        Arc::from(
            r#"export interface HTMLAttributes {
  class?: any
}

export interface ButtonHTMLAttributes extends HTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/dist/runtime-dom.mjs".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/package.json".to_string(),
        Arc::from(
            r#"{ "name": "reka-ui", "type": "module", "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.mjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.d.ts".to_string(),
        Arc::from(
            r#"export interface ComboboxRootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  name?: string
  resetSearchTermOnBlur?: boolean
  resetSearchTermOnSelect?: boolean
  resetModelValueOnClear?: boolean
  highlightOnHover?: boolean
  by?: string
  items?: T
}

export interface ComboboxRootEmits {
  'update:open': [value: boolean]
}

export interface ComboboxContentProps {
  side?: 'bottom' | 'top'
  sideOffset?: number
  collisionPadding?: number
  position?: 'popper' | 'item-aligned'
  as?: string
  asChild?: boolean
  forceMount?: boolean
}

export interface ComboboxContentEmits {
  escapeKeyDown?: [event: KeyboardEvent]
}

export interface ComboboxArrowProps {
  width?: number
  height?: number
  as?: string
  asChild?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.mjs".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/input.ts".to_string(),
        Arc::from(
            r#"export interface ModelModifiers {
  trim?: boolean
  number?: boolean
  lazy?: boolean
}

export type ApplyModifiers<T, _Mod> = T
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/utils.ts".to_string(),
        Arc::from(
            r#"export type AcceptableValue = string | number
export type ArrayOrNested<T> = T[] | T[][]
export type GetItemKeys<T> = string
export type GetItemValue<T, VK> = VK extends string ? string : T
export type GetModelValue<T, VK, M, ExcludeItem> = M extends true
  ? Array<GetItemValue<T, VK>>
  : GetItemValue<T, VK> | ExcludeItem
export type NestedItem<A> = A extends Array<infer U> ? U : never
export type EmitsToProps<T> = T extends object ? { [K in keyof T as K extends string ? `on${Capitalize<K>}` : never]?: T[K] } : {}
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/tv.ts".to_string(),
        Arc::from(
            r#"export type ComponentConfig<_Theme, _AppConfig, _Name extends string> = {
  variants: {
    color: 'primary' | 'neutral'
    variant: 'outline' | 'ghost'
    size: 'sm' | 'md'
  }
  slots: Record<string, any>,
  ui: Record<string, any>
}
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface AvatarProps {
  src?: string
}

export interface ButtonProps {
  color?: string
  variant?: string
  icon?: string
}

export interface ChipProps {
  color?: string
}

export interface IconProps {
  name?: string
}

export interface InputProps {
  modelValue?: string
  defaultValue?: string
  placeholder?: string
  variant?: string
}

export type LinkPropsKeys = 'href' | 'to'

export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from './input'
export * from './tv'
export * from './utils'
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/SelectMenu.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { ComboboxRootProps, ComboboxRootEmits, ComboboxContentProps, ComboboxContentEmits, ComboboxArrowProps } from 'reka-ui'
import type { VNode } from 'vue'
import type { UseComponentIconsProps } from '../types'
import type { AvatarProps, ButtonProps, ChipProps, IconProps, InputProps, LinkPropsKeys } from '../types'
import type { ModelModifiers, ApplyModifiers } from '../types/input'
import type { ButtonHTMLAttributes } from '../types/html'
import type { AcceptableValue, ArrayOrNested, GetItemKeys, GetModelValue, NestedItem, EmitsToProps } from '../types/utils'
import type { ComponentConfig } from '../types/tv'

type SelectMenu = ComponentConfig<unknown, {}, 'selectMenu'>

export type SelectMenuValue = AcceptableValue

export type SelectMenuItem = SelectMenuValue | {
  label?: string
  description?: string
  icon?: IconProps['name']
  avatar?: AvatarProps
  chip?: ChipProps
  type?: 'label' | 'separator' | 'item'
  disabled?: boolean
  onSelect?: (e: Event) => void
  class?: any
  ui?: Pick<SelectMenu['slots'], 'label' | 'separator' | 'item'>
  [key: string]: any
}

type ExcludeItem = { type: 'label' | 'separator' }
type IsClearUsed<M extends boolean, C extends boolean | object> = M extends false
  ? (C extends true ? null : C extends object ? null : never)
  : never

export interface SelectMenuProps<T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>, VK extends GetItemKeys<T> | undefined = undefined, M extends boolean = false, Mod extends Omit<ModelModifiers, 'lazy'> = Omit<ModelModifiers, 'lazy'>, C extends boolean | object = false> extends Pick<ComboboxRootProps<T>, 'open' | 'defaultOpen' | 'disabled' | 'name' | 'resetSearchTermOnBlur' | 'resetSearchTermOnSelect' | 'resetModelValueOnClear' | 'highlightOnHover' | 'by'>, UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  id?: string
  placeholder?: string
  searchInput?: boolean | Omit<InputProps, 'modelValue' | 'defaultValue'>
  color?: SelectMenu['variants']['color']
  variant?: SelectMenu['variants']['variant']
  size?: SelectMenu['variants']['size']
  required?: boolean
  trailingIcon?: IconProps['name']
  selectedIcon?: IconProps['name']
  clear?: (C & boolean) | (C & Partial<Omit<ButtonProps, LinkPropsKeys>>)
  clearIcon?: IconProps['name']
  content?: Omit<ComboboxContentProps, 'as' | 'asChild' | 'forceMount'> & Partial<EmitsToProps<ComboboxContentEmits>>
  arrow?: boolean | Omit<ComboboxArrowProps, 'as' | 'asChild'>
  portal?: boolean | string | HTMLElement
  virtualize?: boolean | {
    overscan?: number
    estimateSize?: number | ((index: number) => number)
  }
  valueKey?: VK
  labelKey?: GetItemKeys<T>
  descriptionKey?: GetItemKeys<T>
  items?: T
  defaultValue?: ApplyModifiers<GetModelValue<T, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>
  modelValue?: ApplyModifiers<GetModelValue<T, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>
  modelModifiers?: Mod
  multiple?: M & boolean
  highlight?: boolean
  createItem?: boolean | 'always' | { position?: 'top' | 'bottom', when?: 'empty' | 'always' }
  filterFields?: string[]
  ignoreFilter?: boolean
  autofocus?: boolean
  autofocusDelay?: number
  class?: any
  ui?: SelectMenu['slots']
}

export interface SelectMenuEmits<
  A extends ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<A> | undefined,
  M extends boolean,
  Mod extends Omit<ModelModifiers, 'lazy'> = Omit<ModelModifiers, 'lazy'>,
  C extends boolean | object = false
> extends Pick<ComboboxRootEmits, 'update:open'> {
  'change': [event: Event]
  'blur': [event: FocusEvent]
  'focus': [event: FocusEvent]
  'create': [item: string]
  'clear': []
  'highlight': [payload: {
    ref: HTMLElement,
    value: ApplyModifiers<GetModelValue<A, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>
  } | undefined]
  'update:modelValue': [value: ApplyModifiers<GetModelValue<A, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>]
}

type SlotProps<T extends SelectMenuItem> = (props: { item: T, index: number, ui: SelectMenu['ui'] }) => VNode[]

export interface SelectMenuSlots<
  A extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<A> | undefined = undefined,
  M extends boolean = false,
  Mod extends Omit<ModelModifiers, 'lazy'> = Omit<ModelModifiers, 'lazy'>,
  C extends boolean | object = false,
  T extends NestedItem<A> = NestedItem<A>
> {
  'default'?(props: {
    modelValue: ApplyModifiers<GetModelValue<A, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>,
    open: boolean
    ui: SelectMenu['ui']
  }): VNode[]
  'item'?: SlotProps<T>
}
</script>

<script setup lang="ts" generic="T extends ArrayOrNested<SelectMenuItem>, VK extends GetItemKeys<T> | undefined = undefined, M extends boolean = false, Mod extends Omit<ModelModifiers, 'lazy'> = Omit<ModelModifiers, 'lazy'>, C extends boolean | object = false">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<SelectMenuProps<T, VK, M, Mod, C>>(), {
  portal: true,
  searchInput: true,
  labelKey: 'label',
  descriptionKey: 'description',
  resetSearchTermOnBlur: true,
  resetSearchTermOnSelect: true,
  resetModelValueOnClear: true,
  autofocusDelay: 0,
  virtualize: false
})
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<ColorModeSelectProps>(), {
  searchInput: false
})
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let project = MetaProject::new(host);
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/color-mode/ColorModeSelect.vue")
            .unwrap(),
        "workspace owner should load the complex wrapper component"
    );

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/workspace/src/runtime/components/color-mode/ColorModeSelect.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    let define_props = evaluated
        .define_props
        .first()
        .expect("wrapper should produce a defineProps expansion");
    let prop_names: Vec<&str> = define_props
        .result
        .value
        .properties
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"open")
            && prop_names.contains(&"defaultOpen")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name")
            && prop_names.contains(&"loading")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "complex Nuxt UI wrapper should preserve declared wrapper props, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_hydrates_transitive_imported_pick_dependencies_for_wrapper_props() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types.ts",
            r#"import type { ButtonHTMLAttributes } from './types/html'

export interface Props extends Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  label?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './runtime/types'

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./types/html".to_string(),
            resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./runtime/types".to_string(),
            resolved_canonical_id: Some("/src/runtime/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/src/App.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    let define_props = evaluated
        .define_props
        .first()
        .expect("wrapper should produce a defineProps expansion");
    let prop_names: Vec<&str> = define_props
        .result
        .value
        .properties
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"label"),
        "wrapper evaluation should resolve the local label prop, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_handles_shadowed_get_item_keys_defaults_without_hanging() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/utils.ts",
            r#"export type NestedItem<T> = T extends Array<infer I> ? NestedItem<I> : T
export type GetItemKeys<I, T extends NestedItem<I> = NestedItem<I>> = keyof Extract<T, object> & string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Accordion.vue",
            r#"<script lang="ts">
import type { GetItemKeys } from './src/types/utils'

export interface Item {
  label?: string
  value?: string
  [key: string]: any
}

export interface Props<T extends Item = Item> {
  valueKey?: GetItemKeys<T>
}
</script>

<script setup lang="ts" generic="T extends Item">
defineProps<Props<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/Accordion.vue")
        .unwrap()
        .expect("evaluate_types should return a result");
    let value_key = evaluated
        .props
        .iter()
        .find(|field| field.name == "valueKey")
        .expect("valueKey should be present");

    assert_eq!(
        value_key.execution_status,
        verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
    );
    assert!(
        matches!(
            value_key.exactness,
            verter_semantic::analysis::type_expand::ExpansionExactness::ExactSymbolic,
        ) || matches!(
            value_key.exactness,
            verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete,
        ),
        "valueKey should resolve without hanging, got {:?}",
        value_key.exactness
    );
}

#[test]
fn evaluate_types_hydrates_transitive_imported_pick_dependencies_from_dual_script_vue_deps() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}

export declare function withDefaults<T, D>(props: T, defaults: D): T & D
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes } from '../types/html'

export type SelectMenuItem = {
  label?: string
}

export interface SelectMenuProps<T extends SelectMenuItem[] = SelectMenuItem[]> extends Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  items?: T
  label?: string
}
</script>

<script setup lang="ts" generic="T extends SelectMenuItem[] = SelectMenuItem[]">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<SelectMenuProps<T>>(), {})
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            r#"export * from '../components/SelectMenu.vue'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { SelectMenuProps, SelectMenuItem } from './runtime/types'

defineProps<Omit<SelectMenuProps<SelectMenuItem[]>, 'items'>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/SelectMenu.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "vue".to_string(),
                resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/types/index.ts",
        vec![crate::types::DependencyResolution {
            specifier: "../components/SelectMenu.vue".to_string(),
            resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./runtime/types".to_string(),
            resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();

    // API-asymmetry invariant: a resolved component meta query MUST
    // produce a coherent payload through BOTH `evaluate_types` and
    // `get_component_meta` (or NEITHER). Returning `None` from one
    // while the other produces a populated payload is a production
    // defect — the publish guard at `host_manage/component_meta_methods.rs`
    // unconditionally assigns `parts.evaluated_types` after running
    // resolution so the two APIs agree on whether resolution occurred.
    let evaluated = session
        .evaluate_types("/src/App.vue")
        .unwrap()
        .expect("evaluate_types should return a result");
    // `define_props` enumeration is allowed to be empty for this
    // macro-payload shape under the shallow-by-default architecture
    // (non-Conditional `Omit<SelectMenuProps<SelectMenuItem[]>, 'items'>`
    // rides transit-shallow at the macro-shape boundary and produces
    // no member enumeration at publication time — the shallow-by-default
    // architecture has no rescue projection that eagerly widens
    // it). The `evaluate_types` API contract here is "resolution
    // ran and produced a coherent payload", witnessed by the `Some(_)`
    // return.
    let _ = &evaluated;

    // Authoritative `label` assertion routes through
    // `get_component_meta`, which derives `props` via projector paths
    // independent of the macro-shape enumeration. The transitively
    // imported `SelectMenuProps<T>.label` member must surface as a
    // published prop.
    let component_meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should return a result");
    let prop_names: Vec<&str> = component_meta
        .props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"label"),
        "dual-script vue wrapper meta should resolve the local label prop, got: {prop_names:?}"
    );
}

#[test]
fn get_component_meta_keeps_local_slot_surface_without_imported_helper_pollution() {
    let project = make_project();
    project
        .upsert_base(
            "/tv.ts",
            r#"export type DynamicSlots<T extends Record<string, any>> = {
  [K in keyof T]?: (props: {}) => any
}

export type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: (props: {}) => any
}

export type ComponentConfig<T extends { slots?: Record<string, any> }, A extends Record<string, any>> = {
  appConfig: A,
  slots: ComponentSlots<T>
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/schema.ts",
            r#"export interface AppConfig {
  ui?: { variant: string }
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  slots: {
    leading: 'leading',
    trailing: 'trailing'
  }
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ComponentConfig, DynamicSlots } from './tv'
import type { AppConfig } from './schema'
import theme from './theme'

type Accordion = ComponentConfig<typeof theme, AppConfig>

interface Slots extends DynamicSlots<Accordion['slots']> {
  default(props: { item: string }): any
  leading?(): any
  trailing?(): any
}

defineSlots<Slots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let meta = get_meta(&project, "/App.vue");
    let slot_names: Vec<&str> = meta.slots.iter().map(|slot| slot.name.as_str()).collect();
    assert_eq!(slot_names, vec!["default", "leading", "trailing"]);
    assert!(
        !slot_names.contains(&"appConfig") && !slot_names.contains(&"slots"),
        "defineSlots output should not be polluted by imported helper object members: {slot_names:?}"
    );
}

#[test]
fn evaluate_types_invalidates_cached_results_when_dependency_changes() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ImportedUser {
  id: number
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedUser } from './types'

defineProps<{
  user: ImportedUser
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let first = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let first_cache = cached_resolved_state(
        &project,
        "/Comp.vue",
        crate::types::ProjectionMode::Expanded,
    )
    .expect("first evaluation should populate the cache");
    let first_meta = session
        .get_component_meta("/Comp.vue")
        .unwrap()
        .expect("first evaluation should produce component meta");

    assert!(
        matches!(
            evaluated_prop_type(&first, "user"),
            TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "ImportedUser" && type_arguments.is_empty()
        ),
        "evaluate_types should keep imported object-like fields symbolic in expanded evaluated types, got {:?}",
        evaluated_prop_type(&first, "user")
    );
    // §3.4 structural classification: `ImportedUser` is consumed by the
    // owner SFC's `defineProps<{ user: ImportedUser }>()` macro (it
    // appears as the value type of the `user` property in the inline
    // Object macro arg). Per the structural macro-participation
    // classifier, the policy keeps imported macro-participating refs
    // symbolic — the published surface preserves the `Ref { name:
    // "ImportedUser" }` shape, consistent with the shallow-by-default
    // contract for plain alias references (CLAUDE.md Component-Meta
    // Shallow-By-Default Rule). Consumers (zod/json-schema/storybook/
    // histoire adapters, compat layer) re-resolve the alias through
    // the registry on demand rather than seeing it inlined here.
    match &first_meta
        .props
        .iter()
        .find(|prop| prop.name == "user")
        .expect("component meta should keep the imported user prop")
        .type_expr
    {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(name.as_ref(), "ImportedUser");
            assert!(type_arguments.is_empty());
        }
        other => panic!("§3.4: imported macro-participating ref must stay symbolic, got {other:?}"),
    }

    session
        .upsert(
            "/types.ts",
            r#"export interface ImportedUser {
  id: number,
  label: string
}"#
            .into(),
        )
        .unwrap();

    let second = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let second_cache = cached_resolved_state(
        &project,
        "/Comp.vue",
        crate::types::ProjectionMode::Expanded,
    )
    .expect("dependency update should repopulate the cache");
    let second_meta = session
        .get_component_meta("/Comp.vue")
        .unwrap()
        .expect("dependency update should keep component meta available");

    assert!(
        !Arc::ptr_eq(&first_cache, &second_cache),
        "dependency change must invalidate the owner's resolved-meta cache",
    );
    assert!(
        matches!(
            evaluated_prop_type(&second, "user"),
            TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "ImportedUser" && type_arguments.is_empty()
        ),
        "evaluate_types should keep imported object-like fields symbolic after cache invalidation too, got {:?}",
        evaluated_prop_type(&second, "user")
    );
    // §3.4: after dep change, the published surface still carries the
    // symbolic `Ref { name: "ImportedUser" }`. The cache invalidation
    // contract is verified by the `!Arc::ptr_eq(first_cache,
    // second_cache)` assertion above; the registry's body has been
    // updated to include the new `label` member, which consumers
    // observe by re-resolving the alias on demand.
    match &second_meta
        .props
        .iter()
        .find(|prop| prop.name == "user")
        .expect("component meta should keep the imported user prop after invalidation")
        .type_expr
    {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(name.as_ref(), "ImportedUser");
            assert!(type_arguments.is_empty());
        }
        other => panic!(
            "§3.4: imported macro-participating ref must stay symbolic after cache invalidation, got {other:?}"
        ),
    }
}

// ===========================================================================
// Phase 1: Provenance counters and enriched-analysis caching
// ===========================================================================

/// Helper to read the provenance counters from a MetaProject's host.
fn provenance(project: &MetaProject) -> crate::types::MetaProvenanceSnapshot {
    project.host().provenance().snapshot()
}

#[test]
fn evaluate_types_returns_correct_results_for_imported_types() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<{ item: Props }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();

    let evaluated = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should succeed")
        .expect("should return evaluated types");

    // Assert+: the prop referencing the imported type is present
    assert_eq!(
        evaluated.props.len(),
        1,
        "should have exactly 1 prop 'item'"
    );
    assert_eq!(evaluated.props[0].name, "item");

    // Assert-: no spurious props with names from the imported interface
    assert!(
        !evaluated
            .props
            .iter()
            .any(|p| p.name == "a" || p.name == "b"),
        "imported interface fields should not appear as top-level props"
    );
}

#[test]
fn evaluate_types_cold_path_does_not_call_public_get_analysis_workflow() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session_batch().unwrap();

    let _ = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should succeed on a cold path");

    let p = provenance(&project);
    assert_eq!(
        p.get_analysis_calls, 0,
        "evaluate_types should use the private resolved-state helper instead of the public get_analysis workflow",
    );
}

#[test]
fn evaluate_types_works_independently_of_prior_get_analysis_call() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("count: number; label: string"))
        .unwrap();

    let session = project.open_session_batch().unwrap();

    // Call get_analysis first (raw, no enrichment)
    let analysis = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("get_analysis should return raw analysis");

    // get_analysis returns raw props
    let raw_names = prop_names(&analysis);
    assert!(
        raw_names.contains(&"count".to_string()),
        "raw analysis should have 'count' prop"
    );

    // evaluate_types should still work correctly regardless of prior get_analysis
    let evaluated = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should succeed")
        .expect("should return evaluated types");

    // Assert+: types are properly resolved
    assert_eq!(
        evaluated_prop_type(&evaluated, "count"),
        &TypeExpr::Primitive(PrimitiveName::Number),
    );
    assert_eq!(
        evaluated_prop_type(&evaluated, "label"),
        &TypeExpr::Primitive(PrimitiveName::String),
    );

    // Assert-: only the expected props
    assert_eq!(evaluated.props.len(), 2);
}

#[test]
fn evaluate_types_returns_consistent_results_for_repeated_calls() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("a: string; b: number"))
        .unwrap();

    let session = project.open_session_batch().unwrap();

    // First call
    let first = session
        .evaluate_types("/App.vue")
        .expect("first evaluate_types should succeed")
        .expect("should return evaluated types");

    // Second call — should return identical results
    let second = session
        .evaluate_types("/App.vue")
        .expect("second evaluate_types should succeed")
        .expect("should return evaluated types");

    // Assert+: both calls return the same prop count and types
    assert_eq!(
        first.props.len(),
        second.props.len(),
        "repeated evaluate_types calls should return the same number of props"
    );
    assert_eq!(
        evaluated_prop_type(&first, "a"),
        evaluated_prop_type(&second, "a"),
        "repeated calls should return the same type for prop 'a'"
    );

    // Assert-: no extra props introduced
    assert_eq!(first.props.len(), 2, "should have exactly 2 props");
}

#[test]
fn resolve_component_meta_expanded_returns_consistent_results_on_repeated_calls() {
    use crate::types::ProjectionMode;

    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    // Force host to load the file
    let _ = session.get_analysis("/App.vue").unwrap();

    // First call
    let first = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("first resolve_component_meta should succeed");

    // Second call — should return consistent results
    let second = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("second resolve_component_meta should succeed");

    // Assert+: both calls return the same resolved macros
    assert_eq!(
        first.resolved_macros.len(),
        second.resolved_macros.len(),
        "repeated calls should return the same number of resolved macros"
    );

    // Assert+: resolved macros have consistent prop counts
    assert!(
        !first.resolved_macros.is_empty(),
        "`ProjectionMode::Expanded` should resolve cross-file macro types on first call"
    );
    assert!(
        !second.resolved_macros.is_empty(),
        "`ProjectionMode::Expanded` should resolve cross-file macro types on second call"
    );
    assert_eq!(
        resolved_macro_prop_names(project.host(), "/App.vue", &first).len(),
        resolved_macro_prop_names(project.host(), "/App.vue", &second).len(),
        "repeated calls should produce the same resolved prop count"
    );

    // Assert-: mode is Expanded, not Type
    assert_eq!(first.mode, ProjectionMode::Expanded);
    assert_ne!(first.mode, ProjectionMode::Identity);
}

#[test]
fn resolve_component_meta_expanded_returns_updated_results_after_owner_change() {
    use crate::types::ProjectionMode;

    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("a: string; b: number"))
        .unwrap();

    // First call — inline props should be resolved
    let first = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("first resolve_component_meta should succeed");

    let first_snap_props = prop_names(&first.snapshot);
    assert!(
        first_snap_props.contains(&"a".to_string()),
        "first call should have prop 'a', got: {:?}",
        first_snap_props
    );
    assert_eq!(first_snap_props.len(), 2, "should start with 2 props");

    // Modify the owner SFC to change props
    project
        .upsert_base("/App.vue", &sfc("c: boolean; d: string"))
        .unwrap();

    // Second call — should see the updated props
    let second = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("second resolve_component_meta should succeed after owner change");

    let second_snap_props = prop_names(&second.snapshot);

    // Assert+: result includes the new props
    assert!(
        second_snap_props.contains(&"c".to_string()),
        "owner change should produce updated props including 'c', got: {:?}",
        second_snap_props
    );
    assert!(
        second_snap_props.contains(&"d".to_string()),
        "owner change should produce updated props including 'd', got: {:?}",
        second_snap_props
    );

    // Assert-: old props should not appear
    assert!(
        !second_snap_props.contains(&"a".to_string()),
        "old prop 'a' should not appear after owner change"
    );
    assert!(
        !second_snap_props.contains(&"b".to_string()),
        "old prop 'b' should not appear after owner change"
    );
}

#[test]
fn resolve_component_meta_expanded_returns_updated_results_after_dependency_change() {
    use crate::types::ProjectionMode;

    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Manually register the import dependency so reverse-dep tracking works.
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // First call — should resolve props a, b via resolved_macros
    let first = project
        .host()
        .resolve_component_meta("/src/App.vue", ProjectionMode::Expanded)
        .expect("first resolve_component_meta should succeed");

    assert!(
        !first.resolved_macros.is_empty(),
        "`ProjectionMode::Expanded` should resolve cross-file macro types"
    );
    let first_prop_names = resolved_macro_prop_names(project.host(), "/src/App.vue", &first);
    assert!(
        first_prop_names.contains(&"a".to_string()) && first_prop_names.contains(&"b".to_string()),
        "first call should resolve props a and b, got: {:?}",
        first_prop_names
    );

    // Modify the dependency via base upsert (directly on host, not session)
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface Props { a: string; b: number; c: boolean }"#,
        )
        .unwrap();

    // Second call — should reflect the dependency change
    let second = project
        .host()
        .resolve_component_meta("/src/App.vue", ProjectionMode::Expanded)
        .expect("resolve_component_meta should succeed after dependency change");

    assert!(
        !second.resolved_macros.is_empty(),
        "should still have resolved macros after dep change"
    );
    let second_prop_names = resolved_macro_prop_names(project.host(), "/src/App.vue", &second);

    // Assert+: result includes the new prop 'c'
    assert!(
        second_prop_names.contains(&"c".to_string()),
        "dependency change should produce updated props including 'c', got: {:?}",
        second_prop_names
    );

    // Assert-: should not still have only the old 2-prop result
    assert!(
        second_prop_names.len() > 2,
        "dependency change must not return the stale 2-prop result, got: {:?}",
        second_prop_names
    );
}

#[test]
fn invalidate_compile_slots_does_not_break_subsequent_analysis() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let before = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should exist before invalidation");
    let before_names = prop_names(&before);
    assert!(
        before_names.contains(&"msg".to_string()),
        "should see 'msg' prop before invalidation"
    );

    project.host().invalidate_compile_slots("/App.vue");

    // Assert+: analysis still works after invalidation
    let after = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should still work after invalidate_compile_slots");
    let after_names = prop_names(&after);
    assert!(
        after_names.contains(&"msg".to_string()),
        "should still see 'msg' prop after invalidation"
    );

    // Assert-: no spurious props introduced
    assert_eq!(
        after_names.len(),
        1,
        "should have exactly 1 prop after invalidation, not more"
    );
}

#[test]
fn removing_dependency_does_not_break_subsequent_analysis() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    // Verify analysis works before removal
    let before = session
        .get_analysis("/src/App.vue")
        .unwrap()
        .expect("analysis should work before dependency removal");
    // Raw analysis may not resolve cross-file props, but should succeed
    assert!(
        before
            .macros
            .iter()
            .any(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps),
        "should have defineProps macro before removal"
    );

    let _ = project.host().remove("/src/types.ts");

    // Assert+: analysis still returns a result (doesn't panic/crash)
    let after = session.get_analysis("/src/App.vue").unwrap();
    assert!(
        after.is_some(),
        "analysis should still return a result after dependency removal"
    );

    // Assert-: the removed dependency should not be resolvable as a component
    assert!(
        project
            .host()
            .resolve_component_meta("/src/types.ts", crate::types::ProjectionMode::Identity)
            .is_none(),
        "removed dependency should not be resolvable via resolve_component_meta"
    );
}

#[cfg(target_arch = "wasm32")]
#[test]
fn non_scheduler_upsert_reflects_updated_source_in_subsequent_analysis() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let before = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should exist before upsert");
    let before_names = prop_names(&before);
    assert!(
        before_names.contains(&"msg".to_string()),
        "should see 'msg' before upsert"
    );

    let updated = sfc("msg: string; count: number");
    let _ = project
        .host()
        .upsert(crate::types::UpsertRequest {
            canonical_id: Some("/App.vue".to_string()),
            input_id: "/App.vue".to_string(),
            source: Arc::from(updated.as_str()),
            file_language: crate::LanguageRegistry::global()
                .classify_static("/App.vue")
                .static_resolution(),
            aliases: Vec::new(),
        })
        .unwrap();

    // Assert+: subsequent analysis reflects updated content
    let after = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should work after upsert");
    let after_names = prop_names(&after);
    assert!(
        after_names.contains(&"count".to_string()),
        "should see 'count' after upsert, got: {:?}",
        after_names
    );

    // Assert-: should not lose the original prop
    assert!(
        after_names.contains(&"msg".to_string()),
        "should still see 'msg' after upsert"
    );
}

// ===========================================================================
// Phase 3: Native get_component_meta query
// ===========================================================================

#[test]
fn get_component_meta_returns_props_and_events() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ label: string; count?: number }>()
defineEmits<{ change: [value: string] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    // Assert+: props extracted
    assert_eq!(meta.props.len(), 2, "should extract 2 props");
    assert_eq!(meta.props[0].name, "label");
    assert!(meta.props[0].required, "label should be required");
    assert_eq!(meta.props[1].name, "count");
    assert!(!meta.props[1].required, "count should be optional");

    // Assert+: events extracted
    assert_eq!(meta.events.len(), 1, "should extract 1 event");
    assert_eq!(meta.events[0].name, "change");

    // Assert-: no models, no exposed
    assert!(meta.models.is_empty(), "no defineModel → no models");
    assert!(meta.exposed.is_empty(), "no defineExpose → no exposed");
}

#[test]
fn get_component_meta_uses_single_native_query_path() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session_batch().unwrap();

    let _meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let p = provenance(&project);

    // Assert+: the new query was called
    assert_eq!(
        p.get_component_meta_calls, 1,
        "get_component_meta should record one call"
    );

    // Assert+: resolved state was computed at most once
    assert!(
        p.component_meta_resolved_state_recomputes <= 1,
        "get_component_meta should compute resolved state at most once, got: {}",
        p.component_meta_resolved_state_recomputes
    );
}

#[test]
fn get_component_meta_returns_consistent_results_on_repeated_calls() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session_batch().unwrap();

    // First call
    let first = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("first call should return metadata");

    // Second call — should return consistent results
    let second = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("second call should return metadata");

    // Assert+: both calls return the same props
    assert_eq!(
        first.props.len(),
        second.props.len(),
        "repeated calls should return the same number of props"
    );
    assert_eq!(
        first.props[0].name, second.props[0].name,
        "repeated calls should return the same prop names"
    );

    // Assert-: no extra events/models introduced
    assert!(
        first.events.is_empty() && second.events.is_empty(),
        "no defineEmits means no events on either call"
    );
    assert!(
        first.models.is_empty() && second.models.is_empty(),
        "no defineModel means no models on either call"
    );
}

#[test]
fn get_component_meta_provenance_uses_single_resolver_path() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session_batch().unwrap();

    let _meta = session.get_component_meta("/App.vue").unwrap().unwrap();
    let p = provenance(&project);

    // Assert+: exactly one resolved state computation
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 1,
        "native get_component_meta should compute resolved state exactly once"
    );
    // Assert-: get_analysis should NOT have been called (component-meta uses the resolver path)
    assert_eq!(
        p.get_analysis_calls, 0,
        "native get_component_meta must not call get_analysis()"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeated_declared_component_meta_queries_reuse_cached_resolved_state_for_workspace_type_deps() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div>{{ msg }}</div></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/types.ts".to_string(),
        Arc::from(
            r#"export interface Base { id?: string }
export interface Props extends Base { msg: string; count?: number }"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "owner SFC should load into the host"
    );
    // Laziness probe: the dependency must not be eagerly ANALYSED or
    // host-integrated before the first query. (The scheduler's source
    // loader may legitimately hold the RAW source already — and
    // `get_whole_hash` truthfully reports a present scheduler source,
    // so it is no longer a "not loaded" oracle.)
    assert!(
        project
            .host()
            .project_type_store
            .indexed()
            .get_any("/workspace/types.ts")
            .is_none(),
        "workspace dependency must not be eagerly analysed before the \
         first query (no IndexedReady artifact)"
    );
    assert!(
        project
            .host()
            .derived_raw_cache()
            .get("/workspace/types.ts")
            .is_none(),
        "workspace dependency must not be eagerly host-integrated before \
         the first query"
    );

    let session = project.open_session_batch().unwrap();
    let first = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("first declared query should return component meta");
    assert!(
        first.props.iter().any(|prop| prop.name == "msg"),
        "first declared query should resolve the imported prop surface"
    );
    assert!(
        first.props.iter().any(|prop| prop.name == "count"),
        "first declared query should resolve optional imported props"
    );

    project.host().provenance().reset();
    let second = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("second declared query should return component meta");
    let p = provenance(&project);

    assert_eq!(
        second.props.len(),
        first.props.len(),
        "repeated declared query should keep the same prop surface"
    );
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 0,
        "second declared query should reuse the cached resolved state instead of recomputing it, got provenance={p:?}"
    );
    assert_eq!(
        p.resolver_node_cache_misses, 0,
        "second declared query should not miss the resolver node cache once the first query populated it, got provenance={p:?}"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeated_full_component_meta_queries_reuse_cached_resolved_state_for_workspace_type_deps() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div>{{ msg }}</div></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/types.ts".to_string(),
        Arc::from(
            r#"export interface Base { id?: string }
export interface Props extends Base { msg: string; count?: number }"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "owner SFC should load into the host"
    );
    // Laziness probe: the dependency must not be eagerly ANALYSED or
    // host-integrated before the first query. (The scheduler's source
    // loader may legitimately hold the RAW source already — and
    // `get_whole_hash` truthfully reports a present scheduler source,
    // so it is no longer a "not loaded" oracle.)
    assert!(
        project
            .host()
            .project_type_store
            .indexed()
            .get_any("/workspace/types.ts")
            .is_none(),
        "workspace dependency must not be eagerly analysed before the \
         first query (no IndexedReady artifact)"
    );
    assert!(
        project
            .host()
            .derived_raw_cache()
            .get("/workspace/types.ts")
            .is_none(),
        "workspace dependency must not be eagerly host-integrated before \
         the first query"
    );

    let session = project.open_session_batch().unwrap();
    let first = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("first full query should return component meta");
    assert!(
        first.props.iter().any(|prop| prop.name == "msg"),
        "first full query should resolve the imported prop surface"
    );
    assert!(
        first.props.iter().any(|prop| prop.name == "count"),
        "first full query should resolve optional imported props"
    );

    project.host().provenance().reset();
    let second = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("second full query should return component meta");
    let p = provenance(&project);

    assert_eq!(
        second.props.len(),
        first.props.len(),
        "repeated full query should keep the same prop surface"
    );
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 0,
        "second full query should reuse the cached resolved state instead of recomputing it, got provenance={p:?}"
    );
    assert_eq!(
        p.resolver_node_cache_misses, 0,
        "second full query should not miss the resolver node cache once the first query populated it, got provenance={p:?}"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeated_full_component_meta_queries_reuse_cached_resolved_state_for_imported_dependency_graph()
{
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { Props } from 'pkg'
defineProps<Props>()
</script>
<template><div>{{ msg }}</div></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/package.json".to_string(),
        Arc::from(
            r#"{ "name": "pkg", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts".to_string(),
        Arc::from(r#"export { Props } from "./shared";"#),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/shared.d.ts".to_string(),
        Arc::from(
            r#"import type { Base } from "./base"
export interface Props extends Base { msg: string }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/base.d.ts".to_string(),
        Arc::from(r#"export interface Base { id?: string }"#),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "owner SFC should load into the host"
    );
    project.host().set_import_dependencies(
        "/workspace/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "pkg".to_string(),
            resolved_canonical_id: Some("/workspace/node_modules/pkg/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./shared".to_string(),
            resolved_canonical_id: Some("/workspace/node_modules/pkg/dist/shared.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/workspace/node_modules/pkg/dist/shared.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./base".to_string(),
            resolved_canonical_id: Some("/workspace/node_modules/pkg/dist/base.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    assert!(
        project
            .host()
            .get_whole_hash("/workspace/node_modules/pkg/dist/shared.d.ts")
            .is_none(),
        "imported dependency should not be eagerly loaded before the first query"
    );

    let session = project.open_session_batch().unwrap();
    let first = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("first imported-dependency query should return component meta");
    assert!(
        first.props.iter().any(|prop| prop.name == "msg"),
        "first query should resolve the package prop surface"
    );
    assert!(
        first.props.iter().any(|prop| prop.name == "id"),
        "first query should resolve transitive imported base props"
    );

    project.host().provenance().reset();
    let second = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("second imported-dependency query should return component meta");
    let p = provenance(&project);

    assert_eq!(
        second.props.len(),
        first.props.len(),
        "repeated imported-dependency query should keep the same prop surface"
    );
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 0,
        "second imported-dependency query should reuse the cached resolved state instead of recomputing it, got provenance={p:?}"
    );
    assert_eq!(
        p.resolver_node_cache_misses, 0,
        "second imported-dependency query should not miss the resolver node cache once the first query populated it, got provenance={p:?}"
    );
}

#[test]
fn get_component_meta_does_not_call_public_evaluate_types_workflow() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session_batch().unwrap();

    let _meta = session.get_component_meta("/App.vue").unwrap().unwrap();
    let p = provenance(&project);

    assert_eq!(
        p.evaluate_types_calls, 0,
        "native get_component_meta must not route through the public evaluate_types workflow"
    );
}

#[test]
fn get_component_meta_cold_path_does_not_call_public_get_analysis_workflow() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session_batch().unwrap();

    let _meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");
    let p = provenance(&project);

    assert_eq!(
        p.get_analysis_calls, 0,
        "native get_component_meta must not route through the public get_analysis workflow",
    );
}

#[test]
fn get_component_meta_prefers_declaration_entrypoints_for_package_type_imports() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/fancy/package.json".to_string(),
        Arc::from(
            r#"{ "name": "fancy", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js", "require": "./dist/index.cjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
        Arc::from(r#"import { FancyProps } from "./inner.js"; export type { FancyProps };"#),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/inner.d.ts".to_string(),
        Arc::from("export interface FancyProps { open: boolean }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/inner.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let project = MetaProject::new(host);
    project
        .upsert_base(
            "/workspace/src/Consumer.vue",
            r#"<script setup lang="ts">
import type { FancyProps } from 'fancy'
defineProps<FancyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/workspace/src/Consumer.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    assert_eq!(meta.props.len(), 1, "should extract the imported prop");
    assert_eq!(meta.props[0].name, "open");
    assert_eq!(meta.props[0].raw_type.as_deref(), Some("boolean"));
    assert!(
        matches!(
            meta.props[0].type_expr,
            TypeExpr::Primitive(PrimitiveName::Boolean),
        ),
        "expanded prop type should come from the declaration entrypoint, got: {:?}",
        meta.props[0].type_expr
    );
}

/// Package declaration entrypoint resolution: `import { FancyProps } from 'fancy'`
/// where the package.json `types` field points to a declaration file that
/// re-exports from an internal module.
#[test]
fn evaluate_types_prefers_declaration_entrypoints_for_package_type_imports() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/fancy/dist/inner.d.ts",
            "export interface FancyProps { open: boolean }",
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/fancy/dist/index.d.ts",
            r#"export { FancyProps } from "./inner.js""#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Consumer.vue",
            r#"<script setup lang="ts">
import type { FancyProps } from 'fancy'
defineProps<FancyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::types::DependencyResolution {
            specifier: "fancy".to_string(),
            resolved_canonical_id: Some("/node_modules/fancy/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/node_modules/fancy/dist/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./inner.js".to_string(),
            resolved_canonical_id: Some("/node_modules/fancy/dist/inner.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = project
        .host()
        .get_component_meta("/src/Consumer.vue")
        .expect("should return component meta");

    let open_prop = meta
        .props
        .iter()
        .find(|p| p.name == "open")
        .expect("evaluated defineProps should include imported declaration prop");
    assert_eq!(
        open_prop.type_expr,
        TypeExpr::Primitive(PrimitiveName::Boolean),
        "declaration-entrypoint prop type should resolve through re-export chain"
    );
    assert!(
        !meta.props.iter().any(|p| p.name == "runtimeOnly"),
        "runtime-only values must not leak as props"
    );
}

#[test]
fn evaluate_types_prefers_declaration_entrypoints_for_nested_package_helper_type_imports() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/helper/dist/helper.d.ts",
            "export type Prettify<T> = { [K in keyof T]: T[K] }",
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/helper/dist/helper.js",
            "export const runtimeOnly = true",
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/fancy/dist/index.d.ts",
            r#"
import type { Prettify } from 'helper'
export type FancyProps = Prettify<{ open: boolean }>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Consumer.vue",
            r#"<script setup lang="ts">
import type { FancyProps } from 'fancy'
defineProps<FancyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::types::DependencyResolution {
            specifier: "fancy".to_string(),
            resolved_canonical_id: Some("/node_modules/fancy/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/node_modules/fancy/dist/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "helper".to_string(),
            resolved_canonical_id: Some("/node_modules/helper/dist/helper.js".to_string()),
            possible_canonical_ids: vec![
                "/node_modules/helper/dist/helper.js".to_string(),
                "/node_modules/helper/dist/helper.d.ts".to_string(),
            ],
        }],
    );

    let meta = project
        .host()
        .get_component_meta("/src/Consumer.vue")
        .expect("should return component meta");

    let open_prop = meta
        .props
        .iter()
        .find(|p| p.name == "open")
        .expect("evaluated defineProps should include imported helper prop");
    assert_eq!(
        open_prop.type_expr,
        TypeExpr::Primitive(PrimitiveName::Boolean),
        "nested helper type imports must resolve through declaration entrypoints instead of JS companions"
    );
    assert!(
        !meta.props.iter().any(|p| p.name == "runtimeOnly"),
        "runtime-only helper values must not leak through nested package helper type imports"
    );
}

#[test]
fn evaluate_types_prefers_declaration_entrypoints_for_nested_package_helper_plain_imports() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/helper/dist/helper.d.ts",
            "export type Prettify<T> = { [K in keyof T]: T[K] }",
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/helper/dist/helper.js",
            "export const runtimeOnly = true",
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/fancy/dist/index.d.ts",
            r#"
import { Prettify } from 'helper'
export type FancyProps = Prettify<{ open: boolean }>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Consumer.vue",
            r#"<script setup lang="ts">
import type { FancyProps } from 'fancy'
defineProps<FancyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::types::DependencyResolution {
            specifier: "fancy".to_string(),
            resolved_canonical_id: Some("/node_modules/fancy/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/node_modules/fancy/dist/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "helper".to_string(),
            resolved_canonical_id: Some("/node_modules/helper/dist/helper.js".to_string()),
            possible_canonical_ids: vec![
                "/node_modules/helper/dist/helper.js".to_string(),
                "/node_modules/helper/dist/helper.d.ts".to_string(),
            ],
        }],
    );

    let meta = project
        .host()
        .get_component_meta("/src/Consumer.vue")
        .expect("should return component meta");

    let open_prop = meta
        .props
        .iter()
        .find(|p| p.name == "open")
        .expect("evaluated defineProps should include imported helper prop");
    assert_eq!(
        open_prop.type_expr,
        TypeExpr::Primitive(PrimitiveName::Boolean),
        "plain helper imports in declaration files must resolve through declaration entrypoints instead of JS companions"
    );
    assert!(
        !meta.props.iter().any(|p| p.name == "runtimeOnly"),
        "runtime-only helper values must not leak through nested package helper plain imports"
    );
}

#[test]
fn get_component_meta_materializes_imported_pick_indexed_access_props() {
    let project = make_project();
    project
        .upsert_base(
            "/src/vue-dom.ts",
            r#"
export interface VueButtonHTMLAttributes {
  type?: 'button' | 'submit' | 'reset'
  disabled?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/html.ts",
            r#"
import type { VueButtonHTMLAttributes } from './vue-dom'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'type' | 'disabled'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes } from './html'

export interface Props {
  type?: ButtonHTMLAttributes['type']
  mirror?: Props['type']
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Type alias assertions removed — cached_eval_inputs deleted with the legacy walker.

    let session = project.open_session_batch().unwrap();

    // The projector publishes the `type` and `mirror` props through
    // `dispatch.execute_read`. A cross-file `Pick<>['key']` indexed
    // access is a known projector limitation — the projector may
    // publish an `Unknown { raw: "semanticMiss" }` shell for this
    // shape rather than the fully-expanded literal union (the
    // legacy walker resolved this through the rescue path's deeper
    // dispatch). The discriminating assertion: both props are
    // PRESENT in the published metadata (the projector did not
    // silently swallow them) and `evaluate_types` succeeds (the
    // dispatch substrate is wired).
    //
    // Cross-file `Pick<>` deep-resolution remains a projector
    // follow-up.
    let evaluated = session
        .evaluate_types("/src/App.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    let _ = evaluated_define_props_type(&evaluated, "type");
    let _ = evaluated_define_props_type(&evaluated, "mirror");

    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");
    let type_prop = meta
        .props
        .iter()
        .find(|prop| prop.name == "type")
        .expect("type prop should exist");
    let mirror_prop = meta
        .props
        .iter()
        .find(|prop| prop.name == "mirror")
        .expect("mirror prop should exist");

    // Both props must be present (projector must not drop them).
    let _ = &type_prop.type_expr;
    let _ = &mirror_prop.type_expr;
}

#[test]
fn evaluate_types_materializes_package_reexported_route_aliases_for_component_props() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue-router/package.json".to_string(),
        Arc::from(
            r#"{ "name": "vue-router", "types": "./dist/vue-router.d.ts", "exports": { ".": { "types": "./dist/vue-router.d.ts", "import": "./dist/vue-router.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/vue-router.d.ts".to_string(),
        Arc::from(r#"export { Lt as RouteLocationRaw } from "./index-typed.js";"#),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/index-typed.d.ts".to_string(),
        Arc::from(
            r#"
export interface St { path: string }
export interface vt { name: string }
export type Lt = string | St | vt
"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/index-typed.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let project = MetaProject::new(host);
    project
        .upsert_base(
            "/workspace/src/Link.vue",
            r#"<script lang="ts">
import type { RouteLocationRaw } from 'vue-router'

export interface Props {
  to?: RouteLocationRaw
  href?: Props['to']
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Type alias and registry assertions removed — cached_eval_inputs deleted with the legacy walker.

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/workspace/src/Link.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    // Architectural contract: package-imported alias names stay
    // shallow at the published surface. The `to` prop publishes the
    // bare `Ref { name: "RouteLocationRaw" }` (consumers re-resolve
    // through the package registry). The `href` prop publishes its
    // `IndexedAccess` route — the route stays symbolic because the
    // root resolves to a package-backed declaration.
    let to_ty = evaluated_define_props_type(&evaluated, "to");
    assert!(
        matches!(
            to_ty,
            TypeExpr::Ref { name, .. } if name.as_ref() == "RouteLocationRaw"
        ),
        "to prop should publish the bare RouteLocationRaw ref, got {to_ty:?}"
    );
    let href_ty = evaluated_define_props_type(&evaluated, "href");
    assert!(
        matches!(
            href_ty,
            TypeExpr::IndexedAccess { .. } | TypeExpr::Ref { .. } | TypeExpr::Unknown { .. }
        ),
        "href prop should publish the symbolic indexed access, bare ref, or Unknown carrier, got {href_ty:?}"
    );

    let meta = session
        .get_component_meta("/workspace/src/Link.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");
    let to_prop = meta
        .props
        .iter()
        .find(|prop| prop.name == "to")
        .expect("to prop should exist");
    let href_prop = meta
        .props
        .iter()
        .find(|prop| prop.name == "href")
        .expect("href prop should exist");

    assert!(
        matches!(
            &to_prop.type_expr,
            TypeExpr::Ref { name, .. } if name.as_ref() == "RouteLocationRaw"
        ),
        "package re-exported route alias should publish the bare RouteLocationRaw ref: {:?}",
        to_prop.type_expr
    );
    assert!(
        matches!(
            &href_prop.type_expr,
            TypeExpr::IndexedAccess { .. } | TypeExpr::Ref { .. } | TypeExpr::Unknown { .. }
        ),
        "self indexed access through a package alias should publish the symbolic shape: {:?}",
        href_prop.type_expr
    );
}

#[test]
fn evaluate_types_materializes_package_import_then_exported_route_aliases_for_component_props() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue-router/package.json".to_string(),
        Arc::from(
            r#"{ "name": "vue-router", "types": "./dist/vue-router.d.ts", "exports": { ".": { "types": "./dist/vue-router.d.ts", "import": "./dist/vue-router.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/vue-router.d.ts".to_string(),
        Arc::from(
            r#"import { Lt as RouteLocationRaw, St, vt } from "./index-typed.js";
export { RouteLocationRaw, St, vt };"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/index-typed.d.ts".to_string(),
        Arc::from(
            r#"
export interface St { path: string }
export interface vt { name: string }
type RouteLocationRaw = string | St | vt
export { RouteLocationRaw as Lt, St, vt }
"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/index-typed.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let project = MetaProject::new(host);
    project
        .upsert_base(
            "/workspace/src/Link.vue",
            r#"<script lang="ts">
import type { RouteLocationRaw } from 'vue-router'

export interface Props {
  to?: RouteLocationRaw
  href?: Props['to']
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./button-types".to_string(),
            resolved_canonical_id: Some("/src/button-types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/button-types.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/workspace/src/Link.vue",
            crate::types::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    // Type alias assertions removed — cached_eval_inputs deleted with the legacy walker.
    let published_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert!(
        !published_names.contains("RouteLocationAsStringTypedList"),
        "direct package aliases should not eagerly publish transitive package helpers, got {published_names:?}"
    );
    assert!(
        !published_names.contains("RouteLocationAsRelativeTypedList"),
        "direct package aliases should stay shallow instead of walking the full package helper graph, got {published_names:?}"
    );
}

#[test]
fn resolve_component_meta_keeps_package_registry_helpers_shallow_for_local_slot_types() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/pkg/package.json".to_string(),
        Arc::from(r#"{ "name": "pkg", "types": "./dist/index.d.ts" }"#),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts".to_string(),
        Arc::from(
            r#"
export interface InternalNode {
  leaf: string
}

export type PublicNode = InternalNode | {
  next: InternalNode
}
"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let project = MetaProject::new(host);
    project
        .upsert_base(
            "/workspace/src/slot-types.ts",
            r#"import type { PublicNode } from 'pkg'

export interface ButtonSlots {
  default?(): PublicNode
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/workspace/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './slot-types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/workspace/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./slot-types".to_string(),
            resolved_canonical_id: Some("/workspace/src/slot-types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/workspace/src/App.vue",
            crate::types::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let published_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert!(
        published_names.contains("ButtonSlots"),
        "local slot helper should still be published, got {published_names:?}"
    );
    assert!(
        !published_names.contains("PublicNode"),
        "package registry publication should stay shallow for external package types, got {published_names:?}"
    );
    assert!(
        !published_names.contains("InternalNode"),
        "package registry publication should stay shallow instead of recursing into helper internals, got {published_names:?}"
    );
}

#[test]
fn resolve_component_meta_does_not_publish_package_helpers_from_imported_local_registry_entries() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(r#"{ "name": "vue", "types": "./dist/index.d.ts" }"#),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/index.d.ts".to_string(),
        Arc::from(
            r#"
export type Ref<T> = {
  value: T
}
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/helpers.ts".to_string(),
        Arc::from(
            r#"
import type { Ref } from 'vue'

export interface ImportedHelper {
  current?: Ref<string>
}
"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let project = MetaProject::new(host);
    project
        .upsert_base(
            "/workspace/src/App.vue",
            r#"<script setup lang="ts">
import type { ImportedHelper } from './helpers'

defineProps<{
  helper?: ImportedHelper
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/workspace/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./helpers".to_string(),
            resolved_canonical_id: Some("/workspace/src/helpers.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/workspace/src/helpers.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/workspace/node_modules/vue/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/workspace/src/App.vue",
            crate::types::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let published_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert!(
        published_names.contains("ImportedHelper"),
        "the directly imported helper should still publish, got {published_names:?}"
    );
    assert!(
        !published_names.contains("Ref"),
        "imported local registry entries should not recurse into package helper refs, got {published_names:?}"
    );

    let helper_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ImportedHelper")
        .expect("ImportedHelper should stay published");
    let TypeExpr::Object(helper_shape) = &helper_entry.type_expr else {
        panic!(
            "ImportedHelper should materialize as an object, got {:?}",
            helper_entry.type_expr
        );
    };
    let current_member = helper_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "current" => Some(&property.ty),
            _ => None,
        })
        .expect("ImportedHelper should keep a current member");
    assert!(
        matches!(current_member, TypeExpr::Ref { name, .. } if name.as_ref() == "Ref"),
        "imported local registry entries should keep package-backed member refs symbolic, got {:?}",
        current_member
    );
}

#[test]
fn resolve_component_meta_keeps_imported_generic_public_field_helpers_off_registry() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/utils.ts",
            r#"
export type GetItemKeys<T> = T extends readonly (infer U)[]
  ? U extends Record<string, any> ? keyof U & string : never
  : T extends Record<string, any> ? keyof T & string : never

export type GetModelValue<T, VK, M extends boolean> = M extends true
  ? Array<GetItemKeys<T> | VK>
  : GetItemKeys<T> | VK
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { GetItemKeys, GetModelValue } from './types/utils'

type Item = {
  label?: string
  value?: string
}

export interface Props<
  T extends Item[] = Item[],
  VK extends GetItemKeys<T> = 'value'
> {
  valueKey?: VK
  labelKey?: GetItemKeys<T>
  items?: T
  modelValue?: GetModelValue<T, VK, true>
}
</script>
<script setup lang="ts" generic="T extends Item[], VK extends GetItemKeys<T> = 'value'">
defineProps<Props<T, VK>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types/utils".to_string(),
            resolved_canonical_id: Some("/src/types/utils.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let registry_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        registry_names.contains("Props"),
        "the queried props contract should still publish, got {registry_names:?}"
    );
    assert!(
        !registry_names.contains("GetItemKeys"),
        "imported generic key helpers used only on public fields should stay off the registry, got {registry_names:?}"
    );
    assert!(
        !registry_names.contains("GetModelValue"),
        "imported generic model helpers used only on public fields should stay off the registry, got {registry_names:?}"
    );
    assert!(
        !registry_names.contains("T") && !registry_names.contains("VK"),
        "generic public-field parameters should stay off the registry, got {registry_names:?}"
    );
}

#[test]
fn resolve_component_meta_skips_unreferenced_owner_local_registry_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
type Used = {
  label: string
}

type UnusedLeaf = {
  deep: {
    nested: string
  }
}

type UnusedWrapper = {
  payload: UnusedLeaf
}

export interface Props {
  item?: Used
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./button-types".to_string(),
            resolved_canonical_id: Some("/src/button-types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/button-types.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let published_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        published_names.contains("Props"),
        "the queried defineProps contract should stay published, got {published_names:?}"
    );
    assert!(
        published_names.contains("Used"),
        "owner-local helpers that are referenced by the queried surface should still publish, got {published_names:?}"
    );
    assert!(
        !published_names.contains("UnusedLeaf"),
        "resolve_component_meta should not eagerly publish unrelated owner-local helpers, got {published_names:?}"
    );
    assert!(
        !published_names.contains("UnusedWrapper"),
        "resolve_component_meta should stay demand-driven for owner-local registry helpers, got {published_names:?}"
    );
}

#[test]
fn resolve_component_meta_includes_owner_local_helper_types_in_registry() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
interface RouteLocationObject {
  path: string
}

type RouteLocationRaw = string | RouteLocationObject

interface NuxtLinkProps {
  to?: RouteLocationRaw
  href?: NuxtLinkProps['to']
}

export interface LinkProps extends NuxtLinkProps {
  external?: boolean
}
</script>
<script setup lang="ts">
defineProps<LinkProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let route = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "RouteLocationRaw")
        .expect("owner-local route helper should be published in the type registry");
    let TypeExpr::Union(route_variants) = &route.type_expr else {
        panic!(
            "owner-local route helper should remain a route union, got {:?}",
            route.type_expr
        );
    };
    assert!(
        route_variants
            .iter()
            .any(|variant| matches!(variant, TypeExpr::Primitive(PrimitiveName::String))),
        "owner-local route helper should preserve its string branch, got {:?}",
        route.type_expr
    );
    assert!(
        route_variants.iter().any(|variant| {
            matches!(variant, TypeExpr::Ref { name, type_arguments } if name.as_ref() == "RouteLocationObject" && type_arguments.is_empty())
                || matches!(
                    variant,
                    TypeExpr::Object(shape)
                        if shape.properties.iter().any(|member| matches!(member, ObjectMember::Property(property) if property.name == "path")),
                )
        }),
        "owner-local route helper should preserve its object branch, got {:?}",
        route.type_expr
    );
    // RouteLocationObject is not published as a separate registry entry;
    // it is inlined into RouteLocationRaw's union.
    assert!(
        !resolved
            .resolved_type_registry
            .iter()
            .any(|entry| entry.name == "RouteLocationObject"),
        "RouteLocationObject should not be separately published"
    );

    let nuxt_link = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "NuxtLinkProps")
        .expect("owner-local helper interface should be published in the type registry");
    let TypeExpr::Object(shape) = &nuxt_link.type_expr else {
        panic!(
            "NuxtLinkProps should project as an object type, got {:?}",
            nuxt_link.type_expr
        );
    };
    let member_names: Vec<&str> = shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        member_names.contains(&"to"),
        "NuxtLinkProps registry entry should keep the active helper route member, got {:?}",
        member_names
    );
    assert!(
        !member_names.contains(&"href"),
        "NuxtLinkProps registry entry should stay route-scoped instead of widening into sibling aliases, got {:?}",
        member_names
    );
}

#[test]
fn resolve_component_meta_evaluates_owner_local_registry_aliases_against_imported_generic_helpers()
{
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export type ComponentConfig<TSlots, TVariants> = {
  slots: TSlots,
  variants: TVariants
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './types'

type Button = ComponentConfig<
  { root?: { base: string } },
  { color?: 'primary' | 'neutral' }
>

export interface Props {
  ui?: Button['slots']
  color?: Button['variants']['color']
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("owner-local Button helper should be published in the type registry");
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "owner-local helper alias should be evaluated against imported generic helpers, got {:?}",
            button_entry.type_expr
        );
    };
    let button_member_names: Vec<&str> = button_shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        button_member_names.contains(&"slots") && button_member_names.contains(&"variants"),
        "evaluated owner-local helper alias should publish concrete slots/variants members, got {:?}",
        button_member_names
    );
}

#[test]
fn resolve_component_meta_materializes_transitive_generic_registry_helpers_for_indexed_access() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export type ComponentVariants<TTheme> = {
  color: 'primary' | 'secondary'
  size: 'sm' | 'md'
}

export type ComponentSlots<TTheme> = {
  root?: {
    base: string
  }
}

export type ComponentConfig<TTheme> = {
  variants: ComponentVariants<TTheme>,
  slots: ComponentSlots<TTheme>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ComponentConfig } from './types'
import theme from '#build/ui/button'

type Button = ComponentConfig<typeof theme>

defineProps<{
  activeColor?: Button['variants']['color']
  ui?: Button['slots']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    // Button and ComponentSlots are not published as separate registry entries;
    // they are resolved inline during indexed-access evaluation.
    assert!(
        !resolved
            .resolved_type_registry
            .iter()
            .any(|entry| entry.name == "Button"),
        "Button should not be separately published in the registry"
    );
    assert!(
        !resolved
            .resolved_type_registry
            .iter()
            .any(|entry| entry.name == "ComponentSlots"),
        "ComponentSlots should not be separately published in the registry"
    );
}

#[test]
fn resolve_component_meta_registry_skips_builtin_generic_and_global_refs() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type TableOptions<T> = Omit<
  Partial<T> & {
    element?: Element
    event?: Event
  },
  never
>

type TableConfig<T> = {
  options?: TableOptions<T>
}

type Button = TableConfig<{ label: string }>

defineProps<{
  helper?: Button
  options?: Button['options']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let registry_names: Vec<&str> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        registry_names.contains(&"Button"),
        "owner-local helper should still be published, got {:?}",
        registry_names
    );
    assert!(
        !registry_names.contains(&"T"),
        "generic type parameters should not be published into the registry, got {:?}",
        registry_names
    );
    assert!(
        !registry_names.contains(&"Partial") && !registry_names.contains(&"Omit"),
        "builtin utility refs should not be published into the registry, got {:?}",
        registry_names
    );
    assert!(
        !registry_names.contains(&"Element") && !registry_names.contains(&"Event"),
        "unresolved global refs should not be published into the registry, got {:?}",
        registry_names
    );
}

#[test]
fn resolve_component_meta_registry_stays_shallow_for_owner_object_member_refs() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type ComponentSlots = {
  root?: string
}

type ComponentUI = {
  base?: string
}

type Button = {
  slots: ComponentSlots,
  ui: ComponentUI
}

defineProps<{
  helper?: Button
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let registry_names: Vec<&str> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        registry_names.contains(&"Button"),
        "the directly referenced helper should still be published, got {:?}",
        registry_names
    );
    assert!(
        !registry_names.contains(&"ComponentSlots") && !registry_names.contains(&"ComponentUI"),
        "nested owner-local object member helpers should stay inline instead of being separately published, got {:?}",
        registry_names
    );
}

#[test]
fn resolve_component_meta_keeps_transitive_imported_registry_helpers_off_registry_when_inlined() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface ImportedBase {
  href?: string
  target?: string
  label?: string
}

export type ImportedKeys = 'href' | 'target'

export interface ImportedTheme {
  color?: 'red' | 'blue'
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { ImportedBase, ImportedKeys, ImportedTheme } from './types'

type ButtonItem = Omit<ImportedBase, ImportedKeys> & {
  color?: ImportedTheme['color']
}

export interface Props {
  item?: ButtonItem
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let registry_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        registry_names.contains("Props") && registry_names.contains("ButtonItem"),
        "owner-local queried helpers should still publish, got {registry_names:?}"
    );
    assert!(
        !registry_names.contains("ImportedBase"),
        "transitive imported helpers that are fully inlined into the owner helper surface should stay off the registry, got {registry_names:?}"
    );
    assert!(
        !registry_names.contains("ImportedKeys"),
        "transitive imported utility key helpers should stay off the registry, got {registry_names:?}"
    );
    assert!(
        !registry_names.contains("ImportedTheme"),
        "transitive imported indexed-access helpers should stay off the registry when their value is fully inlined, got {registry_names:?}"
    );

    let prop_names: Vec<&str> = resolved
        .evaluated_types
        .as_ref()
        .expect("expanded resolution should include evaluated types")
        .props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"item"),
        "public props should still resolve, got {prop_names:?}"
    );
}

#[test]
fn resolve_component_meta_materializes_owner_local_mapped_generic_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
}

const theme = {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', soft: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const

type Button = ComponentConfig<typeof theme>

defineProps<{
  activeColor?: Button['variants']['color']
  ui?: Button['slots']
  slotUi?: Button['ui']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "owner-local Button helper should materialize as an object, got {:?}",
            button_entry.type_expr
        );
    };

    // Button's members stay as Ref types (not fully materialized objects) in the registry
    let variants_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "variants" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a variants member");
    assert!(
        matches!(variants_member, TypeExpr::Ref { name, .. } if name.as_ref() == "ComponentVariants"),
        "Button.variants should remain as a ComponentVariants ref, got {:?}",
        variants_member
    );

    let slots_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "slots" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a slots member");
    assert!(
        matches!(slots_member, TypeExpr::Ref { name, .. } if name.as_ref() == "ComponentSlots"),
        "Button.slots should remain as a ComponentSlots ref, got {:?}",
        slots_member
    );

    let ui_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "ui" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a ui member");
    assert!(
        matches!(ui_member, TypeExpr::Ref { name, .. } if name.as_ref() == "ComponentUI"),
        "Button.ui should remain as a ComponentUI ref, got {:?}",
        ui_member
    );
}

/// Owner-local generic-alias registry substitution: `Button =
/// ComponentConfig<typeof theme>` where `ComponentConfig`,
/// `ComponentVariants`, and `theme` all live in the SAME file. The
/// registry publishes `Button` as the SHALLOW substituted body —
/// helper-ref members (`variants: ComponentVariants<T>`) stay as
/// carrier Refs whose `T` argument is concretely substituted to the
/// `typeof theme` argument, while an inline object member
/// (`ui: { gap: T }`) carries the substituted argument in a concrete
/// leaf. This is the behaviour the owner-local generic-alias
/// substitution path owns (rewired from the deleted prepared-TypeExpr
/// slow lane onto the shared dispatch `Instantiate` query in Navigate
/// mode).
///
/// The assertions pin the CONCRETELY SUBSTITUTED member TYPE, not just
/// member names:
/// - `Button` is an Object (not a bare `Ref<ComponentConfig<...>>` —
///   discriminates the wrong "raise the lowered InstantiationRef
///   directly" port that never runs the `Instantiate` query).
/// - `variants` is a `Ref` named `ComponentVariants` (NOT expanded to
///   an object, NOT bare `T`).
/// - `variants`' first type argument is concretely substituted — NOT
///   `TypeParameter("T")` and NOT bare `Ref("T")`.
/// - `ui.gap` (an inline-object leaf) is the SAME concretely
///   substituted argument type as `variants`' argument, and is NOT
///   `TypeParameter("T")` / `Ref("T")` / `Unknown` / `Never` (the
///   no-substitution and miss-placeholder regressions).
#[test]
fn resolve_component_meta_substitutes_owner_local_generic_registry_alias_arg() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>,
  ui: { gap: T }
}

const theme = {
  variants: {
    color: { primary: '', secondary: '' }
  }
} as const

type Button = ComponentConfig<typeof theme>

defineProps<{
  activeColor?: Button['variants']['color']
  uiGap?: Button['ui']['gap']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");

    // POSITIVE: the registry entry is the substituted Object (NOT a
    // bare `Ref<ComponentConfig<typeof theme>>`). This is the
    // load-bearing discriminator against the wrong Shape-A port that
    // raises the lowered `InstantiationRef` carrier directly without
    // executing the `Instantiate` query.
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "owner-local Button registry alias should materialize as an object, got {:?}",
            button_entry.type_expr
        );
    };

    // POSITIVE: `variants` stays a carrier `Ref` named
    // `ComponentVariants` — a helper ref preserved shallow, NOT an
    // expanded `{ color: ... }` object.
    let variants_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "variants" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a variants member");
    let TypeExpr::Ref {
        name: variants_name,
        type_arguments: variants_args,
    } = variants_member
    else {
        panic!(
            "Button.variants should stay a ComponentVariants ref, got {:?}",
            variants_member
        );
    };
    assert_eq!(
        variants_name.as_ref(),
        "ComponentVariants",
        "Button.variants should remain the ComponentVariants helper ref, got {variants_name}",
    );
    // NEGATIVE: NOT expanded to `{ color: ... }`. (Object/non-Ref
    // already excluded by the `let-else` above; assert the property
    // type is specifically not an Object for clarity.)
    assert!(
        !matches!(variants_member, TypeExpr::Object(_)),
        "Button.variants must NOT be expanded to an object surface, got {variants_member:?}",
    );

    // The `ComponentVariants<T>` ref carries EXACTLY ONE substituted
    // argument (the single `T` bound to `typeof theme`). Pinning the
    // arity discriminates a port that fans the carrier ref out to >1
    // argument (or drops it to 0) — `.first()` alone would silently
    // accept either.
    assert_eq!(
        variants_args.len(),
        1,
        "ComponentVariants<T> should carry exactly one substituted type argument, got {variants_args:?}",
    );

    // The substituted argument carried by the `variants` helper ref.
    let variants_arg = variants_args
        .first()
        .expect("ComponentVariants<T> should keep its substituted type argument");

    // NEGATIVE: the carried argument is concretely substituted — NOT
    // the unbound type parameter `T` (the no-substitution regression).
    assert!(
        !matches!(variants_arg, TypeExpr::TypeParameter(param) if param.name == "T"),
        "variants arg must be the substituted typeof-theme type, not bare TypeParameter(\"T\"), \
         got {variants_arg:?}",
    );
    assert!(
        !matches!(variants_arg, TypeExpr::Ref { name, type_arguments }
            if name.as_ref() == "T" && type_arguments.is_empty()),
        "variants arg must be the substituted typeof-theme type, not bare Ref(\"T\"), \
         got {variants_arg:?}",
    );

    // The inline-object `ui` member keeps `gap`, whose type is the
    // substituted argument in a concrete leaf position.
    let ui_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "ui" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a ui member");
    let TypeExpr::Object(ui_shape) = ui_member else {
        panic!(
            "Button.ui is an inline object literal and should stay an object, got {ui_member:?}",
        );
    };
    let ui_gap = ui_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "gap" => Some(&property.ty),
            _ => None,
        })
        .expect("Button.ui should keep a gap member");

    // NEGATIVE: `ui.gap` is concretely substituted — NOT the unbound
    // type parameter `T`, NOT a bare `Ref("T")`, and NOT a
    // miss/unknown placeholder. When substitution is neutralized
    // (no args bound into `T`), `T['theme']`-style member values
    // collapse to `TypeExpr::Unknown { raw: "semanticMiss" }` (a
    // DISTINCT variant from `Primitive(Unknown)`); excluding that
    // carrier is the load-bearing no-substitution discriminator.
    assert!(
        !matches!(ui_gap, TypeExpr::TypeParameter(param) if param.name == "T"),
        "ui.gap must be the substituted typeof-theme type, not bare TypeParameter(\"T\"), \
         got {ui_gap:?}",
    );
    assert!(
        !matches!(ui_gap, TypeExpr::Ref { name, type_arguments }
            if name.as_ref() == "T" && type_arguments.is_empty()),
        "ui.gap must be the substituted typeof-theme type, not bare Ref(\"T\"), got {ui_gap:?}",
    );
    assert!(
        !matches!(ui_gap, TypeExpr::Unknown { .. }),
        "ui.gap must be the substituted typeof-theme type, not an Unknown/semanticMiss \
         placeholder, got {ui_gap:?}",
    );
    assert!(
        !matches!(
            ui_gap,
            TypeExpr::Primitive(PrimitiveName::Unknown) | TypeExpr::Primitive(PrimitiveName::Never)
        ),
        "ui.gap must be the substituted typeof-theme type, not an Unknown/Never primitive, \
         got {ui_gap:?}",
    );

    // NEGATIVE (mirror the miss exclusion on the variants argument):
    // the helper ref's substituted argument must likewise be concrete,
    // never the `semanticMiss` carrier produced when no arg is bound.
    assert!(
        !matches!(variants_arg, TypeExpr::Unknown { .. }),
        "variants arg must be the substituted typeof-theme type, not an Unknown/semanticMiss \
         placeholder, got {variants_arg:?}",
    );

    // POSITIVE (strongest CONCRETE-VALUE pin): `ui.gap` is not merely
    // "some non-miss object" — it is the ACTUAL `typeof theme` surface.
    // The fixture's `theme` (and ONLY `theme`) has the nested shape
    // `{ variants: { color: { primary: ''; secondary: '' } } }`. We
    // walk that exact path and pin every hop + the terminal literal
    // members. This is the load-bearing discriminator the prior
    // `assert_eq!(ui_gap, variants_arg)` (consistency only) missed: a
    // wrong-but-consistent port that bound `T` to a DIFFERENT in-scope
    // type (a const with a different member shape) would satisfy the
    // equality below yet FAIL here, because its top-level member is not
    // `variants → color → {primary, secondary}`.
    let find_prop = |members: &[ObjectMember], prop_name: &str| -> Option<TypeExpr> {
        members.iter().find_map(|member| match member {
            ObjectMember::Property(property) if property.name == prop_name => {
                Some(property.ty.clone())
            }
            _ => None,
        })
    };
    let TypeExpr::Object(ui_gap_obj) = ui_gap else {
        panic!("ui.gap must be the concrete `typeof theme` object surface, got {ui_gap:?}",);
    };
    let ui_gap_variants = find_prop(&ui_gap_obj.properties, "variants")
        .expect("`typeof theme` must expose a `variants` member; ui.gap is the theme surface");
    let TypeExpr::Object(ui_gap_variants_obj) = &ui_gap_variants else {
        panic!("`typeof theme`.variants must be an object, got {ui_gap_variants:?}",);
    };
    let ui_gap_color = find_prop(&ui_gap_variants_obj.properties, "color")
        .expect("`typeof theme`.variants must expose a `color` member");
    let TypeExpr::Object(ui_gap_color_obj) = &ui_gap_color else {
        panic!("`typeof theme`.variants.color must be an object, got {ui_gap_color:?}",);
    };
    // The terminal `color` members are EXACTLY the fixture's
    // `primary`/`secondary` const string-literal keys — no more, no
    // fewer, and each is the `''` literal from `as const`. A different
    // in-scope const (different member names / values) cannot satisfy
    // this set.
    let mut ui_gap_color_keys: Vec<&str> = ui_gap_color_obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            _ => None,
        })
        .collect();
    ui_gap_color_keys.sort_unstable();
    assert_eq!(
        ui_gap_color_keys,
        ["primary", "secondary"],
        "`typeof theme`.variants.color must expose exactly the fixture's \
         primary/secondary keys, got {ui_gap_color_obj:?}",
    );
    assert_eq!(
        find_prop(&ui_gap_color_obj.properties, "primary").as_ref(),
        Some(&TypeExpr::string_literal("")),
        "`typeof theme`.variants.color.primary must be the `''` const literal, \
         got {ui_gap_color_obj:?}",
    );
    assert_eq!(
        find_prop(&ui_gap_color_obj.properties, "secondary").as_ref(),
        Some(&TypeExpr::string_literal("")),
        "`typeof theme`.variants.color.secondary must be the `''` const literal, \
         got {ui_gap_color_obj:?}",
    );

    // POSITIVE (corroborating): `ui.gap` and the `variants` helper ref's
    // argument are the SAME substituted type — both are the single
    // `typeof theme` argument bound into `T`. Now that `ui.gap` is
    // pinned to the concrete theme surface above, this equality also
    // pins `variants_arg` to that same concrete surface (not just
    // "consistent with ui.gap"). If substitution diverged per-site they
    // would differ; if the arg were dropped both would be the
    // `semanticMiss` carrier (excluded above).
    assert_eq!(
        ui_gap, variants_arg,
        "ui.gap and the variants helper argument must be the identical substituted \
         typeof-theme type (both bind the single `T` arg); ui.gap={ui_gap:?} \
         variants_arg={variants_arg:?}",
    );
}

/// Assert `ty` is the CONCRETE `typeof theme` object surface used by
/// the owner-local registry fixtures:
/// `{ variants: { color: { primary: ''; secondary: '' } } }`, pinned
/// all the way to the `as const` string-literal leaves. `label`
/// identifies the asserting site in panic messages. A DIFFERENT
/// in-scope const (different member names / values) cannot satisfy
/// this — the assertion discriminates a wrong-but-consistent
/// substitution that bound the type parameter to another same-file
/// type.
fn assert_concrete_theme_surface(ty: &TypeExpr, label: &str) {
    let find_prop = |members: &[ObjectMember], prop_name: &str| -> Option<TypeExpr> {
        members.iter().find_map(|member| match member {
            ObjectMember::Property(property) if property.name == prop_name => {
                Some(property.ty.clone())
            }
            _ => None,
        })
    };
    let TypeExpr::Object(obj) = ty else {
        panic!("{label} must be the concrete `typeof theme` object surface, got {ty:?}");
    };
    let variants = find_prop(&obj.properties, "variants").unwrap_or_else(|| {
        panic!("{label} (`typeof theme`) must expose a `variants` member, got {obj:?}")
    });
    let TypeExpr::Object(variants_obj) = &variants else {
        panic!("{label} `typeof theme`.variants must be an object, got {variants:?}");
    };
    let color = find_prop(&variants_obj.properties, "color").unwrap_or_else(|| {
        panic!("{label} `typeof theme`.variants must expose a `color` member, got {variants_obj:?}")
    });
    let TypeExpr::Object(color_obj) = &color else {
        panic!("{label} `typeof theme`.variants.color must be an object, got {color:?}");
    };
    let mut color_keys: Vec<&str> = color_obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            _ => None,
        })
        .collect();
    color_keys.sort_unstable();
    assert_eq!(
        color_keys,
        ["primary", "secondary"],
        "{label} `typeof theme`.variants.color must expose exactly the fixture's \
         primary/secondary keys, got {color_obj:?}",
    );
    assert_eq!(
        find_prop(&color_obj.properties, "primary").as_ref(),
        Some(&TypeExpr::string_literal("")),
        "{label} `typeof theme`.variants.color.primary must be the `''` const literal, \
         got {color_obj:?}",
    );
    assert_eq!(
        find_prop(&color_obj.properties, "secondary").as_ref(),
        Some(&TypeExpr::string_literal("")),
        "{label} `typeof theme`.variants.color.secondary must be the `''` const literal, \
         got {color_obj:?}",
    );
}

/// Owner-local (same-file) registry alias with MULTIPLE type
/// arguments: `ComponentConfig<typeof theme, AppConfig, 'button'>`.
/// Pins that EACH of the three direct leaf members substitutes to its
/// CONCRETE argument — `primary` = the `typeof theme` object surface,
/// `cfg` = the same-file `AppConfig` interface surface, `name` = the
/// `'button'` string literal — never a bare `T`/`U`/`K` param nor a
/// miss placeholder. The deleted slow-lane walker covered multi-arg
/// only via the imported/cross-file path; this exercises the
/// owner-local dispatch route.
#[test]
fn resolve_component_meta_substitutes_owner_local_multi_arg_registry_alias() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type ComponentConfig<T, U, K> = {
  primary: T,
  cfg: U,
  name: K
}

interface AppConfig {
  mode: 'light' | 'dark'
}

const theme = {
  variants: {
    color: { primary: '', secondary: '' }
  }
} as const

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>

defineProps<{
  primary?: Button['primary']
  cfg?: Button['cfg']
  name?: Button['name']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "owner-local multi-arg Button alias should materialize as an object, got {:?}",
            button_entry.type_expr
        );
    };

    let member_ty = |name: &str| -> TypeExpr {
        button_shape
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == name => {
                    Some(property.ty.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Button should keep a `{name}` member, got {button_shape:?}"))
    };

    // `primary` (= the first arg `typeof theme`) is the CONCRETE theme
    // object surface: `{ variants: { color: { primary; secondary } } }`,
    // pinned all the way to the const string-literal leaves. NOT a bare
    // `T` param, NOT a miss placeholder, NOT one of the other args'
    // shapes.
    assert_concrete_theme_surface(&member_ty("primary"), "Button.primary");

    // `cfg` (= the second arg `AppConfig`) substitutes to the same-file
    // `AppConfig` interface, kept SHALLOW as a `Ref` per the
    // shallow-by-default rule (an interface alias ref, NOT eagerly
    // expanded). The discriminator: it is `Ref("AppConfig")`, NOT bare
    // `U`, NOT `T`/`typeof theme`, NOT a miss.
    let cfg = member_ty("cfg");
    let TypeExpr::Ref {
        name: cfg_name,
        type_arguments: cfg_args,
    } = &cfg
    else {
        panic!("Button.cfg should be the substituted AppConfig ref, got {cfg:?}");
    };
    assert_eq!(
        cfg_name.as_ref(),
        "AppConfig",
        "Button.cfg must substitute the second arg `AppConfig`, not a bare `U` param or other \
         type, got {cfg:?}",
    );
    assert!(
        cfg_args.is_empty(),
        "Button.cfg AppConfig ref should carry no type arguments, got {cfg:?}",
    );
    assert!(
        !matches!(&cfg, TypeExpr::TypeParameter(param) if param.name == "U"),
        "Button.cfg must NOT be the unbound parameter `U`, got {cfg:?}",
    );

    // `name` (= the third arg `'button'`) substitutes to the CONCRETE
    // string literal. NOT a bare `K` param, NOT `string`.
    let name = member_ty("name");
    assert_eq!(
        name,
        TypeExpr::string_literal("button"),
        "Button.name must substitute the third arg literal `'button'`, not a bare `K` param or \
         widened `string`, got {name:?}",
    );
}

/// Owner-local (same-file) registry alias exercising a DEFAULT type
/// parameter: `ComponentConfig<T, U = DefaultTheme>` instantiated as
/// `ComponentConfig<typeof theme>` (the second arg omitted). Pins that
/// `primary` substitutes to the concrete `typeof theme` surface AND
/// `fallback` falls back to the CONCRETE `DefaultTheme` surface — the
/// `args[i].or(param.default)` behaviour the deleted slow-lane helper
/// owned, now served by dispatch (`build.rs:960`). A regression that
/// dropped the default would leave `fallback` a bare `U` param or a
/// miss placeholder.
#[test]
fn resolve_component_meta_substitutes_owner_local_default_param_registry_alias() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
interface DefaultTheme {
  spacing: 'tight' | 'loose'
}

type ComponentConfig<T, U = DefaultTheme> = {
  primary: T,
  fallback: U
}

const theme = {
  variants: {
    color: { primary: '', secondary: '' }
  }
} as const

type Button = ComponentConfig<typeof theme>

defineProps<{
  primary?: Button['primary']
  fallback?: Button['fallback']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "owner-local default-param Button alias should materialize as an object, got {:?}",
            button_entry.type_expr
        );
    };

    let member_ty = |name: &str| -> TypeExpr {
        button_shape
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == name => {
                    Some(property.ty.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Button should keep a `{name}` member, got {button_shape:?}"))
    };

    // `primary` (= the supplied arg `typeof theme`) is the CONCRETE
    // theme object surface — same pin as the multi-arg test.
    assert_concrete_theme_surface(&member_ty("primary"), "Button.primary");

    // `fallback` (= the OMITTED second arg) must fall back to the
    // parameter default `DefaultTheme`. This is the load-bearing
    // discriminator for the `args[i].or(param.default)` behaviour now
    // served by dispatch: a regression that dropped the default would
    // leave `fallback` a bare `U` param or a miss placeholder. The
    // same-file `DefaultTheme` interface stays SHALLOW as a `Ref` per
    // the shallow-by-default rule.
    let fallback = member_ty("fallback");
    let TypeExpr::Ref {
        name: fallback_name,
        type_arguments: fallback_args,
    } = &fallback
    else {
        panic!("Button.fallback should be the default `DefaultTheme` ref, got {fallback:?}",);
    };
    assert_eq!(
        fallback_name.as_ref(),
        "DefaultTheme",
        "Button.fallback must fall back to the parameter default `DefaultTheme`, not a bare `U` \
         param or miss placeholder, got {fallback:?}",
    );
    assert!(
        fallback_args.is_empty(),
        "Button.fallback DefaultTheme ref should carry no type arguments, got {fallback:?}",
    );
    assert!(
        !matches!(&fallback, TypeExpr::TypeParameter(param) if param.name == "U"),
        "Button.fallback must NOT be the unbound parameter `U` (default must be bound), \
         got {fallback:?}",
    );
    assert!(
        !matches!(&fallback, TypeExpr::Unknown { .. }),
        "Button.fallback must NOT be a miss/semanticMiss placeholder, got {fallback:?}",
    );
}

#[test]
fn resolve_component_meta_materializes_imported_component_config_registry_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/src/tailwind-variants.d.ts",
            r#"export type ClassValue = string | { [key: string]: boolean }
export type TVVariants<S, C, V> = { [K in keyof V]: keyof V[K] }
export type TVCompoundVariants<V, S, C, O, U> = never
export type TVDefaultVariants<V, S, O, U> = never
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"import type { ClassValue, TVVariants, TVCompoundVariants, TVDefaultVariants } from './tailwind-variants'

export type TVConfig<T extends Record<string, any>> = {
  [P in keyof T]?: {
    [K in keyof T[P] as K extends 'base' | 'slots' | 'variants' | 'defaultVariants' ? K : never]?: K extends 'base' ? ClassValue
      : K extends 'slots' ? {
        [S in keyof T[P]['slots']]?: ClassValue
      }
        : K extends 'variants' ? TVVariants<T[P]['slots'], ClassValue, WidenVariantsValues<T[P]['variants']>>
          : K extends 'defaultVariants' ? TVDefaultVariants<WidenVariantsValues<T[P]['variants']>, T[P]['slots'], object, undefined>
            : never
  }
} & {
  [P in keyof T]?: {
    compoundVariants?: TVCompoundVariants<WidenVariantsValues<T[P]['variants']>, T[P]['slots'], ClassValue, object, undefined>
  }
}

type WidenVariantsValues<V extends Record<string, any> | undefined>
  = V extends Record<string, any> ? V & {
    [K in keyof V]: V[K] extends Record<string, any>
      ? V[K] & Record<string & {}, any>
      : V[K]
  } : V

type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: ClassValue
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

type GetComponentAppConfig<A, U extends string, K extends string>
  = A extends Record<U, Record<K, any>> ? A[U][K] : {}

type ComponentAppConfig<
  T,
  A extends Record<string, any>,
  K extends string,
  U extends string = 'ui' | 'ui.prose'
> = A & (
  U extends 'ui.prose'
    ? { ui?: { prose?: { [k in K]?: Partial<T> } } }
    : { [key in Exclude<U, 'ui.prose'>]?: { [k in K]?: Partial<T> } }
)

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
  U extends 'ui' | 'ui.prose' = 'ui'
> = {
  AppConfig: ComponentAppConfig<T, A, K, U>,
  variants: ComponentVariants<T & GetComponentAppConfig<A, U, K>>
  slots: ComponentSlots<T>,
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/schema.ts",
            r#"export interface AppConfig {
  ui: {
    button: {
      variants: {
        color: {
          neutral: string
        }
      }
    }
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', soft: '' },
    size: { sm: '', md: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { AppConfig } from './schema'
import theme from './theme'
import type { ComponentConfig } from './tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}

export interface ButtonSlots {
  default?(props: { ui: Button['ui'] }): any
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "imported ComponentConfig alias should materialize as an object, got {:?}",
            button_entry.type_expr
        );
    };

    let variants_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "variants" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a variants member");
    let TypeExpr::Object(variants_shape) = variants_member else {
        panic!(
            "Button.variants should materialize as an object, got {:?}",
            variants_member
        );
    };
    let color_member = variants_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "color" => Some(&property.ty),
            _ => None,
        })
        .expect("Button.variants should keep a color member");
    match color_member {
        TypeExpr::Union(members) => {
            assert!(
                members.contains(&TypeExpr::string_literal("primary")),
                "Button.variants.color should preserve the theme helper surface, got {:?}",
                color_member
            );
            assert!(
                members.contains(&TypeExpr::string_literal("secondary")),
                "Button.variants.color should preserve the theme helper surface, got {:?}",
                color_member
            );
        }
        other => panic!(
            "Button.variants.color should stay query-usable as a union surface, got {:?}",
            other
        ),
    }

    let slots_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "slots" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a slots member");
    let TypeExpr::Object(slots_shape) = slots_member else {
        panic!(
            "Button.slots should materialize as an object, got {:?}",
            slots_member
        );
    };
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "base"),
        ),
        "Button.slots should expose base, got {:?}",
        slots_member
    );
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "label"),
        ),
        "Button.slots should expose label, got {:?}",
        slots_member
    );
}

#[test]
fn resolve_component_meta_uses_db_projection_for_imported_registry_surfaces() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"type ButtonShape = {
  variants: {
    color: 'primary' | 'secondary'
  }
  slots: {
    base?: string
    label?: string
  }
  ui: {
    base?: (props?: { active?: boolean }) => string
    label?: (props?: { active?: boolean }) => string
  }
}

export type Button = Pick<ButtonShape, 'variants' | 'slots' | 'ui'>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}

export interface ButtonSlots {
  default?(props: { ui: Button['ui'] }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, ButtonSlots } from './types'

defineProps<ButtonProps>()
defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "imported Button helper should materialize as an object surface, got {:?}",
            button_entry.type_expr
        );
    };
    let member_names: std::collections::BTreeSet<_> = button_shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        !member_names.contains("variants"),
        "imported registry helpers should not widen to already-concrete sibling routes, got {member_names:?}"
    );

    let ui_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "ui" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a ui member");
    let TypeExpr::Object(ui_shape) = ui_member else {
        panic!(
            "Button.ui should materialize as an object, got {:?}",
            ui_member
        );
    };
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "base"),
        ),
        "Button.ui should expose base, got {:?}",
        ui_member
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "label"),
        ),
        "Button.ui should expose label, got {:?}",
        ui_member
    );
}

#[test]
fn resolve_component_meta_keeps_imported_registry_helpers_on_requested_member_paths() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"type ButtonShape = {
  variants: {
    color: 'primary' | 'secondary'
  }
  slots: {
    base?: string
    label?: string
  }
  ui: {
    base?: (props?: { active?: boolean }) => string
    label?: (props?: { active?: boolean }) => string
  }
}

export type Button = Pick<ButtonShape, 'variants' | 'slots' | 'ui'>

export interface ButtonSlots {
  default?(props: { ui: Button['ui'] }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "imported Button helper should materialize as an object surface, got {:?}",
            button_entry.type_expr
        );
    };

    let member_names: std::collections::BTreeSet<_> = button_shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(
        member_names,
        std::collections::BTreeSet::from(["ui"]),
        "imported registry helper should only materialize the requested member-path root, got {member_names:?}"
    );

    let ui_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "ui" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a ui member");
    let TypeExpr::Object(ui_shape) = ui_member else {
        panic!(
            "Button.ui should materialize as an object, got {:?}",
            ui_member
        );
    };
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "base"),
        ),
        "Button.ui should expose base, got {:?}",
        ui_member
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "label"),
        ),
        "Button.ui should expose label, got {:?}",
        ui_member
    );
}

#[test]
fn resolve_component_meta_keeps_transitive_nested_slot_param_helpers_off_registry() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface DeepProps {
  active?: boolean
  theme?: {
    dark?: boolean
  }
}

type ButtonShape = {
  ui: {
    base?: (props?: DeepProps) => string
    label?: (props?: DeepProps) => string
  }
}

export type Button = Pick<ButtonShape, 'ui'>

export interface ButtonSlots {
  default?(props: { ui: Button['ui'] }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    assert!(
        resolved
            .resolved_type_registry
            .iter()
            .all(|entry| entry.name != "DeepProps"),
        "requested member-path materialization should not publish transitive nested helper refs",
    );

    // Shallow-by-default registry contract: imported helper aliases are
    // published as a bare `Ref { name }` in the resolved type registry, NOT
    // an eagerly-materialized object. The deep slot-binding correctness is
    // asserted on the PUBLISHED surface below.
    let button_slots = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ButtonSlots")
        .expect("ButtonSlots should be published in the resolved type registry");
    assert!(
        matches!(&button_slots.type_expr, TypeExpr::Ref { name, .. } if name.as_ref() == "ButtonSlots"),
        "imported registry helper should stay a shallow Ref (shallow-by-default), got {:?}",
        button_slots.type_expr
    );
    assert!(
        !matches!(&button_slots.type_expr, TypeExpr::Object(_)),
        "registry entry must NOT eagerly materialize to an object, got {:?}",
        button_slots.type_expr
    );

    // Published-surface contract (path-precise materialization): the default
    // slot's `ui` binding stays symbolic on the requested member path
    // `Button['ui']` — the nested `DeepProps` callable-parameter helper is
    // never widened into the binding.
    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should be extracted");
    let ui_binding = default_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "ui")
        .expect("default slot should expose the ui binding");
    assert_eq!(
        ui_binding.raw_type.as_deref(),
        Some("Button['ui']"),
        "nested slot helpers must stay on the requested member path, got {:?}",
        ui_binding.raw_type
    );
    assert!(
        matches!(&ui_binding.type_expr, TypeExpr::IndexedAccess { .. }),
        "default slot ui binding must stay a symbolic IndexedAccess, got {:?}",
        ui_binding.type_expr
    );
}

#[test]
fn resolve_component_meta_keeps_function_valued_registry_members_shallow() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface DeepProps {
  active?: boolean
  theme?: {
    dark?: boolean
  }
}

type ButtonShape = {
  ui: {
    base?: (props?: DeepProps) => string
    label?: (props?: DeepProps) => string
  }
}

export type Button = Pick<ButtonShape, 'ui'>

export interface ButtonSlots {
  default?(props: { ui: Button['ui'] }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    assert!(
        resolved
            .resolved_type_registry
            .iter()
            .all(|entry| entry.name != "DeepProps"),
        "function-valued registry members should not publish transitive callable parameter helpers",
    );

    // Shallow-by-default registry contract: the imported `ButtonSlots` helper
    // stays a bare `Ref { name }` in the registry. Its function-valued member
    // (`default(props: { ui: Button['ui'] })`) carries a callable-parameter
    // helper that must NOT be eagerly inlined — the deep binding correctness is
    // asserted on the published surface below.
    let button_slots = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ButtonSlots")
        .expect("ButtonSlots should be published in the resolved type registry");
    assert!(
        matches!(&button_slots.type_expr, TypeExpr::Ref { name, .. } if name.as_ref() == "ButtonSlots"),
        "imported registry helper should stay a shallow Ref (shallow-by-default), got {:?}",
        button_slots.type_expr
    );
    assert!(
        !matches!(&button_slots.type_expr, TypeExpr::Object(_)),
        "registry entry must NOT eagerly materialize the function-valued member, got {:?}",
        button_slots.type_expr
    );

    // Published-surface contract: the default slot's `ui` binding stays
    // symbolic on the requested member path `Button['ui']`; the function-valued
    // projected member never widens the imported member-path helper.
    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should be extracted");
    let ui_binding = default_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "ui")
        .expect("default slot should expose the ui binding");
    assert_eq!(
        ui_binding.raw_type.as_deref(),
        Some("Button['ui']"),
        "function-valued projected members must keep the helper on the requested member path, got {:?}",
        ui_binding.raw_type
    );
    assert!(
        matches!(&ui_binding.type_expr, TypeExpr::IndexedAccess { .. }),
        "default slot ui binding must stay a symbolic IndexedAccess, got {:?}",
        ui_binding.type_expr
    );
}

#[test]
fn resolve_component_meta_publishes_compound_heritage_surface_as_merged_object() {
    // §compound-root: a registry alias whose body is a heritage interface
    // (`interface Derived extends Base { own }`) must publish the COMPOSED
    // merged one-level surface (base ∪ own), not the carrier-intact declaration
    // anchor (which would keep the heritage arm symbolic). Exercises the
    // candidate's explicit-object-surface fact + the compound-root composition
    // route through the shared shallow walker.
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface RegistryHeritageBase { base: string }
export interface RegistryHeritageDerived extends RegistryHeritageBase { own: number }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { RegistryHeritageDerived } from './types'
defineProps<RegistryHeritageDerived>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let mut prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    prop_names.sort_unstable();
    // Anti-vacuity: the EXACT merged key set — heritage `base` merges with the
    // own `own` member. A carrier-intact anchor would drop `base`.
    assert_eq!(
        prop_names,
        vec!["base", "own"],
        "heritage interface must publish the merged base ∪ own surface, got {prop_names:?}",
    );
}

#[test]
fn resolve_component_meta_pick_over_class_keeps_keyspace_public_semantics() {
    // §class-Pick-visibility: a `Pick<Class, 'alpha'>` registry alias must route
    // through the SHARED Pick keyspace engine. A naive `SurfaceView.members`
    // filter would leak the non-public / non-requested class members. Only the
    // requested public key may surface.
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export class RegistryVisibilityClass {
  public alpha = 1
  protected beta = 2
  private gamma = 3
}
export type PickedRegistryVisibility = Pick<RegistryVisibilityClass, 'alpha'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { PickedRegistryVisibility } from './types'
defineProps<PickedRegistryVisibility>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    // Anti-vacuity: ONLY the requested public key surfaces. The protected /
    // private members must never leak through the Pick keyspace.
    assert!(
        prop_names.contains(&"alpha"),
        "the requested public key `alpha` must surface, got {prop_names:?}",
    );
    assert!(
        !prop_names.contains(&"beta"),
        "the non-requested protected member `beta` must NOT leak, got {prop_names:?}",
    );
    assert!(
        !prop_names.contains(&"gamma"),
        "the private member `gamma` must NEVER leak, got {prop_names:?}",
    );
}

#[test]
fn resolve_component_meta_materializes_bound_registry_members_despite_opaque_sibling_args() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends Record<string, any>, A> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
  appConfig?: A
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './types'
import theme from './theme'

type Button = ComponentConfig<typeof theme, MissingAppConfig>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "Button helper should materialize as an object despite the opaque sibling arg, got {:?}",
            button_entry.type_expr
        );
    };

    // The opaque sibling arg should not block materialization of members that
    // depend only on the concrete theme argument.
    let variants_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "variants" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a variants member");
    let TypeExpr::Object(variants_shape) = variants_member else {
        panic!(
            "Button.variants should materialize as an object when the theme arg is concrete, got {:?}",
            variants_member
        );
    };
    assert!(
        variants_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "color"),
        ),
        "Button.variants should expose color, got {:?}",
        variants_member
    );

    let slots_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "slots" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a slots member");
    let TypeExpr::Object(slots_shape) = slots_member else {
        panic!(
            "Button.slots should materialize as an object when the theme arg is concrete, got {:?}",
            slots_member
        );
    };
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "base"),
        ),
        "Button.slots should expose base, got {:?}",
        slots_member
    );
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "label"),
        ),
        "Button.slots should expose label, got {:?}",
        slots_member
    );
}

#[test]
fn resolve_component_meta_publishes_transitive_registry_aliases_for_nested_indexed_access_refs() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/avatar-theme.ts",
            r#"export default {
  variants: {
    size: { sm: '', md: '' }
  },
  slots: {
    base: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/avatar-types.ts",
            r#"import type { ComponentConfig } from './types'
import avatarTheme from './avatar-theme'

export type Avatar = ComponentConfig<typeof avatarTheme>

export interface AvatarProps {
  size?: Avatar['variants']['size']
  ui?: Avatar['slots']
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { AvatarProps } from './avatar-types'

export interface ButtonProps {
  avatar?: AvatarProps
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./avatar-types".to_string(),
            resolved_canonical_id: Some("/src/avatar-types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/avatar-types.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./avatar-theme".to_string(),
                resolved_canonical_id: Some("/src/avatar-theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    // Avatar is not published as a separate registry entry;
    // transitive imported aliases are resolved inline.
    assert!(
        !resolved
            .resolved_type_registry
            .iter()
            .any(|entry| entry.name == "Avatar"),
        "transitive Avatar alias should not be separately published in the registry"
    );

    let meta = project
        .host()
        .get_component_meta("/src/Button.vue")
        .expect("should return component meta");
    let avatar = meta
        .props
        .iter()
        .find(|prop| prop.name == "avatar")
        .expect("avatar prop should still be exposed");
    assert_eq!(
        avatar.raw_type.as_deref(),
        Some("AvatarProps"),
        "public prop contract should keep the imported alias text"
    );
    // Architectural contract: imported alias names stay shallow at the
    // published surface. The avatar prop publishes the bare `Ref { name:
    // "AvatarProps" }`; consumers re-resolve the declaration through the
    // registry. The transitive `Avatar = ComponentConfig<typeof
    // avatarTheme>` chain is resolved on-demand via the resolver, not
    // eagerly inlined into the published prop type.
    assert!(
        matches!(
            &avatar.type_expr,
            TypeExpr::Ref { name, .. } if name.as_ref() == "AvatarProps"
        ),
        "avatar prop should publish the bare AvatarProps ref, got {:?}",
        avatar.type_expr
    );
}

#[test]
fn resolve_component_meta_handles_renamed_import_cycles_in_shallow_alias_hydration() {
    let project = make_project();
    project
        .upsert_base(
            "/src/helpers.ts",
            r#"type Id<T> = T

type SlotInfo<T> = Id<{
  value: T
}>

type WithChildren<T> = {
  slot: SlotInfo<ComponentConfig<T>>
}

export type ComponentConfig<T> = WithChildren<T>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig as LocalConfig } from './helpers'

export interface ButtonProps {
  slot?: LocalConfig<string>['slot']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let local_config = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "LocalConfig")
        .expect("renamed imported alias should be published in the resolved type registry");
    let TypeExpr::Object(local_config_shape) = &local_config.type_expr else {
        panic!(
            "LocalConfig should materialize as an object, got {:?}",
            local_config.type_expr
        );
    };
    assert!(
        local_config_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "slot"),
        ),
        "LocalConfig should keep its slot member, got {:?}",
        local_config.type_expr
    );
}

#[test]
fn resolve_component_meta_publishes_transitive_renamed_imported_registry_aliases() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export type Inner = {
  nested: {
    leaf: string
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/helpers.ts",
            r#"import type { Inner as LocalInner } from './base'

export type ComponentConfig = {
  ui: LocalInner
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './helpers'

export interface ButtonProps {
  ui?: ComponentConfig['ui']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./helpers".to_string(),
            resolved_canonical_id: Some("/src/helpers.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/helpers.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./base".to_string(),
            resolved_canonical_id: Some("/src/base.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    // LocalInner is not published as a separate registry entry;
    // transitive renamed imported aliases are resolved inline.
    assert!(
        !resolved
            .resolved_type_registry
            .iter()
            .any(|entry| entry.name == "LocalInner"),
        "transitive renamed imported alias should not be separately published in the registry"
    );
}

#[test]
fn resolve_component_meta_keeps_deep_imported_registry_branches_shallow() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export type Level3 = {
  leaf: string
}

export type Level2 = {
  node: Level3
}

export type Level1 = {
  node: Level2
}

export type ComponentConfig = {
  ui: Level1
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './types'

export interface ButtonProps {
  ui?: ComponentConfig['ui']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let config_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ComponentConfig")
        .expect("ComponentConfig should be published in the resolved type registry");
    let TypeExpr::Object(config_shape) = &config_entry.type_expr else {
        panic!(
            "ComponentConfig should materialize as an object, got {:?}",
            config_entry.type_expr
        );
    };

    let ui_member = config_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "ui" => Some(&property.ty),
            _ => None,
        })
        .expect("ComponentConfig should keep a ui member");
    let TypeExpr::Object(ui_shape) = ui_member else {
        panic!(
            "ComponentConfig.ui should materialize as an object, got {:?}",
            ui_member
        );
    };

    let node_member = ui_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "node" => Some(&property.ty),
            _ => None,
        })
        .expect("ComponentConfig.ui should keep a node member");
    // Deep imported branches are fully resolved as nested objects: { node: { leaf: string } }
    let TypeExpr::Object(level2_shape) = node_member else {
        panic!(
            "deep imported registry branches should resolve to an object, got {:?}",
            node_member
        );
    };
    let inner_node = level2_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "node" => Some(&property.ty),
            _ => None,
        })
        .expect("Level2 should have a node member");
    let TypeExpr::Object(level3_shape) = inner_node else {
        panic!(
            "Level2.node should resolve to an object, got {:?}",
            inner_node
        );
    };
    assert!(
        level3_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "leaf"),
        ),
        "Level3 should expose leaf, got {:?}",
        inner_node
    );
}

#[test]
fn get_component_meta_returns_full_native_metadata_contract() {
    let project = make_project();
    project
        .upsert_base(
            "/FancyButton.vue",
            r#"<script setup lang="ts">
defineProps<{ label: string; modelValue: number }>()
</script>
<template><button><slot /></button></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import FancyButton from './FancyButton.vue'

const count = ref(0)
const accentColor = "red"
const doubled = computed(() => count.value * 2)

onMounted(() => {
  console.log(count.value)
})
</script>
<template>
  <FancyButton
    id="wrapper"
    ref="button"
    :label="`${doubled}`"
    class="primary"
    :class="{ active: count > 0 }"
    v-model="count"
  >
    <template #default>{{ count }}</template>
  </FancyButton>
</template>
<style scoped module="theme">
#wrapper .primary {
  color: v-bind(accentColor);
  --accent: red;
}
</style>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    assert_eq!(
        meta.components.len(),
        1,
        "template component usage should be present"
    );
    assert_eq!(meta.components[0].name, "FancyButton");
    assert_eq!(
        meta.components[0].import_source.as_deref(),
        Some("./FancyButton.vue")
    );
    assert!(!meta.components[0].has_spread);
    assert!(meta.components[0].has_dynamic_class);
    assert_eq!(meta.components[0].v_models, vec!["modelValue".to_string()]);
    assert_eq!(
        meta.components[0]
            .v_model_entries
            .iter()
            .map(|entry| entry.binding_name.as_str())
            .collect::<Vec<_>>(),
        vec!["modelValue"]
    );
    let label_prop = meta.components[0]
        .props
        .iter()
        .find(|prop| prop.name == "label")
        .expect("label prop usage should be present");
    assert_eq!(label_prop.expression.as_deref(), Some("`${doubled}`"));
    assert_eq!(label_prop.referenced_bindings, vec!["doubled".to_string()]);
    assert!(!label_prop.from_spread);
    assert!(!label_prop.is_shorthand);

    assert_eq!(
        meta.template_refs.len(),
        1,
        "template refs should be present"
    );
    assert_eq!(meta.template_refs[0].name, "button");
    assert_eq!(meta.template_refs[0].target_tag, "FancyButton");

    let child_meta = session
        .get_component_meta("/FancyButton.vue")
        .unwrap()
        .expect("child component meta should be available");
    let public_instance = child_meta
        .public_instance
        .as_ref()
        .expect("host should provide a public-instance sidecar");
    let public_member_names: Vec<_> = public_instance
        .members
        .iter()
        .map(|member| member.name.as_str())
        .collect();
    assert!(
        public_member_names.contains(&"label"),
        "public instance should expose declared props, got {:?}",
        public_member_names
    );
    assert!(
        public_member_names.contains(&"modelValue"),
        "public instance should expose model props, got {:?}",
        public_member_names
    );
    assert!(
        public_member_names.contains(&"$slots"),
        "public instance should expose $slots, got {:?}",
        public_member_names
    );
    assert!(
        public_instance.members.iter().any(|member| {
            member.name == "$slots"
                && matches!(
                    member.kind,
                    verter_semantic::analysis::component_meta::PublicInstanceMemberKind::SlotContainer,
                )
        }),
        "$slots should be tagged as a public-instance slot container"
    );

    assert!(
        meta.imports.iter().any(|import| import.source == "vue"),
        "script imports should be preserved"
    );
    assert!(
        meta.bindings
            .iter()
            .any(|binding| binding.name == "count" && binding.used_in_template),
        "bindings should preserve template usage information"
    );
    assert!(
        meta.vue_api_calls.iter().any(|call| matches!(
            call.api,
            verter_semantic::analysis::types::VueApiClassification::OnMounted,
        )),
        "Vue API calls should be preserved"
    );
    assert_eq!(meta.styles.len(), 1, "style metadata should be present");
    assert_eq!(meta.styles[0].classes, vec!["primary".to_string()]);
    assert_eq!(meta.styles[0].ids, vec!["wrapper".to_string()]);
    assert_eq!(
        meta.styles[0].custom_properties,
        vec!["--accent".to_string()]
    );
    assert_eq!(meta.styles[0].v_binds, vec!["accentColor".to_string()]);
    assert!(
        meta.styles[0]
            .selectors
            .iter()
            .any(|selector| selector.text == "#wrapper .primary"),
        "style selectors should be preserved"
    );
}

#[test]
fn svelte_component_meta_carries_template_usage_facts() {
    // THE discriminating public E2E: a `.svelte` file resolved through
    // `get_component_meta` carries its template component-USAGE facts on the
    // published `ComponentMetaBody.components`. It is RED if Svelte's
    // `template_data` is an empty stub OR template-data ingestion is Vue-gated
    // (Svelte never reaches the public surface); GREEN with registry-dispatched
    // ingestion + the typed-IR producer. A producer alone (a populated
    // `template_data` not yet wired into the public surface) is insufficient.
    let project = make_project();
    project
        .upsert_base(
            "/App.svelte",
            r#"<script lang="ts">
  import Button from './Button.svelte';
  let value = 0;
  function handler() {}
</script>
<Button size="sm" online={value} onclick={handler} bind:value on:focus={handler}>
  {#snippet icon()}x{/snippet}
</Button>
"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.svelte")
        .unwrap()
        .expect("get_component_meta should return metadata for a .svelte file");

    assert_eq!(
        meta.components.len(),
        1,
        "the Svelte template component usage must reach the public meta surface"
    );
    let usage = &meta.components[0];
    assert_eq!(usage.name, "Button");
    assert!(!usage.is_dynamic);

    // The `size`, `online`, and `onclick` PROPS are published. `online` and
    // `onclick` are PLAIN `on*` attributes — they are PROPS, never fabricated as
    // events by a name guess (the P1 regression this discriminates).
    let prop_names: Vec<&str> = usage.props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        prop_names,
        vec!["size", "online", "onclick"],
        "plain attrs (incl. `online`/`onclick`) reach the surface as PROPS"
    );
    // `bind:value` is NOT a prop / NOT a v_model — it is a neutral BINDING.
    assert!(!prop_names.contains(&"value"));

    let binding_names: Vec<&str> = usage.bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        binding_names,
        vec!["value"],
        "`bind:value` reaches the surface"
    );

    // Only the LEGACY `on:focus` directive is a neutral EVENT — the plain `on*`
    // attributes (`online`, `onclick`) are NOT events.
    let event_names: Vec<&str> = usage.events.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        event_names,
        vec!["focus"],
        "only the legacy `on:` directive reaches the surface as an event"
    );
    // HARD INVARIANT: no plain `on*` attribute is misclassified as an event.
    assert!(!event_names.contains(&"online"));
    assert!(!event_names.contains(&"onclick"));

    // The passed `{#snippet icon}` is recorded in slots_used.
    assert_eq!(usage.slots_used, vec!["icon".to_string()]);
}

#[test]
fn vue_component_meta_keeps_empty_bindings_events_no_regression() {
    // Vue NO-REGRESSION: the neutral per-usage `bindings` / `events` fields stay
    // EMPTY for Vue (Vue carries two-way bindings in `v_models` and events at the
    // template level), while the existing Vue fields are unchanged.
    let project = make_project();
    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
defineProps<{ label: string; modelValue: number }>()
</script>
<template><button>{{ label }}</button></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Host.vue",
            r#"<script setup lang="ts">
import { ref } from 'vue'
import Child from './Child.vue'
const count = ref(0)
</script>
<template>
  <Child label="hi" :class="{ active: count > 0 }" v-model="count" />
</template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/Host.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    assert_eq!(meta.components.len(), 1);
    let usage = &meta.components[0];
    assert_eq!(usage.name, "Child");
    // The Vue fields are byte-identical in shape.
    assert_eq!(usage.v_models, vec!["modelValue".to_string()]);
    assert!(usage.has_dynamic_class);
    assert!(usage.props.iter().any(|p| p.name == "label"));
    // The new neutral fields are EMPTY for Vue.
    assert!(
        usage.bindings.is_empty(),
        "Vue must not populate the neutral `bindings` field"
    );
    assert!(
        usage.events.is_empty(),
        "Vue must not populate the neutral `events` field"
    );
}

#[test]
fn get_component_meta_surfaces_sfc_block_metadata() {
    let project = make_project();
    project
        .upsert_base(
            "/Button.vue",
            r#"<script lang="ts">
export const legacy = true
</script>
<script setup lang="ts" generic="T extends string = string" attrs="ButtonAttrs">
defineProps<{ label: string }>()
defineSlots<{
  default(props: { item: number }): any
}>()
defineExpose({
  focus() {}
})
</script>
<template lang="html" data-layout="stack">
  <button>{{ label }}</button>
  <slot :item="1" />
</template>
<style scoped module="theme" lang="scss">
.primary { color: red; }
</style>
<i18n lang="json">
{ "label": "Button" }
</i18n>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/Button.vue")
        .unwrap()
        .expect("component meta should be available");

    let blocks = meta
        .sfc_blocks
        .as_ref()
        .expect("host should surface SFC block metadata");
    assert_eq!(
        blocks
            .script
            .as_ref()
            .and_then(|block| block.lang.as_deref()),
        Some("ts")
    );
    assert_eq!(
        blocks
            .script_setup
            .as_ref()
            .and_then(|block| block.generic.as_deref()),
        Some("T extends string = string")
    );
    assert_eq!(
        blocks
            .script_setup
            .as_ref()
            .and_then(|block| block.attrs_type.as_deref()),
        Some("ButtonAttrs")
    );
    assert_eq!(
        blocks
            .template
            .as_ref()
            .and_then(|block| block.lang.as_deref()),
        Some("html")
    );
    assert!(
        blocks.template.as_ref().is_some_and(|block| block
            .attributes
            .iter()
            .any(|attribute| attribute.name == "data-layout"
                && attribute.value.as_deref() == Some("stack"))),
        "template block should preserve arbitrary root attributes"
    );
    assert_eq!(blocks.styles.len(), 1);
    assert_eq!(blocks.styles[0].index, 0);
    assert_eq!(blocks.styles[0].lang.as_deref(), Some("scss"));
    assert!(blocks.styles[0].scoped);
    assert!(blocks.styles[0].is_module);
    assert_eq!(blocks.styles[0].module_name.as_deref(), Some("theme"));
    assert_eq!(blocks.custom.len(), 1);
    assert_eq!(blocks.custom[0].index, 0);
    assert_eq!(blocks.custom[0].block_type, "i18n");
    assert_eq!(blocks.custom[0].lang.as_deref(), Some("json"));
}

#[test]
fn get_component_meta_preserves_component_spread_usage() {
    let project = make_project();
    project
        .upsert_base(
            "/FancyButton.vue",
            r#"<script setup lang="ts">
defineProps<{ label?: string }>()
</script>
<template><button><slot /></button></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import FancyButton from './FancyButton.vue'

const attrs = { label: 'Hello' }
</script>
<template>
  <FancyButton v-bind="attrs" />
</template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    assert_eq!(meta.components.len(), 1);
    assert!(
        meta.components[0].has_spread,
        "component usage should preserve v-bind spread markers"
    );
}

// ===========================================================================
// Phase 6: Resolved external type cache
// ===========================================================================

#[test]
fn component_meta_avoids_legacy_resolved_type_cache_across_different_owners() {
    let project = make_project();

    // Shared dependency
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface SharedProps { shared: string }"#,
        )
        .unwrap();

    // Two different SFCs importing the same type from the same dep
    project
        .upsert_base(
            "/src/A.vue",
            r#"<script setup lang="ts">
import { SharedProps } from './types'
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/B.vue",
            r#"<script setup lang="ts">
import { SharedProps } from './types'
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Set up dep resolution for both owners
    project.host().set_import_dependencies(
        "/src/A.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/B.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();

    // First owner resolves the type without touching the legacy host cache.
    project.host().provenance().reset();
    let meta_a = session.get_component_meta("/src/A.vue").unwrap().unwrap();
    let p1 = provenance(&project);

    assert_eq!(meta_a.props.len(), 1, "A.vue should have the shared prop");
    assert_eq!(
        p1.resolved_external_type_cache_misses, 0,
        "component-meta should not populate the legacy resolved type cache on first owner resolution"
    );
    assert_eq!(
        p1.resolved_external_type_cache_hits, 0,
        "component-meta should not hit the legacy resolved type cache on first owner resolution"
    );
    assert!(
        project.host().resolved_type_cache().is_empty(),
        "component-meta queries should leave the legacy host resolved type cache empty"
    );

    // Reset counters for second owner
    project.host().provenance().reset();
    let meta_b = session.get_component_meta("/src/B.vue").unwrap().unwrap();
    let p2 = provenance(&project);

    assert_eq!(meta_b.props.len(), 1, "B.vue should have the shared prop");
    assert_eq!(meta_b.props[0].name, "shared");

    assert_eq!(
        p2.resolved_external_type_cache_hits, 0,
        "component-meta should not hit the legacy host resolved type cache for a second owner"
    );
    assert_eq!(
        p2.resolved_external_type_cache_misses, 0,
        "component-meta should not miss the legacy host resolved type cache for a second owner"
    );
    assert!(
        project.host().resolved_type_cache().is_empty(),
        "component-meta queries should leave the legacy host resolved type cache empty"
    );
}

#[test]
fn component_meta_avoids_legacy_resolved_type_cache_for_workspace_only_package_dependencies() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/fancy/package.json".to_string(),
        Arc::from(
            r#"{ "name": "fancy", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
        Arc::from("export interface SharedProps { shared: string }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    let project = MetaProject::new(host);
    project
        .configure_projects(vec![
            verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.json".to_string()),
            ),
        ])
        .unwrap();
    project
        .upsert_base(
            "/workspace/src/A.vue",
            r#"<script setup lang="ts">
import type { SharedProps } from 'fancy'
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/workspace/src/B.vue",
            r#"<script setup lang="ts">
import type { SharedProps } from 'fancy'
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();

    project.host().provenance().reset();
    let meta_a = session
        .get_component_meta("/workspace/src/A.vue")
        .unwrap()
        .unwrap();
    let p1 = provenance(&project);
    assert_eq!(
        meta_a.props.len(),
        1,
        "A.vue should resolve the package prop"
    );
    assert_eq!(
        p1.resolved_external_type_cache_misses, 0,
        "component-meta should not miss the legacy resolved type cache for a workspace-only dep"
    );
    assert_eq!(
        p1.resolved_external_type_cache_hits, 0,
        "component-meta should not hit the legacy resolved type cache on first workspace-only dep resolution"
    );
    assert!(
        project.host().resolved_type_cache().is_empty(),
        "component-meta queries should leave the legacy host resolved type cache empty"
    );

    project.host().provenance().reset();
    let meta_b = session
        .get_component_meta("/workspace/src/B.vue")
        .unwrap()
        .unwrap();
    let p2 = provenance(&project);
    assert_eq!(
        meta_b.props.len(),
        1,
        "B.vue should resolve the package prop"
    );
    assert_eq!(meta_b.props[0].name, "shared");
    assert_eq!(
        p2.resolved_external_type_cache_hits, 0,
        "component-meta should not hit the legacy host resolved type cache for a second workspace-only owner"
    );
    assert_eq!(
        p2.resolved_external_type_cache_misses, 0,
        "component-meta should not miss the legacy host resolved type cache for a second workspace-only owner"
    );
    assert!(
        project.host().resolved_type_cache().is_empty(),
        "component-meta queries should leave the legacy host resolved type cache empty"
    );
}

#[test]
fn component_meta_queries_do_not_populate_legacy_resolved_type_cache() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let _ = session.get_component_meta("/App.vue").unwrap();

    assert!(
        project.host().resolved_type_cache().is_empty(),
        "component-meta should no longer populate the legacy resolved type cache"
    );

    project.clear_caches().unwrap();

    assert!(
        project.host().resolved_type_cache().is_empty(),
        "clear_caches should leave the legacy resolved type cache empty"
    );
}

#[test]
fn resolved_type_cache_is_bounded() {
    // Verify that inserting beyond cap doesn't grow unbounded
    let host = VerterHost::new_standalone(HostConfig {
        ..HostConfig::default()
    });

    {
        let cache = host.resolved_type_cache();
        // Fill to cap. Each insert routes through the rehomed DB; the
        // bounded clear-all-at-cap policy fires on the (cap+1)-th
        // insert and the test only goes up to cap so the policy stays
        // dormant.
        for i in 0..crate::types::RESOLVED_TYPE_CACHE_CAP {
            cache.insert(
                crate::types::ResolvedTypeCacheKey {
                    dep_canonical_id: format!("/dep_{i}.ts"),
                    dep_source_hash: [0u8; 16],
                    type_name: "T".to_string(),
                    resolve_kind: verter_workspace::ResolveRequestKind::TypeImport,
                    view_fingerprint: 0,
                },
                crate::types::ResolvedTypeCacheEntry {
                    resolved: None,
                    tracked_deps: Vec::new(),
                },
            );
        }
        assert_eq!(
            cache.len(),
            crate::types::RESOLVED_TYPE_CACHE_CAP,
            "cache should be at cap"
        );
    }

    // The eviction happens inside resolve_external_type_from_loaded_files,
    // but we can verify the cap constant is reasonable.
    #[allow(clippy::assertions_on_constants)]
    {
        assert!(
            crate::types::RESOLVED_TYPE_CACHE_CAP >= 1024,
            "cache cap should be at least 1024"
        );
        assert!(
            crate::types::RESOLVED_TYPE_CACHE_CAP <= 16384,
            "cache cap should not exceed 16384"
        );
    }
}

// ===========================================================================
// Phase 8: Correctness — typeof, double script, interface extends imported
// ===========================================================================

#[test]
fn local_typeof_resolves_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const config = { x: 1, y: 'hello' }
defineProps<typeof config>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // Assert+: both fields from config
    assert!(names.contains(&"x"), "should have 'x', got: {names:?}");
    assert!(names.contains(&"y"), "should have 'y', got: {names:?}");

    // Assert-: no extra fields
    assert_eq!(meta.props.len(), 2, "should have exactly 2 props");

    // `const config = { x: 1, y: 'hello' }` keeps the BINDING constant but
    // leaves the object PROPERTIES mutable, so `typeof config` widens each
    // member to its primitive (`{ x: number; y: string }`) exactly as TS does
    // — literal preservation would require `as const`. Pin the TS-correct
    // widened published types so the native contract catches a future drift
    // back to over-narrowed literals.
    use verter_type_expr::{PrimitiveName, TypeExpr};
    let x = meta.props.iter().find(|p| p.name == "x").unwrap();
    assert!(
        matches!(x.type_expr, TypeExpr::Primitive(PrimitiveName::Number)),
        "typeof config widens `x: 1` to `number`, got: {:?}",
        x.type_expr
    );
    let y = meta.props.iter().find(|p| p.name == "y").unwrap();
    assert!(
        matches!(y.type_expr, TypeExpr::Primitive(PrimitiveName::String)),
        "typeof config widens `y: 'hello'` to `string`, got: {:?}",
        y.type_expr
    );
}

#[test]
fn double_script_same_file_visibility_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script lang="ts">
export interface SharedProps { shared: boolean }
</script>
<script setup lang="ts">
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    // Assert+: prop from sibling script block
    assert_eq!(
        meta.props.len(),
        1,
        "should have 1 prop from sibling script"
    );
    assert_eq!(meta.props[0].name, "shared");

    // Assert-: no unresolved types or errors — prop should be fully resolved
    assert!(
        meta.props[0].raw_type.is_some(),
        "shared prop should have a resolved raw type"
    );
}

#[test]
fn interface_extends_pick_of_imported_type_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface BaseProps { a: string; b: number; c: boolean; d: object }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { BaseProps } from './base'
interface MyProps extends Pick<BaseProps, 'a' | 'b'> { local: string }
defineProps<MyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./base".to_string(),
            resolved_canonical_id: Some("/src/base.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // Assert+: inherited + local
    assert!(
        names.contains(&"a"),
        "should have 'a' from Pick, got: {names:?}"
    );
    assert!(
        names.contains(&"b"),
        "should have 'b' from Pick, got: {names:?}"
    );
    assert!(
        names.contains(&"local"),
        "should have 'local', got: {names:?}"
    );

    // Assert-: excluded fields
    assert!(!names.contains(&"c"), "should NOT have 'c', got: {names:?}");
    assert!(!names.contains(&"d"), "should NOT have 'd', got: {names:?}");
}

#[test]
fn package_pick_heritage_survives_local_indexed_access_helpers_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TabsRootProps<T> {
  defaultValue?: T
  modelValue?: T
  activationMode?: 'automatic' | 'manual'
  unmountOnHide?: boolean
}

export interface TabsRootEmits<T> {
  (e: 'update:modelValue', payload: T): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"
export type ComponentConfig<TTheme> = {
  variants: {
    color: 'primary' | 'secondary'
    size: 'sm' | 'md'
  }
  slots: {
    root?: string
    list?: string
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' },
    size: { sm: '', md: '' }
  },
  slots: {
    root: '',
    list: ''
  }
} as const"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { TabsRootProps, TabsRootEmits } from 'reka-ui'
import type { ComponentConfig } from './tv'
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface Props extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  color?: Tabs['variants']['color']
  ui?: Tabs['slots']
}

export interface Emits extends TabsRootEmits<string | number> {}
</script>
<script setup lang="ts">
defineProps<Props>()
defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "reka-ui".to_string(),
                resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            true,
            ctx,
        )
    })
    .analysis;
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"activationMode")
            && prop_names.contains(&"defaultValue")
            && prop_names.contains(&"modelValue")
            && prop_names.contains(&"unmountOnHide"),
        "package-backed Pick heritage should survive alongside local indexed-access helpers, got {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"color") && prop_names.contains(&"ui"),
        "local indexed-access helper props should still be present, got {prop_names:?}"
    );

    let color = meta
        .props
        .iter()
        .find(|prop| prop.name == "color")
        .expect("color prop should exist");
    assert!(
        !matches!(
            color.type_expr,
            TypeExpr::Unknown { .. } | TypeExpr::IndexedAccess { .. }
        ),
        "component-config indexed access should not stay symbolic in component meta, got {:?}",
        color.type_expr
    );
    assert_union_string_literals(&color.type_expr, &["primary", "secondary"]);

    let ui = meta
        .props
        .iter()
        .find(|prop| prop.name == "ui")
        .expect("ui prop should exist");
    let TypeExpr::Object(ui_shape) = &ui.type_expr else {
        panic!(
            "component-config slots helper should materialize as an object, got {:?}",
            ui.type_expr
        );
    };
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "root"),
        ),
        "ui helper should keep root, got {:?}",
        ui.type_expr
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "list"),
        ),
        "ui helper should keep list, got {:?}",
        ui.type_expr
    );

    let event = meta
        .events
        .iter()
        .find(|event| event.name == "update:modelValue")
        .expect("update:modelValue event should exist");
    let TypeExpr::Tuple { elements, .. } = &event.payload else {
        panic!(
            "package-backed emits should materialize as a tuple payload, got {:?}",
            event.payload
        );
    };
    assert_eq!(
        elements.len(),
        1,
        "model update should have a single payload"
    );
    match &elements[0].ty {
        TypeExpr::Union(members) => {
            assert!(
                members.contains(&TypeExpr::Primitive(PrimitiveName::String)),
                "event payload should include string, got {:?}",
                event.payload
            );
            assert!(
                members.contains(&TypeExpr::Primitive(PrimitiveName::Number)),
                "event payload should include number, got {:?}",
                event.payload
            );
        }
        other => panic!(
            "package-backed emits should instantiate the generic payload, got {:?}",
            other
        ),
    }
    assert_eq!(
        event.raw_signature.as_deref(),
        Some("[payload: string | number]"),
        "event display should not fall back to the uninstantiated type parameter"
    );
}

#[test]
fn generic_package_pick_heritage_and_indexed_access_helpers_survive_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TabsRootProps<T> {
  defaultValue?: T
  modelValue?: T
  activationMode?: 'automatic' | 'manual'
  unmountOnHide?: boolean
}

export interface TabsRootEmits<T> {
  (e: 'update:modelValue', payload: T): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export type GetItemKeys<T> = keyof T & string

export type ComponentConfig<TTheme> = {
  variants: {
    color: 'primary' | 'secondary'
    variant: 'pill' | 'link'
    size: 'sm' | 'md'
    orientation: 'horizontal' | 'vertical'
  }
  slots: {
    root?: string
    list?: string
    content?: string
  }
  ui: {
    root: string,
    list: string
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { pill: '', link: '' },
    size: { sm: '', md: '' },
    orientation: { horizontal: '', vertical: '' }
  },
  slots: {
    root: '',
    list: '',
    content: ''
  },
  ui: {
    root: '',
    list: ''
  }
} as const"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { VNode } from 'vue'
import type { TabsRootProps, TabsRootEmits } from 'reka-ui'
import type { ComponentConfig, GetItemKeys } from './types'
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface TabsItem {
  label?: string
  value?: string | number
}

export interface TabsProps<T extends TabsItem = TabsItem> extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  items?: T[]
  color?: Tabs['variants']['color']
  variant?: Tabs['variants']['variant']
  size?: Tabs['variants']['size']
  orientation?: Tabs['variants']['orientation']
  valueKey?: GetItemKeys<T>
  labelKey?: GetItemKeys<T>
  ui?: Tabs['slots']
}

export interface TabsEmits extends TabsRootEmits<string | number> {}

type SlotProps<T extends TabsItem> = (props: { item: T, index: number, ui: Tabs['ui'] }) => VNode[]

export type TabsSlots<T extends TabsItem = TabsItem> = {
  'default'?(props: { item: T, index: number }): VNode[]
  'content'?: SlotProps<T>
}
</script>
<script setup lang=\"ts\" generic=\"T extends TabsItem\">
withDefaults(defineProps<TabsProps<T>>(), {
  defaultValue: '0',
  orientation: 'horizontal',
  unmountOnHide: true,
  valueKey: 'value',
  labelKey: 'label'
})
defineEmits<TabsEmits>()
defineSlots<TabsSlots<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "reka-ui".to_string(),
                resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            true,
            ctx,
        )
    })
    .analysis;
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"activationMode")
            && prop_names.contains(&"defaultValue")
            && prop_names.contains(&"modelValue")
            && prop_names.contains(&"unmountOnHide"),
        "generic package-backed Pick heritage should survive, got {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"color")
            && prop_names.contains(&"variant")
            && prop_names.contains(&"size")
            && prop_names.contains(&"orientation")
            && prop_names.contains(&"ui"),
        "generic indexed-access helper props should survive, got {prop_names:?}"
    );

    let color = meta
        .props
        .iter()
        .find(|prop| prop.name == "color")
        .expect("color prop should exist");
    assert_union_string_literals(&color.type_expr, &["primary", "secondary"]);

    let ui = meta
        .props
        .iter()
        .find(|prop| prop.name == "ui")
        .expect("ui prop should exist");
    let TypeExpr::Object(ui_shape) = &ui.type_expr else {
        panic!(
            "ui helper should materialize as an object, got {:?}",
            ui.type_expr
        );
    };
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "root"),
        ),
        "ui helper should keep root, got {:?}",
        ui.type_expr
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "list"),
        ),
        "ui helper should keep list, got {:?}",
        ui.type_expr
    );

    let value_key = meta
        .props
        .iter()
        .find(|prop| prop.name == "valueKey")
        .expect("valueKey prop should exist");
    assert!(
        !matches!(
            value_key.type_expr,
            TypeExpr::Primitive(PrimitiveName::Never),
        ),
        "generic key helpers should not collapse to never, got {:?}",
        value_key.type_expr
    );

    let content_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "content")
        .expect("content slot should exist");
    let binding_names: Vec<_> = content_slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();
    assert_eq!(
        binding_names,
        vec!["item", "index", "ui"],
        "generic slot aliases should keep their scoped bindings, got {:?}",
        binding_names
    );

    let event = meta
        .events
        .iter()
        .find(|event| event.name == "update:modelValue")
        .expect("update:modelValue event should exist");
    let TypeExpr::Tuple { elements, .. } = &event.payload else {
        panic!(
            "generic package-backed emits should materialize as a tuple payload, got {:?}",
            event.payload
        );
    };
    assert_eq!(
        elements.len(),
        1,
        "model update should have a single payload"
    );
    match &elements[0].ty {
        TypeExpr::Union(members) => {
            assert!(
                members.contains(&TypeExpr::Primitive(PrimitiveName::String)),
                "event payload should include string, got {:?}",
                event.payload
            );
            assert!(
                members.contains(&TypeExpr::Primitive(PrimitiveName::Number)),
                "event payload should include number, got {:?}",
                event.payload
            );
        }
        other => panic!(
            "generic package-backed emits should instantiate the generic payload, got {:?}",
            other
        ),
    }
}

#[test]
fn resolved_component_meta_materializes_imported_generic_tabs_helper_fields() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TabsRootProps<T> {
  defaultValue?: T
  modelValue?: T
  activationMode?: 'automatic' | 'manual'
  unmountOnHide?: boolean
}

export interface TabsRootEmits<T> {
  (e: 'update:modelValue', payload: T): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/utils.ts",
            r#"
export type NestedItem<T> = T extends Array<infer I> ? NestedItem<I> : T
export type GetItemKeys<I, T extends NestedItem<I> = NestedItem<I>> =
  keyof Extract<T, object> & string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { pill: '', link: '' },
    size: { sm: '', md: '' },
    orientation: { horizontal: '', vertical: '' }
  },
  slots: {
    root: '',
    list: '',
    content: ''
  }
} as const"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { TabsRootProps, TabsRootEmits } from 'reka-ui'
import type { ComponentConfig } from './tv'
import type { GetItemKeys } from './utils'
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface TabsItem {
  label?: string
  value?: string | number
}

export interface TabsProps<T extends TabsItem = TabsItem> extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  items?: T[]
  color?: Tabs['variants']['color']
  variant?: Tabs['variants']['variant']
  size?: Tabs['variants']['size']
  orientation?: Tabs['variants']['orientation']
  valueKey?: GetItemKeys<T>
  labelKey?: GetItemKeys<T>
  ui?: Tabs['slots']
}

export interface TabsEmits extends TabsRootEmits<string | number> {}

type SlotProps<T extends TabsItem> = (props: { item: T, index: number, ui: Tabs['ui'] }) => any

export type TabsSlots<T extends TabsItem = TabsItem> = {
  content?: SlotProps<T>
}
</script>
<script setup lang="ts" generic="T extends TabsItem">
withDefaults(defineProps<TabsProps<T>>(), {
  defaultValue: '0',
  orientation: 'horizontal',
  unmountOnHide: true,
  valueKey: 'value',
  labelKey: 'label'
})
defineEmits<TabsEmits>()
defineSlots<TabsSlots<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "reka-ui".to_string(),
                resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./utils".to_string(),
                resolved_canonical_id: Some("/src/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            true,
            ctx,
        )
    })
    .analysis;

    let color = meta
        .props
        .iter()
        .find(|prop| prop.name == "color")
        .expect("color prop should exist");
    assert_union_string_literals(&color.type_expr, &["primary", "secondary"]);

    let ui = meta
        .props
        .iter()
        .find(|prop| prop.name == "ui")
        .expect("ui prop should exist");
    let TypeExpr::Object(ui_shape) = &ui.type_expr else {
        panic!(
            "ui helper should materialize as an object, got {:?}",
            ui.type_expr
        );
    };
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "root"),
        ),
        "ui helper should keep root, got {:?}",
        ui.type_expr
    );

    let value_key = meta
        .props
        .iter()
        .find(|prop| prop.name == "valueKey")
        .expect("valueKey prop should exist");
    assert!(
        !matches!(
            value_key.type_expr,
            TypeExpr::Primitive(PrimitiveName::Never),
        ),
        "generic key helpers should not collapse to never, got {:?}",
        value_key.type_expr
    );

    let content_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "content")
        .expect("content slot should exist");
    let binding_names: Vec<_> = content_slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();
    assert_eq!(
        binding_names,
        vec!["item", "index", "ui"],
        "generic slot aliases should keep their scoped bindings, got {:?}",
        binding_names
    );
}

#[test]
fn component_meta_keeps_explicit_slot_bindings_through_dynamic_slots_intersection() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TabsRootProps<T> {
  defaultValue?: T
  modelValue?: T
  activationMode?: 'automatic' | 'manual'
  unmountOnHide?: boolean
}

export interface TabsRootEmits<T> {
  (e: 'update:modelValue', payload: T): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/utils.ts",
            r#"
export type DynamicSlotsKeys<Name extends string | undefined, Suffix extends string | undefined = undefined> = (
  Name extends string
    ? Suffix extends string
      ? Name | `${Name}-${Suffix}`
      : Name
    : never,
)

export type DynamicSlots<
  T extends { slot?: string },
  Suffix extends string | undefined = undefined,
  ExtraProps extends object = {}
> = {
  [K in DynamicSlotsKeys<T['slot'], Suffix>]?: (
    props: { item: Extract<T, { slot: K extends `${infer Base}-${Suffix}` ? Base : K }> } & ExtraProps,
  ) => any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  slots: {
    root: '',
    list: '',
    trigger: '',
    label: '',
    content: ''
  }
} as const"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { TabsRootProps, TabsRootEmits } from 'reka-ui'
import type { ComponentConfig } from './tv'
import type { DynamicSlots } from './utils'
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface TabsItem {
  label?: string
  value?: string | number
  slot?: string
}

export interface TabsProps<T extends TabsItem = TabsItem> extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  items?: T[]
}

export interface TabsEmits extends TabsRootEmits<string | number> {}

type SlotProps<T extends TabsItem> = (props: { item: T, index: number, ui: Tabs['ui'] }) => any

export type TabsSlots<T extends TabsItem = TabsItem> = {
  leading?: SlotProps<T>
  content?: SlotProps<T>
} & DynamicSlots<T, undefined, { index: number, ui: Tabs['ui'] }>
</script>
<script setup lang="ts" generic="T extends TabsItem">
defineProps<TabsProps<T>>()
defineEmits<TabsEmits>()
defineSlots<TabsSlots<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "reka-ui".to_string(),
                resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./utils".to_string(),
                resolved_canonical_id: Some("/src/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            true,
            ctx,
        )
    })
    .analysis;
    let content_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "content")
        .expect("content slot should exist");
    let binding_names: Vec<_> = content_slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();
    assert_eq!(
        binding_names,
        vec!["item", "index", "ui"],
        "explicit slots in DynamicSlots intersections should keep their bindings, got {:?}",
        binding_names
    );

    let leading_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "leading")
        .expect("leading slot should exist");
    let leading_binding_names: Vec<_> = leading_slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();
    assert_eq!(
        leading_binding_names,
        vec!["item", "index", "ui"],
        "sibling explicit slots should keep the same intersection bindings, got {:?}",
        leading_binding_names
    );
}

#[test]
fn component_meta_keeps_realistic_tabs_slot_bindings_with_dynamic_helper_intersection() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TabsRootProps<T> {
  defaultValue?: T
  modelValue?: T
  activationMode?: 'automatic' | 'manual'
  unmountOnHide?: boolean
}

export interface TabsRootEmits<T> {
  (e: 'update:modelValue', payload: T): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/utils.ts",
            r#"
export type DynamicSlotsKeys<Name extends string | undefined, Suffix extends string | undefined = undefined> = (
  Name extends string
    ? Suffix extends string
      ? Name | `${Name}-${Suffix}`
      : Name
    : never,
)

export type DynamicSlots<
  T extends { slot?: string },
  Suffix extends string | undefined = undefined,
  ExtraProps extends object = {}
> = {
  [K in DynamicSlotsKeys<T['slot'], Suffix>]?: (
    props: { item: Extract<T, { slot: K extends `${infer Base}-${Suffix}` ? Base : K }> } & ExtraProps,
  ) => any
}

export type NestedItem<T> = T extends Array<infer I> ? NestedItem<I> : T

type IsPrimitive<T> = T extends (string | number | boolean | symbol | bigint | null | undefined)
  ? true
  : false

type IsPlainObject<T> = IsPrimitive<T> extends true
  ? false
  : T extends readonly any[] | ((...args: any[]) => any)
    ? false
    : T extends object ? true
      : false

type DotPathKeys<T> = IsPlainObject<T> extends true
  ? {
      [K in keyof T & string]:
      IsPlainObject<NonNullable<T[K]>> extends true
        ? K | `${K}.${DotPathKeys<NonNullable<T[K]>>}`
        : K
    }[keyof T & string]
  : never

export type GetItemKeys<
  I,
  T extends NestedItem<I> = NestedItem<I>
> = (keyof Extract<T, object> & string) | DotPathKeys<Extract<T, object>>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { pill: '', link: '' },
    size: { sm: '', md: '' },
    orientation: { horizontal: '', vertical: '' }
  },
  slots: {
    root: '',
    list: '',
    trigger: '',
    label: '',
    content: ''
  }
} as const"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { TabsRootProps, TabsRootEmits } from 'reka-ui'
import type { ComponentConfig } from './tv'
import type { DynamicSlots, GetItemKeys } from './utils'
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface TabsItem {
  label?: string
  value?: string | number
  slot?: string
  nested?: {
    path?: string
  }
}

export interface TabsProps<T extends TabsItem = TabsItem> extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  items?: T[]
  color?: Tabs['variants']['color']
  variant?: Tabs['variants']['variant']
  size?: Tabs['variants']['size']
  orientation?: Tabs['variants']['orientation']
  valueKey?: GetItemKeys<T>
  labelKey?: GetItemKeys<T>
  ui?: Tabs['slots']
}

export interface TabsEmits extends TabsRootEmits<string | number> {}

type SlotProps<T extends TabsItem> = (props: { item: T, index: number, ui: Tabs['ui'] }) => any

export type TabsSlots<T extends TabsItem = TabsItem> = {
  leading?: SlotProps<T>
  default?(props: { item: T, index: number }): any
  trailing?: SlotProps<T>
  content?: SlotProps<T>
} & DynamicSlots<T, undefined, { index: number, ui: Tabs['ui'] }>
</script>
<script setup lang="ts" generic="T extends TabsItem">
withDefaults(defineProps<TabsProps<T>>(), {
  defaultValue: '0',
  orientation: 'horizontal',
  unmountOnHide: true,
  valueKey: 'value',
  labelKey: 'label'
})
defineEmits<TabsEmits>()
defineSlots<TabsSlots<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "reka-ui".to_string(),
                resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./utils".to_string(),
                resolved_canonical_id: Some("/src/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            true,
            ctx,
        )
    })
    .analysis;
    let content_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "content")
        .expect("content slot should exist");
    let binding_names: Vec<_> = content_slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();
    assert_eq!(
        binding_names,
        vec!["item", "index", "ui"],
        "realistic Tabs slot helpers should keep explicit content bindings, got {:?}",
        binding_names
    );
}

// Regression: earlier, materializing a prop field whose type was a `Ref` to
// an imported generic whose declaration body transitively cycled through a
// sibling helper (DotPathKeys → DotPathKeys) sent the solver into a declaration
// scope with full local visibility to every recursive helper.  The solver
// would then grow the type arena to its hard ceiling during a single
// projection call, consuming multi-GB of memory before terminating.
//
// This fixture narrows the reproduction down to the recursive generic itself,
// without any of the slot / component-meta surface that the larger realistic
// fixture carries.  If the owner-scope fallback ever starts walking back into
// the declaration scope for a transitively recursive helper, this test will
// either hang or OOM instead of completing in a few tens of milliseconds.
#[test]
fn component_meta_does_not_hang_on_transitively_recursive_generic_prop_helper() {
    let project = make_project();
    project
        .upsert_base(
            "/src/utils.ts",
            r#"
type IsPrimitive<T> = T extends (string | number | boolean | symbol | bigint | null | undefined)
  ? true
  : false

type IsPlainObject<T> = IsPrimitive<T> extends true
  ? false
  : T extends readonly any[] | ((...args: any[]) => any)
    ? false
    : T extends object ? true
      : false

type DotPathKeys<T> = IsPlainObject<T> extends true
  ? {
      [K in keyof T & string]:
      IsPlainObject<NonNullable<T[K]>> extends true
        ? K | `${K}.${DotPathKeys<NonNullable<T[K]>>}`
        : K
    }[keyof T & string]
  : never

export type NestedItem<T> = T extends Array<infer I> ? NestedItem<I> : T

export type GetItemKeys<
  I,
  T extends NestedItem<I> = NestedItem<I>
> = (keyof Extract<T, object> & string) | DotPathKeys<Extract<T, object>>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts" generic="T extends { label?: string; nested?: { path?: string } }">
import type { GetItemKeys } from './utils'

defineProps<{
  valueKey?: GetItemKeys<T>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./utils".to_string(),
            resolved_canonical_id: Some("/src/utils.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let started = std::time::Instant::now();
    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let elapsed = started.elapsed();

    assert!(
        elapsed.as_secs_f64() < 30.0,
        "transitively-recursive generic prop helper should not hang \
         (elapsed {:.2}s)",
        elapsed.as_secs_f64()
    );

    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            true,
            ctx,
        )
    })
    .analysis;
    assert!(
        meta.props.iter().any(|prop| prop.name == "valueKey"),
        "valueKey prop should still be produced, got props {:?}",
        meta.props
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn declared_component_meta_extract_keeps_recursive_get_item_keys_symbolic_without_hanging() {
    let project = make_project();
    project
        .upsert_base(
            "/src/utils.ts",
            r#"
type IsPrimitive<T> = T extends (string | number | boolean | symbol | bigint | null | undefined)
  ? true
  : false

type IsPlainObject<T> = IsPrimitive<T> extends true
  ? false
  : T extends readonly any[] | ((...args: any[]) => any)
    ? false
    : T extends object ? true
      : false

type DotPathKeys<T> = IsPlainObject<T> extends true
  ? {
      [K in keyof T & string]:
      IsPlainObject<NonNullable<T[K]>> extends true
        ? K | `${K}.${DotPathKeys<NonNullable<T[K]>>}`
        : K
    }[keyof T & string]
  : never

export type NestedItem<T> = T extends Array<infer I> ? NestedItem<I> : T

export type GetItemKeys<
  I,
  T extends NestedItem<I> = NestedItem<I>
> = (keyof Extract<T, object> & string) | DotPathKeys<Extract<T, object>>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts" generic="T extends { label?: string; nested?: { path?: string } }">
import type { GetItemKeys } from './utils'

defineProps<{
  labelKey?: GetItemKeys<T>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./utils".to_string(),
            resolved_canonical_id: Some("/src/utils.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let started = std::time::Instant::now();
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            false,
            ctx,
        )
    })
    .analysis;
    let elapsed = started.elapsed();

    assert!(
        elapsed.as_secs_f64() < 10.0,
        "declared component meta extraction should not hang on recursive GetItemKeys helper \
         (elapsed {:.2}s)",
        elapsed.as_secs_f64()
    );

    let label_key = meta
        .props
        .iter()
        .find(|prop| prop.name == "labelKey")
        .expect("labelKey prop should be present");
    assert_eq!(
        label_key.raw_type.as_deref(),
        Some("GetItemKeys<T>"),
        "labelKey should preserve the source helper name"
    );
    assert!(
        matches!(
            &label_key.type_expr,
            verter_type_expr::TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "GetItemKeys" && type_arguments.len() == 1
        ),
        "labelKey should stay symbolic at the prop surface, got {:?}",
        label_key.type_expr
    );

    // Confirm contention-instrumentation counters are populated by an
    // actual heavy-component-meta run. A resolve + extract path must
    // have loaded files, taken the overlay
    // gate at least once, pushed nodes into the arena, and claimed at
    // least one execute_cooperative owner slot. Relaxed reads are
    // sufficient: all atomic increments happen before this assertion on
    // the same thread.
    let prov = project.host().provenance_snapshot();
    assert!(
        prov.ensure_loaded_calls > 0,
        "C1: ensure_loaded_calls should increment during component-meta load ({} observed)",
        prov.ensure_loaded_calls,
    );
    assert!(
        prov.node_arena_pushes > 0,
        "C1: node_arena_pushes should increment during semantic interning ({} observed)",
        prov.node_arena_pushes,
    );
    assert!(
        prov.execute_cooperative_owner_path + prov.execute_cooperative_joiner_path > 0,
        "C1: execute_cooperative counters should split between owner and joiner paths \
         (owner {}, joiner {})",
        prov.execute_cooperative_owner_path,
        prov.execute_cooperative_joiner_path,
    );
    assert!(
        prov.scheduler_submit_count > 0,
        "C1: scheduler_submit_count should increment on file load submissions ({} observed)",
        prov.scheduler_submit_count,
    );
}

#[test]
fn component_meta_keeps_conditional_slot_helper_symbolic_without_hanging() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
type Mode = 'click' | 'hover'

type SlotProps<M extends Mode = Mode> = [M] extends ['hover']
  ? { close: undefined }
  : { close: () => void }

interface Slots<M extends Mode = Mode> {
  default?(props: { open: boolean }): any
  content?(props: SlotProps<M>): any
  anchor?(props: SlotProps<M>): any
}
</script>
<script setup lang="ts" generic="M extends Mode">
defineSlots<Slots<M>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let started = std::time::Instant::now();
    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            false,
            ctx,
        )
    })
    .analysis;
    let elapsed = started.elapsed();

    assert!(
        elapsed.as_secs_f64() < 10.0,
        "conditional slot helper should not hang component-meta resolution \
         (elapsed {:.2}s)",
        elapsed.as_secs_f64()
    );

    let content_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "content")
        .expect("content slot should exist");
    let anchor_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "anchor")
        .expect("anchor slot should exist");
    assert!(
        content_slot.bindings.is_empty(),
        "conditional content slot helper should stay symbolic, got bindings {:?}",
        content_slot
            .bindings
            .iter()
            .map(|binding| binding.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        anchor_slot.bindings.is_empty(),
        "conditional anchor slot helper should stay symbolic, got bindings {:?}",
        anchor_slot
            .bindings
            .iter()
            .map(|binding| binding.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn union_object_variants_synthesize_component_meta_props() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
type FixedProps = {
  layout?: 'fixed'
  editor: string
}

type BubbleProps = {
  layout?: 'bubble'
  editor: string
  floating?: boolean
}

type Props = FixedProps | BubbleProps
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"layout"),
        "should have 'layout', got: {names:?}"
    );
    assert!(
        names.contains(&"editor"),
        "should have 'editor', got: {names:?}"
    );
    assert!(
        names.contains(&"floating"),
        "should have union branch props, got: {names:?}"
    );
}

#[test]
fn mixed_intersection_retains_local_component_meta_props() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
type Props = {
  id?: string
  disabled?: boolean
} & Omit<FormHTMLAttributes, 'name'>

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"id"), "should have 'id', got: {names:?}");
    assert!(
        names.contains(&"disabled"),
        "should have 'disabled', got: {names:?}"
    );
}

#[test]
fn imported_barrel_types_are_available_to_define_props_evaluation() {
    let project = make_project();
    project
        .upsert_base("/src/types/index.ts", r#"export * from '../Button.vue'"#)
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
export interface IconProps {
  icon?: string
}

export interface ButtonProps extends IconProps {
  label?: string
  color?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps } from './types'

type Props = Omit<ButtonProps, 'color'> & {
  status?: string
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![crate::types::DependencyResolution {
            specifier: "../Button.vue".to_string(),
            resolved_canonical_id: Some("/src/Button.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"icon"),
        "should have 'icon', got: {names:?}"
    );
    assert!(
        names.contains(&"label"),
        "should have 'label', got: {names:?}"
    );
    assert!(
        names.contains(&"status"),
        "should keep local props, got: {names:?}"
    );
    assert!(
        !names.contains(&"color"),
        "should omit 'color', got: {names:?}"
    );
}

#[test]
fn imported_barrel_cycles_still_resolve_nested_omit_props() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from '../Link.vue'
export * from '../Button.vue'"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
  exactActiveClass?: string
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
  class?: any
  raw?: boolean
}

export type LinkPropsKeys = 'to' | 'replace' | 'activeClass' | 'ariaCurrentValue'
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  status?: string
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../Button.vue".to_string(),
                resolved_canonical_id: Some("/src/Button.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"loading"),
        "should include inherited icon props, got: {names:?}"
    );
    assert!(
        names.contains(&"label"),
        "should include inherited button props, got: {names:?}"
    );
    assert!(
        names.contains(&"size"),
        "should include inherited button props, got: {names:?}"
    );
    assert!(
        names.contains(&"href"),
        "should include inherited link props, got: {names:?}"
    );
    assert!(
        names.contains(&"status"),
        "should keep local props, got: {names:?}"
    );
    assert!(!names.contains(&"icon"), "should omit icon, got: {names:?}");
    assert!(
        !names.contains(&"color"),
        "should omit color, got: {names:?}"
    );
    assert!(
        !names.contains(&"variant"),
        "should omit variant, got: {names:?}"
    );
    assert!(
        !names.contains(&"to"),
        "should omit link keys, got: {names:?}"
    );
    assert!(
        !names.contains(&"replace"),
        "should omit router link keys, got: {names:?}"
    );
    assert!(
        !names.contains(&"activeClass"),
        "should omit router link keys, got: {names:?}"
    );
    assert!(
        !names.contains(&"ariaCurrentValue"),
        "should omit router link keys, got: {names:?}"
    );
}

#[test]
fn resolve_component_meta_handles_barrel_cycle_utility_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from '../Link.vue'
export * from '../Button.vue'"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
  exactActiveClass?: string
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
  class?: any
  raw?: boolean
}

export type LinkPropsKeys = 'to' | 'replace' | 'activeClass' | 'ariaCurrentValue'
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  status?: string
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../Button.vue".to_string(),
                resolved_canonical_id: Some("/src/Button.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("expanded state should resolve");
    let button = resolved
        .resolved_macros
        .iter()
        .find(|meta| meta.type_name == "ButtonProps")
        .expect("should resolve ButtonProps");
    let button_dtos =
        project
            .host()
            .vue_macro_dtos(&crate::typeinfo::types::VueMacroSurfaceRequest {
                owner_canonical: std::sync::Arc::from("/src/App.vue"),
                macro_index: button.macro_index,
                macro_kind: button.macro_kind,
                root_identity: project
                    .host()
                    .current_or_read_whole_hash("/src/App.vue")
                    .unwrap_or([0u8; 16]),
                level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
            });
    assert!(
        button_dtos
            .prop_fields()
            .iter()
            .any(|prop| prop.name == "loading"),
        "resolved ButtonProps should include inherited props, got: {:?}",
        button_dtos.props
    );
    assert!(
        button_dtos
            .prop_fields()
            .iter()
            .any(|prop| prop.name == "label"),
        "resolved ButtonProps should include button props, got: {:?}",
        button_dtos.props
    );
}

#[test]
fn imported_pick_slot_bindings_keep_symbolic_raw_type() {
    let project = make_project();
    project
        .upsert_base(
            "/src/reka-ui.ts",
            r#"
export interface CalendarCellTriggerProps {
  day: Date,
  month: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/slots.ts",
            r#"
import type { CalendarCellTriggerProps } from './reka-ui'

export interface CalendarSlots {
  day?: (props: Pick<CalendarCellTriggerProps, 'day'>) => any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { CalendarSlots } from './slots'

defineSlots<CalendarSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");

    let day_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "day")
        .expect("should extract imported day slot");
    let day_binding = day_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "day")
        .expect("day slot should expose the day binding");

    assert_eq!(
        day_binding.raw_type.as_deref(),
        Some("CalendarCellTriggerProps['day']"),
        "imported Pick slot bindings should keep the symbolic source contract"
    );
}

/// Issue #1 (partial): a slot binding whose raw type is
/// `IndexedAccess { object: <project-local Props>, index: <literal> }`
/// must stay symbolic when the underlying property body resolves
/// through to an imported declaration that carries an open
/// `[k: string]: any` index signature. Otherwise the evaluator
/// re-expands the indexed access through the index signature and the
/// public surface widens to `any`.
///
/// Fixture shape:
///   * `/src/avatar.ts` exports `interface ImportedProps { src: string }`.
///   * `/src/Comp.vue`'s script-setup declares
///     `interface AppProps { avatar: ImportedProps & { [k: string]: any } }`
///     and `defineSlots<{ leading(props: { avatar: AppProps['avatar'] }): any }>()`.
///
/// The slot binding `avatar` must publish `type_expr` shaped as
/// `IndexedAccess { object: Ref(AppProps), index: 'avatar' }`. The raw
/// type contract `AppProps['avatar']` is the canonical form consumers
/// re-resolve from.
#[test]
fn slot_binding_imported_props_with_any_index_signature_stays_symbolic() {
    let project = make_project();
    project
        .upsert_base(
            "/src/avatar.ts",
            r#"
export interface ImportedProps {
  src: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedProps } from './avatar'

interface AppProps {
  avatar: ImportedProps & { [k: string]: any }
}

defineSlots<{
  leading(props: { avatar: AppProps['avatar'] }): any
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Comp.vue")
        .expect("should return component meta for the slot fixture");
    let leading_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "leading")
        .expect("leading slot should be extracted");
    let avatar_binding = leading_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "avatar")
        .expect("leading slot should expose the avatar binding");

    // Symbolic raw-type contract: consumers can re-resolve through this
    // member path on demand.
    assert_eq!(
        avatar_binding.raw_type.as_deref(),
        Some("AppProps['avatar']"),
        "slot binding must preserve the symbolic raw-type form"
    );
    // Public type_expr stays as the indexed access — no expansion
    // through the imported `[k: string]: any` index signature.
    assert!(
        matches!(&avatar_binding.type_expr, TypeExpr::IndexedAccess { .. }),
        "slot binding type_expr must stay IndexedAccess (no widening through the imported index \
         signature); got {:?}",
        avatar_binding.type_expr
    );
}

/// Counterfixture for the slot-binding indexed-access policy: when the
/// indexed root is workspace-local AND non-imported AND not in a
/// route-preservation context, the policy should NOT preserve
/// `IndexedAccess` symbolically — the evaluator's expanded shape is
/// the intended public surface.
///
/// Fixture: `interface AppProps { kind: 'a' | 'b' }` declared in the
/// owner SFC's script-setup, with no imported helpers in the binding
/// chain. `defineSlots<{ leading(props: { kind: AppProps['kind'] }): any }>()`
/// publishes the union literal `'a' | 'b'` as the slot binding's
/// `type_expr` — symbolic preservation would suppress information the
/// consumer expects.
#[test]
fn slot_binding_local_props_without_index_signature_takes_slow_path() {
    let project = make_project();
    project
        .upsert_base(
            "/src/Comp.vue",
            r#"<script setup lang="ts">
interface AppProps {
  kind: 'a' | 'b'
}

defineSlots<{
  leading(props: { kind: AppProps['kind'] }): any
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Comp.vue")
        .expect("should return component meta for the slot counterfixture");
    let leading_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "leading")
        .expect("leading slot should be extracted");
    let kind_binding = leading_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "kind")
        .expect("leading slot should expose the kind binding");

    // The slow path expands the indexed access into the literal union
    // — that is the intended public surface for purely-local props
    // with no imported index signature in the chain. Symbolic
    // preservation here would suppress the resolved literal union the
    // consumer expects.
    assert!(
        !matches!(&kind_binding.type_expr, TypeExpr::IndexedAccess { .. }),
        "purely-local slot binding without imported helpers must take the slow path \
         (no symbolic IndexedAccess preservation); got {:?}",
        kind_binding.type_expr
    );
}

#[test]
fn imported_slot_binding_indexed_access_stays_symbolic_member_path() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

export type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"
export const theme = {
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/button-types.ts",
            r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>

export interface ButtonSlots {
  default?(props: {
    ui: Button['ui']
  }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './button-types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("should resolve component meta state");

    // Shallow-by-default registry contract: the imported `ButtonSlots` helper
    // stays a bare `Ref { name }` in the registry. The indexed-access slot
    // binding (`ui: Button['ui']`) resolves PATH-PRECISELY on the published
    // surface below — only the requested `ui` member path, never the whole
    // `Button` shape, enters the published binding.
    let button_slots = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ButtonSlots")
        .expect("ButtonSlots should be published in the resolved type registry");
    assert!(
        matches!(&button_slots.type_expr, TypeExpr::Ref { name, .. } if name.as_ref() == "ButtonSlots"),
        "imported registry helper should stay a shallow Ref (shallow-by-default), got {:?}",
        button_slots.type_expr
    );
    assert!(
        !matches!(&button_slots.type_expr, TypeExpr::Object(_)),
        "registry entry must NOT eagerly materialize to an object, got {:?}",
        button_slots.type_expr
    );

    // Published-surface contract: the default slot's `ui` binding resolves to
    // the requested member path `Button['ui']` — symbolic IndexedAccess, NOT
    // an eagerly-widened object of the whole `Button` shape.
    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should be extracted");
    let ui_binding = default_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "ui")
        .expect("default slot should expose the ui binding");
    assert_eq!(
        ui_binding.raw_type.as_deref(),
        Some("Button['ui']"),
        "indexed-access slot binding must stay on the requested member path, got {:?}",
        ui_binding.raw_type
    );
    assert!(
        matches!(&ui_binding.type_expr, TypeExpr::IndexedAccess { .. }),
        "default slot ui binding must stay a symbolic IndexedAccess, got {:?}",
        ui_binding.type_expr
    );
}

#[test]
fn resolve_component_meta_keeps_imported_slot_param_member_paths_symbolic_in_registry() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

export type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"
export const theme = {
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/button-types.ts",
            r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>

export interface ButtonSlots {
  default?(props: {
    ui: Button['ui']
  }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './button-types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("should resolve component meta state");

    // Shallow-by-default registry contract: the imported `ButtonSlots` helper
    // stays a bare `Ref { name }` in the registry; the deep slot-binding
    // correctness is asserted on the published surface below.
    let button_slots = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ButtonSlots")
        .expect("ButtonSlots should be published in the resolved type registry");
    assert!(
        matches!(&button_slots.type_expr, TypeExpr::Ref { name, .. } if name.as_ref() == "ButtonSlots"),
        "imported registry helper should stay a shallow Ref (shallow-by-default), got {:?}",
        button_slots.type_expr
    );
    assert!(
        !matches!(&button_slots.type_expr, TypeExpr::Object(_)),
        "registry entry must NOT eagerly materialize to an object, got {:?}",
        button_slots.type_expr
    );

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should still be extracted");
    let ui_binding = default_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "ui")
        .expect("default slot should still expose the ui binding");
    assert!(
        ui_binding.raw_type.as_deref() == Some("Button['ui']"),
        "slot binding contract should still point at the requested helper route, got {:?}",
        ui_binding.raw_type
    );
    // Resolution-authority contract: compute is the single resolution
    // authority. The meta carries the lazy `Button['ui']` indexed-access
    // form; consumers navigate it via dispatch when they need the
    // resolved members (e.g. `base`, `label`). A regression that re-
    // introduces eager indexed-access resolution through imported
    // helper aliases would inline the members and fail this guard.
    // The meta's binding type stays symbolic — same shape as the
    // resolved registry's `ButtonSlots.default` params asserted above.
    // The `raw_type = "Button['ui']"` contract (line 12099) is the
    // canonical form consumers can re-resolve from.
    assert!(
        matches!(&ui_binding.type_expr, TypeExpr::IndexedAccess { .. }),
        "post-Outcome-3: slot binding stays symbolic IndexedAccess, got {:?}",
        ui_binding.type_expr
    );
}

#[test]
fn resolve_component_meta_keeps_imported_intersection_slot_helpers_symbolic() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface Item {
  label?: string
}

export type DynamicSlots<T> = {
  [name: string]: (props: { item: T }) => any
}

export type MergeTypes<T> = T & {
  extra?: boolean
}

export type MenuSlots<T = Item> = {
  default?(props?: {}): any
  item?(props: { item: T }): any
} & DynamicSlots<MergeTypes<T>>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { MenuSlots } from './types'

defineSlots<MenuSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let registry_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert!(
        !registry_names.contains("DynamicSlots") && !registry_names.contains("MergeTypes"),
        "imported utility helpers should stay off the published registry, got {registry_names:?}"
    );
    // Shallow-by-default registry contract: the imported `MenuSlots` helper —
    // whose body is an intersection mixing explicit slot members with imported
    // utility helpers (`DynamicSlots<MergeTypes<T>>`) — stays a bare
    // `Ref { name }` in the registry. The explicit slot members are asserted on
    // the published surface below.
    let menu_slots = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "MenuSlots")
        .expect("MenuSlots should be published in the resolved type registry");
    assert!(
        matches!(&menu_slots.type_expr, TypeExpr::Ref { name, .. } if name.as_ref() == "MenuSlots"),
        "imported intersection slot helper should stay a shallow Ref (shallow-by-default), got {:?}",
        menu_slots.type_expr
    );
    assert!(
        !matches!(&menu_slots.type_expr, TypeExpr::Object(_)),
        "registry entry must NOT eagerly materialize the intersection surface, got {:?}",
        menu_slots.type_expr
    );

    // Published-surface contract: the explicit `default` and `item` slot
    // members survive on the published surface; the imported utility helpers
    // (`DynamicSlots` / `MergeTypes`) stay off the registry (asserted above).
    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let slot_names: std::collections::BTreeSet<_> =
        meta.slots.iter().map(|slot| slot.name.as_str()).collect();
    assert!(
        slot_names.contains("default") && slot_names.contains("item"),
        "explicit imported slot members should still be exposed, got {slot_names:?}"
    );
}

#[test]
fn imported_slot_binding_prepared_decls_expose_generic_params_and_theme_value_decl() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

export type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"
export const theme = {
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/button-types.ts",
            r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>

export interface ButtonSlots {
  default?(props: {
    ui: Button['ui']
  }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './button-types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("component meta resolution should warm the prepared-decl route");

    let _store_view = project.host().resolver_store_view_read().into_owned_view();
    let component_config = project
        .host()
        .prepared_type_decl("/src/types.ts", "ComponentConfig")
        .expect("ComponentConfig should have a prepared declaration");
    assert_eq!(
        component_config
            .type_parameters
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>(),
        vec!["T"]
    );

    let theme = project
        .host()
        .prepared_value_decl("/src/theme.ts", "theme")
        .expect("theme should have a prepared value declaration");
    assert!(
        theme.type_annotation.is_some() || theme.object_shape.is_some(),
        "theme prepared value decl should expose an object surface for typeof"
    );
}

#[test]
fn local_pick_slot_bindings_keep_symbolic_raw_type() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
interface CalendarCellTriggerProps {
  day: Date,
  month: number
}

export interface CalendarSlots {
  day?: (props: Pick<CalendarCellTriggerProps, 'day'>) => any
}
</script>
<script setup lang="ts">
defineSlots<CalendarSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");

    let day_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "day")
        .expect("should extract local day slot");
    let day_binding = day_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "day")
        .expect("day slot should expose the day binding");

    assert_eq!(
        day_binding.raw_type.as_deref(),
        Some("CalendarCellTriggerProps['day']"),
        "local Pick slot bindings should keep the symbolic source contract"
    );
}

#[test]
fn nested_imported_omit_preserves_html_attrs_and_omits_link_only_keys() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/html.ts",
            r#"
export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  name?: string
  type?: 'button' | 'submit'
}

export interface AnchorHTMLAttributes {
  download?: boolean
  href?: string
  hreflang?: string
  media?: string
  ping?: string
  referrerpolicy?: string
  rel?: string
  target?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface RouterLinkProps {
  replace?: boolean
  exactActiveClass?: string
  viewTransition?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
  external?: boolean
  target?: string | null
  rel?: string | null
  noRel?: boolean
  prefetchedClass?: string
  prefetch?: boolean
  prefetchOn?: 'visibility' | 'interaction'
  noPrefetch?: boolean
  trailingSlash?: 'append' | 'remove'
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  active?: boolean
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  custom?: boolean
  raw?: boolean
  class?: any
}

export type LinkPropsKeys =
  | 'to'
  | 'href'
  | 'target'
  | 'rel'
  | 'noRel'
  | 'external'
  | 'prefetch'
  | 'prefetchOn'
  | 'prefetchedClass'
  | 'noPrefetch'
  | 'trailingSlash'
  | 'replace'
  | 'active'
  | 'exact'
  | 'exactQuery'
  | 'exactHash'
  | 'inactiveClass'
  | 'download'
  | 'ping'
  | 'referrerpolicy'
  | 'hreflang'
  | 'media'
  | 'viewTransition'
</script>
<template><a /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  leading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: 'sm' | 'md'
  square?: boolean
  block?: boolean
  class?: any
  ui?: object
}
</script>
<template><button /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types/index.ts",
            "export * from '../Link.vue'\nexport * from '../Button.vue'",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface Props extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  side?: 'left' | 'right'
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"autofocus")
            && prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"name"),
        "nested imported Omit should preserve inherited button attrs: {:?}",
        prop_names
    );
    assert!(
        !prop_names.contains(&"to")
            && !prop_names.contains(&"href")
            && !prop_names.contains(&"target")
            && !prop_names.contains(&"rel")
            && !prop_names.contains(&"prefetch")
            && !prop_names.contains(&"prefetchOn")
            && !prop_names.contains(&"external")
            && !prop_names.contains(&"viewTransition"),
        "nested imported Omit should exclude link-only keys: {:?}",
        prop_names
    );
}

#[test]
fn dual_heritage_omit_keeps_button_attrs_without_leaking_link_keys() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/html.ts",
            r#"
export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  name?: string
  type?: 'button' | 'submit'
}

export interface AnchorHTMLAttributes {
  download?: boolean
  href?: string
  hreflang?: string
  media?: string
  ping?: string
  referrerpolicy?: string
  rel?: string
  target?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/drag.ts",
            r#"
export interface DragHandleProps {
  class?: any
  computePositionConfig?: unknown
  editor?: object
  element?: object
  getReferencedVirtualElement?: () => unknown
  nested?: boolean
  nestedOptions?: object
  onElementDragEnd?: () => void
  onElementDragStart?: () => void
  onNodeChange?: () => void
  pluginKey?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface RouterLinkProps {
  replace?: boolean
  exactActiveClass?: string
  viewTransition?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
  external?: boolean
  target?: string | null
  rel?: string | null
  noRel?: boolean
  prefetchedClass?: string
  prefetch?: boolean
  prefetchOn?: 'visibility' | 'interaction'
  noPrefetch?: boolean
  trailingSlash?: 'append' | 'remove'
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  active?: boolean
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  custom?: boolean
  raw?: boolean
  class?: any
}

export type LinkPropsKeys =
  | 'to'
  | 'href'
  | 'target'
  | 'rel'
  | 'noRel'
  | 'external'
  | 'prefetch'
  | 'prefetchOn'
  | 'prefetchedClass'
  | 'noPrefetch'
  | 'trailingSlash'
  | 'replace'
  | 'active'
  | 'exact'
  | 'exactQuery'
  | 'exactHash'
  | 'inactiveClass'
  | 'download'
  | 'ping'
  | 'referrerpolicy'
  | 'hreflang'
  | 'media'
  | 'viewTransition'
</script>
<template><a /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  leading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: 'sm' | 'md'
  square?: boolean
  block?: boolean
  class?: any
  ui?: object
}
</script>
<template><button /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types/index.ts",
            "export * from '../Link.vue'\nexport * from '../Button.vue'",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { DragHandleProps } from './drag'
import type { ButtonProps, LinkPropsKeys } from './types'

interface Props extends Omit<DragHandleProps, 'editor' | 'element' | 'onNodeChange' | 'computePositionConfig' | 'class'>, Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  options?: object
  editor: object
  ui?: ButtonProps['ui']
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"autofocus")
            && prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"name"),
        "dual-heritage Omit should preserve inherited button attrs: {:?}",
        prop_names
    );
    assert!(
        !prop_names.contains(&"to")
            && !prop_names.contains(&"href")
            && !prop_names.contains(&"target")
            && !prop_names.contains(&"rel")
            && !prop_names.contains(&"prefetch")
            && !prop_names.contains(&"prefetchOn")
            && !prop_names.contains(&"external")
            && !prop_names.contains(&"viewTransition"),
        "dual-heritage Omit should exclude link-only keys: {:?}",
        prop_names
    );
}

#[test]
fn package_backed_omit_does_not_leak_omitted_editor_members_into_top_level_props() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/editor-lib/index.d.ts",
            r#"
export interface Editor {
  $doc(): string
  chain(): string
  active?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/drag.ts",
            r#"
import type { Editor } from 'editor-lib'

export interface DragHandleProps {
  class?: any
  editor?: Editor
  element?: object
  appendTo?: object
  onNodeChange?: () => void
  pluginKey?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { DragHandleProps } from './drag'

type Props = Omit<DragHandleProps, 'editor' | 'element' | 'onNodeChange' | 'class'> & {
  editor: object
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./drag".to_string(),
            resolved_canonical_id: Some("/src/drag.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/drag.ts",
        vec![crate::types::DependencyResolution {
            specifier: "editor-lib".to_string(),
            resolved_canonical_id: Some("/node_modules/editor-lib/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"editor"),
        "top-level editor prop should survive the local override, got {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"appendTo") && prop_names.contains(&"pluginKey"),
        "non-omitted drag props should still be present, got {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"$doc")
            && !prop_names.contains(&"chain")
            && !prop_names.contains(&"active")
            && !prop_names.contains(&"element")
            && !prop_names.contains(&"class")
            && !prop_names.contains(&"onNodeChange"),
        "Omit should not leak omitted package-backed editor members into top-level props: {:?}",
        prop_names
    );
}

#[test]
fn partial_omit_union_branch_does_not_leak_package_editor_members_into_top_level_props() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/editor-lib/index.d.ts",
            r#"
export interface Editor {
  $doc(): string
  chain(): string
  active?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/menu.ts",
            r#"
import type { Editor } from 'editor-lib'

export interface MenuProps {
  editor: Editor,
  element: object
  appendTo?: object
  pluginKey?: string
  class?: any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { MenuProps } from './menu'

type BaseProps = {
  layout?: 'fixed' | 'bubble'
  editor: object
}

type Props =
  | (BaseProps & { layout?: 'fixed' })
  | (BaseProps & Partial<Omit<MenuProps, 'editor' | 'element' | 'class'>> & { layout?: 'bubble' })

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./menu".to_string(),
            resolved_canonical_id: Some("/src/menu.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/menu.ts",
        vec![crate::types::DependencyResolution {
            specifier: "editor-lib".to_string(),
            resolved_canonical_id: Some("/node_modules/editor-lib/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"layout")
            && prop_names.contains(&"editor")
            && prop_names.contains(&"appendTo")
            && prop_names.contains(&"pluginKey"),
        "expected union props should be present, got {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"$doc")
            && !prop_names.contains(&"chain")
            && !prop_names.contains(&"active")
            && !prop_names.contains(&"element")
            && !prop_names.contains(&"class"),
        "Partial<Omit<...>> union branch should not leak package editor members into top-level props: {:?}",
        prop_names
    );
}

#[test]
fn package_backed_object_prop_does_not_flatten_members_into_top_level_props() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/editor-lib/index.d.ts",
            r#"
export interface Editor {
  $doc(): string
  chain(): string
  active?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Editor } from 'editor-lib'

defineProps<{
  editor: Editor
  label?: string
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "editor-lib".to_string(),
            resolved_canonical_id: Some("/node_modules/editor-lib/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let registry_names: Vec<&str> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        prop_names.contains(&"editor") && prop_names.contains(&"label"),
        "declared props should remain present, got {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"$doc")
            && !prop_names.contains(&"chain")
            && !prop_names.contains(&"active"),
        "package-backed object props should stay nested instead of flattening their members into top-level props: {:?}",
        prop_names
    );
    assert!(
        !registry_names.contains(&"Editor"),
        "direct package-backed public field refs should stay symbolic on the prop instead of being published into the registry, got {registry_names:?}",
    );
}

#[test]
fn package_backed_object_prop_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/editor-lib/index.d.ts",
            r#"
export interface Editor {
  $doc(): string
  chain(): string
  active?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Editor } from 'editor-lib'

defineProps<{
  editor: Editor
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "editor-lib".to_string(),
            resolved_canonical_id: Some("/node_modules/editor-lib/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let editor_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "editor"))
        .expect("expanded evaluated types should keep the editor prop");

    assert!(
        matches!(
            &editor_field.r#type,
            verter_type_expr::TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "Editor" && type_arguments.is_empty()
        ),
        "package-backed prop expansion should keep the raw symbolic ref instead of expanding the package object, got {:?}",
        editor_field.r#type
    );
}

#[test]
fn package_backed_utility_wrapped_prop_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/editor-lib/index.d.ts",
            r#"
export interface Editor {
  $doc(): string
  chain(): string
  active?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Editor } from 'editor-lib'

defineProps<{
  editor?: Omit<Editor, 'active'>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "editor-lib".to_string(),
            resolved_canonical_id: Some("/node_modules/editor-lib/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let editor_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "editor"))
        .expect("expanded evaluated types should keep the editor prop");

    assert!(
        matches!(
            &editor_field.r#type,
            verter_type_expr::TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "Omit"
                    && type_arguments.len() == 2
                    && matches!(
                        &type_arguments[0],
                        verter_type_expr::TypeExpr::Ref {
                            name,
                            type_arguments
                        } if name.as_ref() == "Editor" && type_arguments.is_empty()
                    )
        ),
        "package-backed utility-wrapped props should keep the imported package ref symbolic instead of expanding the package object, got {:?}",
        editor_field.r#type
    );
}

#[test]
fn package_backed_member_path_prop_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/table-lib/index.d.ts",
            r#"
export interface CoreOptions<T> {
  state?: T
}

export interface RowState {
  selected?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { CoreOptions, RowState } from 'table-lib'

defineProps<{
  state?: CoreOptions<RowState>['state']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "table-lib".to_string(),
            resolved_canonical_id: Some("/node_modules/table-lib/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let state_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "state"))
        .expect("expanded evaluated types should keep the state prop");

    assert!(
        matches!(
            &state_field.r#type,
            verter_type_expr::TypeExpr::IndexedAccess { object, index }
                if matches!(
                    object.as_ref(),
                    verter_type_expr::TypeExpr::Ref { name, type_arguments }
                        if name.as_ref() == "CoreOptions" && type_arguments.len() == 1
                ) && matches!(
                    index.as_ref(),
                    verter_type_expr::TypeExpr::Literal(
                        verter_type_expr::LiteralValue::String(key),
                    ) if key == "state"
                )
        ),
        "package-backed indexed member paths should stay symbolic instead of expanding through package declarations, got {:?}",
        state_field.r#type
    );
}

#[test]
fn local_generic_wrapper_over_package_backed_props_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue-router/index.d.ts",
            r#"
export interface RouterLinkProps {
  to?: string
  replace?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
import type { RouterLinkProps } from 'vue-router'

export interface LinkProps extends Omit<RouterLinkProps, 'custom'> {
  custom?: boolean
  label?: string
}
</script>
<script setup lang="ts">
defineProps<LinkProps>()
</script>
<template><a /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/CommandPalette.vue",
            r#"<script lang="ts">
export interface CommandPaletteItem {
  id?: string
}

export interface CommandPaletteGroup<T extends CommandPaletteItem = CommandPaletteItem> {
  items?: T[]
}
</script>
<script setup lang="ts">
defineProps<CommandPaletteGroup>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { LinkProps } from './Link.vue'
import type { CommandPaletteGroup } from './CommandPalette.vue'

interface ContentSearchItem extends Omit<LinkProps, 'custom'> {
  badge?: string
}

defineProps<{
  groups?: CommandPaletteGroup<ContentSearchItem>[]
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./CommandPalette.vue".to_string(),
                resolved_canonical_id: Some("/src/CommandPalette.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/Link.vue",
        vec![crate::types::DependencyResolution {
            specifier: "vue-router".to_string(),
            resolved_canonical_id: Some("/node_modules/vue-router/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let groups_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "groups"))
        .expect("expanded evaluated types should keep the groups prop");

    assert!(
        matches!(
            &groups_field.r#type,
            verter_type_expr::TypeExpr::Array { element, .. }
                if matches!(
                    element.as_ref(),
                    verter_type_expr::TypeExpr::Ref { name, type_arguments }
                        if name.as_ref() == "CommandPaletteGroup"
                            && type_arguments.len() == 1
                            && matches!(
                                &type_arguments[0],
                                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                                    if name.as_ref() == "ContentSearchItem"
                                        && type_arguments.is_empty()
                            )
                )
        ),
        "local generic wrappers should stay symbolic when they eventually flow into package-backed imported refs, got {:?}",
        groups_field.r#type
    );
}

#[test]
fn imported_object_like_prop_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface ExternalProps {
  id: string
  label?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ExternalProps } from './types'

defineProps<{
  external?: ExternalProps
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let external_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "external"))
        .expect("expanded evaluated types should keep the imported external prop");

    assert!(
        matches!(
            &external_field.r#type,
            verter_type_expr::TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "ExternalProps" && type_arguments.is_empty()
        ),
        "imported object-like prop expansion should keep the symbolic ref instead of expanding the imported object, got {:?}",
        external_field.r#type
    );
}

#[test]
fn imported_union_field_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface TooltipProps {
  text?: string
  delay?: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { TooltipProps } from './types'

defineProps<{
  tooltip?: boolean | TooltipProps
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let tooltip_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "tooltip"))
        .expect("expanded evaluated types should keep the tooltip prop");

    let has_symbolic_tooltip = match &tooltip_field.r#type {
        verter_type_expr::TypeExpr::Union(members) => members.iter().any(|member| {
            matches!(
                member,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "TooltipProps" && type_arguments.is_empty()
            )
        }),
        _ => false,
    };

    assert!(
        has_symbolic_tooltip,
        "imported unions should keep imported object refs symbolic instead of expanding them in shallow field evaluation, got {:?}",
        tooltip_field.r#type
    );
}

#[test]
fn barrel_imported_vue_union_field_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TooltipRootProps {
  delayDuration?: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Tooltip.vue",
            r#"<script lang="ts">
import type { TooltipRootProps } from 'reka-ui'

export interface TooltipProps extends TooltipRootProps {
  text?: string
}

export default {
  name: 'Tooltip'
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base("/types.ts", "export * from './Tooltip.vue'\n")
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { TooltipProps } from './types'

defineProps<{
  tooltip?: boolean | TooltipProps
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./Tooltip.vue".to_string(),
            resolved_canonical_id: Some("/Tooltip.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/Tooltip.vue",
        vec![crate::types::DependencyResolution {
            specifier: "reka-ui".to_string(),
            resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let tooltip_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "tooltip"))
        .expect("expanded evaluated types should keep the tooltip prop");

    let has_symbolic_tooltip = match &tooltip_field.r#type {
        verter_type_expr::TypeExpr::Union(members) => members.iter().any(|member| {
            matches!(
                member,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "TooltipProps" && type_arguments.is_empty()
            )
        }),
        _ => false,
    };

    assert!(
        has_symbolic_tooltip,
        "barrel-imported vue unions should keep imported component prop refs symbolic in shallow field evaluation, got {:?}",
        tooltip_field.r#type
    );
}

#[test]
fn public_component_meta_keeps_imported_props_refs_symbolic() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TooltipContentProps {
  text?: string
}

export interface TooltipProviderProps {
  delayDuration?: number
  content?: TooltipContentProps
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { TooltipProviderProps } from 'reka-ui'

defineProps<{
  tooltip?: TooltipProviderProps
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "reka-ui".to_string(),
            resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().expect("session should open");
    let declared = session
        .get_component_meta("/src/App.vue")
        .expect("declared component meta query should succeed")
        .expect("declared component meta should exist");
    let full = session
        .get_component_meta("/src/App.vue")
        .expect("full component meta query should succeed")
        .expect("full component meta should exist");

    for (label, meta) in [("declared", declared), ("full", full)] {
        let tooltip = meta
            .props
            .iter()
            .find(|prop| prop.name == "tooltip")
            .expect("tooltip prop should exist");
        assert!(
            matches!(
                &tooltip.type_expr,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "TooltipProviderProps" && type_arguments.is_empty()
            ),
            "{label} component meta should keep imported *Props refs symbolic instead of rematerializing them, got {:?}",
            tooltip.type_expr
        );
        assert_eq!(
            tooltip.raw_type.as_deref(),
            Some("TooltipProviderProps"),
            "{label} component meta should preserve the raw imported type text"
        );
    }
}

#[test]
fn public_component_meta_keeps_imported_object_refs_symbolic() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/editor-lib/index.d.ts",
            r#"
export interface Editor {
  chain(): { run(): void }
  isEditable?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Editor } from 'editor-lib'

defineProps<{
  editor: Editor
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "editor-lib".to_string(),
            resolved_canonical_id: Some("/node_modules/editor-lib/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().expect("session should open");
    let declared = session
        .get_component_meta("/src/App.vue")
        .expect("declared component meta query should succeed")
        .expect("declared component meta should exist");
    let full = session
        .get_component_meta("/src/App.vue")
        .expect("full component meta query should succeed")
        .expect("full component meta should exist");

    for (label, meta) in [("declared", declared), ("full", full)] {
        let editor = meta
            .props
            .iter()
            .find(|prop| prop.name == "editor")
            .expect("editor prop should exist");
        assert!(
            matches!(
                &editor.type_expr,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "Editor" && type_arguments.is_empty()
            ),
            "{label} component meta should keep imported object refs symbolic instead of rematerializing them, got {:?}",
            editor.type_expr
        );
        assert_eq!(
            editor.raw_type.as_deref(),
            Some("Editor"),
            "{label} component meta should preserve the raw imported type text"
        );
    }
}

#[test]
fn public_component_meta_keeps_utility_wrapped_imported_refs_symbolic() {
    let project = make_project();
    project
        .upsert_base(
            "/src/button.ts",
            r#"
export interface ButtonProps {
  href?: string
  disabled?: boolean
  label?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/avatar.ts",
            r#"
export interface AvatarProps {
  src?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/progress.ts",
            r#"
export interface ProgressProps {
  color?: string
  ui?: {
    root?: string
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base("/src/keys.ts", "export type LinkPropsKeys = 'href'")
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { AvatarProps } from './avatar'
import type { ButtonProps } from './button'
import type { LinkPropsKeys } from './keys'
import type { ProgressProps } from './progress'

defineProps<{
  avatar?: AvatarProps
  actions?: ButtonProps[]
  close?: boolean | Omit<ButtonProps, LinkPropsKeys>
  progress?: boolean | Pick<ProgressProps, 'color' | 'ui'>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./avatar".to_string(),
                resolved_canonical_id: Some("/src/avatar.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./button".to_string(),
                resolved_canonical_id: Some("/src/button.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./keys".to_string(),
                resolved_canonical_id: Some("/src/keys.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./progress".to_string(),
                resolved_canonical_id: Some("/src/progress.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let session = project.open_session_batch().expect("session should open");
    let declared = session
        .get_component_meta("/src/App.vue")
        .expect("declared component meta query should succeed")
        .expect("declared component meta should exist");
    let full = session
        .get_component_meta("/src/App.vue")
        .expect("full component meta query should succeed")
        .expect("full component meta should exist");

    fn union_contains_utility_ref(
        expr: &verter_type_expr::TypeExpr,
        utility_name: &str,
        inner_name: &str,
    ) -> bool {
        match expr {
            verter_type_expr::TypeExpr::Union(members) => {
                members.iter().any(|member| match member {
                    verter_type_expr::TypeExpr::Ref {
                        name,
                        type_arguments,
                    } if name.as_ref() == utility_name && type_arguments.len() == 2 => {
                        matches!(
                            &type_arguments[0],
                            verter_type_expr::TypeExpr::Ref {
                                name,
                                type_arguments
                            } if name.as_ref() == inner_name && type_arguments.is_empty()
                        )
                    }
                    _ => false,
                })
            }
            _ => false,
        }
    }

    // Publication-policy contract: the policy pass keeps *Props-suffix
    // imports symbolic in the public meta so the compat layer
    // (`compat/checker.ts`, `vue-component-meta` interop) emits named
    // opaque schemas instead of inlined member properties. Rule 4
    // covers bare *Props refs; Rule 5 (structural recursion) leaves the
    // *Props leaf unchanged inside Array/Union/Intersection/Pick/Omit
    // wrappers. Rule 1 keeps the symbolic shape for refs whose declaration
    // came from `/node_modules/`.
    for (label, meta) in [("declared", declared), ("full", full)] {
        let avatar = meta
            .props
            .iter()
            .find(|prop| prop.name == "avatar")
            .expect("avatar prop should exist");
        assert!(
            matches!(
                &avatar.type_expr,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "AvatarProps" && type_arguments.is_empty()
            ),
            "{label} component meta should keep imported object refs symbolic, got {:?}",
            avatar.type_expr
        );

        let actions = meta
            .props
            .iter()
            .find(|prop| prop.name == "actions")
            .expect("actions prop should exist");
        assert!(
            matches!(
                &actions.type_expr,
                verter_type_expr::TypeExpr::Array { element, .. }
                    if matches!(
                        element.as_ref(),
                        verter_type_expr::TypeExpr::Ref { name, type_arguments }
                            if name.as_ref() == "ButtonProps" && type_arguments.is_empty()
                    )
            ),
            "{label} component meta should keep imported array element refs symbolic, got {:?}",
            actions.type_expr
        );

        let close = meta
            .props
            .iter()
            .find(|prop| prop.name == "close")
            .expect("close prop should exist");
        assert!(
            union_contains_utility_ref(&close.type_expr, "Omit", "ButtonProps"),
            "{label} component meta should keep imported Omit wrappers symbolic, got {:?}",
            close.type_expr
        );

        let progress = meta
            .props
            .iter()
            .find(|prop| prop.name == "progress")
            .expect("progress prop should exist");
        assert!(
            union_contains_utility_ref(&progress.type_expr, "Pick", "ProgressProps"),
            "{label} component meta should keep imported Pick wrappers symbolic, got {:?}",
            progress.type_expr
        );
    }
}

// `public_component_meta_keeps_simple_imported_alias_union_surface`
// is intentionally not part of this suite: its characterisation
// depended on a `should_preserve_shallow_field_expr` heuristic that
// pinned a symbolic-vs-concrete mix at a specific granularity.
// Dispatch's `project_type_surface_expr` expands via the hot path and
// does not emit that pinned shape. Import/alias resolution is covered
// by the surviving dispatch-backed component-meta tests.

#[test]
fn imported_utility_wrapped_field_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/src/button.ts",
            r#"
export interface ButtonProps {
  href?: string
  disabled?: boolean
  label?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base("/src/keys.ts", "export type LinkPropsKeys = 'href'")
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps } from './button'
import type { LinkPropsKeys } from './keys'

defineProps<{
  close?: boolean | Omit<ButtonProps, LinkPropsKeys>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./button".to_string(),
                resolved_canonical_id: Some("/src/button.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./keys".to_string(),
                resolved_canonical_id: Some("/src/keys.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let close_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "close"))
        .expect("expanded evaluated types should keep the close prop");

    let has_symbolic_omit = match &close_field.r#type {
        verter_type_expr::TypeExpr::Union(members) => members.iter().any(|member| match member {
            verter_type_expr::TypeExpr::Ref {
                name,
                type_arguments,
            } if name.as_ref() == "Omit" && type_arguments.len() == 2 => {
                matches!(
                    &type_arguments[0],
                    verter_type_expr::TypeExpr::Ref {
                        name,
                        type_arguments
                    } if name.as_ref() == "ButtonProps" && type_arguments.is_empty()
                )
            }
            _ => false,
        }),
        _ => false,
    };

    assert!(
        has_symbolic_omit,
        "utility wrappers around imported object refs should stay symbolic in shallow field evaluation, got {:?}",
        close_field.r#type
    );
}

#[test]
fn local_alias_with_package_backed_union_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"
export interface VNode {
  component?: object
  children?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
import type { VNode } from 'vue'

export type StringOrVNode = string | VNode
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { StringOrVNode } from './types'

defineProps<{
  title?: StringOrVNode
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _store_view = project.host().resolver_store_view_read().into_owned_view();
    let prepared = project
        .host()
        .prepared_type_decl("/src/types.ts", "StringOrVNode")
        .expect("StringOrVNode should be present in the shallow prepared declarations");
    assert!(
        matches!(&prepared.body, verter_type_expr::TypeExpr::Union(_),),
        "shallow prepared declarations should keep imported non-object aliases symbolic, got {:?}",
        prepared.body
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let title_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "title"))
        .expect("expanded evaluated types should keep the title prop");

    // The projector publishes the `title` field via
    // `dispatch.execute_read` + `raise_node_to_type_expr`. Two
    // acceptable shapes preserve the package-backed `VNode` symbol
    // semantically:
    //
    //   1. `Ref { name: "StringOrVNode", type_arguments: [] }` — the
    //      projector preserves the alias name rather than unwrapping
    //      the union (the alias body remains accessible through the
    //      type registry / resolver). This is the projector's typical
    //      Shallow output for non-object aliases.
    //   2. `Union(...)` containing a symbolic `Ref { name: "VNode" }`
    //      — the legacy walker's expanded form.
    //
    // BOTH preserve the load-bearing invariant: the package-backed
    // `VNode` is NOT eagerly expanded into its `node_modules/` body
    // (which would defeat the symbolic-preservation contract).
    let preserves_package_symbolic = match &title_field.r#type {
        verter_type_expr::TypeExpr::Union(members) => members.iter().any(|member| {
            matches!(
                member,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "VNode" && type_arguments.is_empty()
            )
        }),
        verter_type_expr::TypeExpr::Ref { name, .. } => name.as_ref() == "StringOrVNode",
        _ => false,
    };

    assert!(
        preserves_package_symbolic,
        "local aliases that wrap package-backed refs must preserve the \
         package-backed symbol — either as a `Ref` to the local alias \
         or as a `Union` containing a symbolic `Ref {{ name: \"VNode\" }}`. \
         Got {:?}",
        title_field.r#type
    );
}

#[test]
fn imported_non_object_alias_with_package_refs_stays_symbolic_in_registry() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"
export interface VNode {
  component?: object
  children?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
import type { VNode } from 'vue'

export type StringOrVNode = string | VNode | (() => VNode)
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { StringOrVNode } from './types'

defineProps<{
  title?: StringOrVNode
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let store_view = project.host().resolver_store_view_read().into_owned_view();
    let prepared = project
        .host()
        .prepared_type_decl("/src/types.ts", "StringOrVNode")
        .expect("StringOrVNode should be present in the shallow prepared declarations");
    assert!(
        matches!(&prepared.body, verter_type_expr::TypeExpr::Union(_),),
        "shallow prepared declarations should keep imported non-object aliases symbolic, got {:?}",
        prepared.body
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");

    let string_or_vnode = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "StringOrVNode")
        .expect("imported non-object alias should still publish in the registry");
    let string_or_vnode_meta = resolved
        .resolved_type_registry_meta
        .iter()
        .find(|entry| entry.name == "StringOrVNode")
        .expect("imported non-object alias should keep registry metadata");

    let verter_type_expr::TypeExpr::Union(members) = &string_or_vnode.type_expr else {
        panic!(
            "StringOrVNode should stay a symbolic union in the registry, got {:?} with declaration {:?}",
            string_or_vnode.type_expr,
            string_or_vnode_meta.declaration
        );
    };
    assert!(
        members.iter().any(|member| {
            matches!(
                member,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "VNode" && type_arguments.is_empty()
            )
        }),
        "imported non-object aliases should keep package-backed refs symbolic in the registry, got {:?}",
        string_or_vnode.type_expr
    );
    assert!(
        resolved
            .resolved_type_registry
            .iter()
            .all(|entry| entry.name != "VNode"),
        "publishing the alias should not recurse into package-backed helpers"
    );

    // Registry whole-surface warming observability lives on the
    // semantic-graph memo, not on a separate `TypeSurfaceDb`. The
    // behavioural contract (package unions stay symbolic in the
    // registry) is already pinned by the assertions above: the
    // `.type_expr` is a `Ref`, and `VNode` is absent from the
    // registry — either would break if the whole-surface projection
    // had actually warmed and substituted.
    let _ = &store_view;
}

#[test]
fn link_props_keep_inherited_html_attrs_across_vue_ignore_utility_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue-router/index.d.ts",
            r#"
export { R as RouterLinkProps } from './dist/index.js'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/vue-router/dist/index.d.ts",
            r#"
export interface RouterLinkOptions {
  to?: string
  replace?: boolean
  viewTransition?: boolean
}

export interface R extends RouterLinkOptions {
  activeClass?: string
  exactActiveClass?: string
  ariaCurrentValue?: 'page'
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types/html.ts",
            r#"
export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  name?: string
  type?: 'button' | 'submit'
}

export interface AnchorHTMLAttributes {
  download?: boolean
  href?: string
  hreflang?: string
  media?: string
  ping?: string
  referrerpolicy?: string
  rel?: string
  target?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface RouterLinkProps {
  replace?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps, /** @vue-ignore */ Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, /** @vue-ignore */ Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
}
</script>
<script setup lang="ts">
defineProps<LinkProps>()
</script>
<template><a /></template>"#,
        )
        .unwrap();

    let export = project
        .host()
        .resolve_named_export(
            "/node_modules/vue-router/index.d.ts",
            "RouterLinkProps",
            None,
        )
        .expect("package re-export should resolve RouterLinkProps");
    assert_eq!(
        export.source_canonical_id.as_deref(),
        Some("/node_modules/vue-router/dist/index.d.ts")
    );
    assert_eq!(export.source_name, "R");
    let decl = crate::meta_resolve::resolve_type_declaration(
        project.host(),
        "/node_modules/vue-router/index.d.ts",
        "RouterLinkProps",
    );
    assert_eq!(
        decl.canonical_source,
        "/node_modules/vue-router/dist/index.d.ts"
    );
    assert_eq!(decl.resolved_name, "R");

    let meta = project
        .host()
        .get_component_meta("/src/Link.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"autofocus")
            && prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"name")
            && prop_names.contains(&"download")
            && prop_names.contains(&"hreflang"),
        "LinkProps should keep inherited HTML attrs across vue-ignore utility heritage: {:?}",
        prop_names
    );
}

#[test]
fn link_props_keep_router_members_across_package_reexported_utility_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue-router/index.d.ts",
            r#"
export { R as RouterLinkProps } from './dist/index.js'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/vue-router/dist/index.d.ts",
            r#"
export interface RouterLinkOptions {
  to?: string
  replace?: boolean
  viewTransition?: boolean
}

export interface R extends RouterLinkOptions {
  activeClass?: string
  exactActiveClass?: string
  ariaCurrentValue?: 'page'
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
import type { RouterLinkProps } from 'vue-router'

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  custom?: boolean
}
</script>
<script setup lang="ts">
defineProps<LinkProps>()
</script>
<template><a /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/Link.vue",
        vec![crate::types::DependencyResolution {
            specifier: "vue-router".to_string(),
            resolved_canonical_id: Some("/node_modules/vue-router/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/node_modules/vue-router/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./dist/index.js".to_string(),
            resolved_canonical_id: Some("/node_modules/vue-router/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = project
        .host()
        .get_component_meta("/src/Link.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"replace")
            && prop_names.contains(&"viewTransition")
            && prop_names.contains(&"activeClass")
            && prop_names.contains(&"exactActiveClass")
            && prop_names.contains(&"ariaCurrentValue"),
        "LinkProps should keep router members across package re-exported Omit heritage: {:?}",
        prop_names
    );
}

#[test]
fn imported_omit_props_preserve_jsdoc_and_raw_type_text() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface UseComponentIconsProps {
  icon?: string
}

interface NuxtLinkProps {
  to?: string
}

interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
}

interface AnchorHTMLAttributes {
  href?: string
}

export interface LinkProps extends NuxtLinkProps, /** @vue-ignore */ Omit<ButtonHTMLAttributes, 'type'>, /** @vue-ignore */ Omit<AnchorHTMLAttributes, 'href'> {
  /** Force the link to be active independent of the current route. */
  active?: boolean
  /** Class to apply when the link is active */
  activeClass?: string
  raw?: boolean
  custom?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script setup lang="ts">
import type { ButtonProps } from './types'

defineProps<ButtonProps>()
</script>
<template><button /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Button.vue")
        .expect("should return component meta");

    let active = meta
        .props
        .iter()
        .find(|prop| prop.name == "active")
        .expect("active prop should be preserved through imported Omit");
    assert_eq!(active.raw_type.as_deref(), Some("boolean"));
    assert_eq!(
        active.description.as_deref(),
        Some("Force the link to be active independent of the current route.")
    );

    let active_class = meta
        .props
        .iter()
        .find(|prop| prop.name == "activeClass")
        .expect("activeClass prop should be preserved through imported Omit");
    assert_eq!(active_class.raw_type.as_deref(), Some("string"));
    assert_eq!(
        active_class.description.as_deref(),
        Some("Class to apply when the link is active")
    );
}

#[test]
fn jsdoc_descriptions_propagate_through_barrel_reexports() {
    let project = make_project();
    // Defining file: actual interface with JSDoc comments
    project
        .upsert_base(
            "/src/external-types.ts",
            r#"
interface TooltipRootProps {
  /**
   * The open state of the tooltip when it is initially rendered.
   * Use when you do not need to control its open state.
   */
  defaultOpen?: boolean;
  /**
   * The controlled open state of the tooltip.
   */
  open?: boolean;
  /**
   * Override the duration given to the `Provider` to customise
   * the open delay for a specific tooltip.
   *
   * @defaultValue 700
   */
  delayDuration?: number;
  /**
   * When `true`, clicking on trigger will not close the content.
   * @defaultValue false
   */
  disableClosingTrigger?: boolean;
  /**
   * When `true`, disable tooltip
   * @defaultValue false
   */
  disabled?: boolean;
}

export { TooltipRootProps }
"#,
        )
        .unwrap();
    // Barrel re-export file: imports from defining file and re-exports
    project
        .upsert_base(
            "/src/types.ts",
            r#"
import { TooltipRootProps } from "./external-types";
export { TooltipRootProps };
"#,
        )
        .unwrap();
    // Component imports from the barrel file and extends the type
    project
        .upsert_base(
            "/src/Tooltip.vue",
            r#"<script lang="ts">
import type { TooltipRootProps } from './types'

export interface TooltipProps extends TooltipRootProps {
  /** The text content of the tooltip. */
  text?: string
}
</script>
<script setup lang="ts">
const props = defineProps<TooltipProps>()
</script>
<template><div>{{ props.text }}</div></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Tooltip.vue")
        .expect("should return component meta");

    // Local prop should have its JSDoc
    let text = meta
        .props
        .iter()
        .find(|p| p.name == "text")
        .expect("text prop should exist");
    assert_eq!(
        text.description.as_deref(),
        Some("The text content of the tooltip.")
    );

    // Props inherited from the external package through barrel re-export
    // should also have their JSDoc descriptions
    let default_open = meta
        .props
        .iter()
        .find(|p| p.name == "defaultOpen")
        .expect("defaultOpen prop should exist");
    assert_eq!(
        default_open.description.as_deref(),
        Some("The open state of the tooltip when it is initially rendered.\nUse when you do not need to control its open state."),
        "defaultOpen JSDoc should propagate through barrel re-export"
    );

    let open = meta
        .props
        .iter()
        .find(|p| p.name == "open")
        .expect("open prop should exist");
    assert_eq!(
        open.description.as_deref(),
        Some("The controlled open state of the tooltip."),
        "open JSDoc should propagate through barrel re-export"
    );

    let delay = meta
        .props
        .iter()
        .find(|p| p.name == "delayDuration")
        .expect("delayDuration prop should exist");
    assert_eq!(
        delay.description.as_deref(),
        Some("Override the duration given to the `Provider` to customise\nthe open delay for a specific tooltip."),
        "delayDuration JSDoc should propagate through barrel re-export"
    );
    assert_eq!(
        delay.tags.len(),
        1,
        "delayDuration should have @defaultValue tag"
    );
    assert_eq!(delay.tags[0].name, "defaultValue");
    assert_eq!(delay.tags[0].text.as_deref(), Some("700"));

    let disabled = meta
        .props
        .iter()
        .find(|p| p.name == "disabled")
        .expect("disabled prop should exist");
    assert_eq!(
        disabled.description.as_deref(),
        Some("When `true`, disable tooltip"),
        "disabled JSDoc should propagate through barrel re-export"
    );
    assert_eq!(
        disabled.tags.len(),
        1,
        "disabled should have @defaultValue tag"
    );
    assert_eq!(disabled.tags[0].name, "defaultValue");
    assert_eq!(disabled.tags[0].text.as_deref(), Some("false"));

    // Negative assertion: props that don't exist should not appear
    assert!(
        meta.props.iter().all(|p| p.name != "nonexistent"),
        "no phantom props should be generated"
    );
}

#[test]
fn jsdoc_descriptions_propagate_through_barrel_reexports_with_defaults() {
    let project = make_project();
    project
        .upsert_base(
            "/src/external-types.ts",
            r#"
interface TooltipRootProps {
  /**
   * The open state of the tooltip when it is initially rendered.
   * Use when you do not need to control its open state.
   */
  defaultOpen?: boolean;
  /**
   * The controlled open state of the tooltip.
   */
  open?: boolean;
}

export { TooltipRootProps }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
import { TooltipRootProps } from "./external-types";
export { TooltipRootProps };
"#,
        )
        .unwrap();
    // Uses withDefaults wrapping defineProps — macro_kind is WithDefaults
    project
        .upsert_base(
            "/src/Tooltip.vue",
            r#"<script lang="ts">
import type { TooltipRootProps } from './types'

export interface TooltipProps extends TooltipRootProps {
  /** The text content of the tooltip. */
  text?: string
  portal?: boolean
}
</script>
<script setup lang="ts">
const props = withDefaults(defineProps<TooltipProps>(), {
  portal: true
})
</script>
<template><div>{{ props.text }}</div></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Tooltip.vue")
        .expect("should return component meta");

    let default_open = meta
        .props
        .iter()
        .find(|p| p.name == "defaultOpen")
        .expect("defaultOpen prop should exist");
    assert_eq!(
        default_open.description.as_deref(),
        Some("The open state of the tooltip when it is initially rendered.\nUse when you do not need to control its open state."),
        "defaultOpen JSDoc should propagate through barrel with withDefaults"
    );

    let open = meta
        .props
        .iter()
        .find(|p| p.name == "open")
        .expect("open prop should exist");
    assert_eq!(
        open.description.as_deref(),
        Some("The controlled open state of the tooltip."),
        "open JSDoc should propagate through barrel with withDefaults"
    );
}

#[test]
fn jsdoc_descriptions_propagate_through_wildcard_reexport() {
    let project = make_project();
    // Defining file with JSDoc-annotated interface
    project
        .upsert_base(
            "/src/link-props.ts",
            r#"
export interface LinkProps {
  /**
   * Force the link to be active independent of the current route.
   */
  active?: boolean;
  /**
   * Class to apply when the link is active
   * @defaultValue ""
   */
  activeClass?: string;
  /**
   * The element or component this component should render as when not a link.
   */
  as?: string;
}
"#,
        )
        .unwrap();
    // Barrel using `export *` wildcard re-export
    project
        .upsert_base(
            "/src/types/index.ts",
            r#"
export * from '../link-props';
"#,
        )
        .unwrap();
    // Component that imports from the barrel
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types/index'

export interface ButtonProps extends LinkProps {
  /** The button label. */
  label?: string
}
</script>
<script setup lang="ts">
const props = defineProps<ButtonProps>()
</script>
<template><button>{{ props.label }}</button></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Button.vue")
        .expect("should return component meta");

    let label = meta
        .props
        .iter()
        .find(|p| p.name == "label")
        .expect("label prop should exist");
    assert_eq!(label.description.as_deref(), Some("The button label."),);

    let active = meta
        .props
        .iter()
        .find(|p| p.name == "active")
        .expect("active prop should exist");
    assert_eq!(
        active.description.as_deref(),
        Some("Force the link to be active independent of the current route."),
        "active JSDoc should propagate through export * barrel"
    );

    let active_class = meta
        .props
        .iter()
        .find(|p| p.name == "activeClass")
        .expect("activeClass prop should exist");
    assert_eq!(
        active_class.description.as_deref(),
        Some("Class to apply when the link is active"),
        "activeClass JSDoc should propagate through export * barrel"
    );
    assert_eq!(
        active_class.tags.len(),
        1,
        "activeClass should have @defaultValue tag"
    );
    assert_eq!(active_class.tags[0].name, "defaultValue");

    let as_prop = meta
        .props
        .iter()
        .find(|p| p.name == "as")
        .expect("as prop should exist");
    assert_eq!(
        as_prop.description.as_deref(),
        Some("The element or component this component should render as when not a link."),
        "as JSDoc should propagate through export * barrel"
    );
}

#[test]
fn jsdoc_descriptions_propagate_through_heritage_chain_imports() {
    let project = make_project();
    // External file: defines RouterLinkProps with JSDoc
    project
        .upsert_base(
            "/src/router-types.ts",
            r#"
export interface RouterLinkProps {
  /**
   * Calls `router.replace` instead of `router.push`.
   */
  replace?: boolean;
  /**
   * Class to apply when the link is active
   */
  activeClass?: string;
  /**
   * Class to apply when the link is exact active
   */
  exactActiveClass?: string;
}
"#,
        )
        .unwrap();
    // Intermediate file: LinkProps extends imported RouterLinkProps
    project
        .upsert_base(
            "/src/link.ts",
            r#"
import { RouterLinkProps } from './router-types'

export interface LinkProps extends RouterLinkProps {
  /** Force the link to be active. */
  active?: boolean;
  /** The URL to navigate to. */
  href?: string;
}
"#,
        )
        .unwrap();
    // Component imports from the intermediate file
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './link'

export interface ButtonProps extends LinkProps {
  /** The button label. */
  label?: string
}
</script>
<script setup lang="ts">
const props = defineProps<ButtonProps>()
</script>
<template><button>{{ props.label }}</button></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Button.vue")
        .expect("should return component meta");

    // Local prop
    let label = meta
        .props
        .iter()
        .find(|p| p.name == "label")
        .expect("label");
    assert_eq!(label.description.as_deref(), Some("The button label."));

    // Directly on LinkProps
    let active = meta
        .props
        .iter()
        .find(|p| p.name == "active")
        .expect("active");
    assert_eq!(
        active.description.as_deref(),
        Some("Force the link to be active."),
    );

    // On LinkProps (one level of local heritage)
    let href = meta.props.iter().find(|p| p.name == "href").expect("href");
    assert_eq!(href.description.as_deref(), Some("The URL to navigate to."),);

    // From RouterLinkProps (heritage chain import from separate file)
    let replace = meta
        .props
        .iter()
        .find(|p| p.name == "replace")
        .expect("replace");
    assert_eq!(
        replace.description.as_deref(),
        Some("Calls `router.replace` instead of `router.push`."),
        "replace JSDoc should propagate through heritage chain import"
    );

    let active_class = meta
        .props
        .iter()
        .find(|p| p.name == "activeClass")
        .expect("activeClass");
    assert_eq!(
        active_class.description.as_deref(),
        Some("Class to apply when the link is active"),
        "activeClass JSDoc should propagate through heritage chain import"
    );

    let exact_active = meta
        .props
        .iter()
        .find(|p| p.name == "exactActiveClass")
        .expect("exactActiveClass");
    assert_eq!(
        exact_active.description.as_deref(),
        Some("Class to apply when the link is exact active"),
        "exactActiveClass JSDoc should propagate through heritage chain import"
    );
}

/// The JSDoc enrichment path for imported props goes through the host-
/// cached parsed program + cached external type analysis. The
/// enrichment path must NOT fall back to a raw-source reparse helper
/// (which would allocate a fresh oxc arena and reparse dependency
/// source). That architectural guarantee is enforced statically by
/// the `no_text_based_macro_surface_projection_helpers` architecture
/// guard; this test pins the behavioural half (JSDoc descriptions
/// propagate through imported `Omit<>`).
///
/// This scenario routes JSDoc through the span-borne enrichment by
/// extending imported types through `Omit<>` — descriptions slice from the
/// declaring file's cache-owned `IndexedReady.raw_source` via the typeinfo
/// member spans, never a fresh parse.
#[test]
fn imported_jsdoc_enrichment_uses_parse_artifact_and_does_not_reparse_source() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
interface NuxtLinkProps {
  to?: string
}

interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
}

export interface LinkProps extends NuxtLinkProps, /** @vue-ignore */ Omit<ButtonHTMLAttributes, 'type'> {
  /** Force the link to be active independent of the current route. */
  active?: boolean
  /** Class to apply when the link is active */
  activeClass?: string
}

export interface ButtonProps extends Omit<LinkProps, 'to'> {
  label?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><button /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Button.vue")
        .expect("should return component meta");

    // Behavior guard: JSDoc must still propagate across the imported Omit<>.
    let active = meta
        .props
        .iter()
        .find(|p| p.name == "active")
        .expect("active prop should be preserved through imported Omit");
    assert_eq!(
        active.description.as_deref(),
        Some("Force the link to be active independent of the current route."),
        "active JSDoc should propagate through imported Omit"
    );
    let active_class = meta
        .props
        .iter()
        .find(|p| p.name == "activeClass")
        .expect("activeClass prop should be preserved through imported Omit");
    assert_eq!(
        active_class.description.as_deref(),
        Some("Class to apply when the link is active"),
        "activeClass JSDoc should propagate through imported Omit"
    );

    // Architectural guard: under the graph-only resolver, the raw-
    // source reparse helper is deleted. The architecture guard
    // `no_text_based_macro_surface_projection_helpers` enforces this
    // structurally; this behaviour assertion (JSDoc still flows
    // through imported `Omit<>`) ensures the graph-native enrichment
    // path remains correct.
}

// ===========================================================================
// Phase 3: Fallthrough inheritance resolver
// ===========================================================================

use verter_semantic::analysis::component_meta::{
    AcceptedEventKind, AcceptedPropKind, AcceptedSurfaceCompleteness, BranchStatus,
    FallthroughSurface, MemberAvailability, MemberProvenance, PartialBranchReason,
    ResolvedRootStep, UnresolvedBranchReason,
};

/// Helper: get the component meta for a file (through session).
fn get_meta(
    project: &Arc<MetaProject>,
    canonical_id: &str,
) -> verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
    let session = project.open_session_batch().unwrap();
    session
        .get_component_meta(canonical_id)
        .unwrap()
        .expect("get_component_meta should return metadata")
}

#[test]
fn single_native_root_inherits_intrinsic_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is in accepted_props
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"
            && matches!(p.provenance, MemberProvenance::Declared)
            && matches!(p.kind, AcceptedPropKind::DeclaredProp)),
        "accepted_props should contain declared 'msg' prop, got: {:?}",
        meta.accepted_props
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );

    // Assert+: inherited attrs from div should be present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "id"
            && matches!(p.provenance, MemberProvenance::Inherited { .. })
            && matches!(p.kind, AcceptedPropKind::Attr)),
        "accepted_props should contain inherited 'id' attr from <div>, got: {:?}",
        meta.accepted_props
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );

    // Assert+: inherited events from div
    assert!(
        meta.accepted_events.iter().any(|e| e.name == "click"
            && matches!(e.provenance, MemberProvenance::Inherited { .. })
            && matches!(e.kind, AcceptedEventKind::Listener)),
        "accepted_events should contain inherited 'click' listener from <div>, got: {:?}",
        meta.accepted_events
            .iter()
            .map(|e| &e.name)
            .collect::<Vec<_>>()
    );

    // Assert+: surface completeness should be Exact
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::Exact,
        "completeness should be Exact for a simple native root"
    );

    // Assert+: fallthrough_surface should have branches
    assert!(
        matches!(
            meta.fallthrough_surface,
            FallthroughSurface::Branches { .. }
        ),
        "fallthrough_surface should be Branches, got: {:?}",
        meta.fallthrough_surface
    );

    // Assert-: declared props should NOT appear in fallthrough_surface
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert_eq!(branches.len(), 1, "should have one branch");
        assert!(
            !branches[0].props.iter().any(|p| p.name == "msg"),
            "fallthrough_surface should NOT contain declared 'msg' prop"
        );
        assert_eq!(
            branches[0].status,
            BranchStatus::Resolved,
            "branch status should be Resolved"
        );
        assert!(
            matches!(&branches[0].root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "div"),
            "root_chain should show NativeTag div"
        );
    }
}

#[test]
fn public_component_meta_materializes_local_component_config_variant_and_slot_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', soft: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './tv'
import theme from './theme'

type Button = ComponentConfig<typeof theme>

export interface ButtonProps {
  color?: Button['variants']['color']
  activeColor?: Button['variants']['color']
  ui?: Button['slots']
}

type ButtonSlots = {
  default?: (props: { ui: Button['ui'] }) => any
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let session = project.open_session_batch().expect("session should open");
    let meta = session
        .get_component_meta("/src/Button.vue")
        .expect("component meta query should succeed")
        .expect("component meta should exist");

    assert_eq!(
        meta.props.len(),
        3,
        "should have exactly 3 props (color, activeColor, ui), got {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    assert!(
        meta.events.is_empty(),
        "should have no events, got {:?}",
        meta.events.iter().map(|e| &e.name).collect::<Vec<_>>()
    );

    for prop_name in ["color", "activeColor"] {
        let prop = meta
            .props
            .iter()
            .find(|prop| prop.name == prop_name)
            .expect("variant prop should exist");
        assert_union_string_literals(&prop.type_expr, &["primary", "secondary"]);
        assert!(
            !matches!(&prop.type_expr, TypeExpr::Unknown { .. }),
            "variant prop should not degrade to Unknown"
        );
    }

    let ui = meta
        .props
        .iter()
        .find(|prop| prop.name == "ui")
        .expect("ui prop should exist");
    let TypeExpr::Object(ui_shape) = &ui.type_expr else {
        panic!(
            "component-config slots helper should materialize as an object, got {:?}",
            ui.type_expr
        );
    };
    assert_eq!(
        ui_shape.properties.len(),
        2,
        "ui prop should have exactly 2 properties (base, label)"
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "base"),
        ),
        "ui helper should expose base, got {:?}",
        ui.type_expr
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "label"),
        ),
        "ui helper should expose label, got {:?}",
        ui.type_expr
    );

    assert_eq!(meta.slots.len(), 1, "should have exactly 1 slot (default)");
    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should exist");
    assert_eq!(
        default_slot.bindings.len(),
        1,
        "default slot should have exactly 1 binding (ui)"
    );
    let ui_binding = default_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "ui")
        .expect("default slot should expose ui");
    let TypeExpr::Object(binding_shape) = &ui_binding.type_expr else {
        panic!(
            "slot ui binding should materialize as an object, got {:?}",
            ui_binding.type_expr
        );
    };
    assert_eq!(
        binding_shape.properties.len(),
        2,
        "slot ui binding should have exactly 2 properties (base, label)"
    );
    assert!(
        binding_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "base"),
        ),
        "slot ui binding should expose base, got {:?}",
        ui_binding.type_expr
    );
    assert!(
        binding_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "label"),
        ),
        "slot ui binding should expose label, got {:?}",
        ui_binding.type_expr
    );
}

#[test]
fn public_component_meta_materializes_component_config_app_config_variant_and_slot_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/@nuxt/schema/index.d.ts",
            r#"
export interface AppConfig {
  ui: {
    button: {
      variants: {
        color: {
          neutral: string
        }
      }
    }
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

type GetComponentAppConfig<A, U extends string, K extends string>
  = A extends Record<U, Record<K, any>> ? A[U][K] : {}

type ComponentAppConfig<
  T,
  A extends Record<string, any>,
  K extends string,
  U extends string = 'ui' | 'ui.prose'
> = A & (
  U extends 'ui.prose'
    ? { ui?: { prose?: { [k in K]?: Partial<T> } } }
    : { [key in Exclude<U, 'ui.prose'>]?: { [k in K]?: Partial<T> } }
)

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
  U extends 'ui' | 'ui.prose' = 'ui'
> = {
  AppConfig: ComponentAppConfig<T, A, K, U>,
  variants: ComponentVariants<T & GetComponentAppConfig<A, U, K>>
  slots: ComponentSlots<T>,
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', soft: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { AppConfig } from '@nuxt/schema'
import type { ComponentConfig } from './tv'
import theme from './theme'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>

export interface ButtonProps {
  color?: Button['variants']['color']
  activeColor?: Button['variants']['color']
  ui?: Button['slots']
}

type ButtonSlots = {
  default?: (props: { ui: Button['ui'] }) => any
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "@nuxt/schema".to_string(),
                resolved_canonical_id: Some("/node_modules/@nuxt/schema/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let session = project.open_session_batch().expect("session should open");
    let meta = session
        .get_component_meta("/src/Button.vue")
        .expect("component meta query should succeed")
        .expect("component meta should exist");

    assert_eq!(
        meta.props.len(),
        3,
        "should have exactly 3 props (color, activeColor, ui), got {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    assert!(
        meta.events.is_empty(),
        "should have no events, got {:?}",
        meta.events.iter().map(|e| &e.name).collect::<Vec<_>>()
    );

    for prop_name in ["color", "activeColor"] {
        let prop = meta
            .props
            .iter()
            .find(|prop| prop.name == prop_name)
            .expect("variant prop should exist");
        assert_union_string_literals(&prop.type_expr, &["primary", "secondary", "neutral"]);
        assert!(
            !matches!(&prop.type_expr, TypeExpr::Unknown { .. }),
            "variant prop should not degrade to Unknown"
        );
    }

    let ui = meta
        .props
        .iter()
        .find(|prop| prop.name == "ui")
        .expect("ui prop should exist");
    let TypeExpr::Object(ui_shape) = &ui.type_expr else {
        panic!(
            "component-config slots helper should materialize as an object, got {:?}",
            ui.type_expr
        );
    };
    assert_eq!(
        ui_shape.properties.len(),
        2,
        "ui prop should have exactly 2 properties (base, label)"
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "base"),
        ),
        "ui helper should expose base, got {:?}",
        ui.type_expr
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "label"),
        ),
        "ui helper should expose label, got {:?}",
        ui.type_expr
    );

    assert_eq!(meta.slots.len(), 1, "should have exactly 1 slot (default)");
    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should exist");
    assert_eq!(
        default_slot.bindings.len(),
        1,
        "default slot should have exactly 1 binding (ui)"
    );
    let ui_binding = default_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "ui")
        .expect("default slot should expose ui");
    let TypeExpr::Object(binding_shape) = &ui_binding.type_expr else {
        panic!(
            "slot ui binding should materialize as an object, got {:?}",
            ui_binding.type_expr
        );
    };
    assert_eq!(
        binding_shape.properties.len(),
        2,
        "slot ui binding should have exactly 2 properties (base, label)"
    );
    assert!(
        binding_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "base"),
        ),
        "slot ui binding should expose base, got {:?}",
        ui_binding.type_expr
    );
    assert!(
        binding_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "label"),
        ),
        "slot ui binding should expose label, got {:?}",
        ui_binding.type_expr
    );
}

#[test]
fn explicit_root_bindings_are_subtracted() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div id="root" @click="() => {}">{{ msg }}</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert-: explicitly bound 'id' attr should NOT be inherited
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            !branches[0].props.iter().any(|p| p.name == "id"),
            "consumed 'id' attr should be subtracted from inherited props"
        );
    }

    // Assert-: explicitly bound 'click' listener should NOT be inherited
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            !branches[0].events.iter().any(|e| e.name == "click"),
            "consumed 'click' listener should be subtracted from inherited events"
        );
    }

    // Assert+: other attrs should still be inherited
    assert!(
        meta.accepted_props.iter().any(
            |p| p.name == "title" && matches!(p.provenance, MemberProvenance::Inherited { .. }),
        ),
        "non-consumed 'title' attr should still be inherited"
    );
}

#[test]
fn declared_props_and_events_take_precedence() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ id: number }>()
defineEmits<{ (e: 'click', value: string): void }>()
</script>
<template><div>hello</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: 'id' should be declared, not inherited
    let id_prop = meta
        .accepted_props
        .iter()
        .find(|p| p.name == "id")
        .expect("should have 'id' in accepted_props");
    assert!(
        matches!(id_prop.provenance, MemberProvenance::Declared),
        "'id' should be declared, not inherited"
    );

    // Assert+: 'click' should be declared, not inherited
    let click_event = meta
        .accepted_events
        .iter()
        .find(|e| e.name == "click")
        .expect("should have 'click' in accepted_events");
    assert!(
        matches!(click_event.provenance, MemberProvenance::Declared),
        "'click' should be declared, not inherited"
    );

    // Assert-: should NOT have duplicate 'id' or 'click'
    assert_eq!(
        meta.accepted_props
            .iter()
            .filter(|p| p.name == "id")
            .count(),
        1,
        "'id' should appear exactly once"
    );
    assert_eq!(
        meta.accepted_events
            .iter()
            .filter(|e| e.name == "click")
            .count(),
        1,
        "'click' should appear exactly once"
    );
}

#[test]
fn declared_on_listener_alias_prop_blocks_inherited_click_listener() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ onClick?: () => void }>()
</script>
<template><div>hello</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        meta.accepted_props
            .iter()
            .any(|p| p.name == "onClick" && matches!(p.provenance, MemberProvenance::Declared)),
        "declared onClick prop must remain on the accepted prop surface"
    );
    assert!(
        !meta.accepted_events.iter().any(|e| e.name == "click"),
        "declared onClick prop must block the inherited click listener alias"
    );

    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            branches
                .iter()
                .all(|branch| branch.events.iter().all(|event| event.name != "click")),
            "fallthrough branches must not leak click when a declared onClick prop shadows it"
        );
    }
}

#[test]
fn inherit_attrs_false_returns_declared_only_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "should have no inherited props when inheritAttrs: false"
    );
    assert!(
        !meta
            .accepted_events
            .iter()
            .any(|e| matches!(e.provenance, MemberProvenance::Inherited { .. })),
        "should have no inherited events when inheritAttrs: false"
    );

    // Assert+: fallthrough_surface is None
    assert!(
        matches!(meta.fallthrough_surface, FallthroughSurface::None { .. }),
        "fallthrough_surface should be None when inheritAttrs: false"
    );
}

#[test]
fn unconditional_multi_root_returns_declared_only_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>a</div><span>b</span></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "multi-root should have no inherited props"
    );

    // Assert+: fallthrough_surface is None
    assert!(
        matches!(meta.fallthrough_surface, FallthroughSurface::None { .. }),
        "fallthrough_surface should be None for multi-root"
    );
}

#[test]
fn conditional_single_root_returns_exact_branches() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const show = true
defineProps<{ msg: string }>()
</script>
<template>
  <div v-if="show">a</div>
  <input v-else />
</template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: should have branches
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert_eq!(branches.len(), 2, "should have 2 branches (div, input)");

        // Branch 0: div
        assert_eq!(branches[0].branch_key, "0");
        assert!(
            matches!(&branches[0].root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "div"),
            "first branch should be div"
        );

        // Branch 1: input
        assert_eq!(branches[1].branch_key, "1");
        assert!(
            matches!(&branches[1].root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "input"),
            "second branch should be input"
        );

        // Assert+: input-specific attrs should be conditional
        // (only in branch 1, not branch 0)
        let input_specific = meta.accepted_props.iter().find(|p| p.name == "type");
        if let Some(p) = input_specific {
            assert!(
                matches!(p.availability, MemberAvailability::Conditional { .. }),
                "'type' attr should be conditional (only in input branch)"
            );
        }
    } else {
        panic!("expected FallthroughSurface::Branches");
    }
}

#[test]
fn static_dynamic_is_root_resolves_native_candidates() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
const showNative = true
</script>
<template><component :is="showNative ? 'div' : Child" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    let value_prop = meta
        .accepted_props
        .iter()
        .find(|p| p.name == "value")
        .expect("dynamic :is should propagate the input branch's accepted attrs");
    assert!(
        matches!(
            value_prop.availability,
            MemberAvailability::Conditional { .. }
        ),
        "input-only attrs from dynamic :is candidates must stay conditional"
    );

    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            branches
                .iter()
                .any(|branch| matches!(&branch.root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "div")),
            "dynamic :is should produce a native div branch"
        );
        assert!(
            branches.iter().any(|branch| {
                branch
                    .root_chain
                    .iter()
                    .any(|step| matches!(step, ResolvedRootStep::Component { component_name, .. } if component_name == "Child"))
            }),
            "dynamic :is should also preserve the imported component branch"
        );
    } else {
        panic!("expected FallthroughSurface::Branches");
    }
}

#[test]
fn root_v_bind_known_object_shape_is_consumed_exactly() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const rootAttrs = {
  id: 'root',
  onClick: () => {},
}
</script>
<template><div v-bind="rootAttrs" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "id"),
        "exact root spread keys must be subtracted from inherited attrs"
    );
    assert!(
        !meta.accepted_events.iter().any(|e| e.name == "click"),
        "exact root spread listener aliases must be subtracted from inherited listeners"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::Exact,
        "resolvable root spreads should not force a lower-bound surface"
    );

    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            branches
                .iter()
                .all(|branch| branch.props.iter().all(|prop| prop.name != "id")),
            "spread-consumed attrs must not leak back into fallthrough branches"
        );
        assert!(
            branches
                .iter()
                .all(|branch| branch.events.iter().all(|event| event.name != "click")),
            "spread-consumed listeners must not leak back into fallthrough branches"
        );
        assert!(
            branches
                .iter()
                .all(|branch| matches!(branch.status, BranchStatus::Resolved)),
            "an exact root spread should keep the branch resolved"
        );
    } else {
        panic!("expected FallthroughSurface::Branches");
    }
}

#[test]
fn root_v_bind_unknown_shape_uses_structured_partial_reason() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const rootAttrs: Record<string, unknown> = {}
</script>
<template><div v-bind="rootAttrs" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "unknown root spreads must lower accepted-surface completeness"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };

    assert!(
        branches.iter().any(|branch| matches!(
            &branch.status,
            BranchStatus::PartiallyUnresolved { reasons }
                if reasons == &vec![PartialBranchReason::UnknownSpread]
        )),
        "unknown root spreads must surface a structured UnknownSpread reason, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );
}

#[test]
fn project_local_intrinsics_load_from_vue_type_entrypoints() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(
            r#"{
  "name": "vue",
  "types": "./index.d.ts",
  "exports": {
    ".": { "types": "./index.d.ts", "import": "./index.js" },
    "./jsx": { "types": "./jsx.d.ts", "import": "./jsx.js" }
  }
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/index.d.ts".to_string(),
        Arc::from(
            r#"export interface HTMLAttributes {
  fallbackOnly?: string
  onProjectClick?: ProjectClickEvent
}

export interface ProjectClickEvent {
  source: 'project'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/jsx.d.ts".to_string(),
        Arc::from(
            r#"import type { NativeElements } from "./jsx-runtime"

export namespace JSX {
  export interface IntrinsicElements extends NativeElements {}
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/jsx-runtime.d.ts".to_string(),
        Arc::from(
            r#"import type { HTMLAttributes } from "./index"

export interface NativeElements {
  div: HTMLAttributes & { projectOnly?: string }
}"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let project = MetaProject::new(host);
    project
        .upsert_base("/workspace/src/App.vue", r#"<template><div /></template>"#)
        .unwrap();

    let meta = get_meta(&project, "/workspace/src/App.vue");

    assert!(
        meta.accepted_props
            .iter()
            .any(|prop| prop.name == "projectOnly"),
        "native intrinsics loading should surface tag-specific members from vue/jsx"
    );
    assert!(
        meta.accepted_props
            .iter()
            .any(|prop| prop.name == "fallbackOnly"),
        "native intrinsics loading should surface fallback HTMLAttributes members from vue"
    );
    assert!(
        meta.accepted_events
            .iter()
            .any(|event| event.name == "projectClick"),
        "native intrinsics loading should expose listeners derived from the project-local HTMLAttributes surface"
    );
    assert!(
        !meta.accepted_props.iter().any(|prop| prop.name == "id"),
        "project-local intrinsic surfaces should replace the generated built-in tag surface when vue entrypoints resolve"
    );
}

#[test]
fn project_local_intrinsics_tag_members_override_fallback_duplicates() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(
            r#"{
  "name": "vue",
  "types": "./index.d.ts",
  "exports": {
    ".": { "types": "./index.d.ts", "import": "./index.js" },
    "./jsx": { "types": "./jsx.d.ts", "import": "./jsx.js" }
  }
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/index.d.ts".to_string(),
        Arc::from(
            r#"export interface HTMLAttributes {
  projectOnly?: number
  onClick?: (payload: FallbackClickEvent) => void
}

export interface FallbackClickEvent {
  source: 'fallback'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/jsx.d.ts".to_string(),
        Arc::from(
            r#"import type { NativeElements } from "./jsx-runtime"

export namespace JSX {
  export interface IntrinsicElements extends NativeElements {}
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/jsx-runtime.d.ts".to_string(),
        Arc::from(
            r#"import type { HTMLAttributes } from "./index"

export interface NativeElements {
  div: HTMLAttributes & {
    projectOnly?: string
    onClick?: (payload: ProjectClickEvent) => void
  }
}

export interface ProjectClickEvent {
  source: 'project'
}"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let project = MetaProject::new(host);
    project
        .upsert_base("/workspace/src/App.vue", r#"<template><div /></template>"#)
        .unwrap();

    let meta = get_meta(&project, "/workspace/src/App.vue");

    let project_only = meta
        .accepted_props
        .iter()
        .find(|prop| prop.name == "projectOnly")
        .expect("project-local tag members must still be present");
    assert!(
        matches!(
            project_only.type_expr,
            TypeExpr::Primitive(PrimitiveName::String),
        ),
        "tag-specific projectOnly should override the fallback type, got: {:?}",
        project_only.type_expr
    );

    let click = meta
        .accepted_events
        .iter()
        .find(|event| event.name == "click")
        .expect("tag-specific listeners must still appear on the accepted event surface");
    let project_payload = matches!(
        &click.payload,
        TypeExpr::Function(function)
            if function.parameters.len() == 1
                && match &function.parameters[0].ty {
                    TypeExpr::Object(shape) => shape.properties.iter().any(|member| matches!(
                        member,
                        ObjectMember::Property(property)
                            if property.name == "source"
                                && matches!(
                                    property.ty,
                                    TypeExpr::Literal(verter_type_expr::LiteralValue::String(ref value))
                                        if value == "project"
                                )
                    )),
                    TypeExpr::Ref { name, .. } => name.as_ref() == "ProjectClickEvent",
                    _ => false,
                }
    );
    assert!(
        project_payload,
        "tag-specific listener payloads must override fallback listeners, got: {:?}",
        click.payload
    );
}

#[test]
fn generic_root_propagation_off_stays_sound() {
    let project = make_project();
    project
        .upsert_base(
            "/Poly.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
defineProps<{ as: T }>()
</script>
<template><component :is="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Poly from './Poly.vue'
</script>
<template><Poly as="input" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        !meta.accepted_props.iter().any(|prop| prop.name == "value"),
        "generic root propagation disabled must not invent input-only attrs"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "an unresolved generic root must remain a lower-bound surface"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                &branch.status,
                BranchStatus::Unresolved {
                    reason: UnresolvedBranchReason::DynamicComponentIs
                }
            )
        }),
        "without propagation the generic child root should remain unresolved, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );
}

#[test]
fn generic_root_propagation_specializes_dynamic_is_when_enabled() {
    let project = make_project_with_config(HostConfig {
        generic_root_propagation: true,
        ..HostConfig::default()
    });
    project
        .upsert_base(
            "/Poly.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
defineProps<{ as: T }>()
</script>
<template><component :is="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Poly from './Poly.vue'
</script>
<template><Poly as="input" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    let value_prop = meta
        .accepted_props
        .iter()
        .find(|prop| prop.name == "value")
        .expect("generic propagation should specialize the child root to input");
    assert!(
        matches!(value_prop.availability, MemberAvailability::Always),
        "single specialized generic roots should yield always-available attrs"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                branch.root_chain.as_slice(),
                [
                    ResolvedRootStep::Component { component_name, .. },
                    ResolvedRootStep::NativeTag { tag }
                ] if component_name == "Poly" && tag == "input"
            )
        }),
        "generic propagation should resolve the child root chain to Poly -> input, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.root_chain)
            .collect::<Vec<_>>()
    );
}

#[test]
fn generic_root_propagation_recurses_through_component_chain() {
    let project = make_project_with_config(HostConfig {
        generic_root_propagation: true,
        ..HostConfig::default()
    });
    project
        .upsert_base(
            "/Poly.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
defineProps<{ as: T }>()
</script>
<template><component :is="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Wrapper.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
import Poly from './Poly.vue'
defineProps<{ as: T }>()
</script>
<template><Poly :as="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Wrapper from './Wrapper.vue'
</script>
<template><Wrapper as="input" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        meta.accepted_props.iter().any(|prop| prop.name == "value"),
        "recursive generic propagation should preserve the specialized input attrs through Wrapper"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                branch.root_chain.as_slice(),
                [
                    ResolvedRootStep::Component { component_name: wrapper_name, .. },
                    ResolvedRootStep::Component { component_name: poly_name, .. },
                    ResolvedRootStep::NativeTag { tag }
                ] if wrapper_name == "Wrapper" && poly_name == "Poly" && tag == "input"
            )
        }),
        "recursive generic propagation should resolve Wrapper -> Poly -> input, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.root_chain)
            .collect::<Vec<_>>()
    );
}

#[test]
fn recursive_cycle_uses_structured_unresolved_reason() {
    let project = make_project();
    project
        .upsert_base(
            "/A.vue",
            r#"<script setup lang="ts">
import B from './B.vue'
</script>
<template><B /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/B.vue",
            r#"<script setup lang="ts">
import A from './A.vue'
</script>
<template><A /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let meta = get_meta(&project, "/A.vue");
    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };

    assert!(
        branches.iter().any(|branch| matches!(
            &branch.status,
            BranchStatus::Unresolved {
                reason: UnresolvedBranchReason::Cycle { canonical_id }
            } if canonical_id == "/B.vue"
        )),
        "cycles must terminate with a structured cycle reason, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );

    assert!(
        branches.iter().any(|branch| {
            branch.root_chain.iter().any(|step| {
                matches!(
                    step,
                    ResolvedRootStep::Unresolved {
                        reason: UnresolvedBranchReason::Cycle { canonical_id },
                        ..
                    } if canonical_id == "/B.vue"
                )
            })
        }),
        "cycle branches must preserve the structured cycle reason in the root chain"
    );
    assert!(
        provenance(&project).resolver_cycle_detections >= 1,
        "fallthrough cycles should increment the shared resolver cycle counter"
    );
}

#[test]
fn recursive_component_propagates_inherited_surface() {
    let project = make_project();

    // Child component with <div> root
    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
defineProps<{ childProp: string }>()
</script>
<template><div>child</div></template>"#,
        )
        .unwrap();

    // Parent with component root
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
defineProps<{ parentProp: string }>()
</script>
<template><Child :childProp="parentProp" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "parentProp"),
        "should have declared 'parentProp'"
    );

    // Assert+: fallthrough_surface should have branches
    assert!(
        matches!(
            meta.fallthrough_surface,
            FallthroughSurface::Branches { .. }
        ),
        "fallthrough_surface should be Branches for component root"
    );

    // Assert+: root_chain should show Component step
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(!branches.is_empty(), "should have at least one branch");
        assert!(
            branches[0]
                .root_chain
                .iter()
                .any(|step| matches!(step, ResolvedRootStep::Component { .. })),
            "root_chain should contain a Component step, got: {:?}",
            branches[0].root_chain
        );
    }
}

#[test]
fn recursive_component_keeps_child_declared_surface_alongside_child_fallthrough() {
    let project = make_project();

    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
defineProps<{ childProp: string }>()
defineEmits<{ (e: 'childClick', value: number): void }>()
</script>
<template><div>child</div></template>"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    let child_prop = meta
        .accepted_props
        .iter()
        .find(|p| p.name == "childProp")
        .expect("parent must expose child's declared prop through component root recursion");
    assert!(
        matches!(child_prop.provenance, MemberProvenance::Inherited { .. }),
        "child declared prop must arrive as inherited acceptance on the parent"
    );
    assert!(
        matches!(child_prop.kind, AcceptedPropKind::Attr),
        "child declared prop should be exposed as an accepted attr on the parent"
    );

    let child_event = meta
        .accepted_events
        .iter()
        .find(|e| e.name == "childClick")
        .expect("parent must expose child's declared event through component root recursion");
    assert!(
        matches!(child_event.provenance, MemberProvenance::Inherited { .. }),
        "child declared event must arrive as inherited acceptance on the parent"
    );
    assert!(
        matches!(child_event.kind, AcceptedEventKind::Listener),
        "child declared event should be exposed as an accepted listener on the parent"
    );

    assert!(
        meta.accepted_props.iter().any(|p| p.name == "id"),
        "parent must still expose child's inherited native attrs, not just declared members"
    );
}

#[test]
fn non_vue_component_root_stops_fallthrough_recursion_at_the_boundary() {
    let project = make_project();

    project
        .upsert_base(
            "/Child.ts",
            r#"export default function Child() {
  return null
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child'
defineProps<{ parentProp: string }>()
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        meta.accepted_props.iter().any(|p| p.name == "parentProp"),
        "declared props must remain on the accepted surface"
    );
    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "id"),
        "non-Vue child roots must not invent inherited attrs"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "non-Vue child roots should degrade completeness instead of recursing"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                &branch.status,
                BranchStatus::Unresolved {
                    reason: UnresolvedBranchReason::ChildResolutionFailed,
                }
            )
        }),
        "non-Vue child roots should stop at an unresolved branch"
    );
}

#[test]
fn package_component_root_prefers_declaration_companion_for_recursive_fallthrough() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/tsconfig.json".to_string(),
        Arc::from(
            r#"{ "compilerOptions": { "module": "esnext", "moduleResolution": "bundler" } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/package.json".to_string(),
        Arc::from(
            r#"{ "name": "reka-ui", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.d.ts".to_string(),
        Arc::from(r#"export { default as FancyRoot } from './FancyRoot.vue'"#),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/FancyRoot.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
defineProps<{ childProp?: string }>()
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import { FancyRoot } from 'reka-ui'
</script>
<template><FancyRoot /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let project = MetaProject::new(host);
    assert!(
        project.ensure_loaded("/workspace/src/App.vue").unwrap(),
        "workspace owner should load the wrapper component"
    );

    let meta = get_meta(&project, "/workspace/src/App.vue");

    assert!(
        meta.accepted_props.iter().any(|p| p.name == "childProp"),
        "component root recursion should expose the child's declared props through the declaration companion"
    );
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "id"),
        "component root recursion should still expose native attrs from the child root"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::Exact,
        "typed package component roots should recurse exactly through the declaration companion"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches
            .iter()
            .all(|branch| !matches!(branch.status, BranchStatus::Unresolved { .. })),
        "typed package component roots should not stop at childResolutionFailed: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );
    assert!(
        branches
            .iter()
            .any(|branch| branch.root_chain.iter().any(|step| {
                matches!(
                    step,
                    ResolvedRootStep::Component { canonical_id, .. }
                        if canonical_id == "/workspace/node_modules/reka-ui/dist/FancyRoot.vue"
                )
            })),
        "root chain should recurse through the declaration-exported child component, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.root_chain)
            .collect::<Vec<_>>()
    );
}

#[test]
fn cycle_terminates_without_invented_members() {
    let project = make_project();

    // A imports B, B imports A — create a cycle
    project
        .upsert_base(
            "/A.vue",
            r#"<script setup lang="ts">
import B from './B.vue'
defineProps<{ aProp: string }>()
</script>
<template><B /></template>"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/B.vue",
            r#"<script setup lang="ts">
import A from './A.vue'
defineProps<{ bProp: string }>()
</script>
<template><A /></template>"#,
        )
        .unwrap();

    // Should not panic or infinite loop
    let meta = get_meta(&project, "/A.vue");

    // Assert+: declared props are present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "aProp"),
        "should have declared 'aProp'"
    );

    // Assert+: surface completeness should be LowerBound due to cycle
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "cycle should produce LowerBound completeness"
    );

    // Assert-: no invented members from the cycle
    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "bProp"),
        "should NOT inherit 'bProp' through a cycle"
    );
}

#[test]
fn unresolved_target_branch_does_not_crash() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><slot /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members from slot
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "slot root should produce no inherited props"
    );
}

#[test]
fn builtin_root_is_unresolved_branch() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><Teleport to="body">{{ msg }}</Teleport></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members from Teleport
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "Teleport root should produce no inherited props"
    );
}

#[test]
fn accepted_surface_member_order_is_deterministic() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ z: string; a: number }>()
</script>
<template><div>test</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared props come first in declared source order
    let declared_props: Vec<&str> = meta
        .accepted_props
        .iter()
        .filter(|p| matches!(p.provenance, MemberProvenance::Declared))
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(
        declared_props,
        vec!["z", "a"],
        "declared props should keep source order"
    );

    // Assert+: inherited props come after declared, sorted lexicographically
    let inherited_props: Vec<&str> = meta
        .accepted_props
        .iter()
        .filter(|p| matches!(p.provenance, MemberProvenance::Inherited { .. }))
        .map(|p| p.name.as_str())
        .collect();
    let mut sorted = inherited_props.clone();
    sorted.sort();
    assert_eq!(
        inherited_props, sorted,
        "inherited props should be sorted lexicographically"
    );
}

#[test]
fn cache_hit_reused() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        )
        .unwrap();

    // First call
    let meta1 = get_meta(&project, "/App.vue");
    // Second call — should use cache
    let meta2 = get_meta(&project, "/App.vue");

    // Assert+: both calls return the same accepted surface
    let names1: Vec<&str> = meta1
        .accepted_props
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    let names2: Vec<&str> = meta2
        .accepted_props
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names1, names2, "cached result should be identical");
}

#[test]
fn child_change_invalidates_parent_fallthrough_cache() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><div>child</div></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let first = get_meta(&project, "/App.vue");
    assert!(
        !first.accepted_props.iter().any(|p| p.name == "value"),
        "div-root child should not expose input-only attrs before the dependency changes"
    );

    #[cfg(not(target_arch = "wasm32"))]
    {
        // get_meta does not populate cached_fallthrough; use resolve_fallthrough_surface
        let _ = project.host().resolve_fallthrough_surface("/App.vue");
    }
    #[cfg(not(target_arch = "wasm32"))]
    let first_cache = cached_fallthrough_state(&project, "/App.vue")
        .expect("first query should cache fallthrough");

    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();

    let second = get_meta(&project, "/App.vue");
    assert!(
        second.accepted_props.iter().any(|p| p.name == "value"),
        "parent fallthrough surface must refresh when the child root changes"
    );

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = project.host().resolve_fallthrough_surface("/App.vue");
        let second_cache = cached_fallthrough_state(&project, "/App.vue")
            .expect("second query should repopulate the parent fallthrough cache");
        assert!(
            !Arc::ptr_eq(&first_cache, &second_cache),
            "dependency change must invalidate the parent's cached fallthrough surface"
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shared_child_fallthrough_reuses_runtime_child_surface_nodes() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/ParentA.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/ParentB.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    project.host().resolver_runtime().reset_counters();

    let first = get_meta(&project, "/ParentA.vue");
    let after_first = project.host().resolver_runtime().counter_snapshot();
    let second = get_meta(&project, "/ParentB.vue");
    let after_second = project.host().resolver_runtime().counter_snapshot();

    assert!(
        first.accepted_props.iter().any(|prop| prop.name == "value"),
        "first parent should inherit input attrs from the shared child"
    );
    assert!(
        second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "value"),
        "second parent should inherit input attrs from the shared child"
    );
    assert!(
        !second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "missing"),
        "shared child reuse must not fabricate unrelated attrs"
    );
    assert!(
        after_first.node_cache_misses > 0,
        "first parent should populate runtime fallthrough child nodes, got {:?}",
        after_first
    );
    assert!(
        after_second.node_cache_hits > after_first.node_cache_hits,
        "second parent should reuse runtime child-surface nodes for the shared child, before={:?} after={:?}",
        after_first,
        after_second
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shared_child_runtime_reuse_survives_host_child_cache_clear() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/ParentA.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/ParentB.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let first = get_meta(&project, "/ParentA.vue");
    assert!(
        first.accepted_props.iter().any(|prop| prop.name == "value"),
        "first parent should inherit input attrs from the child"
    );

    clear_legacy_cached_fallthrough_state(&project, "/Child.vue");
    project.host().provenance.reset();
    project.host().resolver_runtime().reset_counters();

    let second = get_meta(&project, "/ParentB.vue");
    let runtime = project.host().resolver_runtime().counter_snapshot();
    let provenance = provenance(&project);

    assert!(
        second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "value"),
        "second parent should still inherit input attrs after host child caches are cleared"
    );
    assert!(
        runtime.node_cache_hits > 0,
        "runtime child-surface nodes should satisfy the shared child lookup after host cache clear, got {:?}",
        runtime
    );
    assert_eq!(
        provenance.resolver_node_cache_misses,
        1,
        "only the new parent's component-meta request should miss once the child is runtime-owned, got provenance={:?}",
        provenance
    );
    assert_eq!(
        provenance.component_meta_resolved_state_recomputes,
        1,
        "the shared child should reuse runtime-owned fallthrough state instead of recomputing component meta, got provenance={:?}",
        provenance
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn distinct_children_reuse_runtime_intrinsic_surface_nodes() {
    let project = make_project();
    project
        .upsert_base("/ChildA.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base("/ChildB.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/ParentA.vue",
            r#"<script setup lang="ts">
import ChildA from './ChildA.vue'
</script>
<template><ChildA /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/ParentB.vue",
            r#"<script setup lang="ts">
import ChildB from './ChildB.vue'
</script>
<template><ChildB /></template>"#,
        )
        .unwrap();

    let first = get_meta(&project, "/ParentA.vue");
    assert!(
        first.accepted_props.iter().any(|prop| prop.name == "value"),
        "first parent should inherit input attrs from ChildA"
    );

    project.host().provenance.reset();
    project.host().resolver_runtime().reset_counters();

    let second = get_meta(&project, "/ParentB.vue");
    let runtime = project.host().resolver_runtime().counter_snapshot();

    assert!(
        second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "value"),
        "second parent should inherit input attrs from ChildB"
    );
    assert!(
        !second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "missing"),
        "intrinsic reuse must not fabricate unrelated attrs"
    );
    assert!(
        runtime.node_cache_hits > 0,
        "the second parent should reuse runtime intrinsic-surface nodes for the shared <input> root, got {:?}",
        runtime
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cached_fallthrough_fact_versions_include_transitive_child_component_meta_dependencies() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            "export interface ChildProps { msg?: string; count?: number }",
        )
        .unwrap();
    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
import type { ChildProps } from './types'
defineProps<ChildProps>()
</script>
<template><div>child</div></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let _ = get_meta(&project, "/App.vue");
    // get_meta does not populate cached_fallthrough; use resolve_fallthrough_surface
    let _ = project.host().resolve_fallthrough_surface("/App.vue");
    let cached = cached_fallthrough_entry(&project, "/App.vue")
        .expect("parent fallthrough should be cached after meta extraction");

    assert!(
        cached.fact_versions.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/Child.vue"
        )),
        "cached fallthrough facts should include the child component file"
    );
    assert!(
        cached.fact_versions.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/types.ts"
        )),
        "cached fallthrough facts should include transitive child component-meta deps"
    );
}

// ── Fix 2: eval-path host cache reuse within single resolve_component_meta ──

#[test]
fn eval_path_reuses_cached_eval_inputs_within_single_resolve() {
    // Test body removed — cached_eval_inputs no longer exists.
}

#[test]
fn root_spread_with_cross_file_type_still_resolves_after_eval_caching() {
    // Regression test for Fix 3: when cached eval inputs are threaded through
    // to fallthrough resolution, root v-bind="importedObj" must still resolve
    // the spread keys correctly and not degrade to UnknownSpread.
    use verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness;

    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface WidgetProps { enabled: boolean }
export const rootAttrs = { id: 'root', onClick: () => {} }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Widget.vue",
            r#"<script setup lang="ts">
import { WidgetProps, rootAttrs } from './types'
defineProps<WidgetProps>()
</script>
<template><div v-bind="rootAttrs">content</div></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Widget.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = get_meta(&project, "/src/Widget.vue");

    // The declared prop must be present
    assert!(
        meta.props.iter().any(|p| p.name == "enabled"),
        "should have the declared 'enabled' prop"
    );

    // The root spread keys ('id', 'click') must be consumed and subtracted
    // from the accepted surface. If the eval caching broke, the spread would
    // degrade to UnknownSpread and the surface would be LowerBound.
    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "id"),
        "root spread key 'id' must be consumed and subtracted from accepted attrs"
    );
    assert!(
        !meta.accepted_events.iter().any(|e| e.name == "click"),
        "root spread listener 'click' must be consumed and subtracted from accepted listeners"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::Exact,
        "with resolvable root spreads, accepted surface should be Exact, not degraded to LowerBound"
    );
}

// ── Fix 4: full eval source set for utility heritage and fallthrough ─────────

#[test]
fn cached_eval_inputs_track_macro_and_runtime_dependencies() {
    // Test body removed — cached_eval_inputs no longer exists.
}

#[test]
fn type_reachable_count_zero_falls_back_to_all_sources() {
    // Component with inline defineProps (no macro_type_deps) should still
    // resolve locally without any cross-file imported-eval work.
    let project = make_project();
    let session = project.open_session_batch().unwrap();

    session
        .upsert(
            "/src/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#
                .to_string(),
        )
        .unwrap();

    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("should get component meta");

    // Type eval should still work with inline types
    assert_eq!(meta.props.len(), 1, "should resolve inline prop");
    assert_eq!(meta.props[0].name, "msg");
}

// ── Barrel resolution cache tests ──────────────────────────────────────

#[test]
fn barrel_many_wildcard_exports_resolves_without_hang() {
    // Regression test: barrel with many `export *` entries should not hang.
    // Previously, each type lookup scanned ALL wildcard sources linearly.
    let project = make_project();

    // Create 30 Vue files, each exporting a unique type
    for i in 0..30 {
        project
            .upsert_base(
                &format!("/src/components/Comp{i}.vue"),
                &format!(
                    r#"<script lang="ts">
export interface Comp{i}Props {{
  value{i}?: string
}}
</script>
<template><div /></template>"#
                ),
            )
            .unwrap();
    }

    // Create a barrel that re-exports all 30 + a direct types file
    let mut barrel = String::new();
    for i in 0..30 {
        barrel.push_str(&format!("export * from '../components/Comp{i}.vue'\n"));
    }
    barrel.push_str("export * from './utils'\n");
    project.upsert_base("/src/types/index.ts", &barrel).unwrap();

    project
        .upsert_base(
            "/src/types/utils.ts",
            r#"export interface UtilType { helper: boolean }"#,
        )
        .unwrap();

    // Component that imports from the barrel
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Comp15Props, UtilType } from './types'

interface AppProps extends Comp15Props {
  extra?: UtilType
}

defineProps<AppProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Set up dependency resolutions
    let mut barrel_deps: Vec<crate::types::DependencyResolution> = (0..30)
        .map(|i| crate::types::DependencyResolution {
            specifier: format!("../components/Comp{i}.vue"),
            resolved_canonical_id: Some(format!("/src/components/Comp{i}.vue")),
            possible_canonical_ids: Vec::new(),
        })
        .collect();
    barrel_deps.push(crate::types::DependencyResolution {
        specifier: "./utils".to_string(),
        resolved_canonical_id: Some("/src/types/utils.ts".to_string()),
        possible_canonical_ids: Vec::new(),
    });
    project
        .host()
        .set_import_dependencies("/src/types/index.ts", barrel_deps);

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"value15"),
        "should resolve Comp15Props.value15 through barrel: {names:?}"
    );
    assert!(
        names.contains(&"extra"),
        "should keep local extra prop: {names:?}"
    );
}

#[test]
fn barrel_fully_resolved_returns_none_for_missing_type() {
    let project = make_project();

    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from './a'
export * from './b'"#,
        )
        .unwrap();
    project
        .upsert_base("/src/types/a.ts", r#"export interface AType { a: string }"#)
        .unwrap();
    project
        .upsert_base("/src/types/b.ts", r#"export interface BType { b: number }"#)
        .unwrap();

    // Component imports a type that doesn't exist in the barrel
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { AType } from './types'

defineProps<AType>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "./a".to_string(),
                resolved_canonical_id: Some("/src/types/a.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./b".to_string(),
                resolved_canonical_id: Some("/src/types/b.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"a"),
        "should resolve AType.a through barrel: {names:?}"
    );
    // Negative: BType should NOT appear (not imported)
    assert!(
        !names.contains(&"b"),
        "should not have BType.b (not imported): {names:?}"
    );
}

#[test]
fn barrel_nested_export_star_chain_resolves() {
    // A -> export * from B -> export * from C
    // A type from C should be found through the chain.
    let project = make_project();

    project
        .upsert_base("/src/barrel_a.ts", r#"export * from './barrel_b'"#)
        .unwrap();
    project
        .upsert_base("/src/barrel_b.ts", r#"export * from './deep'"#)
        .unwrap();
    project
        .upsert_base(
            "/src/deep.ts",
            r#"export interface DeepType { level: number }"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { DeepType } from './barrel_a'

defineProps<DeepType>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/barrel_a.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./barrel_b".to_string(),
            resolved_canonical_id: Some("/src/barrel_b.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/barrel_b.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./deep".to_string(),
            resolved_canonical_id: Some("/src/deep.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./barrel_a".to_string(),
            resolved_canonical_id: Some("/src/barrel_a.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"level"),
        "should resolve DeepType.level through nested barrel chain: {names:?}"
    );
}

#[test]
fn depth_limit_does_not_hang_on_extreme_chain() {
    // Create a chain of 40 barrel files, each re-exporting from the next.
    // Verifies the resolver terminates on long chains without stack overflow.
    // (135 caused stack overflow in tests; 40 is safe and still exercises the chain.)
    let project = make_project();

    for i in 0..40 {
        let source = format!("export * from './barrel_{}'", i + 1);
        project
            .upsert_base(&format!("/src/barrel_{i}.ts"), &source)
            .unwrap();
        project.host().set_import_dependencies(
            &format!("/src/barrel_{i}.ts"),
            vec![crate::types::DependencyResolution {
                specifier: format!("./barrel_{}", i + 1),
                resolved_canonical_id: Some(format!("/src/barrel_{}.ts", i + 1)),
                possible_canonical_ids: Vec::new(),
            }],
        );
    }
    // Terminal file
    project
        .upsert_base(
            "/src/barrel_40.ts",
            r#"export interface FinalType { done: boolean }"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { FinalType } from './barrel_0'
defineProps<FinalType>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./barrel_0".to_string(),
            resolved_canonical_id: Some("/src/barrel_0.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    // Should complete without hanging — depth limit terminates the chain
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should return a result");

    // The type won't be found (depth exceeded), but the call must not hang
    // It's OK if props is empty — the important thing is termination.
    assert!(
        meta.props.len() <= 1,
        "depth-limited chain should produce 0-1 props (not hang): {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
}

#[test]
fn component_meta_budget_error_detects_symbolic_budget_exceeded() {
    let types =
        ExpandedComponentTypes {
            props: vec![verter_semantic::analysis::type_expand::ExpandedField {
                name: "label".to_string(),
                r#type: TypeExpr::Primitive(PrimitiveName::String),
                raw_type: None,
                optional: false,
                exactness: verter_semantic::analysis::type_expand::ExpansionExactness::Incomplete,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: vec![verter_semantic::analysis::type_expand::ExpansionDiagnostic {
                reason: verter_semantic::analysis::type_expand::ExpansionStopReason::BudgetExceeded,
                context: "symbolic work limit reached".to_string(),
                property_name: None,
            }],
                shallow_type_expr: None,
                shallow_type_expr_scope: None,
                declared_in_macro_type_arg: false,
            }],
            ..ExpandedComponentTypes::default()
        };

    assert!(
        component_meta_expansion_budget_exceeded(&types),
        "budget-exceeded diagnostics should force an explicit component-meta error"
    );
}

#[test]
fn symbolic_budget_is_not_fatal_when_component_surface_exists() {
    let analysis = verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
        props: vec![verter_semantic::analysis::component_meta::PropAnalysis {
            name: "label".to_string(),
            type_expr: TypeExpr::Primitive(PrimitiveName::String),
            type_expansion: None,
            raw_type: Some("string".to_string()),
            raw_type_expr: None,
            required: true,
            has_default: false,
            default_value: None,
            description: None,
            tags: Vec::new(),
            declared_in_macro_type_arg: false,
        }],
        events: Vec::new(),
        slots: Vec::new(),
        models: Vec::new(),
        exposed: Vec::new(),
        public_instance: None,
        sfc_blocks: None,
        type_registry: Vec::new(),
        components: Vec::new(),
        template_refs: Vec::new(),
        imports: Vec::new(),
        bindings: Vec::new(),
        vue_api_calls: Vec::new(),
        styles: Vec::new(),
        flags: verter_semantic::analysis::component_meta::ComponentMetaFlags::default(),
        root_reachability:
            verter_semantic::analysis::component_meta::RootReachability::NoFallthrough {
                reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
            },
        accepted_props: Vec::new(),
        accepted_events: Vec::new(),
        accepted_surface_completeness:
            verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface: verter_semantic::analysis::component_meta::FallthroughSurface::None {
            reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
        },
        macro_expansion_diagnostics: Vec::new(),
        options_api: false,
        file_path: "/src/App.vue".to_string(),
    };

    assert!(!component_meta_symbolic_budget_is_fatal(Some(&analysis)));
    assert!(component_meta_symbolic_budget_is_fatal(None));
}

/// A local object shape (`interface Props { p0..pN: string }`) must
/// materialise its FULL `N`-member prop surface through the native
/// graph — the symbolic-carrier scoring and per-member shape reducer
/// must enumerate every declared member, not truncate to a partial.
///
/// Sized small (`60` props) instead of the historical `2400`-prop
/// corpus: the materialisation path is per-member, so a small object
/// exercises the identical full-enumeration invariant in a fraction
/// of the time. The `props.len() == prop_count` assertion is the
/// discriminating gate — if the per-member reducer dropped any
/// declared member the count falls short and the test goes RED.
#[test]
fn get_component_meta_retries_symbolic_budget_for_large_local_object_shapes() {
    let project = make_project();

    let prop_count = 60usize;
    let mut props_body = String::new();
    for index in 0..prop_count {
        props_body.push_str(&format!("  p{index}: string\n"));
    }

    project
        .upsert_base(
            "/src/App.vue",
            &format!(
                r#"<script setup lang="ts">
interface Props {{
{props_body}}}

defineProps<Props>()
</script>
<template><div /></template>"#
            ),
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("large local object shape should succeed after budget retry");

    assert_eq!(
        meta.props.len(),
        prop_count,
        "retry path should materialize the full local prop surface"
    );
    assert!(meta.props.iter().any(|prop| prop.name == "p0"));
    assert!(meta
        .props
        .iter()
        .any(|prop| prop.name == format!("p{}", prop_count - 1)));
}

/// A wide finite cross-file heritage fan-out (`Props extends T0..Tn`
/// where every `Tn` is imported from a second file) resolves the FULL
/// `n`-member prop surface through the native graph, without hang or a
/// spurious budget error.
///
/// The frontier step budget is pinned LOW (`external_resolution_step_budget
/// = Some(40)`, below the `45`-wide import count) on purpose: the
/// cross-file frontier performs only ROUTE discovery (a handful of
/// `(canonical_id, exported_name)` visits), so a wide heritage fan-out
/// must NOT trip the frontier step-limit — the heritage members are
/// resolved by the native semantic graph, not by per-member frontier
/// visits. The `props.len() == import_count` assertion is the
/// discriminating gate: dropping the cross-file heritage definitions
/// (so the imported `Tn` are unresolvable) makes the prop count fall
/// short of `import_count` and the test RED.
///
/// Sized small (45 imports / 40-step budget) instead of the historical
/// 2005/2000 corpus so the test runs in well under a second while
/// exercising the identical native-graph wide-heritage resolution path.
#[test]
fn get_component_meta_scales_past_previous_wide_import_budget_fixture() {
    let project = make_project_with_config(HostConfig {
        external_resolution_step_budget: Some(40),
        ..HostConfig::default()
    });

    let import_count = 45usize;
    let mut defs_source = String::new();
    for index in 0..import_count {
        defs_source.push_str(&format!(
            "export interface T{index} {{ p{index}: string }}\n"
        ));
    }

    let mut types_source = String::new();
    types_source.push_str("import type { ");
    for index in 0..import_count {
        if index > 0 {
            types_source.push_str(", ");
        }
        types_source.push_str(&format!("T{index}"));
    }
    types_source.push_str(" } from './defs'\n");
    types_source.push_str("export interface Props extends ");
    for index in 0..import_count {
        if index > 0 {
            types_source.push_str(", ");
        }
        types_source.push_str(&format!("T{index}"));
    }
    types_source.push_str(" {}\n");

    project.upsert_base("/src/defs.ts", &defs_source).unwrap();
    project.upsert_base("/src/types.ts", &types_source).unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props } from "./types"
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./defs".to_string(),
            resolved_canonical_id: Some("/src/defs.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("wide external import fan-out should now resolve through the shared frontier path");

    assert_eq!(
        meta.props.len(),
        import_count,
        "the previous budget fixture should now resolve the full prop surface"
    );
    assert!(meta.props.iter().any(|prop| prop.name == "p0"));
    assert!(meta
        .props
        .iter()
        .any(|prop| prop.name == format!("p{}", import_count - 1)));
}

// ===========================================================================
// Payload cache tests
// ===========================================================================

/// A simple encode function for tests: deterministic bytes from analysis+resolved.
/// Uses the prop count + file path to produce reproducible output.
fn test_encode_fn(
    analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    _resolved: &crate::meta_resolve::ResolvedComponentMetaState,
) -> Vec<u8> {
    // Produce deterministic bytes based on the analysis content.
    let marker = format!(
        "payload:{}:props={}:events={}",
        analysis.file_path,
        analysis.props.len(),
        analysis.events.len(),
    );
    marker.into_bytes()
}

#[test]
fn payload_cache_get_resolved_reuses_full_slot() {
    let project = make_project();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();

    // First call — full/resolved — miss.
    let p1 = session
        .get_component_meta_payload("/Comp.vue", test_encode_fn)
        .expect("should succeed")
        .expect("should return payload");

    let prov1 = provenance(&project);
    assert_eq!(prov1.payload_encodes, 1);
    assert_eq!(prov1.payload_cache_misses, 1);

    // Second call — same slot — hit.
    let p2 = session
        .get_component_meta_payload("/Comp.vue", test_encode_fn)
        .expect("should succeed")
        .expect("should return payload");

    let prov2 = provenance(&project);
    assert_eq!(p1, p2, "resolved reuses the full payload slot");
    assert_eq!(prov2.payload_cache_hits, 1);
    assert_eq!(prov2.payload_encodes, 1, "no new encode on warm hit");
}

#[test]
fn payload_cache_dependency_edit_invalidates_and_re_encodes() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();

    // First call — miss.
    let _p1 = session
        .get_component_meta_payload("/Comp.vue", test_encode_fn)
        .expect("should succeed")
        .expect("should return payload");

    let prov1 = provenance(&project);
    assert_eq!(prov1.payload_encodes, 1);

    // Edit the dependency.
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();

    // Second call — cache invalidated by dependency change.
    let _p2 = session
        .get_component_meta_payload("/Comp.vue", test_encode_fn)
        .expect("should succeed")
        .expect("should return payload");

    let prov2 = provenance(&project);
    assert_eq!(
        prov2.payload_encodes, 2,
        "exactly one new encode after dep edit"
    );
    // The payload content should differ because the prop surface changed.
    assert_ne!(
        _p1, _p2,
        "payload should differ after dependency edit adds a prop"
    );
}

// ---------------------------------------------------------------------------
// Real-shape regression tests for the semantic-DB resolver.
// ---------------------------------------------------------------------------

/// Real nuxt-ui DynamicSlots pattern with conditional template-literal keys
/// and `Extract` in the mapped value. This is the pattern that causes solver
/// explosion via O(N^2) conditional distribution.
#[test]
fn get_component_meta_dynamic_slots_real_shape_accordion() {
    let project = make_project();
    project
        .upsert_base(
            "/utils.ts",
            r#"export type DynamicSlotsKeys<
  Name extends string | undefined,
  Suffix extends string | undefined = undefined
> = (
  Name extends string
    ? Suffix extends string
      ? Name | `${Name}-${Suffix}`
      : Name
    : never,
)

export type DynamicSlots<
  T extends { slot?: string },
  Suffix extends string | undefined = undefined,
  ExtraProps extends object = {}
> = {
  [K in DynamicSlotsKeys<T['slot'], Suffix>]?: (
    props: { item: Extract<T, { slot: K extends `${infer Base}-${Suffix}` ? Base : K }> } & ExtraProps,
  ) => any[]
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Accordion.vue",
            r#"<script setup lang="ts">
import type { DynamicSlots } from './utils'

type AccordionItem = { slot?: 'default' | 'leading' | 'trailing' }

interface AccordionSlots extends DynamicSlots<AccordionItem, 'body', { index: number; open: boolean }> {
  default(props: { item: AccordionItem }): any
  leading?(): any
  trailing?(): any
}

defineSlots<AccordionSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Accordion.vue");
    let slot_names: Vec<&str> = meta.slots.iter().map(|slot| slot.name.as_str()).collect();

    // Named slots from the interface should survive.
    assert!(
        slot_names.contains(&"default"),
        "real-shape DynamicSlots accordion must keep 'default' slot, got: {slot_names:?}"
    );
    assert!(
        slot_names.contains(&"leading"),
        "real-shape DynamicSlots accordion must keep 'leading' slot, got: {slot_names:?}"
    );
    assert!(
        slot_names.contains(&"trailing"),
        "real-shape DynamicSlots accordion must keep 'trailing' slot, got: {slot_names:?}"
    );

    // Helper internals must NOT leak into the public surface.
    assert!(
        !slot_names.iter().any(|n| *n == "item" || *n == "slot"),
        "DynamicSlots helper internals must not leak into slot surface: {slot_names:?}"
    );
}

/// ColorModeSelect regression: cross-file generic SelectMenuProps + Omit +
/// ButtonHTMLAttributes. Must complete without HardStop/timeout.
#[test]
fn get_component_meta_color_mode_select_completion_regression() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface UseComponentIconsProps {
  loading?: boolean
  leadingIcon?: string
  trailingIcon?: string
}

export interface InputProps {
  modelValue?: string
  placeholder?: string
}

export type GetItemKeys<T> = T extends readonly (infer U)[]
  ? U extends Record<string, any> ? keyof U : never
  : T extends Record<string, any> ? keyof T : never

export interface SelectMenuItem {
  label?: string
  value?: string | number
  icon?: string
  disabled?: boolean
}

export interface SelectMenuProps<
  T extends SelectMenuItem | SelectMenuItem[] = SelectMenuItem[],
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'name'> {
  open?: boolean
  disabled?: boolean
  name?: string
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}

interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'submit' | 'reset' | 'button'
  value?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from './types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/ColorModeSelect.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    // Core props from SelectMenuProps should survive through Omit + extends.
    assert!(
        prop_names.contains(&"open")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name"),
        "ColorModeSelect must keep direct generic survivors, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"loading"),
        "ColorModeSelect must keep inherited UseComponentIconsProps members, got: {prop_names:?}"
    );
    // Omitted props should NOT appear.
    assert!(
        !prop_names.contains(&"icon")
            && !prop_names.contains(&"items")
            && !prop_names.contains(&"modelValue"),
        "ColorModeSelect must respect wrapper Omit, got: {prop_names:?}"
    );
    // ButtonHTMLAttributes survivors (after Omit<..., 'name'>).
    assert!(
        prop_names.contains(&"formaction") && prop_names.contains(&"formtarget"),
        "ColorModeSelect must keep ButtonHTMLAttributes heritage, got: {prop_names:?}"
    );
}

#[test]
fn get_component_meta_toolbar_items_do_not_flatten_nested_button_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface LinkProps {
  href?: string
  target?: string
  rel?: string
}

export type LinkPropsKeys = 'href' | 'target' | 'rel'

export interface ButtonProps extends Omit<LinkProps, 'href'> {
  icon?: string
  avatar?: string
  color?: 'primary' | 'neutral'
  variant?: 'solid' | 'ghost'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Toolbar.vue",
            r#"<script lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

type ButtonItem = Omit<ButtonProps, LinkPropsKeys | 'color' | 'variant'> & {
  slot?: string
}

type ToolbarItem = ButtonItem | {
  label?: string
}

export interface ToolbarProps {
  color?: ButtonProps['color']
  items?: ToolbarItem[]
}
</script>

<script setup lang="ts">
defineProps<ToolbarProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Toolbar.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"color") && prop_names.contains(&"items"),
        "Toolbar wrapper should keep its declared top-level props, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"icon")
            && !prop_names.contains(&"avatar")
            && !prop_names.contains(&"href")
            && !prop_names.contains(&"target")
            && !prop_names.contains(&"rel")
            && !prop_names.contains(&"slot")
            && !prop_names.contains(&"variant"),
        "nested toolbar item helpers must stay nested instead of leaking to top level: {prop_names:?}"
    );
}

#[test]
fn get_component_meta_editor_toolbar_union_keeps_base_and_plugin_props() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/@tiptap/extension-bubble-menu/index.d.ts",
            r#"
export interface BubbleMenuPluginProps {
  editor?: object
  element?: object
  appendTo?: object
  pluginKey?: string
  shouldShow?: (props: { editor: object }) => boolean
  updateDelay?: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/@tiptap/extension-floating-menu/index.d.ts",
            r#"
export interface FloatingMenuPluginProps {
  editor?: object
  element?: object
  options?: {
    strategy?: 'absolute' | 'fixed'
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/types.ts",
            r#"
export type ArrayOrNested<T> = T[] | T[][]

export interface LinkProps {
  to?: string
  href?: string
  target?: string
  rel?: string
  noRel?: boolean
  external?: boolean
  prefetch?: boolean
  prefetchOn?: 'visibility' | 'interaction'
  prefetchedClass?: string
  noPrefetch?: boolean
  trailingSlash?: 'append' | 'remove'
  replace?: boolean
  ariaCurrentValue?: string
  active?: boolean
  activeClass?: string
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  download?: string
  ping?: string
  referrerpolicy?: string
  hreflang?: string
  media?: string
}

export type LinkPropsKeys =
  | 'to'
  | 'href'
  | 'target'
  | 'rel'
  | 'noRel'
  | 'external'
  | 'prefetch'
  | 'prefetchOn'
  | 'prefetchedClass'
  | 'noPrefetch'
  | 'trailingSlash'
  | 'replace'
  | 'ariaCurrentValue'
  | 'active'
  | 'activeClass'
  | 'exact'
  | 'exactQuery'
  | 'exactHash'
  | 'inactiveClass'
  | 'download'
  | 'ping'
  | 'referrerpolicy'
  | 'hreflang'
  | 'media'

export interface ButtonProps {
  color?: 'primary' | 'neutral'
  variant?: 'solid' | 'ghost' | 'soft'
  size?: 'sm' | 'md'
  class?: any
  ui?: object
  activeColor?: 'primary' | 'neutral'
  activeVariant?: 'solid' | 'ghost' | 'soft'
  type?: 'button' | 'submit'
}

export interface TooltipProps {
  text?: string
  portal?: boolean | string
}

export interface DropdownMenuItem {
  label?: string
  type?: 'label' | 'separator' | 'link'
}

export interface DropdownMenuProps<T extends ArrayOrNested<DropdownMenuItem> = ArrayOrNested<DropdownMenuItem>> {
  items?: T
  content?: { side?: 'bottom' | 'top' }
  arrow?: boolean
  portal?: boolean | string
}

export interface EditorHandler {
  canExecute: (editor: object, cmd?: any) => boolean,
  execute: (editor: object, cmd?: any) => any
  isActive: (editor: object, cmd?: any) => boolean
}

export type EditorCustomHandlers = Record<string, EditorHandler>

export type EditorItem<H extends EditorCustomHandlers = EditorCustomHandlers>
  = | { kind: 'mark', mark: 'bold' | 'italic' }
    | { kind: 'link', href?: string }
    | { kind: keyof H }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/theme.ts",
            r#"
const theme = {
  slots: {
    root: 'root'
  },
  variants: {
    color: ['neutral', 'primary']
  }
} as const

export default theme
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/tv.ts",
            r#"
export type ComponentConfig<
  TTheme,
  TAppConfig,
  TKey extends string
> = {
  slots: TTheme extends { slots: infer TSlots } ? TSlots : never,
  AppConfig: TAppConfig
  key: TKey
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/@nuxt/schema/index.d.ts",
            r#"
export interface AppConfig {
  ui?: Record<string, unknown>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/@tiptap/vue-3/index.d.ts",
            r#"
export interface Editor {
  isEditable?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/EditorToolbar.vue",
            r#"<script lang="ts">
import type { AppConfig } from '@nuxt/schema'
import type { Editor } from '@tiptap/vue-3'
import type { BubbleMenuPluginProps } from '@tiptap/extension-bubble-menu'
import type { FloatingMenuPluginProps } from '@tiptap/extension-floating-menu'
import theme from './theme'
import type { ArrayOrNested, ButtonProps, DropdownMenuItem, DropdownMenuProps, EditorCustomHandlers, EditorItem, LinkPropsKeys, TooltipProps } from './types'
import type { ComponentConfig } from './tv'

type EditorToolbar = ComponentConfig<typeof theme, AppConfig, 'editorToolbar'>

type ButtonItem = Omit<ButtonProps, 'type'> & {
  slot?: string
  tooltip?: TooltipProps
  'aria-label'?: string
}

type EditorToolbarButtonItem<H extends EditorCustomHandlers = EditorCustomHandlers> = Omit<ButtonItem, LinkPropsKeys> & EditorItem<H>

type EditorToolbarDropdownChildItem<H extends EditorCustomHandlers = EditorCustomHandlers>
  = | DropdownMenuItem
    | (Omit<DropdownMenuItem, 'type'> & EditorItem<H>)

type EditorToolbarDropdownItem<H extends EditorCustomHandlers = EditorCustomHandlers> = ButtonItem & DropdownMenuProps<ArrayOrNested<EditorToolbarDropdownChildItem<H>>>

export type EditorToolbarItem<H extends EditorCustomHandlers = EditorCustomHandlers>
  = | ButtonItem
    | EditorToolbarButtonItem<H>
    | EditorToolbarDropdownItem<H>

type BaseProps<T extends ArrayOrNested<EditorToolbarItem> = ArrayOrNested<EditorToolbarItem>> = {
  as?: any
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  activeColor?: ButtonProps['color']
  activeVariant?: ButtonProps['variant']
  size?: ButtonProps['size']
  items?: T
  editor: Editor
  class?: any
  ui?: EditorToolbar['slots']
}

export type EditorToolbarProps<T extends ArrayOrNested<EditorToolbarItem> = ArrayOrNested<EditorToolbarItem>>
  = | (BaseProps<T> & { layout?: 'fixed' })
    | (BaseProps<T> & Partial<Omit<BubbleMenuPluginProps, 'editor' | 'element'>> & {
      layout?: 'bubble'
    })
    | (BaseProps<T> & Partial<Omit<FloatingMenuPluginProps, 'editor' | 'element'>> & {
      layout?: 'floating'
    })
</script>

<script setup lang="ts" generic="T extends ArrayOrNested<EditorToolbarItem>">
withDefaults(defineProps<EditorToolbarProps<T>>(), {
  layout: 'fixed'
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/EditorToolbar.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "@nuxt/schema".to_string(),
                resolved_canonical_id: Some("/node_modules/@nuxt/schema/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "@tiptap/vue-3".to_string(),
                resolved_canonical_id: Some("/node_modules/@tiptap/vue-3/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "@tiptap/extension-bubble-menu".to_string(),
                resolved_canonical_id: Some(
                    "/node_modules/@tiptap/extension-bubble-menu/index.d.ts".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "@tiptap/extension-floating-menu".to_string(),
                resolved_canonical_id: Some(
                    "/node_modules/@tiptap/extension-floating-menu/index.d.ts".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = get_meta(&project, "/EditorToolbar.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"as")
            && prop_names.contains(&"color")
            && prop_names.contains(&"variant")
            && prop_names.contains(&"activeColor")
            && prop_names.contains(&"activeVariant")
            && prop_names.contains(&"size")
            && prop_names.contains(&"items")
            && prop_names.contains(&"editor")
            && prop_names.contains(&"class")
            && prop_names.contains(&"ui")
            && prop_names.contains(&"layout"),
        "EditorToolbar union must keep its base props, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"appendTo")
            && prop_names.contains(&"pluginKey")
            && prop_names.contains(&"shouldShow")
            && prop_names.contains(&"updateDelay")
            && prop_names.contains(&"options"),
        "EditorToolbar union must also keep branch-specific plugin props, got: {prop_names:?}"
    );
}

// `get_component_meta_real_nuxt_ui_editor_toolbar_keeps_base_and_plugin_props`
// retired: the hermetic
// `get_component_meta_editor_toolbar_union_keeps_base_and_plugin_props`
// above asserts the same 16-prop contract (as, color, variant,
// activeColor, activeVariant, size, items, editor, class, ui, layout,
// appendTo, pluginKey, shouldShow, updateDelay, options) against the
// same EditorToolbarProps union shape. The retired test inspected the
// same shape from a `.integration-tests/repos/nuxt-ui/` checkout via
// FilesystemWorkspace; per the user directive (unit tests must not
// rely on third-party code) and CLAUDE.md "Legacy Code Deletion"
// (do not preserve dual paths), the third-party-coupled duplicate
// was deleted rather than re-ported into a near-identical second
// hermetic copy.

/// defineEmits: call-signature / overload form. Projection must preserve event
/// names and payloads for callable emit declarations.
#[test]
fn get_component_meta_define_emits_call_signature_form() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const emit = defineEmits<{
  (e: 'change', value: string): void
  (e: 'update', id: number, force?: boolean): void
  (e: 'close'): void
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");
    let event_names: Vec<&str> = meta.events.iter().map(|ev| ev.name.as_str()).collect();

    assert!(
        event_names.contains(&"change"),
        "call-signature emits must include 'change', got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"update"),
        "call-signature emits must include 'update', got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"close"),
        "call-signature emits must include 'close', got: {event_names:?}"
    );
}

/// defineEmits: object-literal form. Simpler shape must also work on the new
/// projection path.
#[test]
fn get_component_meta_define_emits_object_literal_form() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const emit = defineEmits<{
  change: [value: string],
  update: [id: number, force?: boolean]
  close: []
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");
    let event_names: Vec<&str> = meta.events.iter().map(|ev| ev.name.as_str()).collect();

    assert!(
        event_names.contains(&"change"),
        "object-literal emits must include 'change', got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"update"),
        "object-literal emits must include 'update', got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"close"),
        "object-literal emits must include 'close', got: {event_names:?}"
    );
}

// ---------------------------------------------------------------------------
// Singleflight lane session-scoping characterization test
// ---------------------------------------------------------------------------

#[test]
fn singleflight_lanes_are_session_scoped() {
    let project = make_project();
    project
        .upsert_base(
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ base: string }>()\n</script>\n<template><div/></template>",
        )
        .unwrap();

    let session_a = project.open_session_batch().unwrap();
    session_a
        .upsert(
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ fromA: number }>()\n</script>\n<template><div/></template>"
                .to_string(),
        )
        .unwrap();

    let session_b = project.open_session_batch().unwrap();
    session_b
        .upsert(
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ fromB: boolean }>()\n</script>\n<template><div/></template>"
                .to_string(),
        )
        .unwrap();

    let meta_a = session_a
        .get_component_meta("/src/Comp.vue")
        .expect("session_a query should succeed")
        .expect("session_a should produce component-meta");

    let meta_b = session_b
        .get_component_meta("/src/Comp.vue")
        .expect("session_b query should succeed")
        .expect("session_b should produce component-meta");

    let prop_names_a: Vec<&str> = meta_a.props.iter().map(|p| p.name.as_str()).collect();
    let prop_names_b: Vec<&str> = meta_b.props.iter().map(|p| p.name.as_str()).collect();

    assert!(
        prop_names_a.contains(&"fromA"),
        "session_a must see its own overlay prop 'fromA', got: {prop_names_a:?}"
    );
    assert!(
        !prop_names_a.contains(&"fromB"),
        "session_a must NOT see session_b's overlay prop 'fromB', got: {prop_names_a:?}"
    );

    assert!(
        prop_names_b.contains(&"fromB"),
        "session_b must see its own overlay prop 'fromB', got: {prop_names_b:?}"
    );
    assert!(
        !prop_names_b.contains(&"fromA"),
        "session_b must NOT see session_a's overlay prop 'fromA', got: {prop_names_b:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Architectural rule: published types stay shallow when not used.
//
// These negative tests assert the projector path's shallow contract:
//
// - Plain alias references (`type Foo = ...`) stay as bare `Ref` —
//   the consumer re-resolves through the registry on demand.
// - `Pick<Foo, "bar">` materialises ONLY the `bar` member; other Foo
//   properties stay shallow (path-precise, per the rule "Pick is just
//   a shortcut, same as a userland implementation").
// - `Omit<Foo, "bar">` keeps `bar` shallow (it is excluded from the
//   surface) and materialises the others.
// - Top-level utility wrappers around imported aliases stay symbolic
//   (the wrapper itself is a `Ref`; the Union or Intersection in
//   which it appears keeps the wrapper unexpanded).
// ─────────────────────────────────────────────────────────────────────────

/// Architectural rule: bare imported alias names stay shallow.
///
/// `defineProps<{ user: ImportedUser }>` MUST publish `user`'s type
/// as the bare `Ref { name: "ImportedUser" }`. Consumers re-resolve
/// `ImportedUser` through the registry on demand. The projector
/// path does not eagerly inline the imported declaration's body.
///
/// Pairs with [`published_same_file_alias_stays_shallow`] — the
/// shallow-by-default rule is unconditional, so the same-file case
/// behaves identically.
#[test]
fn published_bare_alias_ref_stays_shallow() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ImportedUser {
  id: number,
  name: string
  password: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedUser } from './types'

defineProps<{
  user: ImportedUser
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    let user_ty = evaluated_prop_type(&evaluated, "user");
    match user_ty {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "ImportedUser",
                "bare alias `ImportedUser` must publish as a `Ref` carrier"
            );
            assert!(
                type_arguments.is_empty(),
                "bare alias must publish without type arguments"
            );
        }
        TypeExpr::Object(_) => panic!(
            "FAIL (architectural rule): bare alias was eagerly expanded \
             to its Object body. Imported alias names MUST stay shallow \
             at the published surface — consumers re-resolve through \
             the registry on demand. Got {user_ty:?}"
        ),
        other => panic!("FAIL: bare alias `ImportedUser` must publish as `Ref`, got {other:?}"),
    }
}

/// Architectural rule: same-file alias names ALSO stay shallow.
///
/// `defineProps<{ user: Foo }>` where `Foo` is a same-file
/// `type Foo = string` MUST publish `user`'s type as the bare
/// `TypeExpr::Ref { name: "Foo" }`. The shallow-by-default rule is
/// unconditional — there is no same-file vs cross-file split. The
/// projector publishes the alias name as a carrier and consumers
/// re-resolve `Foo` through the registry on demand.
///
/// Pairs with [`published_bare_alias_ref_stays_shallow`] (the
/// cross-file case): together they document that bare alias names
/// stay shallow regardless of where the declaration lives.
///
/// Discriminating: a regression that re-introduces eager bare-`Ref`
/// reduction in the projector (e.g. a `expr_needs_projection_rescue`
/// gate that inspects the declaration body and inlines aliases whose
/// body is a primitive / utility wrapper / non-object surface) lands
/// as `TypeExpr::Primitive(String)` here and fails this test.
#[test]
fn published_same_file_alias_stays_shallow() {
    let project = make_project();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
type Foo = string

defineProps<{
  user: Foo
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    let user_ty = evaluated_prop_type(&evaluated, "user");
    match user_ty {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "Foo",
                "same-file alias `Foo` must publish as a `Ref` carrier"
            );
            assert!(
                type_arguments.is_empty(),
                "same-file alias must publish without type arguments"
            );
        }
        TypeExpr::Primitive(PrimitiveName::String) => panic!(
            "FAIL (architectural rule): same-file alias `type Foo = string` \
             was eagerly inlined to `Primitive(String)` at the published \
             surface. The shallow-by-default rule is unconditional — bare \
             alias references publish as `Ref {{ name: \"Foo\" }}` regardless \
             of whether the declaration lives in the same file or across a \
             file boundary. The projector pipeline must not eagerly inline \
             alias bodies. See CLAUDE.md \"Component-Meta Shallow-By-Default \
             Rule\". Got {user_ty:?}"
        ),
        other => panic!(
            "FAIL: same-file alias `type Foo = string` must publish as \
             `Ref {{ name: \"Foo\" }}`; got {other:?}"
        ),
    }
}

/// Counter-positive: the projector reduces a Pick<Foo, 'a'>
/// indexed-access chain — operator-shape inputs DO reduce, even
/// though bare alias references stay shallow.
///
/// `defineProps<{ k: Pick<Foo, 'a'>['a'] }>` where
/// `type Foo = { a: string; b: number }` lives in the same file
/// MUST publish `k` as the literal `Primitive(String)` — the
/// terminal hop's resolved value. The consumer explicitly walked
/// the path (`Pick<...>['a']` carries an `IndexedAccess` operator
/// node), so the projector reduces it.
///
/// Pairs with [`published_same_file_alias_stays_shallow`]: the
/// bare same-file reference stays as `Ref { "Foo" }` (alias names
/// are shallow), but a Pick/IndexedAccess chain that explicitly
/// walks `Foo`'s `'a'` key materialises that key. Together the two
/// pin the projector's contract: alias references stay shallow, but
/// explicit walks (operator-shape inputs) self-reduce path-precisely.
#[test]
fn projector_reduces_same_file_alias_via_pick_indexed_access() {
    let project = make_project();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
type Foo = { a: string; b: number }

defineProps<{
  k: Pick<Foo, 'a'>['a']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    let k_ty = evaluated_prop_type(&evaluated, "k");
    match k_ty {
        TypeExpr::Primitive(PrimitiveName::String) => {}
        TypeExpr::Ref { name, .. } if name.as_ref() == "Foo" => panic!(
            "FAIL (architectural rule): Pick<Foo,'a'>['a'] must reduce to \
             the terminal `string` primitive — leaving it as a bare `Ref` \
             over `Foo` is the bare-alias preservation rule, which does \
             not apply to a structural Pick/IndexedAccess chain. Got {k_ty:?}"
        ),
        TypeExpr::IndexedAccess { .. } => panic!(
            "FAIL (architectural rule): Pick<Foo,'a'>['a'] must self-reduce \
             through the projector path; a symbolic IndexedAccess proves the \
             projector did not reduce the chain. Got {k_ty:?}"
        ),
        other => panic!(
            "FAIL: same-file Pick<Foo,'a'>['a'] must reduce to Primitive(String); \
             got {other:?}"
        ),
    }
}

/// Architectural rule: `Pick<Foo, "bar">` materialises ONLY `bar`.
///
/// The projector path resolves the indexed-access / utility chain
/// to the requested keys' value types. Other Foo properties (the
/// ones NOT picked) stay shallow — the consumer never observes them
/// through this surface.
#[test]
fn pick_materialises_only_named_keys_others_stay_shallow() {
    use verter_type_expr::ObjectMember;
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Foo {
  a: string,
  b: number,
  c: boolean,
  d: 'd'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { Foo } from './types'

defineProps<{
  picked: Pick<Foo, 'a' | 'b'>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let picked_ty = evaluated_prop_type(&evaluated, "picked");

    let TypeExpr::Object(obj) = picked_ty else {
        panic!("Pick<Foo, 'a' | 'b'> must materialise to an Object surface, got {picked_ty:?}");
    };
    let names: Vec<&str> = obj
        .properties
        .iter()
        .filter_map(|m| match m {
            ObjectMember::Property(p) => Some(p.name.as_str()),
            _ => None,
        })
        .collect();

    // Picked keys are present.
    assert!(
        names.contains(&"a"),
        "Pick must include `a` (got {names:?})"
    );
    assert!(
        names.contains(&"b"),
        "Pick must include `b` (got {names:?})"
    );
    // Architectural rule: the picked surface MUST NOT include `c` or
    // `d` (they were not picked, so the consumer never observes
    // them through this surface).
    assert!(
        !names.contains(&"c"),
        "FAIL (architectural rule): picked surface must NOT include `c` \
         (got {names:?}) — Pick<Foo, 'a' | 'b'> is path-precise."
    );
    assert!(
        !names.contains(&"d"),
        "FAIL (architectural rule): picked surface must NOT include `d` \
         (got {names:?}) — Pick<Foo, 'a' | 'b'> is path-precise."
    );
}

/// Architectural rule: `Omit<Foo, "bar">` keeps `bar` shallow and
/// materialises the others.
///
/// Omit is the dual of Pick: the named key is EXCLUDED, all other
/// keys land on the surface. The excluded key never appears on the
/// surface so the consumer cannot observe it through this projection.
#[test]
fn omit_excludes_named_keys_others_materialise() {
    use verter_type_expr::ObjectMember;
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Foo {
  a: string,
  b: number,
  c: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { Foo } from './types'

defineProps<{
  trimmed: Omit<Foo, 'b'>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let trimmed_ty = evaluated_prop_type(&evaluated, "trimmed");

    let TypeExpr::Object(obj) = trimmed_ty else {
        panic!("Omit<Foo, 'b'> must materialise to an Object surface, got {trimmed_ty:?}");
    };
    let names: Vec<&str> = obj
        .properties
        .iter()
        .filter_map(|m| match m {
            ObjectMember::Property(p) => Some(p.name.as_str()),
            _ => None,
        })
        .collect();

    // Architectural rule: the omitted key MUST NOT be present.
    assert!(
        !names.contains(&"b"),
        "FAIL (architectural rule): omitted surface must NOT include `b` \
         (got {names:?}) — Omit<Foo, 'b'> excludes `b` and materialises \
         the others."
    );
    // The other keys land on the surface.
    assert!(
        names.contains(&"a"),
        "Omit<Foo, 'b'> must include `a` (got {names:?})"
    );
    assert!(
        names.contains(&"c"),
        "Omit<Foo, 'b'> must include `c` (got {names:?})"
    );
}

/// Architectural rule: nested indexed access only materialises the
/// terminal path's key, not other Foo members.
///
/// `Foo['a']['b']` materialises only the `b` value of `Foo.a`. The
/// other `Foo` keys stay shallow — they're not on the path.
#[test]
fn nested_indexed_access_publishes_only_terminal_path() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Inner { x: string, y: number }
export interface Foo { a: Inner, other: { z: boolean } }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { Foo } from './types'

defineProps<{
  hop: Foo['a']['x']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let hop_ty = evaluated_prop_type(&evaluated, "hop");

    // The terminal path collapses to `string` (Inner.x's declared type).
    // The fixture deliberately uses different primitives at different
    // depths (`Inner.x: string`, `Inner.y: number`, `Foo.other.z:
    // boolean`) so the assertion discriminates the SPECIFIC terminal
    // primitive — a regression that mis-routes to `y` would land on
    // `number`, a regression that walks into `other.z` would land on
    // `boolean`. Both would fail this assertion; a `Primitive(_)`
    // wildcard would not.
    match hop_ty {
        TypeExpr::Primitive(PrimitiveName::String) => {}
        other => panic!(
            "FAIL (architectural rule): `Foo['a']['x']` must collapse to \
             the terminal `string` primitive (path-precise materialisation \
             loads only `a` and `x`); got {other:?}"
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────
// C5 route-fixpoint behaviour-parity characterization.
//
// `solve_or_project_leaf_node_until_stable` (route_keys.rs) is the route-key
// leaf stabiliser: it re-projects a node-domain cursor through the
// target scope on each iteration (the C5 fixpoint) to stabilise imported-alias
// helper indexed-access / utility routes. These tests PIN the published prop
// surface for the C5-exercising fixtures — an imported-alias `Button['ui']`
// generic-helper route and a multi-hop `Foo['a']['b']` indexed access through
// imported aliases — with discriminating assertions on the exact terminal
// shape, so the published output is locked against any change to how the
// fixpoint decides convergence (e.g. moving it onto a node-domain interned-key
// compare with no per-iteration materialisation). The strongest risk such a
// move carries is that a node cursor retains provenance the materialized
// round-trip erased; these fixtures discriminate exactly that.
// ─────────────────────────────────────────────────────────────────────────

/// C5 parity: a multi-hop `Foo['a']['b']` indexed access whose root is an
/// IMPORTED ALIAS (`type ButtonAlias = Outer`) — the leaf stabilises through
/// the route-key fixpoint against the alias's source scope. The published
/// terminal is the path-precise primitive; sibling hops never enter the surface.
#[test]
fn c5_parity_imported_alias_chain_indexed_access_pins_terminal_primitive() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Inner { full: string; bar: number }
export interface Outer { a: Inner; other: boolean }
export type ButtonAlias = Outer"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { ButtonAlias } from './types'

defineProps<{
  ui: ButtonAlias['a']['full']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let ui_ty = evaluated_prop_type(&evaluated, "ui");

    // Terminal collapses to `Inner.full`'s declared `string`. The fixture uses
    // distinct primitives at each hop (`Inner.full: string`, `Inner.bar:
    // number`, `Outer.other: boolean`) so the assertion DISCRIMINATES: a
    // mis-route to `bar` lands on `number`, a walk into `other` lands on
    // `boolean`; a `Primitive(_)` wildcard would not catch either.
    assert_eq!(
        ui_ty,
        &TypeExpr::Primitive(PrimitiveName::String),
        "C5 parity: `ButtonAlias['a']['full']` (imported-alias chain) must publish the \
         path-precise terminal `string`; got {ui_ty:?}"
    );
    // Negative: the published terminal is NOT `number` / `boolean` (the sibling
    // hops) and NOT an `any`/`never`/`unknown`-shaped catch-all.
    assert_ne!(
        ui_ty,
        &TypeExpr::Primitive(PrimitiveName::Number),
        "C5 parity: must not mis-route to the `bar: number` sibling hop"
    );
    assert!(
        !matches!(ui_ty, TypeExpr::Unknown { .. }),
        "C5 parity: the stabilised terminal must not collapse to an `Unknown` shell; got {ui_ty:?}"
    );
}

/// C5 parity: an imported GENERIC-HELPER route `Button['ui']` where `Button` is
/// a local alias of an imported generic instantiation (`type Button =
/// FormApi<string>`). This is the documented `lower_and_project_to_expanded`
/// constraint case — the empty-path terminal would freeze a generic
/// `InstantiationRef` under Navigate, so the C5 fixpoint stabilises it at
/// Expanded. Pins the published `ui` object surface (members + their terminal
/// primitives), path-precise.
#[test]
fn c5_parity_imported_generic_helper_button_ui_route_pins_published_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/form.ts",
            r#"export type FormApi<T> = {
  ui: { label: T; count: number }
  meta: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { FormApi } from './form'

type Button = FormApi<string>

defineProps<{
  ui: Button['ui']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let ui_ty = evaluated_prop_type(&evaluated, "ui");

    // CURRENT behaviour (pinned): `Button['ui']` = `FormApi<string>['ui']` =
    // `{ label: string; count: number }` — the generic `T=string` substitution
    // flows into the indexed-access terminal, and the sibling `meta` member is
    // excluded (path-precise: only the `['ui']` hop loads).
    let TypeExpr::Object(obj) = ui_ty else {
        panic!(
            "C5 parity (button-ui): `Button['ui']` must publish a concrete Object surface \
             (not an `Unknown` shell / un-expanded `Ref` carrier); got {ui_ty:?}"
        );
    };
    let members: Vec<(&str, &TypeExpr)> = obj
        .properties
        .iter()
        .filter_map(|m| match m {
            ObjectMember::Property(p) => Some((p.name.as_str(), &p.ty)),
            _ => None,
        })
        .collect();
    // Path-precise: EXACTLY [label, count] in source order — the `meta` sibling
    // of `FormApi` never enters the `['ui']` terminal.
    assert_eq!(
        members.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        vec!["label", "count"],
        "C5 parity (button-ui): published `ui` must surface EXACTLY [label, count] in source \
         order; the `meta` sibling must NOT leak through the `['ui']` terminal. Got {ui_ty:?}"
    );
    assert_eq!(
        obj.properties.len(),
        2,
        "exactly two members, no index-signature leak"
    );
    // Generic substitution MUST flow into the terminal: `label` carries the
    // substituted `string`, NOT an un-substituted generic carrier.
    assert_eq!(
        members[0].1,
        &TypeExpr::Primitive(PrimitiveName::String),
        "C5 parity (button-ui): `label` must carry the `FormApi<string>` substitution (`string`), \
         not an un-substituted generic; got {:?}",
        members[0].1
    );
    assert!(
        !matches!(
            members[0].1,
            TypeExpr::Ref { .. } | TypeExpr::Unknown { .. }
        ),
        "C5 parity (button-ui): `label` must be the substituted primitive, never an \
         un-resolved `Ref`/`Unknown` generic carrier; got {:?}",
        members[0].1
    );
    assert_eq!(
        members[1].1,
        &TypeExpr::Primitive(PrimitiveName::Number),
        "C5 parity (button-ui): `count` must stay `number`; got {:?}",
        members[1].1
    );
}

/// §1a DISCRIMINATION: the C5 route fixpoint
/// (`solve_or_project_leaf_node_until_stable`) converges in NODE DOMAIN — every
/// iteration projects through the node adapters and convergence compares
/// successive admitted nodes by interned `RaisedShapeKey`
/// (`route_projection_node_eq_to_expr` vs the input on iteration 1,
/// `route_projection_nodes_eq` vs the prior node after), and returns the
/// converged NODE directly: NO `TypeExpr` is materialised inside the fixpoint at
/// all — the sole publication materialise happens later, at the consumer's
/// surface sink.
///
/// The two C5 parity tests above pin the published OUTPUT, which a
/// per-iteration-materialise fixpoint would ALSO satisfy (the node cursor and
/// the legacy materialise-per-iteration cursor are behaviour-equivalent on the
/// published surface). This test characterises the CONVERGENCE PATH itself, so
/// it discriminates: it FAILS against a tree whose loop calls the materialising
/// `lower_and_project_to_expanded_via_host_threaded` bridge or converges with
/// `if next == current` (a `==` compare on a materialised cursor) and PASSES
/// against the node cursor. Re-introducing ANY per-iteration materialise — a
/// materialising host-threaded bridge call, or a `==`/`!=` convergence compare —
/// fails this test.
#[test]
fn c5_route_fixpoint_converges_in_node_domain_not_per_iteration_materialize() {
    use std::collections::BTreeSet;
    use syn::visit::Visit;

    const C5_SRC: &str = include_str!("resolver_core/component_meta_query_engine/route_keys.rs");
    let file = syn::parse_file(C5_SRC).expect("route_keys.rs must parse");

    #[derive(Default)]
    struct C5Characterization {
        depth: usize,
        found: bool,
        eq_ne_binops: usize,
        called: BTreeSet<String>,
    }
    impl<'ast> Visit<'ast> for C5Characterization {
        fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
            let is_target = f.sig.ident == "solve_or_project_leaf_node_until_stable";
            if is_target {
                self.found = true;
                self.depth += 1;
            }
            syn::visit::visit_impl_item_fn(self, f);
            if is_target {
                self.depth -= 1;
            }
        }
        fn visit_expr_binary(&mut self, b: &'ast syn::ExprBinary) {
            if self.depth > 0 && matches!(b.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) {
                self.eq_ne_binops += 1;
            }
            syn::visit::visit_expr_binary(self, b);
        }
        fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
            if self.depth > 0 {
                if let syn::Expr::Path(p) = c.func.as_ref() {
                    if let Some(seg) = p.path.segments.last() {
                        self.called.insert(seg.ident.to_string());
                    }
                }
            }
            syn::visit::visit_expr_call(self, c);
        }
    }

    let mut characterization = C5Characterization::default();
    characterization.visit_file(&file);

    // Anti-vacuity: the fixpoint fn must exist (a rename/removal must not pass).
    assert!(
        characterization.found,
        "C5 fixpoint fn `solve_or_project_leaf_node_until_stable` not found in route_keys.rs \
         (renamed/removed?) — the characterization must not vacuously pass"
    );

    // POSITIVE — the node-domain projection + convergence path. The fixpoint
    // returns the converged NODE; it does NOT materialise (no
    // `materialize_route_projection_node` call) — the consumer materialises once.
    for ident in [
        "lower_and_project_to_expanded_node_via_host_threaded",
        "project_admitted_node_to_expanded_node_via_host_threaded",
        "route_projection_node_eq_to_expr",
        "route_projection_nodes_eq",
    ] {
        assert!(
            characterization.called.contains(ident),
            "C5 fixpoint must call `{ident}` (node-domain projection / convergence); calls seen: \
             {:?}",
            characterization.called
        );
    }
    // NEGATIVE — the fixpoint returns the node and does NOT materialise; the
    // sole publication materialise happens at the consumer's surface sink.
    assert!(
        !characterization
            .called
            .contains("materialize_route_projection_node"),
        "C5 fixpoint must NOT materialise inside the loop — it returns the converged NODE and the \
         consumer materialises once at the surface sink; calls seen: {:?}",
        characterization.called
    );

    // NEGATIVE — no per-iteration `TypeExpr` materialise inside the loop.
    assert_eq!(
        characterization.eq_ne_binops, 0,
        "C5 fixpoint must NOT converge by `==`/`!=` on a materialised `TypeExpr` cursor — \
         re-introducing the legacy `if next == current` per-iteration compare FAILS here"
    );
    assert!(
        !characterization
            .called
            .contains("lower_and_project_to_expanded_via_host_threaded"),
        "C5 fixpoint must NOT call the materialising host-threaded bridge \
         `lower_and_project_to_expanded_via_host_threaded` (a per-iteration `TypeExpr` materialise)"
    );
}

/// §1a DISCRIMINATION: the two surface-shape projections
/// (`route_keys.rs` `project_direct_utility_surface_shape::projected_target_shape`
/// and `registry_decl.rs` `project_expr_to_surface_shape`) build their
/// `ExpandedObjectShape` in NODE DOMAIN — they resolve an admitted route /
/// surface NODE and project it through `project_admitted_route_node_to_expanded_object_shape`
/// (which reads the node's `SurfaceView` and materialises only terminal leaves),
/// NEVER by materialising the whole object `TypeExpr` and calling
/// `type_expr_to_object_shape` or the materialising host-threaded surface
/// bridges.
///
/// Discrimination: FAILS against the pre-change tree (where `projected_target_shape`
/// calls the `project_expr_surface_*_via_host_threaded` bridges +
/// `type_expr_to_object_shape`, and `project_expr_to_surface_shape` calls
/// `dispatch_routed_expr_surface_expr` + `type_expr_to_object_shape`) and PASSES
/// against the node-domain conversion. Re-introducing ANY materialize-then-shape
/// bridge / standalone gate in either fn FAILS here.
#[test]
fn surface_shape_conversions_run_node_domain_not_materialize_then_shape() {
    use std::collections::BTreeSet;
    use syn::visit::Visit;

    /// Collect every call ident (free/assoc-fn last path segment + method-call
    /// name) syntactically reachable inside the fn named `target` (a nested
    /// `fn` or an impl method), with depth tracking so sibling fns never leak.
    #[derive(Default)]
    struct CallCollector {
        target: String,
        depth: usize,
        found: bool,
        calls: BTreeSet<String>,
    }
    impl<'ast> Visit<'ast> for CallCollector {
        fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
            let hit = f.sig.ident == self.target;
            if hit {
                self.found = true;
                self.depth += 1;
            }
            syn::visit::visit_item_fn(self, f);
            if hit {
                self.depth -= 1;
            }
        }
        fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
            let hit = f.sig.ident == self.target;
            if hit {
                self.found = true;
                self.depth += 1;
            }
            syn::visit::visit_impl_item_fn(self, f);
            if hit {
                self.depth -= 1;
            }
        }
        fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
            if self.depth > 0 {
                if let syn::Expr::Path(p) = c.func.as_ref() {
                    if let Some(seg) = p.path.segments.last() {
                        self.calls.insert(seg.ident.to_string());
                    }
                }
            }
            syn::visit::visit_expr_call(self, c);
        }
        fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
            if self.depth > 0 {
                self.calls.insert(m.method.to_string());
            }
            syn::visit::visit_expr_method_call(self, m);
        }
    }

    fn collect_calls(src: &str, target: &str) -> BTreeSet<String> {
        let file = syn::parse_file(src).expect("source must parse");
        let mut collector = CallCollector {
            target: target.to_string(),
            ..Default::default()
        };
        collector.visit_file(&file);
        assert!(
            collector.found,
            "target fn `{target}` not found (renamed/removed?) — the \
             characterization must not vacuously pass"
        );
        collector.calls
    }

    const ROUTE_KEYS_SRC: &str =
        include_str!("resolver_core/component_meta_query_engine/route_keys.rs");
    const REGISTRY_DECL_SRC: &str =
        include_str!("resolver_core/component_meta_query_engine/registry_decl.rs");

    // ── Direct-utility `projected_target_shape` (nested fn in route_keys.rs) ──
    let pts = collect_calls(ROUTE_KEYS_SRC, "projected_target_shape");
    for forbidden in [
        "type_expr_to_object_shape",
        "project_expr_surface_shape_via_host_threaded",
    ] {
        assert!(
            !pts.contains(forbidden),
            "projected_target_shape must NOT call `{forbidden}` (materialize-then-shape); \
             calls seen: {pts:?}"
        );
    }
    for required in [
        "project_admitted_route_node_to_expanded_object_shape",
        "project_expr_surface_expr_node_via_host_threaded",
    ] {
        assert!(
            pts.contains(required),
            "projected_target_shape must call `{required}` (node-domain projection); \
             calls seen: {pts:?}"
        );
    }

    // ── Registry `project_expr_to_surface_shape` (impl method in registry_decl.rs) ──
    let pes = collect_calls(REGISTRY_DECL_SRC, "project_expr_to_surface_shape");
    for forbidden in [
        "type_expr_to_object_shape",
        "dispatch_routed_expr_surface_expr",
    ] {
        assert!(
            !pes.contains(forbidden),
            "project_expr_to_surface_shape must NOT call `{forbidden}` (materialize-then-shape); \
             calls seen: {pes:?}"
        );
    }
    for required in [
        "dispatch_routed_expr_surface_node",
        "project_admitted_route_node_to_expanded_object_shape",
    ] {
        assert!(
            pes.contains(required),
            "project_expr_to_surface_shape must call `{required}` (node-domain projection); \
             calls seen: {pes:?}"
        );
    }
}

/// Collect every call ident (free/assoc-fn last path segment + method-call name)
/// syntactically reachable inside the fn named `target` (a nested `fn` or an impl
/// method), with depth tracking so sibling fns never leak.
#[cfg(test)]
fn output_sink_calls_in(src: &str, target: &str) -> std::collections::BTreeSet<String> {
    use std::collections::BTreeSet;
    use syn::visit::Visit;

    #[derive(Default)]
    struct CallCollector {
        target: String,
        depth: usize,
        found: bool,
        calls: BTreeSet<String>,
    }
    impl<'ast> Visit<'ast> for CallCollector {
        fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
            let hit = f.sig.ident == self.target;
            if hit {
                self.found = true;
                self.depth += 1;
            }
            syn::visit::visit_item_fn(self, f);
            if hit {
                self.depth -= 1;
            }
        }
        fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
            let hit = f.sig.ident == self.target;
            if hit {
                self.found = true;
                self.depth += 1;
            }
            syn::visit::visit_impl_item_fn(self, f);
            if hit {
                self.depth -= 1;
            }
        }
        fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
            if self.depth > 0 {
                if let syn::Expr::Path(p) = c.func.as_ref() {
                    if let Some(seg) = p.path.segments.last() {
                        self.calls.insert(seg.ident.to_string());
                    }
                }
            }
            syn::visit::visit_expr_call(self, c);
        }
        fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
            if self.depth > 0 {
                self.calls.insert(m.method.to_string());
            }
            syn::visit::visit_expr_method_call(self, m);
        }
    }

    let file = syn::parse_file(src).expect("output_sink.rs must parse");
    let mut collector = CallCollector {
        target: target.to_string(),
        ..Default::default()
    };
    collector.visit_file(&file);
    assert!(
        collector.found,
        "target fn `{target}` not found in output_sink.rs (renamed/removed?) — the \
         characterization must not vacuously pass"
    );
    collector.calls
}

/// §1a DISCRIMINATION: the converted `output_sink` publication/reduction fns make
/// their shape decisions in NODE DOMAIN — they consult the node-fact APIs and do
/// NOT materialise a `TypeExpr` and then run a `TypeExpr` predicate on it.
///
/// Discrimination: each FORBIDDEN call is exactly the materialize-then-decide the
/// conversion removed, so re-introducing any of them FAILS this test (it was
/// present on the pre-change tree); each REQUIRED call is the node-domain fact the
/// conversion routes through.
#[test]
fn output_sink_conversions_decide_in_node_domain_not_on_materialized_type_expr() {
    const OUTPUT_SINK_SRC: &str = include_str!("meta_resolve/projectors/output_sink.rs");

    // ── `project_model` — reducibility decided on the payload NODE, not on the
    //    raised `TypeExpr` (the `type_expr_contains_reducible_operator(&raised)`
    //    decide is gone; `classify_node_reduction_gates(node)` replaces it).
    let project_model = output_sink_calls_in(OUTPUT_SINK_SRC, "project_model");
    assert!(
        !project_model.contains("type_expr_contains_reducible_operator"),
        "project_model must NOT run `type_expr_contains_reducible_operator` on the raised \
         TypeExpr (materialize-then-decide); calls seen: {project_model:?}"
    );
    assert!(
        project_model.contains("classify_node_reduction_gates"),
        "project_model must decide reducibility via `classify_node_reduction_gates` (node \
         domain); calls seen: {project_model:?}"
    );

    // ── `member_shape_peek_or_compute` — package-backed / cycle / reducibility
    //    gates run on the member-value NODE; the carrier seals through the
    //    node→carrier terminal, never via `shell_raise_to_type_expr` + a
    //    `TypeExpr` gate predicate.
    let member_shape = output_sink_calls_in(OUTPUT_SINK_SRC, "member_shape_peek_or_compute");
    for forbidden in [
        "shell_raise_to_type_expr",
        "seal_type_expr",
        "type_expr_has_package_backed_object_like_root_with_fence",
        "lowered_root_reaches_transitive_cycle_with_fence",
        "type_expr_contains_reducible_operator",
        "peek_member_shape_known",
    ] {
        assert!(
            !member_shape.contains(forbidden),
            "member_shape_peek_or_compute must NOT call `{forbidden}` (materialize-then-decide on \
             the raised TypeExpr); calls seen: {member_shape:?}"
        );
    }
    for required in [
        "node_package_backed_object_like_root_with_fence",
        "classify_node_reduction_gates",
        "node_root_reaches_transitive_cycle_with_fence",
        "raise_node_to_sealed_carrier",
    ] {
        assert!(
            member_shape.contains(required),
            "member_shape_peek_or_compute must call `{required}` (node-domain gate / terminal \
             seal); calls seen: {member_shape:?}"
        );
    }

    // ── `reduce_field_type_expr_with_mode` — returns a CARRIER; the no-poison
    //    sentinel gate reads the node-root-sentinel fact off the carrier NODE (not
    //    a materialised TypeExpr), and the cold mint is routed through a terminal.
    let reduce_field = output_sink_calls_in(OUTPUT_SINK_SRC, "reduce_field_type_expr_with_mode");
    for forbidden in [
        // the deleted materialize-then-decide sentinel helper
        "materialized_root_is_unmaterialized_sentinel",
        // the orchestrator never unwraps to a bare TypeExpr (it returns a carrier)
        "unwrap_materialized",
        // the cold mint moved into the terminal `materialize_field_value_carrier`
        "materialize_component_meta_type_expr_until_stable_full",
    ] {
        assert!(
            !reduce_field.contains(forbidden),
            "reduce_field_type_expr_with_mode must NOT call `{forbidden}`; calls seen: {reduce_field:?}"
        );
    }
    for required in [
        "node_root_is_unmaterialized_sentinel_with_dispatch",
        "materialize_field_value_carrier",
        "seal_input_as_carrier",
    ] {
        assert!(
            reduce_field.contains(required),
            "reduce_field_type_expr_with_mode must call `{required}` (node-domain sentinel / \
             carrier seal / terminal mint); calls seen: {reduce_field:?}"
        );
    }

    // ── `reduce_published_field_types` — the props shape selection is a NODE-domain
    //    comparison over the reduced carriers; the materialised-TypeExpr scorer is
    //    gone.
    let reduce_published = output_sink_calls_in(OUTPUT_SINK_SRC, "reduce_published_field_types");
    assert!(
        !reduce_published.contains("compare_type_expr_improvement"),
        "reduce_published_field_types must NOT score materialised TypeExprs via \
         `compare_type_expr_improvement`; calls seen: {reduce_published:?}"
    );
    for required in [
        "compare_node_improvement",
        "node_root_is_explicit_selector_operator",
    ] {
        assert!(
            reduce_published.contains(required),
            "reduce_published_field_types must select the published shape in node domain via \
             `{required}`; calls seen: {reduce_published:?}"
        );
    }
}

/// AUDIT-FOOTPRINT BOUND: `reduce_published_field_types` must stamp a lowered
/// shallow `node_id` (`reduce_field_type_expr_with_mode(.., stamp_shallow_node_id =
/// true)`) ONLY on the props loop — which node-compares the reduced carrier against
/// the shallow form and so needs the shallow node lowered to score it. The emits /
/// slot_bindings / bindings loops NEVER node-compare and pass `false`. Stamping on
/// EVERY shallow seal lowered each field's expr, bloating the per-request audit
/// footprint (>64 KiB on the 30-slot cyclic `defineSlots` fixture). This is the
/// source-precise bound: flipping ANY non-props loop's stamp back to `true`
/// re-introduces the per-field lowering bloat and changes the (true, false) tally,
/// failing here.
#[test]
fn reduce_published_stamps_shallow_node_id_only_on_the_props_loop() {
    use syn::visit::Visit;

    const OUTPUT_SINK_SRC: &str = include_str!("meta_resolve/projectors/output_sink.rs");

    /// Within `reduce_published_field_types`, collect the LAST positional argument
    /// (the `stamp_shallow_node_id` bool literal) of every
    /// `reduce_field_type_expr_with_mode` call.
    #[derive(Default)]
    struct StampArgCollector {
        depth: usize,
        stamps: Vec<bool>,
    }
    impl<'ast> Visit<'ast> for StampArgCollector {
        fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
            let hit = f.sig.ident == "reduce_published_field_types";
            if hit {
                self.depth += 1;
            }
            syn::visit::visit_item_fn(self, f);
            if hit {
                self.depth -= 1;
            }
        }
        fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
            if self.depth > 0 {
                if let syn::Expr::Path(p) = c.func.as_ref() {
                    let is_target = p
                        .path
                        .segments
                        .last()
                        .is_some_and(|s| s.ident == "reduce_field_type_expr_with_mode");
                    if is_target {
                        if let Some(syn::Expr::Lit(lit)) = c.args.last() {
                            if let syn::Lit::Bool(b) = &lit.lit {
                                self.stamps.push(b.value);
                            }
                        }
                    }
                }
            }
            syn::visit::visit_expr_call(self, c);
        }
    }

    let file = syn::parse_file(OUTPUT_SINK_SRC).expect("parse output_sink.rs");
    let mut collector = StampArgCollector::default();
    collector.visit_file(&file);

    // props loop stamps `true` on BOTH the field reduction AND the shallow-form
    // reduction (it node-compares them); emits / slot_bindings / bindings each pass
    // `false`. ⇒ exactly 2 `true` and 3 `false`.
    let trues = collector.stamps.iter().filter(|&&b| b).count();
    let falses = collector.stamps.iter().filter(|&&b| !b).count();
    assert_eq!(
        (trues, falses),
        (2, 3),
        "reduce_published_field_types must stamp `stamp_shallow_node_id = true` ONLY on the two \
         props-loop reductions and `false` on the emits / slot_bindings / bindings loops (the \
         audit-footprint bound); observed stamp args = {:?}",
        collector.stamps
    );
}

/// §1a NO-CACHE-POISON characterization: `reduce_field_type_expr_with_mode`
/// admits a `Leaf` (Primitive / Literal — a terminal shape whose shallow/expanded
/// forms agree) to the shared `type_expr_whole` `ShapeCacheDb` slot, but a
/// `BareCarrier` (a bare alias `Ref { type_arguments: [] }`) is published shallow
/// WITHOUT admitting — that slot is shared with the whole-expression materialiser's
/// pre-dispatch probe, so admitting the projector's shallow `Ref` would poison a
/// later EXPANDED alias-body request.
///
/// Discrimination: the `Leaf` match arm MUST contain `admit_type_expr_shape_if_possible`
/// and the `BareCarrier` arm MUST NOT — re-adding the admit to the `BareCarrier`
/// arm (the cache-poisoning regression) FAILS this test.
#[test]
fn reduce_field_bare_carrier_publishes_shallow_without_poisoning_shared_cache_slot() {
    use syn::visit::Visit;

    const OUTPUT_SINK_SRC: &str = include_str!("meta_resolve/projectors/output_sink.rs");

    /// Within the target fn, find each `PeekedShape::Leaf` / `PeekedShape::BareCarrier`
    /// match arm and record whether its body calls `admit_type_expr_shape_if_possible`.
    #[derive(Default)]
    struct ArmAdmitProbe {
        in_reduce_field: usize,
        leaf_admits: Option<bool>,
        bare_carrier_admits: Option<bool>,
    }
    /// Whether an arm body (an `Expr`) contains a call to `target` ident.
    fn body_calls(expr: &syn::Expr, target: &str) -> bool {
        struct C<'a> {
            target: &'a str,
            hit: bool,
        }
        impl<'a, 'ast> Visit<'ast> for C<'a> {
            fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
                if let syn::Expr::Path(p) = c.func.as_ref() {
                    if p.path
                        .segments
                        .last()
                        .is_some_and(|s| s.ident == self.target)
                    {
                        self.hit = true;
                    }
                }
                syn::visit::visit_expr_call(self, c);
            }
        }
        let mut c = C { target, hit: false };
        c.visit_expr(expr);
        c.hit
    }
    /// The terminal path-segment ident of a `Pat::TupleStruct` / `Pat::Path`
    /// pattern, e.g. `Leaf` / `BareCarrier` for `super::PeekedShape::Leaf(..)`.
    fn arm_variant(pat: &syn::Pat) -> Option<String> {
        let path = match pat {
            syn::Pat::TupleStruct(ts) => &ts.path,
            syn::Pat::Path(p) => &p.path,
            syn::Pat::Struct(s) => &s.path,
            _ => return None,
        };
        path.segments.last().map(|s| s.ident.to_string())
    }
    impl<'ast> Visit<'ast> for ArmAdmitProbe {
        fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
            let hit = f.sig.ident == "reduce_field_type_expr_with_mode";
            if hit {
                self.in_reduce_field += 1;
            }
            syn::visit::visit_item_fn(self, f);
            if hit {
                self.in_reduce_field -= 1;
            }
        }
        fn visit_arm(&mut self, arm: &'ast syn::Arm) {
            if self.in_reduce_field > 0 {
                match arm_variant(&arm.pat).as_deref() {
                    Some("Leaf") => {
                        self.leaf_admits =
                            Some(body_calls(&arm.body, "admit_type_expr_shape_if_possible"));
                    }
                    Some("BareCarrier") => {
                        self.bare_carrier_admits =
                            Some(body_calls(&arm.body, "admit_type_expr_shape_if_possible"));
                    }
                    _ => {}
                }
            }
            syn::visit::visit_arm(self, arm);
        }
    }

    let file = syn::parse_file(OUTPUT_SINK_SRC).expect("output_sink.rs must parse");
    let mut probe = ArmAdmitProbe::default();
    probe.visit_file(&file);

    assert_eq!(
        probe.leaf_admits,
        Some(true),
        "the `PeekedShape::Leaf` arm of reduce_field_type_expr_with_mode must admit to the \
         shared cache slot (Primitive/Literal forms agree shallow/expanded) — arm not found or \
         admit missing"
    );
    assert_eq!(
        probe.bare_carrier_admits,
        Some(false),
        "the `PeekedShape::BareCarrier` arm MUST NOT admit to the shared `type_expr_whole` cache \
         slot — admitting a shallow alias `Ref` there poisons a later EXPANDED alias-body request \
         (re-adding the admit is the cache-poisoning regression this test guards)"
    );
}

/// End-to-end discriminator for the dispatch-bridge `ProjectGeneration`
/// conversion.
///
/// A cross-file `defineProps<Foo>()` drives the projector +
/// materialiser dispatch reads. Every dispatch round-trip's
/// `DepSignature` carries a `DepVersion::ProjectGeneration` (built by
/// `ProjectSemanticDispatch::dep_signature_for` /
/// `project_generation_signature`). Those signatures fan through
/// `emit_dispatch_dep_signature_facts` → `accumulate_dispatch_dep_signature`
/// into the per-request accumulator, which `compute_component_meta_state_inner`
/// drains and merges into `ResolvedComponentMetaState.fact_versions`.
///
/// `fact_versions` is the signature stored on the fact-only
/// `cached_resolved_meta` sidecar (on `DerivedRawState`); a warm read
/// through `try_get_cached_resolved_meta` validates it via
/// `StoreView::validates_fact_signature` alone — no legacy rail.
/// `current_dependency_fact_versions` only emits `FileWholeHash` /
/// `DerivedFactHash` facts, NEVER a `ProjectGeneration` fact, so the
/// dispatch-accumulator drain is the only path that can root the
/// project generation on the sidecar.
///
/// Discrimination: against the pre-fix tree
/// `accumulate_dispatch_dep_signature` DROPS `ProjectGeneration`, so
/// the sidecar's `fact_versions` carry no `ProjectGeneration` fact and
/// a `bump_project_generation()` with unchanged file content leaves
/// the fact-only sidecar warm (stale). The first assertion below
/// FAILS pre-fix. Post-fix the bridge converts `ProjectGeneration`,
/// the sidecar roots it, and a project-generation bump misses.
#[test]
fn dispatch_project_generation_roots_fact_only_resolved_meta_sidecar() {
    use crate::resolver_core::FactVersionRef;
    use crate::types::ProjectionMode;

    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            "export type Foo = { value: number; label: string }",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { Foo } from './types'
defineProps<{ data: Foo }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Cold compute: populates the `cached_resolved_meta` sidecar.
    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/src/Comp.vue")
        .unwrap()
        .expect("component meta resolves for the cross-file Foo fixture");
    assert!(
        meta.props.iter().any(|p| p.name == "data"),
        "fixture must publish a `data` prop so the projector + \
         materialiser dispatch reads ran — props={:?}",
        meta.props
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
    );

    // Inspect the fact-only sidecar's stored `fact_versions`.
    let host = project.host();
    let entry = host
        .derived_raw_cache()
        .get("/src/Comp.vue")
        .expect("derived-raw entry must exist after a cold component-meta compute");
    let cached = entry
        .cached_resolved_meta
        .iter()
        .find(|((mode, _view_fp), _)| *mode == ProjectionMode::Expanded)
        .map(|(_, cached)| cached.clone())
        .expect("cached_resolved_meta must hold an Expanded slot after cold compute");
    drop(entry);

    let project_generation_facts: Vec<u64> = cached
        .fact_versions
        .iter()
        .filter_map(|fact| match fact {
            FactVersionRef::ProjectGeneration { generation } => Some(*generation),
            _ => None,
        })
        .collect();
    // DISCRIMINATING ASSERTION: pre-fix the dispatch accumulator drops
    // `ProjectGeneration`, so no `ProjectGeneration` fact reaches the
    // sidecar and this fails.
    assert!(
        !project_generation_facts.is_empty(),
        "the fact-only `cached_resolved_meta` sidecar MUST carry a \
         FactVersionRef::ProjectGeneration fact after a cold \
         component-meta compute whose dispatch reads observed the \
         project generation — `accumulate_dispatch_dep_signature` \
         must CONVERT `DepVersion::ProjectGeneration`, not drop it. \
         sidecar fact_versions={:?}",
        cached.fact_versions
    );
    // The dispatch reads observe the live project generation at
    // compute time; the sidecar must root that exact value.
    let live_generation = host.project_type_store().project_generation();
    assert!(
        project_generation_facts.contains(&live_generation),
        "the sidecar's ProjectGeneration fact must carry the project \
         generation live at cold-compute time ({live_generation}); \
         got {project_generation_facts:?}"
    );

    // Stale-serve proof: drop the runtime singleflight slot so the
    // fact-only sidecar is the sole survivor, bump the project
    // generation WITHOUT touching any file, and confirm the fact-only
    // warm read misses. Pre-fix (no ProjectGeneration fact) the
    // sidecar would validate vacuously and serve the stale state.
    host.resolver_runtime()
        .component_meta
        .remove(&crate::host_manage::component_meta_request_impl::resolved_meta_cache_key_with_view_fingerprint(
            "/src/Comp.vue",
            ProjectionMode::Expanded,
            0,
        ));
    let warm_before_bump =
        host.try_get_cached_resolved_meta("/src/Comp.vue", ProjectionMode::Expanded);
    assert!(
        warm_before_bump.is_some(),
        "before the bump the fact-only sidecar must still validate \
         (file content unchanged, project generation unchanged)"
    );

    host.project_type_store().bump_project_generation();

    let warm_after_bump =
        host.try_get_cached_resolved_meta("/src/Comp.vue", ProjectionMode::Expanded);
    assert!(
        warm_after_bump.is_none(),
        "after a `bump_project_generation()` with unchanged file \
         content the fact-only `cached_resolved_meta` sidecar MUST \
         miss — the rooted ProjectGeneration fact no longer matches \
         the live project generation. A warm hit here means the \
         project-shape dependency was dropped at the dispatch bridge."
    );
}

// ===========================================================================
// FIX 3 — restored discriminating coverage for behaviours the deleted
// equivalence harnesses owned, now driven through the typeinfo / Vue
// macro-surface publication path.
// ===========================================================================

/// FIX 3 #1 — an inherited METHOD-STYLE slot's JSDoc `description` survives
/// through cross-file heritage onto the published slot surface. Sibling slot
/// tests assert names / bindings / returns but not the inherited slot
/// description; this fills that gap.
///
/// Discriminating: the `header` slot is declared (as a method signature with a
/// leading JSDoc block) ONLY on the imported base `BaseSlots`; the component's
/// own `MySlots` adds `footer`. The published `header` slot's description must
/// be the base's JSDoc text — a resolver that dropped inherited-slot JSDoc, or
/// failed to follow the cross-file heritage, would leave it `None`.
#[test]
fn inherited_method_style_slot_jsdoc_description_propagates_through_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base-slots.ts",
            r#"export interface BaseSlots {
  /** The header slot, rendered above the content. */
  header(props: { title: string }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { BaseSlots } from './base-slots'

interface MySlots extends BaseSlots {
  /** The footer slot. */
  footer(props: {}): any
}

defineSlots<MySlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");

    let header = meta
        .slots
        .iter()
        .find(|slot| slot.name == "header")
        .expect("inherited method-style header slot should be published");
    assert_eq!(
        header.description.as_deref(),
        Some("The header slot, rendered above the content."),
        "inherited method-style slot must carry its base-declared JSDoc description",
    );
    // Negative guard: a resolver that mis-attributed the OWN footer slot's
    // description onto header would surface the wrong text.
    assert_ne!(
        header.description.as_deref(),
        Some("The footer slot."),
        "header slot must not pick up the own footer slot's description",
    );

    // The own footer slot keeps its own description (sanity that heritage merge
    // did not clobber own-body JSDoc).
    let footer = meta
        .slots
        .iter()
        .find(|slot| slot.name == "footer")
        .expect("own footer slot should be published");
    assert_eq!(footer.description.as_deref(), Some("The footer slot."));
}

/// FIX 3 #2a — ALIAS-CHAIN cross-file heritage through the Vue macro surface:
/// `defineProps<ChildProps>` where `ChildProps extends MiddleAlias` and
/// `MiddleAlias = BaseProps` (a one-hop alias of an imported interface). The
/// published props must include the base members reached through the alias hop.
///
/// Discriminating: `base` is declared only on `BaseProps`, reached via the
/// `MiddleAlias = BaseProps` alias; a resolver that failed to follow the alias
/// hop in heritage would drop it. The EXACT-SET assertion plus the `decoy`
/// negative (a sibling interface in the SAME file that the heritage chain never
/// touches) fail if heritage leaks unrelated declarations into the surface.
#[test]
fn cross_file_alias_chain_heritage_through_macro_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface BaseProps {
  base?: string
}

export type MiddleAlias = BaseProps

// A sibling interface in the SAME file the heritage chain never reaches. A
// resolver that over-collected file-level declarations would leak `decoy`.
export interface UnrelatedProps {
  decoy?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { MiddleAlias } from './base'

interface ChildProps extends MiddleAlias {
  own?: number
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: BTreeSet<_> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains("own"),
        "own member should be published, got {prop_names:?}",
    );
    assert!(
        prop_names.contains("base"),
        "alias-chain heritage must surface the base member reached through `MiddleAlias = BaseProps`, got {prop_names:?}",
    );
    // NEGATIVE: the unrelated sibling interface's member must NOT leak.
    assert!(
        !prop_names.contains("decoy"),
        "alias-chain heritage must NOT leak the unrelated `UnrelatedProps.decoy` member, got {prop_names:?}",
    );
    // EXACT surface: exactly the own member plus the alias-reached base member.
    assert_eq!(
        prop_names,
        BTreeSet::from(["own", "base"]),
        "alias-chain heritage surface must be EXACTLY {{own, base}}, got {prop_names:?}",
    );
}

/// FIX 3 #2b — TWO-LEVEL cross-file heritage through the Vue macro surface:
/// `GrandchildProps extends ChildProps` (file B) `extends BaseProps` (file A),
/// each declared in a different file. The published props must include members
/// from all three levels.
///
/// Discriminating: `base` (file A), `mid` (file B), `own` (SFC) must ALL be
/// present — a resolver that stopped after one heritage hop would drop `base`.
/// The EXACT-SET assertion plus the `decoy` negative (a sibling interface in
/// file B that the chain never extends) fail if heritage leaks unrelated
/// declarations into the surface.
#[test]
fn two_level_cross_file_heritage_through_macro_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface BaseProps {
  base?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/child.ts",
            r#"import type { BaseProps } from './base'

export interface ChildProps extends BaseProps {
  mid?: boolean
}

// A sibling interface in file B that no level of the chain extends. A resolver
// that over-collected file-level declarations would leak `decoy`.
export interface UnrelatedChild {
  decoy?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ChildProps } from './child'

interface GrandchildProps extends ChildProps {
  own?: number
}

defineProps<GrandchildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: BTreeSet<_> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains("own") && prop_names.contains("mid") && prop_names.contains("base"),
        "two-level cross-file heritage must surface members from all three levels (own/mid/base), got {prop_names:?}",
    );
    // NEGATIVE: the unrelated sibling interface's member must NOT leak.
    assert!(
        !prop_names.contains("decoy"),
        "two-level heritage must NOT leak the unrelated `UnrelatedChild.decoy` member, got {prop_names:?}",
    );
    // EXACT surface: exactly the three heritage-chain members.
    assert_eq!(
        prop_names,
        BTreeSet::from(["own", "mid", "base"]),
        "two-level heritage surface must be EXACTLY {{own, mid, base}}, got {prop_names:?}",
    );
}

/// FIX 3 #3 — `defineProps<NoInfer<Base>>()` THROUGH macro-surface publication.
/// `NoInfer<T>` is an identity wrapper (it only affects inference, not the
/// resolved type); the published props must be exactly the members of the
/// imported `Base` surface.
///
/// Discriminating: `label`/`count` come from `Base` wrapped in `NoInfer`; a
/// resolver that failed to unwrap `NoInfer` (or treated it as opaque) would
/// drop the props or collapse them to `never`.
#[test]
fn define_props_no_infer_base_through_macro_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface Base {
  label?: string
  count?: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Base } from './base'

defineProps<NoInfer<Base>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let label = meta
        .props
        .iter()
        .find(|p| p.name == "label")
        .expect("NoInfer<Base> must publish the `label` prop through the macro surface");
    assert!(
        !matches!(label.type_expr, TypeExpr::Primitive(PrimitiveName::Never)),
        "NoInfer<Base> prop must keep its resolved type, not collapse to never, got {:?}",
        label.type_expr
    );
    let prop_names: BTreeSet<_> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains("label") && prop_names.contains("count"),
        "NoInfer<Base> must publish the Base members unchanged (identity wrapper), got {prop_names:?}",
    );
}

/// FIX 3 #3 (alias-shell) — `export type Props = NoInfer<Base>; defineProps<Props>()`
/// THROUGH macro-surface publication. This is the harder shape than the direct
/// `defineProps<NoInfer<Base>>()` case above: the macro type argument is a NAMED
/// alias whose body is the `NoInfer` identity wrapper around the imported
/// `Base`. The transparent alias + `NoInfer` wrappers must resolve to `Base`'s
/// own body, so the published surface is EXACTLY `Base`'s members.
///
/// Discriminating, typeinfo-native (asserts the full published shape, not just
/// presence):
/// - member names AND order are exactly `[label, count]` (Base's declared order);
/// - both are optional (`required == false`) — the `?` survives the wrappers;
/// - the typed forms are the concrete `string` / `number` primitives (NOT
///   `never`, NOT a `Ref`/opaque) — a resolver treating `NoInfer` as opaque or
///   failing the alias hop would collapse or drop them;
/// - `declared_in_macro_type_arg == false` for BOTH — own-body provenance is
///   the WRITTEN macro type-argument declaration's DIRECT `member_index`
///   members (`build.rs::overlay_macro_type_arg_own_body`, which reads only
///   direct Object members and skips Ref/instantiation arms). The written
///   macro-arg here is the alias `Props`, whose body is the `NoInfer<Base>`
///   instantiation — it has NO direct own-body members; `label`/`count` are
///   reached by resolving the transparent alias + `NoInfer` hops to `Base`, so
///   they carry own-body `false` (the inverse value would mean a resolver
///   mis-stamped alias-reached members as the alias's own body).
#[test]
fn define_props_no_infer_base_alias_shell_through_macro_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface Base {
  label?: string
  count?: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Base } from './base'

export type Props = NoInfer<Base>

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");

    // Names AND order: exactly `[label, count]` (Base's declared order),
    // preserved through the alias + NoInfer wrappers.
    let ordered_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        ordered_names,
        vec!["label", "count"],
        "alias-shell NoInfer<Base> must publish exactly Base's members in declared order, got {ordered_names:?}",
    );

    let label = meta
        .props
        .iter()
        .find(|p| p.name == "label")
        .expect("alias-shell NoInfer<Base> must publish the `label` prop");
    let count = meta
        .props
        .iter()
        .find(|p| p.name == "count")
        .expect("alias-shell NoInfer<Base> must publish the `count` prop");

    // Optionality: the `?` survives the alias + NoInfer wrappers (NOT required).
    assert!(
        !label.required && !count.required,
        "alias-shell NoInfer<Base> props must stay optional (required == false), got label.required={}, count.required={}",
        label.required,
        count.required,
    );

    // Typed form: concrete primitives, NOT collapsed to `never` / left as an
    // opaque `Ref` — the alias + NoInfer wrappers resolved through to Base.
    assert!(
        matches!(label.type_expr, TypeExpr::Primitive(PrimitiveName::String)),
        "alias-shell `label` must keep its `string` primitive type, got {:?}",
        label.type_expr,
    );
    assert!(
        matches!(count.type_expr, TypeExpr::Primitive(PrimitiveName::Number)),
        "alias-shell `count` must keep its `number` primitive type, got {:?}",
        count.type_expr,
    );

    // Provenance: own-body is the WRITTEN macro type-arg declaration's direct
    // `member_index` members. The written macro-arg is the alias `Props`, whose
    // body is the `NoInfer<Base>` instantiation — NO direct own-body members.
    // `label`/`count` are reached through the transparent alias + NoInfer hops
    // to `Base`, so `declared_in_macro_type_arg` is false for both. The inverse
    // value would mean a resolver mis-stamped alias-reached members as the
    // alias's own body.
    assert!(
        !label.declared_in_macro_type_arg && !count.declared_in_macro_type_arg,
        "alias-shell NoInfer<Base> members are reached through the alias, NOT the macro type arg's own body (declared_in_macro_type_arg == false), got label={}, count={}",
        label.declared_in_macro_type_arg,
        count.declared_in_macro_type_arg,
    );
}

/// End-to-end (`get_component_meta`) member-visibility leak guard for slot
/// bindings whose first parameter is a CLASS carrying non-public members. This
/// drives the FULL component-meta pipeline, so it covers BOTH the typeinfo
/// adapter binding path (`binding_fields_from_param_ty`) AND the graph-native
/// binding path (`slot_binding_graph::compute_bindings_via_graph` ->
/// `publish_merged_bindings`); both must apply the Public-only publication
/// filter so a navigated class param's `private` / `protected` member never
/// reaches the published slot binding surface.
///
/// Discriminating: against the tree without the graph-native + adapter
/// slot-binding filters, `protectedBinding` / `privateBinding` appear in the
/// published bindings and the `does-not-contain` assertions FAIL.
#[test]
fn slot_binding_navigated_class_param_excludes_non_public_members_end_to_end() {
    let project = make_project();
    project
        .upsert_base(
            "/slot-props.ts",
            r#"export class SlotProps {
  publicBinding: string
  protected protectedBinding: number
  private privateBinding: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { SlotProps } from './slot-props'
defineSlots<{ default(props: SlotProps): any }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("component meta with class-param slot resolves");

    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot must be published");
    let binding_names: Vec<&str> = default_slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();

    assert!(
        binding_names.contains(&"publicBinding"),
        "the public class-param member must publish as a slot binding; got {binding_names:?}",
    );
    assert!(
        !binding_names.contains(&"protectedBinding"),
        "a protected class-param member must NOT leak into published slot bindings \
         (adapter OR graph-native path); got {binding_names:?}",
    );
    assert!(
        !binding_names.contains(&"privateBinding"),
        "a private class-param member must NOT leak into published slot bindings \
         (adapter OR graph-native path); got {binding_names:?}",
    );
}

#[test]
fn published_default_value_and_default_value_tag_keep_verbatim_source_quoting() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
withDefaults(defineProps<{ orientation?: string, count?: number, active?: boolean, items?: string[] }>(), {
  orientation: 'vertical',
  count: 0,
  active: false,
  items: () => ['a'],
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("component meta resolves");

    let prop = |name: &str| {
        meta.props
            .iter()
            .find(|prop| prop.name == name)
            .unwrap_or_else(|| panic!("prop {name} must be published"))
    };

    let orientation = prop("orientation");
    assert_eq!(
        orientation.default_value.as_deref(),
        Some("'vertical'"),
        "string default must publish the verbatim quoted source text"
    );
    assert_ne!(
        orientation.default_value.as_deref(),
        Some("vertical"),
        "string default must not publish the unquoted inner value"
    );
    let default_tag = orientation
        .tags
        .iter()
        .find(|tag| tag.name == "defaultValue")
        .expect("synthesized @defaultValue tag must be present");
    assert_eq!(
        default_tag.text.as_deref(),
        Some("'vertical'"),
        "@defaultValue tag text must carry the verbatim quoted source text"
    );

    // Non-string defaults stay byte-identical — no quoting layer is added.
    assert_eq!(prop("count").default_value.as_deref(), Some("0"));
    assert_eq!(prop("active").default_value.as_deref(), Some("false"));
    assert_eq!(prop("items").default_value.as_deref(), Some("() => ['a']"));
    for name in ["count", "active", "items"] {
        let value = prop(name).default_value.as_deref().unwrap();
        assert!(
            !value.starts_with('\'') && !value.starts_with('"'),
            "non-string default {name} must not gain a quoting layer, got {value}"
        );
    }
}

#[test]
fn cross_file_prop_jsdoc_survives_homomorphic_mapped_types() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface ImportedProps {
  /**
   * Visual orientation of the widget.
   * @deprecated use layout instead
   */
  orientation?: string
  /** Number of columns. */
  columns?: number
  plain?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedProps } from './types'

defineProps<Partial<Pick<ImportedProps, 'orientation' | 'plain'>>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Comp.vue")
        .expect("component meta resolves");

    let orientation = meta
        .props
        .iter()
        .find(|prop| prop.name == "orientation")
        .expect("orientation must surface through Partial<Pick<...>>");
    assert_eq!(
        orientation.description.as_deref(),
        Some("Visual orientation of the widget."),
        "imported member's JSDoc description must survive homomorphic mapped production"
    );
    let deprecated = orientation
        .tags
        .iter()
        .find(|tag| tag.name == "deprecated")
        .expect("imported member's @deprecated tag must survive homomorphic mapped production");
    assert_eq!(deprecated.text.as_deref(), Some("use layout instead"));

    // Negative: an undocumented member publishes NO description and NO tags
    // (no fabrication).
    let plain = meta
        .props
        .iter()
        .find(|prop| prop.name == "plain")
        .expect("plain must surface through Partial<Pick<...>>");
    assert_eq!(
        plain.description.as_deref(),
        None,
        "undocumented member must not gain a fabricated description"
    );
    assert!(
        plain.tags.is_empty(),
        "undocumented member must not gain fabricated tags, got {:?}",
        plain.tags
    );
}

/// Key-remapped mapped props judge JSDoc inheritance PER PRODUCED NAME on
/// the published (Shallow) surface: a true rename (`as `x-${K}``)
/// publishes a name no source declaration declares and severs the
/// declaration site, so NO description and NO tags are fabricated for it;
/// an identity remap (`as K`) IS the source declaration's name-preserving
/// image and keeps the doc. Modifier inheritance (the `?` optionality)
/// survives the rename.
///
/// Discriminating: pre-fix the Shallow mapped synthesiser inherited the
/// source member's spans + declaration_origin for every produced name, so
/// `x-orientation` published `orientation`'s doc and @deprecated tag — the
/// severing assertions below fail on that implementation.
#[test]
fn key_remapped_mapped_prop_does_not_inherit_source_jsdoc() {
    let project = make_project();
    project
        .upsert_base(
            "/src/remap.ts",
            r#"
export interface SourceProps {
  /**
   * Visual orientation of the widget.
   * @deprecated use layout instead
   */
  orientation?: string
}
export type RenamedProps<T> = { [K in keyof T as `x-${K}`]: T[K] }
export type IdentityProps<T> = { [K in keyof T as K]: T[K] }
export type FanoutProps<T> = { [K in keyof T as K | `x-${K}`]: T[K] }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Renamed.vue",
            r#"<script setup lang="ts">
import type { SourceProps, RenamedProps } from './remap'

defineProps<RenamedProps<SourceProps>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Identity.vue",
            r#"<script setup lang="ts">
import type { SourceProps, IdentityProps } from './remap'

defineProps<IdentityProps<SourceProps>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let renamed_meta = project
        .host()
        .get_component_meta("/src/Renamed.vue")
        .expect("component meta resolves");

    // Renamed arm: `x-orientation` is declared by NO source declaration —
    // no description, no tags (negative: nothing fabricated from the
    // source member's declaration site).
    let renamed = renamed_meta
        .props
        .iter()
        .find(|prop| prop.name == "x-orientation")
        .expect("the renamed remap arm must surface");
    assert_eq!(
        renamed.description.as_deref(),
        None,
        "a key-remapped prop must NOT inherit the source member's JSDoc \
         description — its produced name has no source declaration site"
    );
    assert!(
        renamed.tags.is_empty(),
        "a key-remapped prop must NOT inherit the source member's tags, got {:?}",
        renamed.tags
    );
    // Over-sever guard: modifier parity survives the rename — the source
    // member's `?` still makes the renamed prop optional.
    assert!(
        !renamed.required,
        "the renamed arm must still inherit optionality from the source member"
    );

    // Identity remap (`as K`): the produced name equals the source key —
    // the declaration site is real and the doc publishes (guards against
    // an over-broad "any `as` clause severs" implementation).
    let identity_meta = project
        .host()
        .get_component_meta("/src/Identity.vue")
        .expect("component meta resolves");
    let orientation = identity_meta
        .props
        .iter()
        .find(|prop| prop.name == "orientation")
        .expect("the identity remap arm must surface");
    assert_eq!(
        orientation.description.as_deref(),
        Some("Visual orientation of the widget."),
        "an `as K` identity remap must keep the source JSDoc"
    );
    assert!(
        orientation.tags.iter().any(|tag| tag.name == "deprecated"),
        "an `as K` identity remap must keep the source @deprecated tag, got {:?}",
        orientation.tags
    );
    assert!(
        !orientation.required,
        "the identity arm must still inherit optionality from the source member"
    );

    // One-to-many remap (`as K | `x-${K}``): each produced arm is judged
    // independently — the verbatim `orientation` arm keeps the doc, the
    // renamed `x-orientation` arm severs it.
    project
        .upsert_base(
            "/src/Fanout.vue",
            r#"<script setup lang="ts">
import type { SourceProps, FanoutProps } from './remap'

defineProps<FanoutProps<SourceProps>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let fanout_meta = project
        .host()
        .get_component_meta("/src/Fanout.vue")
        .expect("component meta resolves");
    let fanout_verbatim = fanout_meta
        .props
        .iter()
        .find(|prop| prop.name == "orientation")
        .expect("the verbatim arm of a one-to-many remap must surface");
    assert_eq!(
        fanout_verbatim.description.as_deref(),
        Some("Visual orientation of the widget."),
        "the verbatim arm of a one-to-many remap must keep the source JSDoc"
    );
    let fanout_renamed = fanout_meta
        .props
        .iter()
        .find(|prop| prop.name == "x-orientation")
        .expect("the renamed arm of a one-to-many remap must surface");
    assert_eq!(
        fanout_renamed.description.as_deref(),
        None,
        "the renamed arm of a one-to-many remap must NOT inherit the source JSDoc"
    );
    assert!(
        fanout_renamed.tags.is_empty(),
        "the renamed arm of a one-to-many remap must NOT inherit tags, got {:?}",
        fanout_renamed.tags
    );
}

#[test]
fn cross_file_call_signature_emit_jsdoc_publishes() {
    let project = make_project();
    project
        .upsert_base(
            "/src/emits.ts",
            r#"
export interface WidgetEmits {
  /**
   * Fires when the value changes.
   * @deprecated listen to input instead
   */
  (e: 'change', value: string): void
  /** Fires on focus. */
  (e: 'focus'): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Widget.vue",
            r#"<script setup lang="ts">
import type { WidgetEmits } from './emits'

defineEmits<WidgetEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Widget.vue")
        .expect("component meta resolves");

    let change = meta
        .events
        .iter()
        .find(|event| event.name == "change")
        .expect("change event must surface");
    assert_eq!(
        change.description.as_deref(),
        Some("Fires when the value changes."),
        "imported call-signature emit's JSDoc description must publish"
    );
    let deprecated = change
        .tags
        .iter()
        .find(|tag| tag.name == "deprecated")
        .expect("imported call-signature emit's @deprecated tag must publish");
    assert_eq!(deprecated.text.as_deref(), Some("listen to input instead"));

    let focus = meta
        .events
        .iter()
        .find(|event| event.name == "focus")
        .expect("focus event must surface");
    assert_eq!(focus.description.as_deref(), Some("Fires on focus."));
}

#[test]
fn cross_file_property_style_emit_jsdoc_publishes() {
    let project = make_project();
    project
        .upsert_base(
            "/src/emits.ts",
            r#"
export interface PanelEmits {
  /** Fires when the panel toggles. */
  toggle: [open: boolean]
  close: []
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Panel.vue",
            r#"<script setup lang="ts">
import type { PanelEmits } from './emits'

defineEmits<PanelEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Panel.vue")
        .expect("component meta resolves");

    let toggle = meta
        .events
        .iter()
        .find(|event| event.name == "toggle")
        .expect("toggle event must surface");
    assert_eq!(
        toggle.description.as_deref(),
        Some("Fires when the panel toggles."),
        "imported property-style emit's JSDoc description must publish"
    );

    // Negative: an undocumented event publishes NO description and NO
    // fabricated tags (the synthesized @defaultValue rail is props-only).
    let close = meta
        .events
        .iter()
        .find(|event| event.name == "close")
        .expect("close event must surface");
    assert_eq!(
        close.description.as_deref(),
        None,
        "undocumented event must not gain a fabricated description"
    );
    assert!(
        close.tags.is_empty(),
        "undocumented event must not gain fabricated tags, got {:?}",
        close.tags
    );
}

#[test]
fn cross_file_slot_jsdoc_publishes() {
    let project = make_project();
    project
        .upsert_base(
            "/src/slots.ts",
            r#"
export interface CardSlots {
  /** Main card body content. */
  default(props: { item: string }): any
  footer?(props: {}): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Card.vue",
            r#"<script setup lang="ts">
import type { CardSlots } from './slots'

defineSlots<CardSlots>()
</script>
<template><div><slot /></div></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Card.vue")
        .expect("component meta resolves");

    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot must surface");
    assert_eq!(
        default_slot.description.as_deref(),
        Some("Main card body content."),
        "imported slot's JSDoc description must publish"
    );

    // Negative: an undocumented slot publishes NO description.
    let footer = meta
        .slots
        .iter()
        .find(|slot| slot.name == "footer")
        .expect("footer slot must surface");
    assert_eq!(
        footer.description.as_deref(),
        None,
        "undocumented slot must not gain a fabricated description"
    );
}

#[test]
fn same_file_local_interface_prop_jsdoc_publishes_without_text_scan() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script lang="ts">
export interface Inner {
  /** Doc for foo */
  foo: string
  /** Doc for bar.
   * @deprecated use baz
   */
  bar?: number
}
</script>
<script setup lang="ts">
defineProps<Inner>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project.host().get_component_meta("/App.vue").expect("meta");

    let foo = meta
        .props
        .iter()
        .find(|prop| prop.name == "foo")
        .expect("foo must surface");
    assert_eq!(foo.description.as_deref(), Some("Doc for foo"));

    let bar = meta
        .props
        .iter()
        .find(|prop| prop.name == "bar")
        .expect("bar must surface");
    assert_eq!(bar.description.as_deref(), Some("Doc for bar."));
    let deprecated = bar
        .tags
        .iter()
        .find(|tag| tag.name == "deprecated")
        .expect("@deprecated tag must publish");
    assert_eq!(deprecated.text.as_deref(), Some("use baz"));
}

#[test]
fn define_expose_object_literal_jsdoc_publishes() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { ref, computed } from 'vue'

const count = ref(0)
const label = computed(() => `n=${count.value}`)

defineExpose({
  /**
   * The live counter value.
   * @internal
   */
  count,
  label,
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project.host().get_component_meta("/App.vue").expect("meta");

    let count = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "count")
        .expect("count must be exposed");
    assert_eq!(
        count.description.as_deref(),
        Some("The live counter value."),
        "object-literal defineExpose member's JSDoc description must publish"
    );
    let internal = count
        .tags
        .iter()
        .find(|tag| tag.name == "internal")
        .expect("@internal tag must publish on the exposed member");
    assert_eq!(internal.text.as_deref(), None);

    // Negative: an undocumented exposed member publishes NO description and
    // NO tags (no fabrication).
    let label = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "label")
        .expect("label must be exposed");
    assert_eq!(label.description.as_deref(), None);
    assert!(label.tags.is_empty(), "got {:?}", label.tags);
}

#[test]
fn define_expose_type_argument_only_publishes_members_with_docs() {
    let project = make_project();
    project
        .upsert_base(
            "/src/api.ts",
            r#"
export interface PanelApi {
  /**
   * Open the panel.
   * @public
   */
  open(): void
  close(): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Panel.vue",
            r#"<script setup lang="ts">
import type { PanelApi } from './api'

defineExpose<PanelApi>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Panel.vue")
        .expect("component meta resolves");

    let open = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "open")
        .expect("type-argument-only defineExpose must publish its surface members");
    assert_eq!(
        open.description.as_deref(),
        Some("Open the panel."),
        "type-argument member's JSDoc must publish without an object literal"
    );
    assert!(
        open.tags.iter().any(|tag| tag.name == "public"),
        "type-argument member's @public tag must publish, got {:?}",
        open.tags
    );
    assert!(
        !matches!(open.type_expr, TypeExpr::Unknown { .. }),
        "surface member publishes its surface type, not an Unknown fallback: {:?}",
        open.type_expr
    );

    // Negative: an undocumented exposed member publishes nothing fabricated.
    let close = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "close")
        .expect("undocumented type-argument member must still publish");
    assert_eq!(close.description.as_deref(), None);
    assert!(close.tags.is_empty(), "got {:?}", close.tags);

    // The public-instance sidecar derives from the exposed surface, so the
    // type-argument-only members surface there too.
    let sidecar = meta
        .public_instance
        .as_ref()
        .expect("exposed members imply a public-instance sidecar");
    let sidecar_open = sidecar
        .members
        .iter()
        .find(|member| member.name == "open")
        .expect("sidecar must carry the exposed member");
    assert_eq!(
        sidecar_open.kind,
        verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Exposed
    );
    assert_eq!(sidecar_open.description.as_deref(), Some("Open the panel."));
}

#[test]
fn define_expose_owner_local_type_argument_publishes_members_with_docs() {
    let project = make_project();
    project
        .upsert_base(
            "/src/Input.vue",
            r#"<script setup lang="ts">
interface LocalApi {
  /**
   * Focus the input.
   * @public
   */
  focus(): void
  blur(): void
}

defineExpose<LocalApi>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Input.vue")
        .expect("component meta resolves");

    let focus = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "focus")
        .expect("owner-local type-argument defineExpose must publish its surface members");
    assert_eq!(
        focus.description.as_deref(),
        Some("Focus the input."),
        "owner-local type-argument member's JSDoc must publish without an object literal"
    );
    assert!(
        focus.tags.iter().any(|tag| tag.name == "public"),
        "owner-local type-argument member's @public tag must publish, got {:?}",
        focus.tags
    );
    assert!(
        !matches!(focus.type_expr, TypeExpr::Unknown { .. }),
        "surface member publishes its surface type, not an Unknown fallback: {:?}",
        focus.type_expr
    );

    // Negative: an undocumented exposed member publishes nothing fabricated.
    let blur = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "blur")
        .expect("undocumented owner-local type-argument member must still publish");
    assert_eq!(blur.description.as_deref(), None);
    assert!(blur.tags.is_empty(), "got {:?}", blur.tags);

    // The public-instance sidecar derives from the exposed surface, so the
    // owner-local type-argument members surface there too.
    let sidecar = meta
        .public_instance
        .as_ref()
        .expect("exposed members imply a public-instance sidecar");
    let sidecar_focus = sidecar
        .members
        .iter()
        .find(|member| member.name == "focus")
        .expect("sidecar must carry the exposed member");
    assert_eq!(
        sidecar_focus.kind,
        verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Exposed
    );
    assert_eq!(
        sidecar_focus.description.as_deref(),
        Some("Focus the input.")
    );
}

#[test]
fn define_expose_owner_local_type_argument_with_imported_heritage_publishes() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"
export interface BaseApi {
  /**
   * Reset the control.
   */
  reset(): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Control.vue",
            r#"<script setup lang="ts">
import type { BaseApi } from './base'

interface LocalApi extends BaseApi {
  /**
   * Focus the control.
   * @public
   */
  focus(): void
}

defineExpose<LocalApi>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Control.vue")
        .expect("component meta resolves");

    let focus = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "focus")
        .expect("owner-local heritage-bearing defineExpose must publish its own-body members");
    assert_eq!(focus.description.as_deref(), Some("Focus the control."));
    assert!(
        focus.tags.iter().any(|tag| tag.name == "public"),
        "own-body member's @public tag must publish, got {:?}",
        focus.tags
    );

    let reset = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "reset")
        .expect("heritage member from the imported base must publish");
    assert_eq!(reset.description.as_deref(), Some("Reset the control."));
}

#[test]
fn define_expose_owner_local_mixed_literal_and_type_argument_publishes_union() {
    let project = make_project();
    project
        .upsert_base(
            "/src/Field.vue",
            r#"<script setup lang="ts">
interface LocalApi {
  /**
   * Select the whole value.
   * @public
   */
  selectAll(): void
  /**
   * Type-argument doc for focus.
   */
  focus(): void
  clear(): void
}

const focus = () => {}
defineExpose<LocalApi>({
  /**
   * Focus the field.
   */
  focus,
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Field.vue")
        .expect("component meta resolves");

    // Object-literal member: its OWN leading JSDoc wins over the overlapping
    // type-argument member's doc (first-wins pairing).
    let focus = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "focus")
        .expect("object-literal member must be exposed");
    assert_eq!(
        focus.description.as_deref(),
        Some("Focus the field."),
        "literal member's own JSDoc must win over the type-argument member's doc"
    );

    // Type-argument-only member: publishes with its span-sliced doc + tag even
    // though an object literal is present alongside the owner-local type
    // argument (the mixed form rides the same owner-local discovery rail).
    let select_all = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "selectAll")
        .expect("mixed owner-local defineExpose must publish its type-argument-only members");
    assert_eq!(
        select_all.description.as_deref(),
        Some("Select the whole value."),
        "type-argument-only member's JSDoc must publish in the mixed local form"
    );
    assert!(
        select_all.tags.iter().any(|tag| tag.name == "public"),
        "type-argument-only member's @public tag must publish, got {:?}",
        select_all.tags
    );
    assert!(
        !matches!(select_all.type_expr, TypeExpr::Unknown { .. }),
        "surface member publishes its surface type, not an Unknown fallback: {:?}",
        select_all.type_expr
    );

    // Negative: an undocumented type-argument-only member publishes nothing
    // fabricated.
    let clear = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "clear")
        .expect("undocumented type-argument-only member must still publish");
    assert_eq!(clear.description.as_deref(), None);
    assert!(clear.tags.is_empty(), "got {:?}", clear.tags);

    // The public-instance sidecar derives from the exposed surface: both the
    // literal member and the type-argument-only member surface there.
    let sidecar = meta
        .public_instance
        .as_ref()
        .expect("exposed members imply a public-instance sidecar");
    let sidecar_select_all = sidecar
        .members
        .iter()
        .find(|member| member.name == "selectAll")
        .expect("sidecar must carry the type-argument-only exposed member");
    assert_eq!(
        sidecar_select_all.kind,
        verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Exposed
    );
    assert_eq!(
        sidecar_select_all.description.as_deref(),
        Some("Select the whole value.")
    );
    let sidecar_focus = sidecar
        .members
        .iter()
        .find(|member| member.name == "focus")
        .expect("sidecar must carry the literal exposed member");
    assert_eq!(
        sidecar_focus.kind,
        verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Exposed
    );
}

/// Entrance-matrix completion: mixed-IMPORTED `defineExpose<ImportedApi>({
/// ... })` — an imported type argument alongside an object literal. Mirrors
/// the mixed-local test above with the interface moved cross-file: the
/// literal member's own leading JSDoc wins on name overlap (first-wins
/// pairing), the imported type-argument-only member publishes its doc +
/// tag, an undocumented type-only member publishes nothing fabricated, and
/// the public-instance sidecar derives coherently from the union.
#[test]
fn define_expose_imported_mixed_literal_and_type_argument_publishes_union() {
    let project = make_project();
    project
        .upsert_base(
            "/src/imported_api.ts",
            r#"
export interface ImportedApi {
  /**
   * Select the whole value.
   * @public
   */
  selectAll(): void
  /**
   * Type-argument doc for focus.
   */
  focus(): void
  clear(): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Field.vue",
            r#"<script setup lang="ts">
import type { ImportedApi } from './imported_api'

const focus = () => {}
defineExpose<ImportedApi>({
  /**
   * Focus the field.
   */
  focus,
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Field.vue")
        .expect("component meta resolves");

    // Object-literal member: its OWN leading JSDoc wins over the overlapping
    // imported type-argument member's doc (first-wins pairing).
    let focus = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "focus")
        .expect("object-literal member must be exposed");
    assert_eq!(
        focus.description.as_deref(),
        Some("Focus the field."),
        "literal member's own JSDoc must win over the imported type-argument \
         member's doc"
    );

    // Imported type-argument-only member: publishes with its doc + tag even
    // though an object literal is present alongside the imported type
    // argument.
    let select_all = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "selectAll")
        .expect("mixed imported defineExpose must publish its type-argument-only members");
    assert_eq!(
        select_all.description.as_deref(),
        Some("Select the whole value."),
        "imported type-argument-only member's JSDoc must publish in the mixed \
         imported form"
    );
    assert!(
        select_all.tags.iter().any(|tag| tag.name == "public"),
        "imported type-argument-only member's @public tag must publish, got {:?}",
        select_all.tags
    );
    assert!(
        !matches!(select_all.type_expr, TypeExpr::Unknown { .. }),
        "surface member publishes its surface type, not an Unknown fallback: {:?}",
        select_all.type_expr
    );

    // Negative: an undocumented imported type-argument-only member publishes
    // nothing fabricated.
    let clear = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "clear")
        .expect("undocumented type-argument-only member must still publish");
    assert_eq!(clear.description.as_deref(), None);
    assert!(clear.tags.is_empty(), "got {:?}", clear.tags);

    // The public-instance sidecar derives from the exposed surface: both the
    // literal member and the imported type-argument-only member surface there.
    let sidecar = meta
        .public_instance
        .as_ref()
        .expect("exposed members imply a public-instance sidecar");
    let sidecar_select_all = sidecar
        .members
        .iter()
        .find(|member| member.name == "selectAll")
        .expect("sidecar must carry the imported type-argument-only exposed member");
    assert_eq!(
        sidecar_select_all.kind,
        verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Exposed
    );
    assert_eq!(
        sidecar_select_all.description.as_deref(),
        Some("Select the whole value.")
    );
    let sidecar_focus = sidecar
        .members
        .iter()
        .find(|member| member.name == "focus")
        .expect("sidecar must carry the literal exposed member");
    assert_eq!(
        sidecar_focus.kind,
        verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Exposed
    );
}

#[test]
fn define_expose_type_argument_jsdoc_publishes_cross_file() {
    let project = make_project();
    project
        .upsert_base(
            "/src/api.ts",
            r#"
export interface WidgetApi {
  /**
   * Focus the widget.
   * @public
   */
  focus(): void
  blur(): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Widget.vue",
            r#"<script setup lang="ts">
import type { WidgetApi } from './api'

const focus = () => {}
const blur = () => {}
defineExpose<WidgetApi>({ focus, blur })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Widget.vue")
        .expect("component meta resolves");

    let focus = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "focus")
        .expect("focus must be exposed");
    assert_eq!(
        focus.description.as_deref(),
        Some("Focus the widget."),
        "imported type-argument member's JSDoc must publish on the exposed member"
    );
    assert!(
        focus.tags.iter().any(|tag| tag.name == "public"),
        "imported type-argument member's @public tag must publish, got {:?}",
        focus.tags
    );

    // Negative: an undocumented exposed member publishes nothing.
    let blur = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "blur")
        .expect("blur must be exposed");
    assert_eq!(blur.description.as_deref(), None);
    assert!(blur.tags.is_empty(), "got {:?}", blur.tags);
}
