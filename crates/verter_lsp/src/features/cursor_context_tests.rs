use super::*;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use verter_semantic::analysis::template::*;
use verter_session::FileAnalysisSnapshot;
use verter_span::Span;

// =============================================================================
// Helpers
// =============================================================================

fn empty_analysis() -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        ..Default::default()
    }
}

fn analysis_with_template(template: TemplateAnalysisSnapshot) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        template: Some(template.into()),
        ..Default::default()
    }
}

fn empty_template() -> TemplateAnalysisSnapshot {
    TemplateAnalysisSnapshot::default()
}

fn make_element(
    tag: &str,
    span: (u32, u32),
    tag_span_end: u32,
    content_end: u32,
) -> TemplateElement {
    TemplateElement {
        tag: tag.to_string(),
        span: Span::new(span.0, span.1),
        tag_span_end,
        content_end,
        is_component: tag.starts_with(|c: char| c.is_ascii_uppercase()),
        is_self_closing: false,
        namespace: ElementNamespace::Html,
        attributes: vec![],
        directives: vec![],
        v_for: None,
        v_model: None,
        has_v_if: false,
        has_v_else: false,
        has_v_else_if: false,
        v_if_condition: None,
        has_v_show: false,
        has_v_html: false,
        has_v_text: false,
        has_text_content: false,
        has_bare_text: false,
        has_element_children: false,
        nesting_depth: 0,
        parent_tag: None,
        parent_index: None,
        dynamic_classes: vec![],
        text_children: vec![],
        dynamic_style_vars: vec![],
        static_style_vars: vec![],
        component_usage_index: None,
    }
}

fn make_attr(name: &str, span: (u32, u32), name_end: u32) -> TemplateAttribute {
    TemplateAttribute {
        name: name.to_string(),
        value: None,
        is_dynamic: false,
        span: Span::new(span.0, span.1),
        name_end,
        value_span: None,
    }
}

fn make_directive(
    name: &str,
    raw_name: &str,
    span: (u32, u32),
    name_end: u32,
) -> TemplateDirective {
    TemplateDirective {
        name: name.to_string(),
        raw_name: raw_name.to_string(),
        argument: None,
        modifiers: vec![],
        expression: None,
        span: Span::new(span.0, span.1),
        name_end,
        arg_span: None,
        expression_span: None,
        modifier_spans: vec![],
    }
}

fn make_directive_with_expr(
    name: &str,
    raw_name: &str,
    span: (u32, u32),
    name_end: u32,
    expression_span: (u32, u32),
) -> TemplateDirective {
    TemplateDirective {
        name: name.to_string(),
        raw_name: raw_name.to_string(),
        argument: None,
        modifiers: vec![],
        expression: None,
        span: Span::new(span.0, span.1),
        name_end,
        arg_span: None,
        expression_span: Some(Span::new(expression_span.0, expression_span.1)),
        modifier_spans: vec![],
    }
}

fn make_directive_with_modifiers(
    name: &str,
    raw_name: &str,
    argument: Option<&str>,
    span: (u32, u32),
    name_end: u32,
    modifiers: Vec<&str>,
    modifier_spans: Vec<(u32, u32)>,
) -> TemplateDirective {
    TemplateDirective {
        name: name.to_string(),
        raw_name: raw_name.to_string(),
        argument: argument.map(|s| s.to_string()),
        modifiers: modifiers.into_iter().map(|s| s.to_string()).collect(),
        expression: None,
        modifier_spans: modifier_spans
            .into_iter()
            .map(|(s, e)| Span::new(s, e))
            .collect(),
        span: Span::new(span.0, span.1),
        name_end,
        arg_span: None,
        expression_span: None,
    }
}

// =============================================================================
// Layer 1: SFC-Level Classification
// =============================================================================

#[test]
fn test_root_level_outside_all_blocks() {
    let source = "<template><div></div></template>\n<script setup>\n</script>\n";
    let blocks = scan_sfc_blocks(source);

    // Cursor after </script>\n
    let ctx = classify_cursor_context(57, source, &blocks, None);
    assert!(
        matches!(ctx, CursorContext::RootLevel),
        "should be RootLevel outside all blocks, got: {:?}",
        ctx
    );
}

#[test]
fn uppercase_tag_at_vue_root_stays_root_level() {
    let source = "<template><div></div></template>\n<DraftCard ";
    let blocks = scan_sfc_blocks(source);
    let cursor = source.find("<DraftCard ").unwrap() + "<DraftCard ".len();

    let ctx = classify_cursor_context(cursor as u32, source, &blocks, None);
    assert!(
        matches!(ctx, CursorContext::RootLevel),
        "an invalid Vue-root component tag must not be classified as template markup: {ctx:?}"
    );
}

#[test]
fn script_only_vue_does_not_enable_svelte_root_markup() {
    let source = "<script>export default {}</script>\n<DraftCard ";
    let blocks = scan_sfc_blocks(source);
    let cursor = source.len() as u32;

    let ctx = classify_cursor_context_for_language(
        cursor,
        source,
        &blocks,
        None,
        Some(CarrierTemplateLanguage::Vue),
    );
    assert!(
        matches!(ctx, CursorContext::RootLevel),
        "script-only Vue must not classify carrier-root markup as a template: {ctx:?}"
    );
}

#[test]
fn svelte_template_element_does_not_disable_root_markup() {
    let source = "<template><span>fragment</span></template>\n<DraftCard ";
    let blocks = scan_sfc_blocks(source);
    let cursor = source.len() as u32;

    let ctx = classify_cursor_context_for_language(
        cursor,
        source,
        &blocks,
        None,
        Some(CarrierTemplateLanguage::Svelte),
    );
    assert!(
        matches!(
            ctx,
            CursorContext::Template(TemplateCursorContext::AttributeName {
                ref tag_name,
                is_component: true,
                ..
            }) if tag_name == "DraftCard"
        ),
        "a valid Svelte <template> element must not suppress later root markup: {ctx:?}"
    );
}

#[test]
fn paired_svelte_template_element_opening_and_content_use_template_semantics() {
    let source = "<template >hello</template>";
    let blocks = scan_sfc_blocks(source);
    let mut template = empty_template();
    template
        .elements
        .push(make_element("template", (0, 27), 11, 16));
    let analysis = analysis_with_template(template);

    let opening = classify_cursor_context_for_language(
        10,
        source,
        &blocks,
        Some(&analysis),
        Some(CarrierTemplateLanguage::Svelte),
    );
    assert!(
        matches!(
            opening,
            CursorContext::Template(TemplateCursorContext::AttributeName {
                ref tag_name,
                is_component: false,
                ..
            }) if tag_name == "template"
        ),
        "paired Svelte <template> opening must be ordinary markup: {opening:?}"
    );

    let content = classify_cursor_context_for_language(
        12,
        source,
        &blocks,
        Some(&analysis),
        Some(CarrierTemplateLanguage::Svelte),
    );
    assert!(
        matches!(
            content,
            CursorContext::Template(TemplateCursorContext::TextContent)
        ),
        "paired Svelte <template> content must be ordinary markup: {content:?}"
    );
}

#[test]
fn test_script_block_content() {
    let source = "<script setup>\nconst x = 1\n</script>\n";
    let blocks = scan_sfc_blocks(source);

    let ctx = classify_cursor_context(20, source, &blocks, None);
    assert!(
        matches!(ctx, CursorContext::Script),
        "should be Script in script content, got: {:?}",
        ctx
    );
}

#[test]
fn test_style_block_general() {
    let source =
        "<template><div></div></template>\n<style scoped>\n.foo { color: red; }\n</style>\n";
    let blocks = scan_sfc_blocks(source);
    let analysis = empty_analysis();

    let ctx = classify_cursor_context(55, source, &blocks, Some(&analysis));
    assert!(
        matches!(ctx, CursorContext::Style(StyleCursorContext::General)),
        "should be Style(General) in style content, got: {:?}",
        ctx
    );
}

#[test]
fn test_style_block_vbind() {
    let source = "<template><div></div></template>\n<style scoped>\n.foo { color: v-bind(color); }\n</style>\n";
    let blocks = scan_sfc_blocks(source);

    let analysis = FileAnalysisSnapshot {
        styles: (vec![verter_semantic::analysis::StyleBlockAnalysis {
            v_binds: vec![verter_semantic::analysis::style::AnalyzedVBind {
                expression: "color".to_string(),
                quoted: false,
                start: 68,
                end: 73,
                generated_var_name: None,
            }],
            ..Default::default()
        }])
        .into(),
        ..Default::default()
    };

    let ctx = classify_cursor_context(70, source, &blocks, Some(&analysis));
    assert!(
        matches!(ctx, CursorContext::Style(StyleCursorContext::VBind)),
        "should be Style(VBind) inside v-bind() expression, got: {:?}",
        ctx
    );
}

#[test]
fn test_block_opening_tag() {
    let source = "<script setup lang=\"ts\">\n</script>\n";
    let blocks = scan_sfc_blocks(source);

    let ctx = classify_cursor_context(10, source, &blocks, None);
    assert!(
        matches!(ctx, CursorContext::BlockOpeningTag { .. }),
        "should be BlockOpeningTag, got: {:?}",
        ctx
    );
}

#[test]
fn test_block_closing_tag() {
    let source = "<script setup>\n</script>\n";
    let blocks = scan_sfc_blocks(source);

    let ctx = classify_cursor_context(18, source, &blocks, None);
    assert!(
        matches!(ctx, CursorContext::BlockClosingTag),
        "should be BlockClosingTag, got: {:?}",
        ctx
    );
}

// =============================================================================
// Layer 1: Template Sub-Context — Tag Names
// =============================================================================

#[test]
fn test_template_tag_name() {
    let source = "<template><div></div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    template
        .elements
        .push(make_element("div", (10, 20), 14, 15));

    let analysis = analysis_with_template(template);

    // Cursor at 12 — on "iv" of "<div"
    let ctx = classify_cursor_context(12, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::TagName { partial }) => {
            assert!(
                partial.contains("d"),
                "partial should contain part of 'div', got: '{}'",
                partial
            );
        }
        other => panic!("expected TagName, got: {:?}", other),
    }
}

#[test]
fn test_template_closing_tag_name() {
    let source = "<template><div></div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    template
        .elements
        .push(make_element("div", (10, 20), 14, 15));

    let analysis = analysis_with_template(template);

    // Cursor at 17 — inside "</div>" on "iv"
    let ctx = classify_cursor_context(17, source, &blocks, Some(&analysis));
    assert!(
        matches!(
            ctx,
            CursorContext::Template(TemplateCursorContext::ClosingTagName { .. })
        ),
        "should be ClosingTagName, got: {:?}",
        ctx
    );
}

// =============================================================================
// Layer 1: Template Sub-Context — Attribute Names
// =============================================================================

#[test]
fn test_template_attribute_name_html() {
    //                   0         1         2         3
    //                   0123456789012345678901234567890123456789
    let source = "<template><div class=\"foo\"></div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("div", (10, 31), 25, 25);
    el.attributes.push(make_attr("class", (15, 24), 20));
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 17 — on "as" of "class"
    let ctx = classify_cursor_context(17, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::AttributeName { tag_name, .. }) => {
            assert_eq!(tag_name, "div");
        }
        other => panic!("expected AttributeName, got: {:?}", other),
    }
}

#[test]
fn test_template_attribute_name_gap() {
    //                   0         1         2         3         4
    //                   01234567890123456789012345678901234567890123456789
    let source = "<template><div class=\"foo\" ></div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("div", (10, 33), 27, 27);
    el.attributes.push(make_attr("class", (15, 24), 20));
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 26 — in the gap after class="foo" and before >
    let ctx = classify_cursor_context(26, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::AttributeName { tag_name, .. }) => {
            assert_eq!(tag_name, "div");
        }
        other => panic!("expected AttributeName for gap, got: {:?}", other),
    }
}

#[test]
fn test_template_attribute_name_component() {
    let source = "<template><MyComp ></MyComp></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("MyComp", (10, 27), 18, 18);
    el.is_component = true;
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 17 — after "MyComp " and before ">"
    let ctx = classify_cursor_context(17, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::AttributeName {
            tag_name,
            is_component,
            ..
        }) => {
            assert_eq!(tag_name, "MyComp");
            assert!(is_component, "should be marked as component");
        }
        other => panic!("expected AttributeName for component, got: {:?}", other),
    }
}

// =============================================================================
// Layer 1: Template Sub-Context — Event Modifiers
// =============================================================================

#[test]
fn test_event_modifier_context() {
    //                   0         1         2         3         4         5
    //                   01234567890123456789012345678901234567890123456789012345
    let source = "<template><div @click.prevent.></div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("div", (10, 36), 30, 30);
    el.directives.push(make_directive_with_modifiers(
        "on",
        "@click.prevent.",
        Some("click"),
        (15, 29),
        17,
        vec!["prevent"],
        vec![(22, 29)],
    ));
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 29 — right after the last "."
    let ctx = classify_cursor_context(29, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::EventModifier { event_name, .. }) => {
            assert_eq!(event_name, "click");
        }
        other => panic!("expected EventModifier, got: {:?}", other),
    }
}

#[test]
fn event_handler_value_member_access_is_expression_not_modifier() {
    // G3: in `@click="handle($event.)"` the `.` after `$event` is MEMBER ACCESS in
    // the handler value, NOT an event-modifier separator. The modifier scan is
    // clipped at the `=` value assignment, so a value-position `.` routes to
    // expression/member completion (answered by the type provider), never to the
    // EventModifier completions. The sibling `test_event_modifier_context` only pins
    // the modifier-position case; this pins the value-position case it must NOT be.
    let source = r#"<template><button @click="handle($event.)"></button></template>"#;
    let blocks = scan_sfc_blocks(source);

    let at = source.find("@click").unwrap() as u32;
    let click = source.find("click").unwrap() as u32;
    let value = "handle($event.)";
    let expr_start = source.find(value).unwrap() as u32;
    let expr_end = expr_start + value.len() as u32; // position of the closing `"`
    let dir_span_end = expr_end + 1; // include the closing quote
    let button_start = source.find("<button").unwrap() as u32;
    let tag_close = button_start + source[button_start as usize..].find('>').unwrap() as u32 + 1;

    let mut template = empty_template();
    let mut el = make_element(
        "button",
        (button_start, source.len() as u32),
        tag_close,
        tag_close,
    );
    el.directives.push(TemplateDirective {
        name: "on".into(),
        raw_name: "@click".into(),
        argument: Some("click".into()),
        modifiers: vec![],
        expression: Some(value.into()),
        span: Span::new(at, dir_span_end),
        name_end: click, // end of `@click` name token (= start of the `click` arg)
        arg_span: Some(Span::new(click, click + 5)),
        expression_span: Some(Span::new(expr_start, expr_end)),
        modifier_spans: vec![],
    });
    template.elements.push(el);
    let analysis = analysis_with_template(template);

    // Cursor right after `$event.` — the member-access dot inside the handler value.
    let cursor = (source.find("$event.").unwrap() + "$event.".len()) as u32;
    let ctx = classify_cursor_context(cursor, source, &blocks, Some(&analysis));
    assert!(
        matches!(
            ctx,
            CursorContext::Template(TemplateCursorContext::Expression { .. })
        ),
        "value-position `.` must be a template Expression (member) context, got: {:?}",
        ctx
    );
    assert!(
        !matches!(
            ctx,
            CursorContext::Template(TemplateCursorContext::EventModifier { .. })
        ),
        "value-position `.` must NOT be classified as an EventModifier, got: {:?}",
        ctx
    );
}

#[test]
fn test_vmodel_modifier_context() {
    let source = "<template><input v-model.lazy.></input></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("input", (10, 38), 30, 30);
    el.directives.push(make_directive_with_modifiers(
        "model",
        "v-model.lazy.",
        None,
        (17, 29),
        24,
        vec!["lazy"],
        vec![(25, 29)],
    ));
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 29 — right after the last "."
    let ctx = classify_cursor_context(29, source, &blocks, Some(&analysis));
    assert!(
        matches!(
            ctx,
            CursorContext::Template(TemplateCursorContext::VModelModifier { .. })
        ),
        "expected VModelModifier, got: {:?}",
        ctx
    );
}

// =============================================================================
// Layer 1: Template Sub-Context — Expressions
// =============================================================================

#[test]
fn test_directive_expression() {
    //                   0         1         2         3         4
    //                   01234567890123456789012345678901234567890123456789
    let source = "<template><div v-if=\"show\"></div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("div", (10, 32), 26, 26);
    el.directives.push(make_directive_with_expr(
        "if",
        "v-if",
        (15, 25),
        19,
        (20, 24),
    ));
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 22 — inside "show"
    let ctx = classify_cursor_context(22, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::Expression {
            kind: ExpressionKind::VIf,
        }) => {}
        other => panic!("expected Expression(VIf), got: {:?}", other),
    }
}

#[test]
fn test_event_handler_expression() {
    //                   0         1         2         3         4         5
    //                   012345678901234567890123456789012345678901234567890123456789
    let source = "<template><button @click=\"handleClick\"></button></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("button", (10, 48), 38, 38);
    el.directives.push({
        let mut d = make_directive_with_expr("on", "@click", (17, 37), 23, (25, 36));
        d.argument = Some("click".to_string());
        d
    });
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 30 — inside "handleClick"
    let ctx = classify_cursor_context(30, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::Expression {
            kind: ExpressionKind::EventHandler { event_name },
        }) => {
            assert_eq!(event_name, "click");
        }
        other => panic!("expected Expression(EventHandler), got: {:?}", other),
    }
}

#[test]
fn test_dynamic_prop_expression() {
    let source = "<template><div :title=\"msg\"></div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("div", (10, 33), 27, 27);
    el.directives.push({
        let mut d = make_directive_with_expr("bind", ":title", (15, 26), 21, (22, 25));
        d.argument = Some("title".to_string());
        d
    });
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 23 — inside "msg"
    let ctx = classify_cursor_context(23, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::Expression {
            kind: ExpressionKind::Prop { prop_name },
        }) => {
            assert_eq!(prop_name, "title");
        }
        other => panic!("expected Expression(Prop), got: {:?}", other),
    }
}

// Stale analysis: cursor past expression_span.end but still in directive
// Simulates user typing more into the expression after analysis was computed.
#[test]
fn test_expression_stale_analysis_cursor_past_expr_end() {
    // Current source has a longer expression than what analysis captured
    //                    1111111111222222222233333333334444
    //          01234567890123456789012345678901234567890123
    let source = "<template><div :icon=\"action.icon || x\"></div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    // The element span covers the whole <div ...> tag up to </div>
    let mut el = make_element("div", (10, 45), 39, 39);
    // Stale analysis: expression_span only covers "action.icon" (21..32)
    // but user has since typed " || x" making actual content "action.icon || x" (21..37)
    el.directives.push({
        let mut d = make_directive_with_expr("bind", ":icon", (15, 38), 20, (21, 32));
        d.argument = Some("icon".to_string());
        d
    });
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 35 — on "x" in "|| x", past the stale expression_span.end (32)
    let ctx = classify_cursor_context(35, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::Expression { .. }) => {}
        other => panic!(
            "expected Expression for cursor past stale expr_span.end, got: {:?}",
            other
        ),
    }
}

// Cursor exactly at expression_span.end boundary
#[test]
fn test_expression_at_expr_span_end_boundary() {
    //                    1111111111222222222233
    //          0123456789012345678901234567890123
    let source = "<template><div :icon=\"action.icon\"></div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("div", (10, 40), 34, 34);
    el.directives.push({
        let mut d = make_directive_with_expr("bind", ":icon", (15, 33), 20, (21, 32));
        d.argument = Some("icon".to_string());
        d
    });
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 32 — exactly at expression_span.end (exclusive boundary)
    // This is the position right after the last char of "action.icon" but before the closing quote
    let ctx = classify_cursor_context(32, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::Expression { .. }) => {}
        other => panic!(
            "expected Expression at expr_span.end boundary, got: {:?}",
            other
        ),
    }
}

// =============================================================================
// Layer 1: Template Sub-Context — Interpolations
// =============================================================================

#[test]
fn test_interpolation() {
    let source = "<template><div>{{ count }}</div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("div", (10, 31), 14, 25);
    el.text_children.push(TemplateTextSegment::Interpolation {
        span: Span::new(15, 25),
        expression_span: Span::new(18, 23),
    });
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 20 — inside "count"
    let ctx = classify_cursor_context(20, source, &blocks, Some(&analysis));
    assert!(
        matches!(
            ctx,
            CursorContext::Template(TemplateCursorContext::Interpolation)
        ),
        "expected Interpolation, got: {:?}",
        ctx
    );
}

// =============================================================================
// Layer 1: Template Sub-Context — Static Values
// =============================================================================

#[test]
fn test_static_attribute_value() {
    let source = "<template><div class=\"hello\"></div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("div", (10, 34), 28, 28);
    let mut attr = make_attr("class", (15, 27), 20);
    attr.value = Some("hello".to_string());
    attr.value_span = Some(Span::new(22, 27));
    el.attributes.push(attr);
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 24 — inside "hello"
    let ctx = classify_cursor_context(24, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::StaticValue { attr_name }) => {
            assert_eq!(attr_name, "class");
        }
        other => panic!("expected StaticValue, got: {:?}", other),
    }
}

// =============================================================================
// Layer 1: Template Sub-Context — Text Content
// =============================================================================

#[test]
fn test_text_content() {
    let source = "<template><div>hello world</div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("div", (10, 31), 14, 25);
    el.text_children.push(TemplateTextSegment::Text {
        span: Span::new(15, 25),
        is_entity: false,
    });
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 18 — in "hello world" text
    let ctx = classify_cursor_context(18, source, &blocks, Some(&analysis));
    assert!(
        matches!(
            ctx,
            CursorContext::Template(TemplateCursorContext::TextContent)
        ),
        "expected TextContent, got: {:?}",
        ctx
    );
}

// =============================================================================
// Layer 1: Template Sub-Context — Existing Attrs Dedup
// =============================================================================

#[test]
fn test_attribute_name_existing_attrs() {
    let source = "<template><div class=\"foo\" id=\"bar\" ></div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("div", (10, 43), 36, 36);
    el.attributes.push(make_attr("class", (15, 24), 20));
    el.attributes.push(make_attr("id", (26, 33), 28));
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 35 — in the gap after id="bar"
    let ctx = classify_cursor_context(35, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::AttributeName {
            existing_attrs, ..
        }) => {
            assert!(
                existing_attrs.contains(&"class".to_string()),
                "existing_attrs should contain 'class'"
            );
            assert!(
                existing_attrs.contains(&"id".to_string()),
                "existing_attrs should contain 'id'"
            );
        }
        other => panic!(
            "expected AttributeName with existing_attrs, got: {:?}",
            other
        ),
    }
}

// =============================================================================
// Layer 1: Template without analysis — fallback
// =============================================================================

#[test]
fn test_template_without_analysis_returns_template_fallback() {
    let source = "<template><div></div></template>\n";
    let blocks = scan_sfc_blocks(source);

    let ctx = classify_cursor_context(12, source, &blocks, None);
    assert!(
        matches!(ctx, CursorContext::Template(_)),
        "should still be Template context without analysis, got: {:?}",
        ctx
    );
}

// =============================================================================
// Layer 2: Expression Sub-Context (OXC)
// =============================================================================

#[test]
fn test_expression_context_member_access() {
    let tsx = "foo.";
    let ctx = classify_expression_context(tsx, 4);
    assert!(
        matches!(ctx, ExpressionContext::MemberAccess),
        "expected MemberAccess after '.', got: {:?}",
        ctx
    );
}

#[test]
fn test_expression_context_member_access_partial() {
    let tsx = "foo.bar";
    let ctx = classify_expression_context(tsx, 7);
    assert!(
        matches!(ctx, ExpressionContext::MemberAccess),
        "expected MemberAccess for 'foo.bar', got: {:?}",
        ctx
    );
}

#[test]
fn test_expression_context_identifier() {
    let tsx = "count";
    let ctx = classify_expression_context(tsx, 5);
    assert!(
        matches!(ctx, ExpressionContext::IdentifierExpected),
        "expected IdentifierExpected, got: {:?}",
        ctx
    );
}

#[test]
fn test_expression_context_string_literal() {
    let tsx = "'hello'";
    let ctx = classify_expression_context(tsx, 4);
    assert!(
        matches!(ctx, ExpressionContext::Literal),
        "expected Literal inside string, got: {:?}",
        ctx
    );
}

#[test]
fn test_expression_context_number_literal() {
    let tsx = "1.5";
    let ctx = classify_expression_context(tsx, 2);
    assert!(
        matches!(ctx, ExpressionContext::Literal),
        "expected Literal for number, got: {:?}",
        ctx
    );
}

#[test]
fn test_expression_context_optional_chaining() {
    let tsx = "foo?.bar";
    let ctx = classify_expression_context(tsx, 8);
    assert!(
        matches!(ctx, ExpressionContext::MemberAccess),
        "expected MemberAccess for optional chaining, got: {:?}",
        ctx
    );
}

#[test]
fn test_expression_context_trigger_dot_shortcut() {
    let ctx = classify_expression_context_with_trigger("action.", 7, Some("."));
    assert!(
        matches!(ctx, ExpressionContext::MemberAccess),
        "expected MemberAccess for dot trigger, got: {:?}",
        ctx
    );
}

#[test]
fn test_expression_context_object_property_key() {
    let tsx = "{ key: value }";
    let ctx = classify_expression_context(tsx, 3);
    assert!(
        matches!(ctx, ExpressionContext::PropertyKey),
        "expected PropertyKey, got: {:?}",
        ctx
    );
}

#[test]
fn test_expression_context_empty() {
    let tsx = "";
    let ctx = classify_expression_context(tsx, 0);
    assert!(
        matches!(
            ctx,
            ExpressionContext::IdentifierExpected | ExpressionContext::Unknown
        ),
        "expected IdentifierExpected or Unknown for empty input, got: {:?}",
        ctx
    );
}

// =============================================================================
// Layer 2: Expression Sub-Context — Unknown (broken expressions)
// =============================================================================

#[test]
fn test_expression_context_unknown_broken_binary() {
    // "count +" is a broken binary expression — OXC panics → Unknown
    let tsx = "count +";
    let ctx = classify_expression_context(tsx, 7);
    assert!(
        matches!(ctx, ExpressionContext::Unknown),
        "expected Unknown for broken binary 'count +', got: {:?}",
        ctx
    );
}

#[test]
fn test_expression_context_unknown_trailing_pipe() {
    // "count |" is also broken — Unknown
    let tsx = "count |";
    let ctx = classify_expression_context(tsx, 7);
    assert!(
        matches!(ctx, ExpressionContext::Unknown),
        "expected Unknown for broken 'count |', got: {:?}",
        ctx
    );
}

// =============================================================================
// Layer 1: Nested elements — deepest wins
// =============================================================================

#[test]
fn test_nested_elements_deepest_wins() {
    let source = "<template><div><span ></span></div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    template
        .elements
        .push(make_element("div", (10, 34), 14, 28));
    let mut span_el = make_element("span", (15, 28), 21, 21);
    span_el.parent_index = Some(0);
    template.elements.push(span_el);

    let analysis = analysis_with_template(template);

    // Cursor at 20 — inside <span > tag but after tag name
    let ctx = classify_cursor_context(20, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::AttributeName { tag_name, .. }) => {
            assert_eq!(tag_name, "span", "should pick deepest element (span)");
        }
        other => panic!("expected AttributeName for span, got: {:?}", other),
    }
}

// =============================================================================
// Layer 1: DirectiveArgument context
// =============================================================================

#[test]
fn test_directive_argument() {
    let source = "<template><div v-slot:default></div></template>";
    let blocks = scan_sfc_blocks(source);

    let mut template = empty_template();
    let mut el = make_element("div", (10, 35), 29, 29);
    let mut dir = make_directive("slot", "v-slot:default", (15, 28), 21);
    dir.argument = Some("default".to_string());
    dir.arg_span = Some(Span::new(22, 29));
    el.directives.push(dir);
    template.elements.push(el);

    let analysis = analysis_with_template(template);

    // Cursor at 25 — inside "default" argument
    let ctx = classify_cursor_context(25, source, &blocks, Some(&analysis));
    match ctx {
        CursorContext::Template(TemplateCursorContext::DirectiveArgument {
            directive,
            tag_name,
        }) => {
            assert_eq!(directive, "slot");
            assert_eq!(tag_name, "div");
        }
        other => panic!("expected DirectiveArgument, got: {:?}", other),
    }
}
