//! Static templates and node operations reach the DOM Vue 3.6.0-rc.15 builds.
//!
//! Each expectation is the official `@vue/compiler-vapor` 3.6.0-rc.15 output
//! for the same template, compiled with `mode: "module"` and
//! `prefixIdentifiers: true`.
use super::test_support::render;

fn assert_contains_all(source: &str, expected: &[&str]) {
    let output = render(source);
    for expected in expected {
        assert!(
            output.code.contains(expected),
            "{expected}\n{}",
            output.code
        );
    }
}

#[test]
fn an_svg_element_writes_its_dynamic_attributes_as_svg_attributes() {
    // `width`, `viewBox` and `cx` are read-only `SVGAnimated*` properties:
    // a property write would be lost.
    assert_contains_all(
        "<template><svg :width=\"w\" :viewBox=\"vb\"><circle :cx=\"x\" :r=\"r\"/></svg></template>",
        &[
            "_template(\"<svg><circle>\", 1, 1)",
            "const n0 = _child(n1)",
            "_setAttr(n1, \"width\", _ctx.w, true)",
            "_setAttr(n1, \"viewBox\", _ctx.vb, true)",
            "_setAttr(n0, \"cx\", _ctx.x, true)",
            "_setAttr(n0, \"r\", _ctx.r, true)",
        ],
    );
    let output = render(
        "<template><svg :width=\"w\" :viewBox=\"vb\"><circle :cx=\"x\" :r=\"r\"/></svg></template>",
    );
    assert!(!output.code.contains("_setProp("), "{}", output.code);
}

#[test]
fn an_svg_class_is_flagged_as_svg_and_its_style_is_not() {
    assert_contains_all(
        "<template><svg :class=\"c\" :style=\"s\" :id=\"i\"><text :x=\"x\">{{ t }}</text></svg></template>",
        &[
            "_template(\"<svg><text> \", 1, 1)",
            "_setClass(n1, _ctx.c, true)",
            "_setStyle(n1, _ctx.s)",
            "_setAttr(n1, \"id\", _ctx.i, true)",
            "_setAttr(n0, \"x\", _ctx.x, true)",
        ],
    );
}

#[test]
fn a_template_rooted_inside_svg_is_parsed_in_the_svg_namespace() {
    assert_contains_all(
        "<template><svg><circle v-if=\"a\"/></svg></template>",
        &[
            "const t0 = _template(\"<circle>\", 2, 1)",
            "const t1 = _template(\"<svg>\", 1, 1)",
        ],
    );
}

#[test]
fn an_html_template_carries_no_namespace() {
    assert_contains_all(
        "<template><div><p v-if=\"a\">x</p></div></template>",
        &["_template(\"<p>x\", 2)\n", "_template(\"<div>\", 1)\n"],
    );
}

#[test]
fn a_slot_body_with_several_roots_gives_each_root_its_own_template() {
    // `template()` keeps only its payload's first child, so one payload for
    // both roots would drop the span.
    let output = render("<template><Comp><p>1</p><span>{{ x }}</span></Comp></template>");
    for expected in [
        "_template(\"<p>1\", 2)",
        "_template(\"<span> \")",
        "const x1 = _txt(n1)",
        "_setText(x1, _toDisplayString(_ctx.x))",
        "return [n0, n1]",
    ] {
        assert!(
            output.code.contains(expected),
            "{expected}\n{}",
            output.code
        );
    }
    assert!(
        !output.code.contains("<p>1</p><span>"),
        "both roots share one template:\n{}",
        output.code
    );
}

#[test]
fn a_named_slot_with_several_roots_returns_each_root() {
    let output = render(
        "<template><Comp><template #a>t<b/></template><template #c><i/><Other/></template></Comp></template>",
    );
    for expected in [
        "_template(\"t\", 2)",
        "_template(\"<b>\", 2)",
        "_template(\"<i>\", 2)",
        "return [n0, n1]",
    ] {
        assert!(
            output.code.contains(expected),
            "{expected}\n{}",
            output.code
        );
    }
    assert!(!output.code.contains("\"t<b>"), "{}", output.code);
}

#[test]
fn every_text_run_beside_elements_is_its_own_node() {
    assert_contains_all(
        "<template><div>a {{ x }}<p/>b {{ y }}</div></template>",
        &[
            "_template(\"<div> <p></p> \", 1)",
            "const n0 = _child(n2, true)",
            "const n1 = _nthChild(n2, 2, true)",
            "_setText(n0, \"a \" + _toDisplayString(_ctx.x))",
            "_setText(n1, \"b \" + _toDisplayString(_ctx.y))",
        ],
    );
}

#[test]
fn a_static_text_run_beside_elements_stays_in_the_template() {
    let output = render("<template><div>a {{ x }}<p/>b</div></template>");
    for expected in [
        "_template(\"<div> <p></p>b\", 1)",
        "const n0 = _child(n1, true)",
        "_setText(n0, \"a \" + _toDisplayString(_ctx.x))",
    ] {
        assert!(
            output.code.contains(expected),
            "{expected}\n{}",
            output.code
        );
    }
}

#[test]
fn a_text_run_after_an_element_is_reached_from_its_sibling() {
    assert_contains_all(
        "<template><label><input/>{{ a }}</label></template>",
        &[
            "_template(\"<label><input> \", 1)",
            "const n0 = _next(_child(n1), true)",
            "_setText(n0, _toDisplayString(_ctx.a))",
        ],
    );
}

#[test]
fn a_block_before_kept_whitespace_is_inserted_before_its_anchor() {
    assert_contains_all(
        "<template><div><Comp/> <Other/></div></template>",
        &[
            "_template(\"<div><!> \", 1)",
            "const n2 = _child(n3)",
            "_setInsertionState(n3, n2)",
            "_setInsertionState(n3, 2)",
        ],
    );
}

#[test]
fn a_conditional_inside_a_template_loop_is_multi_root_on_both_sides() {
    assert_contains_all(
        "<template><div><template v-for=\"i in xs\"><p v-if=\"a\">x</p></template></div></template>",
        &["}, null, 10 /* TRUE_MULTI_ROOT, FALSE_MULTI_ROOT */)"],
    );
    assert_contains_all(
        "<template><div><template v-for=\"i in xs\"><p v-if=\"a\">x</p><b v-else-if=\"b\"/><i v-else/></template></div></template>",
        &[
            "522 /* TRUE_MULTI_ROOT, FALSE_MULTI_ROOT, KEYED_INDEX_1 */",
            "270 /* TRUE_MULTI_ROOT, FALSE_ELSE_IF, KEYED_INDEX_0 */",
        ],
    );
}
