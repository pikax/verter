//! The fragment route composes, from the same producers' output, the bytes
//! and decoded map tokens the string route composes, without building a
//! string, a JSON map or a map chain on the way.

use oxc_allocator::Allocator;
use oxc_sourcemap::SourceMap;
use oxc_span::SourceType;
use verter_audit::observe::{Captured, Uncaptured};

use super::{
    compose_main_fragments, compose_main_module, SfcSlots, VueMainDecoration,
    VueMainFragmentsRequest, VueMainModuleRequest, VueRenderPlans, VueScriptFragment,
};
use crate::assembly::emit_plan::{
    EmitPlanBuilder, EmitPlanError, EmittedArtifact, MapMode, MapSource,
};
use crate::assembly::fragment::{
    test_witness, FragmentDialect, SfcExportPlacement, SyntacticContract,
};
use crate::assembly::map_compose::take_source_maps_built;
use crate::code_transform::{
    code_transform_build_string_call_count, reset_code_transform_build_string_call_count,
    CodeTransform, GeneratedContentMarker, SourceMapOptions,
};
use crate::compile::format_import_specifier;
use crate::compile_request::ProductKind;
use crate::diagnostics::{SyntaxPluginContext, SyntaxPluginOptions};
use crate::emission::{test_source, MarkerId, QualifiedSource};
use crate::framework::vue::target::VueHelperSet;
use crate::framework_common::{
    RuntimeCompileOutput, RuntimeOutputDescriptor, RuntimeScriptBlock, RuntimeTemplateBlock,
    SourceMapFidelity, TemplateRenderExport,
};
use crate::parser::Syntax;
use crate::script::bindings::script_lane_bindings;
use crate::script::prepared::PreparedScript;
use crate::script::{generate_script, ScriptCodeGenOptions};
use crate::template::binding::BindingType;
use crate::template::code_gen::{generate_template, TemplateCodeGenOptions};
use crate::template::oxc::parse_template_expressions;
use crate::tokenizer::byte::tokenize;
use crate::work_ledger::CompilerWorkLedger;
#[cfg(feature = "semantic-observe")]
use crate::work_ledger::WorkKind;

const NAME: &str = "App.vue";
const SCOPE_ID: &str = "data-v-7ba5bd90";

/// A `<script setup>` and nothing else.
const SETUP_ONLY: &str =
    "<script setup>\nimport { ref } from 'vue'\nconst count = ref(0)\n</script>\n";
/// A normal `<script>` beside `<script setup>`, with a template.
const NORMAL_AND_SETUP: &str = "<script>\nexport default { name: 'Named' }\n</script>\n\
<script setup>\nconst a = 1\n</script>\n\
<template><div>{{ a }}</div></template>\n";
/// A render function that hoists a static props array.
const RENDER_WITH_HOISTS: &str = "<template><div><span :id=\"x\">hello</span></div></template>\n\
<script setup>\nimport { ref } from 'vue'\nconst x = ref(1)\n</script>\n";
/// A scoped Options-API component: the binding is declared, then read
/// again for the scope id, then exported.
const BINDING_REFERENCED_TWICE: &str =
    "<script>\nexport default { data: () => ({ n: 1 }) }\n</script>\n\
<template><p>{{ n }}</p></template>\n\
<style scoped>\n.a { color: red }\n</style>\n";
/// A script without a default export: the producer writes the component and
/// the export statement assembly drops.
const EXPORT_STATEMENT_DROP: &str = "<script>\nconst helper = 1\n</script>\n";

const CASES: [(&str, &str); 5] = [
    ("setup only", SETUP_ONLY),
    ("normal plus setup", NORMAL_AND_SETUP),
    ("render with hoists", RENDER_WITH_HOISTS),
    ("binding referenced twice", BINDING_REFERENCED_TWICE),
    ("export statement drop", EXPORT_STATEMENT_DROP),
];

/// The script lowering's finished transform and what it declared.
struct ProducedScript<'a> {
    transform: CodeTransform<'a>,
    bindings: Vec<GeneratedContentMarker<'a>>,
    export_statement: Option<GeneratedContentMarker<'a>>,
    imports: Vec<&'static str>,
    setup: bool,
}

/// The render codegen's finished transform and its helper imports.
struct ProducedRender<'a> {
    transform: CodeTransform<'a>,
    imports: Vec<&'static str>,
    ssr_imports: Vec<&'static str>,
}

/// Run the script lowering and the render codegen over `source` the way the
/// compiler's runtime lanes do: blocks outside a transform's own are
/// removed and hoisted constants are prepended. The script's helper import
/// line is prepended when `import_line` asks for it, as the string route
/// does; the fragment route has the composer write it.
fn produce<'a>(
    source: &'a str,
    alloc: &'a Allocator,
    mode: MapMode,
    import_line: bool,
) -> (Option<ProducedScript<'a>>, Option<ProducedRender<'a>>) {
    let options = SyntaxPluginOptions::default();
    let context = SyntaxPluginContext {
        input: source,
        bytes: source.as_bytes(),
        options: &options,
        diagnostics: Vec::new(),
    };
    let mut syntax = Syntax::new(false);
    tokenize(source.as_bytes(), |event| syntax.handle(&event, &context));
    let len = source.len() as u32;
    let has_scoped_style = syntax.has_style_scope();
    let has_script = syntax.script().is_some() || syntax.script_setup().is_some();

    let mut template_bindings: rustc_hash::FxHashMap<&'a str, BindingType> = Default::default();
    let script = has_script.then(|| {
        let mut transform = CodeTransform::with_mode_in(source, alloc, mode);
        let prepared = PreparedScript::build(source, syntax.script(), syntax.script_setup(), alloc);
        let options = ScriptCodeGenOptions {
            component_name: "App",
            scope_id: SCOPE_ID,
            has_scoped_style,
            ..Default::default()
        };
        let lane = script_lane_bindings(syntax.script_setup(), &prepared, None, alloc);
        let result = generate_script(
            syntax.script(),
            syntax.script_setup(),
            &prepared,
            source,
            &mut transform,
            alloc,
            &options,
            lane,
        )
        .expect("recorded script edits apply");

        if syntax.script_setup().is_some() {
            if let Some(companion) = syntax.script() {
                transform.remove(companion.tag_open.start, companion.tag_open.end);
                if let Some(close) = &companion.tag_close {
                    transform.remove(close.start, close.end);
                }
            }
        }
        let mut blocks: Vec<(u32, u32)> = [syntax.script(), syntax.script_setup()]
            .into_iter()
            .flatten()
            .map(|block| {
                let end = block
                    .tag_close
                    .as_ref()
                    .map_or(block.tag_open.end, |close| close.end);
                (block.tag_open.start, end)
            })
            .collect();
        blocks.sort_unstable();
        let mut at = 0;
        for (start, end) in blocks {
            if at < start {
                transform.remove(at, start);
            }
            at = end;
        }
        if at < len {
            transform.remove(at, len);
        }
        if import_line && !result.imports.is_empty() {
            let specifiers: Vec<String> = result
                .imports
                .iter()
                .map(|name| format_import_specifier(name))
                .collect();
            transform.prepend(&format!(
                "import {{ {} }} from \"vue\"\n",
                specifiers.join(", ")
            ));
        }
        template_bindings = result.bindings;
        ProducedScript {
            transform,
            bindings: result.sfc_binding_markers,
            export_statement: result.sfc_export_statement_marker,
            imports: result.imports.clone(),
            setup: syntax.script_setup().is_some(),
        }
    });

    let render = syntax.take_template_ast().map(|ast| {
        let expressions = parse_template_expressions(&ast, source, alloc, SourceType::tsx(), false);
        let mut transform = CodeTransform::with_mode_in(source, alloc, mode);
        let start = ast.root.tag_open.start;
        let end = ast
            .root
            .tag_close
            .as_ref()
            .map_or(ast.root.tag_open.end, |close| close.end);
        if start > 0 {
            transform.remove(0, start);
        }
        if end < len {
            transform.remove(end, len);
        }
        let options = TemplateCodeGenOptions {
            has_script,
            has_scoped_style,
            ..Default::default()
        };
        let imports = generate_template(
            &ast,
            &expressions,
            source,
            &mut transform,
            alloc,
            std::mem::take(&mut template_bindings),
            &options,
        )
        .expect("recorded template edits apply");
        if let Some(preamble) = imports.module_preamble {
            transform.prepend(preamble);
        }
        ProducedRender {
            transform,
            imports: imports.vue,
            ssr_imports: imports.ssr,
        }
    });
    (script, render)
}

fn map_options() -> SourceMapOptions<'static> {
    SourceMapOptions {
        source: Some(NAME),
        file: Some(NAME),
        include_content: true,
    }
}

fn descriptor(code: &str, map: &str) -> RuntimeOutputDescriptor {
    RuntimeOutputDescriptor::generated(
        code,
        Some(map),
        &[("test:space", "test:artifact")],
        SourceMapFidelity::Approximate,
    )
}

/// The string route: each producer builds its text and V3 map, resolves its
/// markers into byte ranges, and `compose_main_module` renames, chains and
/// sequences. Returns the module text and its map.
fn string_route<'a>(
    script: Option<&ProducedScript<'a>>,
    render: Option<&ProducedRender<'a>>,
) -> (String, String) {
    let script_block = script.map(|script| {
        let code = script.transform.build_string();
        let map = script
            .transform
            .generate_map_json(map_options())
            .expect("captured");
        let range = |marker: &GeneratedContentMarker<'a>| {
            script
                .transform
                .generated_content_range(*marker)
                .expect("the marker resolves")
        };
        let placement = SfcExportPlacement {
            binding_ranges: script.bindings.iter().map(range).collect(),
            export_statement_range: script.export_statement.as_ref().map(range),
        };
        RuntimeScriptBlock {
            output_descriptor: descriptor(&code, &map),
            code,
            source_map: map,
            setup: script.setup,
            generated_template_hole: None,
            runtime_imports: owned(&script.imports),
            sfc_export_placement: Some(placement),
        }
    });
    let template_block = render.map(|render| {
        let code = render.transform.build_string();
        let map = render
            .transform
            .generate_map_json(map_options())
            .expect("captured");
        RuntimeTemplateBlock {
            output_descriptor: descriptor(&code, &map),
            code,
            source_map: map,
            imports: owned(&render.imports),
            ssr_imports: owned(&render.ssr_imports),
            render_export: TemplateRenderExport::Render,
        }
    });
    let script_map_json = script_block.as_ref().map(|block| block.source_map.clone());
    let script_map = script_map_json
        .as_deref()
        .map(|json| SourceMap::from_json_string(json).expect("a V3 map"));
    let template_map_json = template_block
        .as_ref()
        .map(|block| block.source_map.clone());
    let compiled = RuntimeCompileOutput {
        script: script_block,
        template: template_block,
        scope_id: SCOPE_ID.to_owned(),
        ..RuntimeCompileOutput::default()
    };
    let set = compose_main_module(VueMainModuleRequest {
        canonical_id: NAME,
        compiled: &compiled,
        dialect: FragmentDialect::JavaScript,
        planned_kind: ProductKind::RuntimeClient,
        runtime: "vue",
        want_maps: true,
        source_root: None,
        script_map: script_map.as_ref(),
        template_map_json,
        decoration: VueMainDecoration::default(),
    })
    .expect("the string route composes");
    let artifact = set
        .artifact(ProductKind::RuntimeClient)
        .expect("the planned artifact");
    (
        artifact.code().to_owned(),
        artifact
            .runtime_source_map()
            .expect("a map was asked for")
            .to_owned(),
    )
}

fn owned(names: &[&'static str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// The fragment route: each producer's transform is frozen, the script's
/// markers as slots, and `compose_main_fragments` writes the module once.
fn fragment_route<'a>(
    source: &QualifiedSource<'a>,
    script: Option<ProducedScript<'a>>,
    render: Option<ProducedRender<'a>>,
    mode: MapMode,
    ledger: &mut CompilerWorkLedger<impl verter_audit::observe::ObserveExec>,
) -> EmittedArtifact {
    let script_helpers: Vec<&'static str> = script
        .as_ref()
        .map(|script| script.imports.clone())
        .unwrap_or_default();
    let script = script.map(|script| {
        let bindings: Vec<MarkerId> = script.bindings.iter().map(|_| MarkerId::fresh()).collect();
        let export_statement = script.export_statement.map(|_| MarkerId::fresh());
        let mut marks: Vec<_> = script
            .bindings
            .iter()
            .copied()
            .zip(bindings.iter().copied())
            .collect();
        marks.extend(script.export_statement.zip(export_statement));
        let fragment = script
            .transform
            .freeze_marked(source.clone(), &marks)
            .expect("every declared position is generated text");
        let slots = SfcSlots::of(&fragment, &bindings, export_statement, None);
        (fragment, slots)
    });
    let render_helpers = render.as_ref().map(|render| VueHelperSet {
        runtime: render.imports.clone(),
        server: render.ssr_imports.clone(),
    });
    let render = render.map(|render| {
        let mut plan = EmitPlanBuilder::new(mode);
        plan.push_transform(render.transform, source.clone())
            .expect("the transform's own source");
        plan.finish().expect("one fragment")
    });
    compose_main_fragments(
        VueMainFragmentsRequest {
            canonical_id: NAME,
            dialect: FragmentDialect::JavaScript,
            planned_kind: ProductKind::RuntimeClient,
            runtime: "vue",
            mode,
            inline: false,
            style_count: 0,
            custom_block_count: 0,
            scope_id: SCOPE_ID,
            script: script.map(|(fragment, slots)| VueScriptFragment {
                fragment,
                slots,
                helpers: &script_helpers,
                witness: None,
            }),
            render: render
                .zip(render_helpers.as_ref())
                .map(|(render, helpers)| VueRenderPlans {
                    preamble: None,
                    render,
                    helpers,
                    render_export: TemplateRenderExport::Render,
                }),
            decoration: VueMainDecoration::default(),
        },
        ledger,
    )
    .expect("the fragment route composes")
}

/// Decoded tokens by authored source name, as a line-scoped lookup reads
/// them. A token that changes no lookup is left out: a sourceless one with
/// no mapped token before it on its line, and one repeating the previous
/// token's authored position on the same line.
type DecodedToken = (u32, u32, Option<(u32, u32, String)>);

fn decoded_tokens(map: &SourceMap<'_>) -> Vec<DecodedToken> {
    let mut tokens: Vec<DecodedToken> = Vec::new();
    for token in map.get_tokens() {
        let source = token
            .get_source_id()
            .and_then(|id| map.get_source(id))
            .map(|name| (token.get_src_line(), token.get_src_col(), name.to_owned()));
        let line = token.get_dst_line();
        let previous = tokens
            .last()
            .filter(|(last, _, _)| *last == line)
            .map(|(_, _, previous)| previous);
        let changes_nothing = match (&source, previous) {
            (None, Some(Some(_))) => false,
            (None, _) => true,
            (Some(_), previous) => previous == Some(&source),
        };
        if changes_nothing {
            continue;
        }
        tokens.push((line, token.get_dst_col(), source));
    }
    tokens
}

#[test]
fn fragments_compose_the_bytes_and_tokens_the_string_route_composes() {
    for (case, sfc) in CASES {
        let alloc = Allocator::default();
        let (script, render) = produce(sfc, &alloc, MapMode::Map, true);
        assert!(
            script
                .as_ref()
                .is_some_and(|script| !script.bindings.is_empty()),
            "{case}: the script declares its bindings"
        );
        let (string_text, string_map) = string_route(script.as_ref(), render.as_ref());
        let (script, render) = produce(sfc, &alloc, MapMode::Map, false);

        let source = test_source(NAME, sfc);
        let written = fragment_route(
            &source,
            script,
            render,
            MapMode::Map,
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        );
        assert_eq!(written.text(), string_text, "{case}: module bytes");

        let fragment_map = written.map().expect("a mapped plan").to_source_map(
            written.text(),
            &[MapSource {
                name: NAME,
                content: Some(sfc),
            }],
            None,
        );
        let string_map = SourceMap::from_json_string(&string_map).expect("a V3 map");
        let fragment_tokens = decoded_tokens(&fragment_map);
        assert!(
            fragment_tokens
                .iter()
                .any(|(_, _, source)| source.is_some()),
            "{case}: the module maps authored text"
        );
        assert_eq!(
            fragment_tokens,
            decoded_tokens(&string_map),
            "{case}: decoded tokens of\n{string_text}"
        );
    }
    // The cases reach every slot kind and the render prelude.
    let hoisted = {
        let alloc = Allocator::default();
        let (script, render) = produce(RENDER_WITH_HOISTS, &alloc, MapMode::Map, true);
        string_route(script.as_ref(), render.as_ref()).0
    };
    assert!(hoisted.contains("const _hoisted_1 = "), "{hoisted}");
}

#[test]
fn authored_text_matching_the_landmarks_is_left_untouched() {
    let sfc = "<script>\nconst decoy = \"__sfc__\"\nexport default { name: \"export default __sfc__;\" }\n</script>\n";
    let alloc = Allocator::default();
    let (script, render) = produce(sfc, &alloc, MapMode::Map, false);
    let source = test_source(NAME, sfc);
    let written = fragment_route(
        &source,
        script,
        render,
        MapMode::Map,
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    );
    let text = written.text();
    assert!(text.contains("const decoy = \"__sfc__\""), "{text}");
    assert!(text.contains("name: \"export default __sfc__;\""), "{text}");
    assert!(text.contains("const _sfc_main = {"), "{text}");
    assert!(!text.contains("export default __sfc__;\n"), "{text}");
    assert!(text.ends_with("export default _sfc_main"), "{text}");
}

#[test]
fn a_main_module_is_materialised_once() {
    // Script lowering records its edits for a mapped transform.
    let alloc = Allocator::default();
    let (script, render) = produce(RENDER_WITH_HOISTS, &alloc, MapMode::Map, false);
    let source = test_source(NAME, RENDER_WITH_HOISTS);
    reset_code_transform_build_string_call_count();
    take_source_maps_built();
    let mut ledger = CompilerWorkLedger::<Captured>::new();
    let written = fragment_route(&source, script, render, MapMode::Map, &mut ledger);
    assert!(written.text().contains("_sfc_main.render = render"));
    assert_eq!(
        code_transform_build_string_call_count(),
        0,
        "no producer builds a string"
    );
    assert_eq!(
        take_source_maps_built(),
        0,
        "no map is encoded between producers"
    );
    // The ledger counts only in the observing build.
    #[cfg(feature = "semantic-observe")]
    {
        let counts = ledger.snapshot().expect("a captured ledger");
        assert_eq!(
            counts.count(WorkKind::EmissionAllocations),
            1,
            "one buffer for the module"
        );
    }
    assert!(
        written.map().is_some(),
        "the decoded map comes from the same write"
    );
}

/// A script module whose component object returns from `setup()` where the
/// render goes, declared as the setup-return slot.
const INLINE_SCRIPT: &str = "const __sfc__ = { setup() {/* hole */} }\nexport default __sfc__;\n";

/// The script fragment of `INLINE_SCRIPT`, its slots with or without the
/// setup-return one, over the carrier `<x>`.
fn inline_script<'a>(
    allocator: &'a Allocator,
    source: &QualifiedSource<'a>,
    with_hole: bool,
) -> (crate::emission::EmissionFragment<'a>, SfcSlots) {
    let mut ct: CodeTransform<'a> =
        CodeTransform::with_mode_in(source.text, allocator, MapMode::Map);
    let content = ct.alloc_str(INLINE_SCRIPT);
    ct.overwrite_unmapped_alloc(0, 3, content);
    let at = |text: &str| INLINE_SCRIPT.find(text).unwrap() as u32;
    let mark = |start: u32, end: u32| ct.mark_generated_sub_range(content, start, end).unwrap();
    let binding = mark(6, 13);
    let hole = mark(at("/* hole */"), at("/* hole */") + 10);
    let export = mark(at("export"), INLINE_SCRIPT.len() as u32);
    let (binding_id, hole_id, export_id) =
        (MarkerId::fresh(), MarkerId::fresh(), MarkerId::fresh());
    let mut marks = vec![(binding, binding_id), (export, export_id)];
    if with_hole {
        marks.push((hole, hole_id));
    }
    let fragment = ct.freeze_marked(source.clone(), &marks).unwrap();
    let slots = SfcSlots::of(
        &fragment,
        &[binding_id],
        Some(export_id),
        with_hole.then_some(hole_id),
    );
    (fragment, slots)
}

fn inline_request<'s>(
    script: VueScriptFragment<'s>,
    render: VueRenderPlans<'s>,
) -> VueMainFragmentsRequest<'s> {
    VueMainFragmentsRequest {
        canonical_id: NAME,
        dialect: FragmentDialect::JavaScript,
        planned_kind: ProductKind::RuntimeClient,
        runtime: "vue",
        mode: MapMode::Map,
        inline: true,
        style_count: 0,
        custom_block_count: 0,
        scope_id: SCOPE_ID,
        script: Some(script),
        render: Some(render),
        decoration: VueMainDecoration::default(),
    }
}

fn unmapped_plan(text: &str) -> crate::assembly::emit_plan::EmitPlan<'_> {
    let mut plan = EmitPlanBuilder::new(MapMode::Map);
    plan.push_unmapped(text);
    plan.finish().unwrap()
}

#[test]
fn an_inline_render_fills_the_setup_return_slot() {
    let allocator = Allocator::default();
    let source = test_source(NAME, "<x>");
    let (fragment, slots) = inline_script(&allocator, &source, true);
    let helpers = VueHelperSet {
        runtime: vec!["_openBlock", "_unref"],
        server: Vec::new(),
    };
    let written = compose_main_fragments(
        inline_request(
            VueScriptFragment {
                fragment,
                slots,
                helpers: &["_defineComponent", "_unref"],
                witness: None,
            },
            VueRenderPlans {
                preamble: Some(unmapped_plan("const _hoisted_1 = 1\n")),
                render: unmapped_plan("return () => null"),
                helpers: &helpers,
                render_export: TemplateRenderExport::Render,
            },
        ),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    )
    .expect("the hole takes the render");
    assert_eq!(
        written.text(),
        "import { defineComponent as _defineComponent, unref as _unref } from \"vue\"\n\
         import { openBlock as _openBlock } from \"vue\"\n\
         const _hoisted_1 = 1\n\
         const _sfc_main = { setup() {\nreturn () => null} }\n\
         export default _sfc_main"
    );
}

#[test]
fn an_inline_render_without_a_declared_hole_is_refused() {
    let allocator = Allocator::default();
    let source = test_source(NAME, "<x>");
    let (fragment, slots) = inline_script(&allocator, &source, false);
    let helpers = VueHelperSet::default();
    let refused = compose_main_fragments(
        inline_request(
            VueScriptFragment {
                fragment,
                slots,
                helpers: &[],
                witness: None,
            },
            VueRenderPlans {
                preamble: None,
                render: unmapped_plan("return () => null"),
                helpers: &helpers,
                render_export: TemplateRenderExport::Render,
            },
        ),
        &mut CompilerWorkLedger::<Uncaptured>::new(),
    );
    assert_eq!(refused.unwrap_err(), EmitPlanError::UnknownSlot);
}

#[test]
fn a_module_whose_parts_are_witnessed_stays_witnessed() {
    let witness = test_witness(
        SyntacticContract::CompleteModule,
        FragmentDialect::JavaScript,
    );
    for script_witness in [Some(witness), None] {
        let allocator = Allocator::default();
        let source = test_source(NAME, "<x>");
        let (fragment, slots) = inline_script(&allocator, &source, false);
        let mut render = EmitPlanBuilder::witnessed(MapMode::Map, witness);
        render.push_unmapped("export function render() {}\n");
        let helpers = VueHelperSet::default();
        let request = VueMainFragmentsRequest {
            inline: false,
            ..inline_request(
                VueScriptFragment {
                    fragment,
                    slots,
                    helpers: &[],
                    witness: script_witness,
                },
                VueRenderPlans {
                    preamble: None,
                    render: render.finish().unwrap(),
                    helpers: &helpers,
                    render_export: TemplateRenderExport::Render,
                },
            )
        };
        let written =
            compose_main_fragments(request, &mut CompilerWorkLedger::<Uncaptured>::new()).unwrap();
        assert_eq!(
            written.grammar().is_some(),
            script_witness.is_some(),
            "{}",
            written.text()
        );
    }
}
