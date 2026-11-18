//! Server render functions written from the shared prerequisites.

use oxc_allocator::Allocator;
use verter_audit::observe::Uncaptured;
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{ContentId, SourceId, SourceUnitId};

use super::VueSsrExecutor;
use crate::assembly::emit_plan::MapMode;
use crate::assembly::source_space::SourceSpaceKind;
use crate::ast::types::TemplateAst;
use crate::compile::types::VueMacroSemanticInput;
use crate::compile_request::demand::{close_demand, DemandSeed, TargetKind};
use crate::compile_request::{ProductKind, RuntimeStyleProcessing};
use crate::emission::{QualifiedSource, SourceKey};
use crate::framework::vue::semantic::{build_semantic_snapshot, VueSemanticInputs};
use crate::framework::vue::structure::build_compile_structure;
use crate::framework::vue::style::{ScopeId, VueComponentScopePlan};
use crate::framework::vue::target::{VueTargetInputs, VueTargetOptions};
use crate::target_plan::TargetExecutor;
use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
use crate::types::BindingType;
use crate::work_ledger::CompilerWorkLedger;

/// Fixtures shared with the compiler's regression cases.
macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!(
            "../../../../tests/cases/sfc_regression_fixtures/",
            $name,
            ".vue"
        ))
    };
}

struct FixtureName;

impl CanonicalEncode for FixtureName {
    const DOMAIN_TAG: &'static str = "verter.compiler.ssr.fixture.v1";
    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, "App.vue");
    }
}

fn qualified(text: &str) -> QualifiedSource<'_> {
    QualifiedSource {
        key: SourceKey {
            unit: SourceUnitId::from_lineage(&SourceId::from_canonical(&FixtureName), "source"),
            content: ContentId::from_content_bytes(text.as_bytes()),
            space: SourceSpaceKind::Original,
        },
        text,
    }
}

/// The render function of `source` on the server target, with comments
/// kept or dropped.
fn render(source: &str, comments: bool) -> String {
    render_with(source, comments, &[])
}

/// [`render`] with the bindings the script reported; a component with
/// bindings has a script.
fn render_with(source: &str, comments: bool, bindings: &[(&str, BindingType)]) -> String {
    render_full(source, comments, bindings, false)
}

/// [`render_with`] for a component whose styles are scoped or not.
fn render_full(
    source: &str,
    comments: bool,
    bindings: &[(&str, BindingType)],
    scoped: bool,
) -> String {
    render_artifact(source, comments, bindings, scoped, MapMode::NoMap)
        .text()
        .to_string()
}

fn render_artifact(
    source: &str,
    comments: bool,
    bindings: &[(&str, BindingType)],
    scoped: bool,
    map_mode: MapMode,
) -> crate::assembly::emit_plan::EmittedArtifact {
    let parsed = crate::framework_common::vue_bridge::parse_sfc(source, None, None);
    let ast: TemplateAst = parsed.template_ast.expect("template block");
    let mut ledger = CompilerWorkLedger::<Uncaptured>::new();
    let topology =
        build_template_topology(source, &ast, TopologyRecoveryFacts::default(), &mut ledger)
            .expect("topology builds");
    let allocator = Allocator::default();
    let demand = close_demand(&[DemandSeed {
        product: ProductKind::RuntimeServer,
        target: TargetKind::Server,
        runtime_source_map: map_mode == MapMode::Map,
        style_processing: RuntimeStyleProcessing::Complete,
        has_template: true,
        has_script: !bindings.is_empty(),
        style_count: 0,
    }]);
    let semantic = build_semantic_snapshot(
        VueSemanticInputs {
            source,
            topology: Some(&topology),
            reported_bindings: bindings.iter().copied().collect(),
            setup_analysis: None,
            companion_analysis: None,
            macros: &VueMacroSemanticInput::Unavailable,
            runtime_module: "vue",
            self_name: "App",
            template_is_ts: false,
            allocator: &allocator,
        },
        &demand,
        &mut ledger,
    )
    .expect("snapshot builds");
    let structure =
        build_compile_structure(source, &topology, &semantic, true, comments, &mut ledger)
            .expect("structure builds");
    let scope = VueComponentScopePlan {
        scope: ScopeId::for_component("App.vue"),
        has_scoped: scoped,
        has_slotted: false,
        css_vars: Vec::new(),
        css_vars_complete: true,
        modules: Vec::new(),
    };
    let options = VueTargetOptions {
        inline: false,
        is_production: !comments,
        comments,
        force_js: false,
        self_name: "App".to_string(),
        has_script: !bindings.is_empty(),
        hoist_static: false,
        cache_handlers: false,
        const_props: None,
        ref_bindable_imports: Default::default(),
        map_mode,
    };
    let inputs = VueTargetInputs {
        source: qualified(source),
        allocator: &allocator,
        topology: &topology,
        structure: &structure,
        semantic: &semantic,
        scope: &scope,
        options: &options,
    };
    let output = VueSsrExecutor::execute(&inputs, &mut ledger).expect("server target renders");
    let artifact = output
        .render
        .write(&mut ledger)
        .expect("render plan writes");
    let parsed = verter_parser::oxc_parse::Parser::new(
        &allocator,
        artifact.text(),
        oxc_span::SourceType::mjs(),
    )
    .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "{:?}\n{}",
        parsed.diagnostics,
        artifact.text()
    );
    artifact
}

#[test]
fn native_slot_children_inherit_scope_alongside_the_component_scope() {
    let source = r#"<template><section><Child><button @click="act()">x</button><template #extra><b/></template></Child></section></template>"#;
    for scoped in [false, true] {
        let code = render_full(source, true, &[CHILD], scoped);
        let own = if scoped {
            format!(" {}", ScopeId::for_component("App.vue").attribute())
        } else {
            String::new()
        };
        assert!(
            code.contains(&format!("<section${{_ssrRenderAttrs(_attrs)}}{own}>")),
            "{code}"
        );
        for tag in ["button", "b"] {
            assert!(
                code.contains(&format!("<{tag}{own}${{_scopeId}}>")),
                "{code}"
            );
        }
    }
}

#[test]
fn implicit_default_fallback_patches_reactive_text_but_skips_setup_constants() {
    for (expression, binding, patch) in [
        ("label", BindingType::LiteralConst, ""),
        ("label", BindingType::SetupConst, ""),
        ("label", BindingType::SetupRef, ", 1 /* TEXT */"),
        ("label.value", BindingType::SetupConst, ", 1 /* TEXT */"),
    ] {
        let source = format!(
            "<template><Child><template #extra><b/></template><i>{{{{ {expression} }}}}</i></Child></template>"
        );
        let code = render_with(&source, true, &[CHILD, ("label", binding)]);
        assert_in_order(&code, &["extra: _withCtx(", "default: _withCtx("]);
        assert!(
            code.contains(&format!(
                "_createVNode(\"i\", null, _toDisplayString($setup.{expression}){patch})"
            )),
            "{code}"
        );
    }
}

#[test]
fn slot_content_is_handed_over_without_a_string() {
    crate::code_transform::reset_code_transform_build_string_call_count();
    let code = render("<template><Child><template #a>{{ first }}</template><template #b>{{ second }}</template></Child></template>", true);
    assert_in_order(
        &code,
        &[
            "a: _withCtx(",
            "_toDisplayString(_ctx.first)",
            "b: _withCtx(",
            "_toDisplayString(_ctx.second)",
        ],
    );
    assert_eq!(
        crate::code_transform::code_transform_build_string_call_count(),
        0
    );
}

#[test]
fn a_token_in_the_client_branch_of_a_slot_resolves_to_the_sfc() {
    for source in [
        "<template><Child><template #a>{{ first }}</template><template #b>{{ second }}</template></Child></template>",
        "<template><Child>{{ first }}<template #b>{{ second }}</template></Child></template>",
        "<template><Child><template #b>{{ second }}</template>{{ first }}</Child></template>",
        "<template><Child><template #a><Inner>{{ first }}</Inner></template><template #b>{{ second }}</template></Child></template>",
    ] {
    let artifact = render_artifact(source, true, &[], false, MapMode::Map);
    let map = artifact.map().expect("mapped render");
    for name in ["first", "second"] {
        let expression = format!("_toDisplayString(_ctx.{name})");
        assert!(artifact.text().contains(&expression), "client branch: {}", artifact.text());
        for (start, _) in artifact.text().match_indices(&expression) {
        let generated = start + "_toDisplayString(_ctx.".len();
        assert_eq!(
            map.lookup(generated as u32),
            Some((0, source.find(name).unwrap() as u32)),
            "{name}: {}",
            artifact.text()
        );
        }
    }
    assert_eq!(artifact.sources(), &[qualified(source).key]);
    crate::emission::reset_map_capture_events();
    let no_map = render_artifact(source, true, &[], false, MapMode::NoMap);
    assert_eq!(crate::emission::map_capture_events(), 0);
    assert!(no_map.map().is_none());
    assert_eq!(artifact.text(), no_map.text());
    }
}

#[test]
fn server_component_tags_follow_the_snapshots_binding_and_self_facts() {
    let code = render_with(
        "<template><Ns.Member/><App/><my-widget/></template>",
        true,
        &[
            ("Ns", BindingType::SetupConst),
            ("myWidget", BindingType::SetupConst),
        ],
    );
    assert!(
        code.contains("_ssrRenderComponent($setup[\"Ns\"].Member"),
        "{code}"
    );
    assert!(code.contains("_resolveComponent(\"App\", true)"), "{code}");
    assert!(
        code.contains("_ssrRenderComponent($setup[\"myWidget\"]"),
        "{code}"
    );
    let builtin = render_with(
        "<template><Suspense/></template>",
        true,
        &[("Suspense", BindingType::SetupConst)],
    );
    assert!(
        builtin.contains("_ssrRenderSuspense(_push, {})"),
        "{builtin}"
    );
    let recursive_slot = render("<template><Child><App/></Child></template>", true);
    assert_eq!(
        recursive_slot
            .matches("const _component_App = _resolveComponent(\"App\", true)")
            .count(),
        1,
        "{recursive_slot}"
    );
}

#[test]
fn a_comment_only_template_pushes_its_comment() {
    let code = render(fixture!("server_comment_only_template"), true);
    assert!(code.contains("_push(`<!-- a b c -->`)"), "{code}");
    assert!(
        code.starts_with("function ssrRender(_ctx, _push, _parent, _attrs) {"),
        "{code}"
    );
    // A lone comment root renders without fragment markers.
    assert!(!code.contains("<!--[-->"), "{code}");
}

#[test]
fn backticks_in_a_comment_only_template_are_escaped() {
    let code = render(fixture!("server_comment_only_template_backticks"), true);
    assert!(code.contains("_push(`<!-- a \\`b\\` c -->`)"), "{code}");
}

#[test]
fn backticks_in_a_nested_comment_are_escaped_inside_the_element_literal() {
    let code = render(fixture!("server_nested_comment_backticks"), true);
    assert!(
        code.contains("_push(`<div${_ssrRenderAttrs(_attrs)}><!-- a \\`b\\` c --></div>`)"),
        "{code}"
    );
}

#[test]
fn a_template_of_dropped_comments_renders_an_empty_function() {
    let code = render(fixture!("server_comment_only_template_backticks"), false);
    assert!(!code.contains("a \\`b\\` c"), "{code}");
    assert_eq!(
        code.trim_end(),
        "function ssrRender(_ctx, _push, _parent, _attrs) {\n}"
    );
}

#[test]
fn an_interpolation_reads_its_setup_binding_through_the_render_parameters() {
    let code = render_with(
        "<template><p>{{ n }}</p></template>",
        true,
        &[("n", BindingType::SetupRef)],
    );
    assert!(code.contains("${_ssrInterpolate($setup.n)}"), "{code}");
    assert!(
        code.starts_with(
            "function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {"
        ),
        "{code}"
    );
}

#[test]
fn text_is_condensed_decoded_and_reencoded() {
    let code = render("<template><p>a   &amp;\n b &lt;c&gt;</p></template>", true);
    assert!(code.contains(">a &amp; b &lt;c&gt;</p>"), "{code}");
}

#[test]
fn several_element_roots_are_wrapped_in_fragment_markers() {
    let code = render("<template><div>a</div><span>b</span></template>", true);
    assert!(
        code.contains("_push(`<!--[--><div>a</div><span>b</span><!--]-->`)"),
        "{code}"
    );
}

#[test]
fn a_scoped_root_merges_its_class_with_the_fallthrough_attrs_and_carries_one_scope_attribute() {
    let code = render_full(
        fixture!("server_scoped_style_root"),
        true,
        &[("n", BindingType::SetupRef)],
        true,
    );
    assert!(
        code.contains(r#"_ssrRenderAttrs(_mergeProps({ class: "a" }, _attrs))"#),
        "{code}"
    );
    let scope = ScopeId::for_component("App.vue").attribute();
    assert_eq!(code.matches(scope.as_str()).count(), 1, "{code}");
}

#[test]
fn dynamic_class_and_boolean_attributes_render_through_their_helpers() {
    let code = render(
        r#"<template><div><p :class="{ on: ok }">x</p><button :disabled="d">y</button></div></template>"#,
        true,
    );
    assert!(code.contains("_ssrRenderClass({ on: _ctx.ok })"), "{code}");
    assert!(
        code.contains(r#"_ssrIncludeBooleanAttr(_ctx.d)) ? " disabled" : """#),
        "{code}"
    );
}

#[test]
fn a_chain_with_an_else_renders_both_branches_and_a_lone_branch_renders_an_empty_comment() {
    let chained = render(
        r#"<template><div><p v-if="a">x</p><p v-else>y</p></div></template>"#,
        true,
    );
    assert!(chained.contains("if (_ctx.a) {"), "{chained}");
    assert!(chained.contains("} else {"), "{chained}");
    assert!(!chained.contains("<!---->"), "{chained}");

    let lone = render(r#"<template><div><p v-if="a">x</p></div></template>"#, true);
    assert!(lone.contains("} else {\n_push(`<!---->`)\n}"), "{lone}");
}

#[test]
fn a_loop_renders_its_list_inside_fragment_markers() {
    let code = render(
        r#"<template><ul><li v-for="i in items">{{ i }}</li></ul></template>"#,
        true,
    );
    assert!(code.contains("<!--[-->"), "{code}");
    assert!(
        code.contains("_ssrRenderList(_ctx.items, (i) => {"),
        "{code}"
    );
    assert!(code.contains("_ssrInterpolate(i)"), "{code}");
    assert!(code.contains("<!--]-->"), "{code}");
}

#[test]
fn v_html_renders_its_value_as_the_content() {
    let code = render(
        r#"<template><div><p v-html="h"></p></div></template>"#,
        true,
    );
    assert!(code.contains("<p>${(_ctx.h) ?? ''}</p>"), "{code}");
}

/// Every piece occurs in `code`, each after the previous one.
fn assert_in_order(code: &str, pieces: &[&str]) {
    let mut at = 0;
    for piece in pieces {
        match code[at..].find(piece) {
            Some(found) => at += found + piece.len(),
            None => panic!("{piece:?} not found after byte {at} in:\n{code}"),
        }
    }
}

const CHILD: (&str, BindingType) = ("Child", BindingType::SetupImport);

#[test]
fn a_slot_handler_with_two_statements_keeps_both_in_the_client_branch() {
    let code = render_with(
        fixture!("server_slot_handler_two_statements"),
        true,
        &[
            CHILD,
            ("a", BindingType::SetupRef),
            ("b", BindingType::SetupRef),
        ],
    );
    let split = code
        .find("} else {")
        .expect("slot function has a client branch");
    let (server, client) = code.split_at(split);
    assert!(server.contains("<button"), "{code}");
    assert!(!server.contains("onClick"), "{code}");
    // Both statements, in source order, in one handler of the client nodes.
    assert_in_order(
        client,
        &[
            "_createVNode(\"button\", {",
            "onClick: $event => {",
            "a = ''; ",
            "b = ''}",
            "[\"onClick\"]",
        ],
    );
}

#[test]
fn a_slot_handler_with_one_statement_stays_an_expression() {
    let code = render_with(
        fixture!("server_slot_handler_one_statement"),
        true,
        &[
            CHILD,
            ("a", BindingType::SetupRef),
            ("b", BindingType::SetupRef),
        ],
    );
    let split = code
        .find("} else {")
        .expect("slot function has a client branch");
    let (server, client) = code.split_at(split);
    assert!(!server.contains("onClick"), "{code}");
    assert_in_order(
        client,
        &[
            "_createVNode(\"button\", {",
            "onClick: $event => (",
            "a = '')",
        ],
    );
}

#[test]
fn a_handler_outside_slot_content_has_no_client_branch() {
    let code = render_with(
        fixture!("server_handler_two_statements"),
        true,
        &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
    );
    assert!(!code.contains("_createVNode"), "{code}");
    assert!(!code.contains("onClick"), "{code}");
}

#[test]
fn a_conditional_named_slot_reading_slots_renders_through_create_slots() {
    let code = render_with(
        fixture!("server_conditional_slot_from_slots"),
        true,
        &[CHILD],
    );
    assert_in_order(
        &code,
        &[
            "_ssrRenderComponent(",
            "_createSlots({",
            "_: 2 /* DYNAMIC */",
            "}, [",
            "(_ctx.$slots.actions)",
            "? {",
            "name: \"actions\"",
            "fn: _withCtx((_, _push, _parent, _scopeId) => {",
            "if (_push) {",
            "_ssrRenderSlot(_ctx.$slots, \"actions\", {}, null, _push, _parent, _scopeId)",
            "} else {",
            "_renderSlot(_ctx.$slots, \"actions\")",
            "key: \"0\"",
            ": undefined",
        ],
    );
}

#[test]
fn a_conditional_named_slot_reading_a_binding_renders_through_create_slots() {
    let code = render_with(
        fixture!("server_conditional_slot_from_binding"),
        true,
        &[CHILD, ("show", BindingType::LiteralConst)],
    );
    assert_in_order(
        &code,
        &[
            "_createSlots({",
            "_: 2 /* DYNAMIC */",
            "}, [",
            "($setup.show)",
            "name: \"actions\"",
            "if (_push) {",
            "x",
            "} else {",
            "key: \"0\"",
            ": undefined",
        ],
    );
}

#[test]
fn a_scoped_slot_with_branches_renders_statements_on_the_server_and_a_ternary_on_the_client() {
    let code = render_with(fixture!("server_scoped_slot_if_else"), true, &[CHILD]);
    assert_in_order(
        &code,
        &[
            "({ row }, _push, _parent, _scopeId) => {",
            "if (_push) {",
            "if (row.a) {",
            "} else {",
            "} else {",
            "(row.a)",
            "key: 0",
            "key: 1",
        ],
    );
}

#[test]
fn a_default_slot_written_before_a_named_slot_moves_after_it() {
    let code = render_with(
        "<template><Child>x<template #named>y</template></Child></template>",
        true,
        &[CHILD],
    );
    assert_in_order(
        &code,
        &[
            "named: _withCtx(",
            "default: _withCtx(",
            "_: 1 /* STABLE */",
        ],
    );
}

#[test]
fn a_conditional_slot_of_a_dynamic_component_renders_through_create_slots() {
    let code = render(
        r#"<template><component :is="c"><template #actions v-if="show">x</template></component></template>"#,
        true,
    );
    assert_in_order(
        &code,
        &[
            "_createVNode(",
            "_createSlots({",
            "}, [",
            "(_ctx.show)",
            "name: \"actions\"",
            "])",
        ],
    );
    assert!(!code.contains("if (_ctx.show)"), "{code}");
}

#[test]
fn a_comment_between_named_slots_renders_nowhere() {
    let code = render_with(
        "<template><Child><!--c--><template #a>x</template></Child></template>",
        true,
        &[CHILD],
    );
    assert!(!code.contains("<!--c-->"), "{code}");
    assert_in_order(&code, &["_ssrRenderComponent(", "{\na: _withCtx("]);
}

#[test]
fn component_handlers_take_their_shape_from_the_parse() {
    let statements = render_with(
        r#"<template><Child @click="a = 1; b = 2"/></template>"#,
        true,
        &[CHILD],
    );
    assert!(statements.contains("onClick: $event => {"), "{statements}");

    let call = render(
        r#"<template><component :is="c" @click="save()"/></template>"#,
        true,
    );
    assert!(call.contains("onClick: $event => (_ctx.save())"), "{call}");
}

#[test]
fn a_slot_outlet_outside_slot_functions_names_no_scope_parameter() {
    let code = render(
        r##"<template><Teleport to="#x"><slot/></Teleport></template>"##,
        true,
    );
    assert!(code.contains("_ssrRenderSlot("), "{code}");
    assert!(!code.contains("_scopeId"), "{code}");
}

#[test]
fn a_shorthand_binding_reads_the_loop_variable() {
    let code = render_with(
        r#"<template><div><Child v-for="item in items" :item/></div></template>"#,
        true,
        &[CHILD],
    );
    assert!(code.contains("item: item"), "{code}");
    assert!(!code.contains("item: _ctx.item"), "{code}");
}

#[test]
fn a_capitalized_component_tag_with_is_resolves_dynamically() {
    let code = render(r#"<template><Component :is="choice"/></template>"#, true);
    assert!(
        code.contains("_resolveDynamicComponent(_ctx.choice)"),
        "{code}"
    );
}
