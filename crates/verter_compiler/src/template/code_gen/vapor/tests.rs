use super::*;
use crate::ast::types::*;
use crate::parser::types::RootNodeTemplateContent;
use crate::template::oxc::types::Dynamism;
use crate::types::NodeTag;
use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

fn make_options() -> TemplateCodeGenOptions {
    TemplateCodeGenOptions {
        mode: super::super::CodeGenMode::Vapor,
        is_inline: false,
        is_production: false,
        comments: true,
        ..Default::default()
    }
}

fn make_options_inline() -> TemplateCodeGenOptions {
    TemplateCodeGenOptions {
        mode: super::super::CodeGenMode::Vapor,
        is_inline: true,
        is_production: false,
        comments: true,
        ..Default::default()
    }
}

fn make_resolver(_alloc: &Allocator) -> BindingResolver<'_> {
    BindingResolver::new(FxHashMap::default(), false)
}

fn make_root(
    tag_open: NodeTag,
    tag_close: Option<NodeTag>,
    content: Option<RootNodeTemplateContent>,
) -> RootNodeTemplate {
    RootNodeTemplate {
        tag_open,
        tag_close,
        lang: None,
        attributes: Vec::new(),
        content,
    }
}

/// Create a minimal empty TemplateAst for tests that don't need AST lookups.
fn make_empty_ast(root: &RootNodeTemplate) -> TemplateAst {
    TemplateAst {
        nodes: Vec::new(),
        root: root.clone(),
    }
}

fn apply_output<'a>(source: &str, out: CodeGenOutput<'a>, alloc: &'a Allocator) -> String {
    let mut ct = crate::code_transform::CodeTransform::new(source, alloc);
    out.apply_to(&mut ct);
    ct.build_string()
}

// ==================== Full pipeline integration tests ====================

/// Run a .vue source through the full new_impl pipeline and return the
/// generated output for CodeGenMode::Vapor.
fn run_full_pipeline(source: &str) -> String {
    run_full_pipeline_mode(source, super::super::CodeGenMode::Vapor)
}

/// Run a .vue source through the full new_impl pipeline and return the
/// generated output for a given CodeGenMode.
fn run_full_pipeline_mode(source: &str, mode: super::super::CodeGenMode) -> String {
    use crate::code_transform::CodeTransform;
    use crate::diagnostics::{SyntaxPluginContext, SyntaxPluginOptions};
    use crate::parser::Syntax as NewSyntax;
    use crate::script::{generate_script, ScriptCodeGenOptions};
    use crate::template::code_gen::generate_template;
    use crate::template::oxc::parse_template_expressions;
    use crate::tokenizer::byte::tokenize;
    use oxc_span::SourceType;

    let alloc = Allocator::default();
    let opts = SyntaxPluginOptions::default();
    let ctx = SyntaxPluginContext {
        input: source,
        bytes: source.as_bytes(),
        options: &opts,
        diagnostics: Vec::new(),
    };
    let mut syntax = NewSyntax::new(false);
    tokenize(source.as_bytes(), |e| syntax.handle(&e, &ctx));

    let mut ct = CodeTransform::new(source, &alloc);
    let script_opts = ScriptCodeGenOptions {
        component_name: "Anonymous",
        scope_id: "a4f2eed6",
        has_scoped_style: syntax.has_style_scope(),
        ..Default::default()
    };
    let script_result = generate_script(
        syntax.script(),
        syntax.script_setup(),
        source,
        &mut ct,
        &alloc,
        &script_opts,
    );

    let template_ast = syntax.take_template_ast();
    if let Some(ast) = &template_ast {
        let oxc_ast = parse_template_expressions(ast, source, &alloc, SourceType::tsx(), false);
        let mut ct2 = CodeTransform::new(source, &alloc);
        generate_template(
            ast,
            &oxc_ast,
            source,
            &mut ct2,
            &alloc,
            script_result.bindings,
            &super::super::TemplateCodeGenOptions {
                mode,
                ..Default::default()
            },
        );
        ct2.build_string()
    } else {
        "(no template)".to_string()
    }
}

// ==================== Full pipeline: basic static elements ====================

#[test]
fn full_pipeline_static_div() {
    let source = "<template><div>hello</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_template("),
        "Expected _template, got: {result}"
    );
    assert!(
        result.contains("function render("),
        "Expected render function, got: {result}"
    );
    assert!(
        result.contains("const n0 = t0()"),
        "Expected template instantiation, got: {result}"
    );
    assert!(
        result.contains("return n0"),
        "Expected return, got: {result}"
    );
}

#[test]
fn full_pipeline_nested_static() {
    let source = "<template><div><span>inner</span></div></template>";
    let result = run_full_pipeline(source);
    // Static nested elements should all be in one template
    assert!(
        result.contains("_template("),
        "Expected _template, got: {result}"
    );
    assert!(
        !result.contains("_renderEffect"),
        "Static content should have no effects, got: {result}"
    );
}

#[test]
fn full_pipeline_interpolation() {
    let source = "<template><div>{{ msg }}</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_renderEffect"),
        "Expected _renderEffect for interpolation, got: {result}"
    );
    assert!(
        result.contains("_setText"),
        "Expected _setText for interpolation, got: {result}"
    );
    assert!(
        result.contains("_toDisplayString"),
        "Expected _toDisplayString, got: {result}"
    );
}

// ==================== HTML minimization ====================

#[test]
fn html_minimization_simple_div() {
    // Vue 3.6: _template("<div>hello", true) — drops closing tag for last child
    let source = "<template><div>hello</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<div>hello\""),
        "Expected minimized HTML '<div>hello', got: {result}"
    );
}

#[test]
fn html_minimization_empty_div() {
    // Vue 3.6: _template("<div>", true) — empty div drops closing tag
    let source = "<template><div></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<div>\""),
        "Expected minimized HTML '<div>', got: {result}"
    );
}

#[test]
fn html_minimization_nested() {
    // Vue 3.6: _template("<div><span>inner", true) — drops all trailing close tags
    let source = "<template><div><span>inner</span></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<div><span>inner\""),
        "Expected minimized HTML '<div><span>inner', got: {result}"
    );
}

#[test]
fn html_minimization_multiple_children() {
    // Vue 3.6: <div><span>a</span><span>b — only the last child's closing tag is dropped
    let source = "<template><div><span>a</span><span>b</span></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<div><span>a</span><span>b\""),
        "Expected minimized HTML with last close tag dropped, got: {result}"
    );
}

#[test]
fn html_minimization_self_closing_br() {
    // Vue 3.6: _template("<br>", true) — no self-close slash
    let source = "<template><br/></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<br>\""),
        "Expected '<br>' (no self-close slash), got: {result}"
    );
}

#[test]
fn html_unquoted_attrs() {
    // Vue 3.6: _template("<div id=app>hi", true) — unquoted attrs when no spaces
    let source = "<template><div id=\"app\">hi</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<div id=app>hi\""),
        "Expected unquoted attr id=app, got: {result}"
    );
}

#[test]
fn html_unquoted_img_attrs() {
    // Vue 3.6: <img src=a.png alt=pic> — unquoted attrs
    let source = "<template><img src=\"a.png\" alt=\"pic\"></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<img src=a.png alt=pic>\""),
        "Expected unquoted img attrs, got: {result}"
    );
}

#[test]
fn html_render_function_signature() {
    // Vue 3.6 Vapor: export function render(_ctx) — not the full VDOM signature
    let source = "<template><div>hi</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("function render(_ctx)"),
        "Expected Vapor render signature 'function render(_ctx)', got: {result}"
    );
    assert!(
        !result.contains("_cache"),
        "Vapor render should not have _cache param, got: {result}"
    );
}

#[test]
fn html_set_attr_for_data_attributes() {
    // Vue 3.6: _setAttr for data-* attributes, not _setProp
    let source = "<template><div :data-id=\"id\">hi</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setAttr(n0, \"data-id\""),
        "Expected _setAttr for data-* attributes, got: {result}"
    );
}

// ==================== Events ====================

#[test]
fn event_click_delegated() {
    // Vue 3.6: delegated click event with _createInvoker
    let source = "<template><button @click=\"handler\">click</button></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_delegateEvents(\"click\")"),
        "Expected _delegateEvents, got: {result}"
    );
    assert!(
        result.contains("$evtclick"),
        "Expected $evtclick assignment, got: {result}"
    );
    assert!(
        result.contains("_createInvoker"),
        "Expected _createInvoker, got: {result}"
    );
}

#[test]
fn event_multiple_delegated() {
    // Vue 3.6: multiple delegated events
    let source = "<template><button @click=\"a\" @mouseover=\"b\">hi</button></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_delegateEvents("),
        "Expected _delegateEvents, got: {result}"
    );
    assert!(
        result.contains("$evtclick"),
        "Expected $evtclick, got: {result}"
    );
    assert!(
        result.contains("$evtmouseover"),
        "Expected $evtmouseover, got: {result}"
    );
}

// ==================== v-show, v-model ====================

#[test]
fn v_show_simple() {
    // Vue 3.6: _applyVShow(n0, () => (_ctx.visible))
    let source = "<template><div v-show=\"visible\">content</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_applyVShow(n0"),
        "Expected _applyVShow, got: {result}"
    );
}

#[test]
fn v_model_input() {
    // Vue 3.6: _applyTextModel(n0, () => (_ctx.text), _value => (_ctx.text = _value))
    let source = "<template><input v-model=\"text\"></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_applyTextModel(n0"),
        "Expected _applyTextModel, got: {result}"
    );
}

#[test]
fn v_model_checkbox() {
    // Vue 3.6: _applyCheckboxModel
    let source = "<template><input type=\"checkbox\" v-model=\"checked\"></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_applyCheckboxModel(n0"),
        "Expected _applyCheckboxModel, got: {result}"
    );
}

#[test]
fn v_model_with_trim_modifier() {
    // Vue 3.6: _applyTextModel(n0, getter, setter, { trim: true })
    let source = "<template><input v-model.trim=\"text\"></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_applyTextModel(n0"),
        "Expected _applyTextModel, got: {result}"
    );
    assert!(
        result.contains("trim: true"),
        "Expected trim modifier, got: {result}"
    );
}

#[test]
fn v_model_with_number_modifier() {
    // Vue 3.6: _applyTextModel(n0, getter, setter, { number: true })
    let source = "<template><input v-model.number=\"val\"></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("number: true"),
        "Expected number modifier, got: {result}"
    );
}

#[test]
fn v_model_with_multiple_modifiers() {
    // Vue 3.6: _applyTextModel(n0, getter, setter, { trim: true, number: true })
    let source = "<template><input v-model.trim.number=\"val\"></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("trim: true"),
        "Expected trim modifier, got: {result}"
    );
    assert!(
        result.contains("number: true"),
        "Expected number modifier, got: {result}"
    );
}

// ==================== v-html ====================

#[test]
fn v_html_directive() {
    // Vue 3.6: _renderEffect(() => _setHtml(n0, _ctx.rawHtml))
    let source = "<template><div v-html=\"rawHtml\"></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setHtml(n0"),
        "Expected _setHtml, got: {result}"
    );
    assert!(
        result.contains("_renderEffect"),
        "Expected _renderEffect for v-html, got: {result}"
    );
}

// ==================== Components ====================

#[test]
fn component_simple() {
    // Vue 3.6: _resolveComponent + _createComponentWithFallback
    let source = "<template><my-element></my-element></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_resolveComponent(\"my-element\")"),
        "Expected _resolveComponent, got: {result}"
    );
    assert!(
        result.contains("_createComponentWithFallback("),
        "Expected _createComponentWithFallback, got: {result}"
    );
}

#[test]
fn component_with_static_props() {
    // Vue 3.6: _createComponentWithFallback(comp, { title: "hello" })
    let source = "<template><my-comp title=\"hello\"></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createComponentWithFallback("),
        "Expected _createComponentWithFallback, got: {result}"
    );
    assert!(
        result.contains("title: \"hello\""),
        "Expected static title prop, got: {result}"
    );
}

#[test]
fn component_with_dynamic_props() {
    // Vue 3.6: _createComponent(comp, { title: () => (expr) })
    let source = "<template><my-comp :title=\"msg\"></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("title: () => (_ctx.msg)"),
        "Expected dynamic title prop with arrow fn, got: {result}"
    );
}

#[test]
fn component_with_event() {
    // Vue 3.6: _createComponent(comp, { onClick: () => handler })
    let source = "<template><my-comp @click=\"handler\"></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("onClick:"),
        "Expected onClick prop, got: {result}"
    );
}

#[test]
fn component_with_default_slot() {
    // Vue 3.6: _createComponent(comp, null, { default: () => { ... } })
    let source = "<template><my-comp><span>hello</span></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("default: () => {"),
        "Expected default slot closure, got: {result}"
    );
}

// ==================== Slot outlets ====================

#[test]
fn slot_default_outlet() {
    // Vue 3.6: _createSlot("default", null)
    let source = "<template><slot></slot></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createSlot(\"default\""),
        "Expected _createSlot(\"default\"), got: {result}"
    );
}

#[test]
fn slot_named_outlet() {
    // Vue 3.6: _createSlot("header", null)
    let source = "<template><slot name=\"header\"></slot></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createSlot(\"header\""),
        "Expected _createSlot(\"header\"), got: {result}"
    );
}

// ==================== Structural directives ====================

#[test]
fn v_if_simple() {
    // Vue 3.6: _createIf(() => (_ctx.show), () => { ... })
    let source = "<template><div v-if=\"show\">yes</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createIf("),
        "Expected _createIf, got: {result}"
    );
}

#[test]
fn v_if_else_chain() {
    // Vue 3.6: _createIf(() => (cond), () => {...}, () => {...})
    let source = "<template><div v-if=\"show\">yes</div><div v-else>no</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createIf("),
        "Expected _createIf, got: {result}"
    );
    // v-else branch should be nested as third arg
    assert!(
        result.contains(", () => {"),
        "Expected v-else closure, got: {result}"
    );
    // Should only return one node (the whole chain is a single root)
    assert!(
        result.contains("return n0"),
        "Expected single return n0, got: {result}"
    );
}

#[test]
fn v_if_else_if_else_chain() {
    // Vue 3.6: _createIf(() => (a), () => {...}, () => _createIf(() => (b), () => {...}, () => {...}))
    let source = "<template><div v-if=\"a\">A</div><div v-else-if=\"b\">B</div><div v-else>C</div></template>";
    let result = run_full_pipeline(source);
    // Outer _createIf
    assert!(
        result.contains("_createIf("),
        "Expected _createIf, got: {result}"
    );
    // Nested _createIf for v-else-if
    let count = result.matches("_createIf(").count();
    assert_eq!(
        count, 2,
        "Expected 2 _createIf calls (one nested), got {count} in: {result}"
    );
    // Should only return one node
    assert!(
        result.contains("return n0"),
        "Expected single return n0, got: {result}"
    );
}

#[test]
fn v_for_simple() {
    // Vue 3.6: _createFor(() => (_ctx.items), (item) => { ... })
    let source = "<template><div v-for=\"item in items\">{{ item }}</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createFor("),
        "Expected _createFor, got: {result}"
    );
}

#[test]
fn v_for_preserves_user_params() {
    // Vue 3.6: preserves user's variable names
    let source = "<template><div v-for=\"(item, index) in items\">{{ item }}</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("(item, index) => {"),
        "Expected preserved user params (item, index), got: {result}"
    );
}

#[test]
fn v_for_with_key() {
    // Vue 3.6: _createFor(() => (items), (item) => {...}, (item) => (item.id))
    let source =
        "<template><div v-for=\"item in items\" :key=\"item.id\">{{ item }}</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createFor("),
        "Expected _createFor, got: {result}"
    );
    assert!(
        result.contains("item.id"),
        "Expected :key expression item.id, got: {result}"
    );
}

// ==================== Template ref ====================

#[test]
fn template_ref_static() {
    // Vue 3.6: _setTemplateRef(n0, "el")
    let source = "<template><div ref=\"el\">content</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setTemplateRef(n0, \"el\")"),
        "Expected _setTemplateRef, got: {result}"
    );
}

// ==================== v-once / v-memo ====================

#[test]
fn v_once_with_dynamic_binding() {
    // Vue 3.6: v-once effects become direct statements (no _renderEffect)
    let source = "<template><div v-once :id=\"foo\">content</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setProp(n0, \"id\""),
        "Expected _setProp for v-once dynamic binding, got: {result}"
    );
    assert!(
        !result.contains("_renderEffect"),
        "v-once should NOT have _renderEffect, got: {result}"
    );
}

#[test]
fn v_once_with_interpolation() {
    // Vue 3.6: v-once interpolation is direct _setText (no _renderEffect)
    let source = "<template><div v-once>{{ msg }}</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setText("),
        "Expected _setText for v-once interpolation, got: {result}"
    );
    assert!(
        !result.contains("_renderEffect"),
        "v-once should NOT have _renderEffect, got: {result}"
    );
}

#[test]
fn v_memo_static() {
    // Vue 3.6: v-memo with static content is a no-op (just static template)
    let source = "<template><div v-memo=\"[dep]\">content</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_template("),
        "Expected _template for v-memo static content, got: {result}"
    );
    assert!(
        !result.contains("_renderEffect"),
        "v-memo static should NOT have _renderEffect, got: {result}"
    );
}

#[test]
fn v_memo_dynamic() {
    // v-memo with dynamic content wraps render effect in _withMemo
    let source = r#"<template><div v-memo="[x]" :class="cls">{{ msg }}</div></template>"#;
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_withMemo([x]"),
        "Expected _withMemo([x]) for v-memo dynamic content, got: {result}"
    );
    assert!(
        result.contains("_renderEffect"),
        "v-memo dynamic should have _renderEffect wrapper, got: {result}"
    );
    assert!(
        result.contains("_cache"),
        "v-memo should reference _cache, got: {result}"
    );
    assert!(
        !result.contains("v-memo"),
        "v-memo attribute must not appear in output, got: {result}"
    );
}

// ==================== enter/leave_template: empty ====================

#[test]
fn empty_template_returns_null() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    let options = make_options();
    let resolver = make_resolver(&alloc);

    let source = "<template></template>";
    let root = make_root(
        NodeTag {
            start: 0,
            end: 10,
            name_end: 9,
        },
        Some(NodeTag {
            start: 10,
            end: 21,
            name_end: 20,
        }),
        Some(RootNodeTemplateContent {
            start: 10,
            end: 10,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
    );
    let ast = make_empty_ast(&root);
    let mut gen = VaporCodeGen::new(&ast, resolver, "", &options);

    gen.enter_template(&root, source, &mut out);
    gen.leave_template(&root, source, &mut out);

    let result = apply_output(source, out, &alloc);
    assert!(result.contains("return null"));
    assert!(result.contains("function render("));
    assert!(result.ends_with('}'));
}

// ==================== single static element ====================

#[test]
fn single_static_element() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    let options = make_options();
    let resolver = make_resolver(&alloc);
    let source = "<template><div>hello</div></template>";

    let root = make_root(
        NodeTag {
            start: 0,
            end: 10,
            name_end: 9,
        },
        Some(NodeTag {
            start: 26,
            end: 37,
            name_end: 36,
        }),
        Some(RootNodeTemplateContent {
            start: 10,
            end: 26,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
    );
    let ast = make_empty_ast(&root);
    let mut gen = VaporCodeGen::new(&ast, resolver, source, &options);

    let element = ElementNode {
        tag_open: NodeTag {
            start: 10,
            end: 15,
            name_end: 14,
        },
        tag_close: Some(NodeTag {
            start: 20,
            end: 26,
            name_end: 25,
        }),
        tag_type: TagType::Element,
        is_self_closing: false,
        props: Vec::new(),
        content: Some(ElementContent {
            start: 15,
            end: 20,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
        v_condition: None,
        v_for: None,
        v_slot: None,
        v_once: None,
        v_ref: None,
        prop_flag: PropFlag::empty(),
        children_flag: ChildrenFlag::empty(),
        children_mode: ChildrenMode::TextOnlyStatic,
        is_fully_static: false,
    };

    let text = TextNode {
        start: 15,
        end: 20,
        is_entity: false,
        is_whitespace_only: false,
    };

    // Simulate the DFS walk
    gen.enter_template(&root, source, &mut out);
    gen.enter_element(NodeId(0), &element, None, source, &mut out);

    // Visit text child
    gen.visit_text(NodeId(1), &text, source, &mut out);

    // Fake OXC data not needed for static text
    gen.leave_element(NodeId(0), &element, None, source, &mut out);
    gen.leave_template(&root, source, &mut out);

    let result = apply_output(source, out, &alloc);
    // Should contain template declaration (minimized: no trailing close tag)
    assert!(result.contains("_template(\"<div>hello\""));
    // Should contain render function
    assert!(result.contains("function render("));
    // Should contain template instantiation
    assert!(result.contains("const n0 = t0()"));
    // Should return the node
    assert!(result.contains("return n0"));
    // No effects for static content
    assert!(!result.contains("_renderEffect"));
}

// ==================== inline mode ====================

#[test]
fn inline_mode_uses_arrow_function() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    let options = make_options_inline();
    let resolver = make_resolver(&alloc);
    let source = "<template><div></div></template>";

    let root = make_root(
        NodeTag {
            start: 0,
            end: 10,
            name_end: 9,
        },
        Some(NodeTag {
            start: 21,
            end: 32,
            name_end: 31,
        }),
        Some(RootNodeTemplateContent {
            start: 10,
            end: 21,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
    );
    let ast = make_empty_ast(&root);
    let mut gen = VaporCodeGen::new(&ast, resolver, source, &options);

    let element = ElementNode {
        tag_open: NodeTag {
            start: 10,
            end: 15,
            name_end: 14,
        },
        tag_close: Some(NodeTag {
            start: 15,
            end: 21,
            name_end: 20,
        }),
        tag_type: TagType::Element,
        is_self_closing: false,
        props: Vec::new(),
        content: Some(ElementContent {
            start: 15,
            end: 15,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
        v_condition: None,
        v_for: None,
        v_slot: None,
        v_once: None,
        v_ref: None,
        prop_flag: PropFlag::empty(),
        children_flag: ChildrenFlag::empty(),
        children_mode: ChildrenMode::Empty,
        is_fully_static: false,
    };

    gen.enter_template(&root, source, &mut out);
    gen.enter_element(NodeId(0), &element, None, source, &mut out);
    gen.leave_element(NodeId(0), &element, None, source, &mut out);
    gen.leave_template(&root, source, &mut out);

    let result = apply_output(source, out, &alloc);
    assert!(result.contains("return (_ctx,_cache) => {"));
}

// ==================== element with dynamic text ====================

#[test]
fn element_with_interpolation() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    let options = make_options();
    let resolver = make_resolver(&alloc);
    // <template><div>{{ msg }}</div></template>
    let source = "<template><div>{{ msg }}</div></template>";

    let root = make_root(
        NodeTag {
            start: 0,
            end: 10,
            name_end: 9,
        },
        Some(NodeTag {
            start: 29,
            end: 40,
            name_end: 39,
        }),
        Some(RootNodeTemplateContent {
            start: 10,
            end: 29,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
    );
    let ast = make_empty_ast(&root);
    let mut gen = VaporCodeGen::new(&ast, resolver, source, &options);

    let element = ElementNode {
        tag_open: NodeTag {
            start: 10,
            end: 15,
            name_end: 14,
        },
        tag_close: Some(NodeTag {
            start: 23,
            end: 29,
            name_end: 28,
        }),
        tag_type: TagType::Element,
        is_self_closing: false,
        props: Vec::new(),
        content: Some(ElementContent {
            start: 15,
            end: 23,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        }),
        v_condition: None,
        v_for: None,
        v_slot: None,
        v_once: None,
        v_ref: None,
        prop_flag: PropFlag::empty(),
        children_flag: ChildrenFlag::empty().add(ChildrenFlags::HasInterpolation),
        children_mode: ChildrenMode::TextOnlyDynamic,
        is_fully_static: false,
    };

    let interp = InterpolationNode {
        start: 15,
        end: 23,
        inner_start: 18,
        inner_end: 21,
    };

    // We need a dummy OxcParsedExpression for the trait call
    let oxc_expr = OxcParsedExpression {
        offset: 0,
        expression: None,
        errors: None,
        bindings: None,
        dynamism: Dynamism::Static,
    };

    gen.enter_template(&root, source, &mut out);
    gen.enter_element(NodeId(0), &element, None, source, &mut out);
    gen.visit_interpolation(NodeId(1), &interp, &oxc_expr, source, &mut out);
    gen.leave_element(NodeId(0), &element, None, source, &mut out);
    gen.leave_template(&root, source, &mut out);

    let result = apply_output(source, out, &alloc);

    // Should have template with space placeholder (minimized: no trailing close tag)
    assert!(result.contains("_template(\"<div> \""));
    // Should have render effect with setText
    assert!(result.contains("_renderEffect"));
    assert!(result.contains("_setText"));
    assert!(result.contains("_toDisplayString"));
    assert!(result.contains("_ctx.msg"));
}

// ==================== Non-root component / slot outlet ====================

/// @ai-generated — Non-root component emits _setInsertionState
#[test]
fn non_root_component_emits_insertion_state() {
    let source = "<template><div><my-comp></my-comp></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setInsertionState("),
        "Expected _setInsertionState for non-root component, got: {result}"
    );
    assert!(
        result.contains("_createComponentWithFallback("),
        "Expected _createComponentWithFallback, got: {result}"
    );
    // The parent div should still have a template
    assert!(
        result.contains("_template("),
        "Expected _template for parent div, got: {result}"
    );
}

/// @ai-generated — Non-root slot outlet emits _setInsertionState
#[test]
fn non_root_slot_emits_insertion_state() {
    let source = "<template><div><slot></slot></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setInsertionState("),
        "Expected _setInsertionState for non-root slot, got: {result}"
    );
    assert!(
        result.contains("_createSlot(\"default\""),
        "Expected _createSlot, got: {result}"
    );
}

/// @ai-generated — Non-root named slot emits _setInsertionState
#[test]
fn non_root_named_slot_emits_insertion_state() {
    let source = "<template><div><slot name=\"header\"></slot></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setInsertionState("),
        "Expected _setInsertionState for non-root named slot, got: {result}"
    );
    assert!(
        result.contains("_createSlot(\"header\""),
        "Expected _createSlot(\"header\"), got: {result}"
    );
}

/// @ai-generated — Non-root component with sibling gets correct child index
#[test]
fn non_root_component_after_sibling() {
    let source = "<template><div><span>text</span><my-comp></my-comp></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setInsertionState("),
        "Expected _setInsertionState, got: {result}"
    );
    // The component is the second child (index 1) — insertion state should reflect this
    assert!(
        result.contains(", null, 1, true)"),
        "Expected child index 1 for component after span, got: {result}"
    );
}

// ==================== Named & scoped slots ====================

/// @ai-generated — Named slot via <template #header>
#[test]
fn component_with_named_slot() {
    let source =
        "<template><my-comp><template #header><span>Header</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("header: () => {"),
        "Expected named slot 'header' closure, got: {result}"
    );
    assert!(
        !result.contains("default: () => {"),
        "Should not have default slot when only named slot is present, got: {result}"
    );
}

/// @ai-generated — Multiple named slots
#[test]
fn component_with_multiple_named_slots() {
    let source = "<template><my-comp><template #header><span>Header</span></template><template #footer><span>Footer</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("header: () => {"),
        "Expected named slot 'header', got: {result}"
    );
    assert!(
        result.contains("footer: () => {"),
        "Expected named slot 'footer', got: {result}"
    );
    assert!(
        result.contains(", _: 2 }"),
        "Expected slot flags, got: {result}"
    );
}

/// @ai-generated — Named slot + implicit default slot from non-template children
#[test]
fn component_with_named_and_default_slots() {
    let source = "<template><my-comp><template #header><span>Header</span></template><span>Default content</span></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("header: () => {"),
        "Expected named slot 'header', got: {result}"
    );
    assert!(
        result.contains("default: () => {"),
        "Expected implicit default slot, got: {result}"
    );
}

/// @ai-generated — Scoped slot with destructured params
#[test]
fn component_with_scoped_slot() {
    let source = "<template><my-comp><template #default=\"{ item }\"><span>{{ item }}</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("default: ({ item }) => {"),
        "Expected scoped default slot with params, got: {result}"
    );
}

/// @ai-generated — Named scoped slot
#[test]
fn component_with_named_scoped_slot() {
    let source = "<template><my-comp><template #header=\"{ title }\"><span>{{ title }}</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("header: ({ title }) => {"),
        "Expected scoped named slot 'header' with params, got: {result}"
    );
}

/// @ai-generated — Bare v-slot (no name) defaults to "default"
#[test]
fn component_with_bare_v_slot() {
    let source =
        "<template><my-comp><template v-slot><span>Content</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("default: () => {"),
        "Expected bare v-slot to produce default slot, got: {result}"
    );
}

/// @ai-generated — Slot name with hyphen gets quoted
#[test]
fn component_with_hyphenated_slot_name() {
    let source = "<template><my-comp><template #my-slot><span>Content</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"my-slot\": () => {"),
        "Expected quoted hyphenated slot name, got: {result}"
    );
}

// ==================== Built-in components ====================

/// @ai-generated — Transition uses direct import, not _resolveComponent
#[test]
fn builtin_component_transition() {
    let source = "<template><Transition name=\"fade\"><div>content</div></Transition></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_Transition"),
        "Expected _Transition helper, got: {result}"
    );
    assert!(
        !result.contains("_resolveComponent"),
        "Built-in should not use _resolveComponent, got: {result}"
    );
}

/// @ai-generated — KeepAlive uses direct import
#[test]
fn builtin_component_keep_alive() {
    let source = "<template><KeepAlive><div>content</div></KeepAlive></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_KeepAlive"),
        "Expected _KeepAlive helper, got: {result}"
    );
    assert!(
        !result.contains("_resolveComponent"),
        "Built-in should not use _resolveComponent, got: {result}"
    );
}

/// @ai-generated — Teleport uses direct import
#[test]
fn builtin_component_teleport() {
    let source = "<template><Teleport to=\"body\"><div>content</div></Teleport></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_Teleport"),
        "Expected _Teleport helper, got: {result}"
    );
    assert!(
        !result.contains("_resolveComponent"),
        "Built-in should not use _resolveComponent, got: {result}"
    );
}

/// @ai-generated — Suspense uses direct import
#[test]
fn builtin_component_suspense() {
    let source = "<template><Suspense><div>content</div></Suspense></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_Suspense"),
        "Expected _Suspense helper, got: {result}"
    );
    assert!(
        !result.contains("_resolveComponent"),
        "Built-in should not use _resolveComponent, got: {result}"
    );
}

/// @ai-generated — kebab-case keep-alive resolves to built-in
#[test]
fn builtin_component_kebab_case_keep_alive() {
    let source = "<template><keep-alive><div>content</div></keep-alive></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_KeepAlive"),
        "Expected _KeepAlive for kebab-case keep-alive, got: {result}"
    );
    assert!(
        !result.contains("_resolveComponent"),
        "Built-in should not use _resolveComponent, got: {result}"
    );
}

// ==================== Byte-identical codegen contract ====================
//
// These goldens pin the EXACT Vapor render output for a representative corpus:
// nested elements, sibling runs, mixed static/dynamic children, components,
// named slots, slot outlets, v-if/v-else, v-for, multiple roots, void
// elements, and deep dynamic subtrees. Vapor codegen emits a deterministic
// LF-terminated string, so every case is compared RAW (no end-of-line folding)
// against its in-source expected output: any single-byte drift — including a
// stray CRLF — fails the contract. The `deep_dynamic` case deliberately
// exercises nav lines that reference outer node refs so the comparison covers
// that shape too.

/// (case name, `.vue` source, exact expected Vapor render output).
const BYTE_IDENTICAL_CORPUS: &[(&str, &str, &str)] = &[
    (
        "nested",
        "<template><div><span><b>deep</b></span></div></template>",
        "const t0 = _template(\"<div><span><b>deep\", true)\nfunction render(_ctx) {\n  const n0 = t0()\n  return n0\n}",
    ),
    (
        "siblings",
        "<template><div><span>a</span><span>b</span><span>c</span></div></template>",
        "const t0 = _template(\"<div><span>a</span><span>b</span><span>c\", true)\nfunction render(_ctx) {\n  const n0 = t0()\n  return n0\n}",
    ),
    (
        "mixed_static_dynamic_children",
        "<template><div><span>static</span><p>{{ msg }}</p><span>more</span></div></template>",
        "const t0 = _template(\"<div><span>static</span><p> </p><span>more\", true)\nfunction render(_ctx) {\n  const n0 = t0()\n  const p0 = _next(n0, 1)\n  const x0 = _txt(p0)\n  _renderEffect(() => {\n    _setText(x0, _toDisplayString(_ctx.msg))\n  })\n  return n0\n}",
    ),
    (
        "dynamic_after_siblings",
        "<template><div><a>x</a><b>y</b><c>{{ z }}</c></div></template>",
        "const t1 = _template(\"<div><a>x</a><b>y\", true)\nconst t0 = _template(\" \", true)\nfunction render(_ctx) {\n  const n1 = t1()\n  _setInsertionState(n1, null, 2, true)\n  const _component_c = _resolveComponent(\"c\")\n  const n0 = _createComponentWithFallback(_component_c, null, { default: () => {\n      const n0 = t0()\n      _renderEffect(() => _setText(x0, _toDisplayString(_ctx.z)))\n      return n0\n    }, _: 2 })\n  return n1\n}",
    ),
    (
        "text_interp_coalesce",
        "<template><div>hello {{ name }} world</div></template>",
        "const t0 = _template(\"<div>hello   world\", true)\nfunction render(_ctx) {\n  const n0 = t0()\n  _renderEffect(() => {\n    _setText(x0, \"hello \" + _toDisplayString(_ctx.name) + \" world\")\n  })\n  return n0\n}",
    ),
    (
        "comment_between_siblings",
        "<template><div><span>a</span><!-- c --><p>{{ b }}</p></div></template>",
        "const t0 = _template(\"<div><span>a</span><!-- c --><p> \", true)\nfunction render(_ctx) {\n  const n0 = t0()\n  const p0 = _next(n0, 2)\n  const x0 = _txt(p0)\n  _renderEffect(() => {\n    _setText(x0, _toDisplayString(_ctx.b))\n  })\n  return n0\n}",
    ),
    (
        "component",
        "<template><div><MyComp :title=\"t\">child</MyComp></div></template>",
        "const t1 = _template(\"<div>\", true)\nconst t0 = _template(\"child\", true)\nfunction render(_ctx) {\n  const n1 = t1()\n  _setInsertionState(n1, null, 0, true)\n  const _component_MyComp = _resolveComponent(\"MyComp\")\n  const n0 = _createComponentWithFallback(_component_MyComp, { title: () => (_ctx.t) }, { default: () => {\n      const n0 = t0()\n      return n0\n    }, _: 2 })\n  return n1\n}",
    ),
    (
        "component_named_slots",
        "<template><MyComp><template #header>H {{ x }}</template>default</MyComp></template>",
        "const t0 = _template(\"H  \", true)\nconst t1 = _template(\"default\", true)\nfunction render(_ctx) {\n  const _component_MyComp = _resolveComponent(\"MyComp\")\n  const n1 = _createComponentWithFallback(_component_MyComp, null, { header: () => {\n      const n0 = t0()\n      _renderEffect(() => _setText(x0, \"H \" + _toDisplayString(_ctx.x)))\n      return n0\n    }, default: () => {\n      const n1 = t1()\n      return n1\n    }, _: 2 })\n  return n1\n}",
    ),
    (
        "slot_outlet",
        "<template><div><slot name=\"head\"/></div></template>",
        "const t0 = _template(\"<div>\", true)\nfunction render(_ctx) {\n  const n1 = t0()\n  _setInsertionState(n1, null, 0, true)\n  const n0 = _createSlot(\"head\", null)\n  return n1\n}",
    ),
    (
        "v_if_else",
        "<template><div v-if=\"a\">A {{ x }}</div><div v-else>B</div></template>",
        "const t0 = _template(\"<div>A  \", true)\nconst t1 = _template(\"<div>B\", true)\nfunction render(_ctx) {\n  const n1 = _createIf(() => (_ctx.a), () => {\n    const n0 = t0()\n    _renderEffect(() => _setText(x0, \"A \" + _toDisplayString(_ctx.x)))\n    return n0\n  }, () => {\n    const n2 = t1()\n    return n2\n})\n  return n1\n}",
    ),
    (
        "v_for",
        "<template><li v-for=\"item in items\">{{ item }}</li></template>",
        "const t0 = _template(\"<li> \", true)\nfunction render(_ctx) {\n  const n0 = _createFor(() => (_ctx.items), (item) => {\n    const n1 = t0()\n    _renderEffect(() => _setText(x0, _toDisplayString(item)))\n    return n1\n  })\n  return n0\n}",
    ),
    (
        "multi_root",
        "<template><div>a</div><span>{{ b }}</span></template>",
        "const t0 = _template(\"<div>a\", true)\nconst t1 = _template(\"<span> \", true)\nfunction render(_ctx) {\n  const n0 = t0()\n  const n1 = t1()\n  _renderEffect(() => {\n    _setText(x0, _toDisplayString(_ctx.b))\n  })\n  return [n0, n1]\n}",
    ),
    (
        "void_self_closing",
        "<template><div><br/><img src=\"x\"></div></template>",
        "const t0 = _template(\"<div><br><img src=x>\", true)\nfunction render(_ctx) {\n  const n0 = t0()\n  return n0\n}",
    ),
    (
        "static_and_dynamic_attrs",
        "<template><div class=\"c\" :id=\"i\">{{ m }}</div></template>",
        "const t0 = _template(\"<div class=c> \", true)\nfunction render(_ctx) {\n  const n0 = t0()\n  _renderEffect(() => {\n    _setProp(n0, \"id\", _ctx.i)\n    _setText(x0, _toDisplayString(_ctx.m))\n  })\n  return n0\n}",
    ),
    (
        "deep_dynamic",
        "<template><div><section><article><p>{{ deep }}</p></article></section></div></template>",
        "const t0 = _template(\"<div><section><article><p> \", true)\nfunction render(_ctx) {\n  const n2 = t0()\n  const p2 = _child(n2)\n  const p1 = _child(n1)\n  const p0 = _child(n0)\n  const x0 = _txt(p0)\n  _renderEffect(() => {\n    _setText(x0, _toDisplayString(_ctx.deep))\n  })\n  return n2\n}",
    ),
    (
        "component_in_element_with_trailing_text",
        "<template><div><MyComp>x</MyComp>after {{ a }}</div></template>",
        "const t1 = _template(\"<div>after  \", true)\nconst t0 = _template(\"x\", true)\nfunction render(_ctx) {\n  const n1 = t1()\n  _renderEffect(() => {\n    _setText(x0, \"after \" + _toDisplayString(_ctx.a))\n  })\n  _setInsertionState(n1, null, 0, true)\n  const _component_MyComp = _resolveComponent(\"MyComp\")\n  const n0 = _createComponentWithFallback(_component_MyComp, null, { default: () => {\n      const n0 = t0()\n      return n0\n    }, _: 2 })\n  return n1\n}",
    ),
];

/// Emitted Vapor render code must stay byte-for-byte identical across the whole
/// corpus. Full-string equality compared RAW (not substring checks, no
/// end-of-line folding) makes any single-byte drift fail loudly.
#[test]
fn vapor_codegen_is_byte_identical_across_corpus() {
    for (name, source, expected) in BYTE_IDENTICAL_CORPUS {
        let actual = run_full_pipeline(source);
        assert_eq!(
            actual, *expected,
            "Vapor output drifted for case `{name}`.\n--- source ---\n{source}\n--- actual ---\n{actual}"
        );
    }
}

/// A node's DOM child index is the count of its preceding DOM siblings:
/// adjacent text/interpolation runs coalesce to one child, comments count only
/// when enabled, and every element counts once. The running child cursor must
/// reproduce this exactly; an off-by-one would corrupt the emitted
/// `_next(n, IDX)` / `_setInsertionState(n, null, IDX, true)` index. Each fixture
/// is chosen so a mis-maintained cursor lands on the wrong integer.
///
/// This pins the index VALUES and binds them to the mechanism that must produce
/// them. A correct per-child preceding-sibling rescan emits the very same
/// integers, so the value assertions alone cannot distinguish a running cursor
/// from the rescan path; the closing source check ties the pinned values to the
/// `observe_dom_*` cursor observers and asserts the `compute_dom_child_index`
/// rescan is gone, so reintroducing it fails THIS test — not only its companion
/// `dom_child_index_comes_from_running_cursor_not_per_child_rescan`.
#[test]
fn dom_child_index_counts_preceding_siblings_exactly() {
    // A dynamic element after three static element siblings → DOM index 3.
    let r = run_full_pipeline(
        "<template><div><span>0</span><span>1</span><span>2</span><span>{{ d }}</span></div></template>",
    );
    assert!(
        r.contains("_next(n0, 3)"),
        "dynamic 4th element child must be DOM index 3, got:\n{r}"
    );
    assert!(
        !r.contains("_next(n0, 2)") && !r.contains("_next(n0, 4)"),
        "DOM index off-by-one for the 4th child, got:\n{r}"
    );

    // span(0) comment(1) p(2): an enabled comment advances the index.
    let r =
        run_full_pipeline("<template><div><span>a</span><!-- c --><p>{{ b }}</p></div></template>");
    assert!(
        r.contains("_next(n0, 2)"),
        "an enabled comment must advance the DOM index to 2, got:\n{r}"
    );

    // A component after two element siblings is inserted at index 2.
    let r = run_full_pipeline("<template><div><a>x</a><b>y</b><c>{{ z }}</c></div></template>");
    assert!(
        r.contains("_setInsertionState(n1, null, 2, true)"),
        "component after two siblings must insert at DOM index 2, got:\n{r}"
    );

    // Coalescing: a leading text+interpolation run is ONE child, so the
    // following element is index 1 (not 2). `<div>t{{i}}<span>{{d}}</span></div>`
    // → the span is DOM child 1.
    let r = run_full_pipeline("<template><div>t{{ i }}<span>{{ d }}</span></div></template>");
    assert!(
        r.contains("_next(n0, 1)"),
        "a leading text+interpolation run coalesces to one child; the span must be DOM index 1, got:\n{r}"
    );

    // Bind those pinned values to their mechanism. A correct preceding-sibling
    // rescan emits the same integers, so the checks above cannot by themselves
    // tell a running cursor apart from the per-child `compute_dom_child_index`
    // rescan that produced identical indices. Reading the walker source closes
    // that gap: the indices must flow from the running cursor observers
    // (elements via `observe_dom_element`, coalescing text/interpolation via
    // `observe_dom_text_run`, rendered comments via `observe_dom_comment`), and
    // the rescan helper must be gone — reintroducing it makes THIS test fail.
    let mod_rs = vapor_production_source("mod.rs");
    let element_rs = vapor_production_source("element.rs");
    assert!(
        mod_rs.contains("observe_dom_element")
            && mod_rs.contains("observe_dom_text_run")
            && mod_rs.contains("observe_dom_comment"),
        "the pinned DOM indices must come from the running cursor observers \
         (`observe_dom_element` / `observe_dom_text_run` / `observe_dom_comment`)"
    );
    assert!(
        !mod_rs.contains("compute_dom_child_index")
            && !element_rs.contains("compute_dom_child_index"),
        "the per-child preceding-sibling rescan `compute_dom_child_index` emitted these same \
         indices; it must be absent so the pinned values come only from the running cursor"
    );
}

/// The DOM child index is produced by a running cursor — advanced once per
/// observed child — NOT by rescanning a node's preceding siblings on every
/// child. A value-only check cannot tell the two apart, because the deleted
/// per-child rescan produced the very same indices; this guard is therefore
/// structural. It is RED against any tree that still carries the rescan helper
/// and GREEN only once the cursor observers own the index.
#[test]
fn dom_child_index_comes_from_running_cursor_not_per_child_rescan() {
    let mod_rs = vapor_production_source("mod.rs");
    let element_rs = vapor_production_source("element.rs");

    // Negative: the per-child preceding-sibling rescan helper is gone. While it
    // existed it produced the same indices a running cursor does, so only a
    // structural check — not an emitted-value check — distinguishes them.
    assert!(
        !mod_rs.contains("compute_dom_child_index")
            && !element_rs.contains("compute_dom_child_index"),
        "the per-child preceding-sibling rescan `compute_dom_child_index` must be deleted; \
         the DOM child index is read from a running cursor instead"
    );

    // Positive: the walk advances one running cursor per child and reads each
    // element's index from it — elements via `observe_dom_element`, coalescing
    // text/interpolation via `observe_dom_text_run`, rendered comments via
    // `observe_dom_comment`.
    assert!(
        mod_rs.contains("observe_dom_element"),
        "each element's DOM index must come from the running cursor (`observe_dom_element`)"
    );
    assert!(
        mod_rs.contains("observe_dom_text_run") && mod_rs.contains("observe_dom_comment"),
        "text/interpolation runs and rendered comments must advance the running cursor via \
         `observe_dom_text_run` / `observe_dom_comment`"
    );
}

/// Read the production portion of a Vapor codegen source file — everything
/// before its first `#[cfg(test)]` attribute, so test-only code in that file
/// never trips the production-code guards below. The file is located via
/// `CARGO_MANIFEST_DIR` + `Path::join` so the scan is cross-platform and
/// independent of the test's working directory.
fn vapor_production_source(file: &str) -> String {
    use std::path::Path;

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("template")
        .join("code_gen")
        .join("vapor")
        .join(file);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read vapor source `{}`: {e}", path.display()));
    src.split("#[cfg(test)]").next().unwrap_or(&src).to_string()
}

/// The render body is assembled through the reusable format-sink arena
/// (`alloc_fmt` → one bump per line) and emitted via a single `CodeTransform`
/// overwrite of the `<template>` span; it is NEVER produced by editing
/// already-emitted output in place. Two guards, both discriminating:
///
/// 1. No Vapor codegen source edits already-emitted output in place
///    (`String::replace`/`replacen`, `replace_range`, or `splice`) — any such
///    call would desync the source map from the emitted bytes. The free
///    function `std::mem::replace` (buffer handoff) reads as `::replace(`, not
///    the method form `.replace(`, so it is correctly NOT matched.
/// 2. The navigation/text-creation merge path goes through `alloc_fmt`, and the
///    superseded `build_child_nav` heap-`String` builder is gone — this fails
///    against a tree that still assembles those lines with a per-line `String`.
#[test]
fn vapor_render_lines_use_format_sink_not_in_place_string_edits() {
    const PRODUCTION_FILES: &[&str] = &[
        "mod.rs",
        "element.rs",
        "text.rs",
        "interpolation.rs",
        "comment.rs",
        "props.rs",
    ];
    const FORBIDDEN_IN_PLACE_EDITS: &[&str] =
        &[".replace(", ".replacen(", ".replace_range(", ".splice("];

    for file in PRODUCTION_FILES {
        let prod = vapor_production_source(file);
        for needle in FORBIDDEN_IN_PLACE_EDITS {
            assert!(
                !prod.contains(needle),
                "{file}: editing already-emitted output in place (`{needle}`) is forbidden in \
                 Vapor codegen; assemble lines through `alloc_fmt` and apply via a `CodeTransform` \
                 op instead"
            );
        }
    }

    // Positive: the merge path assembles nav/text-creation lines via the
    // format-sink arena, and the old heap-`String` nav builder is removed.
    let element_rs = vapor_production_source("element.rs");
    assert!(
        element_rs.contains("alloc_fmt(format_args!"),
        "element.rs merge must assemble navigation via `out.alloc_fmt(format_args!(...))`"
    );
    assert!(
        !element_rs.contains("fn build_child_nav"),
        "the superseded `build_child_nav` heap-String navigation builder must be deleted"
    );
}
