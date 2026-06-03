use super::*;
use crate::code_transform::CodeTransform;

/// Helper: compile a full SFC with TSX template generation.
/// Returns the template portion of the TSX output.
fn gen_tsx_template(source: &str) -> String {
    let alloc = Allocator::new();
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let template_ast = match syntax.take_template_ast() {
        Some(ast) => ast,
        None => return String::new(),
    };

    let source_type = oxc_span::SourceType::tsx();
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        source_type,
    );

    let mut tpl_ct = CodeTransform::new(source, &alloc);
    let mut out = CodeGenOutput::new(&alloc);
    let bindings = FxHashMap::default();
    let options = IdeTemplateOptions {
        self_name: "App",
        comments: true,
        is_jsx: false,
        strict_slots: false,
    };

    generate_ide_template(
        &template_ast,
        &oxc_ast,
        source,
        &mut out,
        &alloc,
        &bindings,
        &options,
    );
    out.apply_to(&mut tpl_ct);

    let full = tpl_ct.build_string();

    let tpl_start = template_ast.root.tag_open.start as usize;
    let tpl_end = template_ast
        .root
        .tag_close
        .as_ref()
        .map(|tc| tc.end as usize)
        .unwrap_or(full.len());
    let suffix_len = source.len() - tpl_end;
    full[tpl_start..full.len() - suffix_len].to_string()
}

fn gen_tsx_template_with_bindings(source: &str, bindings: &[(&str, BindingType)]) -> String {
    let alloc = Allocator::new();
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let template_ast = match syntax.take_template_ast() {
        Some(ast) => ast,
        None => return String::new(),
    };

    let source_type = oxc_span::SourceType::tsx();
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        source_type,
    );

    let tpl_alloc = Allocator::new();
    let mut tpl_ct = CodeTransform::new(source, &tpl_alloc);
    let mut out = CodeGenOutput::new(&tpl_alloc);

    let mut binding_map: FxHashMap<&str, BindingType> = FxHashMap::default();
    for &(name, bt) in bindings {
        binding_map.insert(tpl_alloc.alloc_str(name), bt);
    }

    let options = IdeTemplateOptions {
        self_name: "App",
        comments: true,
        is_jsx: false,
        strict_slots: false,
    };

    generate_ide_template(
        &template_ast,
        &oxc_ast,
        source,
        &mut out,
        &tpl_alloc,
        &binding_map,
        &options,
    );
    out.apply_to(&mut tpl_ct);

    let full = tpl_ct.build_string();
    let tpl_start = template_ast.root.tag_open.start as usize;
    let tpl_end = template_ast
        .root
        .tag_close
        .as_ref()
        .map(|tc| tc.end as usize)
        .unwrap_or(full.len());
    let suffix_len = source.len() - tpl_end;
    full[tpl_start..full.len() - suffix_len].to_string()
}

// ── Basic nodes ────────────────────────────────────────────

#[test]
fn basic_div() {
    let result = gen_tsx_template("<template><div></div></template>");
    assert!(result.contains("<div></div>"), "got: {}", result);
}

#[test]
fn text_content() {
    let result = gen_tsx_template("<template><div>hello</div></template>");
    assert!(result.contains("<div>{\"hello\"}</div>"), "got: {}", result);
}

#[test]
fn text_content_with_lt_wrapped() {
    let result = gen_tsx_template("<template>2 < 1</template>");
    assert!(
        result.contains("{\"2 < 1\"}")
            || (result.contains("{\"2\"}") && result.contains("{\"< 1\"}")),
        "got: {}",
        result
    );
}

#[test]
fn text_content_escapes_quote() {
    let result = gen_tsx_template("<template>\"</template>");
    assert!(result.contains("{\"\\\"\"}"), "got: {}", result);
}

#[test]
fn interpolation_basic() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ msg }}</div></template>",
        &[("msg", BindingType::SetupRef)],
    );
    assert!(
        result.contains("{ msg }"),
        "{{ msg }} should become bare identifier in TSX mode, got: {}",
        result
    );
}

#[test]
fn interpolation_expression() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ a + b }}</div></template>",
        &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
    );
    assert!(result.contains("{ a + b }"), "got: {}", result);
}

#[test]
fn interpolation_partial_known_binding_stays_bare_for_completion() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ cou }}</div></template>",
        &[("count", BindingType::SetupRef)],
    );
    assert!(
        result.contains("{ cou }") || result.contains("{cou}"),
        "partial binding should stay bare for completion context, got: {}",
        result
    );
    assert!(
        !result.contains("___VERTER___instance.cou"),
        "partial binding must not get instance prefix, got: {}",
        result
    );
}

#[test]
fn comment_preserved() {
    let result = gen_tsx_template("<template><!-- hello --></template>");
    assert!(
        result.contains("{/* hello */}"),
        "Comment should be converted to JSX, got: {}",
        result
    );
}

#[test]
fn self_closing_element() {
    let result = gen_tsx_template("<template><br/></template>");
    assert!(result.contains("<br/>"), "got: {}", result);
}

#[test]
fn void_element_without_self_closing_slash() {
    // HTML void elements like <br> (no slash) must become self-closing in JSX
    let result = gen_tsx_template("<template><br></template>");
    // Must be self-closing in JSX output (either <br/> or <br />)
    assert!(
        result.contains("<br/>") || result.contains("<br />"),
        "void element <br> must be self-closing in JSX: {result}"
    );
    // Must NOT have unclosed <br> (which is invalid JSX)
    assert!(
        !result.contains("<br>"),
        "raw <br> must not appear in JSX output: {result}"
    );

    // Multiple adjacent void elements
    let result2 = gen_tsx_template("<template><br><br></template>");
    assert!(
        !result2.contains("<br>"),
        "adjacent void <br><br> must both be self-closing: {result2}"
    );

    // <input> with attributes
    let result3 = gen_tsx_template(r#"<template><input type="text"></template>"#);
    assert!(
        !result3.contains("<input type=\"text\">"),
        "void <input> with attrs must be self-closing: {result3}"
    );
}

#[test]
fn multiline_text_escapes_newlines_in_string_literal() {
    let result = gen_tsx_template("<template><p>\n  Hello\n  World\n</p></template>");
    // Text IS wrapped in {"..."} — but newlines must be escaped as \n
    assert!(
        result.contains("{\""),
        "text should be wrapped in string literal: {result}"
    );
    // Must contain escaped newlines, not raw newlines inside the string
    assert!(
        result.contains("\\n"),
        "newlines in text must be escaped as \\n: {result}"
    );
    // The {"..."} expression must be on a single line (no raw newlines)
    for line in result.lines() {
        if line.contains("{\"") {
            assert!(
                line.contains("\"}"),
                "text string literal must be on single line (no raw newlines): {result}"
            );
        }
    }
}

#[test]
fn nested_elements() {
    let result = gen_tsx_template("<template><div><span></span></div></template>");
    assert!(
        result.contains("<div><span></span></div>"),
        "got: {}",
        result
    );
}

#[test]
fn multiple_root_elements() {
    let result = gen_tsx_template("<template><div></div><span></span></template>");
    assert!(
        result.contains("<>") && result.contains("</>"),
        "Multiple root elements should be wrapped in fragment, got: {}",
        result
    );
}

// ── Interpolation with bindings ────────────────────────────

#[test]
fn interpolation_with_setup_ref() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ count }}</div></template>",
        &[("count", BindingType::SetupRef)],
    );
    // In TSX mode, SetupRef gets no prefix and no .value suffix (block scope handles unwrapping)
    assert!(
        result.contains("{ count }") && !result.contains("count.value"),
        "SetupRef should be bare identifier in TSX mode (no .value), got: {}",
        result
    );
}

#[test]
fn interpolation_with_setup_const() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ msg }}</div></template>",
        &[("msg", BindingType::SetupConst)],
    );
    // SetupConst in inline mode: no prefix, no suffix
    assert!(
        result.contains("{ msg }"),
        "SetupConst should have no prefix/suffix, got: {}",
        result
    );
}

#[test]
fn interpolation_with_props() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ title }}</div></template>",
        &[("title", BindingType::Props)],
    );
    // Props in inline mode: __props. prefix
    assert!(
        result.contains("__props.title"),
        "Props should get __props. prefix, got: {}",
        result
    );
}

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
fn v_else_if_attribute_removed() {
    let result = gen_tsx_template(
        r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
    );
    assert!(
        !result.contains("v-if"),
        "v-if must not appear in output, got: {}",
        result
    );
    assert!(
        !result.contains("v-else-if"),
        "v-else-if must not appear in output, got: {}",
        result
    );
    assert!(
        !result.contains("v-else"),
        "v-else must not appear in output, got: {}",
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
    // Positive: .map() wrapper should be present
    assert!(
        result.contains(".map("),
        "v-for should produce .map() wrapper, got: {}",
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
        result.contains("__props.list).map("),
        "iterable should get __props. prefix, got: {}",
        result
    );
}

#[test]
fn v_slot_attribute_removed_from_output() {
    let result = gen_tsx_template(
        r#"<template><MyComp><template #default="{ item }"><span>{{ item }}</span></template></MyComp></template>"#,
    );
    assert!(
        !result.contains("v-slot") && !result.contains("#default"),
        "v-slot/#default must be removed from output, got: {}",
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

#[test]
fn v_if_and_v_for_on_same_element_both_removed() {
    let result = gen_tsx_template(
        r#"<template><div v-for="item in items" v-if="item.active">{{ item.name }}</div></template>"#,
    );
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    assert!(
        !result.contains("v-if"),
        "v-if must be removed, got: {}",
        result
    );
    assert!(
        result.contains(".map("),
        "should have .map() wrapper, got: {}",
        result
    );
    assert!(
        result.contains("?"),
        "should have ternary from v-if (not IIFE), got: {}",
        result
    );
    assert!(
        result.contains(": null"),
        "should have ternary null branch, got: {}",
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
        result.contains(".map((item, index)"),
        "destructured params should be in .map() callback, got: {}",
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
        result.contains(".map((value, key, index)"),
        "triple destructure should be in .map(), got: {}",
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
        result.contains(".map("),
        "should produce .map() wrapper, got: {}",
        result
    );
    // "of" separator must not leak
    assert!(
        !result.contains(" of items"),
        "v-for 'of' separator must not appear in output, got: {}",
        result
    );
    // Simple identifier iterable should get type annotation
    assert!(
        result.contains(": (typeof items)[number]"),
        "single param with simple iterable should get type annotation, got: {}",
        result
    );
}

#[test]
fn v_for_simple_param_has_type_annotation() {
    let result =
        gen_tsx_template(r#"<template><div v-for="item in items">{{ item }}</div></template>"#);
    assert!(
        result.contains(": (typeof items)[number]"),
        "single param with simple iterable should get type annotation, got: {}",
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
    // Numeric range must be wrapped in Array.from() — calling .map() directly on
    // a number literal (e.g., `10.map(...)`) is invalid JavaScript.
    assert!(
        result.contains("Array.from({length: 10}"),
        "numeric range should use Array.from(), got: {}",
        result
    );
    assert!(
        result.contains(".map("),
        "numeric range should be iterable in .map(), got: {}",
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
        result.contains(".map("),
        "should have .map() wrapper, got: {}",
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
        result.contains("todos).map(") && !result.contains("todos.value"),
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
        result.contains(") })}"),
        "v-for closing should produce CloseParen+CloseBrace+CloseParen+CloseBrace for .map() statement-body closure, got: {}",
        result
    );
}

// ── ref attribute tests ──────────────────────────────────────

#[test]
fn ref_static_converts_to_jsx_expression() {
    let result = gen_tsx_template(r#"<template><div ref="myRef">content</div></template>"#);
    // Should convert to ref={"myRef"} (JSX expression with string literal)
    assert!(
        result.contains(r#"ref={"myRef"}"#),
        "static ref should become ref={{\"myRef\"}}, got: {}",
        result
    );
    // Must NOT have bare ref="myRef" (Vue syntax, not valid JSX expression)
    assert!(
        !result.contains(r#"ref="myRef""#),
        "bare ref=\"myRef\" must not appear in JSX output, got: {}",
        result
    );
}

#[test]
fn ref_dynamic_binding_converts_to_jsx_expression() {
    let result =
        gen_tsx_template(r#"<template><div :ref="el => (myRef = el)">content</div></template>"#);
    assert!(
        result.contains("ref={"),
        "dynamic :ref should become ref={{expr}}, got: {}",
        result
    );
    // The :ref prefix must be removed
    assert!(
        !result.contains(":ref"),
        ":ref prefix must not appear in output, got: {}",
        result
    );
}

#[test]
fn ref_with_other_attrs_preserved() {
    let result = gen_tsx_template(
        r#"<template><input ref="inputRef" type="text" class="field" /></template>"#,
    );
    assert!(
        result.contains(r#"ref={"inputRef"}"#),
        "ref should be converted, got: {}",
        result
    );
    assert!(
        result.contains(r#"type="text""#),
        "type attribute should be preserved, got: {}",
        result
    );
    assert!(
        result.contains(r#"class="field""#),
        "class attribute should be preserved, got: {}",
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
    // Must have IIFE pattern: {()=>{if(cond){...}}}
    assert!(
        result.contains("{()=>{if(visible){"),
        "v-if should open with IIFE if-block, got: {}",
        result
    );
    // Must close with }}} (block close + arrow body close + JSX expression close)
    assert!(
        result.contains("}}}"),
        "v-if standalone should close with }}}}, got: {}",
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
        result.contains("{()=>{if(a){"),
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
        result.contains("}}}"),
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
        result.contains("{()=>{if(a){"),
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
        result.contains("}}}"),
        "chain without else should close with }}}}, got: {}",
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
        result.contains("{()=>{if(__props.show){"),
        "should use __props.show in if-condition, got: {}",
        result
    );
}

// ── v-if prop narrowing guard tests ──────────────────────────

#[test]
fn v_if_event_handler_gets_guard() {
    let result = gen_tsx_template(
        r#"<template><div v-if="show" @click="handler($event)">click</div></template>"#,
    );
    // Event handler with $event should have guard: if (!(...)) { return undefined; }
    assert!(
        result.contains("return undefined"),
        "event handler in v-if should have narrowing guard, got: {}",
        result
    );
    assert!(
        result.contains("show"),
        "guard should reference the condition, got: {}",
        result
    );
    // Positive: still has the event handler
    assert!(
        result.contains("onClick={"),
        "should have onClick handler, got: {}",
        result
    );
    // Negative: v-if should not appear
    assert!(
        !result.contains("v-if"),
        "v-if must be removed, got: {}",
        result
    );
}

#[test]
fn v_else_if_event_handler_gets_combined_guard() {
    let result = gen_tsx_template(
        r#"<template><div v-if="a">A</div><div v-else-if="b" @click="handler($event)">B</div></template>"#,
    );
    // Guard should negate prior siblings: !((a)) and include own condition (b)
    assert!(
        result.contains("!(("),
        "guard should have negation of prior condition, got: {}",
        result
    );
}

#[test]
fn v_if_non_function_prop_no_guard() {
    let result =
        gen_tsx_template(r#"<template><div v-if="show" :class="myClass">content</div></template>"#);
    // Non-function bindings should NOT have guards
    assert!(
        !result.contains("?undefined:"),
        "non-function prop should not have ternary guard, got: {}",
        result
    );
}

// ── v-if nested IIFE tests ──────────────────────────────────

#[test]
fn v_if_nested_gets_block_guard() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="parent"><span v-if="child">nested</span></div></template>"#,
        &[
            ("parent", BindingType::SetupRef),
            ("child", BindingType::SetupRef),
        ],
    );
    // Nested v-if should have block guard: if(!(condText)) return;
    let has_guard = result.contains("return;") && result.contains("if(!(");
    assert!(
        has_guard,
        "nested v-if should have block guard from parent, got: {}",
        result
    );
    // Should still have the nested if-condition
    assert!(
        result.contains("if(child)"),
        "nested v-if should have its own if-condition, got: {}",
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
    // Pattern: {()=>{if(cond){ {/* @ts-expect-error */} <div>...
    assert!(
        result.contains("if(show)"),
        "should have IIFE condition, got:\n{}",
        result
    );
    // Comment must be AFTER the IIFE open, not before it
    let iife_pos = result.find("{()=>{").expect("should have IIFE open");
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
        result.contains("{()=>{if(show){"),
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
        result.contains("{()=>{if(show){"),
        "should have IIFE if-block, got:\n{}",
        result
    );
    assert!(
        result.contains("else{"),
        "should have else block in same IIFE, got:\n{}",
        result
    );

    // Structural: IIFE must NOT close before else — no }}} between IIFE start and else
    let iife_start = result.find("{()=>{if(").unwrap();
    let else_pos = result.find("else{").unwrap();
    let between = &result[iife_start..else_pos];
    assert!(
        !between.contains("}}}"),
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
    let parsed = oxc_parser::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "TSX template output has syntax errors: {:?}\n--- output ---\n{}",
        parsed
            .errors
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
        result.contains("{()=>{if(a){"),
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
    let iife_start = result.find("{()=>{if(").unwrap();
    let else_if_pos = result.find("else if(").unwrap();
    let else_pos = result.find("else{").unwrap();
    let between_if_and_else_if = &result[iife_start..else_if_pos];
    assert!(
        !between_if_and_else_if.contains("}}}"),
        "IIFE must not close before else-if, got:\n{}",
        result
    );
    let between_else_if_and_else = &result[else_if_pos..else_pos];
    assert!(
        !between_else_if_and_else.contains("}}}"),
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
    let parsed = oxc_parser::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "TSX template output has syntax errors: {:?}\n--- output ---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        result
    );
}

// ── Part G: <template v-slot> with v-if ──────────────────────────

#[test]
fn template_v_if_v_slot_skips_iife() {
    // <template v-if v-slot> should NOT get IIFE wrapping (slot handles conditions)
    let result = gen_tsx_template(
        r#"<template><MyComp><template v-if="show" #default>content</template></MyComp></template>"#,
    );
    // The IIFE pattern should NOT wrap the slot template
    assert!(
        !result.contains("{()=>{if(show){"),
        "template with v-if + v-slot should not get IIFE wrapping, got:\n{}",
        result
    );
}

// ── Part C Step 8: v-bind function prop guards ──────────────────

#[test]
fn v_bind_arrow_expr_gets_ternary_guard() {
    // Arrow expression body: `:handler="() => msg.trim()"` inside v-if
    // → handler={() => !(guard)?undefined:msg.trim()}
    let result = gen_tsx_template(
        r#"<template><div v-if="typeof msg === 'string'" :handler="() => msg.trim()">hi</div></template>"#,
    );
    let norm: String = result.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("?undefined:"),
        "arrow expression prop should get ternary guard, got:\n{}",
        result
    );
    assert!(
        !norm.contains("if(!(") || norm.contains("{()=>{if("),
        "arrow expression should use ternary guard, not block guard in handler, got:\n{}",
        result
    );
}

#[test]
fn v_bind_arrow_block_gets_block_guard() {
    // Arrow block body: `:handler="() => { return msg.trim() }"` inside v-if
    // → handler={() => {if(!(guard))return; return msg.trim() }}
    let result = gen_tsx_template(
        r#"<template><div v-if="typeof msg === 'string'" :handler="() => { return msg.trim() }">hi</div></template>"#,
    );
    let norm: String = result.chars().filter(|c| !c.is_whitespace()).collect();
    // The handler value should contain a block guard
    // Find the handler= part and check for block guard inside it
    let handler_pos = norm.find("handler={").expect("should have handler prop");
    let after_handler = &norm[handler_pos..];
    assert!(
        after_handler.contains("if(!(") && after_handler.contains(")return;"),
        "arrow block prop should get block guard inside handler, got:\n{}",
        result
    );
}

#[test]
fn v_bind_function_expr_gets_block_guard() {
    // Function expression: `:handler="function() { return msg.trim() }"` inside v-if
    // → handler={function() {if(!(guard))return; return msg.trim() }}
    let result = gen_tsx_template(
        r#"<template><div v-if="typeof msg === 'string'" :handler="function() { return msg.trim() }">hi</div></template>"#,
    );
    let norm: String = result.chars().filter(|c| !c.is_whitespace()).collect();
    let handler_pos = norm.find("handler={").expect("should have handler prop");
    let after_handler = &norm[handler_pos..];
    assert!(
        after_handler.contains("if(!(") && after_handler.contains(")return;"),
        "function expression prop should get block guard, got:\n{}",
        result
    );
}

#[test]
fn v_bind_non_function_no_guard() {
    // Non-function props should NOT get any guard
    let result = gen_tsx_template(r#"<template><div v-if="show" :class="msg">hi</div></template>"#);
    let norm: String = result.chars().filter(|c| !c.is_whitespace()).collect();
    // Find the class prop
    let class_pos = norm.find("class={").expect("should have class prop");
    let after_class = &norm[class_pos..];
    // Should NOT have any guard
    assert!(
        !after_class.starts_with("class={()=>") && !after_class.contains("?undefined:"),
        "non-function prop should not get guard, got:\n{}",
        result
    );
}

// ── Part H: JSX syntax validation for directive combinations ─────

/// Validate that the generated TSX template is parseable JSX/TSX.
/// Wraps the template output in a JSX fragment so IIFE expressions parse correctly.
fn assert_valid_jsx(source: &str, label: &str) {
    let result = gen_tsx_template(source);
    let wrapper = format!("const x = <>{}</>", result);
    let val_alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "[{}] TSX syntax errors: {:?}\n--- source ---\n{}\n--- output ---\n{}",
        label,
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        source,
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
fn jsx_valid_v_slot_component() {
    assert_valid_jsx(
        r#"<template><MyComp v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
        "v-slot on component",
    );
}

#[test]
fn jsx_valid_named_slot() {
    assert_valid_jsx(
        r#"<template><MyComp><template #header>Header</template><template #default>Body</template></MyComp></template>"#,
        "named slots with template",
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
fn jsx_valid_v_if_with_v_slot() {
    assert_valid_jsx(
        r#"<template><MyComp v-if="show" v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
        "v-if + v-slot on component",
    );
}

#[test]
fn jsx_valid_v_for_with_v_slot() {
    assert_valid_jsx(
        r#"<template><MyComp v-for="item in items" :key="item.id" v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
        "v-for + v-slot",
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
fn v_model_on_native_element() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><input v-model="msg" /></template>"#,
        &[("msg", BindingType::SetupRef)],
    );
    // Native input should use `value` (not `modelValue`) and native event handler
    assert!(
        result.contains("value={msg}"),
        "v-model on native input should produce value prop. Got: {}",
        result
    );
    assert!(
        !result.contains("modelValue"),
        "v-model on native input must NOT use modelValue. Got: {}",
        result
    );
    assert!(
        result.contains("onInput={"),
        "v-model on native input should use onInput event. Got: {}",
        result
    );
    // Must not have any quoted attribute names (invalid JSX)
    assert!(
        !result.contains(r#""onUpdate:"#),
        "native input must not have quoted onUpdate attribute. Got: {}",
        result
    );
    assert!(
        !result.contains("v-model"),
        "v-model attribute must be removed. Got: {}",
        result
    );
}

#[test]
fn v_model_with_explicit_change_handler_no_duplicate() {
    // v-model on <input type="checkbox"> + explicit @change should not produce
    // duplicate onChange attributes (TS17001).
    let result = gen_tsx_template_with_bindings(
        r#"<template><input v-model="model" type="checkbox" @change="handleChange" /></template>"#,
        &[
            ("model", BindingType::SetupRef),
            ("handleChange", BindingType::SetupConst),
        ],
    );
    let on_change_count = result.matches("onChange=").count()
        + result.matches("onChange:").count()
        + result.matches("\"onChange\"").count();
    assert_eq!(
        on_change_count, 1,
        "v-model + @change on native input should produce exactly one onChange. Got {} in: {}",
        on_change_count, result
    );
    assert!(
        !result.contains("v-model"),
        "v-model attribute must be removed. Got: {}",
        result
    );
}

#[test]
fn v_model_with_explicit_input_handler_no_duplicate() {
    // v-model on text <input> + explicit @input should not produce
    // duplicate onInput attributes.
    let result = gen_tsx_template_with_bindings(
        r#"<template><input v-model="text" @input="onInput" /></template>"#,
        &[
            ("text", BindingType::SetupRef),
            ("onInput", BindingType::SetupConst),
        ],
    );
    let on_input_count = result.matches("onInput=").count();
    assert_eq!(
        on_input_count, 1,
        "v-model + @input on text input should produce exactly one onInput. Got {} in: {}",
        on_input_count, result
    );
    // v-model should still produce the value prop
    assert!(
        result.contains("value={text}"),
        "v-model should still produce value prop. Got: {}",
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
fn duplicate_keydown_handlers_use_spread_for_second() {
    // @keydown.space + @keydown.enter both map to onKeyDown —
    // the second must use spread syntax to avoid TS17001.
    let result = gen_tsx_template_with_bindings(
        r#"<template><td @keydown.space.prevent.stop="handleClick" @keydown.enter.prevent.stop="handleClick" /></template>"#,
        &[("handleClick", BindingType::SetupConst)],
    );
    let on_keydown_attr = result.matches("onKeyDown={").count();
    assert!(
        on_keydown_attr <= 1,
        "should have at most one onKeyDown= attribute (rest as spread). Got {} in: {}",
        on_keydown_attr,
        result
    );
    // Should still reference both handlers somehow
    assert!(
        result.contains("handleClick"),
        "handler reference should be present. Got: {}",
        result
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
fn multiline_static_style_merged_with_dynamic_no_unterminated_string() {
    // When static style has newlines and is merged with :style, the static value
    // must not produce an unterminated JS string literal inside normalizeStyle.
    let result = gen_tsx_template_with_bindings(
        "<template><div style=\"\n  position: absolute;\n  top: 0;\n\" :style=\"{ height: h + 'px' }\">hi</div></template>",
        &[("h", BindingType::SetupConst)],
    );
    // Positive: should have normalizeStyle call
    assert!(
        result.contains("normalizeStyle"),
        "merged style should use normalizeStyle. Got: {}",
        result
    );
    // Negative: the static string inside normalizeStyle must NOT have literal newlines
    // (which would be unterminated string literal TS1002)
    let norm_idx = result.find("normalizeStyle").unwrap();
    let after_norm = &result[norm_idx..];
    // Find the string literal inside the normalizeStyle call
    if let Some(quote_idx) = after_norm.find(",\"") {
        let after_quote = &after_norm[quote_idx + 2..];
        let end_quote = after_quote.find('"').unwrap_or(after_quote.len());
        let static_str = &after_quote[..end_quote];
        assert!(
            !static_str.contains('\n'),
            "static style string must not contain newlines. Got: {}",
            static_str
        );
    }
}

// ── Slot outlets in TSX ────────────────────────────────────────

#[test]
fn slot_outlet_default() {
    let result = gen_tsx_template(r#"<template><slot /></template>"#);
    assert!(
        result.contains("___VERTER___instance.$slots.default?.()"),
        "Default slot outlet should produce ___VERTER___instance.$slots.default?.(). Got: {}",
        result
    );
    assert!(
        !result.contains("<slot"),
        "<slot> tag must be replaced. Got: {}",
        result
    );
    assert!(
        !result.contains("{ $slots.default"),
        "Bare $slots without instance prefix must not appear. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_named() {
    let result = gen_tsx_template(r#"<template><slot name="header" /></template>"#);
    assert!(
        result.contains("___VERTER___instance.$slots.header?.()"),
        "Named slot outlet should produce ___VERTER___instance.$slots.header?.(). Got: {}",
        result
    );
    assert!(
        !result.contains("{ $slots.header"),
        "Bare $slots without instance prefix must not appear. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_with_props() {
    let result = gen_tsx_template(r#"<template><slot name="item" :data="itemData" /></template>"#);
    assert!(
        result.contains("___VERTER___instance.$slots.item"),
        "Slot call should reference ___VERTER___instance.$slots.item. Got: {}",
        result
    );
    assert!(
        result.contains("data: ___VERTER___instance.itemData")
            || result.contains("data:___VERTER___instance.itemData"),
        "Slot props should include data binding with instance prefix (unresolved). Got: {}",
        result
    );
}

#[test]
fn slot_outlet_with_fallback() {
    let result = gen_tsx_template(r#"<template><slot>fallback</slot></template>"#);
    assert!(
        result.contains("___VERTER___instance.$slots.default?.()"),
        "Slot with fallback should have ___VERTER___instance.$slots call. Got: {}",
        result
    );
    assert!(
        result.contains("??"),
        "Slot with fallback should use ?? operator. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_hyphenated_name() {
    let result = gen_tsx_template(r#"<template><slot name="overlay-content" /></template>"#);
    assert!(
        result.contains("$slots['overlay-content']"),
        "Hyphenated slot name must use bracket notation. Got: {}",
        result
    );
    assert!(
        !result.contains("$slots.overlay-content"),
        "Must NOT use dot notation for hyphenated names (parses as subtraction). Got: {}",
        result
    );
    assert!(
        !result.contains("<slot"),
        "<slot> tag must be replaced. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_hyphenated_name_with_props() {
    let result = gen_tsx_template(r#"<template><slot name="item-data" :value="x" /></template>"#);
    assert!(
        result.contains("$slots['item-data']"),
        "Hyphenated slot name with props must use bracket notation. Got: {}",
        result
    );
    assert!(
        result.contains("value:") || result.contains("value :"),
        "Slot props should be present. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_dotted_name() {
    let result = gen_tsx_template(r#"<template><slot name="foo.bar" /></template>"#);
    assert!(
        result.contains("$slots['foo.bar']"),
        "Dotted slot name must use bracket notation. Got: {}",
        result
    );
    assert!(
        !result.contains("$slots.foo.bar"),
        "Must NOT use dot notation for dotted names. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_hyphenated_name_with_fallback() {
    let result =
        gen_tsx_template(r#"<template><slot name="overlay-content">fallback</slot></template>"#);
    assert!(
        result.contains("$slots['overlay-content']"),
        "Hyphenated slot name with fallback must use bracket notation. Got: {}",
        result
    );
    assert!(
        result.contains("??"),
        "Slot with fallback should use ?? operator. Got: {}",
        result
    );
}

// ── Instance property resolution in TSX ─────────────────────────

#[test]
fn tsx_unresolved_dollar_emit_gets_instance_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ $emit('click') }}</div></template>"#,
        &[],
    );
    assert!(
        result.contains("___VERTER___instance.$emit"),
        "Unresolved $emit should get instance prefix. Got: {}",
        result
    );
    assert!(
        !result.contains("{ $emit(") && !result.contains("{$emit("),
        "Bare $emit without prefix must not appear. Got: {}",
        result
    );
}

#[test]
fn tsx_unresolved_dollar_attrs_gets_instance_prefix() {
    let result =
        gen_tsx_template_with_bindings(r#"<template><div>{{ $attrs }}</div></template>"#, &[]);
    assert!(
        result.contains("___VERTER___instance.$attrs"),
        "Unresolved $attrs should get instance prefix. Got: {}",
        result
    );
}

#[test]
fn tsx_known_setup_binding_stays_bare() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ count }}</div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    assert!(
        !result.contains("___VERTER___instance.count"),
        "Known binding should NOT get instance prefix. Got: {}",
        result
    );
}

#[test]
fn tsx_props_binding_stays_dunder_props() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ msg }}</div></template>"#,
        &[("msg", BindingType::Props)],
    );
    assert!(
        result.contains("__props.msg"),
        "Props binding should use __props. Got: {}",
        result
    );
    assert!(
        !result.contains("___VERTER___instance.msg"),
        "Props binding should NOT get instance prefix. Got: {}",
        result
    );
}

// ── Dynamic event names in TSX ────────────────────────────────

#[test]
fn dynamic_event_name() {
    let result = gen_tsx_template(r#"<template><div @[eventName]="handler" /></template>"#);
    assert!(
        result.contains("eventName") || result.contains("_ctx.eventName"),
        "Dynamic event should reference eventName. Got: {}",
        result
    );
    assert!(
        !result.contains("@["),
        "Dynamic event syntax must be removed. Got: {}",
        result
    );
}

#[test]
fn dynamic_event_name_with_binding() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @[eventName]="handler" /></template>"#,
        &[("eventName", BindingType::SetupRef)],
    );
    assert!(
        result.contains("eventName") && !result.contains("eventName.value"),
        "Dynamic event name on ref should be bare identifier in TSX mode (no .value). Got: {}",
        result
    );
}

// ── v-for source mapping (#19) ──────────────────────────────────

/// Helper: generate TSX template with bindings AND return source map tokens.
/// Returns (output_string, Vec<(dst_line, dst_col, src_col)>).
fn gen_tsx_template_with_map(
    source: &str,
    bindings: &[(&str, BindingType)],
) -> (String, Vec<(u32, u32, u32)>) {
    let alloc = Allocator::new();
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let template_ast = match syntax.take_template_ast() {
        Some(ast) => ast,
        None => return (String::new(), Vec::new()),
    };

    let source_type = oxc_span::SourceType::tsx();
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        source_type,
    );

    let tpl_alloc = Allocator::new();
    let mut tpl_ct = CodeTransform::new(source, &tpl_alloc);
    let mut out = CodeGenOutput::new(&tpl_alloc);
    let binding_map: FxHashMap<&str, BindingType> = bindings.iter().copied().collect();
    let options = IdeTemplateOptions {
        self_name: "App",
        comments: true,
        is_jsx: false,
        strict_slots: false,
    };

    generate_ide_template(
        &template_ast,
        &oxc_ast,
        source,
        &mut out,
        &tpl_alloc,
        &binding_map,
        &options,
    );
    out.apply_to(&mut tpl_ct);

    let full = tpl_ct.build_string();
    let map =
        tpl_ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("test.vue"));
    let tokens: Vec<(u32, u32, u32)> = map
        .get_tokens()
        .filter(|t| t.get_source_id().is_some())
        .map(|t| (t.get_dst_line(), t.get_dst_col(), t.get_src_col()))
        .collect();

    (full, tokens)
}

#[test]
fn v_for_iterable_is_source_mapped() {
    // v-for="item in items" — the iterable `items` in the .map() wrapper
    // should have a source map token pointing back to the original `items` position.
    let source = r#"<template><div v-for="item in items">{{ item }}</div></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Verify output shape
    assert!(
        output.contains(".map("),
        "v-for should produce .map() wrapper: {output}"
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
    // The iteration parameter `item` in .map((item) => ...) should map back
    // to the parameter position in the v-for attribute value.
    let source = r#"<template><div v-for="item in items">{{ item }}</div></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    assert!(
        output.contains(".map((item"),
        "v-for should produce .map((item...) => ...): {output}"
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
fn component_is_dynamic_expr_is_source_mapped() {
    // <component :is="currentView"> should emit a source-mapped temp variable
    // so TSGO can provide hover info on `currentView`.
    let source = r#"<template><component :is="currentView">hello</component></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("currentView", BindingType::SetupRef)]);

    // The output should contain the temp variable with the expression
    assert!(
        output.contains("currentView"),
        "output should contain `currentView`: {output}"
    );

    // Find the byte offset of "currentView" in the :is attribute value
    let expr_src_offset = source.find("currentView").unwrap();

    // There should be a source map token pointing back to the expression
    let has_expr_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == expr_src_offset as u32);
    assert!(
        has_expr_token,
        "component :is expression should have source map token at src col {}. Tokens: {:?}",
        expr_src_offset, tokens
    );
}

#[test]
fn component_is_dynamic_resolves_bindings() {
    // <component :is="currentView"> with SetupRef binding should resolve
    // the expression through the BindingResolver (e.g., `currentView.value`
    // for refs in non-inline mode, or just `currentView` for inline).
    let source = r#"<template><component :is="currentView">hello</component></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("currentView", BindingType::SetupRef)]);

    // With inline mode (default for TSX), SetupRef bindings are used directly.
    // The expression should be present in the output (not _ctx. prefixed since inline).
    assert!(
        output.contains("currentView"),
        "output should contain resolved `currentView`: {output}"
    );
    // The `:is` attribute itself should be removed
    assert!(
        !output.contains(":is="),
        "`:is` attribute should be removed from output: {output}"
    );
    // The `component` tag should be rewritten
    assert!(
        !output.contains("<component"),
        "`<component` tag should be rewritten: {output}"
    );
}

#[test]
fn component_is_dynamic_resolves_data_binding() {
    // In TSX mode, Data bindings use ___VERTER___instance. prefix (no _ctx. prefix).
    let source = r#"<template><component :is="currentView">hello</component></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("currentView", BindingType::Data)]);

    assert!(
        output.contains("___VERTER___instance.currentView") && !output.contains("_ctx.currentView"),
        "Data binding should use instance prefix in TSX mode: {output}"
    );
    assert!(
        !output.contains(":is="),
        "`:is` attribute should be removed from output: {output}"
    );
}

// ── Data/Options binding instance prefix in TSX mode ─────────────

#[test]
fn data_binding_uses_instance_prefix() {
    let source = r#"<template><div>{{ count }}</div></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("count", BindingType::Data)]);

    // Positive: Data bindings should use ___VERTER___instance. prefix
    assert!(
        output.contains("___VERTER___instance.count"),
        "Data binding should use instance prefix in TSX mode: {output}"
    );
    // Negative: should NOT contain bare `{count}` without instance prefix
    assert!(
        !output.contains("{count}"),
        "Data binding should not be bare — must use instance prefix: {output}"
    );
}

#[test]
fn options_binding_uses_instance_prefix() {
    let source = r#"<template><div>{{ total }}</div></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("total", BindingType::Options)]);

    // Positive: Options bindings should use ___VERTER___instance. prefix
    assert!(
        output.contains("___VERTER___instance.total"),
        "Options binding should use instance prefix in TSX mode: {output}"
    );
}

#[test]
fn event_handler_simple_ident_is_source_mapped() {
    // @click="handler" — the handler identifier should have a source map token.
    let source = r#"<template><button @click="handler">click</button></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("handler", BindingType::SetupConst)]);

    assert!(
        output.contains("onClick={handler}"),
        "should emit onClick={{handler}}: {output}"
    );

    // Find the byte offset of "handler" in the @click value
    let handler_src_offset = source.find("handler").unwrap();

    let has_handler_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == handler_src_offset as u32);
    assert!(
        has_handler_token,
        "event handler should have source map token at src col {}. Tokens: {:?}",
        handler_src_offset, tokens
    );
}

#[test]
fn event_handler_fn_expr_is_source_mapped() {
    // @click="(e) => doSomething(e)" — the expression should be source-mapped.
    let source = r#"<template><button @click="(e) => doSomething(e)">click</button></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("doSomething", BindingType::SetupConst)]);

    assert!(
        output.contains("onClick={(e) => doSomething(e)}"),
        "should emit onClick with fn expr: {output}"
    );

    // Find the byte offset of the expression in the @click value
    let expr_src_offset = source.find("(e) => doSomething").unwrap();

    let has_expr_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == expr_src_offset as u32);
    assert!(
        has_expr_token,
        "fn expression should have source map token at src col {}. Tokens: {:?}",
        expr_src_offset, tokens
    );
}

#[test]
fn event_handler_inline_expr_is_source_mapped() {
    // @click="count++" — the inline expression should be source-mapped.
    // Using SetupConst to avoid .value transformation changing the text.
    let source = r#"<template><button @click="count++">click</button></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupConst)]);

    assert!(
        output.contains("count++"),
        "should contain the expression: {output}"
    );

    // Find byte offset of "count++" in the @click value
    let expr_src_offset = source.find("count++").unwrap();

    let has_expr_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == expr_src_offset as u32);
    assert!(
        has_expr_token,
        "inline expression should have source map token at src col {}. Tokens: {:?}",
        expr_src_offset, tokens
    );
}

// ── Bug 1: Dynamic <component :is> uses extractRenderComponent ──

#[test]
fn component_dynamic_is_uses_extract_render_component() {
    let source = r#"<template><component :is="'div'"></component></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("___VERTER___extractRenderComponent"),
        "should use extractRenderComponent wrapper: {output}"
    );
    assert!(
        output.contains("___VERTER___component_render"),
        "should use ___VERTER___component_render temp name: {output}"
    );
    assert!(
        output.contains("const ___VERTER___component_render=___VERTER___extractRenderComponent("),
        "should declare const with extractRenderComponent wrapper: {output}"
    );
    // Negative: old format should not appear
    assert!(
        !output.contains("__verter_component_render"),
        "old format __verter_component_render should not appear: {output}"
    );
    assert!(
        !output.contains("<component"),
        "<component tag should be rewritten: {output}"
    );
}

#[test]
fn component_dynamic_is_expression() {
    let source = r#"<template><component :is="as || 'div'"></component></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("as", BindingType::SetupRef)]);

    assert!(
        output.contains("___VERTER___extractRenderComponent("),
        "should use extractRenderComponent: {output}"
    );
    assert!(
        output.contains("<___VERTER___component_render"),
        "should rewrite opening tag: {output}"
    );
    assert!(
        output.contains("</___VERTER___component_render>"),
        "should rewrite closing tag: {output}"
    );
}

#[test]
fn component_static_is_unchanged() {
    let source = r#"<template><component is="div" tabindex="1"></component></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("<div"),
        "static is should rewrite to target tag: {output}"
    );
    assert!(
        !output.contains("extractRenderComponent"),
        "static is should not use extractRenderComponent: {output}"
    );
    assert!(
        !output.contains("<component"),
        "<component tag should be rewritten: {output}"
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

// ── Bug 2: Class/Style merge ──

#[test]
fn class_merge_static_and_dynamic() {
    let source = r#"<template><div class="foo" :class="{bar: true}"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );
    assert!(
        output.contains("{bar: true}") && output.contains("\"foo\""),
        "should contain both class expressions: {output}"
    );
    // Count class= occurrences — should be exactly 1
    let class_count = output.matches("class=").count();
    assert_eq!(
        class_count, 1,
        "should have exactly 1 class= attribute, got {class_count}: {output}"
    );
}

#[test]
fn class_merge_with_prop_in_between() {
    let source =
        r#"<template><div class="foo" my-random-prop="true" :class="{bar: true}"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );
    assert!(
        output.contains("my-random-prop"),
        "should preserve other props: {output}"
    );
    let class_count = output.matches("class=").count();
    assert_eq!(
        class_count, 1,
        "should have exactly 1 class= attribute, got {class_count}: {output}"
    );
}

#[test]
fn style_merge_static_and_dynamic() {
    let source = r#"<template><div style="color:red" :style="{ bg: 'blue' }"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("normalizeStyle"),
        "should use normalizeStyle: {output}"
    );
    let style_count = output.matches("style=").count();
    assert_eq!(
        style_count, 1,
        "should have exactly 1 style= attribute, got {style_count}: {output}"
    );
}

#[test]
fn class_and_style_merge_combined() {
    let source = r#"<template><div class="a" :class="b" style="c" :style="d"/></template>"#;
    let output = gen_tsx_template_with_bindings(
        source,
        &[("b", BindingType::SetupRef), ("d", BindingType::SetupRef)],
    );

    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );
    assert!(
        output.contains("normalizeStyle"),
        "should use normalizeStyle: {output}"
    );
    let class_count = output.matches("class=").count();
    assert_eq!(
        class_count, 1,
        "should have exactly 1 class= attribute: {output}"
    );
    let style_count = output.matches("style=").count();
    assert_eq!(
        style_count, 1,
        "should have exactly 1 style= attribute: {output}"
    );
}

#[test]
fn style_object_literal_gets_css_properties_satisfies() {
    let source = r#"<template><div :style="{ color: 'red' }"/></template>"#;
    let output = gen_tsx_template(source);
    // Positive: object literal style should get CSSProperties satisfies annotation
    assert!(
        output.contains("satisfies") && output.contains("CSSProperties"),
        "object literal :style should have satisfies CSSProperties: {output}"
    );
    // Negative: non-object-literal style should NOT get satisfies
    let source2 = r#"<template><div :style="myVar"/></template>"#;
    let output2 = gen_tsx_template(source2);
    assert!(
        !output2.contains("satisfies"),
        "non-object-literal :style should NOT have satisfies: {output2}"
    );
}

#[test]
fn class_only_static_no_merge() {
    let source = r#"<template><div class="foo"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("class=\"foo\""),
        "static class should be unchanged: {output}"
    );
    assert!(
        !output.contains("normalizeClass"),
        "should not use normalizeClass for static-only: {output}"
    );
}

#[test]
fn class_only_dynamic_no_merge() {
    let source = r#"<template><div :class="{bar: true}"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("class={{bar: true}}"),
        "dynamic-only class should be simple binding: {output}"
    );
    assert!(
        !output.contains("normalizeClass"),
        "should not use normalizeClass for dynamic-only: {output}"
    );
}

#[test]
fn class_merge_no_extra_closing_brace() {
    // Bug: `<span :class="$attrs.class" class="ns-popover--wrapper">` generated `])}}`
    // (double closing brace) instead of `])}`
    let source =
        r#"<template><span :class="$attrs.class" class="ns-popover--wrapper">hi</span></template>"#;
    let output = gen_tsx_template(source);

    // Positive: should contain merged normalizeClass with static value
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass for merged class: {output}"
    );
    assert!(
        output.contains("\"ns-popover--wrapper\""),
        "should contain static class value: {output}"
    );

    // Negative: must NOT have double closing brace `])}}` — only `])}`
    let double_brace = "])}}";
    assert!(
        !output.contains(double_brace),
        "must not have extra closing brace: {output}"
    );
    // Positive: should have exactly `])}`
    let single_brace = "])}";
    assert!(
        output.contains(single_brace),
        "should have correct single closing brace: {output}"
    );
}

#[test]
fn class_merge_static_before_dynamic_no_extra_brace() {
    // Popover.vue pattern: static `class` BEFORE dynamic `:class`
    let source =
        r#"<template><span class="ns-popover--wrapper" :class="$attrs.class">hi</span></template>"#;
    let output = gen_tsx_template(source);

    eprintln!("=== OUTPUT ===\n{}\n=== END ===", output);

    // Positive: should contain normalizeClass
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );

    // Negative: must NOT have double closing brace
    let double_brace = "])}}";
    assert!(
        !output.contains(double_brace),
        "must not have extra closing brace: {output}"
    );
}

#[test]
fn class_merge_dynamic_before_static_no_extra_brace() {
    // Original Bug 2 pattern: dynamic `:class` BEFORE static `class`
    let source =
        r#"<template><span :class="$attrs.class" class="ns-popover--wrapper">hi</span></template>"#;
    let output = gen_tsx_template(source);

    eprintln!("=== OUTPUT ===\n{}\n=== END ===", output);

    // Positive: should contain normalizeClass
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );

    // Negative: must NOT have double closing brace
    let double_brace = "])}}";
    assert!(
        !output.contains(double_brace),
        "must not have extra closing brace: {output}"
    );
}

#[test]
fn popover_vue_template_generates_valid_tsx() {
    // Full Popover.vue template pattern that user reports as broken
    let source = r#"<script setup lang="ts">
import { computed, ref, useTemplateRef, watch } from 'vue'
const show = ref(false)
const onClickWrapper = () => {}
const floatingStyles = ref({})
const showArrow = ref(false)
const arrowPos = ref({})
</script>
<template>
  <span
    ref="wrapperElm"
    class="ns-popover--wrapper"
    :class="$attrs.class"
    :style="$attrs.style as any"
    @click="onClickWrapper"
  >
    <slot name="reference" />
  </span>
  <Popup
    ref="popupElm"
    v-model:show="show"
    class="ns-popover"
    :style="[floatingStyles, $attrs.style]"
    position=""
  >
    <div v-if="showArrow" ref="arrowElm" class="ns-popover__arrow" :style="[arrowPos]"></div>
    <div
      role="menu"
      class="ns-popover__content"
      :class="{
        'ns-popover__content--horizontal': true,
      }"
    >
      <slot />
    </div>
  </Popup>
</template>"#;
    let output = gen_tsx_template(source);

    eprintln!("=== POPOVER OUTPUT ===\n{}\n=== END ===", output);

    // Must not have any double closing braces from normalizeClass
    let double_brace = "])}}";
    assert!(
        !output.contains(double_brace),
        "must not have extra closing brace: {output}"
    );

    // normalizeClass should be present for merged class attrs
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass for merged class: {output}"
    );

    // v-if should NOT appear in JSX
    assert!(
        !output.contains("v-if"),
        "v-if attribute must be removed from JSX: {output}"
    );
}

#[test]
fn class_merge_with_script_attrs_and_generic() {
    // Regression: Popover.vue with attrs="{ class: string, style: string }" on
    // <script setup> produces duplicate class/style attributes in JSX.
    let source = r#"<script setup lang="ts" attrs="{ class: string, style: string }" generic="T extends object">
import { ref } from 'vue'
const show = ref(false)
const onClickWrapper = () => {}
</script>
<template>
  <span
    ref="wrapperElm"
    class="ns-popover--wrapper"
    :class="$attrs.class"
    :style="$attrs.style as any"
    @click="onClickWrapper"
  >
    <slot name="reference" />
  </span>
</template>"#;
    let output = gen_tsx_template(source);

    eprintln!("=== ATTRS+GENERIC OUTPUT ===\n{}\n=== END ===", output);

    // Positive: should use normalizeClass for merged class
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass for merged class: {output}"
    );

    // Critical: must have exactly 1 class= attribute (no duplicates → ts(17001))
    let class_count = output.matches("class=").count();
    assert_eq!(
        class_count, 1,
        "should have exactly 1 class= attribute, got {class_count}: {output}"
    );

    // Critical: must have exactly 1 style= attribute (no duplicates)
    let style_count = output.matches("style=").count();
    assert_eq!(
        style_count, 1,
        "should have exactly 1 style= attribute, got {style_count}: {output}"
    );

    // Negative: must not have double closing brace from normalizeClass
    assert!(
        !output.contains("])}}"),
        "must not have extra closing brace: {output}"
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

/// `:data="$attrs"` (static key with instance prefix) must use split overwrite
/// so `$attrs` retains its source map position.
#[test]
fn static_prop_with_prefix_source_map_accuracy() {
    let source = r#"<template><div :data="$attrs"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Positive: the prop should be converted to JSX binding
    assert!(
        output.contains("data={___VERTER___instance.$attrs}"),
        ":data=\"$attrs\" should produce data={{instance.$attrs}}: {output}"
    );
    // Negative: no raw `:data` or `v-bind`
    assert!(
        !output.contains(":data"),
        ":data directive must be removed from JSX: {output}"
    );

    // Source map: verify $attrs maps to its original source position
    let source_attrs_col = source.find("$attrs").expect("$attrs in source") as u32;
    let has_attrs_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == source_attrs_col);
    assert!(
        has_attrs_token,
        "source map must have a token mapping to the original $attrs position (col {}), \
         but only found source columns: {:?}",
        source_attrs_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );
}

/// `:rows="d_rows"` with Data binding (PrimeVue-shaped case) — the prefix-only
/// rewrite must use split overwrite so `d_rows` retains its source map position.
/// Without the split, TSGO hover lands on the synthetic `___VERTER___instance` prefix.
#[test]
fn data_prop_binding_source_map_accuracy() {
    let source = r#"<template><DataTable :rows="d_rows"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("d_rows", BindingType::Data)]);

    // Positive: prop should use instance prefix
    assert!(
        output.contains("rows={___VERTER___instance.d_rows}"),
        ":rows=\"d_rows\" should produce rows={{___VERTER___instance.d_rows}}: {output}"
    );
    // Negative: no raw :rows
    assert!(
        !output.contains(":rows"),
        ":rows directive must be removed from JSX: {output}"
    );

    // Source map: d_rows should map to its original source position
    let source_col = source.find("d_rows").expect("d_rows in source") as u32;
    let has_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == source_col);
    assert!(
        has_token,
        "source map must have a token mapping to the original d_rows position (col {}), \
         but only found source columns: {:?}",
        source_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );
}

/// `:class="{ 'active': visible }"` with Props binding — patch-based approach must
/// preserve source map tokens for identifiers so TSGO hover works on sub-expressions.
/// With Props binding, `visible` gets `__props.` prefix, which previously used a single
/// overwrite destroying source map tokens.
#[test]
fn class_binding_with_props_source_map_accuracy() {
    let source = r#"<template><div :class="{ 'active': visible }"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("visible", BindingType::Props)]);

    // Positive: should produce JSX class binding with __props prefix
    assert!(
        output.contains("class={{ 'active': __props.visible }}"),
        "should convert :class to JSX class binding with props prefix: {output}"
    );
    // Negative: no raw :class
    assert!(
        !output.contains(":class"),
        ":class directive must be removed from JSX: {output}"
    );

    // Source map: `visible` identifier should have a token at its original source position
    // (patch-based approach preserves it via collect_binding_patches)
    let visible_src_col = source.find("visible").expect("visible in source") as u32;
    let has_visible_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == visible_src_col);
    assert!(
        has_visible_token,
        "source map must have a token mapping to the original visible position (col {}), \
         but only found source columns: {:?}",
        visible_src_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );
}

/// `:class` with merged static+dynamic class — source map tokens preserved via patch-based.
#[test]
fn merged_class_binding_source_map_accuracy() {
    let source = r#"<template><div class="base" :class="{ 'active': isActive }"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("isActive", BindingType::Props)]);

    // Positive: should use normalizeClass with merged static value and __props prefix
    assert!(
        output.contains("___VERTER___normalizeClass"),
        "merged class should use normalizeClass: {output}"
    );
    assert!(
        output.contains("__props.isActive"),
        "should apply __props prefix to isActive: {output}"
    );

    // Negative: no raw :class
    assert!(
        !output.contains(":class"),
        ":class directive must be removed from JSX: {output}"
    );

    // Source map: `isActive` identifier should have a token at its original source position
    let src_col = source.find("isActive").expect("isActive in source") as u32;
    let has_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == src_col);
    assert!(
        has_token,
        "source map must have a token mapping to the original isActive position (col {}), \
         but only found source columns: {:?}",
        src_col,
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

/// `$props` member access: `{{ $props.msg }}` — verify sourcemap token for `$props`.
///
/// The PositionMapper interpolates from `$props` token to `.msg`. If the expression is
/// rewritten (e.g., `$props` → `__props`), the original source token should still map
/// correctly. The `.msg` part needs the linear offset from the `$props` token to be intact.
#[test]
fn dollar_props_member_access_source_map() {
    let source = r#"<template><div>{{ $props.msg }}</div></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Positive: should contain $props.msg or a prefixed version
    assert!(
        output.contains("$props") || output.contains("__props"),
        "should contain $props reference: {output}"
    );

    // Sourcemap: verify `$props` has a token
    let props_src_col = source.find("$props").unwrap() as u32;
    let has_props_token = tokens.iter().any(|&(_, _, sc)| sc == props_src_col);
    assert!(
        has_props_token,
        "source map must have token for `$props` at col {}, tokens: {:?}",
        props_src_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );

    // Check: if $props is rewritten to something longer (e.g., __props or ___VERTER___.instance.$props),
    // the interpolation from the $props token to .msg won't work because the generated text
    // is longer than the source. Log the output for diagnosis.
    let msg_src_col = source.find("msg").unwrap() as u32;
    let props_to_msg_src_offset = msg_src_col - props_src_col; // 7 chars ("$props.")

    // Find generated position of $props token
    let props_gen = tokens
        .iter()
        .find(|&&(_, _, sc)| sc == props_src_col)
        .map(|&(dl, dc, _)| (dl, dc));

    if let Some((_gen_line, gen_col)) = props_gen {
        // In the generated output, check what's at gen_col + 7 (the interpolated .msg position)
        let gen_msg_expected = gen_col + props_to_msg_src_offset;
        let lines: Vec<&str> = output.lines().collect();
        if let Some(line_str) = lines.first() {
            if (gen_msg_expected as usize) < line_str.len() {
                let at_expected = &line_str[gen_msg_expected as usize..];
                if !at_expected.starts_with("msg") {
                    // Interpolation broken — $props was rewritten to something longer.
                    // This is the root cause: the generated text between $props and .msg
                    // has different length than the source, breaking linear interpolation.
                    eprintln!(
                        "DIAGNOSIS: $props interpolation broken. At gen col {}: '{}'. \
                         Output: '{}'",
                        gen_msg_expected,
                        &at_expected[..at_expected.len().min(20)],
                        output,
                    );
                }
            }
        }
    }
}

/// Props binding prefix sourcemap accuracy: `:title="myProp"` with Props binding.
/// The generated output has `__props.myProp`. The source map token for `myProp`
/// should point to the generated position of `myProp` (AFTER `__props.`), not to
/// `__props.` itself. This ensures hover at `myProp` in the Vue SFC resolves to the
/// correct prop type rather than the full `__props` object type.
#[test]
fn prop_binding_prefix_source_map_accuracy() {
    let source = r#"<template><div :title="myProp"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("myProp", BindingType::Props)]);

    // Positive: output should contain __props.myProp
    assert!(
        output.contains("__props.myProp"),
        "should apply __props prefix: {output}"
    );

    // Find source column of `myProp` in the :title attribute value
    let src_col = source.find("myProp").unwrap() as u32;

    // There should be a source map token whose source column points to `myProp`
    let token = tokens.iter().find(|&&(_, _, sc)| sc == src_col);
    assert!(
        token.is_some(),
        "source map must have a token for myProp at src col {src_col}. Tokens: {:?}",
        tokens
    );

    // The generated column of that token should point to `myProp` (after `__props.`),
    // not to `__props.` itself.
    let &(gen_line, gen_col, _) = token.unwrap();
    let lines: Vec<&str> = output.lines().collect();
    if let Some(line_str) = lines.get(gen_line as usize) {
        let at_gen = &line_str[gen_col as usize..];
        assert!(
            at_gen.starts_with("myProp"),
            "generated column {gen_col} should point to 'myProp', not '__props.'. \
             At gen col {gen_col}: '{}'. Full output: {output}",
            &at_gen[..at_gen.len().min(20)]
        );
    }
}

/// Props binding in template literal: `:class="\`prefix--${closeIconPosition}\`"`.
/// Same issue as above but within a template literal expression.
#[test]
fn prop_in_template_literal_source_map_accuracy() {
    let source = r#"<template><div :class="`prefix--${closeIconPosition}`"></div></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("closeIconPosition", BindingType::Props)]);

    // Positive: should apply __props prefix
    assert!(
        output.contains("__props.closeIconPosition"),
        "should apply __props prefix: {output}"
    );

    // Find source column of `closeIconPosition` in the template literal
    let src_col = source.find("closeIconPosition").unwrap() as u32;

    // There should be a source map token for closeIconPosition
    let token = tokens.iter().find(|&&(_, _, sc)| sc == src_col);
    assert!(
        token.is_some(),
        "source map must have a token for closeIconPosition at src col {src_col}. Tokens: {:?}",
        tokens
    );

    // The generated column should point to 'closeIconPosition', not '__props.'
    let &(gen_line, gen_col, _) = token.unwrap();
    let lines: Vec<&str> = output.lines().collect();
    if let Some(line_str) = lines.get(gen_line as usize) {
        let at_gen = &line_str[gen_col as usize..];
        assert!(
            at_gen.starts_with("closeIconPosition"),
            "generated column should point to 'closeIconPosition', not '__props.'. \
             At gen col {gen_col}: '{}'. Full output: {output}",
            &at_gen[..at_gen.len().min(30)]
        );
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

/// `.foo` v-bind prop-modifier shorthand without value: `.foo` ≡ `.foo="foo"`.
/// The generated VALUE identifier (inside `foo={…}`) must map back to the source
/// `foo` key token (after the `.`). Pre-fix the WHOLE prop span was overwritten
/// with `format!("{}={{{}}}", key, resolved)`, baking both name and value into one
/// `Overwritten` chunk anchored at `prop.start` (the `.`), so the value
/// identifier had NO token at the source `foo` start.
#[test]
fn dot_prop_shorthand_no_value_value_maps_to_source() {
    let source = r#"<template><Comp .foo/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("foo", BindingType::SetupConst)]);

    assert!(
        output.contains("foo={foo}"),
        "should convert .foo to foo={{foo}}: {output}"
    );

    let dot_src_col = source.find(".foo").unwrap() as u32;
    let key_src_col = dot_src_col + 1; // the `f` of the key token (after `.`)

    let pair_gen_col = output.find("foo={foo}").unwrap() as u32;
    let value_gen_col = pair_gen_col + "foo={".len() as u32;

    let value_maps_to_source = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == key_src_col);
    assert!(
        value_maps_to_source,
        "the generated VALUE identifier `foo` (gen col {value_gen_col}) must map to source col \
         {key_src_col} (the `f` in the `.foo` key). Pre-fix the whole `.foo` span was baked into a \
         mapped overwrite anchored at the `.` and had no such token. Tokens: {:?}",
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );

    // Negative: the value identifier must NOT collapse to the prop start (`.`).
    let value_maps_to_dot = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == dot_src_col);
    assert!(
        !value_maps_to_dot,
        "the generated VALUE identifier must not map to the `.` (col {dot_src_col}). \
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

// ── Slot outlet source map accuracy ─────────────────────────────

#[test]
fn slot_outlet_tag_name_source_mapped_to_slots() {
    // Hovering on `slot` in `<slot name="reference" />` should map to `$slots`
    // in the generated TSX, NOT to `?.()` or other synthetic regions.
    let source = r#"<template><slot name="reference" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Verify output shape
    assert!(output.contains("$slots"), "should contain $slots: {output}");
    assert!(
        output.contains(".reference"),
        "should contain .reference: {output}"
    );

    // Find source position of `s` in `<slot`
    let slot_src_col = source.find("<slot").unwrap() as u32 + 1; // position of `s`

    // Find the generated position of `$slots`
    let gen_slots_pos = output.find("$slots").unwrap() as u32;

    // The source map token at `s` should map to `$slots` in generated output,
    // NOT to positions past `$slots` (like `?.()`)
    let token_for_slot = tokens.iter().find(|&&(_, _, sc)| sc == slot_src_col);
    assert!(
        token_for_slot.is_some(),
        "should have source map token for `slot` tag name at src col {}. Tokens: {:?}",
        slot_src_col,
        tokens
    );

    let &(_, dst_col, _) = token_for_slot.unwrap();
    // dst_col should be within the `$slots` region, not past it
    assert!(
        dst_col >= gen_slots_pos && dst_col < gen_slots_pos + 6,
        "slot tag name should map to `$slots` region (gen cols {}..{}), got gen col {}. Output: {}",
        gen_slots_pos,
        gen_slots_pos + 6,
        dst_col,
        output
    );
}

#[test]
fn slot_outlet_name_attr_does_not_map_to_call_site() {
    // Positions within the `name="reference"` attribute should NOT map to `?.()`.
    // The slot name value `reference` should map to `.reference` in generated output.
    let source = r#"<template><slot name="reference" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Find source position of `reference` value (inside quotes)
    let ref_src_col = source.find("reference").unwrap() as u32;

    // Find generated position of `reference` (in `.reference`)
    let gen_ref_text = ".reference";
    let gen_ref_pos = output.find(gen_ref_text).unwrap() as u32;
    let gen_ref_start = gen_ref_pos + 1; // skip the `.`

    // The token for `reference` should map to the `.reference` region
    let token_for_ref = tokens.iter().find(|&&(_, _, sc)| sc == ref_src_col);
    assert!(
        token_for_ref.is_some(),
        "should have source map token for `reference` at src col {}. Tokens: {:?}",
        ref_src_col,
        tokens
    );

    let &(_, dst_col, _) = token_for_ref.unwrap();
    assert!(
        dst_col >= gen_ref_start && dst_col < gen_ref_start + 9,
        "reference should map to `.reference` region (gen cols {}..{}), got gen col {}. Output: {}",
        gen_ref_start,
        gen_ref_start + 9,
        dst_col,
        output
    );
}

#[test]
fn slot_outlet_no_interpolation_past_mapped_content() {
    // Simulates vue_to_tsx interpolation for the meaningful parts of the slot tag:
    // tag name (`slot`), attribute name (`name`), and attribute value (`reference`).
    // These positions must NOT land on the `(` of `?.()` — that causes `() any` hover.
    // Structural syntax (closing `"`, ` />`) may map to the `?.` operator, which is fine.
    let source = r#"<template><slot name="reference" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Find the generated position of `(` in `?.()` — this is where TSGO shows `() any`
    let call_paren_pos = output.find("?.()").unwrap() as u32 + 2; // position of `(`

    // Meaningful source positions: `<slot name="reference`
    // (excludes closing `"` and ` />` which are structural syntax)
    let tag_start = source.find("<slot").unwrap() as u32;
    let ref_end = source.find("reference").unwrap() as u32 + "reference".len() as u32;

    // Simulate vue_to_tsx for meaningful positions
    for query_col in tag_start..ref_end {
        let best = tokens
            .iter()
            .filter(|&&(_, _, sc)| sc <= query_col)
            .max_by_key(|&&(_, _, sc)| sc);

        if let Some(&(_, dst_col, src_col)) = best {
            let delta = query_col - src_col;
            let interpolated_dst = dst_col + delta;

            assert!(
                interpolated_dst < call_paren_pos,
                "source col {} interpolates to gen col {} (token src={} dst={} + delta={}), \
                 which is at/past `(` in `?.()` (gen col {}). This causes `() any` hover. Output: {}",
                query_col, interpolated_dst, src_col, dst_col, delta,
                call_paren_pos, output
            );
        }
    }
}

// ── Class/style merge source map accuracy ───────────────────────

#[test]
fn class_merge_dynamic_class_position_is_mapped() {
    // When both `class="foo"` and `:class="bar"` exist, the `:class` directive's
    // argument position should have a source map token pointing to the merged
    // `class={normalizeClass(...)}` attribute. The static `class` position is NOT
    // mapped in the codegen (the static attribute is removed from TSX); hover for
    // the static `class` is handled by the LSP hover handler which redirects the
    // TSGO query to the `:class` directive's position.
    let source = r#"<template><div class="foo" :class="bar"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Find source position of the `:` in `:class` (the directive start / overwrite origin)
    let colon_class_col = source.find(":class").unwrap() as u32;

    // Find generated position of the merged `class=` attribute
    let gen_class_pos = output.find("class=").unwrap() as u32;

    // The `:class` directive start should have a source map token mapping
    // to the merged `class=` in generated TSX. This is the redirect target
    // used by the hover handler for the static `class` attribute.
    let token_for_colon = tokens.iter().find(|&&(_, _, sc)| sc == colon_class_col);
    assert!(
        token_for_colon.is_some(),
        "`:class` at src col {} should have a source map token. \
         Generated output: {}. Tokens: {:?}",
        colon_class_col,
        output,
        tokens
    );

    let &(_, dst_col, _) = token_for_colon.unwrap();
    assert!(
        dst_col >= gen_class_pos && dst_col < gen_class_pos + 6,
        "`:class` should map to merged `class=` region (gen cols {}..{}), got gen col {}. Output: {}",
        gen_class_pos, gen_class_pos + 6, dst_col, output
    );
}

// ── v-for body member access (regression test) ──────────────────

/// v-for iteration variables must NOT get the `___VERTER___instance.` prefix
/// in TSX output. They are locally scoped via `.map((param) => ...)`.
#[test]
fn v_for_body_member_access_no_instance_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><button v-for="action in actions" :disabled="action.disabled">{{ action.label }}</button></template>"#,
        &[("actions", BindingType::SetupConst)],
    );
    eprintln!("TSX output:\n{}", result);

    // Positive: .map() wrapper present
    assert!(
        result.contains(".map((action"),
        "should have .map((action...) wrapper, got: {}",
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

    // Positive: both .map() wrappers
    assert!(
        result.contains(".map((user"),
        "outer .map((user...) expected, got: {}",
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

// ── Bug fix tests: verter-tsc false errors ──────────────────────────

#[test]
fn template_v_if_v_slot_no_orphan_iife_close() {
    // Bug: <template v-if v-slot> skips IIFE open but walker adds orphan }} close
    let result = gen_tsx_template(
        r#"<template><MyComp><template v-if="hasSlot" #indicator="bind"><slot name="indicator" /></template></MyComp></template>"#,
    );
    eprintln!("TSX output:\n{}", result);

    // Should not have orphan `}}` (IIFE close without matching open)
    // The JSX should be well-structured
    assert!(
        !result.contains("</>}}"),
        "should not have orphan IIFE close after slot template, got: {}",
        result
    );
}

#[test]
fn dynamic_component_closing_tag_no_attributes() {
    // Bug: </component :is="as"> leaks attributes onto JSX closing tag
    let result = gen_tsx_template(
        r#"<template><component :is="tag">child</component :is="tag"></template>"#,
    );
    eprintln!("TSX output:\n{}", result);

    // POSITIVE: should have the component render variable
    assert!(
        result.contains("___VERTER___component_render"),
        "should use component_render for dynamic :is, got: {}",
        result
    );

    // NEGATIVE: closing tag must NOT contain attributes
    assert!(
        !result.contains("</___VERTER___component_render :is"),
        "closing tag must not have :is attribute, got: {}",
        result
    );
    assert!(
        !result.contains(r#"</___VERTER___component_render "#),
        "closing tag must not have trailing content after tag name, got: {}",
        result
    );
}

#[test]
fn v_for_numeric_range_valid_tsx() {
    // Bug: v-for="i in 12" generates 12.map(...) which is invalid JS
    let result =
        gen_tsx_template(r#"<template><i v-for="i in 12" :key="i" class="line" /></template>"#);
    eprintln!("TSX output:\n{}", result);

    // POSITIVE: should have a .map() call
    assert!(
        result.contains(".map("),
        "should generate a .map() call, got: {}",
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
fn v_for_numeric_expression_range_valid_tsx() {
    // Bug: v-for="i in count + 1" where count+1 might be a numeric expression
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-for="i in count" :key="i">{{ i }}</div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    eprintln!("TSX output:\n{}", result);

    // Non-literal iterables should still work with .map()
    assert!(
        result.contains(".map("),
        "should generate .map() for non-literal iterables, got: {}",
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

#[test]
fn dynamic_component_inside_jsx_children_valid_tsx() {
    // Bug: <component :is> inside another element puts const in JSX children
    let result = gen_tsx_template_with_bindings(
        r#"<template><div><component :is="tag" /></div></template>"#,
        &[("tag", BindingType::SetupConst)],
    );
    eprintln!("TSX output:\n{}", result);

    // The const statement for extractRenderComponent must be in valid JS context,
    // not inside JSX element children where it would be treated as text
    // Valid patterns:
    //   {(() => { const comp = ...; return <comp />; })()}
    //   Block scope before JSX
    // Invalid: <div>const comp = ...; <comp /></div>
    assert!(
        !result.contains(">const ___VERTER___component_render"),
        "const statement must not appear as JSX text children, got: {}",
        result
    );
}

#[test]
fn slot_props_kebab_case_quoted() {
    // Bug: slot scope props with kebab-case names generate unquoted property names
    // e.g., { item-class: "value" } which is invalid JS (item minus class)
    let result = gen_tsx_template(
        r#"<template><MyComp><template #default="{ itemClass }"><slot :item-class="itemClass" /></template></MyComp></template>"#,
    );
    eprintln!("TSX output:\n{}", result);

    // If slot props contain kebab-case keys, they must be quoted
    // This test verifies we don't generate unquoted hyphenated property names
    if result.contains("item-class") {
        assert!(
            result.contains(r#""item-class""#) || result.contains("'item-class'"),
            "kebab-case slot prop key must be quoted in JS object literal, got: {}",
            result
        );
    }
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
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, source_type).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Generated TSX should have no parse errors. Got {} errors. Output:\n{}",
        parsed.errors.len(),
        result
    );
}

/// Regression: complex template (notification-like) with transition, v-show, component :is,
/// v-text, and v-html must produce valid TSX without syntax errors.
#[test]
fn notification_template_complex_no_syntax_errors() {
    let source = r#"<template>
  <transition
    :name="ns.b('fade')"
    @before-leave="onClose"
    @after-leave="$emit('destroy')"
  >
    <div
      v-show="visible"
      :id="id"
      :class="[ns.b(), customClass, horizontalClass]"
      :style="positionStyle"
      role="alert"
      @mouseenter="clearTimer"
      @mouseleave="startTimer"
      @click="onClick"
    >
      <el-icon v-if="iconComponent" :class="[ns.e('icon'), typeClass]">
        <component :is="iconComponent" />
      </el-icon>
      <div :class="ns.e('group')">
        <h2 :class="ns.e('title')" v-text="title" />
        <div
          v-show="message"
          :class="ns.e('content')"
          :style="!!title ? undefined : { margin: 0 }"
        >
          <slot>
            <p v-if="!dangerouslyUseHTMLString">{{ message }}</p>
            <!-- Caution here, message could've been compromised, never use user's input as message -->
            <p v-else v-html="message" />
          </slot>
        </div>
        <el-icon v-if="showClose" :class="ns.e('closeBtn')" @click.stop="close">
          <component :is="closeIcon" />
        </el-icon>
      </div>
    </div>
  </transition>
</template>"#;
    let result = gen_tsx_template(source);
    eprintln!("=== NOTIFICATION TEMPLATE TSX ===\n{}\n=== END ===", result);

    // Negative: no stray leaked binding prefixes
    assert!(
        !result.contains("}}___VERTER___instance."),
        "binding prefix must not leak after style closing braces. Got:\n{}",
        result
    );

    // Parse the result with OXC to check for syntax errors
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::tsx();
    let wrapped = format!("import {{}} from 'vue';\n{}", result);
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, source_type).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Generated TSX should have no parse errors. Got {} errors. Output:\n{}",
        parsed.errors.len(),
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
    let parsed = oxc_parser::Parser::new(&alloc, &result, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Generated TSX should have no parse errors. Got {} errors. Output:\n{}",
        parsed.errors.len(),
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
    let options = crate::compile::CodegenOptions {
        filename: Some("BalCard.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== FULL SFC TSX ===\n{}\n=== END ===", tsx.code);

    // Parse with OXC to verify valid TSX
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Full SFC TSX should have no parse errors. Got {} errors. Output:\n{}",
        parsed.errors.len(),
        tsx.code
    );
}

#[test]
fn balcard_vue_full_sfc_produces_valid_tsx() {
    let source = match std::fs::read_to_string("d:/dev/github/verter-test-repos/balancer-frontend-v2/src/components/_global/BalCard/BalCard.vue") {
        Ok(s) => s,
        Err(_) => {
            eprintln!("SKIP: BalCard.vue not found (test repo not available)");
            return;
        }
    };
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("BalCard.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    let result = crate::compile::compile(&source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== BALCARD FULL TSX ===\n{}\n=== END ===", tsx.code);

    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "BalCard TSX should have no parse errors. Got {} errors",
        parsed.errors.len(),
    );
}

#[test]
fn custom_docs_block_before_template_produces_valid_tsx() {
    let source = r#"<docs>
---
order: 0
title:
  zh-CN: 基本用法
---
## Notes
</docs>

<template>
  <div>hello</div>
</template>
<script lang="ts" setup>
import { ref } from 'vue';
const checked = ref<boolean>(false);
</script>"#;
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("Basic.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== DOCS BLOCK TSX ===\n{}\n=== END ===", tsx.code);

    // Custom block content should not appear in TSX
    assert!(
        !tsx.code.contains("order: 0"),
        "Custom block content should not leak into TSX"
    );

    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "TSX with custom block should have no parse errors. Got {} errors.\nOutput:\n{}",
        parsed.errors.len(),
        tsx.code
    );
}

#[test]
fn ant_design_switch_basic_produces_valid_tsx() {
    let source = match std::fs::read_to_string(
        "d:/dev/github/verter-test-repos/ant-design-vue/components/switch/demo/basic.vue",
    ) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("SKIP: basic.vue not found");
            return;
        }
    };
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("basic.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(&source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== ANT BASIC TSX ===\n{}\n=== END ===", tsx.code);

    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len(),
    );
}

#[test]
fn activist_card_topic_selection_produces_valid_tsx() {
    let source = match std::fs::read_to_string("d:/dev/github/verter-test-repos/activist-org-activist/frontend/app/components/card/CardTopicSelection.vue") {
        Ok(s) => s,
        Err(_) => { eprintln!("SKIP: file not found"); return; }
    };
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("CardTopicSelection.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(&source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== ACTIVIST TSX ===\n{}\n=== END ===", tsx.code);
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len()
    );
}

#[test]
fn activist_machine_steps_produces_valid_tsx() {
    let source = match std::fs::read_to_string("d:/dev/github/verter-test-repos/activist-org-activist/frontend/app/components/MachineStepsCreateEventTime.vue") {
        Ok(s) => s,
        Err(_) => { eprintln!("SKIP: file not found"); return; }
    };
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("MachineStepsCreateEventTime.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(&source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== MACHINE STEPS TSX ===\n{}\n=== END ===", tsx.code);
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len()
    );
}

/// <component :is="..."> should not generate a ___VERTER___Comp function with
/// `instantiateComponent(component, {})` — `component` is not a valid variable.
#[test]
fn component_is_dynamic_no_comp_function() {
    let source = r#"<template>
  <component :is="tag" />
</template>
<script setup lang="ts">
const tag = 'div';
</script>"#;
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");

    // Must NOT contain instantiateComponent(component, ...)
    assert!(
        !tsx.code.contains("instantiateComponent(component"),
        "Should not emit Comp function for <component :is>. Got:\n{}",
        tsx.code
    );

    // Parse to ensure valid TSX
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len()
    );
}

#[test]
fn nexus_notification_produces_valid_tsx() {
    let source = match std::fs::read_to_string("d:/dev/accioresearch/WLS/nexus/nexus-ui/packages/ui/src/components/Notifications/components/Notification.vue") {
        Ok(s) => s,
        Err(_) => { eprintln!("SKIP: file not found"); return; }
    };
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("Notification.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(&source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    // ___VERTER___props must be declared (not just referenced)
    assert!(
        tsx.code.contains("const ___VERTER___props"),
        "Destructured defineProps should declare ___VERTER___props. Got:\n{}",
        tsx.code
    );

    // Parse with OXC to verify valid TSX
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len(),
    );
}

/// Destructured defineProps (`const { foo } = defineProps<{...}>()`) should
/// declare ___VERTER___props so that `const __props = ___VERTER___props` resolves.
#[test]
fn destructured_define_props_declares_verter_props() {
    let source = r#"<script setup lang="ts">
const { msg, count } = defineProps<{
  msg: string
  count: number
}>()
</script>
<template>
  <div>{{ msg }} {{ count }}</div>
</template>"#;
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");

    // ___VERTER___props must be declared, not just referenced
    assert!(
        tsx.code.contains("const ___VERTER___props"),
        "Should declare ___VERTER___props for destructured defineProps. Got:\n{}",
        tsx.code
    );

    // Original destructured pattern should NOT remain
    assert!(
        !tsx.code.contains("const { msg, count }"),
        "Destructuring pattern should be rewritten. Got:\n{}",
        tsx.code
    );

    // Parse to ensure valid TSX
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len()
    );
}

#[test]
fn nexus_bloc_produces_valid_tsx() {
    let source = match std::fs::read_to_string(
        "d:/dev/accioresearch/WLS/nexus/nexus-ui/packages/ui/src/components/atom/Bloc/Bloc.vue",
    ) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("SKIP: file not found");
            return;
        }
    };
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("Bloc.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(&source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== BLOC TSX ===\n{}\n=== END ===", tsx.code);
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len()
    );
}

#[test]
fn runtime_define_props_in_template_scope() {
    // Runtime defineProps({...}) without assignment should expose prop names
    // in the template scope. TS2304 "Cannot find name" if they're not.
    let source = r#"<template>
  <div v-if="showBoard">
    <router-link :to="`/boards/${url}`">{{ name }}</router-link>
  </div>
</template>

<script setup lang="ts">
defineProps({
  name: { type: String, required: true },
  url: { type: String, required: true },
  showBoard: { type: Boolean, required: true },
});
</script>"#;
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("BoardBadge.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== RUNTIME PROPS TSX ===\n{}\n=== END ===", tsx.code);

    // Positive: props should be accessible via __props in template
    assert!(
        tsx.code.contains("__props.showBoard"),
        "showBoard should be accessed via __props in template, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("__props.url") || tsx.code.contains("__props.name"),
        "url/name should be accessed via __props in template, got:\n{}",
        tsx.code
    );

    // Negative: Comp function condition guards must also use __props
    // (TS2304 "Cannot find name 'showBoard'" if bare)
    assert!(
        !tsx.code.contains("if(!((showBoard)))"),
        "Comp function guard must NOT use bare 'showBoard' — should be __props.showBoard, got:\n{}",
        tsx.code
    );

    // OXC validation
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "Full TSX should parse without errors. Got {} errors:\n{}",
        parsed.errors.len(),
        tsx.code
    );
}

#[test]
fn closing_tag_case_mismatch_component() {
    // Vue is case-insensitive for closing tags: <Button>...</button> is valid.
    // JSX is case-sensitive: the closing tag must match the opening tag.
    // Verter must rewrite the closing tag to match the opening tag.
    let result = gen_tsx_template_with_bindings(
        r#"<template>
  <Button class="btn">Click</Button>
  <Button class="btn2">Click2</button>
</template>"#,
        &[("Button", BindingType::SetupConst)],
    );
    eprintln!("=== CASE MISMATCH ===\n{}\n=== END ===", result);

    // Positive: both buttons should have matching closing tags
    let close_count = result.matches("</Button>").count();
    assert!(
        close_count == 2,
        "should have 2 </Button> closing tags (case-corrected), got {} in:\n{}",
        close_count,
        result
    );

    // Negative: lowercase </button> should not appear
    assert!(
        !result.contains("</button>"),
        "lowercase </button> should be rewritten to </Button>, got:\n{}",
        result
    );
}

// ── Kebab-case event handling in spread syntax ─────────────────────────────

#[test]
fn kebab_event_with_dollar_event_wraps_with_event_param() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click-overlay="emit('clickOverlay', $event)" /></template>"#,
        &[("emit", BindingType::SetupConst)],
    );
    // Kebab event → spread syntax with eventCallbacks wrapper
    assert!(
        result.contains("___VERTER___eventCallbacks"),
        "should use eventCallbacks wrapper: {result}"
    );
    assert!(
        result.contains(r#""onClick-overlay""#),
        "should preserve kebab-case event name: {result}"
    );
    assert!(
        result.contains("...___VERTER___eventArgs"),
        "should have rest args for eventCallbacks: {result}"
    );
    // Negative: should NOT have bare ($event) => without eventCallbacks
    assert!(
        !result.contains(r#"": ($event) =>"#),
        "should NOT use bare ($event) => pattern: {result}"
    );
}

#[test]
fn kebab_event_arrow_function_is_raw() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click-overlay="($event) => doSomething($event)" /></template>"#,
        &[("doSomething", BindingType::SetupConst)],
    );
    // Arrow function should be passed raw in spread — no extra wrapping
    assert!(
        result.contains(r#""onClick-overlay": ($event) => doSomething($event)"#),
        "arrow function should be raw in spread: {result}"
    );
    // Negative: should NOT double-wrap
    assert!(
        !result.contains("($event) => {($event)"),
        "should NOT double-wrap arrow function: {result}"
    );
}

#[test]
fn kebab_event_function_expr_is_raw() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click-overlay="function($event) { doSomething($event) }" /></template>"#,
        &[("doSomething", BindingType::SetupConst)],
    );
    // Function expression should be passed raw in spread
    assert!(
        result.contains(r#""onClick-overlay": function($event)"#),
        "function expression should be raw in spread: {result}"
    );
}

#[test]
fn kebab_event_inline_expr_no_dollar_event_wraps_with_no_param() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click-overlay="count++" /></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    // Inline expression without $event → () => { ... }
    assert!(
        result.contains("() => {"),
        "should wrap with () => for inline expr without $event: {result}"
    );
    assert!(
        result.contains(r#""onClick-overlay""#),
        "should preserve kebab-case event name: {result}"
    );
}

// ── Fix 1: Broken interpolation recovery ──────────────────────────

#[test]
fn broken_interpolation_preserves_identifiers() {
    // Broken expression: {{ count + }} — OXC can't parse it, but identifiers must survive
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ count + }}</div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    eprintln!("broken interpolation output: {}", result);
    // Positive: identifiers preserved
    assert!(
        result.contains("count"),
        "broken expression should preserve identifiers: {result}"
    );
    // Positive: mustache delimiters converted
    assert!(
        result.contains('{') && result.contains('}') && result.contains("count"),
        "mustache should be converted to a JSX expression with preserved identifiers: {result}"
    );
    // Negative: no raw mustache delimiters
    assert!(
        !result.contains("{{") && !result.contains("}}"),
        "mustache delimiters must be converted to JSX: {result}"
    );
    assert_valid_tsx(&result, "broken-interpolation");
}

#[test]
fn broken_interpolation_keeps_identifier_source_map_anchor() {
    let source = r#"<template><div>{{ count + }}</div></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupConst)]);

    let count_src_col = source.find("count").unwrap() as u32;
    let anchor = tokens
        .iter()
        .filter(|&&(_, _, src_col)| src_col <= count_src_col)
        .max_by_key(|&&(_, _, src_col)| src_col)
        .copied();

    assert!(
        anchor.is_some(),
        "broken interpolation should retain a usable source-map anchor before 'count', tokens: {:?}",
        tokens
    );

    let (_gen_line, gen_col, anchor_src_col) = anchor.unwrap();
    let mapped_col = gen_col + (count_src_col - anchor_src_col);
    let first_line = output.lines().next().unwrap_or("");
    assert!(
        first_line
            .get(mapped_col as usize..)
            .is_some_and(|suffix| suffix.starts_with("count")),
        "broken interpolation should keep linear mapping from the nearest anchor to 'count', got output: {output}, anchor={anchor:?}"
    );
}

#[test]
fn valid_interpolations_unaffected_by_broken_expr_handling() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ count }}</div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    // Valid expression should still work normally (whitespace from source is preserved)
    assert!(
        result.contains("{ count }") || result.contains("{count}"),
        "valid interpolation should produce {{count}}: {result}"
    );
    assert!(
        !result.contains("{{") && !result.contains("}}"),
        "no raw mustache delimiters: {result}"
    );
}

#[test]
fn mixed_broken_and_valid_interpolations() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ count + }}<span>{{ count }}</span></div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    eprintln!("mixed output: {}", result);
    // Valid expression should be fully patched (whitespace from source is preserved)
    assert!(
        result.contains("{ count }") || result.contains("{count}"),
        "valid interpolation should be patched: {result}"
    );
    // Broken expression: identifiers preserved
    assert!(
        result.contains("count"),
        "broken expression should still preserve identifiers: {result}"
    );
    // No raw mustache delimiters anywhere
    assert!(
        !result.contains("{{") && !result.contains("}}"),
        "no raw mustache delimiters: {result}"
    );
    assert_valid_tsx(&result, "mixed-broken-and-valid-interpolations");
}

// ── Fix 3: v-slot scoped parameter typing ─────────────────────────

#[test]
fn v_slot_params_arrow_wrapper() {
    // Component v-slot with params: should generate IIFE with extractArgumentsFromRenderSlot
    let result = gen_tsx_template(
        r#"<template><MyComp v-slot="{ slotItem }"><span>{{ slotItem }}</span></MyComp></template>"#,
    );
    eprintln!("v-slot params output: {}", result);
    // Positive: should have arrow function wrapper for slot params
    assert!(
        result.contains("{ slotItem }") || result.contains("{slotItem}"),
        "should contain slot params in arrow function: {result}"
    );
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "should use extractArgumentsFromRenderSlot for slot typing: {result}"
    );
    assert!(
        result.contains("instantiateComponent"),
        "should use instantiateComponent for component instance: {result}"
    );
    assert!(
        result.contains(r#""default""#),
        "should reference default slot name: {result}"
    );
    // Negative: v-slot attribute must not appear
    assert!(
        !result.contains("v-slot"),
        "v-slot attribute must be removed: {result}"
    );
    assert!(
        result.contains("const { slotItem } = ___VERTER___extractArgumentsFromRenderSlot")
            || result.contains("const {slotItem} = ___VERTER___extractArgumentsFromRenderSlot"),
        "slot params should bind from the typed slot extract result, got: {result}"
    );
    assert!(
        !result.contains("function({ slotItem })") && !result.contains("function({slotItem})"),
        "slot params should not be introduced as untyped function parameters, got: {result}"
    );
}

#[test]
fn v_slot_named_template_params() {
    // <template #header="{ title }"> should generate typed wrapper with "header"
    let result = gen_tsx_template(
        r#"<template><MyComp><template #header="{ title }"><span>{{ title }}</span></template></MyComp></template>"#,
    );
    eprintln!("named template v-slot output: {}", result);
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "should use extractArgumentsFromRenderSlot: {result}"
    );
    assert!(
        result.contains(r#""header""#),
        "should reference header slot name: {result}"
    );
    // Negative
    assert!(
        !result.contains("#header") && !result.contains("v-slot:header"),
        "v-slot directive must be removed: {result}"
    );
}

#[test]
fn v_slot_default_template_params() {
    // <template v-slot="{ data }"> should use "default" slot name
    let result = gen_tsx_template(
        r#"<template><MyComp><template v-slot="{ data }"><span>{{ data }}</span></template></MyComp></template>"#,
    );
    eprintln!("default template v-slot output: {}", result);
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "should use extractArgumentsFromRenderSlot: {result}"
    );
    assert!(
        result.contains(r#""default""#),
        "should use default slot name: {result}"
    );
}

#[test]
fn v_slot_multiple_named_templates() {
    // Multiple named slots — each gets independent IIFE, params don't leak
    let result = gen_tsx_template(
        r#"<template><MyComp><template #header="{ x }"><span>{{ x }}</span></template><template #footer="{ y }"><span>{{ y }}</span></template></MyComp></template>"#,
    );
    eprintln!("multi-slot output: {}", result);
    assert!(
        result.contains(r#""header""#) && result.contains(r#""footer""#),
        "should reference both slot names: {result}"
    );
    // Count extractArgumentsFromRenderSlot calls — should be 2
    let count = result.matches("extractArgumentsFromRenderSlot").count();
    assert_eq!(
        count, 2,
        "should have 2 extractArgumentsFromRenderSlot calls: {result}"
    );
}

#[test]
fn v_slot_no_params_unchanged() {
    // v-slot without params: no wrapper needed
    let result =
        gen_tsx_template(r#"<template><MyComp v-slot><span>content</span></MyComp></template>"#);
    eprintln!("v-slot no params output: {}", result);
    assert!(
        !result.contains("extractArgumentsFromRenderSlot"),
        "no wrapper for v-slot without params: {result}"
    );
    assert!(
        !result.contains("v-slot"),
        "v-slot attribute must be removed: {result}"
    );
}

#[test]
fn v_slot_params_no_instance_prefix() {
    // Slot params must NOT get ___VERTER___instance. prefix
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp v-slot="{ slotItem }"><span>{{ slotItem }}</span></MyComp></template>"#,
        &[("slotItem", BindingType::SetupConst)], // Even if in bindings, slot takes priority
    );
    assert!(
        !result.contains("___VERTER___instance.slotItem"),
        "slot param must NOT get instance prefix: {result}"
    );
}

#[test]
fn partial_v_slot_param_stays_bare_for_completion() {
    let result = gen_tsx_template(
        r#"<template><MyComp v-slot="{ slotItem, slotIndex, slotTotal }"><span>{{ sl }}</span></MyComp></template>"#,
    );
    assert!(
        result.contains("{ sl }") || result.contains("{sl}"),
        "partial slot param should stay bare for completion context, got: {result}"
    );
    assert!(
        !result.contains("___VERTER___instance.sl"),
        "partial slot param must not get instance prefix, got: {result}"
    );
}

#[test]
fn v_slot_with_v_for() {
    // v-for wraps element, slot wraps children — both should work
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp v-for="item in items" :key="item.id" v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
        &[("items", BindingType::SetupConst)],
    );
    eprintln!("v-for + v-slot output: {}", result);
    // Both v-for map and v-slot IIFE should be present
    assert!(
        result.contains(".map("),
        "v-for should produce .map(): {result}"
    );
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "v-slot should produce extractArgumentsFromRenderSlot: {result}"
    );
}

// ── Object literal in binding: prop name as key must not be rewritten ────

fn compile_full_sfc_tsx(source: &str, filename: &str) -> String {
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some(filename.to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    tsx.code.clone()
}

fn assert_valid_tsx(code: &str, label: &str) {
    let alloc = Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("[{label}] OXC ERROR: {err}");
    }
    assert!(
        parsed.errors.is_empty(),
        "[{label}] TSX should have no parse errors. Got {} errors. Output:\n{code}",
        parsed.errors.len()
    );
}

#[test]
fn object_literal_binding_prop_key_not_rewritten() {
    // Bug: `:overlay-style="{ zIndex: zIndex - 2 }"` where `zIndex` is a prop
    // causes `resolve_all_prop_refs_in_expr` to produce `__props.zIndex: __props.zIndex - 2`
    // which is invalid JS (can't have dots in object keys without quotes).
    let source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
const props = defineProps<{ zIndex: number }>()
</script>
<template>
  <MyComp :overlay-style="{ zIndex: zIndex - 2 }" />
</template>"#;
    let code = compile_full_sfc_tsx(source, "Test.vue");
    eprintln!("Object key test TSX:\n{code}");

    // Should parse without errors (the core assertion)
    assert_valid_tsx(&code, "object-key-prop");

    // Negative: should NOT have __props.zIndex: (invalid object key)
    assert!(
        !code.contains("__props.zIndex:"),
        "object key must NOT be prefixed with __props.: {code}"
    );
}

// ── JSX helper ─────────────────────────────────────────────

fn gen_jsx_template(source: &str) -> String {
    let alloc = Allocator::new();
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let template_ast = match syntax.take_template_ast() {
        Some(ast) => ast,
        None => return String::new(),
    };

    let source_type = oxc_span::SourceType::tsx();
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        source_type,
    );

    let mut tpl_ct = CodeTransform::new(source, &alloc);
    let mut out = CodeGenOutput::new(&alloc);
    let bindings = FxHashMap::default();
    let options = IdeTemplateOptions {
        self_name: "App",
        comments: true,
        is_jsx: true,
        strict_slots: false,
    };

    generate_ide_template(
        &template_ast,
        &oxc_ast,
        source,
        &mut out,
        &alloc,
        &bindings,
        &options,
    );
    out.apply_to(&mut tpl_ct);

    let full = tpl_ct.build_string();
    let tpl_start = template_ast.root.tag_open.start as usize;
    let tpl_end = template_ast
        .root
        .tag_close
        .as_ref()
        .map(|tc| tc.end as usize)
        .unwrap_or(full.len());
    let suffix_len = source.len() - tpl_end;
    full[tpl_start..full.len() - suffix_len].to_string()
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
    assert!(
        result.contains(r#""bar":true"#),
        "should have bar modifier: {result}"
    );
    assert!(
        result.contains(r#""baz":true"#),
        "should have baz modifier: {result}"
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
        result.contains(r#"baz,"foo",{"bar":true}"#),
        "should have value, static arg, and modifier object: {result}"
    );

    // Negative: raw directive must not appear
    assert!(
        !result.contains("v-test:foo"),
        "raw v-test:foo must be removed: {result}"
    );
}

#[test]
fn custom_directive_on_component() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp v-focus /></template>"#,
        &[("MyComp", BindingType::SetupConst)],
    );
    eprintln!("custom_directive_on_component:\n{result}");

    // Should work the same on components
    assert!(
        result.contains("v-directive="),
        "should emit v-directive on component: {result}"
    );
    assert!(
        result.contains(r#"directiveAccessor["vFocus"]"#),
        "should reference vFocus: {result}"
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

// ── @ts-expect-error / @ts-ignore in template comments ──────────────────────

#[test]
fn ts_expect_error_before_component() {
    let result = gen_tsx_template(r#"<template><!-- @ts-expect-error --><MyComp/></template>"#);
    // Comment should appear as JSX comment before the component
    assert!(
        result.contains("{/* @ts-expect-error */}"),
        "should have TS directive comment, got:\n{}",
        result
    );
    // Comment must appear before the component tag
    let comment_pos = result.find("{/* @ts-expect-error */}").unwrap();
    let comp_pos = result.find("<MyComp").unwrap();
    assert!(
        comment_pos < comp_pos,
        "comment should appear before component, got:\n{}",
        result
    );
    // No raw HTML comment markers in output
    assert!(
        !result.contains("<!--"),
        "should not have raw HTML comment markers, got:\n{}",
        result
    );
    assert!(
        !result.contains("-->"),
        "should not have raw HTML comment close, got:\n{}",
        result
    );
}

#[test]
fn ts_expect_error_before_v_for() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><div v-for="x in xs">{{ x }}</div></template>"#,
        &[("xs", BindingType::SetupRef)],
    );
    // v-for wraps in .map() — the comment must be INSIDE the map callback
    assert!(
        result.contains(".map("),
        "should have .map() wrapper, got:\n{}",
        result
    );
    let map_pos = result.find(".map(").unwrap();
    // Comment must be present (as JSX comment with TS directive)
    assert!(
        result.contains("@ts-expect-error"),
        "TS directive comment should be present, got:\n{}",
        result
    );
    let comment_pos = result.find("@ts-expect-error").unwrap();
    assert!(
        comment_pos > map_pos,
        "comment should be inside .map() callback, not before it, got:\n{}",
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
fn ts_expect_error_before_component_is() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><component :is="comp"/></template>"#,
        &[("comp", BindingType::SetupRef)],
    );
    // <component :is> wraps in IIFE — comment must be inside the IIFE
    assert!(
        result.contains("extractRenderComponent"),
        "should have extractRenderComponent IIFE, got:\n{}",
        result
    );
    // Comment should be inside the IIFE (after the IIFE open)
    let iife_pos = result.find("(() =>").unwrap();
    // Check that a TS directive comment appears somewhere after the IIFE open
    let after_iife = &result[iife_pos..];
    assert!(
        after_iife.contains("@ts-expect-error"),
        "TS directive comment should be inside component :is IIFE, got:\n{}",
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
    // v-for + v-if: v-for is outer (.map), v-if uses ternary inside
    assert!(
        result.contains(".map("),
        "should have .map() wrapper, got:\n{}",
        result
    );
    let map_pos = result.find(".map(").unwrap();
    assert!(
        result.contains("@ts-expect-error"),
        "TS directive comment should be present, got:\n{}",
        result
    );
    let comment_pos = result.find("@ts-expect-error").unwrap();
    assert!(
        comment_pos > map_pos,
        "comment should be inside .map() callback, got:\n{}",
        result
    );
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

#[test]
fn ts_ignore_same_behavior() {
    let result = gen_tsx_template(r#"<template><!-- @ts-ignore --><MyComp/></template>"#);
    assert!(
        result.contains("{/* @ts-ignore */}"),
        "should have @ts-ignore comment, got:\n{}",
        result
    );
    let comment_pos = result.find("{/* @ts-ignore */}").unwrap();
    let comp_pos = result.find("<MyComp").unwrap();
    assert!(
        comment_pos < comp_pos,
        "@ts-ignore should appear before component, got:\n{}",
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
    // Regular (non-TS-directive) comment should NOT be repositioned inside .map()
    assert!(
        result.contains("{/* hello */}"),
        "regular comment should be converted to JSX, got:\n{}",
        result
    );
    // Comment should stay at its original position (before .map)
    let comment_pos = result.find("{/* hello */}").unwrap();
    let map_pos = result.find(".map(").unwrap();
    assert!(
        comment_pos < map_pos,
        "regular comment should stay before .map(), not be repositioned inside, got:\n{}",
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
    let iife_pos = result.find("{()=>{").expect("should have IIFE open");
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

// ── Strict slot children type checking ──────────────────────

/// Helper: compile a template with strict_slots enabled.
/// Returns the template portion of the TSX output.
#[allow(dead_code)]
fn gen_tsx_template_strict_slots(source: &str) -> String {
    gen_tsx_template_strict_slots_with_bindings(source, &[])
}

fn gen_tsx_template_strict_slots_with_bindings(
    source: &str,
    bindings: &[(&str, BindingType)],
) -> String {
    let alloc = Allocator::new();
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let template_ast = match syntax.take_template_ast() {
        Some(ast) => ast,
        None => return String::new(),
    };

    let source_type = oxc_span::SourceType::tsx();
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        source_type,
    );

    let tpl_alloc = Allocator::new();
    let mut tpl_ct = CodeTransform::new(source, &tpl_alloc);
    let mut out = CodeGenOutput::new(&tpl_alloc);

    let mut binding_map: FxHashMap<&str, BindingType> = FxHashMap::default();
    for &(name, bt) in bindings {
        binding_map.insert(tpl_alloc.alloc_str(name), bt);
    }

    let options = IdeTemplateOptions {
        self_name: "App",
        comments: true,
        is_jsx: false,
        strict_slots: true,
    };

    generate_ide_template(
        &template_ast,
        &oxc_ast,
        source,
        &mut out,
        &tpl_alloc,
        &binding_map,
        &options,
    );
    out.apply_to(&mut tpl_ct);

    let full = tpl_ct.build_string();
    let tpl_start = template_ast.root.tag_open.start as usize;
    let tpl_end = template_ast
        .root
        .tag_close
        .as_ref()
        .map(|tc| tc.end as usize)
        .unwrap_or(full.len());
    let suffix_len = source.len() - tpl_end;
    full[tpl_start..full.len() - suffix_len].to_string()
}

#[test]
fn strict_slots_component_children() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs><TabItem /><TabItem /></Tabs></template>",
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );
    // Positive: strictRenderSlot call with default slot and TabItem children
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot call, got:\n{}",
        result
    );
    assert!(
        result.contains("$slots"),
        "should reference $slots, got:\n{}",
        result
    );
    assert!(
        result.contains("'default'"),
        "should reference default slot, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should reference TabItem constructor, got:\n{}",
        result
    );
    // Negative: no v-if or v-for artifacts in slot check
    assert!(
        !result.contains("v-if"),
        "v-if should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_html_children() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs><input /><span></span></Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Positive: strictRenderSlot with HTML element type references
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot call, got:\n{}",
        result
    );
    assert!(
        result.contains("HTMLElementTagNameMap"),
        "should reference HTMLElementTagNameMap, got:\n{}",
        result
    );
    assert!(
        result.contains("\"input\""),
        "should reference input element, got:\n{}",
        result
    );
    assert!(
        result.contains("\"span\""),
        "should reference span element, got:\n{}",
        result
    );
    // Negative
    assert!(
        !result.contains("v-slot"),
        "no v-slot in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_text_children() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs>hello world</Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Positive: strictRenderSlot with string type for text
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot for text, got:\n{}",
        result
    );
    assert!(
        result.contains("as string"),
        "should have string type for text node, got:\n{}",
        result
    );
    // Negative
    assert!(
        !result.contains("HTMLElementTagNameMap"),
        "should not have HTMLElementTagNameMap for text, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_named_slot() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs><template #header><input /></template></Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Positive: strictRenderSlot referencing named slot 'header'
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("'header'"),
        "should reference header slot name, got:\n{}",
        result
    );
    // Negative: should NOT have 'default' slot call
    assert!(
        !result.contains("'default'"),
        "should not have default slot (only named), got:\n{}",
        result
    );
}

#[test]
fn strict_slots_mixed_named_default() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs><template #header><input /></template><template #default><span /></template></Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Positive: two separate strictRenderSlot calls
    assert!(
        result.contains("'header'"),
        "should have header slot, got:\n{}",
        result
    );
    assert!(
        result.contains("'default'"),
        "should have default slot, got:\n{}",
        result
    );
    // Count occurrences of strictRenderSlot
    let count = result.matches("strictRenderSlot").count();
    assert!(
        count >= 2,
        "should have at least 2 strictRenderSlot calls, got {}, output:\n{}",
        count,
        result
    );
}

#[test]
fn strict_slots_no_children() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs /></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Negative: no strictRenderSlot for self-closing components
    assert!(
        !result.contains("strictRenderSlot"),
        "should NOT emit strictRenderSlot for self-closing, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_whitespace_only() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs>   \n   </Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Negative: no strictRenderSlot for whitespace-only children
    assert!(
        !result.contains("strictRenderSlot"),
        "should NOT emit strictRenderSlot for whitespace-only children, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_dynamic_component() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><component :is="comp"><span /></component></template>"#,
        &[("comp", BindingType::SetupRef)],
    );
    // Negative: no strictRenderSlot for dynamic <component :is>
    assert!(
        !result.contains("strictRenderSlot"),
        "should NOT emit strictRenderSlot for dynamic component, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_disabled() {
    let result = gen_tsx_template_with_bindings(
        "<template><Tabs><TabItem /></Tabs></template>",
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );
    // Negative: no strictRenderSlot when strict_slots is false (default helper)
    assert!(
        !result.contains("strictRenderSlot"),
        "should NOT emit strictRenderSlot when disabled, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_if_child() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs><TabItem v-if="show" /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
            ("show", BindingType::SetupRef),
        ],
    );
    // Positive: TabItem still in the strict slot check (v-if doesn't change the type)
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should contain TabItem in slot check, got:\n{}",
        result
    );
    // Negative
    assert!(
        !result.contains("v-if"),
        "v-if should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_for_child() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs><TabItem v-for="i in 3" /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );
    // Positive: TabItem still in the strict slot check
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should contain TabItem, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_interpolation_child() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs>{{ msg }}</Tabs></template>",
        &[
            ("Tabs", BindingType::SetupImport),
            ("msg", BindingType::SetupRef),
        ],
    );
    // Positive: strictRenderSlot with string type for interpolation
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot for interpolation, got:\n{}",
        result
    );
    assert!(
        result.contains("as string"),
        "should have string type for interpolation, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_with_v_slot_params() {
    // When a component has v-slot params, BOTH extractArgumentsFromRenderSlot
    // (for slot props typing) AND strictRenderSlot (for children checking) should appear.
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs v-slot="{ item }"><TabItem /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );
    // Positive: both helpers present
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "should have extractArgumentsFromRenderSlot for slot params, got:\n{}",
        result
    );
    assert!(
        result.contains("strictRenderSlot"),
        "should have strictRenderSlot for children, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should reference TabItem in slot check, got:\n{}",
        result
    );
    // Negative: no raw v-slot
    assert!(
        !result.contains("v-slot"),
        "v-slot directive should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_if_narrowing() {
    // v-if/v-else branches produce different element types — both should be in the array
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs><div v-if="isA" /><span v-else /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("isA", BindingType::SetupRef),
        ],
    );
    // Positive: both div and span in the slot check array
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("\"div\""),
        "should reference div element, got:\n{}",
        result
    );
    assert!(
        result.contains("\"span\""),
        "should reference span element, got:\n{}",
        result
    );
    // Negative
    assert!(
        !result.contains("v-if"),
        "v-if should not appear in output, got:\n{}",
        result
    );
    assert!(
        !result.contains("v-else"),
        "v-else should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_for_nested() {
    // <template v-for> with a slot name should still collect children correctly
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs><template v-for="item in items" #default><TabItem /></template></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
            ("items", BindingType::SetupRef),
        ],
    );
    // Positive: TabItem in default slot check
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("'default'"),
        "should reference default slot, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should reference TabItem, got:\n{}",
        result
    );
}

// ── Strict slot sourcemap test ──────────────────────────────

/// Helper: generate TSX template with strict_slots AND return source map tokens.
fn gen_tsx_template_strict_slots_with_map(
    source: &str,
    bindings: &[(&str, BindingType)],
) -> (String, Vec<(u32, u32, u32)>) {
    let alloc = Allocator::new();
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let template_ast = match syntax.take_template_ast() {
        Some(ast) => ast,
        None => return (String::new(), Vec::new()),
    };

    let source_type = oxc_span::SourceType::tsx();
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        source_type,
    );

    let tpl_alloc = Allocator::new();
    let mut tpl_ct = CodeTransform::new(source, &tpl_alloc);
    let mut out = CodeGenOutput::new(&tpl_alloc);
    let binding_map: FxHashMap<&str, BindingType> = bindings
        .iter()
        .map(|&(name, bt)| (tpl_alloc.alloc_str(name) as &str, bt))
        .collect();
    let options = IdeTemplateOptions {
        self_name: "App",
        comments: true,
        is_jsx: false,
        strict_slots: true,
    };

    generate_ide_template(
        &template_ast,
        &oxc_ast,
        source,
        &mut out,
        &tpl_alloc,
        &binding_map,
        &options,
    );
    out.apply_to(&mut tpl_ct);

    let full = tpl_ct.build_string();
    let map =
        tpl_ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("test.vue"));
    let tokens: Vec<(u32, u32, u32)> = map
        .get_tokens()
        .filter(|t| t.get_source_id().is_some())
        .map(|t| (t.get_dst_line(), t.get_dst_col(), t.get_src_col()))
        .collect();

    (full, tokens)
}

#[test]
fn strict_slots_sourcemap_component_child() {
    // Verify that the source map has a token mapping the child constructor
    // name back to its position in the template.
    let source = "<template><Tabs><TabItem /></Tabs></template>";
    let (output, tokens) = gen_tsx_template_strict_slots_with_map(
        source,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );

    // Find `TabItem` position in the source (after `<`)
    let tab_item_src_col = source.find("<TabItem").unwrap() as u32 + 1; // skip `<`

    // The strictRenderSlot call should contain TabItem with a mapped token
    assert!(
        output.contains("strictRenderSlot"),
        "should have strictRenderSlot in output: {}",
        output
    );

    // Find a token that maps to the TabItem source position
    let has_tab_item_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == tab_item_src_col);
    assert!(
        has_tab_item_token,
        "should have a source map token at TabItem position (col {}), tokens: {:?}\noutput: {}",
        tab_item_src_col, tokens, output
    );
}

#[test]
fn strict_slots_sourcemap_html_child() {
    // Verify sourcemap mapping for HTML element children
    let source = "<template><Tabs><input /></Tabs></template>";
    let (output, tokens) =
        gen_tsx_template_strict_slots_with_map(source, &[("Tabs", BindingType::SetupImport)]);

    // `input` position in source (after `<`)
    let input_src_col = source.find("<input").unwrap() as u32 + 1;

    assert!(
        output.contains("HTMLElementTagNameMap[\"input\"]"),
        "should have HTMLElementTagNameMap in output: {}",
        output
    );

    // Find a token that maps to the input source position
    let has_input_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == input_src_col);
    assert!(
        has_input_token,
        "should have a source map token at input position (col {}), tokens: {:?}\noutput: {}",
        input_src_col, tokens, output
    );
}

#[test]
fn strict_slots_v_for_component_var() {
    // v-for introduces a component variable — the strict slot check should use
    // the loop variable name as the constructor reference.
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs v-for="Comp in components"><Comp /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("components", BindingType::SetupRef),
        ],
    );
    // Positive: strictRenderSlot referencing v-for variable Comp
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("'default'"),
        "should reference default slot, got:\n{}",
        result
    );
    // The child constructor should be "Comp" — the v-for loop variable
    // It appears in the strictRenderSlot array
    let slot_call_start = result.find("strictRenderSlot").unwrap();
    let slot_call = &result[slot_call_start..];
    assert!(
        slot_call.contains("Comp"),
        "strictRenderSlot array should contain Comp (v-for variable), got:\n{}",
        slot_call
    );
    // Negative: no raw v-for in output
    assert!(
        !result.contains("v-for"),
        "v-for should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_slot_component_var() {
    // v-slot destructures a component — the strict slot check on the inner
    // component should reference the slot variable name.
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Provider v-slot="{ Child }"><Tabs><Child /></Tabs></Provider></template>"#,
        &[
            ("Provider", BindingType::SetupImport),
            ("Tabs", BindingType::SetupImport),
        ],
    );
    // Positive: strictRenderSlot on Tabs with Child in the array
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    // Find the Tabs strict slot call — it should reference Child
    let slot_call_start = result.find("strictRenderSlot").unwrap();
    let slot_call = &result[slot_call_start..];
    assert!(
        slot_call.contains("Child"),
        "strictRenderSlot array should contain Child (v-slot variable), got:\n{}",
        slot_call
    );
    assert!(
        slot_call.contains("'default'"),
        "should reference default slot, got:\n{}",
        slot_call
    );
    // Negative: raw v-slot should not be in output
    assert!(
        !result.contains("v-slot"),
        "v-slot should not appear in output, got:\n{}",
        result
    );
    // Provider also has children (Tabs) so it should also get a strictRenderSlot call
    let second_call = result.match_indices("strictRenderSlot").nth(1);
    assert!(
        second_call.is_some(),
        "Provider should also get a strictRenderSlot call for its default slot, got:\n{}",
        result
    );
}

// ── Options API component alias resolution ────────────────────────────────

#[test]
fn options_api_component_alias_emits_binding() {
    let source = r#"<script lang="ts">
import { defineComponent } from 'vue'
import SomeComp from './SomeComp.vue'

export default defineComponent({
  components: { MyAlias: SomeComp },
  setup() { return {} }
})
</script>
<template>
  <MyAlias />
</template>"#;
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("Test.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");

    // Template should use <MyAlias> and MyAlias must be in scope
    assert!(
        tsx.code.contains("<MyAlias"),
        "template should contain <MyAlias> JSX tag:\n{}",
        tsx.code
    );
    // There must be a const alias that assigns SomeComp to MyAlias
    assert!(
        tsx.code.contains("const MyAlias = SomeComp"),
        "should emit 'const MyAlias = SomeComp' for the component alias:\n{}",
        tsx.code
    );
    // Must be valid TSX
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "TSX should have no parse errors. Got {} errors:\n{}",
        parsed.errors.len(),
        tsx.code
    );
}

#[test]
fn options_api_component_shorthand_no_alias_needed() {
    // Shorthand: components: { SomeComp } — SomeComp is already imported, no alias needed
    let source = r#"<script lang="ts">
import { defineComponent } from 'vue'
import SomeComp from './SomeComp.vue'

export default defineComponent({
  components: { SomeComp },
  setup() { return {} }
})
</script>
<template>
  <SomeComp />
</template>"#;
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("Test.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");

    // SomeComp is already imported — no extra alias declaration needed
    // (it shouldn't break if one IS emitted, but it's unnecessary)
    assert!(
        tsx.code.contains("<SomeComp"),
        "template should contain <SomeComp> JSX tag:\n{}",
        tsx.code
    );
    // Must be valid TSX
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "TSX should have no parse errors. Got {} errors:\n{}",
        parsed.errors.len(),
        tsx.code
    );
}

// ── Issue #48: $event must not be prefixed with instance ─────────────────

#[test]
fn dollar_event_standalone_not_prefixed() {
    let result = gen_tsx_template(r#"<template><div @click="$event">click</div></template>"#);
    // Positive: $event should appear bare inside the callback
    assert!(
        result.contains("$event"),
        "should contain $event in output: {result}"
    );
    // Negative: $event must NOT be prefixed with ___VERTER___instance.
    assert!(
        !result.contains("___VERTER___instance.$event"),
        "$event must NOT be prefixed with instance, got: {result}"
    );
}

#[test]
fn dollar_event_in_inline_expr_not_prefixed() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click="handleClick($event)">click</div></template>"#,
        &[("handleClick", BindingType::SetupConst)],
    );
    // Positive: handleClick and $event should both be present
    assert!(
        result.contains("handleClick"),
        "should contain handleClick: {result}"
    );
    assert!(result.contains("$event"), "should contain $event: {result}");
    // Negative: $event must NOT be prefixed
    assert!(
        !result.contains("___VERTER___instance.$event"),
        "$event must NOT be prefixed with instance, got: {result}"
    );
}

// ── Issue #46: bare @click (no value) must not emit broken binding ───────

#[test]
fn bare_event_no_value_removed() {
    let result = gen_tsx_template(r#"<template><div @click>click</div></template>"#);
    // Negative: must NOT contain onClick or any broken click binding
    assert!(
        !result.contains("onClick"),
        "bare @click should be removed, must not contain onClick: {result}"
    );
    assert!(
        !result.contains("___VERTER___ctx.click"),
        "bare @click must not produce ctx.click binding: {result}"
    );
    assert!(
        !result.contains("___VERTER___instance.click"),
        "bare @click must not produce instance.click binding: {result}"
    );
}

// ── eventCallbacks wrapper for $event type inference ─────────────────────

#[test]
fn event_handler_with_event_param_uses_event_callbacks_native() {
    let result =
        gen_tsx_template(r#"<template><div @click="handleClick($event)">click</div></template>"#);
    // Positive: should use eventCallbacks wrapper for $event type inference
    assert!(
        result.contains("___VERTER___eventCallbacks"),
        "native event with $event should use eventCallbacks wrapper: {result}"
    );
    assert!(
        result.contains("...___VERTER___eventArgs"),
        "should have rest args for eventCallbacks: {result}"
    );
    // Positive: $event should still be present inside the inner callback
    assert!(
        result.contains("$event"),
        "should still contain $event in inner callback: {result}"
    );
    // Negative: should NOT have bare ($event) => without eventCallbacks
    assert!(
        !result.contains("={($event) =>"),
        "should NOT use bare ($event) => pattern, must use eventCallbacks: {result}"
    );
}

#[test]
fn event_handler_with_event_param_uses_event_callbacks_component() {
    let result =
        gen_tsx_template(r#"<template><MyComp @custom="handleCustom($event)" /></template>"#);
    // Positive: should use eventCallbacks wrapper
    assert!(
        result.contains("___VERTER___eventCallbacks"),
        "component event with $event should use eventCallbacks wrapper: {result}"
    );
    assert!(
        result.contains("...___VERTER___eventArgs"),
        "should have rest args for eventCallbacks: {result}"
    );
    // Negative: should NOT have bare ($event) =>
    assert!(
        !result.contains("={($event) =>"),
        "should NOT use bare ($event) => pattern on component: {result}"
    );
}

#[test]
fn event_handler_without_event_param_no_event_callbacks() {
    // Simple identifier — no eventCallbacks needed
    let result = gen_tsx_template(r#"<template><div @click="handleClick">click</div></template>"#);
    assert!(
        !result.contains("___VERTER___eventCallbacks"),
        "simple ident handler should NOT use eventCallbacks: {result}"
    );

    // Inline expression without $event — no eventCallbacks needed
    let result2 = gen_tsx_template(r#"<template><div @click="count++">click</div></template>"#);
    assert!(
        !result2.contains("___VERTER___eventCallbacks"),
        "inline expr without $event should NOT use eventCallbacks: {result2}"
    );
}

#[test]
fn event_handler_spread_with_event_param_uses_event_callbacks() {
    let result = gen_tsx_template(
        r#"<template><div @click-overlay="emit('clickOverlay', $event)" /></template>"#,
    );
    // Positive: spread path should also use eventCallbacks
    assert!(
        result.contains("___VERTER___eventCallbacks"),
        "spread event with $event should use eventCallbacks wrapper: {result}"
    );
    assert!(
        result.contains("...___VERTER___eventArgs"),
        "spread event should have rest args: {result}"
    );
    // Negative: should NOT have bare ($event) => without eventCallbacks wrapping it
    assert!(
        !result.contains(r#"": ($event) =>"#),
        "spread event should NOT use bare ($event) => pattern (without eventCallbacks): {result}"
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
    // Positive: both branches should have .map()
    let map_count = result.matches(".map(").count();
    assert!(
        map_count >= 2,
        "should have two .map() calls (one per branch), found {map_count}: {result}"
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
    // Should have 3 .map() calls
    let map_count = result.matches(".map(").count();
    assert!(
        map_count >= 3,
        "should have three .map() calls, found {map_count}: {result}"
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
    // First branch has .map(), second doesn't
    assert!(
        result.contains(".map("),
        "first branch should have .map(): {result}"
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
    // Solo v-if + v-for: condition should be lifted outside .map()
    // Vue 3 precedence: v-if has higher precedence, runs before v-for
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in list">{{ item }}</div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_solo_lifts_condition ===\n{}\n=== END ===",
        result
    );
    // Should have lifted ternary: `show ? list.map(...) : null`
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "should have lifted condition outside map: {result}"
    );
    assert!(
        result.contains(": null"),
        "solo lifted should have : null fallback: {result}"
    );
    assert!(result.contains(".map("), "should have .map(): {result}");
}

#[test]
fn v_if_v_for_solo_lifts_condition_before_normal_sibling() {
    // Same as the solo case, but followed by a normal sibling. This still
    // needs the lifted `cond ? map(...) : null` shape.
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in list">{{ item }}</div><p>after</p></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_solo_lifts_condition_before_normal_sibling ===\n{}\n=== END ===",
        result
    );
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "should keep the lifted condition even with a following sibling: {result}"
    );
    assert!(
        result.contains(": null"),
        "solo lifted branch should still fall back to null before the next sibling: {result}"
    );
    assert!(
        result.contains("<p"),
        "following sibling should remain present: {result}"
    );
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="item in list">{{ item }}</div><p>after</p></template>"#,
        "solo v-if+v-for before normal sibling",
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
    // v-for should use statement-body callbacks: `=> { return (...) }`
    let result =
        gen_tsx_template(r#"<template><div v-for="item in items">{{ item }}</div></template>"#);
    eprintln!("=== v_if_v_for_statement_body ===\n{}\n=== END ===", result);
    // Should have statement body with return
    assert!(
        result.contains("=> { return"),
        "v-for should use statement body `=> {{ return (...)  }}`, got: {result}"
    );
    // Negative: should NOT have expression body `=> (`
    assert!(
        !result.contains("=> ("),
        "v-for should NOT use expression body `=> (`, got: {result}"
    );
}

#[test]
fn v_if_v_for_numeric_in_lifted_chain() {
    // Numeric v-for in a lifted chain should use Array.from without leading {
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="n in 5">{{ n }}</div><div v-else>none</div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_numeric_in_lifted_chain ===\n{}\n=== END ===",
        result
    );
    assert!(
        result.contains("Array.from("),
        "numeric v-for should use Array.from: {result}"
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
    let map_count = result.matches(".map(").count();
    assert!(
        map_count >= 2,
        "should have at least two .map() calls: {result}"
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
fn v_if_v_for_v_else_slot_outlet_plain_branch() {
    // Lifted ternary where v-else branch is a plain slot outlet
    let result = gen_tsx_template(
        r#"<template><MyComp><div v-if="show" v-for="item in items">{{ item }}</div><slot v-else name="fallback"/></MyComp></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_v_else_slot_outlet ===\n{}\n=== END ===",
        result
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><MyComp><div v-if="show" v-for="item in items">{{ item }}</div><slot v-else name="fallback"/></MyComp></template>"#,
        "v-if+v-for then slot v-else",
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

// ============================================================================
// Typed EmitOp substrate — IDE-only prefixed-expression emission.
//
// These tests pin the four previously-desynced sites (v-html, v-text,
// dynamic-key bind `:[key]`, native v-model) plus the v-model repeated-
// occurrence contract. Every test asserts BOTH that the user identifier maps
// back to its source byte offset AND that the synthetic prefix/punctuation
// maps to None (no token covers the synthetic generated column).
// ============================================================================

/// Convert a generated byte offset into a (line, col) pair in the generated
/// output, matching the source-map token coordinate space (0-based line, col
/// in UTF-16 code units — ASCII fixtures keep byte==utf16).
fn gen_offset_to_line_col(output: &str, byte_offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in output.char_indices() {
        if i >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    (line, col)
}

/// True iff some mapped token starts exactly at the generated `(line, col)`.
fn has_token_at_gen(tokens: &[(u32, u32, u32)], line: u32, col: u32) -> bool {
    tokens.iter().any(|&(dl, dc, _)| dl == line && dc == col)
}

/// True iff some mapped token maps back to source byte offset `src` (single-line
/// fixtures only — `src_col` equals the byte offset on line 0).
fn has_token_for_src(tokens: &[(u32, u32, u32)], src: u32) -> bool {
    tokens.iter().any(|&(_, _, sc)| sc == src)
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
fn dynamic_key_bind_both_identifiers_map() {
    // <div :[key]="val"/> → {...{[key]: val}}. Both `key` and `val` map back;
    // `{...{[`, `]: `, `}}}` map to None.
    let source = r#"<template><div :[key]="val"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("key", BindingType::SetupConst),
            ("val", BindingType::SetupConst),
        ],
    );

    assert!(
        output.contains("{...{[key]: val}}"),
        ":[key]=\"val\" should emit {{...{{[key]: val}}}}: {output}"
    );
    // Exact closing — no extra brace (regression: the spread+object closes with
    // exactly `}}`, not `}}}`).
    assert!(
        !output.contains("{...{[key]: val}}}"),
        ":[key] emission must close with exactly `}}}}` (no extra brace): {output}"
    );

    // Positive: both identifiers map back.
    let key_src = source.find("[key]").unwrap() as u32 + 1; // inside the [ ]
    let val_src = source.find("\"val\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, key_src),
        "key must map to source col {key_src}. Tokens: {tokens:?}"
    );
    assert!(
        has_token_for_src(&tokens, val_src),
        "val must map to source col {val_src}. Tokens: {tokens:?}"
    );

    // Negative: the `{...{[` boundary start maps to None.
    let boundary_gen = output.find("{...{[").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "{{...{{[ start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );
    // The `]: ` separator between key and val maps to None.
    let sep_gen = output.find("]: ").unwrap();
    let (sl, sc) = gen_offset_to_line_col(&output, sep_gen);
    assert!(
        !has_token_at_gen(&tokens, sl, sc),
        "]: separator (gen {sl}:{sc}) must map to None. Tokens: {tokens:?}"
    );
}

#[test]
fn native_vmodel_every_occurrence_maps_back() {
    // <input v-model="count"/> on a native element emits `count` 2-3 times:
    //   value={count} onInput={($event:any) => ((count) = $event)}
    // Every generated occurrence of `count` must map back to the single source
    // span; the assignment punctuation (=>, ($event, =) maps to None.
    let source = r#"<template><input v-model="count"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupRef)]);

    assert!(
        output.contains("value={"),
        "native v-model should emit value={{...}}: {output}"
    );
    assert!(
        output.contains("onInput={"),
        "native v-model should emit onInput handler: {output}"
    );

    let count_src = source.find("\"count\"").unwrap() as u32 + 1;

    // SetupRef bindings are emitted bare (no prefix) but with a `.value` suffix
    // appended; the identifier text `count` therefore appears at each occurrence.
    // Enumerate ALL generated occurrences of the identifier `count` and assert
    // each one is covered by a token mapping back to the source span.
    let mut occurrence_starts = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = output[search_from..].find("count") {
        let at = search_from + rel;
        occurrence_starts.push(at);
        search_from = at + "count".len();
    }
    assert!(
        occurrence_starts.len() >= 2,
        "expected >=2 generated `count` occurrences (read + write), found {}: {output}",
        occurrence_starts.len()
    );

    for at in &occurrence_starts {
        let (gl, gc) = gen_offset_to_line_col(&output, *at);
        let covered = tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == gl && dc == gc && sc == count_src);
        assert!(
            covered,
            "generated `count` occurrence at gen {gl}:{gc} must map back to source col {count_src}. Tokens: {tokens:?}, output: {output}"
        );
    }

    // Negative: the arrow `=>` of the handler maps to None.
    let arrow_gen = output.find("=>").unwrap();
    let (al, ac) = gen_offset_to_line_col(&output, arrow_gen);
    assert!(
        !has_token_at_gen(&tokens, al, ac),
        "arrow => (gen {al}:{ac}) must map to None. Tokens: {tokens:?}"
    );
    // The `($event` parameter list maps to None.
    let ev_gen = output.find("($event").unwrap();
    let (el, ec) = gen_offset_to_line_col(&output, ev_gen);
    assert!(
        !has_token_at_gen(&tokens, el, ec),
        "($event param (gen {el}:{ec}) must map to None. Tokens: {tokens:?}"
    );
}

#[test]
fn vmodel_source_to_generated_selects_read_occurrence() {
    // P2-A: one source span → multiple generated occurrences. The FIRST covering
    // mapped run in generated byte order is the value-binding (read) occurrence,
    // emitted before the assignment LHS. Discriminating: an LHS-first or
    // non-deterministic selection picks the occurrence inside `((count) = $event)`.
    let source = r#"<template><input v-model="count"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupRef)]);

    let count_src = source.find("\"count\"").unwrap() as u32 + 1;

    // The value-binding occurrence is the one inside `value={...count...}`.
    let value_eq = output.find("value={").expect("value={ in output");
    let assign_lhs = output.find("((").expect("(( assignment LHS in output");
    assert!(
        value_eq < assign_lhs,
        "value binding must be emitted before the assignment LHS: {output}"
    );

    // Collect all tokens that map to count_src, sorted by generated position.
    let mut covering: Vec<(u32, u32)> = tokens
        .iter()
        .filter(|&&(_, _, sc)| sc == count_src)
        .map(|&(dl, dc, _)| (dl, dc))
        .collect();
    covering.sort_unstable();
    assert!(
        !covering.is_empty(),
        "count must have at least one mapped token. Tokens: {tokens:?}"
    );

    // The first covering run (deterministic strict first-covering lookup in
    // generated order) must fall within the value-binding occurrence, NOT the
    // assignment LHS inside `((count) = $event)`.
    let (fl, fc) = covering[0];
    let first_byte = {
        // recover byte offset of (fl, fc) — single fixture, find nth line break
        let mut idx = 0usize;
        let mut line = 0u32;
        let mut col = 0u32;
        for (i, ch) in output.char_indices() {
            if line == fl && col == fc {
                idx = i;
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += ch.len_utf16() as u32;
            }
            idx = i + ch.len_utf8();
        }
        idx
    };
    assert!(
        first_byte >= value_eq && first_byte < assign_lhs,
        "first covering run (gen {fl}:{fc}, byte {first_byte}) must be the value-binding occurrence \
         in [{value_eq}, {assign_lhs}), not the assignment LHS. Output: {output}"
    );
}

#[test]
fn vmodel_modifier_maps_to_source() {
    // <input v-model.trim="x"/> → the `trim` modifier token maps to its source span.
    let source = r#"<template><input v-model.trim="x"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupRef)]);

    assert!(
        output.contains("Modifiers={{"),
        "v-model.trim should emit a modifiers prop: {output}"
    );
    assert!(
        output.contains("trim"),
        "modifiers prop should contain `trim`: {output}"
    );

    let trim_src = source.find(".trim").unwrap() as u32 + 1; // the `trim` after the dot
    assert!(
        has_token_for_src(&tokens, trim_src),
        "modifier `trim` must map to source col {trim_src}. Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn vmodel_prefix_not_double_shifted() {
    // P1-B: with a Data binding the identifier is prefixed by `___VERTER___instance.`.
    // The identifier token must map to the FIRST byte of the identifier in
    // generated output (the byte right after the prefix), not shifted into the
    // prefix and not leaving the identifier interior unmapped.
    let source = r#"<template><MyComp v-model="d_val"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("d_val", BindingType::Data)]);

    let needle = "___VERTER___instance.d_val";
    assert!(
        output.contains(needle),
        "Data v-model should emit the instance prefix: {output}"
    );

    let src_col = source.find("\"d_val\"").unwrap() as u32 + 1;

    // The generated `d_val` (after the FIRST prefix) must carry a token that maps
    // to the source identifier, anchored exactly at the identifier start.
    let prefix_pos = output.find(needle).unwrap();
    let ident_gen = prefix_pos + "___VERTER___instance.".len();
    let (il, ic) = gen_offset_to_line_col(&output, ident_gen);
    let anchored = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == il && dc == ic && sc == src_col);
    assert!(
        anchored,
        "d_val must map to source col {src_col} anchored at the identifier start (gen {il}:{ic}), \
         no double shift. Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the prefix start must NOT carry the identifier's mapping.
    let (pl, pc) = gen_offset_to_line_col(&output, prefix_pos);
    let prefix_carries = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == pl && dc == pc && sc == src_col);
    assert!(
        !prefix_carries,
        "the ___VERTER___instance. prefix start (gen {pl}:{pc}) must NOT carry d_val's mapping. \
         Tokens: {tokens:?}"
    );
}

#[test]
fn synthetic_boundary_start_maps_to_none() {
    // P1-C: the generated column at the start of an OverwriteSyntheticBoundary
    // (`innerHTML=` for v-html) maps to None. Discriminating: a Chunk::Overwritten
    // lowering would map that column back to the prop start.
    let source = r#"<template><div v-html="msg"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("msg", BindingType::SetupConst)]);

    let boundary_gen = output.find("innerHTML=").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "innerHTML= boundary start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}, output: {output}"
    );

    // And specifically: no token at that generated column maps to the prop start
    // (the old Chunk::Overwritten bug).
    let prop_start = source.find("v-html").unwrap() as u32;
    let maps_to_prop_start = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == bl && dc == bc && sc == prop_start);
    assert!(
        !maps_to_prop_start,
        "innerHTML= start must NOT map to the prop start (col {prop_start}) — the desync bug. \
         Tokens: {tokens:?}"
    );
}

#[test]
fn vmodel_does_not_emit_single_overwritten_chunk() {
    // The chunk list must contain NO Overwritten chunk spanning both the synthetic
    // prefix and a user identifier. Asserted via the map: the prop-start generated
    // column must NOT carry the identifier's source mapping (a single
    // overwrite(prop.start, prop_end, "value={count}...") would map the whole run
    // — including `value={` — back to prop.start).
    let source = r#"<template><input v-model="count"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupRef)]);

    let count_src = source.find("\"count\"").unwrap() as u32 + 1;
    let prop_start = source.find("v-model").unwrap() as u32;

    // No token may map a generated position back to the prop start.
    let any_prop_start = tokens.iter().any(|&(_, _, sc)| sc == prop_start);
    assert!(
        !any_prop_start,
        "no generated token may map back to the v-model prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );

    // The generated `value={` prefix must NOT carry count's mapping.
    let value_gen = output.find("value={").unwrap();
    let (vl, vc) = gen_offset_to_line_col(&output, value_gen);
    let value_carries_count = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == vl && dc == vc && sc == count_src);
    assert!(
        !value_carries_count,
        "the `value={{` synthetic prefix (gen {vl}:{vc}) must NOT carry count's mapping. \
         Tokens: {tokens:?}"
    );
}

#[test]
fn emit_codegen_crlf_and_tabs() {
    // P2-B: a CRLF, tab-indented fixture still maps identifiers exactly.
    let source = "<template>\r\n\t<div v-html=\"msg\" />\r\n</template>";
    let (output, tokens) = gen_tsx_template_with_map(source, &[("msg", BindingType::SetupConst)]);

    assert!(
        output.contains("innerHTML={msg}"),
        "CRLF/tab fixture should still emit innerHTML={{msg}}: {output:?}"
    );

    // `msg` source position: byte offset of the `m` in "msg" (the file has CRLF
    // and a leading tab, so compute the absolute byte offset directly).
    let msg_src = source.find("\"msg\"").unwrap() as u32 + 1;
    // The token's src_col is the column on its source LINE; `msg` is on line 1
    // (0-based), so match by src_col == column within that line.
    let line_start = source[..msg_src as usize]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0) as u32;
    let msg_src_col = msg_src - line_start;
    let has_msg = tokens.iter().any(|&(_, _, sc)| sc == msg_src_col);
    assert!(
        has_msg,
        "msg must map to source col {msg_src_col} (line-relative) even with CRLF/tabs. \
         Tokens: {tokens:?}, output: {output:?}"
    );
}

#[test]
fn vmodel_dynamic_arg_modifier_maps_and_is_valid() {
    // <input v-model:[eventName].trim="val"/> — dynamic arg + modifier.
    // The modifiers prop name must be the COMPUTED `[`${...}Modifiers`]` name with
    // the arg expression embedded, NOT an empty JSX attribute name (` ={{`), which
    // is invalid TSX. The embedded arg `eventName` must map back to its source span.
    let source = r#"<template><input v-model:[eventName].trim="val"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("eventName", BindingType::SetupConst),
            ("val", BindingType::SetupRef),
        ],
    );

    // Positive: a computed `[`${...}Modifiers`]` prop name is present.
    assert!(
        output.contains("Modifiers`]"),
        "dynamic-arg v-model with a modifier must emit a computed `[`${{...}}Modifiers`]` \
         prop name: {output}"
    );
    // Negative: the empty-attribute-name shape ` ={{` (the regression) must NOT appear.
    assert!(
        !output.contains(" ={{"),
        "dynamic-arg v-model must NOT emit an empty JSX attribute name ` ={{` (invalid TSX). \
         The computed modifiers name was dropped: {output}"
    );

    // The arg identifier `eventName` must map back to its source span. The arg
    // appears multiple times (computed prop name, event key, modifiers name); at
    // least one occurrence maps back.
    let arg_src = source.find("[eventName]").unwrap() as u32 + 1; // inside the [ ]
    assert!(
        has_token_for_src(&tokens, arg_src),
        "v-model dynamic arg `eventName` must map to source col {arg_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // The whole emission must be valid TSX (no empty attribute name, balanced
    // braces). Wrap as a JSX element attribute list and parse.
    let wrapper = format!("const x = <input {} />", output_attrs(&output));
    let val_alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "dynamic-arg v-model + modifier must produce valid TSX. Errors: {:?}\n--- output ---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        output
    );
}

/// Extract the attribute portion of a generated single-element template
/// (`<input ...attrs.../>`) for re-parsing as a JSX attribute list.
fn output_attrs(output: &str) -> String {
    // Strip the leading `<input` / `<tag` and trailing `/>` or `>` so the inner
    // attribute list can be re-wrapped in a fresh element for syntax validation.
    let after_tag = output
        .find(char::is_whitespace)
        .map(|i| &output[i..])
        .unwrap_or(output);
    let trimmed = after_tag.trim();
    let body = trimmed
        .strip_suffix("/>")
        .or_else(|| trimmed.strip_suffix('>'))
        .unwrap_or(trimmed);
    body.trim().to_string()
}

#[test]
fn v_on_object_spread_handler_maps_to_source() {
    // <div v-on="{ mousedown: doThis }"/> → {...{ mousedown: doThis }}.
    // The handler identifier `doThis` is a navigable user expression — it MUST map
    // back to its source span. The object punctuation (`{...{`, `: `, `}}`) and the
    // event key map to None.
    let source = r#"<template><div v-on="{ mousedown: doThis }"/></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("doThis", BindingType::SetupConst)]);

    assert!(
        output.contains("{...{"),
        "v-on object literal should emit a spread `{{...{{ ... }}}}`: {output}"
    );
    assert!(
        !output.contains("v-on"),
        "v-on directive must be removed: {output}"
    );

    // Positive: `doThis` maps to its source byte offset.
    let handler_src = source.find("doThis").unwrap() as u32;
    assert!(
        has_token_for_src(&tokens, handler_src),
        "v-on handler `doThis` must map to source col {handler_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the `{...{` spread boundary start maps to None (the old baked
    // overwrite mapped the whole run — including the handler — back to prop.start).
    let boundary_gen = output.find("{...{").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "{{...{{ spread boundary start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );

    // Negative: no generated token may map back to the v-on prop start (the desync).
    let prop_start = source.find("v-on").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the v-on prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn v_on_dynamic_event_name_expr_maps() {
    // <div @[event]="handler"/> → {...{[`on${event}` as any]: handler}}.
    // BOTH the dynamic event-name expression `event` and the handler `handler` are
    // navigable user expressions — each must map back to its source span. The
    // computed-key template literal and object punctuation map to None.
    let source = r#"<template><div @[event]="handler"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("event", BindingType::SetupConst),
            ("handler", BindingType::SetupConst),
        ],
    );

    assert!(
        output.contains("as any]:"),
        "dynamic event name should emit the computed-key spread `[`on${{...}}` as any]: ...`: {output}"
    );
    assert!(
        !output.contains("@["),
        "dynamic event syntax must be removed: {output}"
    );

    // Positive: both `event` (arg) and `handler` (value) map back to source.
    let event_src = source.find("[event]").unwrap() as u32 + 1; // inside the [ ]
    let handler_src = source.find("\"handler\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, event_src),
        "dynamic event-name expr `event` must map to source col {event_src}. \
         Tokens: {tokens:?}, output: {output}"
    );
    assert!(
        has_token_for_src(&tokens, handler_src),
        "dynamic event handler `handler` must map to source col {handler_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the `{...{[` boundary start maps to None.
    let boundary_gen = output.find("{...{[").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "{{...{{[ boundary start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );

    // Negative: no generated token may map back to the prop start (the desync).
    let prop_start = source.find('@').unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the @[event] prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn v_show_condition_maps_to_source() {
    // <div v-show="visible"/> → style={{display: visible ? undefined : 'none'}}.
    // The condition `visible` is a navigable user expression relocated into the
    // synthetic style attribute; it MUST map back. The `style={{display: ` prefix
    // and ` ? undefined : 'none'}}` suffix map to None.
    let source = r#"<template><div v-show="visible"/></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("visible", BindingType::SetupConst)]);

    assert!(
        output.contains("display:"),
        "v-show should emit a display style: {output}"
    );
    assert!(
        !output.contains("v-show"),
        "v-show directive must be removed: {output}"
    );

    // Positive: `visible` maps to its source byte offset.
    let visible_src = source.find("\"visible\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, visible_src),
        "v-show condition `visible` must map to source col {visible_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the `style={{display: ` boundary start maps to None (the old baked
    // overwrite mapped the whole run — including `visible` — back to prop.start).
    let boundary_gen = output.find("style={{display:").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "style={{{{display: boundary start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );

    // Negative: no generated token may map back to the v-show prop start.
    let prop_start = source.find("v-show").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the v-show prop start (col {prop_start}). \
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

#[test]
fn migrated_sites_binding_notation_characterization() {
    // P3 characterization — pin the prop-accessor notation the migrated relocated
    // emitters produce, so it is INTENTIONAL, not accidental:
    //
    // 1. A keyword-named prop accessed as a bindingless SIMPLE identifier (e.g.
    //    `v-show="class"`) → BRACKET notation (`__props["class"]`). `emit_relocated_value`
    //    routes a bindingless simple identifier through `resolve_simple_expr`, which
    //    emits the bracket form for keywords (dot notation `__props.class` is valid TS
    //    too, but bracket matches the pre-migration shared-helper behaviour).
    let v_show_kw = gen_tsx_template_with_bindings(
        r#"<template><div v-show="class"/></template>"#,
        &[("class", BindingType::Props)],
    );
    assert!(
        v_show_kw.contains(r#"__props["class"]"#),
        "v-show keyword prop must use bracket notation `__props[\"class\"]`: {v_show_kw}"
    );
    assert!(
        !v_show_kw.contains("__props.class"),
        "v-show keyword prop must NOT use dot notation `__props.class`: {v_show_kw}"
    );

    // 2. A non-keyword prop emitted through the v-on object-spread substrate (where
    //    OXC DOES extract the binding) → DOT notation (`__props.handler`), identical to
    //    the in-place `@click="handler"` form. The migration keeps the two consistent.
    let v_on_obj = gen_tsx_template_with_bindings(
        r#"<template><div v-on="{ click: handler }"/></template>"#,
        &[("handler", BindingType::Props)],
    );
    assert!(
        v_on_obj.contains("__props.handler"),
        "v-on object-spread Props handler must use dot notation `__props.handler`: {v_on_obj}"
    );
    let at_click = gen_tsx_template_with_bindings(
        r#"<template><div @click="handler"/></template>"#,
        &[("handler", BindingType::Props)],
    );
    assert!(
        at_click.contains("__props.handler"),
        "@click Props handler uses dot notation (the spread form must match it): {at_click}"
    );
}
