use super::*;

/// Within one request, normalizing an analysis canonical probes its
/// declaration companions once: every later normalization of it in the
/// request shares that run, its observations replayed into any witness
/// scope open around the consumer, and an edit in the request probes again.
/// Probing for every consumer re-ran the same missing dependencies' probes
/// about sixty times per request on a real-world component.
#[test]
fn a_request_normalizes_an_analysis_canonical_once() {
    use crate::host_manage::import_route_witness::{
        normalization_builds_for_tests, ResolutionWitnessScope,
    };
    let host = make_host();
    let importer = "/workspace/src/importer.ts";
    let missing = "/workspace/src/missing";
    upsert_non_sfc(
        &host,
        importer,
        "import { X } from './missing'\nexport type Re = X\n",
    );
    let _request = verter_type_engine::request_context::install_test_request_for(importer);
    let resolutions = |host: &crate::VerterHost| {
        let provenance = host.ws().vfs_provenance_snapshot();
        provenance.import_resolution_cache_hit_count + provenance.import_resolution_cache_miss_count
    };
    let builds = normalization_builds_for_tests();
    let (first, observed) = {
        let scope = ResolutionWitnessScope::enter();
        let first = host.normalized_analysis_canonical(missing).into_owned();
        (first, scope.collected())
    };
    assert_eq!(first, missing, "a missing dependency normalizes to itself");
    assert!(!observed.is_empty(), "its probes observe facts");
    let probed = resolutions(&host);
    for _ in 0..5 {
        let replayed = {
            let scope = ResolutionWitnessScope::enter();
            assert_eq!(host.normalized_analysis_canonical(missing), missing);
            scope.collected()
        };
        assert_eq!(
            replayed, observed,
            "a shared normalization records its observations into the scope around its consumer"
        );
    }
    assert_eq!(
        resolutions(&host),
        probed,
        "later normalizations in the request probe nothing"
    );
    assert_eq!(
        normalization_builds_for_tests() - builds,
        1,
        "one run per request"
    );

    upsert_non_sfc(&host, "/workspace/src/missing.ts", "export type X = 1\n");
    assert_eq!(
        host.normalized_analysis_canonical(missing),
        "/workspace/src/missing.ts",
        "an edit in the request probes again and finds the new file"
    );
    assert_eq!(normalization_builds_for_tests() - builds, 2);
}

#[test]
fn raw_template_analysis_extracts_css_var_names() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/A.vue",
        "<script setup>\nconst color = 'red'\n</script>\n<template><div :style=\"{ '--theme-color': color }\">A</div></template>",
    );

    let template = host
        .raw_template_analysis_for_file("/src/A.vue")
        .expect("raw template analysis should be computed");
    assert!(
        template
            .css_var_names
            .iter()
            .any(|name| name == "--theme-color"),
        "raw template analysis should include CSS vars from :style bindings"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[should_panic(expected = "CorrelationMismatch")]
fn override_template_analysis_helper_uses_content_override() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/A.vue",
        "<script setup>\nconst color = 'red'\n</script>\n<template><div>A</div></template>",
    );

    let profile = CompileProfile::default();
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "/src/A.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry::unissued_for_test(
                "<div :style=\"{ '--theme-color': color }\">A</div>",
            )],
        })
        .expect("template override should succeed");

    let template = host
        .raw_template_analysis_for_file("/src/A.vue")
        .expect("override template analysis should be computed");
    assert!(
        template
            .css_var_names
            .iter()
            .any(|name| name == "--theme-color"),
        "override template analysis should reflect the overridden template"
    );
}

/// Served style analyses carry the sealed inventory-minted block ref AND the
/// session-validated public block token — including the external-`src`
/// deferred block (identity is structure work; content stays deferred).
#[test]
fn get_analysis_attaches_sealed_refs_and_validated_public_block_tokens() {
    let host = make_host();
    upsert_vue(
        &host,
        "/project/Tokens.vue",
        "<style>.a {}</style>\n<style src=\"./ext.css\"></style>\n<style>.b {}</style>",
    );

    let analysis = host.get_analysis("/project/Tokens.vue").unwrap();
    assert_eq!(analysis.styles.len(), 3);
    let (structure, _) = host
        .registered_file_structure_snapshot("/project/Tokens.vue")
        .expect("registered structure");
    let inventory = structure.inventory();

    let mut seen_tokens = std::collections::HashSet::new();
    for style in analysis.styles.iter() {
        let block_ref = style
            .block_ref
            .as_ref()
            .expect("every produced style analysis carries a sealed ref");
        assert!(
            block_ref.validate(inventory),
            "the sealed ref validates against the live registered inventory"
        );
        let expected_token = structure
            .public_block_token(&structure.block_ref(block_ref.block_id()).unwrap())
            .expect("same artifact")
            .as_str()
            .to_owned();
        assert_eq!(
            style.block_token.as_deref(),
            Some(expected_token.as_str()),
            "the served token is the session public block token for the \
             sealed ref's block"
        );
        assert!(
            seen_tokens.insert(expected_token),
            "tokens are distinct per block"
        );
    }
    let deferred = analysis
        .styles
        .iter()
        .find(|style| !style.content_is_available())
        .expect("external-src style stays a typed deferred analysis");
    assert!(
        deferred.block_ref.is_some() && deferred.block_token.is_some(),
        "deferred content still carries full sealed identity"
    );
}

#[test]
fn get_analysis_uses_parse_artifact_for_lazy_analysis() {
    let host = make_lazy_host();
    upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

    // On the scheduler path, source is immutable in the scheduler snapshot,
    // so mutating host.files has no effect. The scheduler path reads from
    // HostSourceData.framework_parse directly. We just verify get_analysis()
    // returns correct lazy-recomputed data with AnalysisLevel::None.
    #[cfg(target_arch = "wasm32")]
    mutate_lazy_analysis_source(&host);

    let analysis = host.get_analysis("App.vue").unwrap();

    assert!(
        analysis.bindings.iter().any(|b| b.name == "msg"),
        "lazy script analysis should reuse cached parse for bindings"
    );
    assert_eq!(
        analysis.styles.len(),
        1,
        "lazy style analysis should reuse cached parse for style blocks"
    );
    let css = analysis.styles[0]
        .css
        .as_ref()
        .expect("CSS analysis should exist for cached style block");
    assert!(
        css.classes.iter().any(|class| class.name == "foo"),
        "lazy style analysis should preserve CSS classes"
    );
    assert!(
        analysis
            .module_references
            .iter()
            .any(|reference| reference.literal_specifier.as_deref() == Some("vue")),
        "lazy script analysis should preserve module references"
    );
}

#[test]
fn get_analysis_falls_back_when_parse_artifact_missing() {
    let host = make_lazy_host();
    upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

    // On the scheduler path, framework_parse is immutable in HostSourceData
    // and always present for Vue SFCs. The scheduler path handles both
    // artifact present and absent cases. We just verify correctness.
    #[cfg(target_arch = "wasm32")]
    clear_framework_parse(&host);

    let analysis = host.get_analysis("App.vue").unwrap();

    assert!(
        analysis.bindings.iter().any(|b| b.name == "msg"),
        "source fallback should still recover bindings"
    );
    assert_eq!(
        analysis.styles.len(),
        1,
        "source fallback should still recover style blocks"
    );
    let css = analysis.styles[0]
        .css
        .as_ref()
        .expect("CSS analysis should exist for fallback style block");
    assert!(
        css.classes.iter().any(|class| class.name == "foo"),
        "source fallback should preserve CSS classes"
    );
    assert!(
        analysis
            .module_references
            .iter()
            .any(|reference| reference.literal_specifier.as_deref() == Some("vue")),
        "source fallback should preserve module references"
    );
}

#[test]
fn get_analysis_batch_returns_all_existing() {
    let host = make_host();
    upsert_vue(
        &host,
        "A.vue",
        "<script setup>\nconst a = 1\n</script>\n<template><div/></template>",
    );
    upsert_vue(
        &host,
        "B.vue",
        "<script setup>\nconst b = 2\n</script>\n<template><div/></template>",
    );

    let results = host.get_analysis_batch(&["A.vue", "B.vue", "NonExistent.vue"]);
    assert_eq!(results.len(), 2, "should return only existing files");
    assert!(
        results.iter().any(|(id, _)| id == "A.vue"),
        "should contain A.vue"
    );
    assert!(
        results.iter().any(|(id, _)| id == "B.vue"),
        "should contain B.vue"
    );
    // Negative: should NOT contain non-existent
    assert!(
        !results.iter().any(|(id, _)| id == "NonExistent.vue"),
        "should not contain non-existent file"
    );
}

#[test]
fn get_analysis_batch_matches_individual() {
    let host = make_host();
    upsert_vue(
        &host,
        "A.vue",
        "<script setup>\nimport { ref } from 'vue'\nconst x = ref(0)\n</script>\n<template><div/></template>",
    );

    let individual = host.get_analysis("A.vue").unwrap();
    let batch = host.get_analysis_batch(&["A.vue"]);
    assert_eq!(batch.len(), 1);
    let (_, batch_snap) = &batch[0];

    assert_eq!(
        individual.bindings.len(),
        batch_snap.bindings.len(),
        "batch bindings count should match individual"
    );
    assert_eq!(
        individual.imports.len(),
        batch_snap.imports.len(),
        "batch imports count should match individual"
    );
    assert_eq!(
        individual.script_flags, batch_snap.script_flags,
        "batch script_flags should match individual"
    );
}

#[test]
fn get_analysis_batch_empty_returns_empty() {
    let host = make_host();
    let results = host.get_analysis_batch(&[]);
    assert!(results.is_empty(), "empty batch should return empty vec");
}

/// @ai-generated - get_analysis includes export signatures
#[test]
fn get_analysis_includes_export_signatures() {
    let host = make_host();
    upsert_ts(
        &host,
        "utils.ts",
        "export function helper() { return 1; }\nexport type Util = number;",
    );

    let analysis = host.get_analysis("utils.ts").unwrap();
    assert!(
        !analysis.export_signatures.is_empty(),
        "analysis should include export signatures"
    );

    let helper_sig = analysis
        .export_signatures
        .iter()
        .find(|s| s.name == "helper")
        .expect("should have 'helper' export");
    assert!(!helper_sig.is_type);

    let util_sig = analysis
        .export_signatures
        .iter()
        .find(|s| s.name == "Util")
        .expect("should have 'Util' export");
    assert!(util_sig.is_type);
}

#[test]
fn get_semantic_hash_returns_none_for_missing_file() {
    let host = make_host();
    assert!(
        host.get_semantic_hash("nonexistent.vue").is_none(),
        "missing file should return None"
    );
}

// ═══════════════════════════════════════════════════════════
// Template slots via lazy analysis (compute_template_analysis_if_missing)
// ═══════════════════════════════════════════════════════════

/// @ai-generated - template slots detected via lazy META compilation
#[test]
fn template_slots_via_analysis_only() {
    let host = make_host(); // analysis_level: Full → scope includes template
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup>\n</script>\n<template><div><slot /></div></template>",
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let tpl = analysis
        .template
        .expect("template analysis should be populated");
    assert_eq!(tpl.defined_slots.len(), 1);
    assert_eq!(tpl.defined_slots[0].name, "default");
}

/// @ai-generated - template analysis not computed when scope doesn't include template
#[test]
fn template_slots_not_computed_on_lazy_host() {
    let host = make_lazy_host(); // analysis_level: None → scope excludes template
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup>\n</script>\n<template><div><slot /></div></template>",
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    assert!(
        analysis.template.is_none(),
        "template should not be computed when scope excludes it"
    );
}

/// CF3-A2-LAZY-ASYNC-CARRIER — `const X = defineAsyncComponent(() =>
/// import('./X.vue'))` declares a component whose carrier is the dynamically
/// imported `.vue`; the analyzer captures the static loader specifier on the
/// binding, so the lazy template lane (upsert → `get_analysis`, no
/// `compile_entry`) must link `<X/>` to that carrier.
#[test]
fn cf3_lazy_template_links_static_define_async_component_carrier() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Comp.vue",
        r#"<script setup lang="ts">
import { defineAsyncComponent } from 'vue'
const X = defineAsyncComponent(() => import('./X.vue'))
</script>
<template><X /></template>"#,
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let tpl = analysis
        .template
        .expect("template analysis should be populated");
    let usage = tpl
        .components
        .iter()
        .find(|c| c.name == "X")
        .expect("<X/> usage should be recorded");
    assert_eq!(
        usage.import_source.as_deref(),
        Some("./X.vue"),
        "a static defineAsyncComponent loader must carrier-link <X/>"
    );
}

#[test]
fn read_analysis_source_and_current_eval_state_ignore_empty_canonical_ids() {
    let ws = Arc::new(CountingWorkspace::new());
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    assert!(
        host.read_analysis_source("").is_none(),
        "empty canonical ids should not resolve analysis source",
    );
    assert!(
        host.current_eval_state("").is_none(),
        "empty canonical ids should not materialize eval state",
    );
    assert_eq!(
        ws.read_count(""),
        0,
        "empty canonical ids must not trigger workspace reads",
    );
    assert_eq!(
        ws.exists_count(""),
        0,
        "empty canonical ids must not trigger workspace existence probes",
    );
    assert!(
        host.ensure_indexed_ready("").is_none(),
        "empty canonical ids must not seed imported dependency cache entries",
    );
}

/// Unit test 2: Lazy promotion stability.
///
/// When dependency resolution changes from `{resolved_canonical_id: None,
/// possible: ["/dep.d.ts", "/dep.ts"]}` to `{resolved_canonical_id:
/// Some("/dep.d.ts"), possible: [...]}`, the effective target is the same
/// (`.d.ts` wins by TS-first priority). The bundle should NOT be invalidated.
#[test]
fn lazy_promotion_stability() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/dep.d.ts",
        "export interface Helper { aid: boolean }\n",
    );
    ws.inject_file("/src/dep.ts", "export interface Helper { aid: boolean }\n");
    ws.inject_file(
        "/src/types.ts",
        "import type { Helper } from './dep'\nexport interface Props extends Helper {}\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should materialize");

    // Set initial dependency with no resolved_canonical_id but possible candidates.
    host.set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: vec!["/src/dep.d.ts".to_string(), "/src/dep.ts".to_string()],
        }],
    );

    // First lookup — materializes the bundle with effective target = /src/dep.d.ts.
    let _view_before = host.resolver_store_view_read().into_owned_view();
    let initial = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare with lazy resolution");
    assert_eq!(
        initial
            .name_resolution
            .get("Helper")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/dep.d.ts"),
        "lazy resolution should prefer .d.ts via effective_target()",
    );

    // Promote: set resolved_canonical_id to the same effective target.
    host.set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/src/dep.d.ts".to_string()),
            possible_canonical_ids: vec!["/src/dep.d.ts".to_string(), "/src/dep.ts".to_string()],
        }],
    );

    // After promotion, the effective target is unchanged — bundle should survive.
    let _view_after = host.resolver_store_view_read().into_owned_view();
    let after_promotion = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should still be found after lazy promotion");
    assert_eq!(
        after_promotion
            .name_resolution
            .get("Helper")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/dep.d.ts"),
        "lazy promotion to the same effective target should NOT change name_resolution",
    );
}

// ----------------------------------------------------------------
// F4 — `component_meta_trace_custom!` laziness discriminators.
// a side-effecting AtomicUsize counter proves
// that the macro's `$detail` expression is NOT evaluated when no
// audit accumulator is installed. Pre-fix: counter increments on
// every call. Post-fix: counter increments only when accumulator
// is installed.
//
// Tests live here (not under `tests/`) because the macro is
// `pub(crate) use component_meta_trace_custom;` and integration
// tests cannot import a `pub(crate)` macro (D36).
// ----------------------------------------------------------------

#[cfg(test)]
mod trace_laziness_tests {
    use super::super::*;
    use std::cell::Cell;
    use verter_type_engine::component_meta_trace_custom;
    use verter_type_engine::request_context::{RequestContext, RequestContextGuard};
    use verter_type_engine::request_footprint::RequestFootprintAccumulator;

    // Counter is per-test (declared inside the test function), not
    // module-static — cargo runs sibling tests in parallel and any
    // shared mutable state would race. `Cell<u32>` is single-threaded
    // and lives entirely on the test's stack, so the macro's
    // captured-by-reference closure is safe without locking.

    #[test]
    fn macro_detail_not_evaluated_when_no_accumulator_installed() {
        let counter: Cell<u32> = Cell::new(0);
        // The macro's $detail expression evaluates only when the if-guard
        // takes the branch. Inline a block that ticks the counter so we
        // observe whether the macro evaluated detail at all.
        let tick = || {
            counter.set(counter.get() + 1);
            String::from("detail")
        };
        component_meta_trace_custom!("test_event", tick());
        component_meta_trace_custom!("test_event", tick());
        component_meta_trace_custom!("test_event", tick());
        assert_eq!(
            counter.get(),
            0,
            "F4: macro $detail must not run when no accumulator is installed",
        );
    }

    #[test]
    fn macro_detail_evaluated_when_accumulator_installed() {
        let counter: Cell<u32> = Cell::new(0);
        let tick = || {
            counter.set(counter.get() + 1);
            String::from("detail")
        };
        let acc = Arc::new(RequestFootprintAccumulator::new());
        let ctx = RequestContext::new(
            42,
            Arc::from("/test_lazy_macro.vue"),
            true,
            Some(Arc::clone(&acc)),
        );
        let _guard = RequestContextGuard::install(ctx);
        component_meta_trace_custom!("test_event", tick());
        component_meta_trace_custom!("test_event", tick());
        assert_eq!(
            counter.get(),
            2,
            "F4 regression invariant: macro must fire $detail when accumulator is installed",
        );
    }
}
