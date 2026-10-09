use super::*;

// ── Structural directive removal (v-if, v-for, v-slot) ───

#[test]
fn v_if_attribute_removed_from_output() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="show">hello</div></template>"#,
        &[("show", BindingType::SetupRef)],
    );
    // Positive: IIFE if-block should be present
    assert!(
        result.contains("if(show)"),
        "v-if condition should produce IIFE if-block, got: {}",
        result
    );
    // Negative: v-if attribute must NOT appear in output
    assert!(
        !result.contains("v-if"),
        "v-if attribute must be removed from JSX output, got: {}",
        result
    );
}

#[test]
fn v_if_compound_expr_attribute_removed() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="a || b" class="foo">hello</div></template>"#,
        &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
    );
    assert!(
        !result.contains("v-if"),
        "v-if attribute must not appear in output, got: {}",
        result
    );
    // The condition should be in the ternary
    assert!(
        result.contains("a || b"),
        "resolved condition should be in ternary, got: {}",
        result
    );
    // The class attribute should still be present
    assert!(
        result.contains(r#"class="foo""#),
        "class attribute should be preserved, got: {}",
        result
    );
}

#[test]
fn v_if_with_props_binding_attribute_removed() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="show" class="active">content</div></template>"#,
        &[("show", BindingType::Props)],
    );
    assert!(
        !result.contains("v-if"),
        "v-if must be removed from output, got: {}",
        result
    );
    assert!(
        result.contains("if(__props.show)"),
        "should have __props.show in if-condition, got: {}",
        result
    );
    // v-if value should NOT appear as string attribute value
    assert!(
        !result.contains(r#"="show""#) && !result.contains(r#"="__props.show""#),
        "v-if value should not be in attribute quotes, got: {}",
        result
    );
}

#[test]
fn v_for_attribute_removed_from_output() {
    let result = gen_tsx_template(
        r#"<template><div v-for="item in items" :key="item.id">{{ item.name }}</div></template>"#,
    );
    assert!(
        !result.contains("v-for"),
        "v-for attribute must be removed from JSX output, got: {}",
        result
    );
    // Positive: the v-for frame should be present
    assert!(
        result.contains("___VERTER___flowEach"),
        "v-for should produce a frame, got: {}",
        result
    );
    // The " in " separator should not appear as raw text
    assert!(
        !result.contains(r#""item in items""#),
        "v-for expression should not appear as attribute value string, got: {}",
        result
    );
}

#[test]
fn v_for_with_props_binding_attribute_removed() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><li v-for="item in list">{{ item }}</li></template>"#,
        &[("list", BindingType::Props)],
    );
    assert!(
        !result.contains("v-for"),
        "v-for must be removed from output, got: {}",
        result
    );
    assert!(
        result.contains("const ___VERTER___v0 = (__props.list);"),
        "iterable should get __props. prefix, got: {}",
        result
    );
}

#[test]
fn v_once_attribute_removed_from_output() {
    let result = gen_tsx_template(r#"<template><div v-once>static content</div></template>"#);
    assert!(
        !result.contains("v-once"),
        "v-once must be removed from JSX output, got: {}",
        result
    );
    assert!(
        result.contains("<div>"),
        "element should still be present, got: {}",
        result
    );
}

#[test]
fn multiple_directives_all_removed() {
    let result =
        gen_tsx_template(r#"<template><div v-if="show" v-once class="box">hello</div></template>"#);
    assert!(
        !result.contains("v-if"),
        "v-if must be removed, got: {}",
        result
    );
    assert!(
        !result.contains("v-once"),
        "v-once must be removed, got: {}",
        result
    );
    assert!(
        result.contains(r#"class="box""#),
        "regular attributes should be preserved, got: {}",
        result
    );
}

// ── v-for comprehensive tests ────────────────────────────────

#[test]
fn v_for_destructured_params() {
    let result = gen_tsx_template(
        r#"<template><li v-for="(item, index) in items" :key="index">{{ item }}</li></template>"#,
    );
    assert!(
        !result.contains("v-for"),
        "v-for attribute must be removed, got: {}",
        result
    );
    assert!(
        result.contains("const [item, index] = ___VERTER___flowEach2("),
        "destructured params should be the v-for frame aliases, got: {}",
        result
    );
    // " in " separator must not appear as raw text
    assert!(
        !result.contains("\" in \"") && !result.contains(" in items"),
        "v-for separator must not appear in output, got: {}",
        result
    );
}

#[test]
fn v_for_object_destructure() {
    let result = gen_tsx_template(
        r#"<template><div v-for="(value, key, index) in obj">{{ key }}: {{ value }}</div></template>"#,
    );
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    assert!(
        result.contains("const [value, key, index] = ___VERTER___flowEach3("),
        "triple destructure should be the v-for frame aliases, got: {}",
        result
    );
}

#[test]
fn v_for_of_variant() {
    let result =
        gen_tsx_template(r#"<template><span v-for="item of items">{{ item }}</span></template>"#);
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    assert!(
        result.contains("___VERTER___flowEach"),
        "should produce a v-for frame, got: {}",
        result
    );
    // "of" separator must not leak
    assert!(
        !result.contains(" of items"),
        "v-for 'of' separator must not appear in output, got: {}",
        result
    );
    // The alias is typed by the frame helper, never by an indexed access.
    assert!(
        !result.contains("[number]"),
        "alias must not carry an indexed-access annotation, got: {}",
        result
    );
}

#[test]
fn v_for_simple_param_is_typed_by_frame_helper_not_indexed_access() {
    // `v-for="n in count"` (number) / Map / Set / nullable sources have no
    // numeric index; an indexed-access annotation would be a false diagnostic.
    let result = gen_tsx_template(r#"<template><div v-for="n in count">{{ n }}</div></template>"#);
    assert!(
        result.contains("const n = ___VERTER___flowEach1(___VERTER___v0);"),
        "alias must be typed only by flowEach1, got: {}",
        result
    );
    assert!(
        !result.contains("[number]"),
        "no indexed-access annotation on the alias, got: {}",
        result
    );
}

#[test]
fn v_for_destructured_param_has_type_annotation() {
    let result = gen_tsx_template(
        r#"<template><div v-for="{ name, email } in users">{{ name }}</div></template>"#,
    );
    // Destructured pattern without comma in the params (commas are inside braces but
    // the top-level params string is "{ name, email }" which contains commas)
    // This should NOT get annotation because the params contain a comma
    assert!(
        !result.contains("(typeof users)[number]"),
        "destructured params with commas should not get type annotation, got: {}",
        result
    );
}

#[test]
fn v_for_multi_param_no_annotation() {
    let result = gen_tsx_template(
        r#"<template><li v-for="(item, index) in items" :key="index">{{ item }}</li></template>"#,
    );
    assert!(
        !result.contains("(typeof items)[number]"),
        "multi-param v-for should not get type annotation, got: {}",
        result
    );
}

#[test]
fn v_for_complex_iterable_no_annotation() {
    let result = gen_tsx_template(
        r#"<template><span v-for="item in getItems()">{{ item }}</span></template>"#,
    );
    assert!(
        !result.contains("(typeof"),
        "complex iterable (function call) should not get type annotation, got: {}",
        result
    );
}

#[test]
fn v_for_numeric_range() {
    let result = gen_tsx_template(r#"<template><span v-for="n in 10">{{ n }}</span></template>"#);
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    // A numeric range is the frame helper's source (it iterates 1..=N like
    // Vue's `renderList`); it is never a receiver (`10.map(...)` is invalid).
    assert!(
        result.contains("const ___VERTER___v0 = (10);"),
        "numeric range should be the frame source, got: {}",
        result
    );
    assert!(
        result.contains("___VERTER___flowEach"),
        "numeric range should be a v-for frame source, got: {}",
        result
    );
}

#[test]
fn v_for_complex_iterable_expression() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-for="item in items.filter(x => x.active)" :key="item.id">{{ item.name }}</div></template>"#,
        &[("items", BindingType::SetupConst)],
    );
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    assert!(
        result.contains(".filter("),
        "complex iterable expression should be preserved, got: {}",
        result
    );
    assert!(
        result.contains("___VERTER___flowEach"),
        "should have a v-for frame, got: {}",
        result
    );
}

#[test]
fn v_for_setup_ref_iterable_binding() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><li v-for="item in todos">{{ item.text }}</li></template>"#,
        &[("todos", BindingType::SetupRef)],
    );
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    assert!(
        result.contains("const ___VERTER___v0 = (todos);") && !result.contains("todos.value"),
        "SetupRef iterable should be bare identifier in TSX mode (no .value), got: {}",
        result
    );
}

#[test]
fn v_for_closing_structure() {
    let result = gen_tsx_template(
        r#"<template><div v-for="item in items" :key="item.id">text</div></template>"#,
    );
    assert!(
        result.contains("); } })()}"),
        "v-for closing should close the frame body, the frame arrow, its call and the JSX container, got: {}",
        result
    );
}

// ── v-if IIFE structure tests ─────────────────────────────

#[test]
fn v_if_iife_structure() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="visible">hello</div></template>"#,
        &[("visible", BindingType::SetupRef)],
    );
    // Must have IIFE pattern: {(()=>{if(cond){...}}}
    assert!(
        result.contains("{(()=>{if(visible){"),
        "v-if should open with IIFE if-block, got: {}",
        result
    );
    // Must close with }}} (block close + arrow body close + JSX expression close)
    assert!(
        result.contains("}})()}"),
        "v-if standalone should close the branch and the invoked block, got: {}",
        result
    );
    // Must NOT have ternary pattern
    assert!(
        !result.contains("? ("),
        "should not use ternary pattern, got: {}",
        result
    );
    assert!(
        !result.contains(": null}"),
        "should not have null fallback, got: {}",
        result
    );
}

#[test]
fn v_if_else_chain_iife_structure() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
        &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
    );
    // Should have IIFE if/else-if/else chain
    assert!(
        result.contains("{(()=>{if(a){"),
        "should have IIFE if-block, got: {}",
        result
    );
    assert!(
        result.contains("else if(b){"),
        "should have else-if block, got: {}",
        result
    );
    assert!(
        result.contains("else{"),
        "should have else block, got: {}",
        result
    );
    // Should close with }}} at the end (else block close + arrow body + JSX)
    assert!(
        result.contains("}})()}"),
        "chain should close properly, got: {}",
        result
    );
    // Should NOT have standalone "v-else" text
    assert!(
        !result.contains("v-else"),
        "v-else must not appear as attribute, got: {}",
        result
    );
}

#[test]
fn v_if_else_if_without_else_closes() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="a">A</div><div v-else-if="b">B</div></template>"#,
        &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
    );
    assert!(
        result.contains("{(()=>{if(a){"),
        "should have IIFE if-block, got: {}",
        result
    );
    assert!(
        result.contains("else if(b){"),
        "should have else-if block, got: {}",
        result
    );
    // Without v-else, parent loop adds }}
    assert!(
        result.contains("}})()}"),
        "chain without else should close the branch and the invoked block, got: {}",
        result
    );
}

#[test]
fn v_if_with_binding_prefix_iife() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="show">content</div></template>"#,
        &[("show", BindingType::Props)],
    );
    assert!(
        result.contains("{(()=>{if(__props.show){"),
        "should use __props.show in if-condition, got: {}",
        result
    );
}

// ── v-if nested IIFE tests ──────────────────────────────────

#[test]
fn v_if_nested_chain_inherits_parent_narrowing_through_flow() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="parent"><span v-if="child">nested</span></div></template>"#,
        &[
            ("parent", BindingType::SetupRef),
            ("child", BindingType::SetupRef),
        ],
    );
    // Both chains are immediately invoked blocks, the nested one inside the
    // outer branch, so it continues the flow `parent` narrowed.
    let outer = result
        .find("{(()=>{if(parent){")
        .expect("outer chain block");
    let nested = result
        .find("{(()=>{if(child){")
        .expect("nested chain block");
    let outer_close = result.rfind("})()}").expect("outer block close");
    assert!(
        outer < nested && nested < outer_close,
        "the nested chain must sit inside the outer branch, got: {}",
        result
    );
    // Negative: the parent condition is emitted once, never replayed into the
    // nested chain.
    assert_eq!(
        result.matches("parent").count(),
        1,
        "the parent condition must not be replayed, got: {}",
        result
    );
    assert!(
        !result.contains("throw 0") && !result.contains("return;"),
        "a nested chain needs no guard, got: {}",
        result
    );
}

// ── Part F: Comment repositioning ────────────────────────────────

#[test]
fn v_if_comment_before_repositioned_inside_iife() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><div v-if="show">hello</div></template>"#,
        &[("show", BindingType::SetupRef)],
    );
    // Comment should appear INSIDE the IIFE, after the if(cond){ line
    // Pattern: {(()=>{if(cond){ {/* @ts-expect-error */} <div>...
    assert!(
        result.contains("if(show)"),
        "should have IIFE condition, got:\n{}",
        result
    );
    // Comment must be AFTER the IIFE open, not before it
    let iife_pos = result.find("{(()=>{").expect("should have IIFE open");
    let comment_pos = result
        .find("{/* @ts-expect-error */}")
        .expect("comment should be preserved");
    assert!(
        comment_pos > iife_pos,
        "comment should appear AFTER IIFE open, got:\n{}",
        result
    );
    // Negative: comment should NOT appear before the IIFE
    let before_iife = &result[..iife_pos];
    assert!(
        !before_iife.contains("@ts-expect-error"),
        "comment must not appear before IIFE, got:\n{}",
        result
    );
}

#[test]
fn v_if_without_preceding_comment_no_change() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="show">hello</div></template>"#,
        &[("show", BindingType::SetupRef)],
    );
    // No comment to reposition — should work normally
    assert!(
        result.contains("{(()=>{if(show){"),
        "should have IIFE pattern, got:\n{}",
        result
    );
    assert!(
        !result.contains("{/*"),
        "should not have any comments, got:\n{}",
        result
    );
}

// ── Part F2: v-if/v-else with whitespace between elements ────────

#[test]
fn v_if_else_with_whitespace_between_elements() {
    // Simulates formatted template: <img v-if="cond" />\n  <span v-else>fallback</span>
    let result = gen_tsx_template_with_bindings(
        "<template>\n  <img v-if=\"show\" />\n  <span v-else>fallback</span>\n</template>",
        &[("show", BindingType::SetupRef)],
    );

    // Positive: must have complete IIFE chain with if/else
    assert!(
        result.contains("{(()=>{if(show){"),
        "should have IIFE if-block, got:\n{}",
        result
    );
    assert!(
        result.contains("else{"),
        "should have else block in same IIFE, got:\n{}",
        result
    );

    // Structural: IIFE must NOT close before else — no }}} between IIFE start and else
    let iife_start = result.find("{(()=>{if(").unwrap();
    let else_pos = result.find("else{").unwrap();
    let between = &result[iife_start..else_pos];
    assert!(
        !between.contains("})()}"),
        "IIFE must not close before else: premature close found between IIFE start and else, got:\n{}",
        result
    );

    // Negative: v-if/v-else attributes must not appear in output
    assert!(
        !result.contains("v-if"),
        "v-if attribute must be removed from JSX, got:\n{}",
        result
    );
    assert!(
        !result.contains("v-else"),
        "v-else attribute must be removed from JSX, got:\n{}",
        result
    );

    // Validate JSX syntax: the full template output must parse
    let wrapper = format!("const x = {}", result);
    let val_alloc = oxc_allocator::Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "TSX template output has syntax errors: {:?}\n--- output ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        result
    );
}

#[test]
fn v_if_else_if_else_with_whitespace() {
    let result = gen_tsx_template_with_bindings(
        "<template>\n  <div v-if=\"a\">A</div>\n  <div v-else-if=\"b\">B</div>\n  <div v-else>C</div>\n</template>",
        &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
    );

    // Positive: complete IIFE chain
    assert!(
        result.contains("{(()=>{if(a){"),
        "should have IIFE if-block, got:\n{}",
        result
    );
    assert!(
        result.contains("else if(b){"),
        "should have else-if block, got:\n{}",
        result
    );
    assert!(
        result.contains("else{"),
        "should have else block, got:\n{}",
        result
    );

    // Structural: IIFE must NOT close before else-if or else
    let iife_start = result.find("{(()=>{if(").unwrap();
    let else_if_pos = result.find("else if(").unwrap();
    let else_pos = result.find("else{").unwrap();
    let between_if_and_else_if = &result[iife_start..else_if_pos];
    assert!(
        !between_if_and_else_if.contains("})()}"),
        "IIFE must not close before else-if, got:\n{}",
        result
    );
    let between_else_if_and_else = &result[else_if_pos..else_pos];
    assert!(
        !between_else_if_and_else.contains("})()}"),
        "IIFE must not close before else, got:\n{}",
        result
    );

    // Negative: directive attributes must not appear
    assert!(
        !result.contains("v-if"),
        "v-if must be removed, got:\n{}",
        result
    );
    assert!(
        !result.contains("v-else"),
        "v-else must be removed, got:\n{}",
        result
    );

    // Validate JSX syntax: the full template output must parse
    let wrapper = format!("const x = {}", result);
    let val_alloc = oxc_allocator::Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "TSX template output has syntax errors: {:?}\n--- output ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        result
    );
}

// ── v-bind function prop guards ──────────────────

#[test]
fn v_bind_arrow_expr_body_becomes_guarded_block_returning_body() {
    // Arrow expression body under v-if: the outer references it reads are
    // snapshot in the branch block and re-narrowed at the body start, and the
    // body is returned as authored (a return-type error still lands on it).
    let result = gen_tsx_template(
        r#"<template><div v-if="typeof msg === 'string'" :handler="() => msg.trim()">hi</div></template>"#,
    );
    let branch = result
        .find("{(()=>{if(typeof ___VERTER___instance.msg === 'string'){\n")
        .expect("branch block");
    let snapshot = result
        .find("const ___VERTER___o0 = ___VERTER___instance.msg;\nconst ___VERTER___o1 = ___VERTER___instance.msg!.trim;\n")
        .expect("snapshots of every outer reference prefix, in the branch block");
    assert!(
        branch < snapshot,
        "snapshots follow the branch condition: {result}"
    );
    assert!(
        result.contains(
            "handler={() => { if (!___VERTER___flowNarrow(___VERTER___instance.msg, ___VERTER___o0) || ___VERTER___flowExcluded(___VERTER___o0)(___VERTER___instance.msg) || !___VERTER___flowNarrow(___VERTER___instance.msg!.trim, ___VERTER___o1) || ___VERTER___flowExcluded(___VERTER___o1)(___VERTER___instance.msg!.trim)) throw 0; return ___VERTER___instance.msg.trim(); }}"
        ),
        "arrow expression body should become a guarded block returning the body, got:\n{}",
        result
    );
    // Negative: the condition is never replayed into the callback.
    assert_eq!(
        result.matches("=== 'string'").count(),
        1,
        "the condition must be emitted once, got:\n{}",
        result
    );
}

#[test]
fn v_bind_arrow_block_gets_block_guard() {
    // Arrow block body under v-if: the guard opens the authored block.
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="typeof msg === 'string'" :handler="() => { return msg.trim() }">hi</div></template>"#,
        &[("msg", BindingType::SetupConst)],
    );
    assert!(
        result.contains("const ___VERTER___o0 = msg;\nconst ___VERTER___o1 = msg!.trim;\n"),
        "outer references are snapshot in the branch block, got:\n{}",
        result
    );
    assert!(
        result.contains(
            "handler={() => {if (!___VERTER___flowNarrow(msg, ___VERTER___o0) || ___VERTER___flowExcluded(___VERTER___o0)(msg) || !___VERTER___flowNarrow(msg!.trim, ___VERTER___o1) || ___VERTER___flowExcluded(___VERTER___o1)(msg!.trim)) throw 0;  return msg.trim() }}"
        ),
        "arrow block prop should get the guard right after its `{{`, got:\n{}",
        result
    );
}

#[test]
fn jsx_valid_v_if_alone() {
    assert_valid_jsx(
        r#"<template><div v-if="show">content</div></template>"#,
        "v-if alone",
    );
}

#[test]
fn jsx_valid_v_if_else() {
    assert_valid_jsx(
        r#"<template><div v-if="show">A</div><div v-else>B</div></template>"#,
        "v-if/v-else inline",
    );
}

#[test]
fn jsx_valid_v_if_else_if_else() {
    assert_valid_jsx(
        r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
        "v-if/v-else-if/v-else inline",
    );
}

#[test]
fn jsx_valid_v_if_else_whitespace() {
    assert_valid_jsx(
        "<template>\n  <div v-if=\"show\">A</div>\n  <div v-else>B</div>\n</template>",
        "v-if/v-else with whitespace",
    );
}

#[test]
fn jsx_valid_v_if_else_if_else_whitespace() {
    assert_valid_jsx(
        "<template>\n  <div v-if=\"a\">A</div>\n  <div v-else-if=\"b\">B</div>\n  <div v-else>C</div>\n</template>",
        "v-if/v-else-if/v-else with whitespace",
    );
}

#[test]
fn jsx_valid_v_for_alone() {
    assert_valid_jsx(
        r#"<template><div v-for="item in items" :key="item.id">{{ item.name }}</div></template>"#,
        "v-for alone",
    );
}

#[test]
fn jsx_valid_v_for_with_index() {
    assert_valid_jsx(
        r#"<template><div v-for="(item, index) in items" :key="index">{{ item }}</div></template>"#,
        "v-for with index",
    );
}

#[test]
fn jsx_valid_v_if_v_for_same_element() {
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="item in items" :key="item">{{ item }}</div></template>"#,
        "v-if + v-for same element",
    );
}

#[test]
fn jsx_valid_v_for_with_v_if_children() {
    assert_valid_jsx(
        r#"<template><ul><li v-for="item in items" :key="item.id"><span v-if="item.active">active</span><span v-else>inactive</span></li></ul></template>"#,
        "v-for with v-if/v-else children",
    );
}

#[test]
fn jsx_valid_v_for_with_v_if_children_whitespace() {
    assert_valid_jsx(
        "<template>\n  <ul>\n    <li v-for=\"item in items\" :key=\"item.id\">\n      <span v-if=\"item.active\">active</span>\n      <span v-else>inactive</span>\n    </li>\n  </ul>\n</template>",
        "v-for with v-if/v-else children whitespace",
    );
}

#[test]
fn jsx_valid_nested_v_if() {
    assert_valid_jsx(
        r#"<template><div v-if="a"><span v-if="b">B</span><span v-else>not B</span></div></template>"#,
        "nested v-if chains",
    );
}

#[test]
fn jsx_valid_v_if_with_template_v_for() {
    assert_valid_jsx(
        "<template>\n  <div v-if=\"show\">\n    <span v-for=\"item in items\" :key=\"item\">{{ item }}</span>\n  </div>\n  <div v-else>empty</div>\n</template>",
        "v-if with v-for inside + v-else",
    );
}

#[test]
fn jsx_valid_multiple_v_if_chains() {
    assert_valid_jsx(
        "<template>\n  <div v-if=\"a\">A</div>\n  <div v-else>not A</div>\n  <div v-if=\"b\">B</div>\n  <div v-else>not B</div>\n</template>",
        "multiple separate v-if chains with whitespace",
    );
}

#[test]
fn jsx_valid_all_directives_combined() {
    assert_valid_jsx(
        "<template>\n  <div v-if=\"hasItems\">\n    <MyComp v-for=\"item in items\" :key=\"item.id\" v-slot=\"{ row }\">\n      <span v-if=\"row.active\">{{ row.name }}</span>\n      <span v-else>inactive</span>\n    </MyComp>\n  </div>\n  <div v-else>no items</div>\n</template>",
        "v-if + v-for + v-slot + nested v-if/v-else",
    );
}

// ===================================================================
// ===================================================================

#[test]
fn v_show_with_ref_binding_gets_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-show="visible">hi</div></template>"#,
        &[("visible", BindingType::SetupRef)],
    );
    assert!(
        result.contains("visible") && !result.contains("visible.value"),
        "v-show ref binding should be bare identifier in TSX mode (no .value). Got: {}",
        result
    );
    assert!(
        result.contains("display:"),
        "v-show should produce style display. Got: {}",
        result
    );
    assert!(
        !result.contains("v-show"),
        "v-show attribute must be removed. Got: {}",
        result
    );
}

#[test]
fn v_show_with_props_binding_gets_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-show="isVisible">hi</div></template>"#,
        &[("isVisible", BindingType::Props)],
    );
    assert!(
        result.contains("__props.isVisible"),
        "v-show props binding should have __props. prefix. Got: {}",
        result
    );
}

#[test]
fn v_show_compound_expr_resolves_all_bindings() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-show="isAdmin && visible">hi</div></template>"#,
        &[
            ("isAdmin", BindingType::Props),
            ("visible", BindingType::SetupRef),
        ],
    );
    assert!(
        result.contains("__props.isAdmin"),
        "v-show should resolve isAdmin as props. Got: {}",
        result
    );
    assert!(
        result.contains("visible") && !result.contains("visible.value"),
        "v-show should resolve visible as bare identifier in TSX mode (no .value). Got: {}",
        result
    );
}

#[test]
fn v_show_with_existing_style_no_duplicate_attributes() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-show="ready" :style="itemStyle">hi</div></template>"#,
        &[
            ("ready", BindingType::SetupRef),
            ("itemStyle", BindingType::SetupConst),
        ],
    );
    // Should NOT produce duplicate `style` attributes
    let style_count = result.matches("style=").count();
    assert_eq!(
        style_count, 1,
        "v-show + :style should merge into one style attribute, not produce {} style= occurrences. Got: {}",
        style_count, result
    );
    // Should include both the v-show display logic and the existing style
    assert!(
        result.contains("display:"),
        "merged style should include v-show display logic. Got: {}",
        result
    );
    // Should NOT have v-show attribute
    assert!(
        !result.contains("v-show"),
        "v-show attribute must be removed. Got: {}",
        result
    );
}

// ── v-model in TSX ────────────────────────────────────────────

#[test]
fn v_model_basic_component() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><Comp v-model="count" /></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    assert!(
        result.contains("modelValue={count}"),
        "v-model should produce modelValue prop. Got: {}",
        result
    );
    assert!(
        result.contains("\"onUpdate:modelValue\""),
        "v-model should produce onUpdate:modelValue handler. Got: {}",
        result
    );
    // Must use spread syntax (bare quoted attribute is invalid JSX)
    assert!(
        !result.contains("\"onUpdate:modelValue\"={"),
        "onUpdate handler must NOT be a bare JSX attribute. Got: {}",
        result
    );
    assert!(
        !result.contains("v-model"),
        "v-model attribute must be removed from JSX. Got: {}",
        result
    );
}

#[test]
fn v_model_named() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><Comp v-model:title="title" /></template>"#,
        &[("title", BindingType::SetupRef)],
    );
    assert!(
        result.contains("title={title}"),
        "named v-model should produce named prop. Got: {}",
        result
    );
    assert!(
        result.contains("\"onUpdate:title\""),
        "named v-model should produce onUpdate:title handler. Got: {}",
        result
    );
    // Must use spread syntax (bare quoted attribute is invalid JSX)
    assert!(
        !result.contains("\"onUpdate:title\"={"),
        "named onUpdate handler must NOT be a bare JSX attribute. Got: {}",
        result
    );
}

#[test]
fn v_model_with_binding_resolution() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><Comp v-model="count" /></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    assert!(
        result.contains("modelValue={count}") && !result.contains("count.value"),
        "v-model on ref should resolve to bare identifier in TSX mode (no .value). Got: {}",
        result
    );
}

#[test]
fn v_model_with_explicit_checked_prop_no_duplicate() {
    // v-model on <input type="radio"> + explicit :checked + @change should not
    // produce duplicate checked or onChange attributes.
    let result = gen_tsx_template_with_bindings(
        r#"<template><input v-model="modelValue" type="radio" :checked="modelValue === val" @change="handleChange" /></template>"#,
        &[
            ("modelValue", BindingType::SetupRef),
            ("val", BindingType::SetupConst),
            ("handleChange", BindingType::SetupConst),
        ],
    );
    let checked_count = result.matches("checked=").count();
    let on_change_count = result.matches("onChange=").count();
    assert_eq!(
        checked_count, 1,
        "v-model + :checked on radio should produce one checked attr. Got {} in: {}",
        checked_count, result
    );
    assert_eq!(
        on_change_count, 1,
        "v-model + @change on radio should produce one onChange. Got {} in: {}",
        on_change_count, result
    );
}

#[test]
fn self_closing_template_v_if_produces_valid_jsx() {
    // <template v-if="..." /> is self-closing with no children.
    // The IIFE wrapping must produce valid JSX (empty fragment or null).
    let result = gen_tsx_template_with_bindings(
        r#"<template><template v-if="noFooter" /><template v-else><div>footer</div></template></template>"#,
        &[("noFooter", BindingType::SetupConst)],
    );
    // Positive: should have the v-if condition
    assert!(
        result.contains("noFooter"),
        "v-if condition should be present. Got: {}",
        result
    );
    // Negative: no unclosed fragments — count <> and </> should match
    let open_frags = result.matches("<>").count();
    let close_frags = result.matches("</>").count();
    assert_eq!(
        open_frags, close_frags,
        "fragment open/close count should match. Got {} opens and {} closes in: {}",
        open_frags, close_frags, result
    );
}

#[test]
fn v_for_iterable_is_source_mapped() {
    // v-for="item in items" — the iterable `items` in the frame helper call
    // should have a source map token pointing back to the original `items` position.
    let source = r#"<template><div v-for="item in items">{{ item }}</div></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Verify output shape
    assert!(
        output.contains("___VERTER___flowEach"),
        "v-for should produce a frame: {output}"
    );

    // Find the byte offset of "items" in the v-for attribute value
    let items_src_offset = source.find("item in items").unwrap() + "item in ".len();

    // There should be a source map token pointing to the iterable position
    let has_iterable_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == items_src_offset as u32);
    assert!(
        has_iterable_token,
        "v-for iterable should have source map token at src col {}. Tokens: {:?}",
        items_src_offset, tokens
    );
}

#[test]
fn v_for_param_is_source_mapped() {
    // The frame alias `item` in `const item = …` should map back
    // to the parameter position in the v-for attribute value.
    let source = r#"<template><div v-for="item in items">{{ item }}</div></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    assert!(
        output.contains("const item"),
        "v-for should declare its alias in a frame: {output}"
    );

    // "item" starts right after the opening quote of v-for="
    let param_src_offset = source.find("item in items").unwrap();

    let has_param_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == param_src_offset as u32);
    assert!(
        has_param_token,
        "v-for parameter should have source map token at src col {}. Tokens: {:?}",
        param_src_offset, tokens
    );
}

#[test]
fn component_dynamic_is_removes_is_directive() {
    let source = r#"<template><component :is="tag" class="foo"></component></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("tag", BindingType::SetupRef)]);

    assert!(
        output.contains("class=\"foo\""),
        "class attribute should be preserved: {output}"
    );
    assert!(
        !output.contains(":is="),
        ":is= directive should be removed: {output}"
    );
}

// ── Split overwrite tests for source map accuracy ────────────────

/// `v-bind="$attrs"` must produce `{...___VERTER___instance.$attrs}` using split
/// overwrites so that `$attrs` retains its original source position in the source map.
/// Without the split, TSGO hover lands on `___VERTER___instance` instead.
#[test]
fn v_bind_spread_attrs_source_map_accuracy() {
    let source = r#"<template><div v-bind="$attrs"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Positive: spread with instance prefix
    assert!(
        output.contains("{...___VERTER___instance.$attrs}"),
        "v-bind=\"$attrs\" should produce spread with instance prefix: {output}"
    );
    // Negative: no raw v-bind
    assert!(
        !output.contains("v-bind"),
        "v-bind attribute must be removed from JSX: {output}"
    );

    // Source map: find the source column of `$attrs` in the original
    let source_attrs_col = source.find("$attrs").expect("$attrs in source") as u32;

    // Tokens are (dst_line, dst_col, src_col) for line 0 tokens with source_id.
    // With the split overwrite, there should be a token mapping generated $attrs
    // back to source col of $attrs. Without the split, only prop.start is mapped.
    let has_attrs_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == source_attrs_col);
    assert!(
        has_attrs_token,
        "source map must have a token mapping to the original $attrs position (col {}), \
         but only found source columns: {:?}",
        source_attrs_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );
}

// ========================================================================
// Fix 5: Sourcemap coverage for member access and $props (Bugs 7, 11)
// ========================================================================

/// Member access in v-bind: `:prop="obj.field"` — verify sourcemap interpolation covers `.field`.
///
/// The PositionMapper uses interpolation between tokens, so we only need a token at `obj`
/// and the offset to `field` will be computed automatically. Verify the token exists for `obj`
/// and that the output preserves the expression unchanged.
#[test]
fn member_access_in_v_bind_source_map() {
    let source = r#"<template><Comp :prop="obj.field"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("obj", BindingType::SetupConst)]);

    // Positive: should emit prop={obj.field}
    assert!(
        output.contains("obj.field"),
        "should preserve obj.field: {output}"
    );

    // Sourcemap: verify `obj` has a token — interpolation covers `.field` from this token
    let obj_src_col = source.find("obj.field").unwrap() as u32;
    let has_obj_token = tokens.iter().any(|&(_, _, sc)| sc == obj_src_col);
    assert!(
        has_obj_token,
        "source map must have token for `obj` at col {}, tokens: {:?}",
        obj_src_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );

    // Verify no overwrite breaks the linear mapping between obj and field:
    // Both must be on the same generated line and same source line with matching offsets.
    let field_src_col = source.find("field").unwrap() as u32;
    let obj_offset = field_src_col - obj_src_col; // 4 chars ("obj.")

    // Find the generated column of the `obj` token
    let obj_gen = tokens
        .iter()
        .find(|&&(_, _, sc)| sc == obj_src_col)
        .map(|&(dl, dc, _)| (dl, dc));
    if let Some((_obj_line, obj_col)) = obj_gen {
        // Verify the generated output has `field` at obj_col + 4
        // (i.e., no inserted/removed text between obj and field)
        let gen_out = &output;
        let lines: Vec<&str> = gen_out.lines().collect();
        if let Some(line_str) = lines.first() {
            let gen_field_expected_col = obj_col + obj_offset;
            if (gen_field_expected_col as usize) < line_str.len() {
                let actual = &line_str[gen_field_expected_col as usize..];
                assert!(
                    actual.starts_with("field"),
                    "interpolation check: expected 'field' at generated col {}, but got '{}'",
                    gen_field_expected_col,
                    &actual[..actual.len().min(10)]
                );
            }
        }
    }
}

// ── v-bind shorthand `:` off-by-one source map tests ────────────

/// v-bind shorthand `:prop="expr"` — the source map token for the prop name
/// must point to the prop name itself (e.g., `class`), NOT to the `:` prefix.
///
/// Previously, `out.overwrite(prop.start, ...)` used `prop.start` which includes
/// the `:`, making all diagnostics off by 1 column.
#[test]
fn v_bind_shorthand_prop_name_source_map_accuracy() {
    let source = r#"<template><div :class="foo"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("foo", BindingType::SetupConst)]);

    // Positive: should emit class={foo}
    assert!(
        output.contains("class={foo}"),
        "should convert :class to class={{foo}}: {output}"
    );
    // Negative: no raw :class in output
    assert!(
        !output.contains(":class"),
        ":class must be removed from JSX output: {output}"
    );

    // Source map: the `class` prop name token should map to `class` in source,
    // not to the `:` that precedes it.
    let colon_src_col = source.find(":class").unwrap() as u32;
    let class_src_col = colon_src_col + 1; // `class` starts after `:`

    // Find the generated position of `class` in the output
    let class_gen_col = output.find("class={").unwrap() as u32;

    // There must be a token mapping generated `class` back to source `class` (not `:`)
    let has_correct_token = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == class_gen_col && sc == class_src_col);
    let has_wrong_token = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == class_gen_col && sc == colon_src_col);
    assert!(
        has_correct_token,
        "source map token for `class` should point to source col {} (the `c` in `class`), \
         not col {} (the `:`). Tokens: {:?}",
        class_src_col,
        colon_src_col,
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
    assert!(
        !has_wrong_token,
        "source map must NOT map generated `class` to the `:` position (col {}). \
         Tokens: {:?}",
        colon_src_col,
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
}

/// Same as above but for a longer prop name to confirm it's not just `class`.
/// `:title="msg"` — token for `title` should map to `t` not `:`.
#[test]
fn v_bind_shorthand_title_source_map_accuracy() {
    let source = r#"<template><Comp :title="msg"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("msg", BindingType::SetupConst)]);

    // Positive: should emit title={msg}
    assert!(
        output.contains("title={msg}"),
        "should convert :title to title={{msg}}: {output}"
    );

    let colon_src_col = source.find(":title").unwrap() as u32;
    let title_src_col = colon_src_col + 1;
    let title_gen_col = output.find("title={").unwrap() as u32;

    let has_correct_token = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == title_gen_col && sc == title_src_col);
    assert!(
        has_correct_token,
        "source map token for `title` should point to source col {} (the `t` in `title`), \
         not col {} (the `:`). Tokens: {:?}",
        title_src_col,
        colon_src_col,
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
}

/// v-bind shorthand without value: `:foo` → `foo={foo}`.
/// The prop name token should map to `foo`, not the `:`.
#[test]
fn v_bind_shorthand_no_value_source_map_accuracy() {
    let source = r#"<template><Comp :foo/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("foo", BindingType::SetupConst)]);

    // Positive: should emit foo={foo}
    assert!(
        output.contains("foo={foo}"),
        "should convert :foo to foo={{foo}}: {output}"
    );

    let colon_src_col = source.find(":foo").unwrap() as u32;
    let foo_src_col = colon_src_col + 1;
    let foo_gen_col = output.find("foo={").unwrap() as u32;

    let has_correct_token = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == foo_gen_col && sc == foo_src_col);
    assert!(
        has_correct_token,
        "source map token for `foo` should point to source col {} (the `f` in `foo`), \
         not col {} (the `:`). Tokens: {:?}",
        foo_src_col,
        colon_src_col,
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
}

/// v-bind shorthand without value: `:foo` ≡ `:foo="foo"`. The generated VALUE
/// identifier (inside `foo={…}`) must map back to the source `foo` arg token so
/// go-to-definition on the binding-resolved value lands on the template `foo`
/// (whose binding resolves to the declaration). Distinct from
/// `v_bind_shorthand_no_value_source_map_accuracy`, which pins the NAME (LHS)
/// mapping. Pre-fix the value was baked into a single `out.overwrite(arg_end, …,
/// "={foo}")` whose `Overwritten` chunk maps the whole run back to `arg_end`, so
/// the value identifier had NO token at the source `foo` start — this test fails
/// against that tree and passes once the value routes through the `EmitOp`
/// substrate.
#[test]
fn v_bind_shorthand_no_value_value_maps_to_source() {
    let source = r#"<template><Comp :foo/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("foo", BindingType::SetupConst)]);

    assert!(
        output.contains("foo={foo}"),
        "should convert :foo to foo={{foo}}: {output}"
    );

    let colon_src_col = source.find(":foo").unwrap() as u32;
    let foo_src_col = colon_src_col + 1; // the `f` of the arg token

    // The VALUE identifier is the `foo` INSIDE the braces: `foo={foo}` → value at
    // `+ "foo={".len()`. (The first `foo` is the NAME / LHS.)
    let pair_gen_col = output.find("foo={foo}").unwrap() as u32;
    let value_gen_col = pair_gen_col + "foo={".len() as u32;

    // Post-fix: a token at the value's generated column maps to the source `foo`
    // arg start. Pre-fix: the baked overwrite maps the run to `arg_end`, so no
    // token at `value_gen_col` points to `foo_src_col`.
    let value_maps_to_source = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == foo_src_col);
    assert!(
        value_maps_to_source,
        "the generated VALUE identifier `foo` (gen col {value_gen_col}) must map to source col \
         {foo_src_col} (the `f` in the `:foo` arg). Pre-fix it was baked into a mapped overwrite \
         anchored at arg_end and had no such token. Tokens: {:?}",
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );

    // Negative: the value identifier must NOT collapse to the prop start (`:`).
    let value_maps_to_colon = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == colon_src_col);
    assert!(
        !value_maps_to_colon,
        "the generated VALUE identifier must not map to the `:` (col {colon_src_col}). \
         Tokens: {:?}",
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
}

/// Long-form `v-bind:prop="expr"` — the prop name token should map to `prop`, not `v`.
#[test]
fn v_bind_longform_prop_name_source_map_accuracy() {
    let source = r#"<template><div v-bind:class="foo"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("foo", BindingType::SetupConst)]);

    // Positive: should emit class={foo}
    assert!(
        output.contains("class={foo}"),
        "should convert v-bind:class to class={{foo}}: {output}"
    );

    let vbind_src_col = source.find("v-bind:class").unwrap() as u32;
    let class_src_col = source.find(":class").unwrap() as u32 + 1; // after `:` in `v-bind:class`
    let class_gen_col = output.find("class={").unwrap() as u32;

    let has_correct_token = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == class_gen_col && sc == class_src_col);
    assert!(
        has_correct_token,
        "source map token for `class` should point to source col {} (the `c` in `class`), \
         not col {} (the `v` in `v-bind`). Tokens: {:?}",
        class_src_col,
        vbind_src_col,
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
}

// ── v-for body member access (regression test) ──────────────────

/// v-for iteration variables must NOT get the `___VERTER___instance.` prefix
/// in TSX output. They are locally scoped frame declarations.
#[test]
fn v_for_body_member_access_no_instance_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><button v-for="action in actions" :disabled="action.disabled">{{ action.label }}</button></template>"#,
        &[("actions", BindingType::SetupConst)],
    );
    eprintln!("TSX output:\n{}", result);

    // Positive: the v-for frame declares the alias
    assert!(
        result.contains("const action"),
        "should declare `action` in a v-for frame, got: {}",
        result
    );

    // Positive: member access expressions preserved bare
    assert!(
        result.contains("action.disabled"),
        "prop expression should contain bare action.disabled, got: {}",
        result
    );
    assert!(
        result.contains("action.label"),
        "interpolation should contain bare action.label, got: {}",
        result
    );

    // NEGATIVE: v-for locals must NOT get instance prefix
    assert!(
        !result.contains("___VERTER___instance.action"),
        "v-for param must NOT get ___VERTER___instance. prefix, got: {}",
        result
    );
}

/// Source map test: verify that `action.disabled` inside v-for body is source-mapped
/// back to its original position, enabling TSGO/tsserver to resolve member access.
#[test]
fn v_for_body_member_access_source_mapped() {
    let source = r#"<template><button v-for="action in actions" :disabled="action.disabled">text</button></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("actions", BindingType::SetupConst)]);
    eprintln!("TSX output:\n{}", output);
    eprintln!("Tokens (dst_line, dst_col, src_col):");
    for &(_dl, dc, sc) in &tokens {
        eprintln!("  gen_col={}, src_col={}", dc, sc);
    }

    // Find "action.disabled" in the generated output
    let gen_action_pos = output
        .find("action.disabled")
        .expect("action.disabled should be in output");
    let gen_dot_pos = gen_action_pos + "action".len();

    // Find "action.disabled" in the source
    let src_action_pos = source
        .find("action.disabled")
        .expect("action.disabled should be in source");
    let src_dot_pos = src_action_pos + "action".len();

    eprintln!(
        "gen 'action' at col={}, gen '.' at col={}",
        gen_action_pos, gen_dot_pos
    );
    eprintln!(
        "src 'action' at col={}, src '.' at col={}",
        src_action_pos, src_dot_pos
    );

    // Find the best token: the one closest to (but not after) the source position,
    // mimicking the PositionMapper::vue_to_tsx algorithm.
    let best_token = tokens
        .iter()
        .filter(|&&(dl, _, sc)| dl == 0 && (sc as usize) <= src_dot_pos)
        .max_by_key(|&&(_, _, sc)| sc);

    assert!(
        best_token.is_some(),
        "Should have a source map token at or before src_col={}. Tokens: {:?}",
        src_dot_pos,
        tokens
    );

    let &(_, base_dc, base_sc) = best_token.unwrap();
    let delta = src_dot_pos as u32 - base_sc;
    let interpolated_gen_dot = base_dc + delta;
    eprintln!(
        "best token: gen_col={}, src_col={}, delta={}, interpolated gen_dot={}",
        base_dc, base_sc, delta, interpolated_gen_dot
    );
    assert_eq!(
        interpolated_gen_dot as usize, gen_dot_pos,
        "Position interpolation for '.' should map src_col {} to gen_col {} (actual gen_dot={}). \
         This ensures completion at 'action.' maps to the correct TSX offset.",
        src_dot_pos, interpolated_gen_dot, gen_dot_pos
    );
}

/// Nested v-for: both outer and inner iteration variables must be bare.
#[test]
fn nested_v_for_body_no_instance_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-for="user in users" :key="user.id"><span v-for="item in user.items" :key="item.id">{{ user.name }}: {{ item.text }}</span></div></template>"#,
        &[("users", BindingType::SetupConst)],
    );
    eprintln!("TSX output:\n{}", result);

    // Positive: the outer v-for frame
    assert!(
        result.contains("const user"),
        "outer v-for frame declaring `user` expected, got: {}",
        result
    );

    // NEGATIVE: neither v-for local should get instance prefix
    assert!(
        !result.contains("___VERTER___instance.user"),
        "outer v-for param must NOT get instance prefix, got: {}",
        result
    );
    assert!(
        !result.contains("___VERTER___instance.item"),
        "inner v-for param must NOT get instance prefix, got: {}",
        result
    );
}

/// Destructured v-for params should remain bare.
#[test]
fn v_for_destructured_no_instance_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-for="{ name, email } in users" :key="email">{{ name }} ({{ email }})</div></template>"#,
        &[("users", BindingType::SetupConst)],
    );
    eprintln!("TSX output:\n{}", result);

    // NEGATIVE: destructured params must NOT get instance prefix
    assert!(
        !result.contains("___VERTER___instance.name"),
        "destructured v-for param 'name' must NOT get instance prefix, got: {}",
        result
    );
    assert!(
        !result.contains("___VERTER___instance.email"),
        "destructured v-for param 'email' must NOT get instance prefix, got: {}",
        result
    );
}

#[test]
fn v_for_numeric_range_valid_tsx() {
    // Bug: v-for="i in 12" generates 12.map(...) which is invalid JS
    let result =
        gen_tsx_template(r#"<template><i v-for="i in 12" :key="i" class="line" /></template>"#);
    eprintln!("TSX output:\n{}", result);

    // POSITIVE: should have a v-for frame
    assert!(
        result.contains("___VERTER___flowEach"),
        "should generate a v-for frame, got: {}",
        result
    );

    // NEGATIVE: must NOT call .map() directly on a numeric literal
    assert!(
        !result.contains("12.map("),
        "must not call .map() on numeric literal, got: {}",
        result
    );
    // Also check that we don't get 12 followed by .map without space
    assert!(
        !result.contains("12 .map("),
        "must not call .map() on numeric literal with space, got: {}",
        result
    );
}

#[test]
fn comment_between_v_if_v_else_valid_tsx() {
    // Bug: HTML comments between v-if/v-else become JSX comments that break if/else chain
    let result = gen_tsx_template(
        r#"<template><div v-if="a">A</div><!-- comment --><div v-else>B</div></template>"#,
    );
    eprintln!("TSX output:\n{}", result);

    // POSITIVE: should have both if and else branches
    assert!(
        result.contains("if("),
        "should have if condition, got: {}",
        result
    );
    assert!(
        result.contains("else"),
        "should have else branch, got: {}",
        result
    );

    // NEGATIVE: JSX comment must NOT appear between } and else
    // Valid: }else{  or }\nelse{
    // Invalid: }{/* comment */}\nelse{
    let cleaned = result.replace(char::is_whitespace, "");
    assert!(
        !cleaned.contains("}{/*"),
        "JSX comment must not appear between if-closing and else, got: {}",
        result
    );
}

#[test]
fn dynamic_component_inside_v_for_valid_tsx() {
    // Bug: <component :is> IS the v-for element — puts const statement in arrow expression
    // Real pattern from VirtualListItem.vue:
    // <component v-for="(c, index) in children" :key="index" :is="c" />
    let result = gen_tsx_template_with_bindings(
        r#"<template><component v-for="(c, index) in children" :key="index" :is="c" /></template>"#,
        &[("children", BindingType::SetupConst)],
    );
    eprintln!("TSX output:\n{}", result);

    // POSITIVE: should have component_render or extractRenderComponent
    assert!(
        result.contains("___VERTER___component_render")
            || result.contains("extractRenderComponent"),
        "should handle dynamic :is component, got: {}",
        result
    );

    // NEGATIVE: const statement must NOT appear inside .map(() => (...))
    // The arrow function with parens only allows expressions, not statements
    assert!(
        !result.contains("=> (const "),
        "const statement must not appear in arrow expression body, got: {}",
        result
    );
}

/// Regression: v-show + :style on the same element must not leak binding prefixes.
///
/// When `v-show="message"` and `:style="!!title ? undefined : { margin: 0 }"` are
/// both on the same element, the v-show handler merges both into a single `style` attribute.
/// But `process_v_bind` also processes `:style` and calls `collect_binding_patches` which
/// adds prepends at source positions of identifiers. These prepends survive the v-show
/// overwrite and leak as stray text (e.g., `___VERTER___instance.` after the style attribute).
#[test]
fn v_show_with_style_binding_no_leaked_prefix() {
    let source = r#"<template><div v-show="message" :style="!!title ? undefined : { margin: 0 }">hi</div></template>"#;
    let result = gen_tsx_template(source);
    eprintln!("TSX output:\n{}", result);

    // Positive: merged style should include both the v-show display logic and the existing style
    assert!(
        result.contains("display:"),
        "merged style should include v-show display logic. Got: {}",
        result
    );
    assert!(
        result.contains("title"),
        "merged style should include :style expression. Got: {}",
        result
    );

    // Negative: no stray binding prefixes leaked outside the style attribute
    let style_end = result
        .find("}}")
        .expect("should have closing }} for style object");
    let after_style = &result[style_end + 2..];
    assert!(
        !after_style.contains("___VERTER___instance."),
        "binding prefix must not leak after style attribute. After '}}': {:?}",
        after_style
    );

    // Parse the result with OXC to check for syntax errors
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::tsx();
    let wrapped = format!("import {{}} from 'vue';\n{}", result);
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc, &wrapped, source_type).parse();
    for err in &parsed.diagnostics {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.diagnostics.is_empty(),
        "Generated TSX should have no parse errors. Got {} errors. Output:\n{}",
        parsed.diagnostics.len(),
        result
    );
}

/// Regression: `<component :is="tag" v-if="cond" v-text="expr" />` must produce
/// valid TSX — the combination of dynamic :is IIFE + v-if + v-text was causing
/// syntax errors (TS1005: ';' expected).
#[test]
fn component_is_with_v_if_and_v_text_produces_valid_jsx() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><component :is="titleTag" v-if="!!title" v-text="title" /></template>"#,
        &[
            ("titleTag", BindingType::SetupConst),
            ("title", BindingType::SetupConst),
        ],
    );
    eprintln!(
        "=== COMPONENT :IS + V-IF + V-TEXT ===\n{}\n=== END ===",
        result
    );

    // Must contain v-text → textContent conversion
    assert!(
        result.contains("textContent"),
        "v-text should generate textContent prop"
    );

    // Must not have raw v-text in output
    assert!(
        !result.contains("v-text"),
        "v-text directive must be removed from JSX"
    );

    // Parse with OXC to verify valid TSX
    let alloc = Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &result, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.diagnostics {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.diagnostics.is_empty(),
        "Generated TSX should have no parse errors. Got {} errors. Output:\n{}",
        parsed.diagnostics.len(),
        result
    );
}

#[test]
fn component_is_v_text_options_api_full_sfc() {
    let source = r#"<template>
  <div>
    <component :is="titleTag" v-if="!!title" v-text="title" />
  </div>
</template>

<script lang="ts">
export default defineComponent({
  props: {
    title: { type: String, default: '' },
    titleTag: { type: String, default: 'h4' },
  },
  setup(props) {
    return {};
  },
});
</script>"#;
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some("BalCard.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = crate::compile::legacy_test_support::VerterCompileOptions::default();
    let result = crate::compile::legacy_test_support::compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== FULL SFC TSX ===\n{}\n=== END ===", tsx.code);

    // Parse with OXC to verify valid TSX
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    for err in &parsed.diagnostics {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.diagnostics.is_empty(),
        "Full SFC TSX should have no parse errors. Got {} errors. Output:\n{}",
        parsed.diagnostics.len(),
        tsx.code
    );
}

// ── Custom directive type checking ─────────────────────────

#[test]
fn custom_directive_basic_no_args() {
    let result = gen_tsx_template(r#"<template><div v-focus /></template>"#);
    eprintln!("custom_directive_basic_no_args:\n{result}");

    // Positive: should emit v-directive callback with vFocus
    assert!(
        result.contains("v-directive="),
        "should emit v-directive prop: {result}"
    );
    assert!(
        result.contains(r#"directiveAccessor["vFocus"]"#),
        "should reference vFocus from accessor: {result}"
    );
    assert!(
        result.contains("true,undefined,{}"),
        "no-value directive should use true,undefined,{{}}: {result}"
    );

    // Negative: v-focus should NOT appear as raw attribute
    assert!(
        !result.contains("v-focus"),
        "v-focus raw attribute must be removed: {result}"
    );
}

#[test]
fn custom_directive_with_value() {
    let result = gen_tsx_template(r#"<template><div v-test="val" /></template>"#);
    eprintln!("custom_directive_with_value:\n{result}");

    assert!(
        result.contains(r#"directiveAccessor["vTest"]"#),
        "should reference vTest: {result}"
    );
    // Value should be the expression "val"
    assert!(
        result.contains("val,undefined,{}"),
        "should have val as value expression: {result}"
    );
}

#[test]
fn custom_directive_static_arg() {
    let result = gen_tsx_template(r#"<template><div v-test:foo="val" /></template>"#);
    eprintln!("custom_directive_static_arg:\n{result}");

    assert!(
        result.contains(r#"directiveAccessor["vTest"]"#),
        "should reference vTest: {result}"
    );
    assert!(
        result.contains(r#"val,"foo","#),
        "should have static arg 'foo' (quoted): {result}"
    );
}

#[test]
fn custom_directive_dynamic_arg() {
    let result = gen_tsx_template(r#"<template><div v-test:[dyn]="val" /></template>"#);
    eprintln!("custom_directive_dynamic_arg:\n{result}");

    assert!(
        result.contains(r#"directiveAccessor["vTest"]"#),
        "should reference vTest: {result}"
    );
    // Dynamic arg: dyn resolved as expression (no quotes)
    assert!(
        result.contains("instance.dyn,"),
        "dynamic arg should be resolved unquoted expression: {result}"
    );
}

#[test]
fn custom_directive_modifiers() {
    let result = gen_tsx_template(r#"<template><div v-test.bar.baz="val" /></template>"#);
    eprintln!("custom_directive_modifiers:\n{result}");

    assert!(
        result.contains(r#"directiveAccessor["vTest"]"#),
        "should reference vTest: {result}"
    );
    // Identifier-shaped modifiers are BARE keys: TypeScript anchors the
    // excess-property diagnostic at the property-name node, and a quoted key would
    // start at the synthetic quote, so the invalid-modifier squiggle would have no
    // carrier position at all.
    assert!(
        result.contains("bar:true"),
        "should have bar modifier as a bare key: {result}"
    );
    assert!(
        result.contains("baz:true"),
        "should have baz modifier as a bare key: {result}"
    );
    assert!(
        !result.contains(r#""bar":true"#),
        "an identifier-shaped modifier must not be emitted as a quoted key: {result}"
    );
}

#[test]
fn custom_directive_multiple() {
    let result = gen_tsx_template(r#"<template><div v-a v-b="x" /></template>"#);
    eprintln!("custom_directive_multiple:\n{result}");

    // Should have single v-directive= with both calls
    assert!(
        result.contains(r#"directiveAccessor["vA"]"#),
        "should reference vA: {result}"
    );
    assert!(
        result.contains(r#"directiveAccessor["vB"]"#),
        "should reference vB: {result}"
    );
    // Only one v-directive= prop
    assert_eq!(
        result.matches("v-directive=").count(),
        1,
        "should have exactly one v-directive prop: {result}"
    );
}

#[test]
fn custom_directive_hyphenated_name() {
    let result = gen_tsx_template(r#"<template><div v-click-outside="fn" /></template>"#);
    eprintln!("custom_directive_hyphenated_name:\n{result}");

    assert!(
        result.contains(r#"directiveAccessor["vClickOutside"]"#),
        "should camelCase hyphenated name: {result}"
    );

    // Negative: raw attribute must not appear
    assert!(
        !result.contains("v-click-outside"),
        "raw v-click-outside must be removed: {result}"
    );
}

#[test]
fn custom_directive_builtins_not_captured() {
    let result = gen_tsx_template(r#"<template><div v-show="x" /></template>"#);
    eprintln!("custom_directive_builtins_not_captured:\n{result}");

    // v-show is a built-in — should NOT produce v-directive
    assert!(
        !result.contains("v-directive="),
        "built-in v-show should NOT produce v-directive: {result}"
    );
}

#[test]
fn custom_directive_jsx_mode_skips() {
    let result = gen_jsx_template(r#"<template><div v-focus /></template>"#);
    eprintln!("custom_directive_jsx_mode_skips:\n{result}");

    // JSX mode should NOT emit v-directive (TS-only feature)
    assert!(
        !result.contains("v-directive="),
        "JSX mode should not emit v-directive: {result}"
    );
}

#[test]
fn custom_directive_full_combo() {
    // v-test:foo.bar="baz" — value + static arg + modifier
    let result = gen_tsx_template(r#"<template><div v-test:foo.bar="baz" /></template>"#);
    eprintln!("custom_directive_full_combo:\n{result}");

    assert!(
        result.contains(r#"directiveAccessor["vTest"]"#),
        "should reference vTest: {result}"
    );
    assert!(
        result.contains(r#"baz,"foo",{bar:true}"#),
        "should have value, static arg, and modifier object: {result}"
    );

    // Negative: raw directive must not appear
    assert!(
        !result.contains("v-test:foo"),
        "raw v-test:foo must be removed: {result}"
    );
}

#[test]
fn custom_directive_modifier_key_maps_to_authored_modifier() {
    // The invalid-modifier diagnostic TypeScript raises on the generated modifier
    // key must be able to land on the authored `green` token.
    let source = r#"<template><div v-color.green="'red'" /></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("vColor", BindingType::SetupConst)]);

    let green_src = source.find("green").unwrap();
    assert_mapped_run(&output, &tokens, "green", green_src, "modifier key");
}

#[test]
fn custom_directive_each_modifier_maps_to_its_own_authored_span() {
    // MIXED valid + invalid modifiers: each generated key owns its OWN authored
    // span. A single collapsed run would make the invalid-modifier diagnostic land
    // on the wrong modifier — the failure mode a `contains()` check cannot see.
    let source = r#"<template><div v-color.blue.green="'red'" /></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("vColor", BindingType::SetupConst)]);

    let blue_src = source.find(".blue").unwrap() + 1;
    let green_src = source.find(".green").unwrap() + 1;
    assert_ne!(blue_src, green_src);

    assert_mapped_run(&output, &tokens, "blue", blue_src, "first modifier");
    assert_mapped_run(&output, &tokens, "green", green_src, "second modifier");
}

/// A NON-IDENTIFIER modifier must stay a quoted key to be legal JavaScript, and
/// TypeScript anchors the excess-property diagnostic (`TS2353`) at the WHOLE
/// string-literal key — the opening quote through the closing quote.
///
/// Synthetic, unmapped quotes therefore drop the invalid-modifier diagnostic
/// entirely: its range both starts and ends outside any mapped run. Each quote
/// owns the authored delimiter it stands for (`.` before the modifier, and the
/// token that terminates it), so the quoted key composes ONE contiguous carrier
/// range covering the authored `.some-mod`.
#[test]
fn custom_directive_quoted_modifier_key_maps_including_its_quotes() {
    let source = r#"<template><div v-color.some-mod="'red'" /></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("vColor", BindingType::SetupConst)]);

    let dot_src = source.find(".some-mod").expect("fixture modifier");
    let name_src = dot_src + 1;
    let after_name_src = name_src + "some-mod".len();

    // The OPENING quote — where the TS2353 range STARTS — owns the authored `.`.
    assert_mapped_run(
        &output,
        &tokens,
        r#""some-mod""#,
        dot_src,
        "quoted modifier key opening quote",
    );

    let key_gen = output.find(r#""some-mod""#).expect("quoted modifier key");
    // The modifier name keeps its own run (hover / definition on the modifier).
    assert_token_at(
        &output,
        &tokens,
        key_gen + 1,
        name_src,
        "quoted modifier key name",
    );
    // The CLOSING quote — where the TS2353 range ENDS — owns the authored token
    // that terminates the modifier, so the range's exclusive end resolves.
    assert_token_at(
        &output,
        &tokens,
        key_gen + 1 + "some-mod".len(),
        after_name_src,
        "quoted modifier key closing quote",
    );
}

#[test]
fn custom_directive_reference_maps_to_authored_directive_name() {
    // Template `v-color` ↔ script `vColor`: the generated reference must map back
    // to the authored directive name so hover / go-to-definition / find-references
    // bridge the kebab↔camel hop.
    let source = r#"<template><div v-color="'red'" /></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("vColor", BindingType::SetupConst)]);

    let name_src = source.find("v-color").unwrap();
    assert_mapped_run(&output, &tokens, "vColor", name_src, "directive reference");

    // …and the run's EXTENT, not just its first anchor: the anchor assertion above
    // stays green with a six-column run over the seven-column authored `v-color`,
    // which is how the truncated extent shipped.
    let (output, raw) =
        gen_tsx_template_with_raw_tokens(source, &[("vColor", BindingType::SetupConst)]);
    let map = RunMap::build(source, &raw);
    assert_directive_name_projection(
        source,
        &output,
        &map,
        "v-color",
        "vColor",
        "resolved directive reference",
    );
}

#[test]
fn custom_directive_accessor_reference_maps_to_authored_directive_name() {
    // An UNRESOLVED directive resolves through the instance accessor; the mapped
    // token is the directive name inside the string index.
    let source = r#"<template><div v-focus /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    let name_src = source.find("v-focus").unwrap();
    assert_mapped_run(&output, &tokens, "vFocus", name_src, "accessor reference");

    // …and the run's EXTENT. The accessor form is the same projection inside a string
    // index, so it truncates identically and the anchor assertion above cannot see it.
    let (output, raw) = gen_tsx_template_with_raw_tokens(source, &[]);
    let map = RunMap::build(source, &raw);
    assert_directive_name_projection(
        source,
        &output,
        &map,
        "v-focus",
        "vFocus",
        "accessor directive reference",
    );
}

/// The authored `v-color` is SEVEN characters and the generated `vColor` is SIX. One
/// linear mapped run anchored at the authored `v` therefore covers six authored
/// columns — `v-colo` — so the trailing `r` maps to nothing, and worse, the five
/// columns it does cover are shifted by the deleted hyphen and resolve onto the WRONG
/// authored character.
///
/// Consequences of the shipped shape: hover / references with the cursor on the final
/// `r` fail; every returned range omits that character; and a rename derived from the
/// generated `vColor` range replaces `v-colo` and leaves a dangling `r` — a corrupting
/// partial edit.
#[test]
fn custom_directive_name_maps_every_authored_column_including_the_last() {
    let source = r#"<template><div v-color="'red'" /></template>"#;
    let (output, raw) =
        gen_tsx_template_with_raw_tokens(source, &[("vColor", BindingType::SetupConst)]);
    let map = RunMap::build(source, &raw);

    let name_src = source.find("v-color").expect("fixture directive name");
    let name_gen = output
        .find("vColor")
        .expect("relocated directive reference");

    // The defect, stated on its own: the LAST authored character must participate.
    assert_eq!(
        authored_byte_to_generated_byte(source, &output, &map, name_src + "v-colo".len()),
        Some(name_gen + "vColo".len()),
        "the final authored `r` of `v-color` must map to the generated `r` of `vColor`; \
         a single linear run stops one column short of it. Runs: {:?}\n{output}",
        map.runs,
    );

    assert_directive_name_projection(
        source,
        &output,
        &map,
        "v-color",
        "vColor",
        "single-hyphen directive name",
    );
}

/// MULTI-hyphen: `v-click-outside` (15) → `vClickOutside` (13). The deficit is TWO,
/// so a single run drops `de` and shifts by a different amount than the single-hyphen
/// case — proving the defect is the mechanism, not one input's arithmetic.
#[test]
fn custom_directive_multi_hyphen_name_maps_every_authored_column() {
    let source = r#"<template><div v-click-outside="'x'" /></template>"#;
    let (output, raw) =
        gen_tsx_template_with_raw_tokens(source, &[("vClickOutside", BindingType::SetupConst)]);
    let map = RunMap::build(source, &raw);

    assert_directive_name_projection(
        source,
        &output,
        &map,
        "v-click-outside",
        "vClickOutside",
        "multi-hyphen directive name",
    );
}

/// SINGLE-CHARACTER segment: `v-a` (3) → `vA` (2). The whole authored name is the
/// prefix, one hyphen and one letter, so the generated identifier is two columns and
/// a single run covers `v-` only — the sole letter of the directive maps to nothing.
#[test]
fn custom_directive_single_char_name_maps_its_only_letter() {
    let source = r#"<template><div v-a="'x'" /></template>"#;
    let (output, raw) =
        gen_tsx_template_with_raw_tokens(source, &[("vA", BindingType::SetupConst)]);
    let map = RunMap::build(source, &raw);

    assert_directive_name_projection(source, &output, &map, "v-a", "vA", "single-char directive");
}

/// A directive name carrying an ARGUMENT and MODIFIERS: the name projection is
/// independent of them, and the name's mapped runs must not bleed into the argument
/// or modifier spans that follow (each of those owns its own runs).
#[test]
fn custom_directive_name_with_arg_and_modifiers_maps_only_the_name() {
    let source = r#"<template><div v-click-outside:foo.bar="'x'" /></template>"#;
    let (output, raw) =
        gen_tsx_template_with_raw_tokens(source, &[("vClickOutside", BindingType::SetupConst)]);
    let map = RunMap::build(source, &raw);

    assert_directive_name_projection(
        source,
        &output,
        &map,
        "v-click-outside",
        "vClickOutside",
        "directive name with arg and modifiers",
    );

    // The authored `:` that terminates the name is NOT part of the name projection: it
    // is owned by the argument's opening-quote run, which maps it deliberately.
    let colon_src = source.find(":foo").expect("fixture argument");
    let quote_gen = output.find(r#""foo""#).expect("quoted static argument");
    assert_eq!(
        authored_byte_to_generated_byte(source, &output, &map, colon_src),
        Some(quote_gen),
        "the authored `:` belongs to the argument's opening quote, not to the name run. \
         Runs: {:?}\n{output}",
        map.runs,
    );
}

/// The corrupting-rename guard, stated as its own test so it cannot be lost in a
/// helper refactor: a provider edit derived from the generated identifier's range must
/// never splice a PARTIAL authored token, and the individual authored columns must
/// still map (so "unmap the whole name" is not a passing answer).
#[test]
fn custom_directive_name_range_never_composes_a_partial_authored_token() {
    let source = r#"<template><div v-color="'red'" /></template>"#;
    let (output, raw) =
        gen_tsx_template_with_raw_tokens(source, &[("vColor", BindingType::SetupConst)]);
    let map = RunMap::build(source, &raw);

    let name_src = source.find("v-color").expect("fixture directive name");
    let name_gen = output
        .find("vColor")
        .expect("relocated directive reference");
    let (gen_line, gen_col) = gen_line_col(&output, name_gen);

    // Both endpoints of the generated identifier resolve individually…
    assert!(
        map.to_source(gen_line, gen_col).is_some(),
        "the identifier's first generated column must map. Runs: {:?}",
        map.runs,
    );
    assert!(
        map.to_source(gen_line, gen_col + "vColor".len() as u32 - 1)
            .is_some(),
        "the identifier's last generated column must map — unmapping the name is not the \
         fix. Runs: {:?}",
        map.runs,
    );

    // …but they must not COMPOSE, because 6 generated columns cannot span the 7
    // authored ones and a partial splice corrupts the source.
    let composed = map.range_to_source(gen_line, gen_col, gen_col + "vColor".len() as u32);
    assert_eq!(
        composed,
        None,
        "a range over the generated `vColor` must fail CLOSED. It composed {composed:?}; the \
         authored token is [{name_src}, {}). Splicing a rename's `vHighlight` over a partial \
         `v-colo` leaves a dangling `r`. Runs: {:?}\n{output}",
        name_src + "v-color".len(),
        map.runs,
    );
}

#[test]
fn custom_directive_value_identifier_maps_to_authored_expression() {
    let source = r#"<template><div v-color="msg" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("vColor", BindingType::SetupConst),
            ("msg", BindingType::SetupRef),
        ],
    );

    let msg_src = source.find("\"msg\"").unwrap() + 1;
    assert_mapped_run(&output, &tokens, "msg", msg_src, "value identifier");
}

#[test]
fn custom_directive_static_arg_maps_to_authored_arg() {
    let source = r#"<template><div v-color:foo="'red'" /></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("vColor", BindingType::SetupConst)]);

    let arg_src = source.find(":foo").unwrap() + 1;
    assert_mapped_run(&output, &tokens, "foo", arg_src, "static argument");
}

/// A STATIC directive argument is emitted as a QUOTED string literal
/// (`…,"theArg",{…}`), and TypeScript anchors an argument-type diagnostic
/// (`TS2345`) on the WHOLE string literal — the opening quote through the closing
/// quote.
///
/// Synthetic, unmapped quotes therefore drop the invalid-argument diagnostic
/// entirely: its range both starts and ends outside any mapped run, so a strict
/// range mapper (which composes a carrier range only from runs contiguous in BOTH
/// spaces) rejects it and the user sees no squiggle on a real type error. Each
/// quote owns the authored delimiter it stands for — the `:` that introduces the
/// argument and the token that terminates it — so the quoted argument composes ONE
/// contiguous carrier range covering the authored `:theArg`.
///
/// The sibling `custom_directive_static_arg_maps_to_authored_arg` asserts only the
/// NAME anchor, so it stays green while the diagnostic is still being dropped —
/// which is exactly how this defect survived the first mapping pass.
#[test]
fn custom_directive_static_arg_maps_including_its_quotes() {
    let source = r#"<template><div v-color:theArg="'red'" /></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("vColor", BindingType::SetupConst)]);

    let colon_src = source.find(":theArg").expect("fixture argument");
    let name_src = colon_src + 1;
    let after_name_src = name_src + "theArg".len();

    // The OPENING quote — where the TS2345 range STARTS — owns the authored `:`.
    assert_mapped_run(
        &output,
        &tokens,
        r#""theArg""#,
        colon_src,
        "static argument opening quote",
    );

    let arg_gen = output
        .find(r#""theArg""#)
        .expect("quoted static argument in the relocated payload");
    // The argument name keeps its own run (hover / definition on the argument).
    assert_token_at(
        &output,
        &tokens,
        arg_gen + 1,
        name_src,
        "static argument name",
    );
    // The CLOSING quote — where the TS2345 range ENDS — owns the authored token
    // that terminates the argument, so the range's exclusive end resolves.
    assert_token_at(
        &output,
        &tokens,
        arg_gen + 1 + "theArg".len(),
        after_name_src,
        "static argument closing quote",
    );
}

#[test]
fn custom_directive_dynamic_arg_identifier_maps_to_authored_expression() {
    let source = r#"<template><div v-color:[dyn]="'red'" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("vColor", BindingType::SetupConst),
            ("dyn", BindingType::SetupRef),
        ],
    );

    let dyn_src = source.find("[dyn]").unwrap() + 1;
    assert_mapped_run(&output, &tokens, "dyn", dyn_src, "dynamic argument");
}

#[test]
fn custom_directive_callback_parameter_is_explicitly_typed() {
    // The synthetic `v-directive` callback parameter has no contextual type, so an
    // unannotated parameter raises TS7006 (`implicitly has an 'any' type`) under
    // `noImplicitAny` on EVERY custom directive — including correct ones. The
    // parameter must carry an explicit annotation.
    let result = gen_tsx_template(r#"<template><div v-focus /></template>"#);

    assert!(
        result.contains("(___VERTER___slotInstance: any)"),
        "the v-directive callback parameter must be explicitly annotated so it does \
         not raise TS7006 under noImplicitAny: {result}"
    );
    assert!(
        !result.contains("(___VERTER___slotInstance)"),
        "no unannotated v-directive callback parameter may remain: {result}"
    );
}

// ── Script preamble: directive accessor ────────────────────

#[test]
fn script_preamble_directive_accessor() {
    let source = r#"<script setup lang="ts">
const x = 1
</script>
<template><div v-focus /></template>"#;
    let code = compile_full_sfc_tsx(source, "Test.vue");
    eprintln!("script_preamble_directive_accessor:\n{code}");

    assert!(
        code.contains("___VERTER___directiveAccessor"),
        "should emit directiveAccessor declaration: {code}"
    );
    assert!(
        code.contains("retrieveSetupDirectives"),
        "should import retrieveSetupDirectives: {code}"
    );
    assert!(
        code.contains("runCustomDirective"),
        "should import runCustomDirective: {code}"
    );
    assert!(
        code.contains("ExtractLeafElement"),
        "should import ExtractLeafElement type: {code}"
    );
}

#[test]
fn script_setup_local_directive_uses_the_local_binding() {
    let source = r#"<script setup lang="ts">
import type { Directive } from 'vue'
const vColor: Directive<HTMLElement, string> = () => {}
</script>
<template><div v-color="'red'" /></template>"#;
    let code = compile_full_sfc_tsx(source, "Test.vue");
    assert!(
        code.contains(",vColor)(___VERTER___directiveElement"),
        "a setup-local directive must retain its authored binding type: {code}"
    );
    assert!(
        !code.contains("directiveAccessor[\"vColor\"]"),
        "a setup-local directive must not be looked up on the component instance: {code}"
    );
}

#[test]
fn script_preamble_directive_accessor_valid_tsx() {
    let source = r#"<script setup lang="ts">
const x = 1
</script>
<template><div v-focus v-test:foo.bar="baz" /></template>"#;
    let code = compile_full_sfc_tsx(source, "Test.vue");
    eprintln!("script_preamble_directive_accessor_valid_tsx:\n{code}");

    // The output should be valid TSX
    assert_valid_tsx(&code, "directive-accessor-preamble");
}

#[test]
fn ts_expect_error_before_v_for() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><div v-for="x in xs">{{ x }}</div></template>"#,
        &[("xs", BindingType::SetupRef)],
    );
    // v-for opens a frame — the comment must be INSIDE the frame body
    assert!(
        result.contains("___VERTER___flowEach"),
        "should have a v-for frame, got:\n{}",
        result
    );
    let map_pos = result.find("___VERTER___flowEach").unwrap();
    // Comment must be present (as JSX comment with TS directive)
    assert!(
        result.contains("@ts-expect-error"),
        "TS directive comment should be present, got:\n{}",
        result
    );
    let comment_pos = result.find("@ts-expect-error").unwrap();
    assert!(
        comment_pos > map_pos,
        "comment should be inside the v-for frame, not before it, got:\n{}",
        result
    );
    // No raw HTML comment markers
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

#[test]
fn ts_expect_error_v_if_component_is() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><component :is="c" v-if="ok"/></template>"#,
        &[("c", BindingType::SetupRef), ("ok", BindingType::SetupRef)],
    );
    // v-if wraps in IIFE, <component :is> creates nested IIFE
    // The comment should end up inside the component :is IIFE (before `return`)
    assert!(
        result.contains("extractRenderComponent"),
        "should have extractRenderComponent IIFE, got:\n{}",
        result
    );
    // Comment should be somewhere in the output
    assert!(
        result.contains("@ts-expect-error"),
        "TS directive comment should be present, got:\n{}",
        result
    );
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

#[test]
fn ts_expect_error_v_for_v_if() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><div v-for="x in xs" v-if="ok">{{ x }}</div></template>"#,
        &[("xs", BindingType::SetupRef), ("ok", BindingType::SetupRef)],
    );
    // v-for + v-if: the lifted condition is outside, the v-for frame inside
    assert!(
        result.contains("___VERTER___flowEach"),
        "should have a v-for frame, got:\n{}",
        result
    );
    let map_pos = result.find("___VERTER___flowEach").unwrap();
    assert!(
        result.contains("@ts-expect-error"),
        "TS directive comment should be present, got:\n{}",
        result
    );
    let comment_pos = result.find("@ts-expect-error").unwrap();
    assert!(
        comment_pos > map_pos,
        "comment should be inside the v-for frame, got:\n{}",
        result
    );
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

#[test]
fn regular_comment_not_repositioned_for_v_for() {
    let result = gen_tsx_template(
        r#"<template><!-- hello --><div v-for="x in xs">{{ x }}</div></template>"#,
    );
    // Regular (non-TS-directive) comment should NOT be repositioned inside the frame
    assert!(
        result.contains("{/* hello */}"),
        "regular comment should be converted to JSX, got:\n{}",
        result
    );
    // Comment should stay at its original position (before the frame)
    let comment_pos = result.find("{/* hello */}").unwrap();
    let map_pos = result.find("___VERTER___flowEach").unwrap();
    assert!(
        comment_pos < map_pos,
        "regular comment should stay before the v-for frame, not be repositioned inside, got:\n{}",
        result
    );
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

#[test]
fn existing_v_if_comment_repositioning_not_regressed() {
    // The existing v-if comment repositioning should still work
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><div v-if="show">hello</div></template>"#,
        &[("show", BindingType::SetupRef)],
    );
    assert!(
        result.contains("if(show)"),
        "should have IIFE condition, got:\n{}",
        result
    );
    let iife_pos = result.find("{(()=>{").expect("should have IIFE open");
    let comment_pos = result
        .find("{/* @ts-expect-error */}")
        .expect("comment should be preserved");
    assert!(
        comment_pos > iife_pos,
        "comment should appear AFTER IIFE open (inside), got:\n{}",
        result
    );
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

// ── v-if/v-else + v-for lifted chain tests ───────────────────────

#[test]
fn v_if_v_for_followed_by_v_else_v_for() {
    // The primary bug case: sibling elements with v-if+v-for and v-else+v-for
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in items" :key="item.id">{{ item.name }}</div><div v-else v-for="item in others" :key="item.id">{{ item.label }}</div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_followed_by_v_else_v_for ===\n{}\n=== END ===",
        result
    );
    // Positive: should have lifted ternary with condition outside map
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "should have lifted condition outside: {result}"
    );
    // Positive: both branches should have a v-for frame
    let map_count = result.matches("___VERTER___flowEach").count();
    assert!(
        map_count >= 2,
        "should have two v-for frames (one per branch), found {map_count}: {result}"
    );
    // Negative: should NOT have bare `else` keyword (IIFE style)
    assert!(
        !result.contains("else{") && !result.contains("else {"),
        "should NOT use IIFE else (should be lifted ternary): {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="item in items" :key="item.id">{{ item.name }}</div><div v-else v-for="item in others" :key="item.id">{{ item.label }}</div></template>"#,
        "v-if+v-for followed by v-else+v-for",
    );
}

#[test]
fn v_if_v_for_chain_three_branches() {
    let result = gen_tsx_template(
        r#"<template><div v-if="mode === 'a'" v-for="item in listA">{{ item }}</div><div v-else-if="mode === 'b'" v-for="item in listB">{{ item }}</div><div v-else v-for="item in listC">{{ item }}</div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_chain_three_branches ===\n{}\n=== END ===",
        result
    );
    // Should have 3 v-for frames
    let map_count = result.matches("___VERTER___flowEach").count();
    assert!(
        map_count >= 3,
        "should have three v-for frames, found {map_count}: {result}"
    );
    // Should have ternary structure, not IIFE
    assert!(
        !result.contains("else{") && !result.contains("else {"),
        "should NOT use IIFE: {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div v-if="mode === 'a'" v-for="item in listA">{{ item }}</div><div v-else-if="mode === 'b'" v-for="item in listB">{{ item }}</div><div v-else v-for="item in listC">{{ item }}</div></template>"#,
        "three-branch v-if/v-else-if/v-else + v-for chain",
    );
}

#[test]
fn v_if_v_for_mixed_chain_some_with_for_some_without() {
    // v-if+v-for followed by plain v-else (no v-for)
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in items">{{ item }}</div><span v-else>fallback</span></template>"#,
    );
    eprintln!("=== v_if_v_for_mixed_chain ===\n{}\n=== END ===", result);
    // Lifted ternary: condition outside
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "should have lifted condition: {result}"
    );
    // First branch has a v-for frame, second doesn't
    assert!(
        result.contains("___VERTER___flowEach"),
        "first branch should have a v-for frame: {result}"
    );
    assert!(
        result.contains("<span"),
        "second branch should have plain <span>: {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="item in items">{{ item }}</div><span v-else>fallback</span></template>"#,
        "mixed chain: v-if+v-for then plain v-else",
    );
}

#[test]
fn v_if_v_for_solo_lifts_condition() {
    // Solo v-if + v-for: condition should be lifted outside the frame
    // Vue 3 precedence: v-if has higher precedence, runs before v-for
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in list">{{ item }}</div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_solo_lifts_condition ===\n{}\n=== END ===",
        result
    );
    // Should have lifted ternary: `show ? (() => { … })() : null`
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "should have lifted condition outside map: {result}"
    );
    assert!(
        result.contains(": null"),
        "solo lifted should have : null fallback: {result}"
    );
    assert!(
        result.contains("___VERTER___flowEach"),
        "should have a v-for frame: {result}"
    );
}

#[test]
fn v_if_v_for_iife_chain_regression() {
    // Standard v-if/v-else chain WITHOUT any v-for should still use IIFE
    let result =
        gen_tsx_template(r#"<template><div v-if="show">A</div><div v-else>B</div></template>"#);
    // Should use IIFE (if/else), NOT ternary
    assert!(
        result.contains("if(") || result.contains("if ("),
        "no-v-for chain should use IIFE with if(): {result}"
    );
    assert!(
        result.contains("else"),
        "no-v-for chain should have else: {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div v-if="show">A</div><div v-else>B</div></template>"#,
        "IIFE chain regression (no v-for)",
    );
}

#[test]
fn v_if_v_for_statement_body() {
    // A v-for frame is an immediately invoked statement body:
    // `(() => { const <aliases> = …; return (...); })()`
    let result =
        gen_tsx_template(r#"<template><div v-for="item in items">{{ item }}</div></template>"#);
    eprintln!("=== v_if_v_for_statement_body ===\n{}\n=== END ===", result);
    // Should have statement body with return
    assert!(
        result.contains(
            "{(() => { const ___VERTER___v0 = (___VERTER___instance.items); { const item"
        ) && result.contains("); return ("),
        "v-for should use an invoked statement body, got: {result}"
    );
    // Negative: should NOT have expression body `=> (`
    assert!(
        !result.contains("=> ("),
        "v-for should NOT use expression body `=> (`, got: {result}"
    );
}

#[test]
fn v_if_v_for_numeric_in_lifted_chain() {
    // Numeric v-for in a lifted chain is a bare frame (no leading `{`)
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="n in 5">{{ n }}</div><div v-else>none</div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_numeric_in_lifted_chain ===\n{}\n=== END ===",
        result
    );
    assert!(
        result.contains("const ___VERTER___v0 = (5);"),
        "numeric v-for should be the frame source: {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="n in 5">{{ n }}</div><div v-else>none</div></template>"#,
        "numeric v-for in lifted chain",
    );
}

#[test]
fn v_if_v_for_adjacent_chains_independent() {
    // Two separate chains with a <p> separator
    let result = gen_tsx_template(
        "<template><div v-if=\"a\" v-for=\"x in xs\">{{ x }}</div><div v-else>no A</div><p>separator</p><div v-if=\"b\" v-for=\"y in ys\">{{ y }}</div><div v-else>no B</div></template>",
    );
    eprintln!(
        "=== v_if_v_for_adjacent_chains_independent ===\n{}\n=== END ===",
        result
    );
    // Should have 2 separate lifted ternaries
    let map_count = result.matches("___VERTER___flowEach").count();
    assert!(
        map_count >= 2,
        "should have at least two v-for frames: {result}"
    );
    assert!(
        result.contains("<p"),
        "separator <p> should be preserved: {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        "<template><div v-if=\"a\" v-for=\"x in xs\">{{ x }}</div><div v-else>no A</div><p>separator</p><div v-if=\"b\" v-for=\"y in ys\">{{ y }}</div><div v-else>no B</div></template>",
        "two independent chains",
    );
}

#[test]
fn v_if_v_for_inside_nested_element() {
    // Chain inside a parent div (ElementContent chains, not root)
    let result = gen_tsx_template(
        r#"<template><div><span v-if="show" v-for="item in items">{{ item }}</span><span v-else>none</span></div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_inside_nested_element ===\n{}\n=== END ===",
        result
    );
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "nested chain should be lifted: {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div><span v-if="show" v-for="item in items">{{ item }}</span><span v-else>none</span></div></template>"#,
        "chain inside nested element",
    );
}

#[test]
fn v_if_v_for_with_comments_between_branches() {
    // Comments between chain members should be suppressed
    let result = gen_tsx_template(
        "<template><div v-if=\"show\" v-for=\"item in items\">{{ item }}</div><!-- separator comment --><div v-else v-for=\"item in others\">{{ item }}</div></template>",
    );
    eprintln!(
        "=== v_if_v_for_with_comments_between_branches ===\n{}\n=== END ===",
        result
    );
    // Must be valid TSX (comments between ternary branches would break)
    assert_valid_jsx(
        "<template><div v-if=\"show\" v-for=\"item in items\">{{ item }}</div><!-- separator comment --><div v-else v-for=\"item in others\">{{ item }}</div></template>",
        "comments between v-if+v-for branches",
    );
}

#[test]
fn v_if_v_for_with_entity_whitespace_between_branches() {
    // Entity-backed whitespace should be treated like ignorable formatting
    // whitespace for v-if / v-else adjacency.
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in items">{{ item }}</div>&nbsp;<div v-else>fallback</div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_with_entity_whitespace_between_branches ===\n{}\n=== END ===",
        result
    );
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "entity-backed whitespace should not break the lifted chain: {result}"
    );
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="item in items">{{ item }}</div>&nbsp;<div v-else>fallback</div></template>"#,
        "entity whitespace between v-if+v-for branches",
    );
}

#[test]
fn v_if_v_for_v_else_component_is_plain_branch() {
    // Lifted ternary where v-else is a dynamic component
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in items">{{ item }}</div><component v-else :is="fallbackComp"/></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_v_else_component_is ===\n{}\n=== END ===",
        result
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="item in items">{{ item }}</div><component v-else :is="fallbackComp"/></template>"#,
        "v-if+v-for then component :is v-else",
    );
}

#[test]
fn v_html_identifier_maps_to_source() {
    // <div v-html="msg"/> → innerHTML={msg}. `msg` maps back; innerHTML=/{/} → None.
    let source = r#"<template><div v-html="msg"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("msg", BindingType::SetupConst)]);

    assert!(
        output.contains("innerHTML={msg}"),
        "v-html should emit innerHTML={{msg}}: {output}"
    );
    assert!(
        !output.contains("v-html"),
        "v-html directive must be removed: {output}"
    );

    // Positive: `msg` maps to its source byte offset.
    let msg_src = source.find("\"msg\"").unwrap() as u32 + 1; // inside quotes
    assert!(
        has_token_for_src(&tokens, msg_src),
        "msg must map to source col {msg_src}. Tokens: {tokens:?}"
    );

    // Negative: the start of `innerHTML=` carries no source mapping.
    let innerhtml_gen = output.find("innerHTML=").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, innerhtml_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "innerHTML= start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}, output: {output}"
    );
    // The `{` immediately before `msg` and the `}` after must also be unmapped.
    let brace_open = output.find("innerHTML={").unwrap() + "innerHTML=".len();
    let (ol, oc) = gen_offset_to_line_col(&output, brace_open);
    assert!(
        !has_token_at_gen(&tokens, ol, oc),
        "innerHTML opening brace (gen {ol}:{oc}) must map to None. Tokens: {tokens:?}"
    );
}

#[test]
fn v_text_identifier_maps_to_source() {
    // <div v-text="content"/> → textContent={content}.
    let source = r#"<template><div v-text="content"/></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("content", BindingType::SetupConst)]);

    assert!(
        output.contains("textContent={content}"),
        "v-text should emit textContent={{content}}: {output}"
    );
    assert!(
        !output.contains("v-text"),
        "v-text directive must be removed: {output}"
    );

    let content_src = source.find("\"content\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, content_src),
        "content must map to source col {content_src}. Tokens: {tokens:?}"
    );

    let textcontent_gen = output.find("textContent=").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, textcontent_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "textContent= start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );
}

#[test]
fn v_if_guarded_value_binding_maps_to_source() {
    // <div v-if="ok" :onSomething="() => handle()"/>: a function-typed value prop
    // under a condition. The re-narrowing guard and the block it turns the
    // expression body into are spliced around the authored body, which stays in
    // place (each identifier mapped); only the guard text is unmapped.
    let source = r#"<template><div v-if="ok" :onSomething="() => handle()"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("ok", BindingType::SetupConst),
            ("handle", BindingType::SetupConst),
        ],
    );

    // The re-narrowing guard must be present.
    assert!(
        output.contains("onSomething={() => { if (!___VERTER___flowNarrow(handle, ___VERTER___o0)"),
        "function-typed prop under v-if must get the re-narrowing guard: {output}"
    );
    assert!(
        !output.contains("v-if"),
        "v-if directive must be removed: {output}"
    );

    // Positive: the user body identifier `handle` maps back to its source byte offset.
    let handle_src = source.find("handle()").unwrap() as u32;
    assert!(
        has_token_for_src(&tokens, handle_src),
        "the guarded value body `handle` must map to source col {handle_src}. \
         Tokens: {:?}, output: {output}",
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );

    // Negative: the injected guard text maps to None.
    let guard_gen = output.find("{ if (!___VERTER___flowNarrow").unwrap();
    let (gl, gc) = gen_offset_to_line_col(&output, guard_gen);
    assert!(
        !has_token_at_gen(&tokens, gl, gc),
        "the injected guard (gen {gl}:{gc}) must map to None. Tokens: {tokens:?}"
    );

    // Negative: no generated token may map back to the prop start (`:` of :onSomething)
    // — that was the desync anchor.
    let prop_start = source.find(":onSomething").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the :onSomething prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn v_show_merged_style_both_expressions_map() {
    // <div v-show="ready" :style="itemStyle"/> → the v-show condition merges into
    // the existing :style. BOTH `itemStyle` and `ready` are navigable and must map
    // back; the synthetic `style={{...(`, `), display: `, ` ? undefined ...}}` is None.
    let source = r#"<template><div v-show="ready" :style="itemStyle"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("ready", BindingType::SetupConst),
            ("itemStyle", BindingType::SetupConst),
        ],
    );

    assert!(
        output.matches("style=").count() == 1,
        "v-show + :style must merge into one style attribute: {output}"
    );
    assert!(
        output.contains("display:"),
        "merged style should include the display condition: {output}"
    );

    let ready_src = source.find("\"ready\"").unwrap() as u32 + 1;
    let item_src = source.find("\"itemStyle\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, ready_src),
        "v-show condition `ready` must map to source col {ready_src}. \
         Tokens: {tokens:?}, output: {output}"
    );
    assert!(
        has_token_for_src(&tokens, item_src),
        ":style binding `itemStyle` must map to source col {item_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative: neither the v-show nor :style prop start carries a mapping.
    let show_start = source.find("v-show").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == show_start),
        "no generated token may map back to the v-show prop start (col {show_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}
