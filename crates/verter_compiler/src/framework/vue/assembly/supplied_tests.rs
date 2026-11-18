use super::*;
use crate::framework_common::RuntimeBlockContentInput;
use std::sync::Arc;

fn selected(code: &str, lang: &str) -> RuntimeBlockContentInput {
    RuntimeBlockContentInput {
        code: Arc::from(code),
        source_map: None,
        lang: lang.into(),
        content_artifact_token: format!("content:{lang}"),
        source_space_token: format!("space:{lang}"),
        parsed: None,
        producer: None,
        authored_basis: None,
        diagnostics: Vec::new(),
    }
}

fn supplied(
    source: &str,
    request: &CompileRequest,
    blocks: &RuntimeBlockContentInputs,
) -> Result<VueArtifactSet, VueCompileRefusal> {
    supplied_with_execution(source, request, blocks, &Default::default())
}

fn supplied_with_execution(
    source: &str,
    request: &CompileRequest,
    blocks: &RuntimeBlockContentInputs,
    execution: &VueExecutionInputs,
) -> Result<VueArtifactSet, VueCompileRefusal> {
    let parsed = crate::framework_common::vue_bridge::parse_sfc(source, None, None);
    compile_vue_runtime(
        &VueCompileInputs {
            source,
            parsed: &parsed,
            request,
            execution,
            macros: &Default::default(),
            block_content: blocks,
            decoration: &Default::default(),
            canonical_id: "App.vue",
        },
        &Allocator::default(),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
}

#[test]
fn supplied_template_usage_controls_setup_import_returns() {
    let source = "<script setup>import Child from './Child.vue'</script><template src=\"./view.html\"></template>";
    let blocks = RuntimeBlockContentInputs {
        template: Some(selected("<Child />", "html")),
        ..Default::default()
    };
    let execution = VueExecutionInputs {
        template_used_vars: Some(Default::default()),
        ..Default::default()
    };
    let staged = supplied_with_execution(source, &request(), &blocks, &execution)
        .unwrap()
        .stage(ProductKind::RuntimeClient)
        .unwrap();
    assert!(
        staged.code().contains("const __returned__ = { Child };"),
        "{}",
        staged.code()
    );
}

fn mapped_position(
    staged: &crate::assembly::StagedCompileArtifacts,
    needle: &str,
) -> (String, u32, u32) {
    let map = oxc_sourcemap::SourceMap::from_json_string(staged.source_map().unwrap()).unwrap();
    let offset = staged.code().rfind(needle).unwrap();
    let (mut line, mut column) = (0, 0);
    crate::code_transform::advance_generated_position(
        &staged.code()[..offset],
        &mut line,
        &mut column,
    );
    let lookup = map.generate_lookup_table();
    let token = map.lookup_token(&lookup, line, column).unwrap();
    (
        map.get_source(token.get_source_id().unwrap())
            .unwrap()
            .to_owned(),
        token.get_src_line(),
        token.get_src_col(),
    )
}

#[test]
fn supplied_fragments_keep_their_source_identity() {
    for inline in [false, true] {
        let source =
            "<script setup>const count = 1</script><template lang=\"pug\">p {{ count }}</template>";
        let template = "<p>{{count}}</p>";
        let blocks = RuntimeBlockContentInputs {
            template: Some(selected(template, "html")),
            ..Default::default()
        };
        let staged = supplied(
            source,
            &if inline {
                inline_request(true, true)
            } else {
                request()
            },
            &blocks,
        )
        .unwrap()
        .stage(ProductKind::RuntimeClient)
        .unwrap();
        assert_eq!(
            mapped_position(&staged, "count"),
            (
                "space:html".into(),
                0,
                template.find("count").unwrap() as u32
            )
        );
        assert_eq!(
            staged.code().contains("_sfc_main.render ="),
            !inline,
            "{}",
            staged.code()
        );
        let allocator = Allocator::default();
        let parsed = verter_parser::oxc_parse::Parser::new(
            &allocator,
            staged.code(),
            oxc_span::SourceType::mjs(),
        )
        .parse();
        assert!(
            parsed.diagnostics.is_empty(),
            "{:?}\n{}",
            parsed.diagnostics,
            staged.code()
        );
    }
    let source = "<script setup src=\"./logic.ts\"></script><template><p>{{count}}</p></template>";
    let blocks = RuntimeBlockContentInputs {
        script_setup: Some(selected("const count: number = 1", "ts")),
        ..Default::default()
    };
    let staged = supplied(source, &request(), &blocks)
        .unwrap()
        .stage(ProductKind::RuntimeClient)
        .unwrap();
    assert!(
        staged.code().contains("const count = 1"),
        "{}",
        staged.code()
    );
    assert_eq!(
        mapped_position(&staged, "const count"),
        ("space:ts".into(), 0, 0)
    );
    assert_eq!(
        mapped_position(&staged, "count"),
        ("App.vue".into(), 0, source.rfind("count").unwrap() as u32)
    );
}

#[test]
fn supplied_template_tokens_resolve_through_the_host_map() {
    let source = "<script setup>const count = 1</script><template src=\"./view.pug\"></template>";
    let mut template = selected("<p>{{count}}</p>", "html");
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let row = builder.add_source_and_content("view.pug", "\np= count");
    builder.add_token(0, 0, 1, 0, Some(row), None);
    builder.add_token(0, 5, 1, 3, Some(row), None);
    template.source_map = Some(Arc::from(builder.into_sourcemap().to_json_string()));
    let blocks = RuntimeBlockContentInputs {
        template: Some(template),
        ..Default::default()
    };
    for request in [request(), inline_request(true, true)] {
        let set = supplied(source, &request, &blocks).unwrap();
        let staged = set.stage(ProductKind::RuntimeClient).unwrap();
        assert_eq!(mapped_position(&staged, "count"), ("view.pug".into(), 1, 3));
        assert_eq!(
            mapped_position(&staged, "const count"),
            (
                "App.vue".into(),
                0,
                source.find("const count").unwrap() as u32
            )
        );
        assert_eq!(
            staged.source_map(),
            set.stage(ProductKind::RuntimeClient).unwrap().source_map()
        );
    }
}

#[test]
fn supplied_plain_scripts_remain_unavailable() {
    let source = "<script src=\"./plain.js\"></script><script setup src=\"./setup.js\"></script><template><p /></template>";
    for setup in [None, Some(selected("const count = 1", "js"))] {
        let blocks = RuntimeBlockContentInputs {
            script: Some(selected("export default {}", "js")),
            script_setup: setup,
            ..Default::default()
        };
        assert!(matches!(
            supplied(source, &request(), &blocks),
            Err(VueCompileRefusal::Target(VueTargetRefusal::Unsupported {
                code: "runtime-supplied-module-unsupported",
                ..
            }))
        ));
    }
}

#[test]
fn selected_template_semantics_are_validated() {
    for (source, blocks, invalid) in [
        ("<script setup>const count = 1</script><template src=\"./view.html\"></template>", RuntimeBlockContentInputs { template: Some(selected("<div v-slot>content</div>", "html")), ..Default::default() }, true),
        ("<script setup src=\"./setup.js\"></script><template><div v-slot>content</div></template>", RuntimeBlockContentInputs { script_setup: Some(selected("const count = 1", "js")), ..Default::default() }, true),
        ("<script setup>const count = 1</script><template><div v-slot>inactive</div></template>", RuntimeBlockContentInputs { template: Some(selected("<p>selected</p>", "html")), ..Default::default() }, false),
    ] {
        let result = supplied(source, &request(), &blocks);
        if invalid {
            assert!(matches!(&result, Err(VueCompileRefusal::InvalidSource { diagnostics }) if diagnostics.iter().any(|diagnostic| diagnostic.code == "XVSlotMisplaced")), "{result:?}");
        } else {
            assert!(result.is_ok(), "{result:?}");
        }
    }
}

#[test]
fn host_map_coordinates_cannot_cross_a_source_line() {
    let source = "<script setup>const count = 1</script><template src=\"./view.html\"></template>";
    let mut template = selected("<p>{{count}}</p>", "html");
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let row = builder.add_source_and_content("view.pug", "a\nb");
    builder.add_token(0, 0, 0, 2, Some(row), None);
    template.source_map = Some(Arc::from(builder.into_sourcemap().to_json_string()));
    let blocks = RuntimeBlockContentInputs {
        template: Some(template),
        ..Default::default()
    };
    let set = supplied(source, &request(), &blocks).unwrap();
    assert!(matches!(
        set.stage(ProductKind::RuntimeClient),
        Err(ArtifactSchemaError::InvalidSourceRange)
    ));
}

#[test]
fn supplied_host_maps_are_validated_only_for_mapped_products() {
    let source = "<script setup>const count = 1</script><template src=\"./view.html\"></template>";
    let mut template = selected("<p>{{ count }}</p>", "html");
    template.source_map = Some(Arc::from("invalid host map"));
    let blocks = RuntimeBlockContentInputs {
        template: Some(template),
        ..Default::default()
    };
    let set = supplied(source, &request(), &blocks).unwrap();
    assert!(set.stage(ProductKind::RuntimeClient).is_err());
    let no_map = runtime_request(vec![runtime_product(ProductKind::RuntimeClient, false)]);
    let staged = supplied(source, &no_map, &blocks)
        .unwrap()
        .stage(ProductKind::RuntimeClient)
        .unwrap();
    assert!(staged.source_map().is_none());
    assert!(staged.code().contains("_toDisplayString"));
}
