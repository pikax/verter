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

// ───────────────────────── content overrides ─────────────────────────

#[test]
fn block_override_roundtrip_produces_identical_analysis() {
    let source = "<template lang=\"pug\">div hello</template>\n<script setup lang=\"ts\">const msg: string = 'hi';</script>\n";
    let host = make_host();
    upsert_vue(&host, "Ovr.vue", source);

    let profile = CompileProfile::default();
    let result = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "Ovr.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry {
                block_type: PreprocessorBlockType::Template,
                index: 0,
                code: Arc::from("<div>hello</div>"),
                source_map: None,
            }],
        })
        .expect("block override should apply");
    assert!(
        !result.changed_virtual_ids.is_empty() || !result.changed_virtual_nodes.is_empty(),
        "template override must report changed nodes"
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
            overrides: vec![BlockOverrideEntry {
                block_type: PreprocessorBlockType::Template,
                index: 0,
                code: Arc::from("<div>hello</div>"),
                source_map: None,
            }],
        })
        .expect("idempotent re-apply");
    assert!(
        again.changed_virtual_ids.is_empty(),
        "identical override hash must round-trip as no_change"
    );

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
        hash_hex, "fdcc6de32cbf81bc593cdd9c50c47892",
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
                p.raw_type.as_deref().unwrap_or("<none>"),
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
    use verter_compiler::compile::types::{CodegenOptions, CompileTarget, VerterCompileOptions};
    use verter_compiler::compile::{compile, compile_from_parsed, VueMacroSemanticInput};

    // A spread of fixture SFCs covering script-setup, plain script,
    // template, styles, and JS dialect.
    let fixtures = [
        "<script setup lang=\"ts\">const a: number = 1</script>\n<template><div>{{ a }}</div></template>",
        "<script>export default { name: 'X' }</script>\n<template><span class=\"c\">hi</span></template>\n<style scoped>.c{color:red}</style>",
        "<script setup>const n = 1</script>\n<template><p>{{ n }}</p></template>",
        "<template><button @click=\"go\">{{ label }}</button></template>\n<script setup lang=\"ts\">const label='x'; function go(){}</script>",
    ];

    for source in fixtures {
        let core_opts = CodegenOptions {
            filename: Some("App.vue".to_string()),
            target: CompileTarget::BUNDLER | CompileTarget::TSX,
            ..Default::default()
        };
        let verter_opts = VerterCompileOptions {
            source_map: true,
            ..Default::default()
        };

        // Direct path: the compiler's untouched public `compile()`.
        let alloc_a = oxc_allocator::Allocator::new();
        let direct = compile(
            source,
            &core_opts,
            &verter_opts,
            &VueMacroSemanticInput::Unavailable,
            &alloc_a,
        );

        // Rehoused path: the session's carrier dispatch produces the
        // framework-neutral artifact, the host reaches its parsed SFC back
        // out, and `compile_from_parsed` drives the SAME compile.
        let (_snapshot, artifact) = crate::parse::carrier_parse_snapshot(
            "App.vue",
            source,
            verter_semantic::analysis::AnalysisScope::LSP,
            &FileLanguage::vue(),
            &crate::types::MetaProvenance::default(),
        )
        .expect("Vue carrier dispatch yields a snapshot");
        let parsed = crate::typeinfo::adapters::vue::vue_parse(&artifact)
            .expect("the rehoused Vue artifact carries a ParsedSfc");
        let alloc_b = oxc_allocator::Allocator::new();
        let rehoused = compile_from_parsed(
            source,
            parsed,
            &core_opts,
            &verter_opts,
            &VueMacroSemanticInput::Unavailable,
            &alloc_b,
        );

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

/// The rehoused-dispatch artifact's `parser_version` stamp equals the
/// version the legacy direct producer stamped, so the
/// `FileArtifactStore` legacy key dimension is unchanged — a stale
/// artifact cannot serve nor be evicted spuriously by the rehousing.
#[test]
fn rehoused_carrier_artifact_stamps_unchanged_parser_version() {
    let (_snapshot, artifact) = crate::parse::carrier_parse_snapshot(
        "App.vue",
        "<script setup lang=\"ts\">const a = 1</script>",
        verter_semantic::analysis::AnalysisScope::LSP,
        &FileLanguage::vue(),
        &crate::types::MetaProvenance::default(),
    )
    .expect("Vue carrier dispatch yields a snapshot");
    assert_eq!(
        artifact.parser_version,
        crate::file_artifact_store::LEGACY_PARSER_VERSION,
        "the rehoused dispatch must stamp the same parser version the legacy producer did"
    );
}
