use super::test_support::{
    assert_javascript, assert_legacy, assert_legacy_with, legacy, legacy_code, render, with_inputs,
};
use super::{VaporCompiler, VueVaporExecutor};
use crate::assembly::emit_plan::MapMode;
use crate::target_plan::TargetExecutor;
use crate::types::BindingType;
use crate::work_ledger::CompilerWorkLedger;
use verter_audit::observe::Uncaptured;

fn setter(source: &str, expected: &str, helper: &'static str) {
    let output = assert_legacy(source);
    assert!(output.code.contains(expected), "{}", output.code);
    assert!(output.helpers.contains(&helper), "{:?}", output.helpers);
}

#[test]
fn dynamic_class_creates_set_class_effect() {
    setter(
        "<template><div :class=\"cls\"/></template>",
        "_renderEffect(() => _setClass(n0, _ctx.cls))",
        "_setClass",
    );
}
#[test]
fn bind_shorthand_no_value_resolves_to_binding() {
    setter(
        "<template><div :id/></template>",
        "_setProp(n0, \"id\", _ctx.id)",
        "_setProp",
    );
}
#[test]
fn bind_explicit_blank_value_does_not_alias_to_shorthand() {
    for value in ["", "  "] {
        let source = format!("<template><div :id=\"{value}\"/></template>");
        setter(&source, "_setProp(n0, \"id\", \"\")", "_setProp");
    }
}
#[test]
fn dynamic_style_creates_set_style_effect() {
    setter(
        "<template><div :style=\"sty\"/></template>",
        "_setStyle(n0, _ctx.sty)",
        "_setStyle",
    );
}
#[test]
fn dynamic_prop_creates_set_prop_effect() {
    setter(
        "<template><div :id=\"id\"/></template>",
        "_setProp(n0, \"id\", _ctx.id)",
        "_setProp",
    );
}
#[test]
fn static_only_element_has_no_effects() {
    let output = assert_legacy("<template><div id=\"app\"/></template>");
    assert!(!output.helpers.contains(&"_renderEffect"));
    assert!(!output.code.contains("_setProp("));
}
#[test]
fn static_class_only_has_no_effects() {
    let output = assert_legacy("<template><div class=\"static\"/></template>");
    assert!(!output.helpers.contains(&"_setClass"));
    assert!(!output.helpers.contains(&"_renderEffect"));
}
#[test]
fn multiple_dynamic_props_keep_a_braced_effect_group() {
    let output = assert_legacy("<template><div :class=\"cls\" :style=\"sty\"/></template>");
    assert!(
        output.code.contains(
            "_renderEffect(() => {\n    _setClass(n0, _ctx.cls)\n    _setStyle(n0, _ctx.sty)\n  })"
        ),
        "{}",
        output.code
    );
}
#[test]
fn prop_modifier_forces_set_dom_prop_even_when_hyphenated() {
    setter(
        "<template><div :text-content.prop=\"text\"/></template>",
        "_setDOMProp(n0, \"text-content\", _ctx.text)",
        "_setDOMProp",
    );
}
#[test]
fn attr_modifier_forces_set_attr_even_when_not_hyphenated() {
    setter(
        "<template><div :id.attr=\"id\"/></template>",
        "_setAttr(n0, \"id\", _ctx.id)",
        "_setAttr",
    );
}
#[test]
fn hyphenated_attr_without_prop_modifier_stays_set_attr() {
    for name in ["data-id", "aria-label", "custom-attr"] {
        let source = format!("<template><div :{name}=\"value\"/></template>");
        setter(
            &source,
            &format!("_setAttr(n0, \"{name}\", _ctx.value)"),
            "_setAttr",
        );
    }
}
#[test]
fn html_setter_resolves_a_reported_setup_ref() {
    let output = assert_legacy_with(
        "<template><div v-html=\"rawHtml\"/></template>",
        &[("rawHtml", BindingType::SetupRef)],
    );
    assert!(
        output.code.contains("_setHtml(n0, _ctx.rawHtml)"),
        "{}",
        output.code
    );
}
#[test]
fn html_setter_resolves_an_unreported_name_through_context() {
    setter(
        "<template><div v-html=\"rawHtml\"/></template>",
        "_setHtml(n0, _ctx.rawHtml)",
        "_setHtml",
    );
}
#[test]
fn object_spread_resolves_a_reported_setup_ref() {
    let output = assert_legacy_with(
        "<template><div v-bind=\"attrs\"/></template>",
        &[("attrs", BindingType::SetupRef)],
    );
    assert!(
        output.code.contains("_setDynamicProps(n0, [_ctx.attrs])"),
        "{}",
        output.code
    );
}
#[test]
fn dynamic_class_reads_a_reported_ref_through_context() {
    let output = assert_legacy_with(
        "<template><div :class=\"cls\"/></template>",
        &[("cls", BindingType::SetupRef)],
    );
    assert!(
        output.code.contains("_setClass(n0, _ctx.cls)"),
        "{}",
        output.code
    );
}
#[test]
fn dynamic_style_reads_a_reported_prop_through_props() {
    let output = assert_legacy_with(
        "<template><div :style=\"sty\"/></template>",
        &[("sty", BindingType::Props)],
    );
    assert!(
        output.code.contains("_setStyle(n0, $props.sty)"),
        "{}",
        output.code
    );
}
#[test]
fn dynamic_prop_reads_a_setup_constant_through_context() {
    let output = assert_legacy_with(
        "<template><div :id=\"id\"/></template>",
        &[("id", BindingType::SetupConst)],
    );
    assert!(
        output.code.contains("_setProp(n0, \"id\", _ctx.id)"),
        "{}",
        output.code
    );
}
#[test]
fn dynamic_attribute_reads_a_reported_ref_through_context() {
    let output = assert_legacy_with(
        "<template><div :data-value=\"value\"/></template>",
        &[("value", BindingType::SetupRef)],
    );
    assert!(
        output
            .code
            .contains("_setAttr(n0, \"data-value\", _ctx.value)"),
        "{}",
        output.code
    );
}

#[test]
fn camelized_shorthand_has_generated_identifier_provenance() {
    let source = "<template><div :foo-bar/></template>";
    with_inputs(
        source,
        MapMode::Map,
        false,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, ledger| {
            let output = VueVaporExecutor::execute(inputs, ledger).unwrap();
            let mut code = output
                .preamble
                .unwrap()
                .write(ledger)
                .unwrap()
                .text()
                .to_owned();
            let rendered = output.render.write(ledger).unwrap();
            assert!(
                rendered
                    .text()
                    .contains("_setAttr(n0, \"foo-bar\", _ctx.fooBar)"),
                "{}",
                rendered.text()
            );
            let generated = rendered.text().find("fooBar").unwrap() as u32;
            assert_eq!(rendered.map().unwrap().lookup(generated), None);
            code.push_str(rendered.text());
            assert_javascript(&code);
            assert_eq!(code, legacy(source));
        },
    );
}

#[test]
fn typed_prop_values_use_the_shared_removal_ranges() {
    setter(
        "<template><div :title=\"(value as string)\"/></template>",
        "_setProp(n0, \"title\", (_ctx.value))",
        "_setProp",
    );
}

#[test]
fn typed_dynamic_keys_use_rebased_shared_removal_ranges() {
    let source = "<template><div :[key!]=\"value\"/></template>";
    let corrected = render(source);
    assert!(
        corrected
            .code
            .contains("_setDynamicProps(n0, [{ [(_ctx.key) || \"\"]: _ctx.value }])"),
        "{}",
        corrected.code
    );
    let previous = legacy_code(source);
    assert!(
        previous.contains("_setDynamicProps(n0, [{ [(key!) || \"\"]: _ctx.value }])"),
        "{previous}"
    );
    let allocator = oxc_allocator::Allocator::default();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&allocator, &previous, oxc_span::SourceType::mjs())
            .parse();
    assert!(
        !parsed.diagnostics.is_empty(),
        "type-bearing keys must be corrected before JavaScript emission"
    );
}

#[test]
fn identifier_dynamic_keys_fall_back_to_an_empty_name() {
    setter(
        "<template><div :[name]=\"value\"/></template>",
        "_setDynamicProps(n0, [{ [_ctx.name || \"\"]: _ctx.value }])",
        "_setDynamicProps",
    );
}

#[test]
fn simple_dynamic_key_resolution_keeps_authored_compound_expressions() {
    setter(
        "<template><div :[prefix+suffix]=\"value\"/></template>",
        "_setDynamicProps(n0, [{ [(prefix+suffix) || \"\"]: _ctx.value }])",
        "_setDynamicProps",
    );
}

#[test]
fn statements_precede_the_reactive_effect_group() {
    let output = assert_legacy("<template><div :class=\"cls\" v-show=\"visible\"/></template>");
    let statement = output
        .code
        .find("_applyVShow(n0, () => (_ctx.visible))")
        .unwrap();
    let effect = output.code.find("_renderEffect(").unwrap();
    assert!(statement < effect, "{}", output.code);
}

#[test]
fn own_effects_precede_child_effects_in_graph_order() {
    use super::effect::VaporEffect;
    use super::ops::Part;
    use crate::framework::vue::structure::walk_template;

    let source = "<template><div :title=\"outer\"><p :class=\"inner\"/></div></template>";
    with_inputs(
        source,
        MapMode::NoMap,
        false,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, ledger| {
            let mut compiler = VaporCompiler::new(inputs);
            walk_template(inputs.topology, inputs.structure, &mut compiler);
            let group = compiler
                .state
                .runs
                .get(compiler.state.roots[0].run)
                .iter()
                .find_map(|part| match part {
                    Part::Effects(group) => Some(*group),
                    _ => None,
                })
                .unwrap();
            let graph = compiler.state.effect_graph.relations().unwrap();
            assert_eq!((graph.node_count(), graph.edge_count()), (5, 4));
            let edges = graph.edges(group);
            let own = super::graph::effect_row(graph, edges[0].0).unwrap();
            let child = super::graph::effect_row(graph, edges[1].0).unwrap();
            assert!(matches!(
                &compiler.state.effects[own.0 as usize],
                VaporEffect::Prop { .. }
            ));
            assert!(matches!(
                &compiler.state.effects[child.0 as usize],
                VaporEffect::Class { .. }
            ));
            let code = compiler
                .finish()
                .unwrap()
                .render
                .write(ledger)
                .unwrap()
                .text()
                .to_owned();
            assert_javascript(&code);
            assert!(
                code.find("_setProp(").unwrap() < code.find("_setClass(").unwrap(),
                "{code}"
            );
        },
    );
    // The child local the effect writes is the one its navigation declares,
    // as in Vue's output.
    assert_eq!(
        render(source).code,
        "const t0 = _template(\"<div><p>\", 1)\n\
         function render(_ctx, $props, $emit, $attrs, $slots) {\n  \
         const n1 = t0()\n  \
         const n0 = _child(n1)\n  \
         _renderEffect(() => {\n    \
         _setProp(n1, \"title\", _ctx.outer)\n    \
         _setClass(n0, _ctx.inner)\n  \
         })\n  \
         return n1\n\
         }"
    );
}

#[test]
fn constant_props_are_statements_outside_the_effect_graph() {
    use crate::framework::vue::semantic::VueBindingView;
    use crate::framework::vue::structure::walk_template;

    let source = "<template><div :title=\"title\">{{ message }}</div></template>";
    with_inputs(
        source,
        MapMode::Map,
        false,
        &[("title", BindingType::Props)],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, ledger| {
            let mut compiler = VaporCompiler::new(inputs);
            compiler.resolver = VueBindingView::new(
                inputs.semantic.bindings(),
                false,
                true,
                Some(["title"].into_iter().collect()),
                Default::default(),
            )
            .with_template_scopes(&inputs.semantic.expressions().scopes);
            walk_template(inputs.topology, inputs.structure, &mut compiler);
            let graph = compiler.state.effect_graph.relations().unwrap();
            assert_eq!((graph.node_count(), graph.edge_count()), (3, 2));
            let output = compiler.finish().unwrap();
            let code = output.render.write(ledger).unwrap().text().to_owned();
            assert_javascript(&code);
            let statement = code.find("_setProp(n0, \"title\", $props.title)").unwrap();
            let effect = code.find("_renderEffect(() => _setText(").unwrap();
            assert!(statement < effect, "{code}");
        },
    );
}

#[test]
fn unsupported_directives_keep_reference_allocation_order() {
    let output =
        render("<template><div><p v-unknown=\"value\"/><span>{{ text }}</span></div></template>");
    assert!(
        output.code.contains("const n0 = _child(n2)"),
        "{}",
        output.code
    );
    assert!(
        output.code.contains("const x1 = _txt(p0)"),
        "{}",
        output.code
    );
    // The span is the paragraph's next sibling.
    assert!(
        output.code.contains("const p0 = _next(n0)"),
        "{}",
        output.code
    );
}
