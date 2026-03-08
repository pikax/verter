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
        result.contains("))}"),
        "v-for closing should produce CloseParen+CloseParen+CloseBrace for .map() closure, got: {}",
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
        on_keydown_attr, result
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
            "static style string must not contain newlines. Got: {}", static_str
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
    // In TSX mode, Data bindings are bare identifiers (no _ctx. prefix).
    let source = r#"<template><component :is="currentView">hello</component></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("currentView", BindingType::Data)]);

    assert!(
        output.contains("currentView") && !output.contains("_ctx.currentView"),
        "Data binding should be bare identifier in TSX mode: {output}"
    );
    assert!(
        !output.contains(":is="),
        "`:is` attribute should be removed from output: {output}"
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
    for &(dl, dc, sc) in &tokens {
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
    let style_end = result.find("}}").expect("should have closing }} for style object");
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
    eprintln!("=== COMPONENT :IS + V-IF + V-TEXT ===\n{}\n=== END ===", result);

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
    let source = match std::fs::read_to_string("d:/dev/github/verter-test-repos/ant-design-vue/components/switch/demo/basic.vue") {
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
    assert!(parsed.errors.is_empty(), "Got {} errors", parsed.errors.len());
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
    assert!(parsed.errors.is_empty(), "Got {} errors", parsed.errors.len());
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
    assert!(parsed.errors.is_empty(), "Got {} errors", parsed.errors.len());
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
    assert!(parsed.errors.is_empty(), "Got {} errors", parsed.errors.len());
}
