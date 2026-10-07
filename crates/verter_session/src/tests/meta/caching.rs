use super::*;

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
        evaluated_prop_type(&project, "Comp.vue", &evaluated, "label"),
        TypeExpr::Primitive(PrimitiveName::String)
    );
    assert!(
        evaluated.props.iter().all(|field| field.name != "missing"),
        "evaluation should only include actual props"
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
        evaluated_prop_type(&project, "Comp.vue", &first, "count"),
        TypeExpr::Primitive(PrimitiveName::Number)
    );

    let first_cache = cached_resolved_state(
        &project,
        "Comp.vue",
        verter_type_engine::semantic_query::ProjectionMode::Expanded,
    )
    .expect("first evaluation should populate the cache");

    let second = session.evaluate_types("Comp.vue").unwrap().unwrap();
    let second_cache = cached_resolved_state(
        &project,
        "Comp.vue",
        verter_type_engine::semantic_query::ProjectionMode::Expanded,
    )
    .expect("second evaluation should reuse the cache");

    assert_eq!(first.props.len(), second.props.len());
    assert!(Arc::ptr_eq(&first_cache, &second_cache));

    session
        .upsert("Comp.vue", sfc("count: number; label: string"))
        .unwrap();
    let third = session.evaluate_types("Comp.vue").unwrap().unwrap();
    let third_cache = cached_resolved_state(
        &project,
        "Comp.vue",
        verter_type_engine::semantic_query::ProjectionMode::Expanded,
    )
    .expect("updated file should repopulate the cache");

    assert!(third.props.iter().any(|field| field.name == "label"));
    assert!(!Arc::ptr_eq(&second_cache, &third_cache));
}

/// PUBLIC BOUNDARY — a deep macro path projection whose TERMINAL hop misses
/// never publishes an empty props surface as COMPLETE, and never warms.
///
/// `defineProps<Deep['ui']['missing']>()` where `Deep['ui']` resolves but has
/// no `missing` member: the checker REJECTS the program (`Property 'missing'
/// does not exist`), so zero props is not "nothing was declared" — it is a
/// resolution that could not produce the demanded surface. Publishing it
/// complete makes an ill-typed component byte-identical to a props-less one,
/// and warming it replays the wrong-complete answer for the file's lifetime.
///
/// The boundary triple: published `props`, `synthesis_should_suppress`, and
/// the `component_meta_result_cache_hits` delta across a replay.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_missed_terminal_hop_on_a_deep_macro_projection_never_publishes_complete_or_warm() {
    use std::sync::atomic::Ordering::Relaxed;

    let project = make_project();
    project
        .upsert_base(
            "/src/DeepMiss.vue",
            r#"<script setup lang="ts">
interface Deep { ui: { header: { title: string } } }
defineProps<Deep['ui']['missing']>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let meta = get_meta(&project, "/src/DeepMiss.vue");
    let names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        names.is_empty(),
        "a missed terminal hop must not fabricate props; got {names:?}"
    );

    let (_, resolved) = host
        .get_component_meta_with_resolution("/src/DeepMiss.vue")
        .expect("the resolve must still return metadata");
    assert!(
        resolved.synthesis_should_suppress,
        "a props surface whose terminal hop missed must NOT report COMPLETE — \
         the emptiness is a failed resolution, not an authored empty surface"
    );

    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = get_meta(&project, "/src/DeepMiss.vue");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after, hits_before,
        "a missed terminal hop must NOT warm `ComponentMetaResultDb` \
         (hits_before={hits_before}, hits_after={hits_after})"
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

// ── Component-meta result caching roots BOTH the macro type dependency and
//    the runtime root-spread dependency in the cached fact signature, and a
//    content edit to EITHER dependency rejects the cached result as stale. ──

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cached_eval_inputs_track_macro_and_runtime_dependencies() {
    use verter_session_query::facts::fact_cache::FactVersionRef;
    use verter_type_engine::semantic_query::ProjectionMode;

    let project = make_project();
    // Macro type dependency: `defineProps<WidgetProps>` imported from types.ts.
    project
        .upsert_base(
            "/src/types.ts",
            "export interface WidgetProps { enabled: boolean }",
        )
        .unwrap();
    // Runtime dependency: the root `v-bind` spread object from utils.ts.
    project
        .upsert_base(
            "/src/utils.ts",
            "export const rootAttrs = { id: 'root', onClick: () => {} }",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Widget.vue",
            r#"<script setup lang="ts">
import type { WidgetProps } from './types'
import { rootAttrs } from './utils'
defineProps<WidgetProps>()
</script>
<template><div v-bind="rootAttrs">content</div></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Widget.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./utils".to_string(),
                resolved_canonical_id: Some("/src/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    // Cold compute populates the fact-only `cached_resolved_meta` sidecar and
    // records its dependency fact signature.
    let meta = get_meta(&project, "/src/Widget.vue");
    assert!(
        meta.props.iter().any(|p| p.name == "enabled"),
        "fixture must publish the macro-declared `enabled` prop, got: {:?}",
        meta.props
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "id"),
        "the root spread key 'id' must be consumed from utils.ts, proving the \
         runtime spread dependency was actually resolved at compute time"
    );

    let host = project.host();
    // The cold compute must leave the sidecar warm.
    assert!(
        host.try_get_cached_resolved_meta("/src/Widget.vue", ProjectionMode::Expanded)
            .is_some(),
        "cold compute must leave the component-meta result sidecar warm"
    );

    // The cached fact signature must root BOTH dependencies.
    let cached = {
        let entry = host
            .derived_raw_cache()
            .get("/src/Widget.vue")
            .expect("derived-raw entry must exist after a cold component-meta compute");
        entry
            .cached_resolved_meta
            .iter()
            .find(|((mode, _view_fp), _)| *mode == ProjectionMode::Expanded)
            .map(|(_, cached)| cached.clone())
            .expect("cached_resolved_meta must hold an Expanded slot after cold compute")
    };
    let roots_file = |canonical: &str| {
        cached.fact_versions.iter().any(|fact| {
            matches!(
                fact,
                FactVersionRef::FileWholeHash { canonical_id, .. }
                    if canonical_id == canonical
            )
        })
    };
    let extraction = crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
        let fixture_dispatch_10 =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);

        crate::host_manage::extract_component_meta_from_resolved(
            host,
            "/src/Widget.vue",
            cached.state.as_ref(),
            true,
            ctx,
            &fixture_dispatch_10,
        )
    });
    assert!(
        extraction
            .fallthrough_fact_versions
            .as_deref()
            .is_some_and(|facts| facts.iter().any(|fact| matches!(
                fact,
                FactVersionRef::FileWholeHash { canonical_id, .. }
                    if canonical_id == "/src/utils.ts"
            ))),
        "fallthrough extraction MUST publish the exact runtime root-spread dependency facts"
    );
    assert!(
        roots_file("/src/types.ts"),
        "cached fact signature MUST root the macro type dependency \
         /src/types.ts — got {:?}",
        cached.fact_versions
    );
    assert!(
        roots_file("/src/utils.ts"),
        "cached fact signature MUST root the runtime root-spread dependency \
         /src/utils.ts — got {:?}",
        cached.fact_versions
    );

    // Mutating the MACRO dependency rejects the cached result as stale.
    project
        .upsert_base(
            "/src/types.ts",
            "export interface WidgetProps { enabled: boolean; extra: string }",
        )
        .unwrap();
    assert!(
        host.try_get_cached_resolved_meta("/src/Widget.vue", ProjectionMode::Expanded)
            .is_none(),
        "a content edit to the macro dependency /src/types.ts MUST reject the \
         cached component-meta result — the rooted FileWholeHash fact no longer \
         matches the live view"
    );
    // Recompute reflects the edit (a stale warm hit would return 1 prop).
    let after_types = get_meta(&project, "/src/Widget.vue");
    assert!(
        after_types.props.iter().any(|p| p.name == "extra"),
        "recompute after the macro-dep edit must surface the new `extra` prop, \
         got: {:?}",
        after_types
            .props
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
    );

    // Mutating the RUNTIME dependency likewise rejects the cached result.
    project
        .upsert_base(
            "/src/utils.ts",
            "export const rootAttrs = { title: 'root' }",
        )
        .unwrap();
    assert!(
        host.try_get_cached_resolved_meta("/src/Widget.vue", ProjectionMode::Expanded)
            .is_none(),
        "a content edit to the runtime root-spread dependency /src/utils.ts MUST \
         reject the cached component-meta result"
    );
    // Recompute reflects the new spread: 'id' is no longer consumed and now
    // surfaces as an inherited intrinsic attr on the accepted surface.
    let after_utils = get_meta(&project, "/src/Widget.vue");
    assert!(
        after_utils.accepted_props.iter().any(|p| p.name == "id"),
        "after the runtime-dep edit removed 'id' from the spread, 'id' must no \
         longer be consumed and must reappear on the accepted surface, got: {:?}",
        after_utils
            .accepted_props
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
    );
}

/// CACHE RAILS (invariants 10 + 11): an output-materialization failure
/// suppresses ONLY the encoded-payload admission; the independently-complete
/// ANALYSIS/resolved-meta cache entry published by the same request stays
/// warm, and the next (unforced) request recovers AND warms the payload
/// cache.
#[test]
fn output_failure_suppresses_only_encoded_payload_never_analysis_cache() {
    use std::sync::atomic::Ordering::Relaxed;
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ p: string }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let session = project.open_session().expect("session");

    // Force the FIRST output materialization to fail (typed, consumed flag).
    crate::meta_resolve::projectors::OUTPUT_MATERIALIZE_FORCE_FAIL.with(|f| f.set(true));
    let err = session
        .get_component_meta_payload("/App.vue", |_output| Vec::new())
        .expect_err("the forced output failure must surface as the typed MetaError");
    assert!(
        matches!(err, crate::meta::MetaError::OutputMaterialization(_)),
        "typed passthrough, never a silent Unknown; got {err:?}"
    );

    // Invariant 10: the analysis/resolved-meta cache entry admitted BEFORE
    // the output step is untouched by the failure.
    assert!(
        host.derived_raw_cache()
            .get("/App.vue")
            .map(|e| !e.value().cached_resolved_meta.is_empty())
            .unwrap_or(false),
        "the independently-complete resolved-meta admission must survive an output failure"
    );
    // Invariant 11 (suppression side): the encoded payload was NOT admitted.
    assert!(
        host.derived_raw_cache()
            .get("/App.vue")
            .map(|e| e.value().cached_meta_payload.is_none())
            .unwrap_or(true),
        "a failed output must never admit an encoded payload"
    );

    // RECOVERY: the next (unforced) request succeeds and admits the payload.
    host.provenance().payload_encodes.store(0, Relaxed);
    let payload = session
        .get_component_meta_payload("/App.vue", |output| {
            format!("{:?}", output.into_parts().2.into_lanes().props).into_bytes()
        })
        .expect("recovery: the unforced request succeeds")
        .expect("component resolves");
    assert!(!payload.is_empty());
    assert!(
        host.derived_raw_cache()
            .get("/App.vue")
            .map(|e| e.value().cached_meta_payload.is_some())
            .unwrap_or(false),
        "the recovered request admits the encoded payload (with output deps on its rail)"
    );
}

/// COLD → WARM equivalence on the base output entry, plus overlay/session
/// equivalence: the warm-path envelope (validated cache entry + SAME-capture
/// materialization) is byte-identical to the cold envelope, and an overlay
/// session sees ITS overlay content, never the base.
#[test]
fn component_meta_output_cold_warm_base_and_overlay_agree() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ p: string }>()
defineEmits<{ e: [n: number] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();

    let cold = host
        .get_component_meta_output("/App.vue")
        .expect("cold output ok")
        .expect("resolves");
    let (cold_analysis, _, cold_types) = cold.into_parts();
    let cold_debug = format!("{:?}{:?}", cold_analysis, cold_types.into_lanes());

    let warm = host
        .get_component_meta_output("/App.vue")
        .expect("warm output ok")
        .expect("resolves");
    let (warm_analysis, _, warm_types) = warm.into_parts();
    let warm_debug = format!("{:?}{:?}", warm_analysis, warm_types.into_lanes());
    assert_eq!(
        cold_debug, warm_debug,
        "warm-path output must equal the cold output byte-for-byte"
    );

    // Session with NO overlay agrees with base.
    let session = project.open_session().expect("session");
    let base_via_session = session
        .get_component_meta_output("/App.vue")
        .expect("session output ok")
        .expect("resolves");
    let (sa, _, st) = base_via_session.into_parts();
    assert_eq!(
        format!("{:?}{:?}", sa, st.into_lanes()),
        warm_debug,
        "an overlay-less session output equals the base output"
    );

    // Session WITH an overlay sees the overlay surface.
    session
        .upsert(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ p: boolean }>()
</script>
<template><div /></template>"#
                .to_string(),
        )
        .expect("overlay upsert");
    let overlaid = session
        .get_component_meta_output("/App.vue")
        .expect("overlay output ok")
        .expect("resolves");
    let (oa, _, ot) = overlaid.into_parts();
    let olanes = ot.into_lanes();
    let p_idx = oa.props.iter().position(|p| p.name == "p").unwrap();
    assert_eq!(
        published_type(&olanes.props[p_idx]),
        &TypeExpr::Primitive(PrimitiveName::Boolean),
        "the overlay session materializes the OVERLAY's prop type"
    );

    // Base host remains unpolluted by the session overlay.
    let base_after = host
        .get_component_meta_output("/App.vue")
        .expect("base output ok")
        .expect("resolves");
    let (ba, _, bt) = base_after.into_parts();
    assert_eq!(
        format!("{:?}{:?}", ba, bt.into_lanes()),
        warm_debug,
        "the base output is unchanged by a session overlay"
    );
}

/// FIXED-VIEW batch: the output batch equals the scalar outputs slot-by-slot
/// AND performs O(1) (not O(N)) store-view reads on the warm pass — the
/// single `capture_batch_fixed_view` read is threaded into every per-job
/// call (invariant 15: no extra per-item store-view reads).
#[test]
fn output_batch_equals_scalar_and_is_o1_store_view_reads_when_warm() {
    use std::sync::atomic::Ordering::Relaxed;
    let project = make_project();
    const N: usize = 8;
    let mut ids = Vec::new();
    for i in 0..N {
        let id = format!("/Comp{i}.vue");
        project
            .upsert_base(
                &id,
                &format!(
                    r#"<script setup lang="ts">
defineProps<{{ p{i}: string }}>()
</script>
<template><div /></template>"#
                ),
            )
            .unwrap();
        ids.push(id);
    }
    let host = project.host();
    let session = project.open_session_batch().expect("batch session");

    // Cold pass populates the caches.
    let cold = session
        .get_component_meta_output_batch(&ids)
        .expect("cold output batch");
    assert_eq!(cold.len(), N);

    // Scalar reference outputs (warm).
    let scalar_debug: Vec<String> = ids
        .iter()
        .map(|id| {
            let output = session
                .get_component_meta_output(id)
                .expect("scalar ok")
                .expect("resolves");
            let (a, _, t) = output.into_parts();
            format!("{:?}{:?}", a, t.into_lanes())
        })
        .collect();

    // Warm batch: capture-only from_host reads on THIS host — the count is
    // SIZE-INVARIANT (a per-item read implementation scales with N and
    // fails the equality below) and pinned EXACTLY to the per-batch capture
    // cost, not merely `< N`.
    host.provenance()
        .store_view_from_host_reads
        .store(0, Relaxed);
    let warm = session
        .get_component_meta_output_batch(&ids)
        .expect("warm output batch");
    let reads_full = host.provenance().store_view_from_host_reads.load(Relaxed);
    assert!(
        reads_full >= 1,
        "the batch capture's own read must be counted (live counter)"
    );

    // Second warm batch over HALF the inputs: the read count must be
    // IDENTICAL — the per-batch capture is the only store-view read, so it
    // cannot scale with the item count.
    let half_ids: Vec<String> = ids[..N / 2].to_vec();
    host.provenance()
        .store_view_from_host_reads
        .store(0, Relaxed);
    let warm_half = session
        .get_component_meta_output_batch(&half_ids)
        .expect("warm half batch");
    assert!(warm_half.len() == N / 2);
    let reads_half = host.provenance().store_view_from_host_reads.load(Relaxed);
    assert_eq!(
        reads_full,
        reads_half,
        "the warm-batch store-view read count must be SIZE-INVARIANT \
         (capture-only): {N} items took {reads_full} reads, {} items took \
         {reads_half} — a per-item read path scales with the batch size",
        N / 2
    );
    assert_eq!(
        reads_full, 2,
        "the warm output batch performs EXACTLY the per-batch capture-level \
         constant of 2 store-view reads (the overlay pre-warm's read + the \
         fixed-view capture) — any additional read is a per-item or \
         per-stage regression"
    );

    for (slot, expected) in warm.into_iter().zip(scalar_debug.iter()) {
        let output = slot.expect("slot ok").expect("resolves");
        let (a, _, t) = output.into_parts();
        assert_eq!(
            &format!("{:?}{:?}", a, t.into_lanes()),
            expected,
            "batch slot output equals the scalar output for the same canonical"
        );
    }
}

/// WARM-arm view fence (invariant 14): a dependency mutation landing
/// BETWEEN the warm-cache validation and the output materialization must
/// not tear the response — the materialization runs under the SAME captured
/// view the validation used (fully the captured world), never an
/// old-analysis paired with a fresh-view materialization.
///
/// Discrimination: an implementation that materializes under a FRESH
/// store-view read (instead of the entry's one capture) observes the
/// mutated dep and the lane flips to `string` — the exact `number`
/// assertion fails RED.
#[test]
fn warm_output_materializes_under_the_validated_capture_not_a_fresh_view() {
    /// The materialized `x` member of the owner-local `Sentinel` registry
    /// row — an AUTHORED decl-body source, so the materialization DEREFS
    /// the declaration under the request view (view-dependent, unlike the
    /// closed leaf carriers the prop lanes publish).
    fn sentinel_registry_member(
        analysis: &verter_session_query::analysis::component_meta::ComponentMetaAnalysis,
        lanes: &crate::meta_resolve::MaterializedComponentMetaTypeLanes,
    ) -> TypeExpr {
        let idx = analysis
            .type_registry
            .iter()
            .position(|e| e.name == "Sentinel")
            .expect("the owner-local Sentinel type enters the registry");
        match &lanes.type_registry_entries[idx] {
            TypeExpr::Object(object) => object
                .properties
                .iter()
                .find_map(|member| match member {
                    verter_type_expr::ObjectMember::Property(prop)
                        if prop.string_name().expect("string-key fixture") == "x" =>
                    {
                        Some(prop.ty.clone())
                    }
                    _ => None,
                })
                .expect("Sentinel carries the `x` member"),
            other => panic!("the registry row materializes Sentinel's body; got {other:?}"),
        }
    }

    fn warm_fence_sfc(x_type: &str) -> String {
        format!(
            r#"<script setup lang="ts">
type Sentinel = {{ x: {x_type} }}
defineProps<{{ named: Sentinel }}>()
</script>
<template><div /></template>"#
        )
    }

    let project = make_project();
    project
        .upsert_base("/WarmFence.vue", &warm_fence_sfc("number"))
        .unwrap();

    // Cold pass warms the analysis cache and materializes the owner's
    // artifacts under the ORIGINAL content.
    let cold = project
        .host()
        .get_component_meta_output("/WarmFence.vue")
        .expect("cold output ok")
        .expect("resolves");
    let (cold_analysis, _cold_res, cold_types) = cold.into_parts();
    let cold_lanes = cold_types.into_lanes();
    assert_eq!(
        sentinel_registry_member(&cold_analysis, &cold_lanes),
        TypeExpr::Primitive(PrimitiveName::Number)
    );

    // Arm the warm-arm hook: the owner mutation lands AFTER the warm entry
    // validated against the capture, BEFORE the output materializes.
    let mutate = Arc::clone(&project);
    crate::host_manage::component_meta_entry::WARM_OUTPUT_PRE_MATERIALIZE_HOOK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            mutate
                .upsert_base("/WarmFence.vue", &warm_fence_sfc("string"))
                .unwrap();
        }));
    });

    let warm = project
        .host()
        .get_component_meta_output("/WarmFence.vue")
        .expect("warm output ok")
        .expect("resolves");
    let (warm_analysis, _warm_res, warm_types) = warm.into_parts();
    let warm_lanes = warm_types.into_lanes();
    assert_eq!(
        sentinel_registry_member(&warm_analysis, &warm_lanes),
        TypeExpr::Primitive(PrimitiveName::Number),
        "the materialization derefs Sentinel under the SAME capture the warm \
         validation used — a fresh-view materialization would observe the \
         mutated body (`x: string`) and tear the response"
    );
}

// @ai-generated
#[test]
fn warm_output_publication_evidence_keeps_the_exact_validated_cache_key() {
    let project = make_project();
    project
        .upsert_base(
            "/WarmKey.vue",
            r#"<script setup lang="ts">defineProps<{ value: string }>()</script><template/>"#,
        )
        .unwrap();
    let host = project.host();
    host.get_component_meta_output("/WarmKey.vue")
        .expect("cold output")
        .expect("cold projection");

    let view = crate::session_view::HostViewRef::new(host);
    let fixed = host.capture_batch_fixed_view(&view);
    let validated_key = host.component_meta_result_key(
        "/WarmKey.vue",
        &crate::host_manage::ComponentMetaOptions::default(),
    );
    let mutate = Arc::clone(&project);
    crate::host_manage::component_meta_entry::WARM_OUTPUT_PRE_EVIDENCE_HOOK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            mutate
                .host()
                .configure_projects(vec![verter_workspace::ide_project_config(
                    "/".to_string(),
                    "/".to_string(),
                    Some("/tsconfig.json".to_string()),
                )]);
        }));
    });

    let warm = host
        .get_component_meta_output_via_view_with_publication_evidence(
            "/WarmKey.vue",
            &view,
            &fixed,
            false,
        )
        .expect("warm output")
        .expect("warm projection");
    let evidence = warm
        .publication_evidence
        .expect("a warm cache hit carries publication evidence");
    let live_key = host.component_meta_result_key(
        "/WarmKey.vue",
        &crate::host_manage::ComponentMetaOptions::default(),
    );

    assert_eq!(evidence.final_result.key, validated_key);
    assert_ne!(
        evidence.final_result.key, live_key,
        "the hook must move the live environment after validation; evidence must not recompute it"
    );
}
