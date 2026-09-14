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

/// Planted host-side Vue topology reconstruction is refused.
#[test]
fn planted_host_vue_topology_reconstruction_is_refused() {
    use crate::host_resolve::compile_request_build::BoundCompiledProducts;
    use crate::types::{CompileInput, FileMeta};
    use rustc_hash::FxHashMap;
    use std::sync::Arc;
    use verter_compiler::compile_request::ProductKind;
    use verter_compiler::framework_common::{
        RuntimeCompileOutput, RuntimeOutputDescriptor, RuntimeScriptBlock, SourceMapFidelity,
        VueHostCompiledProducts,
    };

    let snapshot = vue_main_reconstruction_diagnostics(4);
    assert_eq!(snapshot.diagnostics[0].code, "HOST_VUE_MAIN_NOT_ASSEMBLED");

    let input = CompileInput {
        canonical_id: "Comp.vue".to_string(),
        source: Arc::<str>::from("<script></script><template><div/></template>"),
        whole_hash: [0; 16],
        meta: FileMeta {
            has_script: true,
            has_template: true,
            main_depends_on_styles: false,
            has_scoped_style: false,
            script_lang: None,
            template_lang: None,
            style_langs: Vec::new(),
            custom_types: Vec::new(),
            custom_langs: Vec::new(),
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
    };
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
        main: Default::default(),
        ..Default::default()
    };
    assert!(bundle.main.body_code.is_none());
    let products = BoundCompiledProducts::Vue(
        VueHostCompiledProducts::from_admitted_runtime_bundle(bundle, ProductKind::RuntimeClient),
    );
    let mut outputs = FxHashMap::default();
    let err = publish_runtime_nodes(
        &input,
        &products,
        &RuntimeNodePublication {
            publish_runtime_module: true,
            publish_script: true,
            publish_template: true,
            publish_style: false,
            runtime_module_name: None,
            assembly: crate::compile::VueMainAssemblyAxes::from(
                &crate::types::CompileProfile::default(),
            ),
        },
        &mut outputs,
    )
    .expect_err("blocks without a compiler-owned body must refuse");
    assert_eq!(err.diagnostics[0].code, "HOST_VUE_MAIN_NOT_ASSEMBLED");
    assert!(!outputs.contains_key(&crate::types::VirtualNodeKind::Main));

    let render = take_compiler_vue_main(products.runtime_bundle().expect("bundle"), &input, false);
    let render_err = render.expect_err("render lane must refuse the same planted reconstruction");
    assert_eq!(
        render_err.diagnostics[0].code,
        "HOST_VUE_MAIN_NOT_ASSEMBLED"
    );

    let mut planted_body = RuntimeCompileOutput {
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
        main: Default::default(),
        ..Default::default()
    };
    planted_body.main.body_code = Some(Arc::from("export default {}"));
    let planted_products =
        BoundCompiledProducts::Vue(VueHostCompiledProducts::from_admitted_runtime_bundle(
            planted_body,
            ProductKind::RuntimeClient,
        ));
    let mut planted_outputs = FxHashMap::default();
    let planted_err = publish_runtime_nodes(
        &input,
        &planted_products,
        &RuntimeNodePublication {
            publish_runtime_module: true,
            publish_script: true,
            publish_template: true,
            publish_style: false,
            runtime_module_name: None,
            assembly: crate::compile::VueMainAssemblyAxes::from(
                &crate::types::CompileProfile::default(),
            ),
        },
        &mut planted_outputs,
    )
    .expect_err("a planted body without a typed main artifact must refuse");
    assert_eq!(
        planted_err.diagnostics[0].code,
        "HOST_VUE_MAIN_NOT_ASSEMBLED"
    );
    assert!(!planted_outputs.contains_key(&crate::types::VirtualNodeKind::Main));
}

#[test]
fn take_compiler_vue_main_preserves_artifact_relations() {
    use crate::types::{CompileInput, FileMeta};
    use std::sync::Arc;
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
    bundle.main.body_code = Some(Arc::from(generated));
    bundle.main.artifacts = Some(
        vue_main_compile_artifacts(
            "Comp.vue",
            &bundle,
            ProductKind::RuntimeClient,
            FragmentDialect::JavaScript,
            generated,
            Some(script_map),
            None,
            &VueMainDecoration::default(),
            true,
        )
        .expect("schema accepts"),
    );
    let input = CompileInput {
        canonical_id: "Comp.vue".to_string(),
        source: Arc::<str>::from("<script>const n = 1</script>"),
        whole_hash: [0; 16],
        meta: FileMeta {
            has_script: true,
            has_template: false,
            main_depends_on_styles: false,
            has_scoped_style: false,
            script_lang: None,
            template_lang: None,
            style_langs: Vec::new(),
            custom_types: Vec::new(),
            custom_langs: Vec::new(),
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
    };
    let taken = take_compiler_vue_main(&bundle, &input, false).expect("body is present");
    assert_eq!(&*taken.code, generated);
    let set = taken
        .artifacts
        .expect("typed artifact set survives handoff");
    assert!(set
        .artifacts()
        .iter()
        .any(|artifact| artifact.name() == "script"));
    let main = set
        .artifacts()
        .iter()
        .find(|artifact| artifact.name() == "main")
        .expect("main");
    assert!(
        !main.maps.is_empty(),
        "qualified maps must survive the host handoff"
    );
}
