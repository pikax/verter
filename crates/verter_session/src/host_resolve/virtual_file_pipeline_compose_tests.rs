use super::*;
use verter_compiler::framework_common::{
    RuntimeOutputDescriptor, SourceMapFidelity, TemplateRenderExport,
};

fn template(code: &str, source_map: &str, imports: Vec<String>) -> RuntimeTemplateBlock {
    RuntimeTemplateBlock {
        code: code.to_string(),
        source_map: source_map.to_string(),
        imports,
        ssr_imports: Vec::new(),
        render_export: TemplateRenderExport::Render,
        output_descriptor: RuntimeOutputDescriptor::generated(
            code,
            None,
            &[("test:space", "test:artifact")],
            SourceMapFidelity::Approximate,
        ),
    }
}

#[test]
fn no_imports_returns_the_template_verbatim() {
    let block = template("const a = 1", "", vec![]);
    let (code, map) =
        compose_template_virtual_file(&block, None).expect("no-import template composes trivially");
    assert_eq!(code, "const a = 1");
    assert!(map.is_none());
}

#[test]
fn imports_present_prepends_the_import_line_and_shifts_the_map() {
    let map_json = "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"MACM\"}";
    let block = template("const n = 1", map_json, vec!["_openBlock".to_string()]);
    let (code, map) =
        compose_template_virtual_file(&block, None).expect("import template composes");
    assert_eq!(
        code, "import { openBlock as _openBlock } from \"vue\"\nconst n = 1",
        "the import preamble must precede the template's own code verbatim"
    );
    let map = map.expect("a present input map must still be present after composition");
    let decoded = verter_compiler::oxc_sourcemap::SourceMap::from_json_string(&map).unwrap();
    let token = decoded
        .get_tokens()
        .next()
        .expect("the shifted segment survives composition");
    assert_eq!(
        token.get_dst_line(),
        1,
        "the segment must move down by exactly the one-line preamble"
    );
    assert_eq!(token.get_dst_col(), 6);
    assert_eq!(
        decoded.get_source(token.get_source_id().unwrap()),
        Some("Comp.vue"),
        "the original source identity must survive — never a synthetic placeholder"
    );
}

#[test]
fn custom_runtime_module_name_reaches_the_import_specifier() {
    let block = template("const n = 1", "", vec!["_openBlock".to_string()]);
    let (code, _) =
        compose_template_virtual_file(&block, Some("@vue/runtime-dom")).expect("composes");
    assert!(code.starts_with("import { openBlock as _openBlock } from \"@vue/runtime-dom\"\n"));
}

fn vue_compile_input(canonical_id: &str, source: &str, has_template: bool) -> CompileInput {
    use crate::types::{CompileInput, FileMeta};
    use std::sync::Arc;
    CompileInput {
        canonical_id: canonical_id.to_string(),
        source: Arc::<str>::from(source),
        whole_hash: [0; 16],
        meta: FileMeta {
            has_script: true,
            has_template,
            main_depends_on_styles: false,
            has_scoped_style: false,
            script_lang: None,
            template_lang: None,
            style_langs: Vec::new(),
            custom_types: Vec::new(),
        },
        parse_diagnostics: crate::types::DiagnosticsSnapshot::default(),
        src_blocks: Vec::new(),
        external_requests: Vec::new(),
        has_supplied_block_content: false,
        block_content_inputs: Default::default(),
        macro_type_deps: Vec::new(),
        script_imports: Vec::new(),
        script_macros: Vec::new(),
        script_bindings: Vec::new(),
        script_macro_usage: None,
        script_vue_api_calls: Vec::new(),
        framework_parse: None,
        style_v_bind_vars: Vec::new(),
        style_v_bind_usage_complete: true,
        prepared_styles: Vec::new(),
    }
}

fn svelte_compile_input(canonical_id: &str, source: &str) -> CompileInput {
    vue_compile_input(canonical_id, source, true)
}

fn runtime_publication() -> RuntimeNodePublication {
    RuntimeNodePublication {
        publish_runtime_module: true,
        publish_script: true,
        publish_template: true,
        publish_style: false,
        runtime_module_name: None,
    }
}

/// Planted host-side Vue topology reconstruction is refused.
///
/// `Main` publishes only from the compiler's staged handoff: a bundle
/// carrying compiled blocks but no staged main refuses on BOTH the
/// publication and the render lane, and publishes no `Main` node.
///
/// "Compiled body present, typed artifact set absent" is deliberately not
/// constructible here. The module's bytes live ON the staged root
/// artifact, so the bundle has no separate body field a planted
/// reconstruction could occupy — the state the old second leg exercised is
/// now closed by the handoff's shape rather than by a runtime check.
#[test]
fn planted_host_vue_topology_reconstruction_is_refused() {
    use crate::host_resolve::compile_request_build::BoundCompiledProducts;
    use rustc_hash::FxHashMap;
    use verter_compiler::compile_request::ProductKind;
    use verter_compiler::framework_common::{
        RuntimeCompileOutput, RuntimeOutputDescriptor, RuntimeScriptBlock, SourceMapFidelity,
        VueHostCompiledProducts,
    };

    let snapshot = vue_main_reconstruction_diagnostics(4);
    assert_eq!(snapshot.diagnostics[0].code, "HOST_VUE_MAIN_NOT_ASSEMBLED");

    let input = vue_compile_input(
        "Comp.vue",
        "<script></script><template><div/></template>",
        true,
    );
    let bundle = RuntimeCompileOutput {
        script: Some(RuntimeScriptBlock {
            code: "const n = 1".to_string(),
            source_map: String::new(),
            setup: true,
            output_descriptor: RuntimeOutputDescriptor::generated(
                "const n = 1",
                None,
                &[("test:space", "test:artifact")],
                SourceMapFidelity::Approximate,
            ),
            generated_template_hole: None,
            runtime_imports: Vec::new(),
            sfc_export_placement: None,
        }),
        template: Some(template("const render = () => {}", "", vec![])),
        main: None,
        ..Default::default()
    };
    assert!(bundle.main.is_none());
    let products = BoundCompiledProducts::Vue(
        VueHostCompiledProducts::from_admitted_runtime_bundle(bundle, ProductKind::RuntimeClient),
    );
    let mut outputs = FxHashMap::default();
    let err = publish_runtime_nodes(&input, &products, &runtime_publication(), &mut outputs)
        .expect_err("blocks without a staged main must refuse");
    assert_eq!(err.diagnostics[0].code, "HOST_VUE_MAIN_NOT_ASSEMBLED");
    assert!(!outputs.contains_key(&crate::types::VirtualNodeKind::Main));

    let render = take_staged_main(
        products.runtime_bundle().expect("bundle"),
        &input,
        StagedMainCarrier::Vue,
    );
    let render_err = render.expect_err("render lane must refuse the same planted reconstruction");
    assert_eq!(
        render_err.diagnostics[0].code,
        "HOST_VUE_MAIN_NOT_ASSEMBLED"
    );
}

/// The staged handoff is the ONLY Vue `Main` transport: its root artifact
/// supplies the published bytes and language, and the complete set — typed
/// contributor relations and qualified maps included — survives the
/// handoff untouched.
#[test]
fn staged_vue_main_carries_bytes_language_and_artifact_relations() {
    use verter_compiler::assembly::{
        vue_main_compile_artifacts, FragmentDialect, VueMainDecoration,
    };
    use verter_compiler::compile_request::ProductKind;
    use verter_compiler::framework_common::{
        RuntimeCompileOutput, RuntimeOutputDescriptor, RuntimeScriptBlock, SourceMapFidelity,
    };

    let script_map = r#"{"version":3,"sources":["Comp.vue"],"sourcesContent":["const n = 1\n"],"names":[],"mappings":"AAAA"}"#;
    let mut bundle = RuntimeCompileOutput {
        script: Some(RuntimeScriptBlock {
            code: "const n = 1\n".to_string(),
            source_map: script_map.to_string(),
            setup: true,
            output_descriptor: RuntimeOutputDescriptor::generated(
                "const n = 1\n",
                Some(script_map),
                &[("test:space", "test:artifact")],
                SourceMapFidelity::Approximate,
            ),
            generated_template_hole: None,
            runtime_imports: Vec::new(),
            sfc_export_placement: None,
        }),
        ..Default::default()
    };
    let generated = "const n = 1\nexport default n;\n";
    let staged = vue_main_compile_artifacts(
        "Comp.vue",
        &bundle,
        ProductKind::RuntimeClient,
        FragmentDialect::TypeScript,
        generated,
        Some(script_map.to_string()),
        &VueMainDecoration::default(),
        true,
    )
    .expect("schema accepts");
    bundle.main = Some(staged);

    let input = vue_compile_input("Comp.vue", "<script>const n = 1</script>", false);
    let taken = take_staged_main(&bundle, &input, StagedMainCarrier::Vue).expect("staged handoff");
    assert_eq!(&**taken.code(), generated);
    assert_eq!(
        taken.lang(),
        "ts",
        "the published language is the dialect the assembly declared, not a \
         carrier-metadata fallback"
    );
    assert_eq!(
        taken.source_map(),
        Some(script_map),
        "the staged runtime map crosses the handoff with the set"
    );
    assert_eq!(taken.root().name(), "main");
    let set = taken.set();
    assert!(set
        .artifacts()
        .iter()
        .any(|artifact| artifact.name() == "script"));
    assert!(
        !taken.root().maps.is_empty(),
        "qualified maps must survive the host handoff"
    );
}

/// Planted host-side Svelte topology reconstruction is refused — the same
/// staged-handoff rule as Vue, reporting the Svelte code.
#[test]
fn planted_host_svelte_topology_reconstruction_is_refused() {
    use crate::host_resolve::compile_request_build::BoundCompiledProducts;
    use rustc_hash::FxHashMap;
    use verter_compiler::compile_request::ProductKind;
    use verter_compiler::framework_common::{
        RuntimeCompileOutput, RuntimeOutputDescriptor, RuntimeScriptBlock, SourceMapFidelity,
        SvelteHostCompiledProducts,
    };

    let snapshot = svelte_main_reconstruction_diagnostics(4);
    assert_eq!(
        snapshot.diagnostics[0].code,
        "HOST_SVELTE_MAIN_NOT_ASSEMBLED"
    );

    let input = svelte_compile_input(
        "Comp.svelte",
        "<script>let n = $state(1);</script>\n<p>{n}</p>",
    );
    let bundle = RuntimeCompileOutput {
        script: Some(RuntimeScriptBlock {
            code: "let n = 1".to_string(),
            source_map: String::new(),
            setup: true,
            output_descriptor: RuntimeOutputDescriptor::generated(
                "let n = 1",
                None,
                &[("test:space", "test:artifact")],
                SourceMapFidelity::Approximate,
            ),
            generated_template_hole: None,
            runtime_imports: Vec::new(),
            sfc_export_placement: None,
        }),
        main: None,
        ..Default::default()
    };
    assert!(bundle.main.is_none());
    let products =
        BoundCompiledProducts::Svelte(SvelteHostCompiledProducts::from_admitted_runtime_bundle(
            bundle,
            ProductKind::RuntimeClient,
        ));
    let mut outputs = FxHashMap::default();
    let err = publish_runtime_nodes(
        &input,
        &products,
        &RuntimeNodePublication {
            publish_script: false,
            publish_template: false,
            ..runtime_publication()
        },
        &mut outputs,
    )
    .expect_err("blocks without a staged main must refuse");
    assert_eq!(err.diagnostics[0].code, "HOST_SVELTE_MAIN_NOT_ASSEMBLED");
    assert!(!outputs.contains_key(&crate::types::VirtualNodeKind::Main));

    let render = take_staged_main(
        products.runtime_bundle().expect("bundle"),
        &input,
        StagedMainCarrier::Svelte,
    );
    let render_err = render.expect_err("render lane must refuse the same planted reconstruction");
    assert_eq!(
        render_err.diagnostics[0].code,
        "HOST_SVELTE_MAIN_NOT_ASSEMBLED"
    );
}

/// The Svelte staged handoff transports the self-contained module the same
/// way: bytes, language, map and the complete typed set.
#[test]
fn staged_svelte_main_carries_bytes_language_and_artifact_relations() {
    use std::sync::Arc;
    use verter_compiler::assembly::{svelte_main_compile_artifacts, SvelteMainCompileRequest};
    use verter_compiler::compile_request::ProductKind;
    use verter_compiler::framework_common::RuntimeCompileOutput;

    let source = "<script>let n = $state(1);</script>\n<p>{n}</p>\n";
    let generated =
        "import * as $ from 'svelte/internal/client';\nexport default function Comp($$anchor) {}\n";
    let map_json = r#"{"version":3,"file":"Comp.svelte","sources":["Comp.svelte"],"sourcesContent":["<script>let n = $state(1);</script>\n<p>{n}</p>\n"],"names":[],"mappings":"AAAA"}"#;
    let bundle = RuntimeCompileOutput {
        main: Some(
            svelte_main_compile_artifacts(SvelteMainCompileRequest {
                canonical_id: "Comp.svelte",
                source,
                code: Arc::from(generated),
                source_map: Some(map_json),
                kind: ProductKind::RuntimeClient,
                want_maps: true,
                is_production: false,
                ssr: false,
                runes: None,
                css_hash_override: None,
                custom_element: false,
                css_code: None,
            })
            .expect("schema accepts"),
        ),
        ..Default::default()
    };
    let input = svelte_compile_input("Comp.svelte", source);
    let taken =
        take_staged_main(&bundle, &input, StagedMainCarrier::Svelte).expect("staged handoff");
    assert_eq!(&**taken.code(), generated);
    assert_eq!(taken.lang(), "js");
    assert_eq!(taken.source_map(), Some(map_json));
    assert_eq!(taken.root().name(), "main");
    assert!(
        !taken.root().maps.is_empty(),
        "qualified maps must survive the host handoff"
    );
}

// ── Custom virtual nodes: descriptor-driven publication ──────────────────
//
// The publish path's custom-block authority is the bundle's source-backed
// descriptors: identity/order/role/lang/content state come from them, and
// bytes come from them OR from the host's sealed block-content selection.

/// A Vue bundle with a staged Main plus a custom-block descriptor set, the
/// shape every custom-node publication consumes.
fn custom_block_bundle(
    fixtures: Vec<verter_compiler::assembly::CustomBlockFixture>,
) -> verter_compiler::framework_common::RuntimeCompileOutput {
    use verter_compiler::framework_common::{
        RuntimeCompileOutput, RuntimeOutputDescriptor, RuntimeScriptBlock, SourceMapFidelity,
    };
    let source = "<script setup>const n = 1</script>";
    let mut bundle = RuntimeCompileOutput {
        script: Some(RuntimeScriptBlock {
            code: "const n = 1\n".to_string(),
            source_map: String::new(),
            setup: true,
            output_descriptor: RuntimeOutputDescriptor::generated(
                "const n = 1\n",
                None,
                &[("test:space", "test:artifact")],
                SourceMapFidelity::Approximate,
            ),
            generated_template_hole: None,
            runtime_imports: Vec::new(),
            sfc_export_placement: None,
        }),
        custom_block_artifacts: Some(
            verter_compiler::assembly::custom_block_fixture_set(source, fixtures)
                .expect("fixture descriptors mint"),
        ),
        ..RuntimeCompileOutput::default()
    };
    let staged = verter_compiler::assembly::vue_main_compile_artifacts(
        "Comp.vue",
        &bundle,
        verter_compiler::compile_request::ProductKind::RuntimeClient,
        verter_compiler::assembly::FragmentDialect::TypeScript,
        "const _sfc_main = {}\n",
        None,
        &verter_compiler::assembly::VueMainDecoration::default(),
        false,
    )
    .expect("schema accepts");
    bundle.main = Some(staged);
    bundle
}

fn published_custom_nodes(
    input: &CompileInput,
    bundle: verter_compiler::framework_common::RuntimeCompileOutput,
) -> FxHashMap<crate::types::VirtualNodeKind, CachedVirtualFile> {
    use crate::host_resolve::compile_request_build::BoundCompiledProducts;
    let products = BoundCompiledProducts::Vue(
        verter_compiler::framework_common::VueHostCompiledProducts::from_admitted_runtime_bundle(
            bundle,
            verter_compiler::compile_request::ProductKind::RuntimeClient,
        ),
    );
    let mut outputs = FxHashMap::default();
    publish_runtime_nodes(input, &products, &runtime_publication(), &mut outputs)
        .expect("a staged bundle publishes");
    outputs
}

/// AC1/AC2: role/tag, language and typed content state publish straight off
/// the descriptors. A `src`-backed descriptor with no admitted selection
/// publishes NO node — unavailable content is not replaced with empty text.
#[test]
fn custom_nodes_publish_from_descriptor_content_state() {
    use verter_compiler::assembly::CustomBlockFixture;
    let mut input = vue_compile_input(
        "Comp.vue",
        "<script setup>const n = 1</script><i18n lang=\"json\">{\"a\":1}</i18n><docs src=\"./d.md\"></docs><note></note>",
        false,
    );
    // Planted session-metadata reconstruction: stale lang/type facts that
    // disagree with the descriptors must never reach the published nodes.
    input.meta.custom_types = vec!["wrong".to_string(); 3];

    let bundle = custom_block_bundle(vec![
        CustomBlockFixture {
            role: "i18n".to_string(),
            lang: Some("json".to_string()),
            src: None,
            text: Some("{\"a\":1}".to_string()),
        },
        CustomBlockFixture::src_backed("docs", "./d.md"),
        CustomBlockFixture::empty("note"),
    ]);
    let outputs = published_custom_nodes(&input, bundle);

    let i18n = outputs
        .get(&crate::types::VirtualNodeKind::Custom { index: 0 })
        .expect("a local descriptor publishes its node");
    assert_eq!(&*i18n.code, "{\"a\":1}");
    assert_eq!(i18n.lang.as_deref(), Some("json"));
    assert_eq!(i18n.meta.block_type.as_deref(), Some("i18n"));
    assert_eq!(i18n.meta.custom_index, Some(0));

    assert!(
        !outputs.contains_key(&crate::types::VirtualNodeKind::Custom { index: 1 }),
        "a src-backed descriptor with no admitted selection must not publish an empty-content node"
    );

    let note = outputs
        .get(&crate::types::VirtualNodeKind::Custom { index: 2 })
        .expect("an empty descriptor publishes its node with empty typed content");
    assert_eq!(&*note.code, "");
    assert_eq!(note.lang, None);
    assert_eq!(note.meta.block_type.as_deref(), Some("note"));
}

/// AC1/AC2: the host's sealed block-content selection is the byte authority
/// for its slot — external `src` bytes and preprocessed `lang` output
/// publish the admitted bytes, not the authored region.
#[test]
fn custom_node_bytes_come_from_the_admitted_selection() {
    use verter_compiler::assembly::CustomBlockFixture;
    use verter_compiler::framework_common::RuntimeBlockContentInput;
    let mut input = vue_compile_input(
        "Comp.vue",
        "<script setup>const n = 1</script><i18n lang=\"yaml\">hello: world</i18n><docs src=\"./d.md\"></docs>",
        false,
    );
    let selected = RuntimeBlockContentInput {
        code: Arc::from("{\"processed\":true}"),
        source_map: None,
        lang: "json".to_string(),
        content_artifact_token: "test:content".to_string(),
        source_space_token: "test:space".to_string(),
        parsed: None,
        producer: None,
        authored_basis: None,
        diagnostics: Vec::new(),
    };
    input.block_content_inputs.custom_blocks = vec![Some(selected.clone()), Some(selected)];

    let bundle = custom_block_bundle(vec![
        CustomBlockFixture {
            role: "i18n".to_string(),
            lang: Some("yaml".to_string()),
            src: None,
            text: Some("hello: world".to_string()),
        },
        CustomBlockFixture::src_backed("docs", "./d.md"),
    ]);
    let outputs = published_custom_nodes(&input, bundle);

    for index in 0..2 {
        let node = outputs
            .get(&crate::types::VirtualNodeKind::Custom { index })
            .expect("a selected slot publishes its node");
        assert_eq!(
            &*node.code, "{\"processed\":true}",
            "the admitted selection is the byte authority for slot {index}"
        );
    }
}
