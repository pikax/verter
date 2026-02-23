use super::*;
use crate::ast::types::*;
use crate::parser::types::RootNodeTemplateContent;
use crate::template::oxc::types::Dynamism;
use crate::types::NodeTag;
use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

// ==================== Full pipeline integration tests ====================

/// Run a .vue source through the full new_impl pipeline and return the
/// generated output for a given CodeGenMode.
fn run_full_pipeline(source: &str, mode: super::super::CodeGenMode) -> String {
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

#[test]
fn full_pipeline_static_element() {
    let source = "<template><div>hello</div></template>";
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_template("),
        "Expected _template, got: {result}"
    );
    assert!(
        result.contains("function render("),
        "Expected render function, got: {result}"
    );
    // Template instantiation should come before return
    let inst_pos = result.find("const n").expect("Expected const nX");
    let ret_pos = result.find("return n").expect("Expected return nX");
    assert!(
        inst_pos < ret_pos,
        "Template instantiation must come before return: {result}"
    );
}

#[test]
fn full_pipeline_interpolation_ordering() {
    // Verify that template instantiation comes before child navigation
    let source = "<template><div><span>{{ msg }}</span></div></template>";
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);

    // The root element const nX = tX() must appear before any _child/_next navigation
    let lines: Vec<&str> = result.lines().collect();
    let mut found_instantiation = false;
    let mut found_nav_before_instantiation = false;

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("const n") && trimmed.contains(" = t") && trimmed.ends_with("()") {
            found_instantiation = true;
        }
        if !found_instantiation && (trimmed.contains("_child(") || trimmed.contains("_next(")) {
            found_nav_before_instantiation = true;
        }
    }

    assert!(
        !found_nav_before_instantiation,
        "Navigation must not appear before template instantiation.\nOutput:\n{result}"
    );
    assert!(
        found_instantiation,
        "Must have template instantiation.\nOutput:\n{result}"
    );
}

#[test]
fn full_pipeline_dynamic_text() {
    let source = "<template><p>{{ count }}</p></template>";
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);

    assert!(
        result.contains("_renderEffect"),
        "Expected _renderEffect for dynamic text, got: {result}"
    );
    assert!(
        result.contains("_setText"),
        "Expected _setText, got: {result}"
    );
    assert!(
        result.contains("_toDisplayString"),
        "Expected _toDisplayString, got: {result}"
    );
    assert!(result.contains("_txt("), "Expected _txt(), got: {result}");
}

#[test]
fn full_pipeline_mixed_text_and_interpolation() {
    let source = "<template><p>Count: {{ count }}</p></template>";
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);

    assert!(
        result.contains("_setText"),
        "Expected _setText for mixed text, got: {result}"
    );
    // Should have both static text and dynamic part
    assert!(
        result.contains("\"Count: \""),
        "Expected static 'Count: ' part, got: {result}"
    );
    assert!(
        result.contains("_toDisplayString"),
        "Expected _toDisplayString, got: {result}"
    );
}

#[test]
fn full_pipeline_vapor2_matches_vapor_structure() {
    // Both Vapor and Vapor2 should produce structurally equivalent output
    let source = "<template><div><p>{{ msg }}</p></div></template>";
    let v1 = run_full_pipeline(source, super::super::CodeGenMode::Vapor);
    let v2 = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);

    // Both should have the same key structural elements
    for feature in &[
        "_template(",
        "function render(",
        "_txt(",
        "_setText(",
        "_renderEffect(",
        "_toDisplayString(",
    ] {
        let v1_count = v1.matches(feature).count();
        let v2_count = v2.matches(feature).count();
        assert_eq!(
            v1_count, v2_count,
            "Feature '{}' count mismatch: vapor={}, vapor2={}\nVapor:\n{}\nVapor2:\n{}",
            feature, v1_count, v2_count, v1, v2
        );
    }
}

#[test]
fn full_pipeline_nested_elements_ordering() {
    // Complex case: parent with multiple dynamic children
    let source =
        r#"<template><div class="app"><h1>{{ title }}</h1><p>{{ body }}</p></div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);

    // Template instantiation must come first
    let template_inst = result.find("= t").expect("Expected template instantiation");
    let first_nav = result.find("_child(").or(result.find("_next("));
    if let Some(nav_pos) = first_nav {
        assert!(
            template_inst < nav_pos,
            "Template instantiation (pos {}) must come before navigation (pos {})\nOutput:\n{}",
            template_inst,
            nav_pos,
            result
        );
    }

    // Should have 2 setText calls (one for title, one for body)
    let set_text_count = result.matches("_setText(").count();
    assert_eq!(
        set_text_count, 2,
        "Expected 2 _setText calls, got {}\nOutput:\n{}",
        set_text_count, result
    );
}

#[test]
fn full_pipeline_event_click() {
    // @click should produce event delegation, not setProp
    let source = r#"<template><button @click="handleClick">Go</button></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);

    // Should NOT produce _setProp for click
    assert!(
        !result.contains("_setProp"),
        "Click event should not produce _setProp.\nOutput:\n{result}"
    );

    // Event handling: either delegation or _on with the correct event name
    let has_delegation = result.contains("$evtclick") || result.contains("_delegateEvents");
    let has_on_click = result.contains("_on(") && result.contains("\"click\"");
    assert!(
        has_delegation || has_on_click,
        "Expected event delegation or _on with 'click' event name.\nOutput:\n{result}"
    );

    // Should not have empty event name
    assert!(
        !result.contains(", \"\","),
        "Event name must not be empty.\nOutput:\n{result}"
    );
}

#[test]
fn full_pipeline_event_click_with_handler_value() {
    // Verify the handler expression is correctly extracted
    let source = r#"<template><button @click="count++">Inc</button></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);

    // The handler should reference count (possibly prefixed)
    assert!(
        result.contains("count"),
        "Expected handler to reference count.\nOutput:\n{result}"
    );
}

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

// ==================== empty template ====================

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
    let mut gen = Vapor2CodeGen::new(&ast, resolver, "", &options);

    gen.enter_template(&root, source, &mut out);
    gen.leave_template(&root, source, &mut out);

    let result = apply_output(source, out, &alloc);
    assert!(
        result.contains("return null"),
        "Expected 'return null', got: {result}"
    );
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
    let mut gen = Vapor2CodeGen::new(&ast, resolver, source, &options);

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

    gen.enter_template(&root, source, &mut out);
    gen.enter_element(NodeId(0), &element, None, source, &mut out);
    gen.visit_text(NodeId(1), &text, source, &mut out);
    gen.leave_element(NodeId(0), &element, None, source, &mut out);
    gen.leave_template(&root, source, &mut out);

    let result = apply_output(source, out, &alloc);
    assert!(
        result.contains("_template(\"<div>hello</div>\""),
        "Expected template, got: {result}"
    );
    assert!(result.contains("function render("));
    assert!(result.contains("const n0 = t0()"));
    assert!(result.contains("return n0"));
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
    let mut gen = Vapor2CodeGen::new(&ast, resolver, source, &options);

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

// ==================== element with interpolation ====================

#[test]
fn element_with_interpolation() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    let options = make_options();
    let resolver = make_resolver(&alloc);
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
    let mut gen = Vapor2CodeGen::new(&ast, resolver, source, &options);

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

    assert!(
        result.contains("_template(\"<div> </div>\""),
        "Expected template with space placeholder, got: {result}"
    );
    assert!(
        result.contains("_renderEffect"),
        "Expected renderEffect, got: {result}"
    );
    assert!(
        result.contains("_setText"),
        "Expected _setText, got: {result}"
    );
    assert!(
        result.contains("_toDisplayString"),
        "Expected _toDisplayString, got: {result}"
    );
    assert!(
        result.contains("_ctx.msg"),
        "Expected _ctx.msg, got: {result}"
    );
}

// ==================== v-if structural directives ====================

#[test]
fn full_pipeline_v_if_simple() {
    let source = r#"<template><div v-if="show">hello</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createIf"),
        "Expected _createIf, got:\n{result}"
    );
    assert!(
        result.contains("() =>"),
        "Expected closure arrow, got:\n{result}"
    );
    assert!(
        result.contains("_template("),
        "Expected template decl, got:\n{result}"
    );
    // Return inside closure body
    assert!(
        result.contains("return n"),
        "Expected return statement, got:\n{result}"
    );
}

#[test]
fn full_pipeline_v_if_else() {
    let source = r#"<template><div v-if="a">A</div><div v-else>B</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    // Should have one _createIf with two branches
    assert!(
        result.contains("_createIf"),
        "Expected _createIf, got:\n{result}"
    );
    // Two template declarations (one for each branch)
    let template_count = result.matches("_template(").count();
    assert_eq!(
        template_count, 2,
        "Expected 2 template decls, got {template_count}:\n{result}"
    );
    // The else branch closure
    assert!(
        result.contains(", () => {"),
        "Expected else branch closure, got:\n{result}"
    );
}

#[test]
fn full_pipeline_v_if_elseif_else() {
    let source = r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    // Should have nested _createIf
    let create_if_count = result.matches("_createIf").count();
    assert_eq!(
        create_if_count, 2,
        "Expected 2 _createIf calls (outer + nested), got {create_if_count}:\n{result}"
    );
    // Three template declarations
    let template_count = result.matches("_template(").count();
    assert_eq!(
        template_count, 3,
        "Expected 3 template decls, got {template_count}:\n{result}"
    );
}

#[test]
fn full_pipeline_v_if_with_dynamic_text() {
    let source = r#"<template><div v-if="show">{{ msg }}</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createIf"),
        "Expected _createIf, got:\n{result}"
    );
    assert!(
        result.contains("_renderEffect"),
        "Expected _renderEffect inside v-if branch, got:\n{result}"
    );
    assert!(
        result.contains("_setText"),
        "Expected _setText, got:\n{result}"
    );
}

#[test]
fn full_pipeline_v_if_root_level() {
    // v-if as direct child of <template> should still produce a valid return
    let source = r#"<template><div v-if="show">hello</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    // The root return should reference the _createIf result
    let lines: Vec<&str> = result.lines().collect();
    let last_return = lines
        .iter()
        .rev()
        .find(|l| l.trim().starts_with("return "))
        .expect("No return statement found");
    assert!(
        last_return.contains("return n"),
        "Root return should reference the _createIf variable, got: {last_return}\nFull:\n{result}"
    );
}

// ==================== v-for structural directives ====================

#[test]
fn full_pipeline_v_for_simple() {
    let source = r#"<template><div v-for="item in items">{{ item }}</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createFor"),
        "Expected _createFor, got:\n{result}"
    );
    assert!(
        result.contains("items"),
        "Expected iterable 'items', got:\n{result}"
    );
    assert!(
        result.contains("(item)"),
        "Expected params '(item)', got:\n{result}"
    );
    assert!(
        result.contains("_template("),
        "Expected template decl, got:\n{result}"
    );
}

#[test]
fn full_pipeline_v_for_with_index() {
    let source = r#"<template><div v-for="(item, i) in items">{{ item }}</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createFor"),
        "Expected _createFor, got:\n{result}"
    );
    // Params should include index
    assert!(
        result.contains("(item, i)"),
        "Expected params '(item, i)', got:\n{result}"
    );
}

#[test]
fn full_pipeline_v_for_with_key() {
    let source =
        r#"<template><div v-for="item in items" :key="item.id">{{ item.name }}</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createFor"),
        "Expected _createFor, got:\n{result}"
    );
    // Should have a key function argument
    assert!(
        result.contains("item.id"),
        "Expected key expression 'item.id', got:\n{result}"
    );
}

// ==================== template v-if / v-for (fragment wrappers) ====================

#[test]
fn full_pipeline_template_v_if() {
    // <template v-if> should produce a fragment with multiple children
    let source =
        r#"<template><template v-if="show"><span>A</span><span>B</span></template></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createIf"),
        "Expected _createIf, got:\n{result}"
    );
    // Should have template declarations for the children
    assert!(
        result.contains("_template("),
        "Expected template decl, got:\n{result}"
    );
}

#[test]
fn full_pipeline_template_v_for() {
    let source = r#"<template><template v-for="item in items"><dt>{{ item.term }}</dt><dd>{{ item.def }}</dd></template></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createFor"),
        "Expected _createFor, got:\n{result}"
    );
}

// ==================== nested structural combinations ====================

#[test]
fn full_pipeline_v_if_inside_v_for() {
    let source = r#"<template><div v-for="item in items"><span v-if="item.show">{{ item.name }}</span></div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createFor"),
        "Expected _createFor, got:\n{result}"
    );
    assert!(
        result.contains("_createIf"),
        "Expected _createIf inside v-for, got:\n{result}"
    );
}

#[test]
fn full_pipeline_v_for_inside_v_if() {
    let source = r#"<template><div v-if="hasItems"><span v-for="item in items">{{ item }}</span></div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createIf"),
        "Expected _createIf, got:\n{result}"
    );
    assert!(
        result.contains("_createFor"),
        "Expected _createFor inside v-if, got:\n{result}"
    );
}

// ==================== component slots ====================

#[test]
fn full_pipeline_component_default_slot() {
    // Component with implicit default slot content should produce
    // a slots object with a `default` closure containing the slot body.
    let source = r#"<template><MyComp><div>content</div></MyComp></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createComponent"),
        "Expected _createComponent, got:\n{result}"
    );
    // The slot body should be a closure in the slots object
    assert!(
        result.contains("default: () =>"),
        "Expected default slot closure, got:\n{result}"
    );
    // The slot body should contain a template for the child element
    assert!(
        result.contains("_template("),
        "Expected _template in slot body, got:\n{result}"
    );
}

#[test]
fn full_pipeline_component_named_slots() {
    // Named slots via v-slot should produce a slots object with named closures.
    let source = r#"<template><MyComp><template v-slot:header><h1>H</h1></template><template v-slot:default><p>D</p></template></MyComp></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createComponent"),
        "Expected _createComponent, got:\n{result}"
    );
    // Should have named slot closures
    assert!(
        result.contains("header: () =>"),
        "Expected header slot closure, got:\n{result}"
    );
    assert!(
        result.contains("default: () =>"),
        "Expected default slot closure, got:\n{result}"
    );
}

#[test]
fn full_pipeline_component_slot_with_dynamic_content() {
    // Slot body with interpolation should have renderEffect inside the slot closure
    let source = r#"<template><MyComp><div>{{ msg }}</div></MyComp></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createComponent"),
        "Expected _createComponent, got:\n{result}"
    );
    assert!(
        result.contains("default: () =>"),
        "Expected default slot closure, got:\n{result}"
    );
    assert!(
        result.contains("_toDisplayString"),
        "Expected _toDisplayString in slot body, got:\n{result}"
    );
}

// ==================== minor directives ====================

#[test]
fn full_pipeline_v_text() {
    let source = r#"<template><div v-text="msg"></div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_setText"),
        "Expected _setText for v-text, got:\n{result}"
    );
}

#[test]
fn full_pipeline_v_once() {
    // v-once should emit effects as direct statements (no _renderEffect wrapper)
    let source = r#"<template><div v-once>{{ msg }}</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_setText"),
        "Expected _setText, got:\n{result}"
    );
    // v-once means no reactive wrapper
    assert!(
        !result.contains("_renderEffect"),
        "v-once should NOT have _renderEffect, got:\n{result}"
    );
}

#[test]
fn full_pipeline_dynamic_component() {
    let source = r#"<template><component :is="comp">content</component></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_resolveDynamicComponent"),
        "Expected _resolveDynamicComponent, got:\n{result}"
    );
}

#[test]
fn full_pipeline_slot_fallback() {
    // <slot> with children should produce a fallback closure as extra arg
    let source = r#"<template><slot><div>fallback</div></slot></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createSlot"),
        "Expected _createSlot, got:\n{result}"
    );
    // Fallback should be a closure
    assert!(
        result.contains("() =>"),
        "Expected fallback closure in _createSlot, got:\n{result}"
    );
}

// ==================== binding resolution in directives ====================

#[test]
fn full_pipeline_v_if_binding_prefix() {
    // v-if condition should get _ctx. prefix for simple identifiers
    let source = r#"<template><div v-if="show">hello</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_ctx.show"),
        "Expected _ctx.show in v-if condition, got:\n{result}"
    );
}

#[test]
fn full_pipeline_v_for_binding_prefix() {
    // v-for iterable should get _ctx. prefix
    let source = r#"<template><div v-for="item in items">{{ item }}</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_ctx.items"),
        "Expected _ctx.items in v-for iterable, got:\n{result}"
    );
}

// ==================== v-pre ====================

#[test]
fn full_pipeline_v_pre_on_root() {
    // v-pre should suppress expression processing — {{ msg }} becomes literal text
    let source = r#"<template><div v-pre>{{ msg }}</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    // v-pre means no _toDisplayString — content is literal
    assert!(
        !result.contains("_toDisplayString"),
        "v-pre should NOT have _toDisplayString, got:\n{result}"
    );
    // Should produce a static template with the literal text
    assert!(
        result.contains("_template("),
        "Expected _template for v-pre, got:\n{result}"
    );
}

#[test]
fn full_pipeline_v_pre_preserves_directive_syntax() {
    // v-pre should preserve directives as literal attributes
    let source = r#"<template><div v-pre :class="cls" @click="handler">text</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    // No dynamic processing should happen — no setClass, no events
    assert!(
        !result.contains("_setClass"),
        "v-pre should NOT have _setClass, got:\n{result}"
    );
    assert!(
        !result.contains("_on("),
        "v-pre should NOT have _on(), got:\n{result}"
    );
}

#[test]
fn full_pipeline_v_pre_nested_element() {
    // v-pre on a parent applies to all descendants
    let source = r#"<template><div v-pre><span :id="x">{{ y }}</span></div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        !result.contains("_toDisplayString"),
        "v-pre nested should NOT have _toDisplayString, got:\n{result}"
    );
    assert!(
        !result.contains("_setProp"),
        "v-pre nested should NOT have _setProp, got:\n{result}"
    );
}

// ==================== v-cloak ====================

#[test]
fn full_pipeline_v_cloak_stripped() {
    // v-cloak should be stripped from HTML — no trace in output
    let source = r#"<template><div v-cloak class="app">hello</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    // v-cloak must NOT appear in the static template HTML
    assert!(
        !result.contains("v-cloak"),
        "v-cloak should be stripped from HTML, got:\n{result}"
    );
    // static class should still be there
    assert!(
        result.contains("class"),
        "Expected class attribute preserved, got:\n{result}"
    );
}

// ==================== v-memo ====================

#[test]
fn full_pipeline_v_memo() {
    // v-memo should wrap the render effect body with _withMemo
    let source = r#"<template><div v-memo="[x]" :class="cls">hello</div></template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_withMemo"),
        "Expected _withMemo for v-memo, got:\n{result}"
    );
}

// ==================== v-for + v-slot dual scope regression ====================

/// Regression: `<template v-for #slot>` pushes two scopes (Structural + NamedSlot).
/// Both must be popped on leave. Previously only the NamedSlot was popped,
/// leaking the Structural scope and corrupting depth tracking.
#[test]
fn full_pipeline_template_v_for_with_v_slot() {
    let source = r#"<template>
    <MyComp :pt="theme">
        <template v-for="(_, slotName) in $slots" #[slotName]="slotProps">
            <slot :name="slotName" v-bind="slotProps ?? {}" />
        </template>
    </MyComp>
</template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createComponent"),
        "Expected _createComponent, got:\n{result}"
    );
    assert!(
        result.contains("_createFor"),
        "Expected _createFor for v-for, got:\n{result}"
    );
}

/// Regression: `<template v-if #slot>` also pushes two scopes.
/// The v-if on a named slot template creates a conditional slot — the
/// important thing is that both scopes are popped without crashing.
#[test]
fn full_pipeline_template_v_if_with_v_slot() {
    let source = r#"<template>
    <MyComp>
        <template v-if="hasHeader" #header>
            <span>Header</span>
        </template>
        <template #default>
            <div>Content</div>
        </template>
    </MyComp>
</template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createComponent"),
        "Expected _createComponent, got:\n{result}"
    );
    // Both slots should be generated (header and default)
    assert!(
        result.contains("header:"),
        "Expected header slot, got:\n{result}"
    );
    assert!(
        result.contains("default:"),
        "Expected default slot, got:\n{result}"
    );
}

/// Regression: after a `<template v-for #slot>`, sibling elements at the same
/// depth must have correct depth tracking (this is the actual crash scenario —
/// depth underflow caused compute_dom_child_index on root nodes).
#[test]
fn full_pipeline_v_for_v_slot_then_sibling_with_dynamic_props() {
    let source = r#"<template>
    <MyComp>
        <template v-for="item in items" #[item.slot]="props">
            <span>{{ props.text }}</span>
        </template>
    </MyComp>
    <div :class="cls">after component</div>
</template>"#;
    let result = run_full_pipeline(source, super::super::CodeGenMode::Vapor2);
    assert!(
        result.contains("_createComponent"),
        "Expected _createComponent, got:\n{result}"
    );
    // The <div :class="cls"> after the component must not crash and should
    // produce a template declaration (it's a root element).
    assert!(
        result.contains("_template("),
        "Expected _template for sibling div, got:\n{result}"
    );
}
