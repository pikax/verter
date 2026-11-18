//! Shared prerequisites for fine-grained pipeline tests.
use crate::assembly::emit_plan::MapMode;
use crate::ast::types::TemplateAst;
use crate::compile::types::VueMacroSemanticInput;
use crate::compile_request::demand::{close_demand, DemandSeed, TargetKind};
use crate::compile_request::{ProductKind, RuntimeStyleProcessing};
use crate::framework::vue::semantic::{build_semantic_snapshot, VueSemanticInputs};
use crate::framework::vue::structure::build_compile_structure;
use crate::framework::vue::style::{ScopeId, VueComponentScopePlan};
use crate::framework::vue::target::{VueTargetInputs, VueTargetOptions};
use crate::target_plan::TargetExecutor;
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::types::BindingType;
use crate::work_ledger::CompilerWorkLedger;
use oxc_allocator::Allocator;
use verter_audit::observe::ObserveExec;

pub(super) fn with_inputs<M: ObserveExec, R>(
    source: &str,
    map_mode: MapMode,
    inline: bool,
    bindings: &[(&'static str, BindingType)],
    ledger: &mut CompilerWorkLedger<M>,
    run: impl for<'c> FnOnce(&'c VueTargetInputs<'c>, &mut CompilerWorkLedger<M>) -> R,
) -> R {
    let mut parsed = crate::framework_common::vue_bridge::parse_sfc(source, None, None);
    let ast: TemplateAst = parsed.template_ast.take().expect("template block");
    let topology = build_template_topology(source, &ast, TopologyRecoveryFacts::default(), ledger)
        .expect("topology builds");
    let allocator = Allocator::default();
    let has_script = parsed.script().is_some() || parsed.script_setup().is_some();
    let prepared = crate::script::prepared::PreparedScript::build(
        source,
        parsed.script(),
        parsed.script_setup(),
        &allocator,
    );
    let lane = crate::script::bindings::script_lane_bindings(
        parsed.script_setup(),
        &prepared,
        None,
        &allocator,
    );
    let ref_bindable_imports = lane
        .ref_bindable_imports
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let mut reported_bindings = lane.bindings;
    reported_bindings.extend(bindings.iter().copied());
    let demand = close_demand(&[DemandSeed {
        product: ProductKind::RuntimeClient,
        target: TargetKind::ClientFineGrained,
        runtime_source_map: map_mode == MapMode::Map,
        style_processing: RuntimeStyleProcessing::Complete,
        has_template: true,
        has_script,
        style_count: 0,
    }]);
    let semantic = build_semantic_snapshot(
        VueSemanticInputs {
            source,
            topology: Some(&topology),
            reported_bindings,
            setup_analysis: None,
            companion_analysis: None,
            macros: &VueMacroSemanticInput::Unavailable,
            runtime_module: "vue",
            self_name: "App",
            template_is_ts: true,
            allocator: &allocator,
        },
        &demand,
        ledger,
    )
    .expect("snapshot builds");
    let structure = build_compile_structure(source, &topology, &semantic, true, true, ledger)
        .expect("structure builds");
    let scope = VueComponentScopePlan {
        scope: ScopeId::for_component("App.vue"),
        has_scoped: false,
        has_slotted: false,
        css_vars: Vec::new(),
        css_vars_complete: true,
        modules: Vec::new(),
    };
    let options = VueTargetOptions {
        inline,
        is_production: false,
        comments: true,
        force_js: true,
        self_name: "App".to_string(),
        has_script: has_script || !bindings.is_empty(),
        hoist_static: true,
        cache_handlers: true,
        const_props: None,
        ref_bindable_imports,
        map_mode,
    };
    let inputs = VueTargetInputs {
        source: crate::emission::test_source("App.vue", source),
        allocator: &allocator,
        topology: &topology,
        structure: &structure,
        semantic: &semantic,
        scope: &scope,
        options: &options,
    };
    run(&inputs, ledger)
}

pub(super) struct Rendered {
    pub(super) code: String,
    pub(super) helpers: Vec<&'static str>,
}

pub(super) fn render(source: &str) -> Rendered {
    render_with(source, &[])
}

pub(super) fn render_with(source: &str, bindings: &[(&'static str, BindingType)]) -> Rendered {
    with_inputs(
        source,
        MapMode::Map,
        false,
        bindings,
        &mut CompilerWorkLedger::<verter_audit::observe::Uncaptured>::new(),
        |inputs, ledger| {
            let output = super::VueVaporExecutor::execute(inputs, ledger)
                .expect("fine-grained target renders");
            let mut code = String::new();
            if let Some(preamble) = output.preamble {
                code.push_str(preamble.write(ledger).expect("preamble writes").text());
            }
            code.push_str(output.render.write(ledger).expect("render writes").text());
            assert_javascript(&code);
            Rendered {
                code,
                helpers: output.helpers.runtime,
            }
        },
    )
}

pub(super) fn assert_javascript(code: &str) {
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

pub(super) fn legacy_code(source: &str) -> String {
    legacy_code_with(source, &[])
}

fn legacy_code_with(source: &str, bindings: &[(&'static str, BindingType)]) -> String {
    with_inputs(
        source,
        MapMode::NoMap,
        false,
        bindings,
        &mut CompilerWorkLedger::<verter_audit::observe::Uncaptured>::new(),
        |inputs, _| {
            let root = &inputs.topology.ast().root;
            let end = root
                .tag_close
                .as_ref()
                .map(|tag| tag.end)
                .or_else(|| root.content.as_ref().map(|content| content.end))
                .unwrap_or(root.tag_open.end);
            let mut transform = crate::code_transform::CodeTransform::<
                verter_audit::observe::Uncaptured,
            >::for_window_in(
                source,
                verter_span::Span::new(root.tag_open.start, end),
                inputs.topology.len(),
                inputs.allocator,
                MapMode::Map,
            )
            .expect("template window");
            crate::template::code_gen::generate_template(
                inputs.topology.ast(),
                inputs.semantic.expressions(),
                source,
                &mut transform,
                inputs.allocator,
                inputs
                    .semantic
                    .bindings()
                    .iter()
                    .map(|(name, fact)| (name, fact.reported))
                    .collect(),
                &crate::template::code_gen::TemplateCodeGenOptions {
                    mode: crate::template::code_gen::CodeGenMode::Vapor,
                    is_inline: false,
                    comments: true,
                    force_js: true,
                    ..Default::default()
                },
            )
            .expect("legacy template emits");
            transform.build_string()
        },
    )
}

pub(super) fn assert_legacy(source: &str) -> Rendered {
    assert_legacy_with(source, &[])
}

pub(super) fn assert_legacy_with(
    source: &str,
    bindings: &[(&'static str, BindingType)],
) -> Rendered {
    let output = render_with(source, bindings);
    let previous = legacy_code_with(source, bindings);
    assert_javascript(&previous);
    assert_eq!(output.code, previous, "{source}");
    output
}

pub(super) fn legacy(source: &str) -> String {
    let code = legacy_code(source);
    assert_javascript(&code);
    code
}
