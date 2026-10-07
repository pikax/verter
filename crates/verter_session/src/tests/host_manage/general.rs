use super::*;

#[test]
fn ensure_indexed_ready_populates_routes_for_non_sfc() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "import type { Base } from './base'\nexport interface Props extends Base { label: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/base.ts",
        "export interface Base { id: string }\n",
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./base", "/src/base.ts")],
    );

    let entry = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("imported dependency state should be materialized");

    // snapshot is Arc<FileAnalysisSnapshot> (non-optional) in IndexedReady.
    // Non-SFC entries always have a snapshot populated after materialization.
    assert!(
        !entry.raw_source.is_empty(),
        "non-SFC imported dependency state should retain the analysis snapshot without caching env",
    );
    assert!(
        entry.script_analysis.is_some() && entry.export_signatures.is_some(),
        "non-SFC imported dependency state should retain script facts alongside the full snapshot for later export-graph reuse",
    );
    assert!(
        entry.route_inventory.counts.top_level_statement_count > 0,
        "non-SFC imported dependency state should retain routes so later resolver lookups stay on cache",
    );
}

/// Typed-completeness gate: a NON-budget partial (a fuse / semantic-miss class
/// signal folded via `mark_request_result_partial`) gates BOTH
/// fallthrough cache-admission sites — `store_node` and
/// `cache_fallthrough_result` — EVEN THOUGH the projection budget is NOT
/// exhausted. This proves the gate keys on the typed cold-compute completeness,
/// not the ad-hoc `is_exhausted()` predicate the fix deletes.
///
/// Without the fix both gates consult `current_request_budget().is_exhausted()`
/// (false here), so the node IS stored and the mirror IS warmed. With it, both
/// gates consult `current_cold_compute_completeness().is_partial()` (true),
/// refusing admission.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn non_budget_partial_gates_fallthrough_admission_with_budget_unexhausted() {
    use crate::resolver_core::fallthrough_resolver::intrinsic_surface_key;
    use crate::resolver_core::FallthroughRequestHost;

    let host = make_host();
    upsert_vue(&host, "/src/App.vue", r#"<template><div /></template>"#);
    let canonical = "/src/App.vue";

    // A request budget with ample headroom — it is NEVER exhausted.
    let rctx =
        verter_type_engine::request_context::RequestContext::with_kind_timing_and_projection_budget(
            1,
            Arc::from(canonical),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            100_000,
        );
    let _guard =
        verter_type_engine::request_context::RequestContextGuard::install(Arc::clone(&rctx));
    let _scope = verter_type_engine::request_context::ColdComputeCompletenessScope::enter();

    // Fold a NON-budget partial (fuse / semantic-miss class) WITHOUT touching
    // the projection budget.
    verter_type_engine::request_context::mark_request_result_partial();

    // The discriminating precondition split: the partial is typed completeness,
    // NOT budget exhaustion. The deleted ad-hoc gate would NOT fire here.
    assert!(
        !rctx.projection_budget.is_exhausted(),
        "the projection budget must NOT be exhausted — this isolates the non-budget partial"
    );
    assert!(
        verter_type_engine::request_context::current_cold_compute_completeness().is_partial(),
        "the cold-compute scope must carry a Partial after a non-budget fold"
    );

    // (a) The owner-run node admission refuses on typed completeness.
    //
    // The cacheability probe is CLEAN here (no fenced serve, no overflow), which
    // is what isolates the rail under test: the ONLY thing that can refuse this
    // admission is the typed cold-compute completeness.
    let (anchor, generation) = host.project_intrinsic_cache_anchor(canonical);
    let key = intrinsic_surface_key(&anchor, "div");
    let members = host.intrinsic_members_for_tag("div");
    let node = host.build_runtime_intrinsic_surface_node(&members, generation);
    host.resolver_runtime()
        .fallthrough
        .compute_and_maybe_admit(&host, || ((), Some((key.clone(), node))));
    let view = FallthroughRequestHost::snapshot_store_view(&host);
    assert!(
        host.resolver_runtime()
            .fallthrough
            .get_cached_node(&key, &view)
            .is_none(),
        "a NON-budget partial (budget not exhausted) MUST refuse owner admission — the gate is \
         typed completeness, NOT is_exhausted() (pre-fix is_exhausted()=false stored the node)"
    );

    // (b) `cache_fallthrough_result` refuses the legacy mirror on the same gate.
    let result = crate::types::FallthroughResolution {
        accepted_props: Vec::new(),
        accepted_events: Vec::new(),
        accepted_surface_completeness:
            verter_session_query::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface:
            verter_session_query::analysis::component_meta::FallthroughSurface::None {
                reason:
                    verter_session_query::analysis::component_meta::NoFallthroughReason::NoTemplate,
            },
        fact_versions: Vec::new(),
    };
    verter_type_engine::fact_signature_helpers::with_cacheability_scope(
        &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::unbound(&host),
        |probe| {
            let admission =
                crate::resolver_core::FallthroughStableAdmission::from_test_scope(probe);
            host.cache_fallthrough_result(canonical, None, &result, &admission);
        },
    );
    let mirror_present = host
        .derived_raw_cache()
        .get(canonical)
        .and_then(|entry| entry.cached_fallthrough.as_ref().map(|_| ()))
        .is_some();
    assert!(
        !mirror_present,
        "a NON-budget partial MUST refuse the cached_fallthrough mirror — typed-completeness gate, \
         not is_exhausted() (pre-fix the un-exhausted budget warmed the mirror)"
    );

    drop(_scope);
    drop(_guard);
}

/// @ai-generated - get_export_span for .ts file returns export signature span
#[test]
fn get_export_span_ts_file() {
    let host = make_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "utils.ts".to_string(),
            source: Arc::from("export function helper() { return 1; }"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    let span = host.get_export_span("utils.ts", "helper");
    assert!(span.is_some(), "should find 'helper' export in .ts file");
    let (start, end) = span.unwrap();
    let source = host.get_source("utils.ts").unwrap();
    let spanned = &source[start as usize..end as usize];
    assert_eq!(
        spanned, "helper",
        "span should cover the function identifier"
    );
}

/// @ai-generated - get_export_span for .vue default import anchors at file start
#[test]
fn get_export_span_vue_default() {
    let host = make_host();
    upsert_vue(
        &host,
        "Child.vue",
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
    );

    // The component default export has no authored source token; the honest
    // anchor is the file start, never an unrelated internal local's span.
    let span = host.get_export_span("Child.vue", "default");
    assert_eq!(
        span,
        Some((0, 0)),
        "default export of .vue should anchor at file start (0, 0)"
    );
}

/// @ai-generated - an EMPTY .vue still carries a default export anchored at file start
#[test]
fn get_export_span_vue_default_empty_sfc() {
    let host = make_host();
    // A completely empty SFC compiles to the synthetic empty-component shell;
    // its default export has no authored token either, so the same honest
    // file-start anchor must hold (navigation terminates at the component).
    upsert_vue(&host, "Empty.vue", "");

    let span = host.get_export_span("Empty.vue", "default");
    assert_eq!(
        span,
        Some((0, 0)),
        "default export of an empty .vue should anchor at file start (0, 0)"
    );
}

#[test]
fn get_export_span_follows_reexport_to_vue() {
    let host = make_host();

    // Target: Popup.vue with a binding
    upsert_vue(
        &host,
        "/project/Popup.vue",
        "<script setup>\nconst message = 'hello'\n</script>\n<template><div>{{ message }}</div></template>",
    );

    // Barrel: index.ts re-exports Popup.vue as default
    upsert_ts(
        &host,
        "/project/index.ts",
        "export { default as Popup } from './Popup.vue'",
    );

    // Follow the re-export: "Popup" in index.ts → default in Popup.vue
    let result = host.get_export_span_follow_reexports("/project/index.ts", "Popup");

    assert!(result.is_some(), "should follow re-export to Popup.vue");
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/Popup.vue",
        "should resolve to Popup.vue canonical ID"
    );
    // The component default export anchors at the file start, like Svelte.
    assert_eq!(
        (start, end),
        (0, 0),
        "should anchor at the Popup.vue file start (start={start}, end={end})"
    );
    // Negative: should NOT return index.ts
    assert_ne!(
        canonical_id, "/project/index.ts",
        "must NOT return the barrel file itself"
    );
}

#[test]
fn get_export_span_follows_two_level_reexport_to_svelte_default() {
    let host = make_host();

    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "/project/BarrelChild.svelte".to_string(),
        source: Arc::from(
            "<script lang=\"ts\">\nlet { label }: { label: string } = $props();\n</script>\n<p>{label}</p>",
        ),
        file_language: FileLanguage::svelte(),
        aliases: Vec::new(),
    })
    .expect("load Svelte child");
    upsert_ts(
        &host,
        "/project/level-one.ts",
        "export { default as BarrelChild } from './BarrelChild.svelte';\n",
    );
    upsert_ts(
        &host,
        "/project/level-two.ts",
        "export * from './level-one';\n",
    );

    let result = host
        .get_export_span_follow_reexports("/project/level-two.ts", "BarrelChild")
        .expect("two export hops must reach the Svelte component default");
    assert_eq!(result, ("/project/BarrelChild.svelte".to_string(), 0, 0));
}

#[test]
fn get_export_span_follows_named_reexport() {
    let host = make_host();

    // Target: utils.ts with an exported function
    upsert_ts(
        &host,
        "/project/utils.ts",
        "export function helper() { return 42 }",
    );

    // Barrel: re-exports helper as myHelper
    upsert_ts(
        &host,
        "/project/index.ts",
        "export { helper as myHelper } from './utils.ts'",
    );

    let result = host.get_export_span_follow_reexports("/project/index.ts", "myHelper");

    assert!(result.is_some(), "should follow named re-export");
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/utils.ts",
        "should resolve to utils.ts"
    );
    assert!(start < end, "should have a valid span");
    // Negative: should NOT return barrel
    assert_ne!(canonical_id, "/project/index.ts");
}

#[test]
fn get_export_span_follows_multi_hop_chain() {
    let host = make_host();

    upsert_ts(&host, "/project/a.ts", "export { b } from './b.ts'");
    upsert_ts(&host, "/project/b.ts", "export { c as b } from './c.ts'");
    upsert_ts(&host, "/project/c.ts", "export const c = 42");

    // Should follow a→b→c (no depth limit, cycle detection only)
    let result = host.get_export_span_follow_reexports("/project/a.ts", "b");
    assert!(result.is_some(), "should follow the chain");
    let (canonical_id, _, _) = result.unwrap();
    assert_eq!(canonical_id, "/project/c.ts", "should reach c.ts");
}

#[test]
fn follow_reexport_indirect_cycle() {
    let host = make_host();

    // A→B→C→A with same binding name "x" at each hop
    upsert_ts(&host, "a.ts", "export { x } from './b.ts'");
    upsert_ts(&host, "b.ts", "export { x } from './c.ts'");
    upsert_ts(&host, "c.ts", "export { x } from './a.ts'");

    let result = host.get_export_span_follow_reexports("a.ts", "x");
    assert!(
        result.is_none(),
        "indirect 3-file cycle should return None, got: {result:?}"
    );
}

#[test]
fn follow_reexport_deep_chain_no_limit() {
    let host = make_host();

    // 15-hop chain: f0→f1→f2→...→f14→terminal.ts
    // Each hop renames: val0→val1→...→val14→val
    for i in 0..15 {
        let next = if i < 14 {
            format!("f{}.ts", i + 1)
        } else {
            "terminal.ts".to_string()
        };
        let next_binding = if i < 14 {
            format!("val{}", i + 1)
        } else {
            "val".to_string()
        };
        let src = format!(
            "export {{ {} as val{} }} from './{}'",
            next_binding, i, next
        );
        upsert_ts(&host, &format!("/project/f{}.ts", i), &src);
    }
    upsert_ts(&host, "/project/terminal.ts", "export const val = 'done'");

    let result = host.get_export_span_follow_reexports("/project/f0.ts", "val0");
    assert!(
        result.is_some(),
        "15-hop chain should resolve without depth limit"
    );
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/terminal.ts",
        "should reach terminal.ts"
    );
    assert!(start < end, "should have a valid span");
}

#[test]
fn prop_shorthand_detected() {
    let host = make_host();
    upsert_vue(
        &host,
        "MyComp.vue",
        "<script setup>\ndefineProps<{ bar: number }>()\n</script>\n<template><div/></template>",
    );
    // `:bar` with no value → shorthand; `:bar="bar"` → not shorthand
    upsert_vue(
        &host,
        "App.vue",
        r#"<script setup>
import MyComp from './MyComp.vue'
const bar = 1
</script>
<template><MyComp :bar /><MyComp :bar="bar" /></template>"#,
    );
    compile_template(&host, "App.vue");

    let analysis = host.get_analysis("App.vue").unwrap();
    let tmpl = analysis
        .template
        .as_ref()
        .expect("should have template analysis");
    assert!(
        tmpl.components.len() >= 2,
        "should have at least 2 component usages, got {}",
        tmpl.components.len()
    );

    // First usage: `:bar` (shorthand)
    let comp1 = &tmpl.components[0];
    assert_eq!(comp1.props.len(), 1, "first usage has 1 prop");
    assert!(
        comp1.props[0].is_shorthand,
        "`:bar` (no value) should be shorthand"
    );

    // Second usage: `:bar="bar"` (not shorthand)
    let comp2 = &tmpl.components[1];
    assert_eq!(comp2.props.len(), 1, "second usage has 1 prop");
    assert!(
        !comp2.props[0].is_shorthand,
        "`:bar=\"bar\"` should NOT be shorthand"
    );
}

#[test]
fn prop_name_span_covers_name() {
    let host = make_host();
    upsert_vue(
        &host,
        "MyComp.vue",
        "<script setup>\ndefineProps<{ bar: number }>()\n</script>\n<template><div/></template>",
    );
    let sfc = r#"<script setup>
import MyComp from './MyComp.vue'
const bar = 1
</script>
<template><MyComp :bar="bar" foo="static" /></template>"#;
    upsert_vue(&host, "App.vue", sfc);
    compile_template(&host, "App.vue");

    let analysis = host.get_analysis("App.vue").unwrap();
    let tmpl = analysis
        .template
        .as_ref()
        .expect("should have template analysis");
    assert!(!tmpl.components.is_empty());

    let comp = &tmpl.components[0];
    // Find the bound prop `:bar`
    let bound_prop = comp.props.iter().find(|p| p.name == "bar").unwrap();
    let source = host.get_source("App.vue").unwrap();
    let name_text = &source[bound_prop.name_span.start as usize..bound_prop.name_span.end as usize];
    assert_eq!(
        name_text, "bar",
        "name_span should cover 'bar' (the arg, not ':')"
    );
    assert!(
        bound_prop.name_span.start >= bound_prop.span.start,
        "name_span should be within the full prop span"
    );

    // Find the static prop `foo`
    let static_prop = comp.props.iter().find(|p| p.name == "foo").unwrap();
    let name_text =
        &source[static_prop.name_span.start as usize..static_prop.name_span.end as usize];
    assert_eq!(name_text, "foo", "static prop name_span should cover 'foo'");
    assert!(
        !static_prop.is_shorthand,
        "static prop should not be shorthand"
    );
}

#[test]
fn arc_shared_fields_are_pointer_equal() {
    let host = make_host();
    upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

    let a1 = host.get_analysis("App.vue").unwrap();
    let a2 = host.get_analysis("App.vue").unwrap();

    // Arc-shared fields should be pointer-equal between two calls
    // on the same unchanged file.
    assert!(
        Arc::ptr_eq(&a1.module_references, &a2.module_references),
        "module_references should be Arc-shared (pointer equal)"
    );
    assert!(
        Arc::ptr_eq(&a1.macros, &a2.macros),
        "macros should be Arc-shared (pointer equal)"
    );
    assert!(
        Arc::ptr_eq(&a1.styles, &a2.styles),
        "styles should be Arc-shared (pointer equal)"
    );
    assert!(
        Arc::ptr_eq(&a1.vue_api_calls, &a2.vue_api_calls),
        "vue_api_calls should be Arc-shared (pointer equal)"
    );
}

/// resolve_component_meta(Expanded) merges props from intersection types
#[test]
fn enrich_intersection_merges_all_deps() {
    let host = make_host();
    upsert_non_sfc(&host, "/src/a.ts", "export interface A { x: string }");
    upsert_non_sfc(&host, "/src/b.ts", "export interface B { y: number }");
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { A } from './a'
import type { B } from './b'
defineProps<A & B>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta(
            "/src/Comp.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("should return resolved state");
    let names = hm_prop_names(&host, "/src/Comp.vue", &state);
    assert!(
        names.contains(&"x".to_string()),
        "should have 'x' from A: {:?}",
        names
    );
    assert!(
        names.contains(&"y".to_string()),
        "should have 'y' from B: {:?}",
        names
    );
}

/// resolve_component_meta(Expanded) wraps call-signature emit payloads in brackets
#[test]
fn enrich_emit_call_signature_wraps_brackets() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/events.ts",
        "export interface Events { (e: 'change', id: number): void }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Events } from './events'
defineEmits<Events>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta(
            "/src/Comp.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("should return resolved state");
    let emit_dtos = dtos_for_kind(
        &host,
        "/src/Comp.vue",
        &state,
        verter_session_query::analysis::types::AnalyzedMacroKind::DefineEmits,
    );
    let emits: Vec<_> = emit_dtos
        .iter()
        .flat_map(|d| d.emit_fields().iter())
        .collect();
    let change = emits.iter().find(|e| e.name == "change");
    assert!(change.is_some(), "should have 'change' emit");
    let payload = change.unwrap().payload_type.as_deref().unwrap_or("");
    assert!(
        payload.starts_with('[') && payload.ends_with(']'),
        "call-signature payload should be wrapped in brackets, got: {payload}"
    );
}

/// `:class="props.variant"` union resolution through the lazy template lane:
/// the props-root binding of `const props = withDefaults(defineProps<T>(), …)`
/// lives on the OUTER `WithDefaults` macro (the inner `DefineProps` records
/// `None`) and must reach the converter.
#[test]
fn with_defaults_bound_props_root_drives_dynamic_class_resolution() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Comp.vue",
        r#"<script setup lang="ts">
const props = withDefaults(defineProps<{ variant: 'primary' | 'secondary' }>(), { variant: 'primary' });
</script>
<template><div :class="props.variant" /></template>"#,
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let tpl = analysis
        .template
        .expect("template analysis should be populated");
    let classes: Vec<&str> = tpl
        .elements
        .iter()
        .flat_map(|e| e.dynamic_classes.iter().map(String::as_str))
        .collect();
    assert!(
        classes.contains(&"primary") && classes.contains(&"secondary"),
        "`:class=\"props.variant\"` must resolve through the bound withDefaults root, \
         got: {classes:?}"
    );
}

/// A subject's own semantic decision is final: a DIFFERENT subject that happens
/// to share its label may never supply a domain the facts refused to assert.
///
/// `props.variant` (a closed prop union) and a bare local `variant` are two
/// distinct subjects. When the local's own row is `Unresolved` or `NotClosed`,
/// the bare `:class="variant"` position must publish NOTHING — never the
/// same-named prop's closed domain.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_bare_subject_never_inherits_a_same_named_prop_domain() {
    let host = strict_host();

    // (1) The local's own row is UNRESOLVED (missing dependency).
    let unresolved = "/workspace/src/BareUnresolved.vue";
    upsert_vue(
        &host,
        unresolved,
        r#"<script setup lang="ts">
import type { Later } from './later-absent'
const props = defineProps<{ variant: 'primary' | 'secondary' }>()
const variant: Later = null as never
</script><template>
  <div :class="props.variant" />
  <span :class="variant" />
</template>"#,
    );
    let template = host
        .get_analysis(unresolved)
        .expect("analysis")
        .template
        .expect("template");
    assert_eq!(
        template.elements[0].dynamic_classes,
        ["primary", "secondary"],
        "the prop subject still publishes its own closed domain"
    );
    assert!(
        template.elements[1].dynamic_classes.is_empty(),
        "a bare local the resolver reported UNRESOLVED must publish no closed \
         domain — least of all the same-named prop's"
    );

    // (2) The local's own row is merely OPEN (`NotClosed`) — the true domain is
    // not the prop's, so inheriting it would mis-attribute CSS class usage.
    let open = "/workspace/src/BareOpen.vue";
    upsert_vue(
        &host,
        open,
        r#"<script setup lang="ts">
const props = defineProps<{ variant: 'primary' | 'secondary' }>()
const variant: string = props.variant + '-x'
</script><template>
  <div :class="props.variant" />
  <span :class="variant" />
</template>"#,
    );
    let open_template = host
        .get_analysis(open)
        .expect("analysis")
        .template
        .expect("template");
    assert_eq!(
        open_template.elements[0].dynamic_classes,
        ["primary", "secondary"],
        "the prop subject still publishes its own closed domain"
    );
    assert!(
        open_template.elements[1].dynamic_classes.is_empty(),
        "a bare local whose own domain is OPEN must publish no closed domain"
    );

    // ARMING CONTROL: a bare-requested subject that DOES resolve still
    // publishes, so the two `is_empty()` assertions above are real refusals
    // rather than a bare lookup that never works.
    let resolves = "/workspace/src/BareResolves.vue";
    upsert_vue(
        &host,
        resolves,
        r#"<script setup lang="ts">
const props = defineProps<{ variant: 'primary' | 'secondary' }>()
const local: 'own-a' | 'own-b' = 'own-a'
</script><template>
  <div :class="props.variant" />
  <span :class="local" />
  <em :class="variant" />
</template>"#,
    );
    let resolves_template = host
        .get_analysis(resolves)
        .expect("analysis")
        .template
        .expect("template");
    assert_eq!(
        resolves_template.elements[0].dynamic_classes,
        ["primary", "secondary"]
    );
    assert_eq!(
        resolves_template.elements[1].dynamic_classes,
        ["own-a", "own-b"],
        "a bare local with its own closed domain publishes it"
    );
    assert_eq!(
        resolves_template.elements[2].dynamic_classes,
        ["primary", "secondary"],
        "a bare-REQUESTED prop (no local binding shadows it) still resolves \
         through the prop surface"
    );
}

#[test]
fn mutual_recursive_union_domain_is_typed_cycle_and_return_only() {
    let host = make_host();
    let canonical = "/workspace/src/Cycle.vue";
    upsert_vue(
        &host,
        canonical,
        r#"<script setup lang="ts">
type A = 'a' | B
type B = 'b' | A
const variant: A = null as never
</script><template><div :class="variant" /></template>"#,
    );
    let analysis = host.get_analysis(canonical).expect("analysis");
    assert!(analysis.template.expect("template").elements[0]
        .dynamic_classes
        .is_empty());
    let facts = template_class_facts_for(&host, canonical);
    assert_eq!(
        facts.completeness(),
        verter_session_query::analysis::template_class_facts::TemplateClassFactsCompleteness::ReturnOnly
    );
    assert!(matches!(
        facts.rows()[0].domain,
        verter_type_expr::ClosedLiteralDomain::Unresolved {
            reason: verter_type_expr::ClosedLiteralDomainUnresolvedReason::Cycle,
            ..
        }
    ));
}

/// A6-01 — every direct wrapper role in the closed Vue vocabulary is decided
/// STRUCTURALLY, from the lowered authored return head plus a composed
/// package-backed route proof. Restoring a type-text prefix classifier collapses
/// `ComputedRef`/`ShallowRef`/`ModelRef` onto `Ref`, cannot produce
/// `WritableComputedRef → ComputedRef`, and publishes no provenance at all.
#[test]
fn return_wrapper_roles_cover_the_exact_vue_vocabulary_structurally() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        RETURN_WRAPPER_VUE_DTS,
    );
    let cases = [
        ("Ref", verter_type_expr::ReactiveWrapperRole::Ref),
        (
            "ShallowRef",
            verter_type_expr::ReactiveWrapperRole::ShallowRef,
        ),
        (
            "ComputedRef",
            verter_type_expr::ReactiveWrapperRole::ComputedRef,
        ),
        // The vocabulary NORMALIZATION: a distinct authored export folds onto
        // the same role.
        (
            "WritableComputedRef",
            verter_type_expr::ReactiveWrapperRole::ComputedRef,
        ),
        ("ModelRef", verter_type_expr::ReactiveWrapperRole::ModelRef),
        ("Reactive", verter_type_expr::ReactiveWrapperRole::Reactive),
        (
            "ShallowReactive",
            verter_type_expr::ReactiveWrapperRole::ShallowReactive,
        ),
    ];
    for (index, (wrapper, expected_role)) in cases.into_iter().enumerate() {
        let canonical = format!("/workspace/src/direct{index}.ts");
        upsert_ts(
            &host,
            &canonical,
            &format!(
                "import type {{ {wrapper} }} from 'vue'\n\
                 export function getValue(): {wrapper}<number> {{ return null as never; }}\n"
            ),
        );
        host.set_import_dependencies(
            &canonical,
            vec![exact_dependency(
                "vue",
                "/workspace/node_modules/vue/index.d.ts",
            )],
        );
        let (role, provenance) = return_wrapper_role_for(&host, &canonical, "getValue");
        assert_eq!(role, expected_role, "{wrapper} must classify structurally");
        let provenance = provenance
            .unwrap_or_else(|| panic!("{wrapper} must publish a complete route provenance"));
        assert_eq!(provenance.terminal_import_source.as_ref(), "vue");
        assert_eq!(provenance.package.as_ref(), "vue");
        assert_eq!(provenance.import_source.as_ref(), "vue");
        assert_eq!(provenance.local_binding.as_ref(), wrapper);
        assert_eq!(provenance.imported_name.as_ref(), wrapper);
        assert_eq!(provenance.owner_canonical.as_ref(), canonical);
        // The published head is the AUTHORED spelling, never the terminal name.
        assert!(
            matches!(
                &provenance.authored_head,
                verter_type_expr::facts::AuthoredReferenceHeadFact::Bare { local_name, .. }
                    if local_name.as_ref() == wrapper
            ),
            "{wrapper} provenance must carry the authored head, got {:?}",
            provenance.authored_head
        );
        // Negative: the role is never the fail-closed default, and never `None`.
        assert!(!matches!(
            role,
            verter_type_expr::ReactiveWrapperRole::Unresolved { .. }
                | verter_type_expr::ReactiveWrapperRole::None
        ));
    }
}

/// Producer contract: publishing the canonical post-parse artifact mints NO
/// authored return head (zero declaration bodies lower), and carrying the head
/// to the prepared declaration is a COPY — no locator deref, no import
/// resolution, no semantic dispatch at the producer boundary.
#[test]
fn signature_return_head_is_minted_without_extra_lowering_or_resolution() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/vue/index.d.ts",
        RETURN_WRAPPER_VUE_DTS,
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        RETURN_WRAPPER_VUE_DTS,
    );
    let canonical = "/workspace/src/producer.ts";
    upsert_ts(
        &host,
        canonical,
        "import type { Ref } from 'vue'\n\
         export type Filler0 = { v: 0 }\n\
         export type Filler1 = { v: 1 }\n\
         export function getValue(): Ref<number> { return null as never; }\n",
    );
    host.set_import_dependencies(
        canonical,
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );

    host.provenance().reset();
    ws.reset_reads();
    let indexed = host
        .ensure_indexed_ready(canonical)
        .expect("artifact must materialise");
    assert!(indexed.shallow_state.has_value_symbol("getValue"));
    assert_eq!(
        host.provenance().snapshot().decl_bodies_lowered,
        0,
        "publishing IndexedReady must lower ZERO declaration bodies, so it mints \
         ZERO authored return heads"
    );
    // `read_count` is inert for an UPSERTED canonical (served from the host's
    // own store, `read_file` never fires), so the load-bearing rail is the
    // artifact store: upserting does not index, so any producer-side resolution
    // of the import would have to materialise the dependency's IndexedReady.
    assert!(
        host.project_type_store()
            .indexed()
            .get_any("/workspace/node_modules/vue/index.d.ts")
            .is_none(),
        "the producer boundary must resolve nothing — the vue dependency must \
         not be indexed by minting the head"
    );

    host.provenance().reset();
    let prepared = host
        .prepared_value_decl_in(
            canonical,
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "getValue",
        )
        .expect("prepared value decl");
    let head = &prepared.signatures[0].return_reference_head;
    assert!(
        matches!(
            head,
            verter_type_expr::facts::AuthoredReferenceHeadFact::Bare { local_name, args }
                if local_name.as_ref() == "Ref" && args.len() == 1
        ),
        "the prepared declaration carries the producer-minted head by copy, got {head:?}"
    );
    assert_eq!(
        host.provenance().snapshot().decl_bodies_lowered,
        1,
        "preparing the requested declaration lowers exactly its own body — never \
         the file's unrelated declarations"
    );
    assert!(
        host.project_type_store()
            .indexed()
            .get_any("/workspace/node_modules/vue/index.d.ts")
            .is_none(),
        "carrying the head to the prepared declaration must not resolve the \
         import — the dependency stays unindexed"
    );
    // Positive control: the probe itself can detect indexing — materialise the
    // dependency deliberately and the same probe flips.
    host.ensure_indexed_ready("/workspace/node_modules/vue/index.d.ts")
        .expect("control materialisation");
    assert!(
        host.project_type_store()
            .indexed()
            .get_any("/workspace/node_modules/vue/index.d.ts")
            .is_some(),
        "control: the unindexed probe must be capable of detecting an index"
    );
}

/// PRECISION — a whole-return role answers a different question from a
/// destructured member's reactivity, so it must never decide one. `const { count
/// } = useCounter()` publishes NO role and keeps `MaybeRef`, while the
/// whole-value form on the SAME composable in the SAME file resolves exactly —
/// the control that proves the gate discriminates rather than disabling demand.
///
/// Discriminating mutation: drop the `binds_whole_call_result` gate (or stop
/// clearing the flag in `extract_destructured_bindings`) and `count` becomes
/// `Ref`.
#[test]
fn component_meta_destructured_member_is_not_decided_by_the_whole_return_role() {
    let host = make_host();
    let owner = "/workspace/src/Destructured.vue";
    upsert_vue(
        &host,
        owner,
        "<template><div>{{ count }}{{ whole }}</div></template>\n\
         <script setup lang=\"ts\">\n\
         import { useCounter } from './composables'\n\
         const { count } = useCounter()\n\
         const whole = useCounter()\n\
         </script>\n",
    );
    a6_wire_composable_host(
        &host,
        owner,
        "/workspace/src/composables.d.ts",
        A6_BODILESS_COMPOSABLE_DTS,
    );

    let meta = host.get_component_meta(owner).expect("component meta");
    let count = a6_binding(&meta, "count");
    assert_eq!(
        count.return_wrapper_role, None,
        "a destructured member must never be demanded a WHOLE-return role"
    );
    assert_eq!(
        count.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::MaybeRef,
        "a destructured member keeps its value-space classification"
    );
    // Control on the same fixture: the whole-value form DOES resolve, so the
    // gate is discriminating and not merely switching the demand off.
    let whole = a6_binding(&meta, "whole");
    assert_eq!(
        whole.return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::Ref),
        "control: the whole-value binding on the same composable resolves exactly"
    );
}

/// A COLD artifact store is not a fence.
///
/// The lazy `raw_template_analysis_for_file` lane attests
/// `store_published = true` — live scheduler reads joined at one generation.
/// Whether the content-addressed `FileArtifactStore` happens to hold an
/// `IndexedReady` at that `whole_hash` yet is a CACHE-WARMTH fact: not an
/// overlay, not a fenced input, and not one of the enumerated `ReturnOnly`
/// triggers (overflow, budget exhaustion, cancellation, generation
/// supersession, incomplete self-rooting, unresolved provenance). Folding it
/// into the attestation made a zero-`:class` file's class-fact set
/// `ReturnOnly`, which made `complete_dependency_signature()` `None`, which
/// made `RawTemplateSlotAdmission::admitted_generation()` decline — so a file
/// with NO class content lost its raw-template persist entirely.
///
/// POSITIVE: the persist happens on a cold store, and the recorded signature
/// is EMPTY — a dependency-free fact set is an empty PRESENT signature, never
/// an absent one (empty and overflowed/absent are different states).
///
/// NEGATIVE (same test): an otherwise identical file whose only class subject
/// is unresolvable still declines the slot — the fix restored the persist
/// without widening what `ReturnOnly` declines.
#[test]
fn cold_artifact_store_does_not_fence_a_store_published_class_fact_set() {
    let host = make_host();

    let zero_subject = "/workspace/src/ColdZeroSubject.vue";
    upsert_vue(
        &host,
        zero_subject,
        r#"<script setup lang="ts">
const label = 'plain'
</script><template><div>{{ label }}</div></template>"#,
    );

    // FIXTURE INVARIANT: the lane must run against a COLD artifact store, so
    // it takes the cold-seed fork. A warm `IndexedReady` would take the base
    // fork and the test would prove nothing about the cold classification.
    let whole_hash = current_whole_hash(&host, zero_subject);
    assert!(
        host.exact_current_indexed_for_test(zero_subject, whole_hash)
            .is_none(),
        "fixture invariant: no IndexedReady may be cached at the file's current \
         whole_hash before the lazy lane runs",
    );

    let template = host
        .raw_template_analysis_for_file(zero_subject)
        .expect("the store-published lazy lane must serve its template");
    assert!(
        template
            .elements
            .iter()
            .all(|element| element.dynamic_classes.is_empty()),
        "fixture invariant: the file carries no `:class` subject at all",
    );

    let (_, signature) = persisted_raw_template(&host, zero_subject).expect(
        "a store-published lane must persist its raw template even when the \
         artifact store is COLD — artifact-cache warmth is not a fence and \
         cannot decide publication scope",
    );
    assert!(
        !signature.overflowed,
        "a dependency-free class-fact set is not an overflow",
    );
    assert!(
        signature.facts.is_empty(),
        "a dependency-free class-fact set records an EMPTY PRESENT signature; \
         got {} facts",
        signature.facts.len(),
    );

    // NEGATIVE half — the decline is not widened. The same host, the same
    // lane, the same store-published attestation: a file whose ONLY class
    // subject is unresolvable has no rail that could fire when the dependency
    // later arrives, so it must still decline.
    let unresolvable = "/workspace/src/ColdUnresolvableSubject.vue";
    upsert_vue(
        &host,
        unresolvable,
        r#"<script setup lang="ts">
import type { Absent } from './absent-module'
const variant: Absent = null as never
</script><template><div :class="variant" /></template>"#,
    );
    let unresolvable_template = host
        .raw_template_analysis_for_file(unresolvable)
        .expect("the lane still serves the template");
    assert!(
        unresolvable_template.elements[0].dynamic_classes.is_empty(),
        "an unresolvable class subject publishes no closed domain",
    );
    assert!(
        persisted_raw_template(&host, unresolvable).is_none(),
        "an unresolvable class row keeps the raw-template slot DECLINED — \
         admitting it would serve a stale empty domain forever",
    );
}

/// THE DIRECT GUARD on the ratified sentence: "overlay or fenced inputs remain
/// return-only and cannot populate base caches".
///
/// Three arms, all NEGATIVE, one per producer of a genuine fence. Each arm
/// first proves its lane still RESOLVES and SERVES the closed domain, so the
/// decline is a publication fence and not a resolution failure — a decline that
/// happened because nothing resolved would guard nothing.
///
/// 1. content-override bytes (`compute_override_template_analysis` →
///    `build_template_analysis`);
/// 2. store-published bytes over a NON-CURRENT cold seed
///    (`ColdSeedHostStoreView::is_current() == false`) with a fully resolvable
///    closed domain — no base slot AND no pure-content entry;
/// 3. session-overlay bytes (`get_analysis_via_view`, `store_published == false`).
///
/// WHICH ARM CATCHES WHICH REGRESSION — recorded, because the three arms are
/// NOT equally railed, and claiming otherwise would overstate them.
///
/// * Arm 2's base-slot decline is carried by the class-fact publication scope
///   ALONE: removing the fenced arm from the builder turns it RED, as does
///   making the scope ignore the seed's currentness. It is the discriminating
///   arm for this rail.
/// * Arms 1 and 3 are genuinely fenced lanes whose base-slot decline is held by
///   rails OUTSIDE the class-fact scope, so no scope mutation can falsify them:
///   `build_template_analysis` (arm 1) has no persist site at all, and the
///   overlay builder (arm 3) attests `source_generation: None` on top of
///   `store_published: false` — either alone declines. They are therefore
///   characterization guards on the ratified sentence (they fire if a future
///   change adds a persist site to the override lane, or lets an overlay lane
///   attest a node generation), not discriminators of this change. Their
///   non-vacuity halves — each fenced lane still RESOLVES and SERVES its own
///   closed domain — are the assertions that discriminate here: a fence that
///   suppressed resolution rather than publication would fail them.
#[cfg(not(target_arch = "wasm32"))]
#[test]
#[should_panic(expected = "CorrelationMismatch")]
fn fenced_and_overridden_class_fact_lanes_still_never_populate_the_base_slot() {
    const SOURCE: &str = r#"<script setup lang="ts">
type Variant = 'primary' | 'secondary'
const variant: Variant = 'primary'
</script><template><div :class="variant" /></template>"#;

    // ── Arm 1: content-override bytes ──
    {
        let host = make_host();
        let canonical = "/workspace/src/FencedOverride.vue";
        upsert_vue(&host, canonical, SOURCE);
        let profile = CompileProfile::default();
        let _ = host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: canonical.to_string(),
                compile_profile: profile,
                overrides: vec![BlockOverrideEntry::unissued_for_test(
                    "<span :class=\"variant\" />",
                )],
            })
            .expect("template override must apply");
        let served = host
            .raw_template_analysis_for_file(canonical)
            .expect("the override lane must serve its own template");
        assert_eq!(
            served.elements[0].dynamic_classes,
            ["primary", "secondary"],
            "arm 1 non-vacuity: the override lane RESOLVES and SERVES the closed \
             domain — it is fenced from publishing, not from resolving",
        );
        assert!(
            persisted_raw_template(&host, canonical).is_none(),
            "arm 1: content-override bytes must never populate the base \
             raw-template slot",
        );
        assert_eq!(
            host.compile_output_pure_content_entry_count(),
            0,
            "arm 1: content-override facts must never seed the pure-content cache",
        );
    }

    // ── Arm 2: store-published bytes over a NON-CURRENT cold seed ──
    {
        let host = make_host();
        let canonical = "/workspace/src/FencedStaleSeed.vue";
        upsert_vue(&host, canonical, SOURCE);
        // The lane must take the COLD-SEED fork, where the seed's own
        // currentness is the second half of the derivation. A warm
        // `IndexedReady` would take the base fork and the seed would never be
        // consulted.
        let whole_hash = current_whole_hash(&host, canonical);
        assert!(
            host.exact_current_indexed_for_test(canonical, whole_hash)
                .is_none(),
            "arm 2 invariant: the artifact store must be cold so the lane takes \
             the cold-seed fork",
        );

        // Force every store-view publish to decline WITHOUT advancing any token
        // dimension, so the read exhausts its bounded retry and hands back a
        // `StoreViewRead::ReturnOnly` — a known-stale seed and nothing else.
        let _ = host.resolver_store_view_read();
        host.bump_store_view_epoch();
        crate::resolver_store::HostStoreView::arm_reset_fence_decline_always_for_tests();
        let seed_is_stale = !host.resolver_store_view_read().is_current_for_tests();
        let served = host.raw_template_analysis_for_file(canonical);
        crate::resolver_store::HostStoreView::disarm_reset_fence_decline_always_for_tests();

        assert!(
            seed_is_stale,
            "arm 2 choreography: the armed knob must produce a known non-current \
             store-view read",
        );
        let served = served.expect("arm 2: a fenced lane still SERVES its caller");
        assert_eq!(
            served.elements[0].dynamic_classes,
            ["primary", "secondary"],
            "arm 2 non-vacuity: the domain is fully resolvable, so the decline \
             below is attributable to the non-current seed alone",
        );
        assert!(
            persisted_raw_template(&host, canonical).is_none(),
            "arm 2: store-published bytes resolved against a KNOWN-STALE seed are \
             return-only — they must not populate the base raw-template slot",
        );
        assert_eq!(
            host.compile_output_pure_content_entry_count(),
            0,
            "arm 2: a non-current seed must not seed the pure-content cache",
        );
    }

    // ── Arm 3: session-overlay bytes ──
    {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
        let canonical = "/workspace/src/FencedOverlay.vue";
        upsert_vue(&host, canonical, SOURCE);

        let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> =
            rustc_hash::FxHashMap::default();
        overlays.insert(
            canonical.to_string(),
            Arc::from(
                r#"<script setup lang="ts">
type Variant = 'overlaid-a' | 'overlaid-b'
const variant: Variant = 'overlaid-a'
</script><template><div :class="variant" /></template>"#,
            ),
        );
        let view = crate::session_view::OverlaidView::new(Arc::clone(&host), overlays);

        let snapshot = host
            .get_analysis_via_view(canonical, &view)
            .expect("the overlay arm must serve the overlay snapshot");
        let served = snapshot
            .template
            .as_ref()
            .expect("the overlay caller must be served a template");
        assert_eq!(
            served.elements[0].dynamic_classes,
            ["overlaid-a", "overlaid-b"],
            "arm 3 non-vacuity: the overlay lane RESOLVES and SERVES the OVERLAY's \
             own closed domain — it is fenced from publishing, not from resolving",
        );
        assert!(
            persisted_raw_template(&host, canonical).is_none(),
            "arm 3: session-overlay bytes must never populate the base \
             raw-template slot (overlay results never populate base caches)",
        );
        assert_eq!(
            host.compile_output_pure_content_entry_count(),
            0,
            "arm 3: overlay facts must never seed the pure-content cache",
        );
    }
}

#[test]
fn effective_target_picks_dts_over_ts_over_js() {
    let res = crate::types::DependencyResolution {
        specifier: "./utils".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: vec![
            "/src/utils.js".to_string(),
            "/src/utils.ts".to_string(),
            "/src/utils.d.ts".to_string(),
        ],
    };
    assert_eq!(
        res.effective_target(),
        Some("/src/utils.d.ts"),
        ".d.ts should have highest priority"
    );
}

#[test]
fn effective_target_picks_ts_over_js() {
    let res = crate::types::DependencyResolution {
        specifier: "./utils".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: vec!["/src/utils.jsx".to_string(), "/src/utils.tsx".to_string()],
    };
    assert_eq!(
        res.effective_target(),
        Some("/src/utils.tsx"),
        ".tsx should win over .jsx"
    );
}

#[test]
fn effective_target_returns_none_when_empty() {
    let res = crate::types::DependencyResolution {
        specifier: "./missing".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: Vec::new(),
    };
    assert_eq!(res.effective_target(), None);
}

#[test]
fn effective_target_prefers_dcts_over_cjs() {
    let res = crate::types::DependencyResolution {
        specifier: "./lib".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: vec!["/lib/index.cjs".to_string(), "/lib/index.d.cts".to_string()],
    };
    assert_eq!(
        res.effective_target(),
        Some("/lib/index.d.cts"),
        ".d.cts should win over .cjs"
    );
}

/// CHARACTERIZATION (RouteDb stale-serve hole 2, review finding 1 facet a —
/// already correct on the INDEXED path; documents WHY no producer change was
/// needed). Review finding 1 facet a hypothesised that a barrel mixing an
/// UNRESOLVABLE `export * from './missing'` with a RESOLVABLE sibling
/// stale-serves because the bare `export *` edge is "not in
/// `required_import_sources` (a bare `export *` has no exported name)". That
/// premise is FALSE for the indexed materialiser: a bare `export *` is captured
/// in `export_signatures` as `ExportSignature { name: "*", reexport_source:
/// Some("./missing"), .. }` (see `verter_semantic::analysis::exports` —
/// `Statement::ExportAllDeclaration`), so it enters `required_import_sources`
/// and `prepared_decl`'s `resolve_missing` records it in `import_routes` as a
/// known-miss `DependencyResolution { resolved_canonical_id: None,
/// possible_canonical_ids: [] }` — WITHOUT any sibling, resolvable or not.
/// the owner's import-route witness therefore already detects the
/// known-miss and re-resolves `./missing` against the live workspace, so the
/// recorded `ImportRoute` fact MOVES the moment `./missing` appears and the
/// cached `Miss` invalidates.
///
/// This test PASSES both pre- and post-fix (no producer change lands for the
/// indexed path); it is retained as a regression guard against a future change
/// that drops `export *` sources from `export_signatures` / the import-route
/// known-miss rail. The genuine facet-b drop bug is fixed + pinned by
/// `route_resolved_via_later_wildcard_not_dropped_by_unresolvable_earlier_wildcard`.
#[test]
fn mixed_barrel_indexed_wildcard_known_miss_already_rooted_via_export_signatures() {
    let host = make_host();
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);

    // A resolvable sibling exists; the wildcard target does NOT yet exist.
    upsert_non_sfc(
        &host,
        "/workspace/present.ts",
        "export type Present = number;\n",
    );
    // Barrel mixes an UNRESOLVABLE `export *` with a RESOLVABLE named reexport.
    upsert_non_sfc(
        &host,
        "/workspace/index.ts",
        "export * from './missing';\nexport { Present } from './present';\n",
    );

    // Force the indexed surface: the resolvable sibling populates
    // `import_routes`, so the route entry is ADMITTED (the bug's precondition —
    // an admitted entry whose recorded fact does NOT track the wildcard
    // known-miss).
    let _ = host.ensure_indexed_ready("/workspace/index.ts");

    // Cold resolve a name ONLY the unresolvable wildcard can provide → MISS.
    let first = host.resolve_named_type_export_target("/workspace/index.ts", "Missing");
    assert_eq!(
        first, None,
        "precondition: Missing must miss while ./missing is unresolvable"
    );

    // The wildcard target appears (provider content unchanged).
    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Missing = string;\n",
    );

    let second = host.resolve_named_type_export_target("/workspace/index.ts", "Missing");
    assert_eq!(
        second,
        Some(("/workspace/missing.ts".to_string(), "Missing".to_string())),
        "the mixed-barrel cached Miss MUST invalidate when ./missing appears — the \
         resolvable sibling's ImportRoute hash does NOT re-resolve the wildcard \
         known-miss, so without rooting the unresolvable wildcard in import_routes \
         the stale Miss is served forever (RouteDb stale-serve hole 2, facet a)"
    );
}

/// DISCRIMINATING regression (route-surface seed, negative-route staleness):
/// the base `build_indexed_route_surface` seed loop must NOT re-bake a
/// `set_import_dependencies` known-miss that is stale against the live file
/// set. A known-miss carries no currency stamp at all precisely so a
/// later file appearance forces a
/// re-resolve — but the seed loop gated only on the POSITIVE stamp sidecar,
/// so the unstamped-positive known-miss seeded unconditionally,
/// `resolve_missing` skipped it (`import_routes.contains_key`), and the
/// rebuilt `IndexedReady` published the stale negative route under a FRESH
/// `edge_generation` — permanently edge-current, never re-resolved.
///
/// The reexport is type-only so the re-resolve flows through the TypeImport
/// lane, which `set_import_dependencies` leaves to the live resolver (its
/// exact-resolution rows pin only the ESM lanes for a known-miss).
///
/// FAILS pre-fix: the refreshed surface still records the known-miss.
/// PASSES post-fix: the stale known-miss is skipped at seed and `./missing`
/// re-resolves to the now-existing target.
#[test]
fn base_seed_does_not_rebake_stale_known_miss_after_target_appears() {
    let host = make_host();
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);

    upsert_non_sfc(
        &host,
        "/workspace/owner.ts",
        "export type { Foo } from './missing';\n",
    );
    // The caller's resolver reports `./missing` as unresolvable — a
    // known-miss admitted at the CURRENT content generation.
    host.set_import_dependencies(
        "/workspace/owner.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./missing".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: Vec::new(),
        }],
    );

    let first = host
        .ensure_indexed_ready("/workspace/owner.ts")
        .expect("owner IndexedReady materialises");
    assert_eq!(
        first
            .shallow_state
            .exports
            .values()
            .filter_map(|export| match export {
                verter_session_query::inputs::shallow::ExportTarget::Reexport {
                    source_specifier,
                    ..
                } => Some(source_specifier.as_str()),
                verter_session_query::inputs::shallow::ExportTarget::Local { .. } => None,
            })
            .collect::<Vec<_>>(),
        vec!["./missing"],
        "precondition: the owner's authored reexport specifier is published"
    );
    assert_eq!(
        host.resolve_type_dependency_canonical_shallow("/workspace/owner.ts", "./missing"),
        None,
        "precondition: while ./missing does not exist the specifier resolves to nothing"
    );

    // The target appears. The owner's own content — hence its published
    // artifact — does NOT change.
    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Foo = string;\n",
    );

    let second = host
        .ensure_indexed_ready("/workspace/owner.ts")
        .expect("owner IndexedReady stays served");
    assert_eq!(
        second.whole_hash, first.whole_hash,
        "the owner's parse artifact is unchanged — this is a dependency-set move"
    );
    assert_eq!(
        host.resolve_type_dependency_canonical_shallow("/workspace/owner.ts", "./missing")
            .as_deref(),
        Some("/workspace/missing.ts"),
        "STALE NEGATIVE ROUTE: a specifier that becomes resolvable must resolve \
         through the live authority — no artifact, seed loop, or host memo may \
         pin the earlier miss"
    );
}

/// INVERTED-POLARITY successor to
/// `import_route_only_artifact_goes_edge_stale_after_generation_advance`.
///
/// That case pinned the import-route-only hole: an artifact whose ONLY
/// cross-file edges lived in `IndexedReady.import_routes` (a caller-pushed
/// route for a specifier the file's own source never names — the SFC
/// external `src=` class) had to be judged edge-STALE on a global
/// `content_generation` advance, because those baked targets were
/// dependency-set-derived.
///
/// The artifact carries no route table now, so there is no such shape and
/// no stamp to judge it by. The property that mattered — a caller-pushed
/// route retargeting must be OBSERVABLE — moved onto the resolve domain,
/// and it is asserted here in both directions: the artifact does NOT stale
/// on an unrelated content-generation move, while the owner's import-route
/// witness DOES stop validating when the pushed specifier retargets.
#[test]
fn caller_pushed_route_retarget_is_witnessed_not_artifact_staled() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/workspace/side.ts", "export const side = 1;\n");
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    upsert_non_sfc(&host, "/workspace/plain.ts", "export const x = 1;\n");
    // Caller-supplied route for a specifier the file's own source never
    // mentions: the owner's authored inventory stays empty.
    host.set_import_dependencies(
        "/workspace/plain.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./side".to_string(),
            resolved_canonical_id: Some("/workspace/side.ts".to_string()),
            possible_canonical_ids: vec!["/workspace/side.ts".to_string()],
        }],
    );

    let indexed = host
        .ensure_indexed_ready("/workspace/plain.ts")
        .expect("plain.ts must materialise an IndexedReady");
    assert!(
        !indexed.shallow_state.has_shallow_cross_file_edges(),
        "fixture: the owner authors NO cross-file edge — the caller push is \
         the only route"
    );
    assert!(
        host.indexed_surface_is_current("/workspace/plain.ts", &indexed),
        "precondition: the freshly built artifact is current"
    );

    let view_before = host.resolver_store_view_read().into_owned_view();
    let witness = host
        .owner_import_route_witness_for_tests("/workspace/plain.ts")
        .expect("the owner must produce a rootable witness");
    assert!(
        !witness.is_empty(),
        "precondition: the caller-pushed specifier must be witnessed — an \
         empty witness would make the retarget assertion vacuous"
    );
    for fact in &witness {
        assert!(
            verter_session_query::facts::store_view::StoreView::validates(&view_before, fact),
            "precondition: {fact:?} must validate against the view it was captured from"
        );
    }

    // A dependency-set change advances content_generation while the
    // owner's content stays put.
    ws.inject_file("/workspace/unrelated.ts", "export const u = 1;\n");

    assert!(
        host.indexed_surface_is_current("/workspace/plain.ts", &indexed),
        "GLOBAL EDGE STAMP REINTRODUCED: the parse artifact bakes no route \
         target, so an unrelated content-generation advance must NOT stale it"
    );

    // Now the pushed specifier's target actually moves.
    host.set_import_dependencies(
        "/workspace/plain.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./side".to_string(),
            resolved_canonical_id: Some("/workspace/unrelated.ts".to_string()),
            possible_canonical_ids: vec!["/workspace/unrelated.ts".to_string()],
        }],
    );
    let view_after = host.resolver_store_view_read().into_owned_view();
    assert!(
        witness.iter().any(
            |fact| !verter_session_query::facts::store_view::StoreView::validates(
                &view_after,
                fact
            )
        ),
        "a caller-pushed route RETARGET must invalidate the owner's witness — \
         that is where the currency the deleted artifact stamp used to carry \
         now lives. Witness: {witness:?}"
    );
}

/// DISCRIMINATING regression (RouteDb stale-serve hole 3): the ESM-fallback
/// effective-target normalization MUST be identical between route traversal
/// (`resolve_route_type_edge`) and stale-entry revalidation
/// (`generation_current_route_resolution` — the type-route lane, i.e. a
/// known-miss or a type/ESM-recorded positive). Route traversal normalized
/// the ESM fallback (mapping a runtime `.js` to its `.d.ts` declaration
/// companion) while the revalidation path kept the raw `source_id`, so the
/// two recorded divergent `ImportRoute` facts — a known-miss re-resolved
/// against the current file set produced one canonical, the actual route
/// resolution produced another, and the dependent cache entry stale-served.
///
/// The scenario forces the ESM fallback: the specifier resolves ONLY under the
/// `EsmImport` kind (no `TypeImport` resolution exists), and the resolved
/// runtime target carries a `.d.ts` companion — so a NORMALIZED fallback
/// returns the declaration and a RAW fallback returns the runtime script.
///
/// FAILS pre-fix: the revalidation path returned the raw
/// `/workspace/runtime.js` while `resolve_route_type_edge` returned the
/// normalized `/workspace/runtime.d.ts`. PASSES post-fix: both return the
/// declaration companion through the single shared route-edge policy.
#[test]
fn esm_fallback_normalization_parity_between_route_edge_and_known_miss() {
    let ws = Arc::new(CountingWorkspace::new());
    let owner = "/workspace/owner.ts";
    ws.inject_file(owner, "export {}\n");
    // Runtime target plus its declaration companion: normalization maps the
    // `.js` to the `.d.ts` via `resolve_eval_dependency_canonical`.
    ws.inject_file("/workspace/runtime.js", "export const runtime = true\n");
    ws.inject_file("/workspace/runtime.d.ts", "export type Runtime = boolean\n");
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    // Resolvable ONLY via `EsmImport` — `TypeImport` resolution of this bare
    // specifier returns `None`, so both code paths fall through to the ESM
    // fallback (the exact site where the policies diverged).
    ws.set_exact_resolutions(
        owner,
        vec![verter_workspace::ExactResolution {
            specifier: "runtimedep".to_string(),
            phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
            kind: verter_session_query::resolution::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/workspace/runtime.js".to_string()),
            possible_canonical_ids: vec!["/workspace/runtime.js".to_string()],
        }],
    );

    let route_edge = host.resolve_route_type_edge(owner, "runtimedep");
    let known_miss = host.generation_current_route_resolution(owner, "runtimedep", None);

    assert_eq!(
        route_edge.as_deref(),
        Some("/workspace/runtime.d.ts"),
        "route traversal normalizes the ESM fallback to the declaration companion"
    );
    assert_eq!(
        known_miss, route_edge,
        "known-miss revalidation MUST apply the SAME ESM-fallback normalization \
         as route traversal — divergent policies record divergent ImportRoute \
         facts and stale-serve a known-miss after the file set changes"
    );
}

/// CRASH regression: an ARTIFACT-ONLY (no scheduler `DerivedRawState`)
/// wildcard barrel whose `edge_generation` is stale must NOT cause a mutual
/// recursion between `artifact_current_indexed` and `ensure_indexed_ready`.
/// The artifact-current reader re-indexes an edge-stale wildcard surface via
/// `ensure_indexed_ready`, whose own artifact fast-path must NOT call back into
/// the re-indexing `artifact_current_indexed` (it peeks the artifact raw and
/// non-recursively, then falls through to the single `materialize` re-index) —
/// otherwise the two bounce forever and overflow the stack.
///
/// Pre-fix: `artifact_current_indexed` → `ensure_indexed_ready` →
/// `artifact_current_indexed` → … → stack overflow (process abort).
/// Post-fix: terminates and re-indexes from the backing source to the FRESH
/// target (`runtime.ts`), never the stale planted `runtime/index.ts` edge.
#[test]
fn artifact_only_wildcard_barrel_edge_stale_does_not_recurse() {
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    ws.inject_file(barrel, "export type * from './runtime';\n");
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    let real = host
        .ensure_indexed_ready(barrel)
        .expect("the barrel materialises");
    assert!(real.shallow_state.has_wildcard_reexports());

    // A genuinely artifact-only canonical: a real backing source in the
    // workspace (so a re-index CAN re-resolve it) but NO scheduler
    // `DerivedRawState` — the artifact is planted directly into the store as a
    // clone of the wildcard barrel's `IndexedReady` (its `edge_generation`
    // captured at the current generation).
    let foreign = "/workspace/foreign_barrel.ts";
    ws.inject_file(foreign, "export type * from './runtime';\n");
    host.project_type_store()
        .indexed()
        .insert(Arc::from(foreign), Arc::clone(&real));

    // A more-specific `./runtime.ts` appears: `content_generation` advances
    // past the planted clone's `edge_generation`, so the planted wildcard
    // surface is now edge-stale.
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");

    // MUST terminate (no stack overflow) and re-index to the FRESH target.
    let result = host
        .artifact_current_indexed(foreign)
        .expect("artifact-only reader serves the wildcard barrel");
    assert!(
        result.shallow_state.has_wildcard_reexports(),
        "the artifact-only reader MUST terminate and serve the wildcard surface — \
         never recurse"
    );
    assert_eq!(
        host.resolve_route_edge_canonical(foreign, "./runtime")
            .as_deref(),
        Some("/workspace/runtime.ts"),
        "the artifact-only owner's wildcard edge resolves to the FRESH \
         ./runtime.ts target — no artifact pins the directory-index answer"
    );
    // The sibling readers over the same canonical also terminate + retarget.
    assert!(host.shallow_file_state(foreign).is_some());
    assert!(host.ensure_indexed_ready(foreign).is_some());
}

#[test]
fn shallow_index_preserves_vue_tsx_source_type() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/types.vue",
        r#"<script lang="tsx">
const Button = () => <button />

export type Props = {
  render: typeof Button
}
</script>
<template><div /></template>"#,
    );

    let indexed = host
        .ensure_indexed_ready("/src/types.vue")
        .expect("tsx vue shallow index should be built from the script block");
    let state = &indexed.shallow_state;

    assert!(
        state.has_type_symbol_in(verter_type_expr::TopLevelOwnerId::module(0), "Props"),
        "tsx shallow analysis should retain exported type symbols from the script block",
    );
    assert!(
        state
            .import_target_in(verter_type_expr::TopLevelOwnerId::module(0), "Button")
            .is_none(),
        "tsx shallow analysis should not invent import targets for local JSX-bearing bindings",
    );
    assert!(
        matches!(
            state.export_target("Props"),
            Some(verter_session_query::inputs::shallow::ExportTarget::Local { owner, symbol_name })
                if *owner == verter_type_expr::TopLevelOwnerId::module(0)
                    && symbol_name == "Props"
        ),
        "local tsx exports should stay local instead of being routed through synthetic reexport edges",
    );
    assert!(
        state
            .required_import_names_in(verter_type_expr::TopLevelOwnerId::module(0), "Props")
            .is_empty(),
        "local tsx-only types should not invent import dependencies",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn route_and_root_resolution_do_not_fall_back_through_frontier() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/index.ts", "export * from './types'\n");
    ws.inject_file(
        "/src/types.ts",
        "export interface Props { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let route = host.resolve_named_type_export_target("/src/index.ts", "Props");
    assert_eq!(
        route,
        Some(("/src/types.ts".to_string(), "Props".to_string())),
        "named export routing should resolve through DB-owned shallow facts without frontier fallback",
    );

    let root = host.resolve_imported_type_root("/src/index.ts", "Props");
    assert_eq!(
        root,
        expected_imported_root(
            "/src/types.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Props",
        ),
        "imported-root proof should reuse the DB-owned route without frontier fallback",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ensure_indexed_ready_for_vue_exports_stays_local() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Link.vue'\nexport * from './Unused.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
import type { LinkProps } from './types'

export interface ButtonProps extends Omit<LinkProps, 'raw'> {
  label?: string
}
</script>
<template><button /></template>"#,
    );
    ws.inject_file(
        "/src/Link.vue",
        r#"<script lang="ts">
export interface LinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><a /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_reads();
    let entry = host
        .ensure_indexed_ready("/src/Button.vue")
        .expect("button dependency should build shallow state");

    // Module facts for Vue files build shallow state with locally declared
    // symbols and export surface. The entry should exist and be non-empty
    // since Button.vue has local exports (ButtonProps).
    assert!(
        !entry.shallow_state.is_empty(),
        "vue module facts should have a populated shallow state",
    );
    assert!(
        entry.shallow_state.exports.contains_key("ButtonProps"),
        "vue module facts should expose locally declared export ButtonProps",
    );
}

#[test]
fn ensure_indexed_ready_defers_prepared_decl_materialization_until_lookup() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"
export interface Props {
  label: string
}

export const defaults: Props = { label: 'ok' }
"#,
    );

    let _entry = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should seed shallow imported state");

    let prepared_type = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("prepared type decl should materialize on demand");
    assert!(
        prepared_type
            .member_index
            .contains_key(&verter_type_engine::semantic_query::PropertyKey::identifier("label")),
        "on-demand prepared type materialization should retain the shallow member index",
    );

    let prepared_value = host
        .prepared_value_decl_in(
            "/src/types.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "defaults",
        )
        .expect("prepared value decl should materialize on demand");
    let annotation_source = prepared_value
        .type_annotation
        .annotation
        .as_ref()
        .unwrap_or_else(|| {
            panic!(
                "on-demand prepared value materialization should retain the annotation source, got {:?}",
                prepared_value.type_annotation
            )
        });
    let annotation_ty = crate::test_only::semantic_source_probe::shallow_type_expr(
        &host,
        "/src/types.ts",
        annotation_source,
    )
    .unwrap_or_else(|| panic!("the prepared value annotation source must shell-materialize"));
    assert!(
        matches!(
            &annotation_ty,
            TypeExpr::Ref { name, .. } if name.as_ref() == "Props"
        ),
        "on-demand prepared value materialization should retain the shallow type annotation, got {annotation_ty:?}",
    );

    assert!(
        host.prepared_type_decl("/src/types.ts", "Props").is_some(),
        "on-demand prepared type materialization should be available through the bundle cache",
    );
    assert!(
        host.prepared_value_decl_in(
            "/src/types.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "defaults",
        )
        .is_some(),
        "on-demand prepared value materialization should be available through the bundle cache",
    );

    let (audit, _cm_counters) = host.component_meta_audit_store_snapshot(None);
    assert_eq!(
        audit.prepared_type_decls, 1,
        "audit store snapshot should count prepared type decls from the bundle cache",
    );
    assert_eq!(
        audit.prepared_value_decls, 1,
        "audit store snapshot should count prepared value decls from the bundle cache",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn get_component_meta_named_barrel_lookup_skips_unrelated_siblings() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { IconProps } from './types'
defineProps<IconProps>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types/index.ts",
        "export * from './icon'\nexport * from './a'\nexport * from './b'\n",
    );
    ws.inject_file(
        "/src/types/icon.ts",
        "export interface IconProps { name: string }\n",
    );
    ws.inject_file(
        "/src/types/a.ts",
        "export interface AProps { unused: boolean }\n",
    );
    ws.inject_file(
        "/src/types/b.ts",
        "export interface BProps { unused: number }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types", "/src/types/index.ts")],
    );
    host.set_import_dependencies(
        "/src/types/index.ts",
        vec![
            exact_dependency("./icon", "/src/types/icon.ts"),
            exact_dependency("./a", "/src/types/a.ts"),
            exact_dependency("./b", "/src/types/b.ts"),
        ],
    );

    ws.reset_reads();
    let meta = host
        .get_component_meta("/src/Consumer.vue")
        .expect("component meta should resolve for the consumer");

    assert!(
        meta.props.iter().any(|prop| prop.name == "name"),
        "resolved props should include IconProps.name, got {:?}",
        meta.props,
    );
    // BFS shallows same-layer barrel siblings by design. Each sibling may be
    // read once for route discovery and once for materialization (two adapters
    // with independent route caches), so the upper bound is 2 per cold request.
    assert!(
        ws.read_count("/src/types/a.ts") <= 2,
        "same-layer sibling should be read at most twice per cold request, got {} reads for /src/types/a.ts",
        ws.read_count("/src/types/a.ts"),
    );
    assert!(
        ws.read_count("/src/types/b.ts") <= 2,
        "same-layer sibling should be read at most twice per cold request, got {} reads for /src/types/b.ts",
        ws.read_count("/src/types/b.ts"),
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Unit tests 4.1–4.4 and Regression guards 4.7–4.11
// from the Fact-Validated Declaration-Surface Cache plan
// ═══════════════════════════════════════════════════════════════════════════════

/// Unit test 1: Bundle fact validation round-trip.
///
/// Verify that `prepared_type_decl` caches bundles (second call returns
/// the same result), and that changing file content via `upsert` invalidates
/// the old bundle so the next lookup returns the updated type.
#[test]
fn bundle_fact_validation_round_trip() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }",
    );
    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should materialize");

    // First lookup — materializes the bundle.
    let first = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare on first lookup");
    assert_eq!(first.root_identity.symbol_name.as_ref(), "Props");

    // Second lookup — should hit cache and return the same result.
    let second = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare on second lookup (cache hit)");
    assert_eq!(
        first.root_identity.canonical_id, second.root_identity.canonical_id,
        "repeated lookups should return the same prepared decl identity",
    );
    assert_eq!(
        first.root_identity.symbol_name, second.root_identity.symbol_name,
        "repeated lookups should return the same symbol name",
    );

    // Change the file content — replace Props with a different shape.
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { title: number }",
    );
    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should re-materialize after content change");

    // New lookup should reflect the updated type.
    let updated = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare after content change");
    assert_eq!(updated.root_identity.symbol_name.as_ref(), "Props");

    // Negative: the old symbol shape should be gone (the surface should have
    // changed). We verify the bundle was invalidated by checking the prepared
    // decl's member index reflects the new content.
    assert!(
        updated
            .member_index
            .contains_key(&verter_type_engine::semantic_query::PropertyKey::identifier("title")),
        "updated prepared decl should contain the new property 'title', got: {:?}",
        updated.member_index.keys().collect::<Vec<_>>()
    );
    assert!(
        !updated
            .member_index
            .contains_key(&verter_type_engine::semantic_query::PropertyKey::identifier("label")),
        "updated prepared decl should NOT contain the old property 'label', got: {:?}",
        updated.member_index.keys().collect::<Vec<_>>()
    );
}

/// Unit test 4: with_declaration_scope parity.
///
/// Verify that component-meta resolution correctly resolves props when the
/// component imports a type from another file. This proves that
/// `with_declaration_scope` correctly builds `import_bindings` from the
/// bundle's `dep_edges` path.
#[test]
fn with_declaration_scope_parity_via_component_meta() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface ImportedProps { label: string; count: number }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { ImportedProps } from './types'
defineProps<ImportedProps>()
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );

    let state = host
        .resolve_component_meta(
            "/src/Comp.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("should return resolved state");
    let props = hm_prop_names(&host, "/src/Comp.vue", &state);
    assert!(
        props.contains(&"label".to_string()),
        "component-meta should resolve 'label' prop via bundle dep_edges path: {:?}",
        props
    );
    assert!(
        props.contains(&"count".to_string()),
        "component-meta should resolve 'count' prop via bundle dep_edges path: {:?}",
        props
    );
    // Negative: no phantom props.
    assert_eq!(
        props.len(),
        2,
        "should have exactly 2 props, no phantom data: {:?}",
        props
    );
}

/// Regression guard 8: Declaration-scoped solving with local closure.
///
/// Verify that local type aliases referencing other local types survive the
/// atomic bundle build — `local_deps` or `name_resolution` must retain the
/// local closure symbols.
#[test]
fn regression_declaration_scoped_solving_with_local_closure() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "type Inner = { x: number }\nexport interface Props { child: Inner }\n",
    );

    let prepared = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare with local closure");

    // The local type `Inner` must appear in local_deps or name_resolution,
    // proving local closure symbols survive the atomic bundle build.
    let has_inner_in_local_deps = prepared.local_deps.iter().any(|dep| dep == "Inner");
    let has_inner_in_name_resolution = prepared.name_resolution.contains_key("Inner");

    assert!(
        has_inner_in_local_deps || has_inner_in_name_resolution,
        "local closure symbol 'Inner' must survive in local_deps ({:?}) or name_resolution ({:?})",
        prepared.local_deps,
        prepared.name_resolution.keys().collect::<Vec<_>>(),
    );

    // Additionally verify Props itself is well-formed.
    assert_eq!(prepared.root_identity.symbol_name.as_ref(), "Props");
    assert_eq!(
        prepared.root_identity.canonical_id.as_ref(),
        "/src/types.ts"
    );
}

/// Regression guard 9: Shallow alias resolution through barrel re-exports.
///
/// A barrel file `export { Props } from './inner'` does not own a local
/// `Props` declaration — it is a re-export. `prepared_type_decl` on
/// the barrel file for `Props` should return `None` because the symbol is not
/// a local declaration of the barrel.
#[test]
fn regression_barrel_reexport_returns_none_for_prepared_decl() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/inner.ts",
        "export interface Props { label: string }",
    );
    upsert_non_sfc(&host, "/src/barrel.ts", "export { Props } from './inner'");
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![exact_dependency("./inner", "/src/inner.ts")],
    );

    let _ = host.ensure_indexed_ready("/src/barrel.ts");

    let prepared = host.prepared_type_decl("/src/barrel.ts", "Props");
    assert!(
        prepared.is_none(),
        "barrel re-exports should NOT produce local prepared decls — Props is owned by inner.ts, not barrel.ts",
    );

    // Positive: the defining file should have the prepared decl.
    let inner_prepared = host.prepared_type_decl("/src/inner.ts", "Props");
    assert!(
        inner_prepared.is_some(),
        "the defining file (inner.ts) should have the prepared decl for Props",
    );
}

/// The surface reached INCREMENTALLY after a generation-advancing edit must be
/// the one a FRESH execution produces on the same basis.
///
/// The generation that decides whether a cached intrinsic surface is still valid
/// moved from the cache KEY onto the cached VALUE. A key-side version axis is
/// self-validating — a differently-generationed entry is simply a different entry
/// — while a value-side one has to be COMPARED, and a comparison that accepts an
/// older stamp serves a superseded surface under a key that looks current.
/// Counting cache keys proves the retention bound; it cannot see that.
///
/// So this pins the other half. The document is driven through repeated edits,
/// each of which advances the workspace content generation AND changes the
/// component's accepted surface, then the complete resolved surface is compared
/// against a host that only ever saw the final revision. Same basis, two
/// execution histories, one required answer.
///
/// The fingerprint is proved revision-sensitive in the test itself (it must name
/// the final revision's prop and none of the superseded ones), so an equality
/// that held because the comparison cannot see a stale answer is excluded.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn incremental_and_fresh_fallthrough_agree_on_the_same_basis() {
    /// The complete resolved surface, order-normalized. `fact_versions` is
    /// provenance about HOW the answer was reached rather than the answer itself,
    /// and is the one field two execution histories may legitimately differ on.
    fn surface_fingerprint(resolution: &crate::types::FallthroughResolution) -> String {
        let mut props: Vec<String> = resolution
            .accepted_props
            .iter()
            .map(|prop| format!("{prop:?}"))
            .collect();
        props.sort();
        let mut events: Vec<String> = resolution
            .accepted_events
            .iter()
            .map(|event| format!("{event:?}"))
            .collect();
        events.sort();
        format!(
            "completeness={:?}\nsurface={:?}\nprops=[{}]\nevents=[{}]",
            resolution.accepted_surface_completeness,
            resolution.fallthrough_surface,
            props.join(", "),
            events.join(", "),
        )
    }

    const EDITS: usize = 8;
    let canonical = "/src/Basis.vue";
    // Every revision declares a DIFFERENT prop, so the accepted surface — and
    // therefore the fingerprint — changes with the content generation.
    let source = |revision: usize| {
        format!(
            r#"<script setup lang="ts">
defineProps<{{ label?: string; rev{revision}?: number }}>()
</script>
<template><div :title="label"></div></template>"#
        )
    };

    let incremental_host = make_host();
    for revision in 0..=EDITS {
        upsert_vue(&incremental_host, canonical, &source(revision));
        assert!(
            incremental_host
                .resolve_fallthrough_surface(canonical)
                .is_some(),
            "revision {revision} must resolve a fallthrough surface"
        );
    }
    let incremental = incremental_host
        .resolve_fallthrough_surface(canonical)
        .expect("the final revision resolves incrementally");

    let fresh_host = make_host();
    upsert_vue(&fresh_host, canonical, &source(EDITS));
    let fresh = fresh_host
        .resolve_fallthrough_surface(canonical)
        .expect("the final revision resolves from cold");

    let incremental_fingerprint = surface_fingerprint(&incremental);
    assert_eq!(
        incremental_fingerprint,
        surface_fingerprint(&fresh),
        "the incrementally reused surface must be the one a fresh execution \
         produces on the same basis"
    );

    // The comparison is only worth anything if it can SEE a superseded answer.
    assert!(
        incremental_fingerprint.contains(&format!("rev{EDITS}")),
        "the fingerprint must name the final revision's declared prop"
    );
    for superseded in 0..EDITS {
        assert!(
            !incremental_fingerprint.contains(&format!("rev{superseded}\"")),
            "the fingerprint must not carry superseded revision {superseded}'s prop — \
             a fingerprint blind to the revision could not discriminate a stale answer"
        );
    }
    assert!(
        !incremental.accepted_props.is_empty(),
        "the comparison must be over a non-empty surface, not two empty results"
    );
}

/// A reader that observed a superseded intrinsic surface must neither erase a
/// NEWER surface a concurrent writer admits while the reader is paused, nor
/// publish its own result — stamped with the generation it sampled BEFORE the
/// writer's advance — over that newer completion.
///
/// The intrinsic surface lives under a stable `(project_anchor, tag)` key with
/// the workspace content generation on the VALUE, so the reader — not the
/// store view — decides staleness, and it decides from a generation sampled
/// some time earlier. Two requests on the same key:
///
/// - Reader A samples the live generation G, reads the warm surface stamped
///   G-1, observes the mismatch — and is paused right there, between that
///   observation and its retirement, through the one-shot `#[cfg(test)]` seam
///   in `intrinsic_members_for_tag` (`pause_before_intrinsic_retirement_for_test`).
/// - Writer B, staged inside the pause, advances the workspace to G+1 with an
///   edit to an unrelated file, retires the G-1 surface as any reader would,
///   and admits a surface stamped G+1 under the same key, carrying a marker
///   member no real projection produces.
/// - A resumes: it retires, computes, and admits.
///
/// Discriminating, per half of the defect (each verified by reverting that
/// half alone):
/// - Unconditional retirement (the former `retire_node`): A's resumed
///   retirement removes the whole slot, so B's G+1 surface is gone. A's own
///   seed went non-current under B's edit, so the request recomputes on a
///   fresh seed and repopulates the slot with a REAL projection stamped G+1 —
///   the generation looks right, and it is the marker-member assertion that
///   fails: the warm surface is a recomputation, not B's completion.
/// - Admission that trusts the pre-compute sample: A's surface lands stamped G
///   behind B's G+1 (the slot appends candidates), so the newest candidate's
///   generation is G rather than G+1 and the candidate count is 2 rather
///   than 1.
///
/// The sequential key count in
/// `repeated_edits_do_not_retain_one_intrinsic_surface_per_generation` cannot
/// see either: it never has two requests in flight on one key.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn paused_intrinsic_reader_never_erases_or_outranks_a_newer_admission() {
    use crate::resolver_core::fallthrough_resolver::intrinsic_surface_key;

    const MARKER: &str = "data-admitted-by-writer-b";
    let canonical = "/src/Paused.vue";
    let source = |revision: usize| {
        format!(
            r#"<script setup lang="ts">
defineProps<{{ label?: string; rev{revision}?: number }}>()
</script>
<template><div :title="label"></div></template>"#
        )
    };

    let host = make_host();
    upsert_vue(&host, canonical, &source(0));
    assert!(
        host.resolve_fallthrough_surface(canonical).is_some(),
        "the fixture must resolve a fallthrough surface for a single native root"
    );
    let (anchor, warm_generation) = host.project_intrinsic_cache_anchor(canonical);
    let key = intrinsic_surface_key(&anchor, "div");
    assert_eq!(
        host.resolver_runtime()
            .fallthrough
            .warm_intrinsic_surface_for_test(&key)
            .map(|(_, generation)| generation),
        Some(warm_generation),
        "the first resolve must warm the div surface at the live generation"
    );

    // One edit to the component: the live generation moves past the warm
    // surface, so reader A will observe a superseded candidate.
    upsert_vue(&host, canonical, &source(1));
    let (_, generation_a) = host.project_intrinsic_cache_anchor(canonical);
    assert!(
        generation_a > warm_generation,
        "the edit must advance the workspace content generation"
    );

    // Writer B's surface: a real member renamed to a marker, so a later read
    // can tell B's completion from anything the projection would compute.
    let mut marker_member = host
        .intrinsic_members_for_tag("div")
        .into_iter()
        .next()
        .expect("the static div surface has at least one member");
    marker_member.name = MARKER.to_string();

    let staged = Arc::new(std::sync::Mutex::new(None));
    let staged_in_hook = Arc::clone(&staged);
    pause_before_intrinsic_retirement_for_test(move |host, paused_key, observed_generation| {
        // A is paused between observing the stale surface and retiring it.
        // B: advance the workspace past A's sample, retire the surface B
        // itself observes as stale (what any reader does), and admit the
        // newer completion under the same stable key.
        upsert_vue(
            host,
            "/src/Elsewhere.vue",
            r#"<template><span>elsewhere</span></template>"#,
        );
        let (_, generation_b) = host.project_intrinsic_cache_anchor(canonical);
        assert!(
            generation_b > observed_generation,
            "B's edit must advance the generation past A's sample"
        );
        host.resolver_runtime()
            .fallthrough
            .retire_superseded_intrinsic_surface(paused_key, generation_b);
        let node = host.build_runtime_intrinsic_surface_node(
            std::slice::from_ref(&marker_member),
            generation_b,
        );
        host.resolver_runtime()
            .fallthrough
            .admit_node_for_test(paused_key.clone(), node);
        *staged_in_hook.lock().unwrap() = Some((observed_generation, generation_b));
    });

    // Reader A: samples G, observes the G-1 surface, pauses (B runs), resumes.
    assert!(
        host.resolve_fallthrough_surface(canonical).is_some(),
        "A must still be served its result after the overtaken retirement"
    );
    let (observed_generation, generation_b) = staged
        .lock()
        .unwrap()
        .expect("the seam must fire: A observes the superseded warm surface before retiring");
    assert_eq!(
        observed_generation, generation_a,
        "A must have sampled the generation live before B's advance"
    );

    let slot_names = |warm: &Option<(Vec<crate::resolver_core::IntrinsicSurfaceMember>, u64)>| {
        warm.as_ref().map(|(members, _)| {
            members
                .iter()
                .map(|member| member.name.clone())
                .collect::<Vec<_>>()
        })
    };
    let warm = host
        .resolver_runtime()
        .fallthrough
        .warm_intrinsic_surface_for_test(&key);
    assert_eq!(
        warm.as_ref().map(|(_, generation)| *generation),
        Some(generation_b),
        "B's newer surface must remain current: A's resumed retirement held a generation-\
         {generation_a} sample and may not erase a generation-{generation_b} completion"
    );
    assert_eq!(
        slot_names(&warm),
        Some(vec![MARKER.to_string()]),
        "the warm surface must be the one B admitted, not a recomputation"
    );
    assert_eq!(
        host.resolver_runtime()
            .fallthrough
            .cached_candidate_count(&key),
        1,
        "A's result, computed across B's advance and stamped with the pre-advance \
         generation, must not be admitted beside B's newer surface"
    );

    // A later read through the production path treats B's completion as
    // current: it is served and the slot is left exactly as B published it.
    assert!(
        host.resolve_fallthrough_surface(canonical).is_some(),
        "a read after the interleaving must still resolve"
    );
    let warm_after = host
        .resolver_runtime()
        .fallthrough
        .warm_intrinsic_surface_for_test(&key);
    assert_eq!(
        warm_after.as_ref().map(|(_, generation)| *generation),
        Some(generation_b),
        "a subsequent reader must keep B's surface as the current one"
    );
    assert_eq!(
        slot_names(&warm_after),
        Some(vec![MARKER.to_string()]),
        "a subsequent reader must not retire or replace B's surface"
    );
    assert_eq!(
        host.resolver_runtime()
            .fallthrough
            .cached_candidate_count(&key),
        1,
        "the slot must hold exactly B's candidate after the subsequent read"
    );
}
