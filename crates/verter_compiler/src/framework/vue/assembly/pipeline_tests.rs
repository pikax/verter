use super::*;
use crate::compile_request::{
    CompileProduct, FrameworkCompileRequest, ProductKind, RuntimeProductRequest, VueCompileRequest,
};
use verter_audit::observe::Uncaptured;

#[path = "text_contract_tests.rs"]
mod text_contract;

#[path = "supplied_tests.rs"]
mod supplied;

fn request() -> CompileRequest {
    request_with(None, RuntimeStyleProcessing::Complete)
}

fn request_with(
    component_id: Option<String>,
    style_processing: RuntimeStyleProcessing,
) -> CompileRequest {
    CompileRequest::new(
        vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
            inline: Some(false),
            runtime_source_map: true,
            style_processing,
            ..Default::default()
        })],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("App.vue".into()),
        component_id,
        false,
        true,
    )
    .unwrap()
}

fn compile(source: &str) -> Result<VueArtifactSet, VueCompileRefusal> {
    compile_in(source, &mut CompilerWorkLedger::<Uncaptured>::new())
}

fn compile_in<M: ObserveExec>(
    source: &str,
    ledger: &mut CompilerWorkLedger<M>,
) -> Result<VueArtifactSet, VueCompileRefusal> {
    compile_with(source, &request(), &Default::default(), ledger)
}

fn compile_with<M: ObserveExec>(
    source: &str,
    request: &CompileRequest,
    execution: &VueExecutionInputs,
    ledger: &mut CompilerWorkLedger<M>,
) -> Result<VueArtifactSet, VueCompileRefusal> {
    let parsed = crate::framework_common::vue_bridge::parse_sfc(source, None, None);
    compile_vue_runtime(
        &VueCompileInputs {
            source,
            parsed: &parsed,
            request,
            execution,
            macros: &Default::default(),
            block_content: &Default::default(),
            decoration: &Default::default(),
            canonical_id: "App.vue",
        },
        &Allocator::default(),
        ledger,
    )
}

// Reads optional audit capture: runs in the instrumentation lane.
#[cfg(feature = "semantic-observe")]
#[test]
fn each_prerequisite_runs_once() {
    use crate::work_ledger::WorkKind;
    let source = "<script setup>const a = 1; const b = true; const c = 'ok'</script><template><div :id=\"a\"><p v-if=\"b\">{{ c }}</p></div></template><style scoped>.x { color: red }</style>";
    for request in [request(), inline_request(true, true)] {
        let mut ledger = CompilerWorkLedger::<verter_audit::observe::Captured>::new();
        let staged = compile_with(source, &request, &Default::default(), &mut ledger)
            .unwrap()
            .stage(ProductKind::RuntimeClient)
            .unwrap();
        assert!(staged.code().contains("_toDisplayString"));
        let snapshot = ledger.snapshot().unwrap();
        assert_eq!(snapshot.count(WorkKind::StructureSourceSizedVisits), 1);
        assert_eq!(
            snapshot.count(WorkKind::ParseAuthoritativeExpressionParses),
            1
        );
        assert_eq!(snapshot.count(WorkKind::ParseExpressionAttempts), 3);
        assert_eq!(snapshot.count(WorkKind::StyleBlocks), 1);
    }
}

fn module(source: &str) -> String {
    let staged = compile(source)
        .unwrap()
        .stage(ProductKind::RuntimeClient)
        .unwrap();
    let code = staged.code().to_string();
    let allocator = Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, &code, oxc_span::SourceType::mjs())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{code}",
        parsed.diagnostics
    );
    code
}

#[test]
fn a_template_less_component_compiles() {
    let code = module("<script setup>const msg = 'hello'</script>");
    assert!(code.contains("const msg = 'hello'"));
    assert!(code.ends_with("export default _sfc_main"));
    let empty = module("");
    assert!(
        empty.contains("const _sfc_main = /*@__PURE__*/_defineComponent({\n  __name: \"App\""),
        "{empty}"
    );
}

#[test]
fn a_script_less_component_compiles() {
    let code = module("<template><div>{{ msg }}</div></template>");
    assert!(code.contains("const _sfc_main = {}"));
    assert!(code.contains("_ctx.msg"));
    assert!(code.contains("_sfc_main.render = render"));
    let scoped =
        module("<template><div>{{ msg }}</div></template><style scoped>.a { color: red }</style>");
    assert!(
        scoped.contains("const _sfc_main = {};\n_sfc_main.__scopeId = \"data-v-0d04bfeb\";\n"),
        "{scoped}"
    );
}

#[test]
fn fine_grained_modules_keep_the_component_runtime_option() {
    use crate::compile_request::VueBackendRequest;

    for (source, backend, has_render) in [
        (
            "<template><div /></template>",
            VueBackendRequest::Vapor,
            true,
        ),
        (
            "<template vapor><div /></template>",
            VueBackendRequest::Inferred,
            true,
        ),
        (
            "<template><div /></template><style scoped>.card { color: red }</style>",
            VueBackendRequest::Vapor,
            true,
        ),
        (
            "<script setup>const msg = 'hello'</script><template><div>{{ msg }}</div></template>",
            VueBackendRequest::Vapor,
            true,
        ),
        ("", VueBackendRequest::Vapor, false),
        (
            "<script setup>const msg = 'hello'</script>",
            VueBackendRequest::Vapor,
            false,
        ),
    ] {
        let request = CompileRequest::new(
            vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
                inline: Some(false),
                ..Default::default()
            })],
            FrameworkCompileRequest::Vue(VueCompileRequest {
                backend,
                ..Default::default()
            }),
            None,
            Some("App.vue".into()),
            None,
            false,
            true,
        )
        .unwrap();
        let staged = compile_with(
            source,
            &request,
            &Default::default(),
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
        .unwrap()
        .stage(ProductKind::RuntimeClient)
        .unwrap();
        let code = staged.code();
        assert!(code.contains("__vapor: true"), "{code}");
        assert_eq!(
            code.contains("_sfc_main.render = render"),
            has_render,
            "{code}"
        );
        let allocator = Allocator::default();
        let parsed =
            verter_parser::oxc_parse::Parser::new(&allocator, code, oxc_span::SourceType::mjs())
                .parse();
        assert!(
            parsed.diagnostics.is_empty(),
            "{:?}\n{code}",
            parsed.diagnostics
        );
    }
}

#[test]
fn a_source_with_template_errors_publishes_nothing() {
    for source in [
        "<template><div></template>",
        "<template>{{ foo + }}</template>",
        "<template><div v-else /></template>",
        "<template><div :id=\"foo +\" /></template>",
    ] {
        assert!(
            matches!(compile(source), Err(VueCompileRefusal::InvalidSource { diagnostics }) if !diagnostics.is_empty()),
            "{source}"
        );
    }
}

#[test]
fn an_explicit_scope_id_is_preserved() {
    let source = "<template><div /></template><style scoped>.card { color: red }</style>";
    for (id, expected) in [("deadbeef", "deadbeef"), ("éééaé", "00000000")] {
        let request = request_with(Some(id.into()), RuntimeStyleProcessing::AuthoredOnly);
        let staged = compile_with(
            source,
            &request,
            &Default::default(),
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
        .unwrap()
        .stage(ProductKind::RuntimeClient)
        .unwrap();
        assert!(
            staged.code().contains(&format!("data-v-{expected}")),
            "{}",
            staged.code()
        );
    }
}

#[test]
fn an_unsupported_style_dialect_publishes_nothing() {
    let source =
        "<template><div /></template><style scoped lang=\"postcss\">.card { color: red }</style>";
    assert!(
        matches!(compile(source), Err(VueCompileRefusal::InvalidSource { diagnostics })
        if diagnostics.iter().any(|d| d.code == "XCssParseError"))
    );
}

#[test]
fn a_retained_style_parse_is_reused() {
    use crate::style_planner::{
        parse_ir_invocation_count, reset_parse_ir_invocation_count, PreparedStyleIr,
    };
    use verter_css_syntax::{parse_style_ir, CssDialect, CssParseMode, CssSource};
    let css = ".card { color: red }";
    let ir = parse_style_ir(
        CssSource::new(std::sync::Arc::from(css), 0).unwrap(),
        CssDialect::Css,
        CssParseMode::Recover,
    )
    .unwrap();
    let execution = VueExecutionInputs {
        prepared_styles: vec![Some(PreparedStyleIr::new(ir))],
        ..Default::default()
    };
    let source = "<template><div /></template><style scoped>.card { color: red }</style>";
    reset_parse_ir_invocation_count();
    let set = compile_with(
        source,
        &request(),
        &execution,
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .unwrap();
    assert!(set
        .stage(ProductKind::RuntimeClient)
        .unwrap()
        .code()
        .contains(".__scopeId"));
    assert_eq!(parse_ir_invocation_count(), 0);
}

fn server_request() -> CompileRequest {
    CompileRequest::new(
        vec![CompileProduct::RuntimeServer(RuntimeProductRequest {
            inline: Some(false),
            runtime_source_map: true,
            ..Default::default()
        })],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("App.vue".into()),
        None,
        false,
        true,
    )
    .unwrap()
}

fn runtime_request(products: Vec<CompileProduct>) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("App.vue".into()),
        None,
        false,
        true,
    )
    .unwrap()
}

fn runtime_product(product: ProductKind, map: bool) -> CompileProduct {
    let runtime = RuntimeProductRequest {
        inline: Some(false),
        runtime_source_map: map,
        ..Default::default()
    };
    match product {
        ProductKind::RuntimeClient => CompileProduct::RuntimeClient(runtime),
        ProductKind::RuntimeServer => CompileProduct::RuntimeServer(runtime),
        _ => unreachable!("a runtime product"),
    }
}

#[test]
fn two_product_bytes_equal_two_single_requests() {
    use crate::assembly::artifact_graph::ArtifactGraph;
    for source in [
        "<script setup>import { ref } from 'vue'; const theme = ref({ color: 'red' }); const msg = 'hi'</script><template><div class=\"card\">{{ msg }}</div></template><style scoped>.card { color: v-bind('theme.color') }</style><i18n lang=\"json\">{}</i18n>",
        "<script setup>const msg = 'hi'</script>",
        "<template><p>{{ msg }}</p></template>",
        "",
    ] {
        for (client_map, server_map) in [(true, true), (true, false), (false, true), (false, false)] {
            let products = vec![
                runtime_product(ProductKind::RuntimeClient, client_map),
                runtime_product(ProductKind::RuntimeServer, server_map),
            ];
            let both = compile_with(
                source,
                &runtime_request(products.clone()),
                &Default::default(),
                &mut CompilerWorkLedger::<Uncaptured>::new(),
            )
            .unwrap_or_else(|error| panic!("client map={client_map}, server map={server_map}: {error:?}\n{source}"));
            for product in products {
                let kind = product.kind();
                let single = compile_with(
                    source,
                    &runtime_request(vec![product]),
                    &Default::default(),
                    &mut CompilerWorkLedger::<Uncaptured>::new(),
                )
                .unwrap();
                let actual = both.stage(kind).unwrap();
                let expected = single.stage(kind).unwrap();
                assert_eq!(actual.code(), expected.code(), "{kind:?}: {source}");
                assert_eq!(actual.source_map(), expected.source_map(), "{kind:?}: {source}");
                assert_eq!(actual.source_map().is_some(), if kind == ProductKind::RuntimeClient { client_map } else { server_map });
                let allocator = Allocator::default();
                let parsed = verter_parser::oxc_parse::Parser::new(&allocator, actual.code(), oxc_span::SourceType::mjs()).parse();
                assert!(parsed.diagnostics.is_empty(), "{:?}\n{}", parsed.diagnostics, actual.code());
                let module = both.set().module(kind).unwrap();
                let single_module = single.set().module(kind).unwrap();
                let styles = both.set().styles_of(module.id());
                let single_styles = single.set().styles_of(single_module.id());
                assert_eq!(styles.len(), single_styles.len());
                for (style, single_style) in styles.iter().zip(single_styles) {
                    assert_eq!(artifact_text(style), artifact_text(single_style));
                }
                assert_eq!(artifact_text(both.set().metadata_of(module.id()).unwrap()), artifact_text(single.set().metadata_of(single_module.id()).unwrap()));
            }
        }
    }
}

// Reads optional audit capture: runs in the instrumentation lane.
#[cfg(feature = "semantic-observe")]
#[test]
fn two_products_share_prerequisites() {
    use crate::work_ledger::WorkKind;
    use verter_audit::observe::Captured;
    let source = "<script setup>const msg = 'hi'; const ok = true; const theme = { color: 'red' }</script><template><div class=\"card\"><p v-if=\"ok\">{{ msg }}</p></div></template><style scoped>.card { color: v-bind('theme.color') }</style>";
    let measure = |products| {
        crate::script::css_vars::take_expression_parse_count();
        let mut ledger = CompilerWorkLedger::<Captured>::new();
        let set = compile_with(
            source,
            &runtime_request(products),
            &Default::default(),
            &mut ledger,
        )
        .unwrap();
        assert!(set
            .stage(ProductKind::RuntimeClient)
            .unwrap()
            .code()
            .contains("_toDisplayString"));
        (
            ledger.snapshot().unwrap(),
            crate::script::css_vars::take_expression_parse_count(),
        )
    };
    let (single, single_css_parses) =
        measure(vec![runtime_product(ProductKind::RuntimeClient, true)]);
    let (both, both_css_parses) = measure(vec![
        runtime_product(ProductKind::RuntimeClient, true),
        runtime_product(ProductKind::RuntimeServer, true),
    ]);
    assert!(single_css_parses > 0);
    assert_eq!(both_css_parses, single_css_parses, "CSS expression parses");
    for kind in [
        WorkKind::ParseAuthoritativeExpressionParses,
        WorkKind::ParseExpressionAttempts,
        WorkKind::StructureNodesMaterialized,
        WorkKind::StructureSourceSizedVisits,
        WorkKind::StyleBlocks,
    ] {
        assert!(single.count(kind) > 0, "{kind:?}");
        assert_eq!(both.count(kind), single.count(kind), "{kind:?}");
    }
    assert_eq!(both.count(WorkKind::PlanningTargetEntries), 2);
    assert!(both.count(WorkKind::PlanningMultiTargetSharedPrerequisites) > 0);
    assert_eq!(
        single.count(WorkKind::PlanningMultiTargetSharedPrerequisites),
        0
    );
}

fn artifact_text(artifact: &crate::assembly::fragment::CompileArtifact) -> &str {
    match &artifact.content {
        crate::assembly::fragment::ArtifactContent::Available(code) => code,
        crate::assembly::fragment::ArtifactContent::Unavailable(reason) => {
            panic!("no content: {reason:?}")
        }
    }
}

/// The eight hex digits following the first `data-v-` in `text`.
fn scope_hash_in(text: &str) -> &str {
    let start = text
        .find("data-v-")
        .unwrap_or_else(|| panic!("no scope in {text}"))
        + 7;
    &text[start..start + 8]
}

#[test]
fn custom_blocks_are_attached_and_not_executed() {
    use crate::assembly::artifact_graph::ArtifactGraph;
    use crate::assembly::CustomBlockContent;
    let i18n = r#"{ "en": { "hello": "Hello" } }"#;
    let source = format!(
        "<template><div /></template>\n<i18n lang=\"json\">{i18n}</i18n>\n<docs src=\"./docs.md\" />\n"
    );
    let set = compile(&source).unwrap();
    let graph = set.set();
    let module = graph.module(ProductKind::RuntimeClient).unwrap();
    let blocks = graph.custom_blocks();
    assert_eq!(blocks.len(), 2);
    let (first, second) = (&blocks[0], &blocks[1]);
    assert_eq!(
        (first.role(), first.lang(), first.source_order()),
        ("i18n", Some("json"), 0)
    );
    assert!(
        matches!(first.content(), CustomBlockContent::Local { text, .. } if text == i18n),
        "{:?}",
        first.content()
    );
    let region = first.region();
    assert_eq!(&source[region.start as usize..region.end as usize], i18n);
    assert_eq!(
        (second.role(), second.src(), second.source_order()),
        ("docs", Some("./docs.md"), 1)
    );
    assert!(matches!(second.content(), CustomBlockContent::SrcBacked));
    for block in blocks {
        assert_eq!(block.attached_to(), module.id());
    }
    // The block's content is carried, never written into the module.
    let code = artifact_text(module);
    assert!(!code.contains("Hello"), "{code}");
    assert!(!code.contains("docs.md"), "{code}");
    let metadata: serde_json::Value =
        serde_json::from_str(artifact_text(graph.metadata_of(module.id()).unwrap())).unwrap();
    assert_eq!(
        metadata["customBlocks"],
        serde_json::json!(["i18n", "docs"])
    );

    let without = compile("<template><div /></template>").unwrap();
    assert!(without.set().custom_blocks().is_empty());
}

#[test]
fn the_scope_attribute_agrees_between_module_and_css() {
    use crate::assembly::artifact_graph::ArtifactGraph;
    // server_scoped_style_root: a scoped style over a server-rendered root.
    let source = "<script setup>const msg = 'hi'</script><template><div class=\"card\">{{ msg }}</div></template><style scoped>.card { color: red }</style>";
    let set = compile_with(
        source,
        &server_request(),
        &Default::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .unwrap();
    let graph = set.set();
    assert!(graph.module(ProductKind::RuntimeClient).is_none());
    let module = graph.module(ProductKind::RuntimeServer).unwrap();
    let code = artifact_text(module);
    assert!(code.contains("ssrRender"), "{code}");
    let in_module = scope_hash_in(code);
    let styles = graph.styles_of(module.id());
    assert_eq!(styles.len(), 1);
    let css = artifact_text(styles[0]);
    assert!(
        css.contains(&format!("[data-v-{in_module}]")),
        "{css}\n{code}"
    );
    let metadata: serde_json::Value =
        serde_json::from_str(artifact_text(graph.metadata_of(module.id()).unwrap())).unwrap();
    assert_eq!(
        metadata["scopeId"],
        serde_json::json!(in_module),
        "{metadata}"
    );
    assert_eq!(metadata["hasScopedStyle"], serde_json::json!(true));
}

#[test]
fn an_inline_render_is_separated_from_a_tightly_packed_setup_body() {
    let source = "<script setup>const n = 1</script><template><div>{{ n }}</div></template>";
    let request = inline_request(true, true);
    let staged = compile_with(
        source,
        &request,
        &Default::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .unwrap()
    .stage(ProductKind::RuntimeClient)
    .unwrap();
    let code = staged.code();
    let allocator = Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, code, oxc_span::SourceType::mjs())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{code}",
        parsed.diagnostics
    );
    assert!(code.contains("const n = 1\nreturn "), "{code}");
    assert!(!code.contains("_sfc_main.render ="), "{code}");
}

fn inline_request(force_js: bool, maps: bool) -> CompileRequest {
    CompileRequest::new(
        vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
            inline: Some(true),
            runtime_source_map: maps,
            ..Default::default()
        })],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("App.vue".into()),
        None,
        false,
        force_js,
    )
    .unwrap()
}

#[test]
fn inline_maps_resolve_template_identifiers_to_the_sfc() {
    let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst msg: string = 'hello'\nconst count = ref(1)\n</script>\n<template><div>{{msg}} {{count}}</div></template>";
    for force_js in [false, true] {
        let staged = compile_with(
            source,
            &inline_request(force_js, true),
            &Default::default(),
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
        .unwrap()
        .stage(ProductKind::RuntimeClient)
        .unwrap();
        let code = staged.code();
        let allocator = Allocator::default();
        let parsed = verter_parser::oxc_parse::Parser::new(
            &allocator,
            code,
            if force_js {
                oxc_span::SourceType::mjs()
            } else {
                oxc_span::SourceType::ts()
            },
        )
        .parse();
        assert!(
            parsed.diagnostics.is_empty(),
            "{:?}\n{code}",
            parsed.diagnostics
        );
        assert_eq!(code.contains("msg: string"), !force_js, "{code}");
        let map = oxc_sourcemap::SourceMap::from_json_string(staged.source_map().unwrap()).unwrap();
        let lookup = map.generate_lookup_table();
        for name in ["msg", "count"] {
            let generated = code.rfind(name).unwrap();
            let mut line = 0;
            let mut column = 0;
            crate::code_transform::advance_generated_position(
                &code[..generated],
                &mut line,
                &mut column,
            );
            let token = map.lookup_token(&lookup, line, column).unwrap();
            let authored = source.rfind(name).unwrap();
            let mut source_line = 0;
            let mut source_column = 0;
            crate::code_transform::advance_generated_position(
                &source[..authored],
                &mut source_line,
                &mut source_column,
            );
            assert_eq!(
                (token.get_src_line(), token.get_src_col()),
                (source_line, source_column),
                "{name}\n{code}"
            );
            assert_eq!(
                token.get_source_id().and_then(|id| map.get_source(id)),
                Some("App.vue")
            );
        }
        let without_map = compile_with(
            source,
            &inline_request(force_js, false),
            &Default::default(),
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
        .unwrap()
        .stage(ProductKind::RuntimeClient)
        .unwrap();
        assert_eq!(without_map.code(), code);
        assert!(without_map.source_map().is_none());
    }
}

#[test]
fn css_vars_come_from_the_scope_plan() {
    use crate::assembly::artifact_graph::ArtifactGraph;
    let source = "<script setup>import { ref } from 'vue'; const color = ref('red')</script><template><div>{{ color }}</div></template><style scoped>.x { color: v-bind(color) }</style>";
    let set = compile_with(
        source,
        &inline_request(true, true),
        &Default::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .unwrap();
    let staged = set.stage(ProductKind::RuntimeClient).unwrap();
    let code = staged.code();
    let allocator = Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, code, oxc_span::SourceType::mjs())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{code}",
        parsed.diagnostics
    );
    let graph = set.set();
    let root = graph.module(ProductKind::RuntimeClient).unwrap();
    let styles = graph.styles_of(root.id());
    let css = artifact_text(styles[0]);
    let property = css
        .split("var(--")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap();
    assert!(code.contains("useCssVars as _useCssVars"), "{code}");
    assert!(
        code.contains(&format!("\"{property}\": (color.value)")),
        "{code}\n{css}"
    );
}

#[test]
fn inline_helper_imports_bind_each_local_once() {
    let source = "<script setup>let value = 1; let local = 1</script><template><div>{{ value }} {{ local }}</div></template><style>.x { color: v-bind(value) }</style>";
    let staged = compile_with(
        source,
        &inline_request(true, true),
        &Default::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .unwrap()
    .stage(ProductKind::RuntimeClient)
    .unwrap();
    let code = staged.code();
    let allocator = Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, code, oxc_span::SourceType::mjs())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{code}",
        parsed.diagnostics
    );
    assert_eq!(code.matches("unref as _unref").count(), 1, "{code}");
    assert!(code.contains("_unref(value)"), "{code}");
    assert!(code.contains("_unref(local)"), "{code}");
}

#[test]
fn inline_target_reuses_expression_and_style_prerequisites() {
    use crate::compile::render_route::{with_route, RenderRoute};
    use crate::style_planner::{parse_ir_invocation_count, reset_parse_ir_invocation_count};
    use crate::template::oxc::{
        parse_template_expressions_call_count, reset_parse_template_expressions_calls,
    };
    let source = "<script setup>import { ref } from 'vue'; const count = ref(1)</script><template><div>{{ count }}</div></template><style scoped>.x { color: v-bind(count) }</style>";
    reset_parse_template_expressions_calls();
    reset_parse_ir_invocation_count();
    let allocator = Allocator::default();
    let (_, compiled) = with_route(RenderRoute::Targets, || {
        crate::compile::compile_with_parsed(
            source,
            &inline_request(true, true),
            &Default::default(),
            &Default::default(),
            &allocator,
        )
    })
    .unwrap();
    assert!(compiled.errors.is_empty(), "{:?}", compiled.errors);
    assert_eq!(parse_template_expressions_call_count(), 1);
    assert_eq!(parse_ir_invocation_count(), 1);
    let code = &compiled.script.unwrap().code;
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, code, oxc_span::SourceType::mjs())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{code}",
        parsed.diagnostics
    );
    assert_eq!(code.matches("count.value").count(), 2, "{code}");
}

#[test]
fn server_relocated_types_are_removed_before_slot_moves() {
    let source = r#"<script setup lang="ts">const value = 'ok'</script>
<template><Child>{{ value! }}<template #named>named</template></Child></template>"#;
    for map in [true, false] {
        let set = compile_with(
            source,
            &runtime_request(vec![runtime_product(ProductKind::RuntimeServer, map)]),
            &Default::default(),
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
        .expect("a moved default slot compiles to JavaScript");
        let staged = set.stage(ProductKind::RuntimeServer).unwrap();
        let code = staged.code();
        let allocator = Allocator::default();
        let parsed =
            verter_parser::oxc_parse::Parser::new(&allocator, code, oxc_span::SourceType::mjs())
                .parse();
        assert!(
            parsed.diagnostics.is_empty(),
            "{:?}\n{code}",
            parsed.diagnostics
        );
        assert!(code.contains("_ssrInterpolate($setup.value)"), "{code}");
        assert!(
            code.find("named: _withCtx(").unwrap() < code.find("default: _withCtx(").unwrap(),
            "{code}"
        );
        assert_eq!(staged.source_map().is_some(), map);
        if let Some(json) = staged.source_map() {
            let decoded = oxc_sourcemap::SourceMap::from_json_string(json).unwrap();
            let lookup = decoded.generate_lookup_table();
            let needle = "_ssrInterpolate($setup.value)";
            let generated = code.find(needle).unwrap() + needle.find("value").unwrap();
            let (mut line, mut column) = (0, 0);
            crate::code_transform::advance_generated_position(
                &code[..generated],
                &mut line,
                &mut column,
            );
            let token = decoded.lookup_token(&lookup, line, column).unwrap();
            let authored = source.rfind("value!").unwrap();
            let (mut source_line, mut source_column) = (0, 0);
            crate::code_transform::advance_generated_position(
                &source[..authored],
                &mut source_line,
                &mut source_column,
            );
            assert_eq!(
                (token.get_src_line(), token.get_src_col()),
                (source_line, source_column),
            );
            assert_eq!(
                token.get_source_id().and_then(|id| decoded.get_source(id)),
                Some("App.vue")
            );
        }
    }
}

fn assert_runtime_module_for(
    source: &str,
    product: ProductKind,
    backend: crate::compile_request::VueBackendRequest,
    inline: bool,
) -> String {
    let runtime = RuntimeProductRequest {
        inline: Some(inline),
        runtime_source_map: true,
        ..Default::default()
    };
    let request = CompileRequest::new(
        vec![match product {
            ProductKind::RuntimeServer => CompileProduct::RuntimeServer(runtime),
            _ => CompileProduct::RuntimeClient(runtime),
        }],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            backend,
            ..Default::default()
        }),
        None,
        Some("App.vue".into()),
        None,
        false,
        true,
    )
    .unwrap();
    let staged = compile_with(
        source,
        &request,
        &Default::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .unwrap()
    .stage(product)
    .unwrap();
    let code = staged.code();
    let allocator = Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, code, oxc_span::SourceType::mjs())
            .parse();
    assert!(
        !parsed.fatal_error && parsed.diagnostics.is_empty(),
        "{:?}\n{code}",
        parsed.diagnostics
    );
    code.to_string()
}

#[test]
fn runtime_modules_remove_types_from_relocated_expressions() {
    let source = r#"<script setup lang="ts">const item = { n: 1 }</script><template><Child :n="item!.n">{{ item!.n }}</Child></template>"#;
    assert_runtime_module_for(
        source,
        ProductKind::RuntimeServer,
        crate::compile_request::VueBackendRequest::Vdom,
        false,
    );
    for expression in ["count!++", "++count!", "items![0]++"] {
        let source = format!(
            r#"<script setup lang="ts">let count = 0; const items = [0]</script><template>{{{{ {expression} }}}}</template>"#
        );
        for (product, inline) in [
            (ProductKind::RuntimeClient, false),
            (ProductKind::RuntimeClient, true),
            (ProductKind::RuntimeServer, false),
        ] {
            assert_runtime_module_for(
                &source,
                product,
                crate::compile_request::VueBackendRequest::Vdom,
                inline,
            );
        }
    }
}

#[test]
fn client_module_removes_types_from_loop_sources() {
    for iterable in ["items!", "items! ", "  items!    "] {
        let source = format!(
            r#"<script setup lang="ts">const items = [1]</script><template><div v-for="item in {iterable}">{{{{ item }}}}</div></template>"#
        );
        for inline in [false, true] {
            assert_runtime_module_for(
                &source,
                ProductKind::RuntimeClient,
                crate::compile_request::VueBackendRequest::Vdom,
                inline,
            );
        }
    }
}

#[test]
fn entity_only_conditional_children_form_valid_modules() {
    let source = "<template><div v-if=\"ok\">&nbsp;</div></template>";
    for product in [ProductKind::RuntimeClient, ProductKind::RuntimeServer] {
        let code = assert_runtime_module_for(
            source,
            product,
            crate::compile_request::VueBackendRequest::Vdom,
            false,
        );
        assert!(code.contains('\u{a0}'), "{code}");
    }
}

#[test]
fn conditional_components_before_named_slots_form_valid_modules() {
    let source = "<template><Outer><A v-if=\"ok\"/><B v-else/><template #fallback>wait</template></Outer></template>";
    assert_runtime_module_for(
        source,
        ProductKind::RuntimeServer,
        crate::compile_request::VueBackendRequest::Vdom,
        false,
    );
}
#[test]
fn fine_grained_loop_shorthand_keeps_its_property_key() {
    let source =
        "<template><div v-for=\"model in models\">{{ nameOf({ model }) }}</div></template>";
    let code = assert_runtime_module_for(
        source,
        ProductKind::RuntimeClient,
        crate::compile_request::VueBackendRequest::Vapor,
        false,
    );
    assert!(code.contains("model:"), "{code}");
}
