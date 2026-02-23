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
        let oxc_ast = parse_template_expressions(ast, source, &alloc, SourceType::tsx());
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

// ==================== Phase 1: HTML minimization ====================

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

// ==================== Phase 2: Events ====================

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

// ==================== Phase 3: v-show, v-model ====================

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

// ==================== Phase 4: v-html ====================

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

// ==================== Phase 5: Components ====================

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

// ==================== Phase 6: Slot outlets ====================

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

// ==================== Phase 7: Structural directives ====================

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

// ==================== Phase 7.5: Template ref ====================

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

// ==================== Phase 8: v-once / v-memo ====================

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
        }),
        v_condition: None,
        v_for: None,
        v_slot: None,
        v_once: None,
        v_ref: None,
        prop_flag: PropFlag::empty(),
        children_flag: ChildrenFlag::empty(),
        children_mode: ChildrenMode::TextOnlyStatic,
    };

    let text = TextNode {
        start: 15,
        end: 20,
        is_entity: false,
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
        }),
        v_condition: None,
        v_for: None,
        v_slot: None,
        v_once: None,
        v_ref: None,
        prop_flag: PropFlag::empty(),
        children_flag: ChildrenFlag::empty(),
        children_mode: ChildrenMode::Empty,
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
        }),
        v_condition: None,
        v_for: None,
        v_slot: None,
        v_once: None,
        v_ref: None,
        prop_flag: PropFlag::empty(),
        children_flag: ChildrenFlag::empty().add(ChildrenFlags::HasInterpolation),
        children_mode: ChildrenMode::TextOnlyDynamic,
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
