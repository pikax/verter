use verter_audit::observe::Uncaptured;

use super::{test_support::with_inputs, VueVaporExecutor};
use crate::assembly::emit_plan::MapMode;
use crate::target_plan::TargetExecutor;
use crate::work_ledger::CompilerWorkLedger;

#[test]
fn a_plain_root_is_instantiated_and_returned() {
    with_inputs(
        "<template><div>hello</div></template>",
        MapMode::NoMap,
        false,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, ledger| {
            let output =
                VueVaporExecutor::execute(inputs, ledger).expect("fine-grained target emits");
            assert_eq!(
                output.preamble.unwrap().write(ledger).unwrap().text(),
                "const t0 = _template(\"<div>hello\", 3)\n"
            );
            let render = output.render.write(ledger).unwrap();
            super::test_support::assert_javascript(render.text());
            assert_eq!(
                render.text(),
                "function render(_ctx, $props, $emit, $attrs, $slots) {\n  const n0 = t0()\n  return n0\n}"
            );
        },
    );
}

fn assert_render_contains(source: &str, expected: &str) {
    with_inputs(
        source,
        MapMode::NoMap,
        false,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, ledger| {
            let output =
                VueVaporExecutor::execute(inputs, ledger).expect("fine-grained target emits");
            let code = output.render.write(ledger).unwrap();
            super::test_support::assert_javascript(code.text());
            assert!(code.text().contains(expected), "{}", code.text());
        },
    );
}

#[test]
fn a_bound_class_records_a_render_effect() {
    assert_render_contains(
        "<template><div :class=\"cls\"/></template>",
        "_renderEffect(() => _setClass(n0, _ctx.cls))",
    );
}

#[test]
fn events_and_models_record_one_time_operations() {
    assert_render_contains(
        "<template><input @focus=\"focus\" v-model=\"text\"/></template>",
        "_applyTextModel(",
    );
}

#[test]
fn conditionals_and_loops_record_nested_closures() {
    assert_render_contains(
        "<template><p v-if=\"ok\"/><p v-for=\"item in items\">{{ item }}</p></template>",
        "_createFor(",
    );
}

#[test]
fn components_and_slots_record_nested_closures() {
    assert_render_contains(
        "<template><Child><slot/></Child></template>",
        "_createComponent(",
    );
}

#[test]
fn repeated_reads_are_hoisted_before_the_effects() {
    assert_render_contains(
        "<template><div :class=\"value\" :title=\"value\"/></template>",
        "const _value = _ctx.value",
    );
}

#[test]
fn static_markup_preserves_template_payloads() {
    for (source, expected) in [
        ("<template><div/></template>", "<div>"),
        (
            "<template><div id=\"app\" class=\"hello world\">hello</div></template>",
            "<div id=app class=\\\"hello world\\\">hello",
        ),
        (
            "<template><div><span>inner</span></div></template>",
            "<div><span>inner",
        ),
        ("<template><br/></template>", "<br>"),
    ] {
        let output = super::test_support::assert_legacy(source);
        assert!(output.code.contains(expected), "{}", output.code);
    }
}

#[test]
fn an_empty_non_void_element_keeps_its_end_tag_before_later_siblings() {
    // Vue 3.6.0-rc.15 writes `<span></span>tail`: without the end tag the
    // static template makes the text a child of the empty `<span>`.
    for (source, expected) in [
        (
            "<template><div><!-- hello --><span/>tail</div></template>",
            "_template(\"<div><!-- hello --><span></span>tail\", 3)",
        ),
        (
            "<template><div><textarea rows=6></textarea><div class=\"x\">{{ b }}</div></div></template>",
            "_template(\"<div><textarea rows=6></textarea><div class=x> \", 1)",
        ),
        (
            "<template><div><input v-model=\"a\"><span/><b>{{ x }}</b></div></template>",
            "_template(\"<div><input><span></span><b> \", 1)",
        ),
    ] {
        let output = super::test_support::render(source);
        assert!(output.code.contains(expected), "{expected}\n{}", output.code);
    }
}

#[test]
fn text_after_the_last_element_child_keeps_its_node() {
    // Vue 3.6.0-rc.15: `<div><p>x</p> ` keeps the text node the effect sets,
    // reached as the paragraph's next sibling.
    let output = super::test_support::render("<template><div><p>x</p> {{ a }}</div></template>");
    for expected in [
        "_template(\"<div><p>x</p> \", 1)",
        "= _next(_child(n1), true)",
        "_setText(n0, \" \" + _toDisplayString(_ctx.a))",
    ] {
        assert!(
            output.code.contains(expected),
            "{expected}\n{}",
            output.code
        );
    }
}

#[test]
fn dynamic_text_uses_one_placeholder_and_preserves_static_parts() {
    let output = super::test_support::assert_legacy(
        "<template><div>hello {{ msg }} world {{ suffix }}</div></template>",
    );
    assert!(
        output.code.contains("_template(\"<div> \", 1)"),
        "{}",
        output.code
    );
    assert!(output.code.contains("_setText(x0, \"hello \" + _toDisplayString(_ctx.msg) + \" world \" + _toDisplayString(_ctx.suffix))"), "{}", output.code);
}

#[test]
fn nested_dynamic_text_keeps_navigation_and_allocation_order() {
    // The second child is the first one's next sibling (`next(node)` is
    // `node.nextSibling` in runtime-vapor 3.6.0-rc.15).
    let output = super::test_support::render(
        "<template><div><span>{{ a }}</span><p>{{ b }}</p></div></template>",
    );
    for expected in [
        "const p0 = _child(n1)",
        "const p1 = _next(p0)",
        "const x0 = _txt(p0)",
        "const x2 = _txt(p1)",
        "_setText(x0, _toDisplayString(_ctx.a))",
        "_setText(x2, _toDisplayString(_ctx.b))",
    ] {
        assert!(
            output.code.contains(expected),
            "{expected}\n{}",
            output.code
        );
    }
}

#[test]
fn a_text_directive_records_the_same_text_effect_as_interpolation() {
    let output = super::test_support::assert_legacy("<template><p v-text=\"msg\"/></template>");
    assert!(output
        .code
        .contains("_setText(x0, _toDisplayString(_ctx.msg))"));
}

#[test]
fn multiple_roots_share_an_array_return_and_omit_the_root_flag() {
    let output = super::test_support::assert_legacy("<template><p>one</p><p>two</p></template>");
    assert!(output.code.contains("return [n0, n1]"));
    assert!(output.code.contains("_template(\"<p>one\", 2)"));
    assert!(output.code.contains("_template(\"<p>two\", 2)"));
}

#[test]
fn force_javascript_removes_types_from_every_interpolation() {
    for (expression, previous, corrected) in [
        ("(1 as number)", "(1 as number)", "(1)"),
        (
            "(value as string)",
            "(_ctx.value as string)",
            "(_ctx.value)",
        ),
    ] {
        let source = format!("<template><p>{{{{ {expression} }}}}</p></template>");
        let output = super::test_support::render(&source);
        assert!(
            output
                .code
                .contains(&format!("_toDisplayString({corrected})")),
            "{}",
            output.code
        );
        let old = super::test_support::legacy_code(&source);
        assert!(
            old.contains(&format!("_toDisplayString({previous})")),
            "{old}"
        );
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            verter_parser::oxc_parse::Parser::new(&allocator, &old, oxc_span::SourceType::mjs())
                .parse();
        assert!(
            !parsed.diagnostics.is_empty(),
            "legacy interpolation retains forbidden JavaScript type syntax"
        );
    }
}

#[test]
fn an_authored_identifier_is_an_exact_piece() {
    let source = "<template><p>{{ msg }}</p></template>";
    with_inputs(
        source,
        MapMode::Map,
        false,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, ledger| {
            let output = VueVaporExecutor::execute(inputs, ledger).unwrap();
            let rendered = output.render.write(ledger).unwrap();
            super::test_support::assert_javascript(rendered.text());
            let generated = rendered.text().find("msg").unwrap() as u32;
            let authored = source.find("msg").unwrap() as u32;
            let map = rendered.map().unwrap();
            assert_eq!(map.lookup(generated), Some((0, authored)));
            assert_eq!(map.lookup(generated + 2), Some((0, authored + 2)));
            assert!(map
                .segments()
                .iter()
                .any(|segment| segment.generated.contains(&generated)
                    && segment.kind == crate::emission::MapSegmentKind::Identity));
        },
    );
}

#[test]
fn a_text_only_run_records_no_anchor() {
    crate::emission::reset_map_capture_events();
    with_inputs(
        "<template><p>{{ msg }}</p></template>",
        MapMode::NoMap,
        false,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, ledger| {
            let output = VueVaporExecutor::execute(inputs, ledger).unwrap();
            let rendered = output.render.write(ledger).unwrap();
            super::test_support::assert_javascript(rendered.text());
            assert!(rendered.map().is_none());
            assert!(rendered.text().contains("_ctx.msg"));
            assert_eq!(crate::emission::map_capture_events(), 0);
        },
    );
}

#[test]
fn typed_loop_sources_resolve_reads_and_keep_authored_anchors() {
    let source = "<template><p v-for=\"row in (rows! as number[]).slice(0, limit)\">{{ row }}</p></template>";
    with_inputs(
        source,
        MapMode::Map,
        false,
        &[
            ("rows", crate::types::BindingType::SetupRef),
            ("limit", crate::types::BindingType::Props),
        ],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, ledger| {
            let output = VueVaporExecutor::execute(inputs, ledger).unwrap();
            let rendered = output.render.write(ledger).unwrap();
            super::test_support::assert_javascript(rendered.text());
            assert!(
                rendered
                    .text()
                    .contains("(_ctx.rows).slice(0, $props.limit)"),
                "{}",
                rendered.text()
            );
            let map = rendered.map().unwrap();
            for name in ["rows", "limit"] {
                let generated = rendered.text().find(name).unwrap() as u32;
                let authored = source.find(name).unwrap() as u32;
                assert_eq!(map.lookup(generated), Some((0, authored)));
                assert_eq!(
                    map.lookup(generated + name.len() as u32 - 1),
                    Some((0, authored + name.len() as u32 - 1))
                );
            }
        },
    );
}

#[test]
fn an_empty_template_returns_null_without_runtime_helpers() {
    let output = super::test_support::assert_legacy("<template></template>");
    assert_eq!(
        output.code,
        "function render(_ctx, $props, $emit, $attrs, $slots) {\n  return null\n}"
    );
    assert!(output.helpers.is_empty());
}

#[test]
fn inline_fine_grained_rendering_is_refused_before_the_walk() {
    with_inputs(
        "<template><p>{{ value }}</p></template>",
        MapMode::NoMap,
        true,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, ledger| {
            assert!(matches!(
                VueVaporExecutor::execute(inputs, ledger),
                Err(
                    crate::framework::vue::target::VueTargetRefusal::Unsupported {
                        code: "inline-fine-grained-render",
                        ..
                    }
                )
            ));
        },
    );
}

#[test]
fn function_scoped_source_vars_stay_local_across_blocks() {
    let source = "<template><p v-for=\"x in (() => { { var values = [1] } return values })()\">{{ x }}</p></template>";
    let output = super::test_support::render(source);
    assert!(output.code.contains("return values"), "{}", output.code);
}
