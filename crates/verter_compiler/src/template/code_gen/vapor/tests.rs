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
    let prepared = crate::script::prepared::PreparedScript::build(
        source,
        syntax.script(),
        syntax.script_setup(),
        &alloc,
    );
    let script_result = generate_script(
        syntax.script(),
        syntax.script_setup(),
        &prepared,
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
    // Official `generate()`: non-inline vapor always gets the 5-param
    // signature — `bindingMetadata` defaults to `{}` for `vapor && !ssr`
    // (`compiler-sfc.cjs.js`; rc.3 script-less `slots.vue`).
    let source = "<template><div>hi</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("function render(_ctx, $props, $emit, $attrs, $slots)"),
        "Expected Vapor render signature 'function render(_ctx, $props, $emit, $attrs, $slots)', got: {result}"
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
fn event_click_bare_uses_on_not_delegated() {
    // Official rc.3 `transformVOn`: delegation is opt-in via `.delegate`
    // (`isDelegatableEvent = !!delegateModifier && arg.isStatic &&
    // delegatedEvents(arg.content)`). Bare `@click="handler"` uses `_on()`
    // (`vue/props-emit__vapor__*`).
    let source = "<template><button @click=\"handler\">click</button></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_on(n0, \"click\", "),
        "Expected _on(n0, \"click\", ...) for a bare (unmodified) click handler, got: {result}"
    );
    assert!(
        !result.contains("_delegateEvents") && !result.contains("$evtclick") && !result.contains("_createInvoker"),
        "A bare click handler must NOT delegate without an explicit .delegate modifier, got: {result}"
    );
}

#[test]
fn event_click_delegate_modifier_delegates() {
    // Positive control: `.delegate` activates `_createInvoker` + `$evtclick` + `_delegateEvents`.
    let source = "<template><button @click.delegate=\"handler\">click</button></template>";
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
fn event_multiple_delegate_modifier_delegates() {
    // Multiple events, both explicitly opted into delegation.
    let source =
        "<template><button @click.delegate=\"a\" @mouseover.delegate=\"b\">hi</button></template>";
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

#[test]
fn event_multiple_bare_use_on_not_delegated() {
    // Negative control: without `.delegate`, all events bind via `_on()`.
    let source = "<template><button @click=\"a\" @mouseover=\"b\">hi</button></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_on(n0, \"click\", ") && result.contains("_on(n0, \"mouseover\", "),
        "Expected both events bound via _on(), got: {result}"
    );
    assert!(
        !result.contains("_delegateEvents")
            && !result.contains("$evtclick")
            && !result.contains("$evtmouseover"),
        "Bare handlers must not delegate, got: {result}"
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
    // Official rc.3: unnamed slot with no fallback omits trailing defaults —
    // `_createSlot()`, not `_createSlot("default", null)`.
    let source = "<template><slot></slot></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createSlot()"),
        "Expected _createSlot() with all trailing default args omitted, got: {result}"
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
    // Official v-for renames loop vars to `_for_item{depth}`/`_for_key{depth}`
    // (position 1 is always "key"). User names do not survive the signature.
    let source = "<template><div v-for=\"(item, index) in items\">{{ item }}</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("(_for_item0, _for_key0) => {"),
        "Expected renamed params (_for_item0, _for_key0), got: {result}"
    );
    assert!(
        !result.contains("(item, index) => {"),
        "the user's own variable names must not survive into the closure \
         signature, got: {result}"
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
        multi_statement: false,
        errors: None,
        bindings: None,
        ide_recovery_scope: Vec::new(),
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
    // Unnamed fallback-less slot omits trailing defaults; only content of the
    // div, so insertion is 1-arg append.
    assert!(
        result.contains("_createSlot()"),
        "Expected _createSlot() with all trailing default args omitted, got: {result}"
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
    // Second child (index 1), mounted once — numeric index, no `<!>`.
    // Official rc.3: `_setInsertionState(n2, 2)`, not the former 4-arg call.
    assert!(
        result.contains(", 1)"),
        "Expected child index 1 for component after span, got: {result}"
    );
    assert!(
        !result.contains("<!>"),
        "a mounted-once component never needs a persistent anchor, got: {result}"
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
        "const t0 = _template(\"<div><span><b>deep\", 3)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n0 = t0()\n  return n0\n}",
    ),
    (
        "siblings",
        "<template><div><span>a</span><span>b</span><span>c</span></div></template>",
        "const t0 = _template(\"<div><span>a</span><span>b</span><span>c\", 3)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n0 = t0()\n  return n0\n}",
    ),
    (
        "mixed_static_dynamic_children",
        "<template><div><span>static</span><p>{{ msg }}</p><span>more</span></div></template>",
        // Dynamic <p> text ref consumes id 0 before the root nav ref.
        "const t0 = _template(\"<div><span>static</span><p> </p><span>more\", 1)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n1 = t0()\n  const p0 = _next(n1, 1)\n  const x0 = _txt(p0)\n  _renderEffect(() => {\n    _setText(x0, _toDisplayString(_ctx.msg))\n  })\n  return n1\n}",
    ),
    (
        "dynamic_after_siblings",
        "<template><div><a>x</a><b>y</b><c>{{ z }}</c></div></template>",
        // (1) Hoisted templates emit in allocation-index order (nested
        // closure template before enclosing root — DFS visits children first).
        // (2) Mounted-once component uses 2-arg `_setInsertionState(parent, index)`.
        "const t0 = _template(\" \")\nconst t1 = _template(\"<div><a>x</a><b>y\", 1)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n1 = t1()\n  _setInsertionState(n1, 2)\n  const _component_c = _resolveComponent(\"c\")\n  const n0 = _createComponentWithFallback(_component_c, null, { default: () => {\n      const n0 = t0()\n      const x0 = _txt(n0)\n      _renderEffect(() => _setText(x0, _toDisplayString(_ctx.z)))\n      return n0\n    }, _: 2 })\n  return n1\n}",
    ),
    (
        "text_interp_coalesce",
        "<template><div>hello {{ name }} world</div></template>",
        "const t0 = _template(\"<div>hello   world\", 1)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n0 = t0()\n  const x0 = _txt(n0)\n  _renderEffect(() => {\n    _setText(x0, \"hello \" + _toDisplayString(_ctx.name) + \" world\")\n  })\n  return n0\n}",
    ),
    (
        "comment_between_siblings",
        "<template><div><span>a</span><!-- c --><p>{{ b }}</p></div></template>",
        // Dynamic <p> text ref consumes id 0 before the root nav ref.
        "const t0 = _template(\"<div><span>a</span><!-- c --><p> \", 1)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n1 = t0()\n  const p0 = _next(n1, 2)\n  const x0 = _txt(p0)\n  _renderEffect(() => {\n    _setText(x0, _toDisplayString(_ctx.b))\n  })\n  return n1\n}",
    ),
    (
        "component",
        "<template><div><MyComp :title=\"t\">child</MyComp></div></template>",
        // Nested closure template before enclosing root; 1-arg append
        // (`_setInsertionState(n2)` — component is the div's only content).
        "const t0 = _template(\"child\", 2)\nconst t1 = _template(\"<div>\", 1)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n1 = t1()\n  _setInsertionState(n1)\n  const _component_MyComp = _resolveComponent(\"MyComp\")\n  const n0 = _createComponentWithFallback(_component_MyComp, { title: () => (_ctx.t) }, { default: () => {\n      const n0 = t0()\n      return n0\n    }, _: 2 })\n  return n1\n}",
    ),
    (
        "component_named_slots",
        "<template><MyComp><template #header>H {{ x }}</template>default</MyComp></template>",
        "const t0 = _template(\"H  \")\nconst t1 = _template(\"default\", 2)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const _component_MyComp = _resolveComponent(\"MyComp\")\n  const n1 = _createComponentWithFallback(_component_MyComp, null, { header: () => {\n      const n0 = t0()\n      const x0 = _txt(n0)\n      _renderEffect(() => _setText(x0, \"H \" + _toDisplayString(_ctx.x)))\n      return n0\n    }, default: () => {\n      const n1 = t1()\n      return n1\n    }, _: 2 })\n  return n1\n}",
    ),
    (
        "slot_outlet",
        "<template><div><slot name=\"head\"/></div></template>",
        // 1-arg append; named fallback-less slot omits `props: null`
        // (`_createSlot("head")`, not `_createSlot("head", null)`).
        "const t0 = _template(\"<div>\", 1)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n1 = t0()\n  _setInsertionState(n1)\n  const n0 = _createSlot(\"head\")\n  return n1\n}",
    ),
    (
        "v_if_else",
        "<template><div v-if=\"a\">A {{ x }}</div><div v-else>B</div></template>",
        // Flags match official `genIfFlags` (rc.3 `basic-interpolation.vue`
        // v-if/v-else: 325, same shape). Id order: construct-own, wasted
        // branch-entry, then content (`n0`=if, `n2`=true, `n4`=false; 1 and 3
        // wasted, never printed).
        "const t0 = _template(\"<div>A  \")\nconst t1 = _template(\"<div>B\", 2)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n0 = _createIf(() => (_ctx.a), () => {\n    const n2 = t0()\n    const x2 = _txt(n2)\n    _renderEffect(() => _setText(x2, \"A \" + _toDisplayString(_ctx.x)))\n    return n2\n  }, () => {\n    const n4 = t1()\n    return n4\n}, 325 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, FALSE_NO_SCOPE, KEYED_INDEX_0 */)\n  return n0\n}",
    ),
    (
        "v_for",
        "<template><li v-for=\"item in items\">{{ item }}</li></template>",
        // For-construct id `n0`, one wasted item-entry id, then content `n2`
        // (same `enterBlock` pattern as v-if). Loop vars renamed
        // `_for_item0`/`_for_item0.value`. Flags-present + no `:key` uses
        // `undefined` in the key slot before `8 /* IS_SINGLE_NODE */`.
        "const t0 = _template(\"<li> \")\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n0 = _createFor(() => (_ctx.items), (_for_item0) => {\n    const n2 = t0()\n    const x2 = _txt(n2)\n    _renderEffect(() => _setText(x2, _toDisplayString(_for_item0.value)))\n    return n2\n  }, undefined, 8 /* IS_SINGLE_NODE */)\n  return n0\n}",
    ),
    (
        "multi_root",
        "<template><div>a</div><span>{{ b }}</span></template>",
        // `x`/`n` share one counter — span text ref is `x1`, not `x0`.
        "const t0 = _template(\"<div>a\", 2)\nconst t1 = _template(\"<span> \")\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n0 = t0()\n  const n1 = t1()\n  const x1 = _txt(n1)\n  _renderEffect(() => {\n    _setText(x1, _toDisplayString(_ctx.b))\n  })\n  return [n0, n1]\n}",
    ),
    (
        "void_self_closing",
        "<template><div><br/><img src=\"x\"></div></template>",
        "const t0 = _template(\"<div><br><img src=x>\", 3)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n0 = t0()\n  return n0\n}",
    ),
    (
        "static_and_dynamic_attrs",
        "<template><div class=\"c\" :id=\"i\">{{ m }}</div></template>",
        "const t0 = _template(\"<div class=c> \", 1)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n0 = t0()\n  const x0 = _txt(n0)\n  _renderEffect(() => {\n    _setProp(n0, \"id\", _ctx.i)\n    _setText(x0, _toDisplayString(_ctx.m))\n  })\n  return n0\n}",
    ),
    (
        "deep_dynamic",
        "<template><div><section><article><p>{{ deep }}</p></article></section></div></template>",
        // Deep interpolation text ref (x0) consumes id 0 before root nav
        // (n/x merge shifts root from n2 to n3).
        "const t0 = _template(\"<div><section><article><p> \", 1)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n3 = t0()\n  const p2 = _child(n3)\n  const p1 = _child(n2)\n  const p0 = _child(n1)\n  const x0 = _txt(p0)\n  _renderEffect(() => {\n    _setText(x0, _toDisplayString(_ctx.deep))\n  })\n  return n3\n}",
    ),
    (
        "component_in_element_with_trailing_text",
        "<template><div><MyComp>x</MyComp>after {{ a }}</div></template>",
        // Following dynamic text in the same parent needs `_next()` past
        // this position → persistent `<!>` even though this is a component
        // (`t1 = "<div><!> "`, `_setInsertionState(n4, n3)`). `x`/`n` share
        // one counter (`x1`). `_setInsertionState`+create before
        // `_renderEffect` (`flushPendingOperations`). Official 3rd-arg
        // default-slot form vs `{default:...}` is a disclosed divergence.
        "const t0 = _template(\"x\", 2)\nconst t1 = _template(\"<div><!>after  \", 1)\nfunction render(_ctx, $props, $emit, $attrs, $slots) {\n  const n1 = t1()\n  const n2 = _child(n1)\n  const x1 = _txt(n1)\n  _setInsertionState(n1, n2)\n  const _component_MyComp = _resolveComponent(\"MyComp\")\n  const n0 = _createComponentWithFallback(_component_MyComp, null, { default: () => {\n      const n0 = t0()\n      return n0\n    }, _: 2 })\n  _renderEffect(() => {\n    _setText(x1, \"after \" + _toDisplayString(_ctx.a))\n  })\n  return n1\n}",
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
    // Dynamic 4th span text ref consumes id 0 (dynamic-before-nav), so root is `n1`.
    let r = run_full_pipeline(
        "<template><div><span>0</span><span>1</span><span>2</span><span>{{ d }}</span></div></template>",
    );
    assert!(
        r.contains("_next(n1, 3)"),
        "dynamic 4th element child must be DOM index 3, got:\n{r}"
    );
    assert!(
        !r.contains("_next(n1, 2)") && !r.contains("_next(n1, 4)"),
        "DOM index off-by-one for the 4th child, got:\n{r}"
    );

    // span(0) comment(1) p(2): an enabled comment advances the index.
    let r =
        run_full_pipeline("<template><div><span>a</span><!-- c --><p>{{ b }}</p></div></template>");
    assert!(
        r.contains("_next(n1, 2)"),
        "an enabled comment must advance the DOM index to 2, got:\n{r}"
    );

    // Component after two siblings: numeric index 2 (rc.3 2-arg form).
    let r = run_full_pipeline("<template><div><a>x</a><b>y</b><c>{{ z }}</c></div></template>");
    assert!(
        r.contains("_setInsertionState(n1, 2)"),
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

// Root whose only dynamic content is its own interpolation must emit
// `const xN = _txt(nN)` + Txt/SetText — otherwise `_setText(x0, ...)` is a
// `ReferenceError`.
#[test]
fn root_element_own_interpolation_gets_txt_statement_and_imports() {
    let result = run_full_pipeline("<template><p>{{ msg }}</p></template>");
    assert!(
        result.contains("const x0 = _txt(n0)"),
        "root's own interpolation must get its own _txt() extraction statement, got:\n{result}"
    );
    assert!(
        result.contains("_setText(x0, _toDisplayString("),
        "got:\n{result}"
    );
    // `run_full_pipeline` returns template code only; Txt/SetText imports are
    // covered by the seed-matrix runtime axis and `finalize_root_element`.
}

#[test]
fn root_element_own_interpolation_plus_dynamic_prop_gets_txt_statement() {
    // Same defect with a sibling dynamic prop (`:disabled` + interpolation).
    let result =
        run_full_pipeline("<template><button :disabled=\"d\">{{ msg }}</button></template>");
    assert!(result.contains("const x0 = _txt(n0)"), "got:\n{result}");
    assert!(
        result.contains("_setProp(n0, \"disabled\""),
        "got:\n{result}"
    );
    assert!(
        result.contains("_setText(x0, _toDisplayString("),
        "got:\n{result}"
    );
}

// Nested v-if/v-for/<slot> topology
//
// Official splits one static region into multiple `_template(...)` calls: a
// skeleton with `<!>` anchors plus one template per dynamic branch/slot/item,
// wired via `_child`/`_next` and `_setInsertionState`. Gating this to
// `depth == 0` concatenated all branch HTML into one template (v-if means
// only one branch exists at runtime).

/// A `<slot>` inside a plain wrapper (`<header>`) with no own dynamic
/// content must forward `_createSlot` + `_setInsertionState` — `merge_into_parent`
/// only bubbles the element's own dynamic text/effects, so the slot was
/// dropped (`slots.vue`).
#[test]
fn nested_slot_forwards_through_plain_wrapper_element() {
    let result = run_full_pipeline(
        "<template><div class=\"panel\"><header><slot name=\"header\">Untitled</slot></header><main><slot /></main></div></template>",
    );
    // Fallback ("Untitled") gets its own hoisted template, like a v-if branch.
    assert!(
        result.contains(r#"_template("Untitled""#),
        "fallback content must hoist to its own template, got:\n{result}"
    );
    // Each wrapper's only content is its slot — 1-arg append, no `<!>`.
    assert!(
        !result.contains("<!>"),
        "neither slot needs an anchor (each is the sole content of its \
         wrapper) — got:\n{result}"
    );
    assert!(result.contains("_createSlot("), "got:\n{result}");
    assert!(result.contains("_setInsertionState("), "got:\n{result}");
    // Skeleton is whitespace-clean: `<div class=panel><header></header><main>`.
    assert!(
        result.contains(r#""<div class=panel><header></header><main>""#),
        "inter-tag whitespace must be stripped (WhitespaceNewline), got:\n{result}"
    );
    // Unnamed fallback-less default slot: `_createSlot()`.
    assert!(
        result.contains("_createSlot()"),
        "an unnamed slot with no fallback must omit all trailing default \
         args, got:\n{result}"
    );
}

/// Nested `v-if`/`v-else` (not depth 0) must hoist one template per branch,
/// insert `<!>` where a sibling follows, and wire `_createIf` via
/// `_setInsertionState` — not concatenate both branches into the ancestor
/// skeleton (that also leaves a stray undefined-node ref in effects).
#[test]
fn nested_v_if_else_splits_into_separate_anchored_templates() {
    let result = run_full_pipeline(
        "<template><div class=\"root\"><p v-if=\"a\">A</p><p v-else>zero</p><ul></ul></div></template><script setup>const a = 1;</script>",
    );
    assert!(
        result.contains(r#"_template("<p>A""#),
        "the v-if branch must hoist to its own template, got:\n{result}"
    );
    assert!(
        result.contains(r#"_template("<p>zero""#),
        "the v-else branch must hoist to its own template, got:\n{result}"
    );
    // Followed by `<ul>` → skeleton is `<!>` there, not either branch's HTML.
    assert!(
        result.contains(r#""<div class=root><!><ul>""#),
        "the ancestor's own skeleton must contain just the `<!>` anchor \
         (branches split into their own templates above), got:\n{result}"
    );
    assert!(result.contains("_createIf("), "got:\n{result}");
    assert!(result.contains("_setInsertionState("), "got:\n{result}");
}

/// Same nested `v-if`/`v-else` as last content of its parent — no `<!>`;
/// `_setInsertionState` is 1-arg append.
#[test]
fn nested_v_if_else_without_following_sibling_needs_no_anchor() {
    let result = run_full_pipeline(
        "<template><div class=\"root\"><p v-if=\"a\">A</p><p v-else>zero</p></div></template><script setup>const a = 1;</script>",
    );
    assert!(
        !result.contains("<!>"),
        "the v-if chain is the div's ONLY content — no anchor needed, got:\n{result}"
    );
    assert!(result.contains("_createIf("), "got:\n{result}");
}

/// Nested `v-for` with `:key` must not also `_setProp(..., "key", ...)` —
/// `extract_key_expr` already feeds `_createFor`'s key callback.
#[test]
fn nested_v_for_with_key_does_not_double_handle_key_as_a_prop() {
    let result = run_full_pipeline(
        "<template><div class=\"root\"><ul><li v-for=\"item in items\" :key=\"item\">{{ item }}</li></ul></div></template><script setup>const items = [];</script>",
    );
    assert!(
        !result.contains("_setProp(") || !result.contains("\"key\""),
        "`:key` must never be emitted as a _setProp call, got:\n{result}"
    );
    assert!(
        result.contains("(item) => (item)"),
        "the key must still reach _createFor's trailing key callback, got:\n{result}"
    );
    assert!(result.contains("_createFor("), "got:\n{result}");
}

/// Nested v-for source that is a member chain on an outer loop var
/// (`item.tags`) must rewrite like other in-body refs (`_for_item0.value.tags`).
/// `resolve_simple_expr` alone leaves dotted exprs unchanged — a runtime
/// `ReferenceError`. Inner FAST_REMOVE still fires: a v-if parent does not
/// itself disqualify `onlyChild` (both `_createFor`s end `9 /* FAST_REMOVE,
/// IS_SINGLE_NODE */`).
#[test]
fn nested_v_for_source_rewrites_outer_loop_variable_through_member_access() {
    let result = run_full_pipeline(
        "<template><div><li v-for=\"item in items\"><p v-if=\"item.show\">\
         <span v-for=\"tag in item.tags\">{{ tag }}</span></p></li></div></template>\
         <script setup>const items = [];</script>",
    );
    assert!(
        result.contains("_createFor(() => (_for_item0.value.tags)"),
        "the inner v-for's source must rewrite the outer loop variable through the \
         member-access chain exactly like official rc.3, got:\n{result}"
    );
    assert!(
        !result.contains("_createFor(() => (item.tags)"),
        "the outer loop variable `item` must never reach generated code unrenamed — a \
         raw `item.tags` reference is a runtime ReferenceError, got:\n{result}"
    );
    assert!(
        result.contains("_for_item0.value.show"),
        "the v-if condition's own outer-loop-variable member access must also rewrite \
         (already covered by the existing in-body mechanism), got:\n{result}"
    );
    // Bound the INNER `_createFor` args with a paren scan — a fixed-width
    // window can still contain the OUTER call's `9` flags.
    let inner_call = isolate_call(&result, "_createFor(() => (_for_item0.value.tags)");
    assert!(
        inner_call.contains("9 /* FAST_REMOVE, IS_SINGLE_NODE */"),
        "the inner v-for is still official's sole-child of the `<p v-if>` for \
         onlyChild/FAST_REMOVE purposes — a v-if parent alone must not disqualify it \
         (official emits flags 9, not 8), got the inner _createFor call's own text \
         (bounded to exactly this call, not the outer one):\n{inner_call}\n\nfull output:\n{result}"
    );
    assert!(
        !inner_call.contains("8 /* IS_SINGLE_NODE */"),
        "the inner call's own flags regressed to 8 (FAST_REMOVE lost), got:\n{inner_call}"
    );
}

/// Isolate one `_createFor(`/`_createIf(` call's args via balanced-paren
/// scan. A fixed-width window cannot bound one call among several nearby.
fn isolate_call<'a>(text: &'a str, needle: &str) -> &'a str {
    let start = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not found in:\n{text}"));
    let open_paren = start + needle.find('(').expect("needle names a call");
    let mut depth = 0i32;
    for (offset, byte) in text.as_bytes()[open_paren..].iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return &text[start..open_paren + offset + 1];
                }
            }
            _ => {}
        }
    }
    panic!("{needle:?}'s call has no matching close paren in:\n{text}");
}

/// Four-level nest (`v-for` > `v-for` > `v-if` > `v-for`) with the v-if
/// condition also referencing an active loop var — `flush_vif_chain`
/// placement must generalize. Official ids/nesting/flags match.
#[test]
fn quadruple_nested_v_for_v_if_v_for_matches_official_nesting_and_flags() {
    let result = run_full_pipeline(
        "<template><div><section v-for=\"group in groups\"><li v-for=\"item in group.items\">\
         <p v-if=\"item.show\"><span v-for=\"tag in item.tags\">{{ tag }}</span></p></li>\
         </section></div></template><script setup>const groups = [];</script>",
    );

    // Level 1: outer v-for over `groups`. Its own call text must contain
    // EVERY inner construct (proving the whole chain nests inside it) and
    // its own flags (bounded to itself, not a descendant's).
    let outer_for = isolate_call(&result, "_createFor(() => (_ctx.groups)");
    assert!(
        outer_for.contains("_createFor(() => (_for_item0.value.items)"),
        "level 2 must nest inside level 1's own callback, got:\n{outer_for}"
    );
    assert!(
        outer_for
            .trim_end()
            .ends_with("9 /* FAST_REMOVE, IS_SINGLE_NODE */)"),
        "level 1's own trailing flags must be its own, got:\n{outer_for}"
    );

    // Level 2: v-for over `group.items`, source rewritten through the
    // level-1 loop variable.
    let level2_for = isolate_call(&result, "_createFor(() => (_for_item0.value.items)");
    assert!(
        level2_for.contains("_createIf(() => (_for_item1.value.show)"),
        "level 3 (v-if) must nest inside level 2's own callback, got:\n{level2_for}"
    );
    assert!(
        level2_for
            .trim_end()
            .ends_with("9 /* FAST_REMOVE, IS_SINGLE_NODE */)"),
        "level 2's own trailing flags must be its own, got:\n{level2_for}"
    );

    // Level 3: v-if, condition rewritten through the level-2 loop variable.
    let level3_if = isolate_call(&result, "_createIf(() => (_for_item1.value.show)");
    assert!(
        level3_if.contains("_createFor(() => (_for_item1.value.tags)"),
        "level 4 (inner v-for) must nest inside the v-if's own branch body, got:\n{level3_if}"
    );

    // Level 4: innermost v-for, source rewritten through the level-2 loop
    // variable (the v-if branch's own scope carries no NEW loop variable).
    let level4_for = isolate_call(&result, "_createFor(() => (_for_item1.value.tags)");
    assert!(
        level4_for.contains("_for_item2) => {"),
        "level 4's own callback param must be the depth-2 rename, got:\n{level4_for}"
    );
    assert!(
        level4_for
            .trim_end()
            .ends_with("9 /* FAST_REMOVE, IS_SINGLE_NODE */)"),
        "level 4's own trailing flags must be its own, got:\n{level4_for}"
    );
}

// compute_if_flags
//
// Bit-for-bit / name-for-name against official's `genIfFlags`/
// `genIfFlagNames` (vendored rc.3 `@vue/compiler-vapor`), confirmed against
// the pinned rc.3 golden for basic-interpolation.vue's own v-if/v-else
// (flags 325 for a dynamic positive + static negative + index 0).

#[test]
fn compute_if_flags_bare_if_no_else_dynamic_positive_omits_argument() {
    // Official `flags === 1`: omit the 4th arg (bare v-if, not NO_SCOPE).
    let flags = compute_if_flags(false, IfNegative::None, Some(0), true, false);
    assert_eq!(flags, None);
}

#[test]
fn compute_if_flags_bare_if_no_else_static_positive_emits_no_scope_only() {
    // Static positive, no negative — TRUE_NO_SCOPE only (index unused).
    let flags = compute_if_flags(true, IfNegative::None, Some(0), true, false);
    assert_eq!(
        flags.as_deref(),
        Some("33 /* TRUE_SINGLE_ROOT, TRUE_NO_SCOPE */")
    );
}

#[test]
fn compute_if_flags_dynamic_positive_static_negative_matches_basic_interpolation_golden() {
    // rc.3 `basic-interpolation.vue` `<p v-if>{{ count }}</p><p v-else>zero</p>`.
    let flags = compute_if_flags(false, IfNegative::Terminal(true), Some(0), true, false);
    assert_eq!(
        flags.as_deref(),
        Some("325 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, FALSE_NO_SCOPE, KEYED_INDEX_0 */")
    );
}

#[test]
fn compute_if_flags_both_branches_dynamic_no_no_scope_bits() {
    let flags = compute_if_flags(false, IfNegative::Terminal(false), Some(2), true, false);
    assert_eq!(
        flags.as_deref(),
        Some("773 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, KEYED_INDEX_2 */")
    );
}

#[test]
fn compute_if_flags_else_if_chain_never_gets_false_no_scope() {
    // `v-else-if` is never NO_SCOPE (`negative.type !== 14`).
    let flags = compute_if_flags(true, IfNegative::Chain, Some(1), true, false);
    assert_eq!(
        flags.as_deref(),
        Some("549 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, TRUE_NO_SCOPE, KEYED_INDEX_1 */")
    );
}

#[test]
fn compute_if_flags_nested_v_if_never_gets_no_scope_bits() {
    // Nested v-if (`allowNoScope` false) suppresses both NO_SCOPE bits.
    let flags = compute_if_flags(true, IfNegative::Terminal(true), Some(0), false, false);
    assert_eq!(
        flags.as_deref(),
        Some("261 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, KEYED_INDEX_0 */")
    );
}

#[test]
fn compute_if_flags_production_mode_omits_comment() {
    let flags = compute_if_flags(false, IfNegative::Terminal(true), Some(0), true, true);
    assert_eq!(flags.as_deref(), Some("325"));
}

/// Official `genMulti`: present flags + absent negative → explicit `null`
/// for the skipped 3rd arg (falsy followed by truthy is a placeholder, not dropped).
#[test]
fn bare_static_v_if_with_no_scope_emits_explicit_null_negative_placeholder() {
    let result = run_full_pipeline("<template><div v-if=\"a\">static</div></template>");
    assert!(
        result.contains(", null, "),
        "a bare NO_SCOPE-eligible v-if must emit an explicit `null` for the \
         skipped negative argument before the flags, got:\n{result}"
    );
}

// Dynamic-id allocation order
//
// Official rc.3: every v-if/v-for construct-own id is allocated before its
// branch/item content id. These tests pin that ordering, not exact numbers.

/// Extract the integer id from a `const nN = ` (or `xN`) declaration
/// appearing at or after `from`, returning `(id, position_after_match)`.
fn first_id_after(haystack: &str, from: usize, prefix: &str) -> (u32, usize) {
    let rest = &haystack[from..];
    let marker_pos = rest
        .find(prefix)
        .unwrap_or_else(|| panic!("expected to find `{prefix}` in:\n{haystack}"));
    let digits_start = marker_pos + prefix.len();
    let digits_end = rest[digits_start..]
        .find(|c: char| !c.is_ascii_digit())
        .map(|i| digits_start + i)
        .unwrap_or(rest.len());
    let id: u32 = rest[digits_start..digits_end]
        .parse()
        .unwrap_or_else(|_| panic!("expected digits after `{prefix}` in:\n{haystack}"));
    (id, from + digits_end)
}

#[test]
fn dynamic_id_ordering_if_construct_own_id_precedes_branch_content_ids() {
    let r = run_full_pipeline(
        "<template><div v-if=\"a\">A {{ x }}</div><div v-else>B</div></template>",
    );
    let (construct_id, pos) = first_id_after(&r, 0, "const n");
    let (true_content_id, pos) = first_id_after(&r, pos, "const n");
    let (false_content_id, _) = first_id_after(&r, pos, "const n");
    assert!(
        construct_id < true_content_id,
        "if-construct's own id ({construct_id}) must precede the true branch's \
         content id ({true_content_id}), got:\n{r}"
    );
    assert!(
        true_content_id < false_content_id,
        "true-branch content id ({true_content_id}) must precede the false \
         branch's content id ({false_content_id}), got:\n{r}"
    );
}

#[test]
fn dynamic_id_ordering_for_construct_own_id_precedes_item_content_id() {
    let r = run_full_pipeline("<template><li v-for=\"item in items\">{{ item }}</li></template>");
    let (construct_id, pos) = first_id_after(&r, 0, "const n");
    let (item_content_id, _) = first_id_after(&r, pos, "const n");
    assert!(
        construct_id < item_content_id,
        "for-construct's own id ({construct_id}) must precede the item's own \
         content id ({item_content_id}), got:\n{r}"
    );
}

/// Construct-own id of `const nN = {call_marker}`. Walks backwards from the
/// call so an intervening `const nN` (e.g. a v-for item template) cannot
/// steal the match.
fn construct_own_id_from(haystack: &str, from: usize, call_marker: &str) -> (u32, usize) {
    let call_pos = from
        + haystack[from..]
            .find(call_marker)
            .unwrap_or_else(|| panic!("expected to find `{call_marker}` in:\n{haystack}"));
    let before = &haystack[..call_pos];
    let n_pos = before
        .rfind("const n")
        .unwrap_or_else(|| panic!("expected `const n` before `{call_marker}` in:\n{haystack}"));
    let digits_start = n_pos + "const n".len();
    let digits_end = before[digits_start..]
        .find(|c: char| !c.is_ascii_digit())
        .map(|i| digits_start + i)
        .unwrap_or(before.len());
    let id = before[digits_start..digits_end]
        .parse()
        .unwrap_or_else(|_| panic!("expected digits in:\n{haystack}"));
    (id, call_pos)
}

/// Nested: v-if inside a v-for item. Outer for-id precedes nested if-id,
/// which precedes its branch content (official allocates construct-own
/// before descending). Does not assert the item `<li>` id sits between —
/// that id is allocated late (deferred wrapper-ref), unlike construct-own.
#[test]
fn dynamic_id_ordering_nested_v_if_inside_v_for() {
    let r = run_full_pipeline(
        "<template><li v-for=\"item in items\"><span v-if=\"item.ok\">{{ item.label }}</span><span v-else>skip</span></li></template>",
    );
    let (for_id, _) = construct_own_id_from(&r, 0, "_createFor(");
    let (if_id, if_call_pos) = construct_own_id_from(&r, 0, "_createIf(");
    assert!(
        for_id < if_id,
        "outer for-construct's own id ({for_id}) must precede the nested \
         if-construct's own id ({if_id}), got:\n{r}"
    );

    // The nested if's own id must precede ITS branch content, searched
    // starting right after the if's own call.
    let (true_id, pos) = first_id_after(&r, if_call_pos, "const n");
    let (false_id, _) = first_id_after(&r, pos, "const n");
    assert!(
        if_id < true_id && true_id < false_id,
        "nested if-construct's own id ({if_id}) must precede its branch \
         content ids ({true_id}, {false_id}) in order, got:\n{r}"
    );
}

/// Sibling independent v-ifs: each construct's own-id-then-content order
/// holds independently. Ranges may interleave — root nav-ref is allocated
/// between them (disclosed residual).
#[test]
fn dynamic_id_ordering_sibling_if_constructs_do_not_collide() {
    let r = run_full_pipeline(
        "<template><div><p v-if=\"a\">{{ x }}</p><p v-else>zero</p><span v-if=\"b\">{{ y }}</span><span v-else>zero2</span></div></template>",
    );
    let (first_if, first_call_pos) = construct_own_id_from(&r, 0, "_createIf(");
    let (first_true, pos) = first_id_after(&r, first_call_pos, "const n");
    let (first_false, pos) = first_id_after(&r, pos, "const n");
    assert!(
        first_if < first_true && first_true < first_false,
        "first if-construct's own id ({first_if}) must precede its branch \
         content ({first_true}, {first_false}) in order, got:\n{r}"
    );

    let (second_if, second_call_pos) = construct_own_id_from(&r, pos, "_createIf(");
    let (second_true, pos2) = first_id_after(&r, second_call_pos, "const n");
    let (second_false, _) = first_id_after(&r, pos2, "const n");
    assert!(
        second_if < second_true && second_true < second_false,
        "second if-construct's own id ({second_if}) must precede its branch \
         content ({second_true}, {second_false}) in order, got:\n{r}"
    );
    assert!(
        first_if != second_if && first_true != second_true && first_false != second_false,
        "sibling if-constructs must never reuse an id, got first=({first_if}, \
         {first_true}, {first_false}) second=({second_if}, {second_true}, \
         {second_false}) from:\n{r}"
    );
}

// v-on handler wrapping
//
// Official `genEventHandler` (rc.3): wrap `e => handler(e)` only when not a
// constant binding. `isConstantBinding` is `bindingMetadata[…] === SETUP_CONST`
// AND `value.ast === null` (bare identifier). Dotted `foo.bar` always has an
// ast → always wrapped. `function onClick() {}` is SETUP_CONST → bare
// `_ctx.onClick` (`props-emit.vue`).

#[test]
fn v_on_bare_setup_const_function_reference_is_not_wrapped() {
    let r = run_full_pipeline(
        "<script setup>\nfunction onClick() {}\n</script>\n<template><button @click=\"onClick\">x</button></template>",
    );
    assert!(
        r.contains("_on(n0, \"click\", _ctx.onClick)"),
        "a bare SETUP_CONST (function declaration) handler reference must \
         be emitted BARE, never wrapped in `e => ...(e)`, got:\n{r}"
    );
    assert!(
        !r.contains("e => _ctx.onClick(e)"),
        "got an unexpected arrow-wrapped handler:\n{r}"
    );
}

/// Reassignable binding (e.g. `ref`) still needs the arrow wrap.
#[test]
fn v_on_bare_non_const_reference_is_still_wrapped() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst onClick = ref(() => {})\n</script>\n<template><button @click=\"onClick\">x</button></template>",
    );
    assert!(
        r.contains("e => _ctx.onClick.value(e)") || r.contains("e => _ctx.onClick(e)"),
        "a non-const (ref-backed) handler reference must still be \
         arrow-wrapped, got:\n{r}"
    );
}

/// Dotted `foo.bar` is always wrapped (`isConstantBinding` is bare-ident only).
#[test]
fn v_on_dotted_member_expression_is_always_wrapped() {
    let r = run_full_pipeline(
        "<script setup>\nfunction useHandlers() { return { onClick() {} } }\nconst handlers = useHandlers()\n</script>\n<template><button @click=\"handlers.onClick\">x</button></template>",
    );
    assert!(
        r.contains("e => _ctx.handlers.onClick(e)"),
        "a dotted member-expression handler must always be arrow-wrapped \
         regardless of the root binding's own type, got:\n{r}"
    );
}

// Operation vs effect emission order
//
// Official `flushPendingOperations` (rc.3): operations (`_on()`, etc.) always
// flush before the aggregated `_renderEffect`, regardless of source order
// (`props-emit.vue`: `:disabled` source-first, `_renderEffect` after `_on`).

#[test]
fn root_element_statements_are_emitted_before_effects() {
    let r = run_full_pipeline(
        "<template><button :disabled=\"disabled\" @click=\"onClick\">{{ label }}</button></template>",
    );
    let on_pos = r
        .find("_on(")
        .expect("expected an _on(...) call in the output");
    let effect_pos = r
        .find("_renderEffect(")
        .expect("expected a _renderEffect(...) call in the output");
    assert!(
        on_pos < effect_pos,
        "the _on(...) statement must be emitted BEFORE the _renderEffect(...) \
         block, regardless of :disabled being written before @click in \
         source, got:\n{r}"
    );
}

// Deferred parent-ref id allocation
//
// Official `processDynamicChildren` (rc.3): a scope's own node ref is a
// memoized `context.reference()` after ALL direct children, never eagerly.
// Root `<div>` with if/else then `<ul>`+v-for: root gets the highest id.

#[test]
fn root_own_ref_deferred_past_later_sibling_subtree() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst count = ref(0)\nconst items = ['a', 'b', 'c']\n</script>\n<template>\n  <div class=\"root\">\n    <p v-if=\"count > 0\">{{ count }}</p>\n    <p v-else>zero</p>\n    <ul>\n      <li v-for=\"item in items\" :key=\"item\">{{ item }}</li>\n    </ul>\n  </div>\n</template>",
    );
    assert!(
        r.contains("const n10 = t3()"),
        "root's own ref must be the HIGHEST id (10), allocated only after \
         the trailing <ul>/v-for subtree consumed ids 5-8, got:\n{r}"
    );
    assert!(
        r.contains("const n9 = _child(n10)"),
        "the if-block's anchor (9) must be minted BEFORE root's own ref \
         (10) within the same establishment — anchor first, container \
         second, matching official's real allocation order, got:\n{r}"
    );
    assert!(
        r.contains("const n8 = _next(n9)"),
        "<ul>'s own established ref (8) must chain from the anchor (9), \
         got:\n{r}"
    );
    assert!(
        r.contains("const n5 = _createFor("),
        "the v-for construct's own id (5) must come right after the if-\
         chain's ids (0-4) are fully consumed, unaffected by root's own \
         ref timing, got:\n{r}"
    );
    assert!(
        r.contains("const n0 = _createIf("),
        "the if construct's own id must stay 0 (first allocated), got:\n{r}"
    );
    // Negative: eager mint would give root id 5, colliding with the for-construct.
    assert!(
        !r.contains("const n5 = t3()"),
        "root's own ref must not be prematurely minted as id 5, got:\n{r}"
    );
}

#[test]
fn wrapping_elements_own_refs_deferred_past_later_sibling() {
    let r = run_full_pipeline(
        "<template>\n  <div class=\"panel\">\n    <header>\n      <slot name=\"header\">Untitled</slot>\n    </header>\n    <main>\n      <slot />\n    </main>\n  </div>\n</template>",
    );
    assert!(
        r.contains("const n6 = t1()"),
        "root's own ref must be the HIGHEST id (6), allocated only after \
         BOTH <header>'s and <main>'s own subtrees are fully consumed, \
         got:\n{r}"
    );
    assert!(
        r.contains("const n3 = _child(n6)"),
        "<header>'s own established ref (3) must chain from root (6), \
         got:\n{r}"
    );
    assert!(
        r.contains("const n5 = _next(n3)"),
        "<main>'s own established ref (5) must chain from <header> (3), \
         not be minted before it, got:\n{r}"
    );
    assert!(
        r.contains("const n0 = _createSlot(\"header\""),
        "the header slot's own construct id must stay 0 (first \
         allocated), got:\n{r}"
    );
    assert!(
        r.contains("const n4 = _createSlot()"),
        "the default slot's own construct id must be 4, right after the \
         header slot's own subtree (0-3) is fully consumed, got:\n{r}"
    );
}

// Block-depth vs AST-depth for `allow_no_scope`
//
// Official `allowNoScope = context.block === context.root.block`: a v-if
// inside any number of plain wrappers (no intervening block) stays
// NO_SCOPE-eligible. `depth == 0` was the wrong proxy (`basic-interpolation.vue`:
// `<p v-if>` one level inside the root `<div>`).

#[test]
fn v_if_nested_in_plain_wrapper_still_gets_false_no_scope() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst count = ref(0)\n</script>\n<template><div class=\"root\"><p v-if=\"count > 0\">{{ count }}</p><p v-else>zero</p></div></template>",
    );
    assert!(
        r.contains("FALSE_NO_SCOPE"),
        "a v-if nested inside a plain wrapping element (not another \
         block-creating construct) must still be NO_SCOPE-eligible, got:\n{r}"
    );
}

/// Not merely "not the document root": v-if inside a v-for item is denied
/// NO_SCOPE (`compute_if_flags_nested_v_if_never_gets_no_scope_bits`).
#[test]
fn v_if_nested_in_v_for_still_denied_no_scope() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst items = ref([1])\n</script>\n<template><ul><li v-for=\"item in items\" :key=\"item\"><span v-if=\"item\">a</span><span v-else>b</span></li></ul></template>",
    );
    assert!(
        !r.contains("NO_SCOPE"),
        "a v-if nested inside a v-for's own item body must never be \
         NO_SCOPE-eligible (neither TRUE_ nor FALSE_), got:\n{r}"
    );
}

// v-for flags
//
// Official `genForFlags` (rc.3): FAST_REMOVE (1) when the v-for is the sole
// meaningful child of a plain parent; IS_SINGLE_NODE (8) when the item body
// has a real template (always true here via `build_closure_body`).

#[test]
fn v_for_sole_child_of_plain_wrapper_gets_fast_remove_and_single_node() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst items = ref(['a'])\n</script>\n<template><ul><li v-for=\"item in items\" :key=\"item\">{{ item }}</li></ul></template>",
    );
    assert!(
        r.contains("9 /* FAST_REMOVE, IS_SINGLE_NODE */"),
        "sole v-for child of a plain wrapper must get FAST_REMOVE + \
         IS_SINGLE_NODE (9), got:\n{r}"
    );
}

/// A v-for with a MEANINGFUL sibling in the same parent is not the sole
/// child — FAST_REMOVE must not fire, but IS_SINGLE_NODE still does.
#[test]
fn v_for_with_sibling_omits_fast_remove() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst items = ref(['a'])\n</script>\n<template><ul><li v-for=\"item in items\" :key=\"item\">{{ item }}</li><li>static</li></ul></template>",
    );
    assert!(
        r.contains("8 /* IS_SINGLE_NODE */"),
        "a v-for with a sibling must get IS_SINGLE_NODE only (8), no \
         FAST_REMOVE, got:\n{r}"
    );
    assert!(
        !r.contains("FAST_REMOVE"),
        "FAST_REMOVE must not fire when the v-for has a sibling, got:\n{r}"
    );
}

// v-for loop-variable renaming
//
// Official (`@vue/compiler-vapor`'s real `genFor`/`processFor`, confirmed
// directly against the vendored rc.3 source): a v-for's loop variable is
// renamed to `_for_item{depth}` in the MAIN closure's own param list, and
// every in-body reference to it is rewritten to `_for_item{depth}.value`
// (v-for items are reactive proxies needing `.value` unwrap) —
// `context.withId(fn, idMap)` scoping. `depth` is a genuine push/pop
// nesting-depth counter (`context.scopeLevel`), not a running total:
// sibling (non-nested) v-for loops both get depth 0. The `:key="..."`
// callback's OWN param list stays the RAW, unrenamed name — official's
// `genCallback`/`genSimpleIdMap` never renames there.

#[test]
fn v_for_item_renamed_and_unwrapped_in_body() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst items = ref(['a', 'b', 'c'])\n</script>\n<template><ul><li v-for=\"item in items\" :key=\"item\">{{ item }}</li></ul></template>",
    );
    assert!(
        r.contains("(_for_item0) => {"),
        "the main closure's own param must be renamed to _for_item0, got:\n{r}"
    );
    assert!(
        r.contains("_toDisplayString(_for_item0.value)"),
        "every in-body reference to the loop variable must be rewritten to \
         _for_item0.value, got:\n{r}"
    );
    assert!(
        !r.contains("_toDisplayString(item)"),
        "the raw loop-variable name must not survive inside the body, got:\n{r}"
    );
}

/// The `:key` callback keeps the RAW, unrenamed loop-variable name —
/// confirmed against the pinned rc.3 golden (`(item) => (item)`, never
/// `(_for_item0) => (_for_item0.value)`).
#[test]
fn v_for_key_callback_stays_unrenamed() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst items = ref(['a', 'b', 'c'])\n</script>\n<template><ul><li v-for=\"item in items\" :key=\"item\">{{ item }}</li></ul></template>",
    );
    assert!(
        r.contains("(item) => (item)"),
        "the :key callback must keep the raw, unrenamed loop-variable name, got:\n{r}"
    );
}

/// Two SIBLING (non-nested) v-for loops both get depth 0 — a genuine
/// push/pop nesting-depth counter, not a running total across the whole
/// template.
#[test]
fn v_for_sibling_loops_both_get_depth_zero() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst as_ = ref(['a'])\nconst bs = ref(['b'])\n</script>\n<template><div><ul><li v-for=\"a in as_\">{{ a }}</li></ul><ol><li v-for=\"b in bs\">{{ b }}</li></ol></div></template>",
    );
    assert!(
        r.contains("(_for_item0) => {") && r.matches("(_for_item0) => {").count() == 2,
        "sibling v-for loops must BOTH get depth 0 (the counter returns to \
         0 between them, not a running total), got:\n{r}"
    );
    assert!(
        !r.contains("_for_item1"),
        "no sibling v-for should ever reach depth 1, got:\n{r}"
    );
}
