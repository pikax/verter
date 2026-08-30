//! Byte-identity characterization for the framework parse-carrier
//! surface.
//!
//! Pins the observable behavior of every surface the
//! `FrameworkParseArtifact` carrier replacement touches, so the
//! neutral-carrier representation is provably behavior-neutral:
//!
//!  * `HostSourceData.source_type` — the authoritative parse-time
//!    `SourceType` for the full `<script lang>` matrix;
//!  * eval-source building — the position-preserving script-only
//!    source for a two-script SFC, byte-exact;
//!  * content overrides — `apply_block_overrides` round-trips produce
//!    identical analysis snapshots;
//!  * route-owned shallow state — `cached_route_owned_eval_state`
//!    payload presence + content identity;
//!  * IDE virtual output — byte-stable compile output for a fixture
//!    SFC (content-hash pin);
//!  * component-meta — the published props/emits surface for a fixture
//!    SFC.
//!
//! Every expectation is a LITERAL captured from the Vue-typed carrier
//! tree; the suite must stay green, unchanged, on the neutral carrier.

use super::*;
use std::sync::Arc;
use verter_language::FileLanguage;

fn make_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn upsert_vue(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
}

fn upsert_svelte(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::svelte(),
            aliases: Vec::new(),
        })
        .unwrap();
}

fn upsert_script(host: &VerterHost, id: &str, src: &str, language: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: language,
            aliases: Vec::new(),
        })
        .unwrap();
}

/// Compact, comparable rendering of an `oxc_span::SourceType`.
fn render_source_type(st: oxc_span::SourceType) -> String {
    format!(
        "ts={} jsx={} dts={}",
        st.is_typescript(),
        st.is_jsx(),
        st.is_typescript_definition()
    )
}

// ───────────────────── HostSourceData.source_type matrix ─────────────────────

#[test]
fn vue_source_type_matrix_is_stable() {
    // (fixture name, SFC source, expected rendered source type).
    // Expectations were captured from the live parse pipeline; the
    // neutral carrier representation must not move ANY row.
    let matrix: &[(&str, &str, &str)] = &[
        (
            "lang_ts.vue",
            "<script lang=\"ts\">export default {}</script>",
            "ts=true jsx=false dts=false",
        ),
        (
            "lang_tsx.vue",
            "<script lang=\"tsx\">export default {}</script>",
            "ts=true jsx=true dts=false",
        ),
        (
            "lang_jsx.vue",
            "<script lang=\"jsx\">export default {}</script>",
            "ts=false jsx=true dts=false",
        ),
        (
            "lang_js.vue",
            "<script lang=\"js\">export default {}</script>",
            "ts=false jsx=false dts=false",
        ),
        (
            "no_lang.vue",
            "<script>export default {}</script>",
            "ts=true jsx=false dts=false",
        ),
        (
            "no_script.vue",
            "<template><div /></template>",
            "ts=true jsx=false dts=false",
        ),
        (
            "setup_only_tsx.vue",
            "<script setup lang=\"tsx\">const a = 1</script>",
            "ts=true jsx=true dts=false",
        ),
        // Mixed: plain script carries no lang, setup carries one — the
        // first block WITH a lang attribute decides.
        (
            "mixed_setup_lang.vue",
            "<script>export default {}</script>\n<script setup lang=\"tsx\">const a = 1</script>",
            "ts=true jsx=true dts=false",
        ),
        (
            "lang_uppercase_ts.vue",
            "<script lang=\"TS\">export default {}</script>",
            "ts=true jsx=false dts=false",
        ),
    ];

    let host = make_host();
    for (id, src, expected) in matrix {
        upsert_vue(&host, id, src);
        let st = host
            .authoritative_source_type_for(id)
            .unwrap_or_else(|| panic!("authoritative source type must exist for {id}"));
        assert_eq!(
            render_source_type(st),
            *expected,
            "source-type drift for fixture {id}"
        );
    }

    // Plain scripts derive from their classified `FileLanguage` row.
    upsert_script(
        &host,
        "plain.ts",
        "export const a = 1;",
        FileLanguage::script_ts(),
    );
    let st = host
        .authoritative_source_type_for("plain.ts")
        .expect("plain script source type");
    assert_eq!(render_source_type(st), "ts=true jsx=false dts=false");
}

// ───────────────────────── eval-source building ─────────────────────────

#[test]
fn eval_source_for_two_script_sfc_is_position_preserving_and_stable() {
    // Two script blocks with template noise between them. The eval
    // source must be byte-for-byte the same length as the SFC, with
    // script content at its raw offsets and markup blanked.
    let source = "<script lang=\"ts\">const a: number = 1;</script>\n<template><div>x</div></template>\n<script setup lang=\"ts\">const b = a;</script>\n";
    let host = make_host();
    upsert_vue(&host, "Two.vue", source);
    let indexed = host
        .ensure_indexed_ready("Two.vue")
        .expect("indexed ready for Two.vue");

    let eval_source = indexed.eval_source.as_ref();
    assert_eq!(
        eval_source.len(),
        source.len(),
        "eval source must be position-preserving (same byte length)"
    );
    // Literal pin: script bytes verbatim at their offsets, all other
    // bytes blanked, newlines preserved.
    let expected = "                  const a: number = 1;         \n                                 \n                        const b = a;         \n";
    assert_eq!(eval_source, expected, "eval source drifted byte-wise");

    // The raw source is retained verbatim alongside.
    assert_eq!(indexed.raw_source.as_ref(), source);
}

#[test]
fn eval_source_matches_catalog_authority_not_host_extractor() {
    let source = concat!(
        "<script lang=\"ts\">const a = 1</script>",
        "<script setup lang=\"ts\">const b = a</script>",
        "<template><div /></template>",
    );
    let host = make_host();
    upsert_vue(&host, "Adjacent.vue", source);
    let indexed = host
        .ensure_indexed_ready("Adjacent.vue")
        .expect("indexed ready for Adjacent.vue");
    let eval_source = indexed.eval_source.as_ref();
    assert_eq!(eval_source.len(), source.len());
    let a_end = eval_source.find("const a = 1").expect("first script body") + "const a = 1".len();
    assert_eq!(
        eval_source.as_bytes()[a_end],
        b' ',
        "IndexedReady eval-source must keep catalog authority bytes (no injected newline), got: {eval_source:?}"
    );
    assert!(
        eval_source.contains("const b = a"),
        "second script body must survive: {eval_source:?}"
    );
    assert!(
        !eval_source.contains('<'),
        "markup must not leak into eval source: {eval_source:?}"
    );
    let artifact = indexed
        .framework_parse
        .as_ref()
        .expect("Vue IndexedReady retains a parse artifact");
    let via_catalog = crate::parse::catalog_eval_source(artifact.as_ref(), source)
        .expect("semantic catalog serves the Vue artifact");
    assert_eq!(
        indexed.eval_source.as_ref(),
        via_catalog.as_ref(),
        "IndexedReady must store the catalog eval-source bytes"
    );
    let rebuilt =
        VerterHost::build_eval_script_source("Adjacent.vue", source, Some(artifact.as_ref()))
            .expect("catalog hit returns the backend Arc");
    assert_eq!(rebuilt.as_ref(), indexed.eval_source.as_ref());
    let clone = Arc::clone(&rebuilt);
    assert!(
        Arc::ptr_eq(&rebuilt, &clone),
        "eval-source is kept as Arc; cloning must not copy bytes"
    );
    let parsed = crate::typeinfo::adapters::vue::vue_parse(artifact.as_ref())
        .expect("Vue artifact opens through the blessed accessor");
    let host_extract = crate::host_resolve::extract_vue_script_content(source, &parsed)
        .expect("host extractor still produces a test-only projection");
    assert_ne!(
        eval_source,
        host_extract.as_str(),
        "production eval-source must not be the host extractor dual producer"
    );
}

#[test]
fn cold_upsert_and_indexed_ready_share_one_catalog_eval_source_arc() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "const label = 'hi'\n",
        "</script>\n",
        "<template><div>{{ label }}</div></template>\n",
    );
    let host = make_host();
    let before = crate::parse::catalog_eval_source_call_count_for_host(host.instance_id);
    upsert_vue(&host, "One.vue", source);
    let indexed = host
        .ensure_indexed_ready("One.vue")
        .expect("IndexedReady after upsert");
    assert_eq!(
        crate::parse::catalog_eval_source_call_count_for_host(host.instance_id) - before,
        1,
        "source-stage snapshot and IndexedReady are one cold-load request"
    );
    let snap = host
        .scheduler_source("One.vue")
        .expect("source-stage snapshot after upsert");
    let hd = snap
        .downcast_data::<crate::host_executor::HostSourceData>()
        .expect("source-stage HostSourceData");
    assert!(
        Arc::ptr_eq(&hd.eval_source, &indexed.eval_source),
        "IndexedReady must clone the source-stage catalog Arc, not recatalog"
    );
}

/// Generation A: `alphaExport` plus type-based `defineProps`.
const TORN_IDENTITY_SOURCE_A: &str = concat!(
    "<script setup lang=\"ts\">\n",
    "export const alphaExport = 1\n",
    "defineProps<{ msg: string }>()\n",
    "</script>\n",
    "<template><div>{{ alphaExport }}</div></template>\n",
);

/// Generation B — mid-window move: `betaExport` plus type-based `defineEmits`.
/// A coherent A artifact can carry neither B's eval-source Arc, B's
/// export surface, nor B's synthesized `$emit`.
const TORN_IDENTITY_SOURCE_B: &str = concat!(
    "<script setup lang=\"ts\">\n",
    "export const betaExport = 2\n",
    "defineEmits<{ (e: 'close'): void }>()\n",
    "</script>\n",
    "<template><div>{{ betaExport }}</div></template>\n",
);

fn export_names_of(snapshot: &crate::types::FileAnalysisSnapshot) -> Vec<String> {
    snapshot
        .export_signatures
        .iter()
        .map(|sig| sig.name.clone())
        .collect()
}

fn synthesized_default_member_names(state: &crate::resolver_core::ShallowFileState) -> Vec<String> {
    use verter_type_expr::facts::{ResolvedLocalShape, SemanticTypeSource};

    let Some(lowered) = state.value_decl("default") else {
        return Vec::new();
    };
    let Some(source) = lowered.type_annotation.annotation.as_ref() else {
        return Vec::new();
    };
    let SemanticTypeSource::Synthesized(ResolvedLocalShape::Object(members)) = source else {
        return Vec::new();
    };
    members.iter().map(|m| m.name.clone()).collect()
}

/// A source move landing after the cold IndexedReady flight holds
/// SourceSnapshot A, with B waited through Analysis, must not serve an
/// artifact whose content-addressed products describe more than one
/// snapshot. The raced serve is ReturnOnly of coherent A; the quiescent
/// follow-up is published coherent B.
#[test]
fn source_move_between_parse_facts_and_eval_source_never_serves_torn_identity() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let host = Arc::new(make_host());
    upsert_vue(&host, "Torn.vue", TORN_IDENTITY_SOURCE_A);

    let snap_a = host
        .scheduler_source("Torn.vue")
        .expect("source-stage snapshot A after upsert");
    let hd_a = snap_a
        .downcast_data::<crate::host_executor::HostSourceData>()
        .expect("snapshot A HostSourceData");
    let eval_a = Arc::clone(&hd_a.eval_source);
    let hash_a = hd_a.parse.whole_hash;
    let script_analysis_a = Arc::clone(&hd_a.parse.script_analysis);

    let moved = Arc::new(AtomicBool::new(false));
    {
        let hook_host = Arc::clone(&host);
        let moved = Arc::clone(&moved);
        *host.test_force.indexed_source_capture_seam_hook.0.lock() = Some(Arc::new(move || {
            if !moved.swap(true, Ordering::SeqCst) {
                upsert_vue(&hook_host, "Torn.vue", TORN_IDENTITY_SOURCE_B);
                assert!(
                    hook_host.scheduler.try_get_analysis("Torn.vue").is_some(),
                    "choreography sanity: seam upsert B must wait through Analysis"
                );
            }
        }));
    }
    let serve = host
        .ensure_indexed_ready_serve("Torn.vue")
        .expect("the cold flight must still serve its captured snapshot");
    *host.test_force.indexed_source_capture_seam_hook.0.lock() = None;
    assert!(
        moved.load(Ordering::SeqCst),
        "choreography sanity: the seam must have landed the move"
    );

    let snap_b = host
        .scheduler_source("Torn.vue")
        .expect("scheduler holds generation B after the seam upsert");
    let hd_b = snap_b
        .downcast_data::<crate::host_executor::HostSourceData>()
        .expect("snapshot B HostSourceData");
    let eval_b = Arc::clone(&hd_b.eval_source);
    let hash_b = hd_b.parse.whole_hash;
    let script_analysis_b = Arc::clone(&hd_b.parse.script_analysis);
    assert!(
        !Arc::ptr_eq(&eval_a, &eval_b) && hash_a != hash_b,
        "choreography sanity: A and B must be distinct snapshot objects"
    );
    assert!(
        !Arc::ptr_eq(&script_analysis_a, &script_analysis_b),
        "choreography sanity: A and B script-analysis Arcs must be distinct"
    );

    // THE PIN — every content-addressed product is coherent A. A torn
    // serve pairs A's hash/raw/eval with B's script analysis, exports,
    // or synthesized `$emit`.
    assert!(
        !serve.store_published,
        "raced serve must be ReturnOnly after the mid-flight source move"
    );
    let indexed = &serve.indexed;
    assert_eq!(indexed.whole_hash, hash_a);
    assert!(
        Arc::ptr_eq(&indexed.eval_source, &eval_a),
        "IndexedReady eval-source must be snapshot A's Arc"
    );
    assert!(
        Arc::ptr_eq(&indexed.raw_source, &snap_a.source),
        "IndexedReady raw_source must be snapshot A's Arc"
    );
    let script_analysis = indexed
        .script_analysis
        .as_ref()
        .expect("IndexedReady script_analysis is source-bound");
    assert!(
        Arc::ptr_eq(script_analysis, &script_analysis_a),
        "IndexedReady script_analysis must be snapshot A's Arc"
    );
    assert!(
        !Arc::ptr_eq(script_analysis, &script_analysis_b),
        "IndexedReady script_analysis must not be snapshot B's Arc"
    );
    let export_signatures = indexed
        .export_signatures
        .as_ref()
        .expect("IndexedReady export_signatures is source-bound");
    assert!(
        Arc::ptr_eq(export_signatures, &indexed.snapshot.export_signatures),
        "snapshot and IndexedReady must share one export-signatures Arc"
    );
    let exports = export_names_of(&indexed.snapshot);
    assert!(
        exports.iter().any(|e| e == "alphaExport") && !exports.iter().any(|e| e == "betaExport"),
        "snapshot/export surface must be A (alphaExport), never B (betaExport); got {exports:?}"
    );
    assert!(
        indexed.shallow_state.exports.contains_key("alphaExport")
            && !indexed.shallow_state.exports.contains_key("betaExport"),
        "shallow exports must be A's, never B's; got {:?}",
        indexed.shallow_state.exports.keys().collect::<Vec<_>>()
    );
    let default_members = synthesized_default_member_names(&indexed.shallow_state);
    assert!(
        default_members.iter().any(|m| m == "$props")
            && !default_members.iter().any(|m| m == "$emit"),
        "synthesized default must carry A's $props, never B's $emit; got {default_members:?}"
    );

    // Recovery: the quiescent next read is published coherent B.
    let recovered = host
        .ensure_indexed_ready_serve("Torn.vue")
        .expect("the quiescent follow-up read must serve");
    assert!(
        recovered.store_published,
        "quiescent follow-up must publish coherent B"
    );
    let snap_now = host
        .scheduler_source("Torn.vue")
        .expect("scheduler still holds B");
    let hd_now = snap_now
        .downcast_data::<crate::host_executor::HostSourceData>()
        .expect("current HostSourceData");
    let recovered_indexed = &recovered.indexed;
    assert!(
        Arc::ptr_eq(&recovered_indexed.eval_source, &hd_now.eval_source),
        "quiescent IndexedReady must clone the live snapshot's eval-source Arc"
    );
    assert_eq!(recovered_indexed.whole_hash, hd_now.parse.whole_hash);
    assert!(
        Arc::ptr_eq(&recovered_indexed.raw_source, &snap_now.source),
        "quiescent IndexedReady raw_source must be snapshot B's Arc"
    );
    let recovered_sa = recovered_indexed
        .script_analysis
        .as_ref()
        .expect("quiescent script_analysis");
    assert!(
        Arc::ptr_eq(recovered_sa, &hd_now.parse.script_analysis),
        "quiescent script_analysis must be snapshot B's Arc"
    );
    let recovered_exports = export_names_of(&recovered_indexed.snapshot);
    assert!(
        recovered_exports.iter().any(|e| e == "betaExport")
            && !recovered_exports.iter().any(|e| e == "alphaExport"),
        "quiescent export surface must be B (betaExport), never A (alphaExport); got {recovered_exports:?}"
    );
    let recovered_members = synthesized_default_member_names(&recovered_indexed.shallow_state);
    assert!(
        recovered_members.iter().any(|m| m == "$emit")
            && !recovered_members.iter().any(|m| m == "$props"),
        "quiescent synthesized default must carry B's $emit, never A's $props; got {recovered_members:?}"
    );
}

fn source_stage_executor(host: &VerterHost) -> crate::host_executor::HostStageExecutor {
    crate::host_executor::HostStageExecutor::new(
        host.config.clone(),
        Arc::clone(&host.workspace),
        Arc::clone(&host.provenance),
        Arc::clone(&host.carrier_publication.source_authority),
        Arc::clone(&host.carrier_publication.grammar_authority),
        Arc::clone(&host.carrier_publication.publication_store),
        crate::carrier_publication_store::HostInstanceId::new(host.instance_id),
        Arc::clone(&host.carrier_publication.envelope_ingest),
    )
}

/// A forced catalog miss before publication must be `Err(StageError)`:
/// no unwind, no parse/publication/adoption, and no warm IndexedReady.
#[test]
fn pre_publication_semantic_catalog_miss_is_stage_error_without_publish() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use verter_scheduler::executor::{StageErrorKind, StageExecutor};

    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "const label = 'hi'\n",
        "</script>\n",
        "<template><div>{{ label }}</div></template>\n",
    );
    let host = make_host();
    let executor = source_stage_executor(&host);
    let before = host.carrier_publication.publication_store.audit_snapshot();
    let result = catch_unwind(AssertUnwindSafe(|| {
        crate::parse::with_forced_catalog_eval_source_miss(|| {
            executor.execute_source("Miss.vue", FileLanguage::vue(), Arc::from(source), 1)
        })
    }));
    let stage = result.unwrap_or_else(|_| {
        panic!("forced pre-publication catalog miss must return Err(StageError), not unwind")
    });
    let err = stage.expect_err("forced pre-publication catalog miss must refuse the source stage");
    assert_eq!(err.kind, StageErrorKind::Generic);
    assert!(
        err.message.contains("semantic catalog miss"),
        "refusal must name the catalog miss, got {}",
        err.message
    );
    let after = host.carrier_publication.publication_store.audit_snapshot();
    assert_eq!(
        after.parser_started, before.parser_started,
        "catalog miss must not start a carrier parse"
    );
    assert_eq!(
        after.leaders, before.leaders,
        "catalog miss must not enter a publication lane"
    );
    assert_eq!(
        after.adopted, before.adopted,
        "catalog miss must not adopt a published artifact"
    );
    assert!(
        host.ensure_indexed_ready("Miss.vue").is_none(),
        "catalog miss must not warm IndexedReady"
    );
    assert!(
        host.scheduler_source("Miss.vue").is_none(),
        "catalog miss must not commit a source-stage snapshot"
    );
}

#[test]
fn cold_component_meta_reuses_indexed_ready_eval_source_arc() {
    use crate::resolver_core::ComponentMetaRequestHost;

    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "const label = 'hi'\n",
        "</script>\n",
        "<template><div>{{ label }}</div></template>\n",
    );
    let host = make_host();
    upsert_vue(&host, "Reuse.vue", source);
    let indexed = host
        .ensure_indexed_ready("Reuse.vue")
        .expect("IndexedReady is the sole eval-source producer");
    let before = crate::parse::catalog_eval_source_call_count();
    let store = host.resolver_store_view_read().into_owned_view();
    let captured = host
        .capture_component_meta_inputs("Reuse.vue", &store)
        .expect("cold component-meta capture");
    assert_eq!(
        crate::parse::catalog_eval_source_call_count(),
        before,
        "cold component-meta must not recatalog eval-source after IndexedReady"
    );
    let owner = captured
        .owner_eval_source
        .as_ref()
        .expect("captured eval-source");
    assert!(
        Arc::ptr_eq(owner, &indexed.eval_source),
        "component-meta must clone IndexedReady.eval_source, not recatalog or copy bytes"
    );
}

#[test]
fn script_facts_reuse_indexed_ready_eval_source_without_recatalog() {
    let source = "<script lang=\"ts\">const ordinary = 1;</script>";
    let host = make_host();
    upsert_svelte(&host, "Reuse.svelte", source);
    let indexed = host
        .ensure_indexed_ready("Reuse.svelte")
        .expect("IndexedReady is the sole eval-source producer");
    let before = crate::parse::catalog_eval_source_call_count();
    let evidence = host.resolve_svelte_script_facts("Reuse.svelte");
    assert_eq!(
        crate::parse::catalog_eval_source_call_count(),
        before,
        "script-facts must not recatalog eval-source after IndexedReady"
    );
    assert!(
        matches!(
            evidence,
            crate::framework::script_facts::ScriptFactEvidence::Exact(_)
        ),
        "script-facts still resolve from the stored eval-source"
    );
    assert_eq!(
        indexed.eval_source.len(),
        source.len(),
        "script-facts reuse the stored position-preserving eval-source"
    );
}

#[test]
fn eval_env_reuses_indexed_ready_eval_source_without_recatalog() {
    use crate::resolver_core::ComponentMetaResolutionPurpose;
    use crate::resolver_core::ResolverContext;

    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "const label = 'hi'\n",
        "</script>\n",
        "<template><div>{{ label }}</div></template>\n",
    );
    let host = make_host();
    upsert_vue(&host, "EvalEnv.vue", source);
    let indexed = host
        .ensure_indexed_ready("EvalEnv.vue")
        .expect("IndexedReady is the sole eval-source producer");
    let snapshot = host
        .get_analysis("EvalEnv.vue")
        .expect("analysis after IndexedReady");
    let before = crate::parse::catalog_eval_source_call_count();
    let from_captured = VerterHost::clone_owner_eval_source_arc(
        &host as &dyn ResolverContext,
        "EvalEnv.vue",
        Some(&indexed.eval_source),
    )
    .expect("captured eval-source");
    assert!(
        Arc::ptr_eq(&from_captured, &indexed.eval_source),
        "captured eval-env compute must clone IndexedReady.eval_source, not to_string"
    );
    let from_fallthrough =
        VerterHost::clone_owner_eval_source_arc(&host as &dyn ResolverContext, "EvalEnv.vue", None)
            .expect("fallthrough clones IndexedReady");
    assert!(
        Arc::ptr_eq(&from_fallthrough, &indexed.eval_source),
        "uncaptured eval-env compute must clone IndexedReady.eval_source, not recatalog or to_string"
    );
    let computed = host.compute_evaluated_types_with_tracking_from_owner_context_with_ctx(
        &host as &dyn ResolverContext,
        "EvalEnv.vue",
        &snapshot,
        None,
        ComponentMetaResolutionPurpose::Fallthrough,
    );
    assert!(
        computed.is_some(),
        "uncaptured eval-env compute still runs from the stored eval-source"
    );
    assert_eq!(
        crate::parse::catalog_eval_source_call_count(),
        before,
        "eval-env must not recatalog eval-source after IndexedReady"
    );
}

// ───────────────────────── content overrides ─────────────────────────

#[test]
fn block_override_roundtrip_produces_identical_analysis() {
    let source = "<template lang=\"pug\">div hello</template>\n<script setup lang=\"ts\">const msg: string = 'hi';</script>\n";
    let host = make_host();
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Ovr.vue".to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let request = update.preprocessor_requests.first().expect("Pug request");

    let profile = CompileProfile::default();
    let result = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "Ovr.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry::supplied_for_test(
                request,
                "<div>hello</div>",
            )],
        })
        .expect("block override should apply");
    assert!(
        result.changed,
        "template admission must report a state change"
    );

    // The override-aware analysis surface stays identical: one binding
    // from the script, the synthetic template parses to a snapshot.
    let analysis = host
        .get_analysis("Ovr.vue")
        .expect("analysis after override");
    let binding_names: Vec<&str> = analysis.bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        binding_names,
        vec!["msg"],
        "override must not change script analysis"
    );

    // Re-applying the SAME override is a no-op round-trip.
    let again = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "Ovr.vue".to_string(),
            compile_profile: profile,
            overrides: vec![BlockOverrideEntry::supplied_for_test(
                request,
                "<div>hello</div>",
            )],
        })
        .expect_err("a correlation token is single-use");
    assert!(matches!(
        again,
        HostError::BlockContentRefused(BlockContentRefusal::CorrelationTerminal)
    ));

    // The authoritative source type is computed from the RAW scheduler
    // parse and survives the override layer untouched.
    let st = host
        .authoritative_source_type_for("Ovr.vue")
        .expect("source type for Ovr.vue");
    assert_eq!(render_source_type(st), "ts=true jsx=false dts=false");
}

// ───────────────────────── route-owned shallow state ─────────────────────────

#[test]
fn route_owned_eval_state_carries_parse_payload_for_vue() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let vue_id = "/workspace/node_modules/pkg/dist/Button.vue";
    let vue_src = "<script setup lang=\"ts\">const props = defineProps<{ label: string }>()</script>\n<template><button>{{ props.label }}</button></template>";
    ws.inject_file(vue_id.to_string(), Arc::from(vue_src));
    let dts_id = "/workspace/node_modules/pkg/dist/shared.d.ts";
    ws.inject_file(
        dts_id.to_string(),
        Arc::from("export interface Alpha { alpha?: string }"),
    );

    let host = VerterHost::new(HostConfig::default(), ws);

    // Vue route-owned eval state: raw source verbatim, parse payload
    // PRESENT, whole_hash = content hash.
    let (raw, parse_payload, whole_hash) = host
        .current_eval_state(vue_id)
        .expect("eval state for the imported Vue file");
    assert_eq!(raw.as_ref(), vue_src);
    assert!(
        parse_payload.is_some(),
        "a .vue route-owned entry must carry its parse payload"
    );
    assert_eq!(whole_hash, crate::hash::hash_16(vue_src.as_bytes()));

    // Non-SFC route-owned eval state: no parse payload.
    let (_, parse_payload, _) = host
        .current_eval_state(dts_id)
        .expect("eval state for the imported declaration file");
    assert!(
        parse_payload.is_none(),
        "a plain-script route-owned entry carries no carrier parse payload"
    );
}

// ───────────────────────── IDE virtual output ─────────────────────────

#[test]
fn ide_virtual_output_for_fixture_sfc_is_byte_stable() {
    let source = "<script setup lang=\"ts\">\nconst props = defineProps<{ label: string; count?: number }>()\n</script>\n<template><button :data-count=\"props.count\">{{ props.label }}</button></template>\n";
    let host = make_host();
    upsert_vue(&host, "Fixture.vue", source);

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("Fixture.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: CompileProfile::default(),
        })
        .expect("main virtual file compiles");

    let content = response.code.clone();
    assert!(!content.is_empty(), "main virtual output must be non-empty");
    // Byte-identity pin: the full output hash of the runtime (`Main`) module.
    //
    // The authoritative `MacroRuntimeBundle` DTO emits the OFFICIAL Vue dev
    // shape for an OPTIONAL prop — `count: { type: Number, required: false }`
    // (never a bare `count: { type: Number }`, which would silently drop the
    // required-ness fact) — matching official `@vue/compiler-sfc` and the
    // compiler's own `optional_boolean_prop_emits_no_default` /
    // `optional non-Boolean prop keeps the official dev shape` assertions
    // in `crates/verter_compiler/src/script/tests.rs`.
    let hash_hex: String = crate::hash::hash_16(content.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        hash_hex, "999ab15ddca5440126060c25b9b8bad4",
        "runtime (Main) virtual output drifted byte-wise; content:\n{content}"
    );
}

// ───────────────────────── component-meta surface ─────────────────────────

#[test]
fn component_meta_props_surface_is_stable() {
    let source = "<script setup lang=\"ts\">\nconst props = defineProps<{ label: string; count?: number }>()\nconst emit = defineEmits<{ (e: 'change', value: number): void }>()\n</script>\n<template><button @click=\"emit('change', 1)\">{{ props.label }}</button></template>\n";
    let host = make_host();
    upsert_vue(&host, "Meta.vue", source);

    let meta = host
        .get_component_meta("Meta.vue")
        .expect("component meta present for SFC");

    let mut props: Vec<String> = meta
        .props
        .iter()
        .map(|p| {
            format!(
                "{}:{}:{}",
                p.name,
                p.publication
                    .evidence()
                    .map(verter_type_expr::AuthoredTypeEvidence::text)
                    .unwrap_or("<none>"),
                p.required
            )
        })
        .collect();
    props.sort();
    assert_eq!(
        props,
        vec![
            "count:number:false".to_string(),
            "label:string:true".to_string()
        ],
        "published props surface drifted"
    );

    let event_names: Vec<&str> = meta.events.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        event_names,
        vec!["change"],
        "published events surface drifted"
    );
}

// ─────────────────── carrier dispatch rehousing byte-identity ───────────────────

/// The Vue carrier parse dispatch now routes through the compiler-side
/// carrier registry (the bridge). This pins that the rehoused dispatch
/// produces an artifact whose parsed SFC drives `compile()` to bytes
/// IDENTICAL to the compiler's own untouched public `compile()` entry —
/// the byte-identity crux of the session-dispatch rehousing.
///
/// Discriminating: if the bridge ever drifted from `parse_sfc(source,
/// None, None)` (different delimiters, custom-element prefixes, or a
/// re-parse with different options), the rehoused-dispatch parsed SFC
/// would diverge and `compile_from_parsed` on it would produce different
/// bytes than the direct `compile()`.
#[test]
fn rehoused_carrier_dispatch_drives_compile_byte_identical_to_direct_compile() {
    use verter_compiler::compile::types::VueExecutionInputs;
    use verter_compiler::compile::VueMacroSemanticInput;
    use verter_compiler::compile_request::{
        CompileProduct, CompileRequest, FrameworkCompileRequest, IdeProductRequest,
        RuntimeProductRequest, VueCompileRequest,
    };
    use verter_compiler::framework_common::vue_bridge::compile_registered_vue_artifact;

    // A spread of fixture SFCs covering script-setup, plain script,
    // template, styles, and JS dialect.
    let fixtures = [
        "<script setup lang=\"ts\">const a: number = 1</script>\n<template><div>{{ a }}</div></template>",
        "<script>export default { name: 'X' }</script>\n<template><span class=\"c\">hi</span></template>\n<style scoped>.c{color:red}</style>",
        "<script setup>const n = 1</script>\n<template><p>{{ n }}</p></template>",
        "<template><button @click=\"go\">{{ label }}</button></template>\n<script setup lang=\"ts\">const label='x'; function go(){}</script>",
    ];

    for source in fixtures {
        let request = CompileRequest::new(
            vec![
                CompileProduct::RuntimeClient(RuntimeProductRequest {
                    runtime_source_map: true,
                    ..Default::default()
                }),
                CompileProduct::IdeCompanion(IdeProductRequest {
                    want_source_map: true,
                    ..Default::default()
                }),
            ],
            FrameworkCompileRequest::Vue(VueCompileRequest::default()),
            None,
            Some("App.vue".to_string()),
            None,
            false,
            false,
        )
        .expect("RuntimeClient + IdeCompanion together must construct");

        // Direct path: the compiler's untouched `#[doc(hidden)]` `compile()`
        // — the raw per-block `VerterCompileResult` this test compares has
        // no equivalent on the public one-shot `StandaloneCompiler::compile`
        // atomic contract.
        let alloc_direct = oxc_allocator::Allocator::new();
        let direct = verter_compiler::compile::compile(
            source,
            &request,
            &VueExecutionInputs::default(),
            &VueMacroSemanticInput::Unavailable,
            &alloc_direct,
        )
        .expect("a plain RuntimeClient + IdeCompanion compile must not be refused");

        // Rehoused path: the session's carrier dispatch produces the
        // framework-neutral artifact, the host reaches its parsed SFC back
        // out, and `compile_from_parsed` drives the SAME compile from the
        // SAME canonical request.
        let (_snapshot, artifact) = crate::parse::carrier_parse_snapshot(
            "App.vue",
            source,
            verter_semantic::analysis::AnalysisScope::LSP,
            &FileLanguage::vue(),
            &crate::types::MetaProvenance::default(),
        )
        .expect("Vue carrier dispatch yields a snapshot");
        let alloc_b = oxc_allocator::Allocator::new();
        let rehoused = compile_registered_vue_artifact(
            source,
            &artifact,
            &request,
            &VueExecutionInputs::default(),
            &VueMacroSemanticInput::Unavailable,
            &alloc_b,
        )
        .expect("registered Vue artifact compiles");

        assert_eq!(
            direct.tsx.as_ref().map(|t| &t.code),
            rehoused.tsx.as_ref().map(|t| &t.code),
            "TSX code drifted between direct compile and rehoused-dispatch compile for:\n{source}"
        );
        assert_eq!(
            direct.script.as_ref().map(|s| &s.code),
            rehoused.script.as_ref().map(|s| &s.code),
            "script code drifted between direct and rehoused-dispatch compile for:\n{source}"
        );
        assert_eq!(
            direct.template.as_ref().map(|t| &t.code),
            rehoused.template.as_ref().map(|t| &t.code),
            "template code drifted between direct and rehoused-dispatch compile for:\n{source}"
        );
        let direct_styles: Vec<&String> = direct.styles.iter().map(|s| &s.code).collect();
        let rehoused_styles: Vec<&String> = rehoused.styles.iter().map(|s| &s.code).collect();
        assert_eq!(
            direct_styles, rehoused_styles,
            "style code drifted between direct and rehoused-dispatch compile for:\n{source}"
        );
    }
}

/// The rehoused-dispatch artifact's `parse_key` stamp equals the
/// version the legacy direct producer stamped, so the
/// `FileArtifactStore` legacy key dimension is unchanged — a stale
/// artifact cannot serve nor be evicted spuriously by the rehousing.
#[test]
fn rehoused_carrier_artifact_stamps_exact_parse_key() {
    let source = "<script setup lang=\"ts\">const a = 1</script>";
    let (_snapshot, artifact) = crate::parse::carrier_parse_snapshot(
        "App.vue",
        source,
        verter_semantic::analysis::AnalysisScope::LSP,
        &FileLanguage::vue(),
        &crate::types::MetaProvenance::default(),
    )
    .expect("Vue carrier dispatch yields a snapshot");
    let language = FileLanguage::vue();
    // `carrier_parse_snapshot` dispatches through the real carrier
    // registration path, which uses Vue's actual standard `{{`/`}}`
    // grammar — `ParseOptions::default()` no longer means that (it means
    // "the caller supplied nothing," an empty value).
    let profile = verter_language::syntax_profile_id_for(
        &language,
        &verter_language::ParseOptions::vue_standard(),
    )
    .unwrap();
    let expected = verter_language::parse_key_for(
        source,
        &language,
        verter_language::VUE_SYNTAX_COMPATIBILITY_DOMAIN,
        verter_language::VUE_SYNTAX_COMPATIBILITY_EPOCH,
        &profile,
    )
    .unwrap();
    assert_eq!(artifact.parse_key(), &expected);
}
