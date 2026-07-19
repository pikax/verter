use super::*;
use crate::ast::types::AstNodeKind;
use crate::diagnostics::{CompilerErrorCode, SyntaxPluginContext, SyntaxPluginOptions};

/// Helper: create a SyntaxPluginContext from a source string.
fn make_ctx<'a>(input: &'a str, options: &'a SyntaxPluginOptions) -> SyntaxPluginContext<'a> {
    SyntaxPluginContext {
        input,
        bytes: input.as_bytes(),
        options,
        diagnostics: Vec::new(),
    }
}

/// Helper: feed a slice of events into a Syntax instance.
fn feed<'a>(syntax: &mut Syntax, events: &[TokenizerEvent<'a>], ctx: &SyntaxPluginContext<'a>) {
    for event in events {
        syntax.handle(event, ctx);
    }
}

/// Tokenize input using the byte tokenizer and collect events.
fn tokenize_events(input: &str) -> Vec<TokenizerEvent<'static>> {
    let mut events = Vec::new();
    crate::tokenizer::byte::tokenize(input.as_bytes(), |event| {
        events.push(event);
    });
    events
}

/// Tokenize and feed all events into a Syntax instance.
fn tokenize_and_feed(syntax: &mut Syntax, input: &str, ctx: &SyntaxPluginContext<'_>) {
    let events = tokenize_events(input);
    feed(syntax, &events, ctx);
}

/// Tokenize in SFC mode and feed all events into a Syntax instance.
fn tokenize_sfc_and_feed(syntax: &mut Syntax, input: &str, ctx: &SyntaxPluginContext<'_>) {
    let mut events = Vec::new();
    crate::tokenizer::byte::tokenize_sfc(input.as_bytes(), |event| {
        events.push(event);
    });
    feed(syntax, &events, ctx);
}

/// Extract a substring using u32 span indices.
fn span_str(input: &str, start: u32, end: u32) -> &str {
    &input[start as usize..end as usize]
}

/// Parse an SFC string into a ParsedSfc using the parser's own API.
fn parse_sfc(input: &str) -> crate::parser::types::ParsedSfc {
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_sfc_and_feed(&mut syn, input, &ctx);
    syn.into_parsed_sfc()
}

// ========================================================================
// 1. Script root nodes
// ========================================================================

#[test]
fn script_basic() {
    let input = "<script>console.log('hi')</script>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let node = syn.script_node.as_ref().expect("script_node should exist");
    assert_eq!(
        span_str(input, node.tag_open.start + 1, node.tag_open.name_end),
        "script"
    );
    assert!(!node.is_setup);
    assert!(node.lang.is_none());
    assert!(node.src.is_none());
    let content = node.content.as_ref().expect("content should exist");
    assert_eq!(
        span_str(input, content.start, content.end),
        "console.log('hi')"
    );
    assert!(syn.script_setup_node.is_none());
}

#[test]
fn script_setup_with_lang_ts() {
    let input = "<script setup lang=\"ts\"></script>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.script_node.is_none(),
        "non-setup script_node should be None"
    );
    let node = syn
        .script_setup_node
        .as_ref()
        .expect("script_setup_node should exist");
    assert!(node.is_setup);
    assert_eq!(node.lang, Some(ScriptLanguage::TypeScript));
}

#[test]
fn script_setup_flag_not_applied_to_style() {
    // "setup" on <style> should NOT set prop_setup
    let input = "<style setup></style>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert_eq!(syn.style_nodes.len(), 1);
    assert!(!syn.style_nodes[0].scoped);
    assert!(syn.script_setup_node.is_none());
}

// ========================================================================
// 2. Style root nodes
// ========================================================================

#[test]
fn style_scoped_with_lang_scss() {
    let input = "<style scoped lang=\"scss\"></style>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert_eq!(syn.style_nodes.len(), 1);
    let style = &syn.style_nodes[0];
    assert!(style.scoped);
    assert!(!style.module);
    assert_eq!(style.lang, Some(StyleLang::Scss));
    assert!(syn.has_style_scope);
}

#[test]
fn style_module() {
    let input = "<style module></style>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert_eq!(syn.style_nodes.len(), 1);
    assert!(syn.style_nodes[0].module);
    assert!(syn.has_style_module);
}

#[test]
fn scoped_flag_not_applied_to_script() {
    // "scoped" on <script> should NOT set prop_scoped
    let input = "<script scoped></script>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let node = syn.script_node.as_ref().expect("script_node should exist");
    assert!(!node.is_setup);
    assert!(!syn.has_style_scope);
}

// ========================================================================
// 3. Template root nodes (SFC mode)
// ========================================================================

#[test]
fn template_basic_with_child() {
    let input = "<template><div>hello</div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn
        .template_ast
        .as_ref()
        .expect("template_ast should exist");
    assert!(ast.root.tag_close.is_some());
    let content = ast.root.content.as_ref().expect("content should exist");
    assert_eq!(
        span_str(input, content.start, content.end),
        "<div>hello</div>"
    );
    assert_eq!(content.children.len(), 1);
    let div = &ast.nodes[content.children[0].0];
    if let AstNodeKind::Element(el) = &div.kind {
        let el_content = el.content.as_ref().unwrap();
        assert_eq!(el_content.children.len(), 1);
        let text = &ast.nodes[el_content.children[0].0];
        assert!(matches!(text.kind, AstNodeKind::Text(_)));
    } else {
        panic!("expected Element, got {:?}", div.kind);
    }
}

#[test]
fn element_tag_open_end_is_past_closing_bracket() {
    // Regression test: tag_open.end must be the byte offset past `>`,
    // not the start position. Previously open_element was called before
    // OpenTagEnd fired, leaving tag_open.end == tag_open.start.
    let input = "<template><div>hello</div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn
        .template_ast
        .as_ref()
        .expect("template_ast should exist");
    let content = ast.root.content.as_ref().expect("content should exist");
    let div = &ast.nodes[content.children[0].0];
    if let AstNodeKind::Element(el) = &div.kind {
        // <div> starts at 10, ends at 15 (past the >)
        assert_eq!(
            el.tag_open.start, 10,
            "tag_open.start should be '<' of <div>"
        );
        assert_eq!(
            el.tag_open.end, 15,
            "tag_open.end should be past '>' of <div>"
        );
        assert_eq!(span_str(input, el.tag_open.start, el.tag_open.end), "<div>");
    } else {
        panic!("expected Element, got {:?}", div.kind);
    }
}

#[test]
fn self_closing_element_tag_open_end_is_past_closing_bracket() {
    let input = "<template><br/></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn
        .template_ast
        .as_ref()
        .expect("template_ast should exist");
    let content = ast.root.content.as_ref().expect("content should exist");
    let br = &ast.nodes[content.children[0].0];
    if let AstNodeKind::Element(el) = &br.kind {
        // <br/> starts at 10, ends at 15 (past the />)
        assert_eq!(
            el.tag_open.start, 10,
            "tag_open.start should be '<' of <br/>"
        );
        assert_eq!(
            el.tag_open.end, 15,
            "tag_open.end should be past '/>' of <br/>"
        );
        assert_eq!(span_str(input, el.tag_open.start, el.tag_open.end), "<br/>");
        assert!(el.is_self_closing);
    } else {
        panic!("expected Element, got {:?}", br.kind);
    }
}

#[test]
fn template_self_closing() {
    let input = "<template />";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn
        .template_ast
        .as_ref()
        .expect("template_ast should exist");
    assert!(ast.root.tag_close.is_none());
    assert!(ast.root.content.is_none());
}

#[test]
fn template_vapor_flag() {
    let input = "<template vapor></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(syn.is_vapor);
    assert!(syn.template_ast.is_some());
}

// ========================================================================
// 4. Unknown root nodes
// ========================================================================

#[test]
fn unknown_root_node() {
    let input = "<custom-block>data</custom-block>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert_eq!(syn.unknown_nodes.len(), 1);
    let node = &syn.unknown_nodes[0];
    let content = node.content.as_ref().expect("content should exist");
    assert_eq!(span_str(input, content.start, content.end), "data");
}

// ========================================================================
// 5. Self-closing script/style
// ========================================================================

#[test]
fn script_self_closing() {
    let input = "<script src=\"./foo.ts\" />";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let node = syn.script_node.as_ref().expect("script_node should exist");
    assert!(node.content.is_none());
    assert!(node.tag_close.is_none());
    let src = node.src.as_ref().expect("src should exist");
    assert_eq!(span_str(input, src.start, src.end), "./foo.ts");
}

// ========================================================================
// 6. template_mode — AST-only, no root detection
// ========================================================================

#[test]
fn template_mode_builds_ast_directly() {
    // In template_mode, input is just template content, no root tags.
    // The byte tokenizer emits End which finalizes the AST.
    let input = "<div><span>text</span></div>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(true);

    tokenize_and_feed(&mut syn, input, &ctx);

    // No root nodes should be detected
    assert!(syn.script_node.is_none());
    assert!(syn.style_nodes.is_empty());

    // End event finalizes the AST — builder consumed, template_ast produced
    assert!(
        syn.ast_builder.is_none(),
        "builder should be consumed by End event"
    );
    let ast = syn
        .template_ast
        .as_ref()
        .expect("template_ast should exist after End event");
    let root_content = ast.root.content.as_ref().unwrap();
    assert_eq!(root_content.children.len(), 1);
}

#[test]
fn template_mode_no_root_prop_detection() {
    // In template_mode, even <script> tags are treated as elements, not roots.
    let input = "<script setup></script>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(true);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(syn.script_node.is_none());
    assert!(syn.script_setup_node.is_none());
    assert!(!syn.prop_setup);
}

// ========================================================================
// 7. Prop state reset timing
// ========================================================================

#[test]
fn prop_state_preserved_until_close_for_script() {
    // Ensure that prop_lang/prop_setup survive from OpenTagEnd to CloseTag.
    let input = "<script setup lang=\"ts\">code</script>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    let events = tokenize_events(input);
    // Feed events up to and including OpenTagEnd, then check intermediate state
    let split = events
        .iter()
        .position(|e| matches!(e, TokenizerEvent::OpenTagEnd { .. }))
        .unwrap()
        + 1;
    feed(&mut syn, &events[..split], &ctx);

    // After OpenTagEnd for script, props should NOT be reset yet
    assert!(syn.prop_setup, "prop_setup should survive OpenTagEnd");
    assert!(
        syn.prop_lang.is_some(),
        "prop_lang should survive OpenTagEnd"
    );

    // Now feed the remaining events (close tag, etc.)
    feed(&mut syn, &events[split..], &ctx);

    // Props should be reset after close
    assert!(!syn.prop_setup, "prop_setup should be reset after close");
    assert!(
        syn.prop_lang.is_none(),
        "prop_lang should be reset after close"
    );

    // The script setup node should have captured them
    let node = syn
        .script_setup_node
        .as_ref()
        .expect("script_setup_node should exist");
    assert!(node.is_setup);
    assert_eq!(node.lang, Some(ScriptLanguage::TypeScript));
}

#[test]
fn style_scoped_module_preserved_until_close() {
    // Ensure scoped/module flags survive from OpenTagEnd to CloseTag.
    let input = "<style scoped module>.a{}</style>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    let events = tokenize_events(input);
    let split = events
        .iter()
        .position(|e| matches!(e, TokenizerEvent::OpenTagEnd { .. }))
        .unwrap()
        + 1;
    feed(&mut syn, &events[..split], &ctx);

    // After OpenTagEnd, flags should still be pending
    assert!(syn.prop_scoped, "prop_scoped should survive OpenTagEnd");
    assert!(syn.prop_module, "prop_module should survive OpenTagEnd");

    feed(&mut syn, &events[split..], &ctx);

    // After close, flags reset and captured in the node
    assert!(!syn.prop_scoped, "prop_scoped should be reset after close");
    assert!(!syn.prop_module, "prop_module should be reset after close");
    assert_eq!(syn.style_nodes.len(), 1);
    assert!(syn.style_nodes[0].scoped);
    assert!(syn.style_nodes[0].module);
    assert!(syn.has_style_scope);
    assert!(syn.has_style_module);
}

// ========================================================================
// 8. Multiple root nodes
// ========================================================================

#[test]
fn multiple_style_nodes() {
    let input = "<style>.a{}</style><style scoped>.b{}</style>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert_eq!(syn.style_nodes.len(), 2);
    assert!(!syn.style_nodes[0].scoped);
    assert!(syn.style_nodes[1].scoped);
}

// ========================================================================
// 9. Interpolation and comment leafs
// ========================================================================

#[test]
fn template_with_interpolation_and_comment() {
    let input = "<template>{{ msg }}<!-- comment --></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn
        .template_ast
        .as_ref()
        .expect("template_ast should exist");
    let content = ast.root.content.as_ref().unwrap();
    assert_eq!(content.children.len(), 2);

    // First child: interpolation
    let interp = &ast.nodes[content.children[0].0];
    if let AstNodeKind::Interpolation(i) = &interp.kind {
        assert_eq!(span_str(input, i.start, i.end), "{{ msg }}");
        assert_eq!(span_str(input, i.inner_start, i.inner_end).trim(), "msg");
    } else {
        panic!("expected Interpolation, got {:?}", interp.kind);
    }

    // Second child: comment
    let comment = &ast.nodes[content.children[1].0];
    if let AstNodeKind::Comment(c) = &comment.kind {
        assert_eq!(
            span_str(input, c.content_start, c.content_end).trim(),
            "comment"
        );
    } else {
        panic!("expected Comment, got {:?}", comment.kind);
    }
}

// ========================================================================
// 10. Directive attributes on template elements
// ========================================================================

#[test]
fn directive_with_arg_and_modifiers() {
    let input = "<template><div @click.stop.prevent=\"handler\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let div = &ast.nodes[div_id.0];
    if let AstNodeKind::Element(el) = &div.kind {
        assert_eq!(el.props.len(), 1);
        let prop = &el.props[0];
        assert!(prop.is_directive);
        let arg_start = prop.arg_start.unwrap();
        let arg_end = prop.arg_end.unwrap();
        assert_eq!(span_str(input, arg_start, arg_end), "click");
        assert_eq!(prop.is_dynamic, Some(false));
        assert_eq!(prop.modifiers.len(), 2);
        assert_eq!(
            span_str(input, prop.modifiers[0].start, prop.modifiers[0].end),
            "stop"
        );
        assert_eq!(
            span_str(input, prop.modifiers[1].start, prop.modifiers[1].end),
            "prevent"
        );
    } else {
        panic!("expected Element, got {:?}", div.kind);
    }
}

#[test]
fn v_pre_is_recorded_as_a_directive_fact() {
    // D6: the v-pre prepass owns the subtree skip (pure tokenizer state), but
    // the v-pre token itself must survive the AST as a typed directive fact —
    // the IDE's directive-name doc hover reads exactly that fact.
    let input = "<template><div v-pre>{{ raw }}</div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let div = &ast.nodes[div_id.0];
    if let AstNodeKind::Element(el) = &div.kind {
        let prop = el
            .props
            .iter()
            .find(|p| p.is_directive)
            .expect("v-pre must be recorded as a directive fact");
        assert_eq!(span_str(input, prop.start, prop.name_end), "v-pre");
        assert!(prop.arg_start.is_none());
        assert!(prop.value_start.is_none());
        // The subtree skip is unchanged: the interpolation stays plain text.
        let has_interpolation = ast
            .nodes
            .iter()
            .any(|node| matches!(node.kind, AstNodeKind::Interpolation(_)));
        assert!(
            !has_interpolation,
            "v-pre subtree must remain uncompiled (no interpolation nodes)"
        );
    } else {
        panic!("expected Element, got {:?}", div.kind);
    }
}

// ========================================================================
// 11. Mismatched tags — strict mode
// ========================================================================

/// @ai-generated - Tests strict close-tag validation with mismatched names.
#[test]
fn mismatched_close_tag_emits_diagnostic_and_preserves_stack() {
    // </span> doesn't match <div>. Strict mode: reject the close tag,
    // emit XInvalidEndTag, and leave <div> unclosed.
    // Then </template> doesn't match <div> either → another XInvalidEndTag.
    // At EOF: <div> and <template> are both unclosed → 2× XMissingEndTag.
    let input = "<template><div></span></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let invalid_end: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XInvalidEndTag)
        .collect();
    let missing_end: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XMissingEndTag)
        .collect();

    // XInvalidEndTag for </span> (vs <div>) and </template> (vs <div>)
    assert_eq!(
        invalid_end.len(),
        2,
        "expected 2 XInvalidEndTag diagnostics, got {}",
        invalid_end.len()
    );

    // XMissingEndTag for <div> and <template> (both unclosed at EOF)
    assert_eq!(
        missing_end.len(),
        2,
        "expected 2 XMissingEndTag diagnostics, got {}",
        missing_end.len()
    );

    // Template AST should still be produced (force-closed at EOF).
    assert!(syn.template_ast.is_some());
}

// ========================================================================
// 12. Orphan close tag (empty stack)
// ========================================================================

/// @ai-generated - Tests that an orphan close tag emits XInvalidEndTag.
#[test]
fn orphan_close_tag_emits_diagnostic() {
    // template_mode: </div> with nothing open
    let input = "</div>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(true);

    tokenize_and_feed(&mut syn, input, &ctx);

    let errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::diagnostics::DiagnosticSeverity::Error)
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, CompilerErrorCode::XInvalidEndTag);
}

// ========================================================================
// 13. Unclosed elements at EOF
// ========================================================================

/// @ai-generated - Tests that unclosed elements emit XMissingEndTag at EOF.
#[test]
fn unclosed_element_at_eof_emits_diagnostic() {
    let input = "<template><div><span>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    // Should have XMissingEndTag for <span>, <div>, and <template>
    let missing_end: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XMissingEndTag)
        .collect();
    assert_eq!(
        missing_end.len(),
        3,
        "expected 3 XMissingEndTag diagnostics (span, div, template), got {}",
        missing_end.len()
    );

    // Template AST should still be produced (force-closed)
    assert!(syn.template_ast.is_some());
}

/// @ai-generated - Tests template_mode unclosed elements at EOF.
#[test]
fn template_mode_unclosed_at_eof() {
    let input = "<div><span>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(true);

    tokenize_and_feed(&mut syn, input, &ctx);

    let missing_end: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XMissingEndTag)
        .collect();
    assert_eq!(
        missing_end.len(),
        2,
        "expected XMissingEndTag for span and div"
    );

    // AST should exist with force-closed nodes
    let ast = syn
        .template_ast
        .as_ref()
        .expect("template_ast should exist");
    let content = ast.root.content.as_ref().unwrap();
    assert_eq!(content.children.len(), 1, "div should be attached to root");

    // Root content end should be updated to input length
    assert_eq!(content.end, input.len() as u32);
}

// ========================================================================
// 14. Duplicate script roots
// ========================================================================

/// @ai-generated - Tests that duplicate <script> blocks emit DuplicateScript.
#[test]
fn duplicate_script_emits_diagnostic() {
    let input = "<script>a</script><script>b</script>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let dup: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::DuplicateScript)
        .collect();
    assert_eq!(
        dup.len(),
        1,
        "expected exactly 1 DuplicateScript diagnostic"
    );

    // The second script should overwrite (last-wins)
    let node = syn.script_node.as_ref().unwrap();
    let content = node.content.as_ref().unwrap();
    assert_eq!(span_str(input, content.start, content.end), "b");
}

/// @ai-generated - Tests that duplicate <script setup> blocks emit DuplicateScriptSetup.
#[test]
fn duplicate_script_setup_emits_diagnostic() {
    let input = "<script setup>a</script><script setup>b</script>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let dup: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::DuplicateScriptSetup)
        .collect();
    assert_eq!(
        dup.len(),
        1,
        "expected exactly 1 DuplicateScriptSetup diagnostic"
    );
}

// ========================================================================
// 15. Root-attribute contamination
// ========================================================================

/// @ai-generated - Tests that nested element attributes don't leak into root node attributes.
#[test]
fn nested_attrs_do_not_leak_to_root() {
    let input = "<custom-block><x a=\"1\"></x></custom-block>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert_eq!(syn.unknown_nodes.len(), 1);
    let node = &syn.unknown_nodes[0];
    // The root <custom-block> has no attributes — `a="1"` belongs to <x>.
    assert!(
        node.attributes.is_empty(),
        "root node should have no attributes, but got {:?}",
        node.attributes
    );
}

// ========================================================================
// 16. Quoted attribute span correctness
// ========================================================================

/// @ai-generated - Tests that quoted root attribute values produce correct spans.
#[test]
fn quoted_root_attr_span_correctness() {
    let input = "<script lang=\"ts\"></script>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let node = syn.script_node.as_ref().expect("script_node should exist");
    let lang = node.lang.expect("lang should be set");
    assert_eq!(lang, ScriptLanguage::TypeScript);
}

/// @ai-generated - Tests that NoValue attributes produce valid zero-width spans.
#[test]
fn no_value_attr_produces_valid_span() {
    // "setup" has no value — should not produce inverted spans.
    let input = "<script setup></script>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let node = syn
        .script_setup_node
        .as_ref()
        .expect("script_setup_node should exist");
    assert!(node.is_setup);
    // No diagnostics expected
    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );
}

// ========================================================================
// 17. template_mode root content span
// ========================================================================

/// @ai-generated - Tests that template_mode updates root content end to input length.
#[test]
fn template_mode_root_content_end_updated() {
    let input = "<div>hello</div>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(true);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn
        .template_ast
        .as_ref()
        .expect("template_ast should exist");
    let content = ast.root.content.as_ref().unwrap();
    assert_eq!(
        content.end,
        input.len() as u32,
        "root content end should equal input length"
    );
}

// ========================================================================
// 18. Happy path: no diagnostics on well-formed input
// ========================================================================

/// @ai-generated - Verifies no diagnostics are emitted for well-formed SFC input.
#[test]
fn well_formed_sfc_no_diagnostics() {
    let input = "<template><div>{{ msg }}</div></template><script setup lang=\"ts\">const msg = 'hi'</script><style scoped>.a{}</style>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "well-formed input should produce no diagnostics, got: {:?}",
        syn.diagnostics
    );
    assert!(syn.template_ast.is_some());
    assert!(syn.script_setup_node.is_some());
    assert_eq!(syn.style_nodes.len(), 1);
}

// ========================================================================
// 19. Deeply nested elements (3+ levels)
// ========================================================================

/// @ai-generated - Tests deeply nested template elements are correctly structured.
#[test]
fn deeply_nested_template_elements() {
    let input = "<template><div><span><a>link</a></span></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );
    let ast = syn
        .template_ast
        .as_ref()
        .expect("template_ast should exist");
    let root_content = ast.root.content.as_ref().unwrap();
    assert_eq!(root_content.children.len(), 1);

    // div → span → a → "link"
    let div_id = root_content.children[0];
    let AstNodeKind::Element(div) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element for div");
    };
    let span_id = div.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(span_el) = &ast.nodes[span_id.0].kind else {
        panic!("expected Element for span");
    };
    let a_id = span_el.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(a_el) = &ast.nodes[a_id.0].kind else {
        panic!("expected Element for a");
    };
    let text_id = a_el.content.as_ref().unwrap().children[0];
    let AstNodeKind::Text(text) = &ast.nodes[text_id.0].kind else {
        panic!("expected Text for link");
    };
    assert_eq!(span_str(input, text.start, text.end), "link");

    // Verify parent chain
    assert!(ast.nodes[div_id.0].parent.is_none()); // root child
    assert_eq!(ast.nodes[span_id.0].parent, Some(div_id));
    assert_eq!(ast.nodes[a_id.0].parent, Some(span_id));
    assert_eq!(ast.nodes[text_id.0].parent, Some(a_id));
}

// ========================================================================
// 20. Children flags through the full pipeline
// ========================================================================

/// @ai-generated - Tests children flags are correctly computed through the Syntax pipeline.
#[test]
fn children_flags_through_pipeline() {
    // <div> has one text child and one interpolation → text_only, has_dynamic
    let input = "<template><div>hello {{ name }}</div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(div) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };

    assert!(div
        .children_flag
        .has(crate::ast::types::ChildrenFlags::HasText));
    assert!(div
        .children_flag
        .has(crate::ast::types::ChildrenFlags::HasInterpolation));
    assert!(div.children_flag.is_text_only());
    assert!(div.children_flag.has_dynamic());
    assert!(!div.children_flag.needs_array());
}

/// @ai-generated - Tests children flags with mixed element and text children.
#[test]
fn children_flags_mixed_element_and_text() {
    let input = "<template><div>text<span></span></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(div) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };

    assert!(div
        .children_flag
        .has(crate::ast::types::ChildrenFlags::HasText));
    assert!(div
        .children_flag
        .has(crate::ast::types::ChildrenFlags::HasElement));
    assert!(!div
        .children_flag
        .has(crate::ast::types::ChildrenFlags::SingleChild));
    assert!(!div.children_flag.is_text_only());
    assert!(div.children_flag.needs_array());
}

/// @ai-generated - Tests children flags with single element child → SingleChild.
#[test]
fn children_flags_single_element_child() {
    let input = "<template><div><span></span></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(div) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };

    assert!(div
        .children_flag
        .has(crate::ast::types::ChildrenFlags::HasElement));
    assert!(div
        .children_flag
        .has(crate::ast::types::ChildrenFlags::SingleChild));
}

// ========================================================================
// 21. Self-closing elements within template
// ========================================================================

/// @ai-generated - Tests self-closing elements within template content.
#[test]
fn self_closing_element_in_template() {
    let input = "<template><img /><br /></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );
    let ast = syn.template_ast.as_ref().unwrap();
    let root_content = ast.root.content.as_ref().unwrap();
    assert_eq!(root_content.children.len(), 2);

    // Both are elements with no close tag
    for &child_id in &root_content.children {
        let AstNodeKind::Element(el) = &ast.nodes[child_id.0].kind else {
            panic!("expected Element");
        };
        assert!(el.tag_close.is_none());
        assert!(el.content.is_none()); // self-closing → no content
    }
}

// ========================================================================
// 22. Empty template
// ========================================================================

/// @ai-generated - Tests empty template produces AST with no children.
#[test]
fn empty_template() {
    let input = "<template></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(syn.diagnostics.is_empty());
    let ast = syn.template_ast.as_ref().unwrap();
    let root_content = ast.root.content.as_ref().unwrap();
    assert!(root_content.children.is_empty());
}

// ========================================================================
// 23. Dynamic directive argument
// ========================================================================

/// @ai-generated - Tests dynamic directive argument is parsed correctly.
#[test]
fn dynamic_directive_arg() {
    let input = "<template><div v-bind:[attr]=\"val\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );
    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };
    assert_eq!(el.props.len(), 1);
    let prop = &el.props[0];
    assert!(prop.is_directive);
    assert_eq!(prop.is_dynamic, Some(true));
    let arg_start = prop.arg_start.unwrap();
    let arg_end = prop.arg_end.unwrap();
    let arg_str = span_str(input, arg_start, arg_end);
    // Tokenizer may include brackets in the span; strip them for the assertion.
    let arg_name = arg_str.trim_start_matches('[').trim_end_matches(']');
    assert_eq!(arg_name, "attr");
}

// ========================================================================
// 24. Multiple attributes on template element
// ========================================================================

/// @ai-generated - Tests multiple attributes on a template element.
#[test]
fn multiple_attrs_on_element() {
    let input = "<template><div id=\"app\" class=\"main\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(syn.diagnostics.is_empty());
    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };
    assert_eq!(el.props.len(), 2);
    assert!(!el.props[0].is_directive);
    assert!(!el.props[1].is_directive);
}

// ========================================================================
// 25. template_mode with mixed content
// ========================================================================

/// @ai-generated - Tests template_mode with mixed text, elements, and interpolations.
#[test]
fn template_mode_mixed_content() {
    let input = "hello <span>world</span> {{ name }}";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(true);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );
    let ast = syn.template_ast.as_ref().unwrap();
    let root_content = ast.root.content.as_ref().unwrap();

    // Should have: text "hello ", <span>world</span>, text " ", interpolation "{{ name }}"
    // The exact count depends on tokenizer behavior, but we should have at least 3 children
    assert!(
        root_content.children.len() >= 3,
        "expected at least 3 root children, got {}",
        root_content.children.len()
    );

    // Verify there's at least one of each type
    let has_text = root_content
        .children
        .iter()
        .any(|id| matches!(ast.nodes[id.0].kind, AstNodeKind::Text(_)));
    let has_element = root_content
        .children
        .iter()
        .any(|id| matches!(ast.nodes[id.0].kind, AstNodeKind::Element(_)));
    let has_interpolation = root_content
        .children
        .iter()
        .any(|id| matches!(ast.nodes[id.0].kind, AstNodeKind::Interpolation(_)));
    assert!(has_text, "should have text node");
    assert!(has_element, "should have element node");
    assert!(has_interpolation, "should have interpolation node");
}

// ========================================================================
// 26. Sibling navigation through Syntax pipeline
// ========================================================================

/// @ai-generated - Tests sibling navigation on AST built through the Syntax pipeline.
#[test]
fn sibling_navigation_through_pipeline() {
    let input = "<template><a></a><b></b><c></c></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let root_content = ast.root.content.as_ref().unwrap();
    assert_eq!(root_content.children.len(), 3);

    let a = root_content.children[0];
    let b = root_content.children[1];
    let c = root_content.children[2];

    assert_eq!(ast.prev_sibling(a), None);
    assert_eq!(ast.next_sibling(a), Some(b));
    assert_eq!(ast.prev_sibling(b), Some(a));
    assert_eq!(ast.next_sibling(b), Some(c));
    assert_eq!(ast.prev_sibling(c), Some(b));
    assert_eq!(ast.next_sibling(c), None);
}

// ========================================================================
// 27. Style lang variants
// ========================================================================

/// @ai-generated - Tests various style lang attribute values.
#[test]
fn style_lang_variants() {
    for (lang_val, expected) in [
        ("css", StyleLang::Css),
        ("less", StyleLang::Less),
        ("sass", StyleLang::Sass),
        ("stylus", StyleLang::Stylus),
        ("xyz", StyleLang::Unknown),
    ] {
        let input = format!("<style lang=\"{}\"></style>", lang_val);
        let opts = SyntaxPluginOptions::default();
        let ctx = make_ctx(&input, &opts);
        let mut syn = Syntax::new(false);

        tokenize_and_feed(&mut syn, &input, &ctx);

        assert_eq!(syn.style_nodes.len(), 1, "failed for lang={}", lang_val);
        assert_eq!(
            syn.style_nodes[0].lang,
            Some(expected),
            "wrong lang for '{}'",
            lang_val
        );
    }
}

// ========================================================================
// 28. Script lang variants
// ========================================================================

/// @ai-generated - Tests various script lang attribute values.
#[test]
fn script_lang_variants() {
    let input = "<script lang=\"tsx\"></script>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let node = syn.script_node.as_ref().expect("script_node should exist");
    assert_eq!(node.lang, Some(ScriptLanguage::TSX));
}

// ========================================================================
// 29. Multiple unknown root nodes
// ========================================================================

/// @ai-generated - Tests multiple unknown root nodes are all collected.
#[test]
fn multiple_unknown_root_nodes() {
    let input = "<i18n>data</i18n><docs>info</docs>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert_eq!(syn.unknown_nodes.len(), 2);
    let c0 = syn.unknown_nodes[0].content.as_ref().unwrap();
    let c1 = syn.unknown_nodes[1].content.as_ref().unwrap();
    assert_eq!(span_str(input, c0.start, c0.end), "data");
    assert_eq!(span_str(input, c1.start, c1.end), "info");
}

// ========================================================================
// 30. Complete SFC with all sections
// ========================================================================

/// @ai-generated - Tests a complete SFC with template, script, script setup, style, and unknown.
#[test]
fn complete_sfc_all_sections() {
    let input = "<template><div>hi</div></template><script>export default {}</script><script setup lang=\"ts\">const x = 1</script><style scoped>.a{}</style><style module>.b{}</style><i18n>locale</i18n>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );

    // Template
    let ast = syn.template_ast.as_ref().expect("template_ast");
    assert!(!ast.root.content.as_ref().unwrap().children.is_empty());

    // Script (non-setup)
    let script = syn.script_node.as_ref().expect("script_node");
    assert!(!script.is_setup);
    let script_content = script.content.as_ref().unwrap();
    assert_eq!(
        span_str(input, script_content.start, script_content.end),
        "export default {}"
    );

    // Script setup
    let setup = syn.script_setup_node.as_ref().expect("script_setup_node");
    assert!(setup.is_setup);
    assert_eq!(setup.lang, Some(ScriptLanguage::TypeScript));

    // Styles
    assert_eq!(syn.style_nodes.len(), 2);
    assert!(syn.style_nodes[0].scoped);
    assert!(!syn.style_nodes[0].module);
    assert!(!syn.style_nodes[1].scoped);
    assert!(syn.style_nodes[1].module);
    assert!(syn.has_style_scope);
    assert!(syn.has_style_module);

    // Unknown
    assert_eq!(syn.unknown_nodes.len(), 1);
}

// ========================================================================
// 31. DFS traversal through pipeline
// ========================================================================

/// @ai-generated - Tests DFS traversal on AST built through the full pipeline.
#[test]
fn dfs_through_pipeline() {
    let input = "<template><div><span>text</span></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];

    let mut visited_kinds = Vec::new();
    ast.dfs(div_id, |_id, node| {
        visited_kinds.push(std::mem::discriminant(&node.kind));
    });

    // div → span → text
    assert_eq!(visited_kinds.len(), 3);
    assert_eq!(
        visited_kinds[0],
        std::mem::discriminant(&AstNodeKind::Element(Box::new(
            crate::ast::types::ElementNode {
                tag_open: crate::types::NodeTag {
                    start: 0,
                    end: 0,
                    name_end: 0
                },
                tag_close: None,
                props: Vec::new(),
                content: None,
                v_condition: None,
                v_for: None,
                v_slot: None,
                v_once: None,
                v_ref: None,
                tag_type: crate::ast::types::TagType::Element,
                is_self_closing: false,
                prop_flag: crate::ast::types::PropFlag::empty(),
                children_flag: crate::ast::types::ChildrenFlag::empty(),
                children_mode: crate::ast::types::ChildrenMode::Empty,
                is_fully_static: false,
            }
        )))
    );
    assert_eq!(
        visited_kinds[2],
        std::mem::discriminant(&AstNodeKind::Text(crate::ast::types::TextNode {
            start: 0,
            end: 0,
            is_entity: false,
            is_whitespace_only: false,
        }))
    );
}

// ========================================================================
// 32. Built-in directive caching: v-if
// ========================================================================

/// @ai-generated - Tests v-if directive is cached on ElementNode.v_condition through pipeline.
#[test]
fn directive_cache_v_if() {
    let input = "<template><div v-if=\"show\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );
    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };

    let cond = el
        .v_condition
        .as_ref()
        .expect("v_condition should be cached");
    assert_eq!(cond.kind, crate::ast::types::ElementNodeConditionKind::If);
    assert!(cond.prop.is_directive);
}

// ========================================================================
// 33. Built-in directive caching: v-else-if
// ========================================================================

/// @ai-generated - Tests v-else-if directive is cached on ElementNode.v_condition through pipeline.
#[test]
fn directive_cache_v_else_if() {
    // v-else-if on a standalone element (no adjacent v-if — that's a separate validation concern)
    let input = "<template><div v-else-if=\"x\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };

    let cond = el
        .v_condition
        .as_ref()
        .expect("v_condition should be cached");
    assert_eq!(
        cond.kind,
        crate::ast::types::ElementNodeConditionKind::ElseIf
    );
}

// ========================================================================
// 34. Built-in directive caching: v-else
// ========================================================================

/// @ai-generated - Tests v-else directive is cached on ElementNode.v_condition through pipeline.
#[test]
fn directive_cache_v_else() {
    let input = "<template><div v-else></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };

    let cond = el
        .v_condition
        .as_ref()
        .expect("v_condition should be cached");
    assert_eq!(cond.kind, crate::ast::types::ElementNodeConditionKind::Else);
}

// ========================================================================
// 35. Built-in directive caching: v-for
// ========================================================================

/// @ai-generated - Tests v-for directive is cached on ElementNode.v_for through pipeline.
#[test]
fn directive_cache_v_for() {
    let input = "<template><div v-for=\"item in items\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );
    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };

    let vfor = el.v_for.as_ref().expect("v_for should be cached");
    assert!(vfor.is_directive);
    assert_eq!(span_str(input, vfor.start, vfor.name_end), "v-for");
}

// ========================================================================
// 36. Built-in directive caching: v-slot (longhand)
// ========================================================================

/// @ai-generated - Tests v-slot directive is cached on ElementNode.v_slot through pipeline.
#[test]
fn directive_cache_v_slot() {
    let input = "<template><Comp v-slot:default=\"{ item }\"></Comp></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );
    let ast = syn.template_ast.as_ref().unwrap();
    let comp_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[comp_id.0].kind else {
        panic!("expected Element");
    };

    let vslot = el.v_slot.as_ref().expect("v_slot should be cached");
    assert!(vslot.is_directive);
}

// ========================================================================
// 37. Built-in directive caching: v-slot shorthand (#)
// ========================================================================

/// @ai-generated - Tests # shorthand for v-slot is cached on ElementNode.v_slot through pipeline.
#[test]
fn directive_cache_v_slot_shorthand() {
    let input = "<template><Comp #default=\"{ item }\"></Comp></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );
    let ast = syn.template_ast.as_ref().unwrap();
    let comp_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[comp_id.0].kind else {
        panic!("expected Element");
    };

    let vslot = el
        .v_slot
        .as_ref()
        .expect("v_slot should be cached for # shorthand");
    assert!(vslot.is_directive);
}

// ========================================================================
// 38. Built-in directive caching: v-once
// ========================================================================

/// @ai-generated - Tests v-once directive is cached on ElementNode.v_once through pipeline.
#[test]
fn directive_cache_v_once() {
    let input = "<template><div v-once></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );
    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };

    assert!(el.v_once.is_some(), "v_once should be set");
}

// ========================================================================
// 39. Duplicate v-if emits warning, first wins
// ========================================================================

/// @ai-generated - Tests duplicate v-if on same element emits warning and first wins.
#[test]
fn duplicate_v_if_emits_warning() {
    let input = "<template><div v-if=\"a\" v-if=\"b\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    // Should have exactly 1 warning for duplicate
    let dup_warnings: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XDuplicateDirective)
        .collect();
    assert_eq!(
        dup_warnings.len(),
        1,
        "expected 1 duplicate directive warning, got {:?}",
        syn.diagnostics
    );
    assert_eq!(
        dup_warnings[0].severity,
        crate::diagnostics::DiagnosticSeverity::Warning
    );

    // First occurrence wins
    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };
    let cond = el
        .v_condition
        .as_ref()
        .expect("v_condition should be cached");
    assert_eq!(cond.kind, crate::ast::types::ElementNodeConditionKind::If);
}

// ========================================================================
// 40. Duplicate v-for emits warning, first wins
// ========================================================================

/// @ai-generated - Tests duplicate v-for on same element emits warning and first wins.
#[test]
fn duplicate_v_for_emits_warning() {
    let input = "<template><div v-for=\"a in b\" v-for=\"c in d\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let dup_warnings: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XDuplicateDirective)
        .collect();
    assert_eq!(
        dup_warnings.len(),
        1,
        "expected 1 duplicate directive warning"
    );
    assert_eq!(
        dup_warnings[0].severity,
        crate::diagnostics::DiagnosticSeverity::Warning
    );
}

// ========================================================================
// 41. Duplicate v-slot emits warning, first wins
// ========================================================================

/// @ai-generated - Tests duplicate v-slot on same element emits warning and first wins.
#[test]
fn duplicate_v_slot_emits_warning() {
    let input = "<template><Comp v-slot:a=\"x\" v-slot:b=\"y\"></Comp></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let dup_warnings: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XDuplicateDirective)
        .collect();
    assert_eq!(
        dup_warnings.len(),
        1,
        "expected 1 duplicate directive warning"
    );
}

// ========================================================================
// 42. Duplicate v-once emits warning
// ========================================================================

/// @ai-generated - Tests duplicate v-once on same element emits warning.
#[test]
fn duplicate_v_once_emits_warning() {
    let input = "<template><div v-once v-once></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let dup_warnings: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XDuplicateDirective)
        .collect();
    assert_eq!(
        dup_warnings.len(),
        1,
        "expected 1 duplicate directive warning"
    );
    assert!(syn.template_ast.as_ref().unwrap().nodes.iter().any(|n| {
        if let AstNodeKind::Element(el) = &n.kind {
            el.v_once.is_some()
        } else {
            false
        }
    }));
}

// ========================================================================
// Duplicate static-attribute detection (span-backed)
// ========================================================================

/// Duplicate static HTML attributes emit exactly one `DuplicateAttribute`
/// error, anchored on the SECOND occurrence's name span.
#[test]
fn duplicate_static_attr_emits_error_on_second_occurrence() {
    let input = "<template><div id=\"a\" id=\"b\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let dup: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::DuplicateAttribute)
        .collect();
    assert_eq!(
        dup.len(),
        1,
        "expected exactly 1 duplicate-attribute error, got {:?}",
        syn.diagnostics
    );
    assert_eq!(
        dup[0].severity,
        crate::diagnostics::DiagnosticSeverity::Error
    );

    // The error span must anchor on the SECOND `id` name (start..name_end).
    let first_id = input.find("id=").expect("first id");
    let second_id = first_id + 3 + input[first_id + 3..].find("id=").expect("second id");
    let span = dup[0].span.expect("duplicate diagnostic carries a span");
    assert_eq!(
        (span.start, span.end),
        (second_id as u32, (second_id + 2) as u32),
        "span must cover the second `id` name"
    );
    assert_eq!(span_str(input, span.start, span.end), "id");
}

/// The duplicate check compares raw attribute-name source bytes, so it is
/// case-sensitive: `id` and `ID` are distinct names and are NOT flagged as a
/// duplicate. This pins the byte-equality comparison against any future drift
/// toward case-insensitive (ASCII-folded) matching.
#[test]
fn duplicate_attr_check_is_case_sensitive() {
    let input = "<template><div id=\"a\" ID=\"b\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics
            .iter()
            .all(|d| d.code != CompilerErrorCode::DuplicateAttribute),
        "case-variant names (`id` vs `ID`) differ by byte and must NOT be \
         flagged as duplicate, got {:?}",
        syn.diagnostics
    );
}

/// Namespaced (colon-bearing) static attribute names are compared literally by
/// their full source bytes: two byte-identical `xlink:href` names ARE a
/// duplicate, while a colon does not turn the attribute into a directive that
/// would be exempt from the check.
#[test]
fn duplicate_namespaced_attr_is_flagged_literally() {
    let input = "<template><a xlink:href=\"x\" xlink:href=\"y\"></a></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let dup: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::DuplicateAttribute)
        .collect();
    assert_eq!(
        dup.len(),
        1,
        "byte-identical namespaced names must be flagged exactly once, got {:?}",
        syn.diagnostics
    );
    // Anchored on the SECOND `xlink:href` name.
    let first = input.find("xlink:href").expect("first xlink:href");
    let second = first + 10 + input[first + 10..].find("xlink:href").expect("second");
    let span = dup[0].span.expect("duplicate diagnostic carries a span");
    assert_eq!(
        (span.start, span.end),
        (second as u32, (second + 10) as u32),
        "span must cover the second `xlink:href` name"
    );
    assert_eq!(span_str(input, span.start, span.end), "xlink:href");
}

/// A non-duplicate attribute list emits zero `DuplicateAttribute` diagnostics
/// AND parses to the exact expected prop set. Asserting the parsed names,
/// values, and spans (not just the absence of the diagnostic) means any
/// AST/prop drift in the duplicate-detection path would fail this test, not
/// only duplicate-diagnostic drift.
#[test]
fn distinct_static_attrs_emit_no_duplicate_error() {
    let input = "<template><div id=\"a\" class=\"b\" data-x=\"c\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics
            .iter()
            .all(|d| d.code != CompilerErrorCode::DuplicateAttribute),
        "expected no duplicate-attribute diagnostics, got {:?}",
        syn.diagnostics
    );

    // The parse RESULT must be byte-identical to the expected prop set: each
    // static attribute is preserved with its name span and quoted value span.
    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element")
    };
    assert_eq!(el.props.len(), 3, "all three attributes must be retained");
    let observed: Vec<(&str, &str)> = el
        .props
        .iter()
        .map(|p| {
            assert!(!p.is_directive, "static attrs must not be directives");
            let vs = p.value_start.expect("value_start set for quoted attr");
            let ve = p.value_end.expect("value_end set for quoted attr");
            (
                span_str(input, p.start, p.name_end),
                span_str(input, vs, ve),
            )
        })
        .collect();
    assert_eq!(observed, vec![("id", "a"), ("class", "b"), ("data-x", "c")]);

    // Spans index the real source positions, not synthesized offsets. Pin the
    // EXACT (start, name_end, value_start, value_end) tuple for every prop so
    // any drift in any single span field fails this test.
    // <template><div id="a" class="b" data-x="c"></div></template>
    //           1111111111222222222233333333334444
    // 0123456789012345678901234567890123456789012345
    let span_tuple = |p: &NodeProp| {
        (
            p.start,
            p.name_end,
            p.value_start.expect("value_start set for quoted attr"),
            p.value_end.expect("value_end set for quoted attr"),
        )
    };
    assert_eq!(span_tuple(&el.props[0]), (15, 17, 19, 20), "id span");
    assert_eq!(span_tuple(&el.props[1]), (22, 27, 29, 30), "class span");
    assert_eq!(span_tuple(&el.props[2]), (32, 38, 40, 41), "data-x span");
}

/// Duplicate directives are NOT reported as duplicate attributes — Vue allows
/// e.g. multiple `@click` handlers, so the static-attr check must skip them.
#[test]
fn duplicate_directive_is_not_a_duplicate_attribute() {
    let input = "<template><div @click=\"a\" @click=\"b\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics
            .iter()
            .all(|d| d.code != CompilerErrorCode::DuplicateAttribute),
        "directives must not trigger DuplicateAttribute, got {:?}",
        syn.diagnostics
    );
}

/// Seen-name tracking is reset per element: the same attribute name on two
/// SIBLING elements is not a duplicate.
#[test]
fn same_attr_on_sibling_elements_is_not_duplicate() {
    let input = "<template><div id=\"a\"></div><span id=\"b\"></span></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics
            .iter()
            .all(|d| d.code != CompilerErrorCode::DuplicateAttribute),
        "sibling elements sharing an attr name must not be duplicates, got {:?}",
        syn.diagnostics
    );
}

/// Allocation invariant: seen attribute names are tracked as source-backed
/// spans (byte offsets into the source), never owned byte copies. Each
/// recorded span must locate its name within the source buffer, and duplicate
/// detection compares those borrowed source-byte ranges directly.
#[test]
fn seen_attr_tracking_is_span_backed() {
    let input = "<template><div id=\"a\" data-x=\"b\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    // Feed everything up to (but not past) the <div>'s open-tag end so the
    // per-element span buffer still reflects the div's attributes (the first
    // `OpenTagEnd` belongs to <template>, the second to <div>).
    let events = tokenize_events(input);
    let mut open_tag_ends = 0;
    for event in &events {
        syn.handle(event, &ctx);
        if matches!(event, TokenizerEvent::OpenTagEnd { .. }) {
            open_tag_ends += 1;
            if open_tag_ends == 2 {
                break;
            }
        }
    }

    assert_eq!(
        syn.seen_attr_spans.len(),
        2,
        "both static attribute names should be recorded as spans"
    );
    // Spans index into the source buffer and reproduce the original names.
    let names: Vec<&str> = syn
        .seen_attr_spans
        .iter()
        .map(|s| {
            assert!(
                (s.end as usize) <= input.len() && s.start < s.end,
                "span must be a valid source range: {s:?}"
            );
            span_str(input, s.start, s.end)
        })
        .collect();
    assert_eq!(names, vec!["id", "data-x"]);
    // The recorded spans point at the actual source positions of the names.
    assert_eq!(
        syn.seen_attr_spans[0].start as usize,
        input.find("id=").expect("id position")
    );
}

// ========================================================================
// 43. Non-cached directives don't populate cache fields
// ========================================================================

/// @ai-generated - Tests that non-cached directives (v-show, v-bind, etc.) don't set cache fields.
#[test]
fn non_cached_directives_leave_fields_none() {
    let input = "<template><div v-show=\"x\" v-bind:id=\"y\" @click=\"z\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );
    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };

    assert!(el.v_condition.is_none());
    assert!(el.v_for.is_none());
    assert!(el.v_slot.is_none());
    assert!(el.v_once.is_none());
    // But props should still contain all 3 directives
    assert_eq!(el.props.len(), 3);
}

// ========================================================================
// 44. Cached directives are moved out of props
// ========================================================================

/// @ai-generated - Tests that cached directives are moved into cache fields and not in props.
#[test]
fn cached_directives_not_in_props() {
    let input = "<template><div v-if=\"a\" v-for=\"b in c\" v-once></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );
    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };

    // Cached directives are NOT in props — they are moved into cache fields
    assert_eq!(el.props.len(), 0);
    // Cache fields should be populated
    assert!(el.v_condition.is_some());
    assert!(el.v_for.is_some());
    assert!(el.v_once.is_some());
}

// ========================================================================
// 45. Children flags auto-derive HasVIf/HasVFor from cached fields
// ========================================================================

/// @ai-generated - Tests that children flags automatically derive HasVIf/HasVFor
/// from cached directive fields set through the pipeline (no manual mutation).
#[test]
fn children_flags_auto_derive_from_cached_directives() {
    let input =
        "<template><div><span v-if=\"x\"></span><p v-for=\"i in list\"></p></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(
        syn.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        syn.diagnostics
    );
    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(div) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element");
    };

    assert!(
        div.children_flag
            .has(crate::ast::types::ChildrenFlags::HasVIf),
        "parent should have HasVIf from child's cached v_condition"
    );
    assert!(
        div.children_flag
            .has(crate::ast::types::ChildrenFlags::HasVFor),
        "parent should have HasVFor from child's cached v_for"
    );
}

// ========================================================================
// 46. PropFlags: :key sets HasDynamicKey
// ========================================================================

/// @ai-generated - Tests :key binding sets PropFlags::HasDynamicKey.
#[test]
fn prop_flag_dynamic_key() {
    let input = "<template><div :key=\"id\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element")
    };
    assert!(el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasDynamicKey));
}

// ========================================================================
// 47. PropFlags: :class sets HasDynamicClass
// ========================================================================

/// @ai-generated - Tests :class binding sets PropFlags::HasDynamicClass.
#[test]
fn prop_flag_dynamic_class() {
    let input = "<template><div :class=\"cls\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element")
    };
    assert!(el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasDynamicClass));
}

// ========================================================================
// 48. PropFlags: :style sets HasDynamicStyle
// ========================================================================

/// @ai-generated - Tests :style binding sets PropFlags::HasDynamicStyle.
#[test]
fn prop_flag_dynamic_style() {
    let input = "<template><div :style=\"s\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element")
    };
    assert!(el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasDynamicStyle));
}

// ========================================================================
// 49. PropFlags: ref attribute sets HasRef
// ========================================================================

/// @ai-generated - Tests ref attribute sets PropFlags::HasRef.
#[test]
fn prop_flag_ref() {
    let input = "<template><div ref=\"el\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element")
    };
    assert!(el.prop_flag.has(crate::ast::types::PropFlags::HasRef));
}

// ========================================================================
// 50. PropFlags: @click sets HasEventListener
// ========================================================================

/// @ai-generated - Tests @click sets PropFlags::HasEventListener.
#[test]
fn prop_flag_event_listener() {
    let input = "<template><div @click=\"handler\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element")
    };
    assert!(el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasEventListener));
}

// ========================================================================
// 51. PropFlags: custom directive sets HasCustomDirective
// ========================================================================

/// @ai-generated - Tests custom directive (v-focus) sets PropFlags::HasCustomDirective.
#[test]
fn prop_flag_custom_directive() {
    let input = "<template><div v-focus></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element")
    };
    assert!(el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasCustomDirective));
}

// ========================================================================
// 52. PropFlags: built-in v-show does NOT set HasCustomDirective
// ========================================================================

/// @ai-generated - Tests v-show does not set HasCustomDirective.
#[test]
fn prop_flag_v_show_not_custom() {
    let input = "<template><div v-show=\"x\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element")
    };
    assert!(!el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasCustomDirective));
}

// ========================================================================
// 53. PropFlags: element with no directives has empty prop_flag
// ========================================================================

/// @ai-generated - Tests element with only non-class/style static attrs has empty prop_flag.
#[test]
fn prop_flag_empty_for_static_attrs() {
    let input = "<template><div id=\"app\" title=\"hello\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element")
    };
    assert!(el.prop_flag.is_empty());
}

// ========================================================================
// 54. ChildrenFlags: HasChildWithVSlot from child with v-slot
// ========================================================================

/// @ai-generated - Tests parent gets HasChildWithVSlot when child has v-slot.
#[test]
fn children_flag_has_child_with_v_slot() {
    let input = "<template><Comp><template v-slot:default>hi</template></Comp></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let comp_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(comp) = &ast.nodes[comp_id.0].kind else {
        panic!("expected Element")
    };
    assert!(comp
        .children_flag
        .has(crate::ast::types::ChildrenFlags::HasChildWithVSlot));
}

// ========================================================================
// 55. ChildrenFlags: HasChildWithKey from child with :key
// ========================================================================

/// @ai-generated - Tests parent gets HasChildWithKey when child has :key.
#[test]
fn children_flag_has_child_with_key() {
    let input = "<template><div><span :key=\"id\"></span></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(div) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element")
    };
    assert!(div
        .children_flag
        .has(crate::ast::types::ChildrenFlags::HasChildWithKey));
}

// ========================================================================
// 56. v-else adjacency: valid v-if → v-else
// ========================================================================

/// @ai-generated - Tests valid v-if → v-else adjacency produces no diagnostic.
#[test]
fn v_else_valid_adjacent_v_if() {
    let input = "<template><div v-if=\"a\"></div><div v-else></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let else_errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XVElseNoAdjacentIf)
        .collect();
    assert!(
        else_errors.is_empty(),
        "valid v-if → v-else should not emit XVElseNoAdjacentIf, got: {:?}",
        syn.diagnostics
    );
}

// ========================================================================
// 57. v-else adjacency: valid with comment between
// ========================================================================

/// @ai-generated - Tests v-if → comment → v-else is valid (comments skipped).
#[test]
fn v_else_valid_with_comment_between() {
    let input = "<template><div v-if=\"a\"></div><!-- comment --><div v-else></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let else_errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XVElseNoAdjacentIf)
        .collect();
    assert!(
        else_errors.is_empty(),
        "comment between v-if and v-else should be valid"
    );
}

// ========================================================================
// 58. v-else adjacency: valid with whitespace between
// ========================================================================

/// @ai-generated - Tests v-if → whitespace → v-else-if is valid (whitespace skipped).
#[test]
fn v_else_if_valid_with_whitespace_between() {
    let input = "<template><div v-if=\"a\"></div>   <div v-else-if=\"b\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let else_errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XVElseNoAdjacentIf)
        .collect();
    assert!(
        else_errors.is_empty(),
        "whitespace between v-if and v-else-if should be valid"
    );
}

// ========================================================================
// 59. v-else adjacency: valid chain v-if → v-else-if → v-else
// ========================================================================

/// @ai-generated - Tests full v-if chain is valid.
#[test]
fn v_else_valid_full_chain() {
    let input =
        "<template><div v-if=\"a\"></div><div v-else-if=\"b\"></div><div v-else></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let else_errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XVElseNoAdjacentIf)
        .collect();
    assert!(else_errors.is_empty(), "full v-if chain should be valid");
}

// ========================================================================
// 60. v-else adjacency: invalid — v-else alone
// ========================================================================

/// @ai-generated - Tests v-else without preceding v-if emits XVElseNoAdjacentIf.
#[test]
fn v_else_invalid_alone() {
    let input = "<template><div v-else></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let else_errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XVElseNoAdjacentIf)
        .collect();
    assert_eq!(
        else_errors.len(),
        1,
        "v-else alone should emit XVElseNoAdjacentIf"
    );
}

// ========================================================================
// 61. v-else adjacency: invalid — after non-v-if element
// ========================================================================

/// @ai-generated - Tests v-else after plain element emits XVElseNoAdjacentIf.
#[test]
fn v_else_invalid_after_plain_element() {
    let input = "<template><span></span><div v-else></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let else_errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XVElseNoAdjacentIf)
        .collect();
    assert_eq!(
        else_errors.len(),
        1,
        "v-else after plain element should emit XVElseNoAdjacentIf"
    );
}

// ========================================================================
// 62. v-else adjacency: invalid — after v-for element
// ========================================================================

/// @ai-generated - Tests v-else after v-for element emits XVElseNoAdjacentIf.
#[test]
fn v_else_invalid_after_v_for() {
    let input = "<template><div v-for=\"x in y\"></div><div v-else></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let else_errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XVElseNoAdjacentIf)
        .collect();
    assert_eq!(
        else_errors.len(),
        1,
        "v-else after v-for should emit XVElseNoAdjacentIf"
    );
}

// ========================================================================
// 63. v-else adjacency: invalid — v-else after v-else (not after v-if/v-else-if)
// ========================================================================

/// @ai-generated - Tests v-else after v-else emits XVElseNoAdjacentIf.
/// v-else is a terminator — another v-else cannot follow it.
#[test]
fn v_else_invalid_after_v_else() {
    let input = "<template><div v-if=\"a\"></div><div v-else></div><div v-else></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let else_errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XVElseNoAdjacentIf)
        .collect();
    assert_eq!(
        else_errors.len(),
        1,
        "v-else after v-else should emit XVElseNoAdjacentIf, got: {:?}",
        syn.diagnostics
    );
}

// ========================================================================
// 64. v-if alone does NOT emit adjacency error
// ========================================================================

/// @ai-generated - Tests v-if alone does not emit XVElseNoAdjacentIf.
#[test]
fn v_if_alone_no_adjacency_error() {
    let input = "<template><div v-if=\"a\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let else_errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::XVElseNoAdjacentIf)
        .collect();
    assert!(
        else_errors.is_empty(),
        "v-if alone should not emit adjacency error"
    );
}

// ========================================================================
// 64. TagType: HTML element → TagType::Element
// ========================================================================

/// @ai-generated - Tests known HTML tag gets TagType::Element.
#[test]
fn tag_type_html_element() {
    let input = "<template><div></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert_eq!(el.tag_type, crate::ast::types::TagType::Element);
}

// ========================================================================
// 65. TagType: PascalCase → TagType::Component
// ========================================================================

/// @ai-generated - Tests PascalCase tag gets TagType::Component.
#[test]
fn tag_type_pascal_case_component() {
    let input = "<template><MyComp></MyComp></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert_eq!(el.tag_type, crate::ast::types::TagType::Component);
}

// ========================================================================
// 66. TagType: dash-case → TagType::Component
// ========================================================================

/// @ai-generated - Tests dash-case tag gets TagType::Component.
#[test]
fn tag_type_dash_case_component() {
    let input = "<template><my-comp></my-comp></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert_eq!(el.tag_type, crate::ast::types::TagType::Component);
}

// ========================================================================
// 67. TagType: unknown lowercase tag → TagType::Component
// ========================================================================

/// @ai-generated - Tests unknown lowercase tag gets TagType::Component.
#[test]
fn tag_type_unknown_lowercase_component() {
    let input = "<template><foobar></foobar></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert_eq!(el.tag_type, crate::ast::types::TagType::Component);
}

// ========================================================================
// 68. TagType: <slot> → TagType::SlotOutlet
// ========================================================================

/// @ai-generated - Tests <slot> gets TagType::SlotOutlet.
#[test]
fn tag_type_slot_outlet() {
    let input = "<template><slot></slot></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert_eq!(el.tag_type, crate::ast::types::TagType::SlotOutlet);
}

// ========================================================================
// 69. TagType: <template> inside content → TagType::Template
// ========================================================================

/// @ai-generated - Tests <template> inside content gets TagType::Template.
#[test]
fn tag_type_template_wrapper() {
    let input = "<template><div><template v-if=\"x\">hi</template></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(div) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element")
    };
    let tmpl_id = div.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(tmpl) = &ast.nodes[tmpl_id.0].kind else {
        panic!("expected Element")
    };
    assert_eq!(tmpl.tag_type, crate::ast::types::TagType::Template);
}

// ========================================================================
// 70. TagType: SVG tags → TagType::Element
// ========================================================================

/// @ai-generated - Tests SVG tag gets TagType::Element (not Component).
#[test]
fn tag_type_svg_element() {
    let input = "<template><svg></svg></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert_eq!(el.tag_type, crate::ast::types::TagType::Element);
}

// ========================================================================
// 71. is_self_closing: self-closing tag
// ========================================================================

/// @ai-generated - Tests self-closing tag sets is_self_closing = true.
#[test]
fn is_self_closing_true() {
    let input = "<template><br /></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert!(el.is_self_closing);
}

// ========================================================================
// 72. is_self_closing: normal close tag
// ========================================================================

/// @ai-generated - Tests normal close tag has is_self_closing = false.
#[test]
fn is_self_closing_false() {
    let input = "<template><div></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert!(!el.is_self_closing);
}

// ========================================================================
// 73. PropFlags: v-model sets HasModel
// ========================================================================

/// @ai-generated - Tests v-model sets PropFlags::HasModel.
#[test]
fn prop_flag_has_model() {
    let input = "<template><input v-model=\"val\" /></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert!(el.prop_flag.has(crate::ast::types::PropFlags::HasModel));
}

// ========================================================================
// 74. PropFlags: v-show sets HasShow
// ========================================================================

/// @ai-generated - Tests v-show sets PropFlags::HasShow.
#[test]
fn prop_flag_has_show() {
    let input = "<template><div v-show=\"x\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert!(el.prop_flag.has(crate::ast::types::PropFlags::HasShow));
}

// ========================================================================
// 75. PropFlags: v-html sets HasVHtml
// ========================================================================

/// @ai-generated - Tests v-html sets PropFlags::HasVHtml.
#[test]
fn prop_flag_has_v_html() {
    let input = "<template><div v-html=\"content\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert!(el.prop_flag.has(crate::ast::types::PropFlags::HasVHtml));
}

// ========================================================================
// 76. PropFlags: v-text sets HasVText
// ========================================================================

/// @ai-generated - Tests v-text sets PropFlags::HasVText.
#[test]
fn prop_flag_has_v_text() {
    let input = "<template><div v-text=\"msg\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert!(el.prop_flag.has(crate::ast::types::PropFlags::HasVText));
}

// ========================================================================
// 77. PropFlags: static class attribute sets HasStaticClass
// ========================================================================

/// @ai-generated - Tests static class attribute sets PropFlags::HasStaticClass.
#[test]
fn prop_flag_has_static_class() {
    let input = "<template><div class=\"foo\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert!(el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasStaticClass));
}

// ========================================================================
// 78. PropFlags: static style attribute sets HasStaticStyle
// ========================================================================

/// @ai-generated - Tests static style attribute sets PropFlags::HasStaticStyle.
#[test]
fn prop_flag_has_static_style() {
    let input = "<template><div style=\"color:red\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert!(el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasStaticStyle));
}

// ========================================================================
// 79. PropFlags: v-bind spread (no arg) sets HasBindSpread
// ========================================================================

/// @ai-generated - Tests v-bind="obj" (no arg) sets PropFlags::HasBindSpread.
#[test]
fn prop_flag_has_bind_spread() {
    let input = "<template><div v-bind=\"obj\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert!(el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasBindSpread));
}

// ========================================================================
// 80. PropFlags: v-on spread (no arg) sets HasOnSpread
// ========================================================================

/// @ai-generated - Tests v-on="handlers" (no arg) sets PropFlags::HasOnSpread.
#[test]
fn prop_flag_has_on_spread() {
    let input = "<template><div v-on=\"handlers\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert!(el.prop_flag.has(crate::ast::types::PropFlags::HasOnSpread));
}

// ========================================================================
// 81. PropFlags: merge_class derivable (static + dynamic class)
// ========================================================================

/// @ai-generated - Tests both static and dynamic class set their respective flags.
#[test]
fn prop_flag_merge_class() {
    let input = "<template><div class=\"a\" :class=\"b\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert!(el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasStaticClass));
    assert!(el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasDynamicClass));
}

// ========================================================================
// 82. PropFlags: merge_style derivable (static + dynamic style)
// ========================================================================

/// @ai-generated - Tests both static and dynamic style set their respective flags.
#[test]
fn prop_flag_merge_style() {
    let input = "<template><div style=\"color:red\" :style=\"s\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
        panic!("expected Element")
    };
    assert!(el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasStaticStyle));
    assert!(el
        .prop_flag
        .has(crate::ast::types::PropFlags::HasDynamicStyle));
}

// ========================================================================
// 83. Attribute value_end tracking on template element props
// ========================================================================

/// @ai-generated - Tests that template element attribute values have correct value_end spans.
#[test]
fn template_element_attr_value_end() {
    let input = "<template><div id=\"app\" :class=\"cls\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element")
    };

    // First prop: id="app"
    let id_prop = &el.props[0];
    let vs = id_prop.value_start.expect("value_start should be set");
    let ve = id_prop
        .value_end
        .expect("value_end should be set for quoted attr");
    assert_eq!(span_str(input, vs, ve), "app");

    // Second prop: :class="cls" (directive with arg)
    let class_prop = &el.props[1];
    let vs = class_prop.value_start.expect("value_start should be set");
    let ve = class_prop
        .value_end
        .expect("value_end should be set for quoted directive");
    assert_eq!(span_str(input, vs, ve), "cls");
}

/// @ai-generated - Tests that no-value attributes have value_end = None.
#[test]
fn template_element_attr_no_value_end() {
    let input = "<template><div v-once></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element")
    };

    // v-once is cached, not in props — but let's test a regular no-value attr
    // Actually v-once has no value. Let's use the cached prop directly.
    let v_once = el.v_once.as_ref().expect("v_once should be cached");
    assert!(v_once.value_start.is_none());
    assert!(v_once.value_end.is_none());
}

// ========================================================================
// 74. Element with both v-if and v-for cached + parent flags
// ========================================================================

/// @ai-generated - Tests element with both v-if and v-for: both cached, parent flags reflect both.
#[test]
fn element_with_v_if_and_v_for_both_cached() {
    let input = r#"<template><div><span v-if="ok" v-for="i in list"></span></div></template>"#;
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast.as_ref().unwrap();
    let div_id = ast.root.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(div_el) = &ast.nodes[div_id.0].kind else {
        panic!("expected Element for div");
    };

    // Child span should have both v-if and v-for cached
    let span_id = div_el.content.as_ref().unwrap().children[0];
    let AstNodeKind::Element(span_el) = &ast.nodes[span_id.0].kind else {
        panic!("expected Element for span");
    };

    assert!(
        span_el.v_condition.is_some(),
        "v-if should be cached on span"
    );
    assert!(span_el.v_for.is_some(), "v-for should be cached on span");

    // Parent div's children_flag should reflect both
    assert!(div_el
        .children_flag
        .has(crate::ast::types::ChildrenFlags::HasVIf));
    assert!(div_el
        .children_flag
        .has(crate::ast::types::ChildrenFlags::HasVFor));
}

// ========================================================================
// 75. Public getters on Syntax
// ========================================================================

/// @ai-generated - Tests Syntax public getters return correct results.
#[test]
fn syntax_public_getters() {
    let input = r#"<script setup lang="ts">const x = 1;</script>
<template><div>hello</div></template>
<style scoped lang="scss">.foo{}</style>
<custom-block>data</custom-block>"#;

    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_and_feed(&mut syn, input, &ctx);

    assert!(syn.script().is_none(), "no plain script block");
    assert!(syn.script_setup().is_some(), "script setup should exist");
    assert_eq!(syn.style_nodes().len(), 1);
    assert!(syn.style_nodes()[0].scoped);
    assert!(syn.has_style_scope());
    assert!(!syn.has_style_module());
    assert!(!syn.is_vapor());
    assert!(syn.template_ast().is_some());
    assert_eq!(syn.unknown_nodes().len(), 1);
}

// @ai-generated - Tests that `ref` attribute is cached in v_ref and NOT in props
#[test]
fn ref_attribute_cached_in_v_ref() {
    let input = r#"<div ref="myRef" class="foo"></div>"#;
    let options = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &options);
    let mut syn = Syntax::new(true); // template_mode
    tokenize_and_feed(&mut syn, input, &ctx);

    let ast = syn.template_ast().expect("should have template AST");
    let root_children = ast.root.content.as_ref().unwrap();
    assert_eq!(root_children.children.len(), 1);

    let node = &ast.nodes[root_children.children[0].0];
    let AstNodeKind::Element(el) = &node.kind else {
        panic!("expected Element");
    };

    // v_ref should be populated
    assert!(el.v_ref.is_some(), "v_ref should be cached on the element");
    let ref_prop = el.v_ref.as_ref().unwrap();
    let ref_value = span_str(
        input,
        ref_prop.value_start.unwrap(),
        ref_prop.value_end.unwrap(),
    );
    assert_eq!(ref_value, "myRef");

    // ref should NOT be in element.props (it was taken out)
    for prop in &el.props {
        let prop_name = span_str(input, prop.start, prop.name_end);
        assert_ne!(prop_name, "ref", "ref should not be in element.props");
    }

    // class should still be in element.props
    assert!(
        el.props.iter().any(|p| {
            let n = span_str(input, p.start, p.name_end);
            n == "class"
        }),
        "class should remain in element.props"
    );

    // HasRef flag should still be set
    assert!(
        el.prop_flag.has(crate::ast::types::PropFlags::HasRef),
        "HasRef prop flag should still be set"
    );
}

// ========================================================================
// Void elements (img, br, input, etc.)
// ========================================================================

#[test]
fn void_element_img_no_close_tag() {
    // <img> without closing tag or self-closing slash — should parse without errors
    let input =
        "<template><div><img src=\"test.png\" alt=\"test\"><span>text</span></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::diagnostics::DiagnosticSeverity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Void <img> should not cause errors: {:?}",
        errors
    );

    let ast = syn
        .template_ast
        .as_ref()
        .expect("template_ast should exist");
    // The div should have 2 children: img and span
    let root_children = ast.root.content.as_ref().unwrap().children.as_slice();
    assert_eq!(root_children.len(), 1, "root should have 1 child (div)");
    let div = &ast.nodes[root_children[0].0];
    if let AstNodeKind::Element(el) = &div.kind {
        let content = el.content.as_ref().expect("div should have content");
        assert_eq!(
            content.children.len(),
            2,
            "div should have 2 children (img + span)"
        );
    } else {
        panic!("Expected element node for div");
    }
}

#[test]
fn void_element_br_and_hr() {
    let input = "<template><p>text<br>more<hr>end</p></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::diagnostics::DiagnosticSeverity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Void <br> and <hr> should not cause errors: {:?}",
        errors
    );
}

#[test]
fn void_element_input_with_attrs() {
    let input = "<template><form><input type=\"text\" v-model=\"name\"><button>Submit</button></form></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::diagnostics::DiagnosticSeverity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Void <input> should not cause errors: {:?}",
        errors
    );
}

#[test]
fn void_element_explicit_close_tag_tolerated() {
    // Some codebases write </img> — we should tolerate this
    let input = "<template><div><img src=\"test.png\"></img></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::diagnostics::DiagnosticSeverity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Explicit </img> should be tolerated: {:?}",
        errors
    );
}

#[test]
fn void_element_self_closing_still_works() {
    // Self-closing syntax should still work as before
    let input = "<template><img src=\"test.png\" /></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_and_feed(&mut syn, input, &ctx);

    let errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::diagnostics::DiagnosticSeverity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Self-closing <img /> should still work: {:?}",
        errors
    );
}

// ========================================================================
// SFC mode: custom block RCDATA
// ========================================================================

#[test]
fn sfc_mode_custom_block_html_like_content_no_errors() {
    // <docs> block containing `Array<string>` should not produce parse errors
    // because the tokenizer enters RCDATA mode for custom blocks in SFC mode.
    let input = "<docs>\n## Title\n\nDefault to `@`, `Array<string>` also supported.\n</docs>\n<template><div>hi</div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    let errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::diagnostics::DiagnosticSeverity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Custom block with HTML-like content should not produce errors: {:?}",
        errors
    );

    // Custom block content should be captured as raw text
    assert_eq!(syn.unknown_nodes.len(), 1);
    let content = syn.unknown_nodes[0].content.as_ref().unwrap();
    let text = span_str(input, content.start, content.end);
    assert!(
        text.contains("Array<string>"),
        "Content should contain raw text including HTML-like tokens: {}",
        text
    );

    // Template should still be parsed normally
    let ast = syn.template_ast().expect("template AST should exist");
    assert!(ast.root.content.is_some());
}

#[test]
fn sfc_mode_custom_block_component_inside_template_not_affected() {
    // A <docs> component inside <template> should NOT enter RCDATA
    // (SFC RCDATA only applies at root level, depth 0)
    let input = "<template><docs>inner content</docs></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    let errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::diagnostics::DiagnosticSeverity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Component inside template should work normally: {:?}",
        errors
    );

    // Should NOT be captured as a root unknown node
    assert_eq!(
        syn.unknown_nodes.len(),
        0,
        "docs inside template is a component, not a root block"
    );
}

#[test]
fn sfc_mode_multiple_custom_blocks_with_html_content() {
    let input = "<i18n>{\"key\": \"<b>value</b>\"}</i18n>\n<docs>Array<T> is generic</docs>\n<template><div/></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);

    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    let errors: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::diagnostics::DiagnosticSeverity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Multiple custom blocks with HTML-like content should not produce errors: {:?}",
        errors
    );
    assert_eq!(syn.unknown_nodes.len(), 2);
}

// =========================================================================
// Tokenizer error diagnostics — previously silently dropped error codes
// =========================================================================

#[test]
fn tokenizer_error_eof_in_comment() {
    let input = "<template><!-- unclosed comment</template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    let errs: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::EofInComment)
        .collect();
    assert_eq!(errs.len(), 1, "should emit EofInComment diagnostic");
    assert_eq!(
        errs[0].severity,
        crate::diagnostics::DiagnosticSeverity::Error
    );
    // Negative: must NOT emit EofInTag for this case
    assert!(
        !syn.diagnostics
            .iter()
            .any(|d| d.code == CompilerErrorCode::EofInTag),
        "should not emit EofInTag for unclosed comment"
    );
}

#[test]
fn tokenizer_error_abrupt_closing_of_empty_comment_short() {
    // <!-->  — abrupt closing of empty comment (3-char form)
    let input = "<template><!-->text</template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    let warns: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::AbruptClosingOfEmptyComment)
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "should emit AbruptClosingOfEmptyComment for <!-->",
    );
    assert_eq!(
        warns[0].severity,
        crate::diagnostics::DiagnosticSeverity::Warning
    );
    // Negative: must NOT emit EofInComment
    assert!(
        !syn.diagnostics
            .iter()
            .any(|d| d.code == CompilerErrorCode::EofInComment),
        "should not emit EofInComment for abrupt close"
    );
}

#[test]
fn tokenizer_error_abrupt_closing_of_empty_comment_long() {
    // <!--->  — abrupt closing of empty comment (4-char form)
    let input = "<template><!--->text</template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    let warns: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::AbruptClosingOfEmptyComment)
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "should emit AbruptClosingOfEmptyComment for <!--->",
    );
    assert_eq!(
        warns[0].severity,
        crate::diagnostics::DiagnosticSeverity::Warning
    );
}

#[test]
fn tokenizer_error_incorrectly_opened_comment_declaration() {
    // `<!DOCTYPE>` or `<!something>` — not a valid comment opening
    let input = "<template><!something></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    let warns: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::IncorrectlyOpenedComment)
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "should emit IncorrectlyOpenedComment for <!something>"
    );
    assert_eq!(
        warns[0].severity,
        crate::diagnostics::DiagnosticSeverity::Warning
    );
}

#[test]
fn tokenizer_error_incorrectly_opened_comment_single_dash() {
    // `<!-x>` — single dash, not a valid comment
    let input = "<template><!-x></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    let warns: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::IncorrectlyOpenedComment)
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "should emit IncorrectlyOpenedComment for <!-x>"
    );
    assert_eq!(
        warns[0].severity,
        crate::diagnostics::DiagnosticSeverity::Warning
    );
}

#[test]
fn tokenizer_error_cdata_in_html_content() {
    let input = "<template><div><![CDATA[text]]></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    let warns: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::CdataInHtmlContent)
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "should emit CdataInHtmlContent for <![CDATA[ in HTML"
    );
    assert_eq!(
        warns[0].severity,
        crate::diagnostics::DiagnosticSeverity::Warning
    );
}

#[test]
fn tokenizer_error_eof_in_cdata() {
    // CDATA that reaches EOF without ]]>
    let input = "<template><![CDATA[unclosed";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    let errs: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::EofInCdata)
        .collect();
    assert_eq!(errs.len(), 1, "should emit EofInCdata diagnostic");
    assert_eq!(
        errs[0].severity,
        crate::diagnostics::DiagnosticSeverity::Error
    );
}

#[test]
fn tokenizer_error_unexpected_equals_sign_before_attribute_name() {
    let input = "<template><div =\"val\"></div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    let warns: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::UnexpectedEqualsSignBeforeAttributeName)
        .collect();
    assert!(
        !warns.is_empty(),
        "should emit UnexpectedEqualsSignBeforeAttributeName"
    );
    assert_eq!(
        warns[0].severity,
        crate::diagnostics::DiagnosticSeverity::Warning
    );
}

#[test]
fn tokenizer_error_unexpected_question_mark_instead_of_tag_name() {
    let input = "<template><?xml version=\"1.0\"?></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    let warns: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| d.code == CompilerErrorCode::UnexpectedQuestionMarkInsteadOfTagName)
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "should emit UnexpectedQuestionMarkInsteadOfTagName for <?xml>"
    );
    assert_eq!(
        warns[0].severity,
        crate::diagnostics::DiagnosticSeverity::Warning
    );
}

#[test]
fn tokenizer_no_invalid_first_char_for_text_with_less_than() {
    // `<` followed by a digit is common in Vue templates (e.g., `count < 10`).
    // The tokenizer falls back to text mode — no diagnostic should be emitted.
    let input = "<template>2 < 1</template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    assert!(
        !syn.diagnostics
            .iter()
            .any(|d| d.code == CompilerErrorCode::InvalidFirstCharacterOfTagName),
        "should NOT emit InvalidFirstCharacterOfTagName for text containing '<'"
    );
}

#[test]
fn tokenizer_no_spurious_diagnostics_for_valid_template() {
    // Ensure valid templates don't emit any of the new diagnostics
    let input = "<template><div class=\"foo\"><!-- valid comment -->text</div></template>";
    let opts = SyntaxPluginOptions::default();
    let ctx = make_ctx(input, &opts);
    let mut syn = Syntax::new(false);
    tokenize_sfc_and_feed(&mut syn, input, &ctx);

    let new_codes = [
        CompilerErrorCode::EofInComment,
        CompilerErrorCode::EofInCdata,
        CompilerErrorCode::AbruptClosingOfEmptyComment,
        CompilerErrorCode::IncorrectlyOpenedComment,
        CompilerErrorCode::CdataInHtmlContent,
        CompilerErrorCode::UnexpectedEqualsSignBeforeAttributeName,
        CompilerErrorCode::UnexpectedQuestionMarkInsteadOfTagName,
    ];
    let spurious: Vec<_> = syn
        .diagnostics
        .iter()
        .filter(|d| new_codes.contains(&d.code))
        .collect();
    assert!(
        spurious.is_empty(),
        "valid template should not emit any new tokenizer error diagnostics, got: {:?}",
        spurious
    );
}

#[test]
fn is_member_expression_accepts_ts_as_cast() {
    // v-model:expanded="expanded as string[]" should be valid.
    // The `as Type` suffix is a TypeScript cast, not an operator.
    assert!(super::is_member_expression("expanded as string[]"));
    assert!(super::is_member_expression(
        "form.value as Record<string, any>"
    ));
    assert!(super::is_member_expression("items as unknown"));
    // Plain member expressions still work
    assert!(super::is_member_expression("expanded"));
    assert!(super::is_member_expression("form.value"));
    assert!(super::is_member_expression("obj['key']"));
    assert!(super::is_member_expression("obj?.nested"));
    // Invalid expressions still fail
    assert!(!super::is_member_expression("a + b"));
    assert!(!super::is_member_expression("fn()"));
    assert!(!super::is_member_expression(""));
}

/// The official Vue compiler validates v-model expressions with
/// `parseExpression` + `unwrapTSNode`: the unwrapped node must be a
/// `MemberExpression` / `OptionalMemberExpression` / `Identifier` (not
/// `undefined`). TS wrappers (`as`, `satisfies`, `!`, parentheses) are
/// transparent. This matrix pins Verter to those semantics.
#[test]
fn is_member_expression_accepts_official_vue_valid_forms() {
    let valid = [
        // plain identifiers
        "foo",
        "_foo",
        "$foo",
        "foo1",
        "NaN",
        " foo ",
        // unicode identifiers
        "变量",
        "café",
        // member chains
        "a.b",
        "a.b.c",
        "a['k']",
        "a[0]",
        "a[idx]",
        "a.b[c].d",
        "this.foo",
        // computed access with arbitrary inner expressions
        "a[idx + 1]",
        "a[fn(x)]",
        "a[\"with ] in string\"]",
        "a['it\\'s']",
        // optional chaining (accepted by the official compiler)
        "a?.b",
        "a?.[k]",
        "a?.b.c",
        // whitespace between tokens
        "a . b",
        "a\n  .b",
        "a [0]",
        "obj\n    .prop",
        // TS `as` casts
        "foo as string",
        "foo as string[]",
        "foo as Record<string, any>",
        "a.b as unknown as string",
        "foo as (x: string) => void",
        "foo as A<B<C>>",
        // satisfies
        "foo satisfies string",
        "a.b satisfies Record<string, unknown>",
        // parenthesized forms (the official compiler drops parens; unwrapTSNode sees through)
        "(foo)",
        "((foo))",
        "(a.b)",
        "(foo as string)",
        "((foo as string))",
        "( foo as string )",
        "(a.b as any)",
        "(foo satisfies string)",
        "(myValue as unknown) as string",
        // member access on parenthesized casts
        "(foo as any).bar",
        "(arr as string[])[0]",
        "(obj.a as MyObj).b",
        // non-null assertions
        "foo!",
        "a!.b",
        "a.b!",
        "a!.b!.c",
        "(a.b)!",
        "foo! as string",
        "(foo!)",
        // member access on call results is a member expression —
        // the official compiler accepts it
        "fn(x).y",
        "a.b().c",
        "list[getIdx()].value",
        "(a + b).c",
        // literal-rooted member chains and optional calls followed by a
        // member — both pass the official compiler's member check
        "'str'.length",
        "\"str\".length",
        "fn?.().x",
        "a?.b?.().c",
        // type-grammar coverage in cast suffixes
        "foo as -1",
        "foo as 'lit'",
        "foo as Ns.Inner",
        "foo as keyof T",
        "foo as readonly string[]",
        "foo as | A | B",
        "foo as A<B>[]",
        "foo as T extends U ? A : B",
        "foo as (x: unknown) => x is string",
        "foo as typeof window",
        "foo as A & B | C",
    ];
    for expr in valid {
        assert!(
            super::is_member_expression(expr),
            "expected VALID (official Vue accepts): {expr:?}"
        );
    }
}

#[test]
fn is_member_expression_rejects_official_vue_invalid_forms() {
    let invalid = [
        // empty
        "",
        "   ",
        // binary / unary / assignment / ternary / sequence expressions
        "a + b",
        "a - b",
        "a + b.c",
        "a.b + c",
        "a = b",
        "a += b",
        "a ? b : c",
        "a, b",
        "(a, b)",
        "!foo",
        "-a.b",
        "typeof a",
        "void 0",
        "new Foo()",
        // calls as the final node
        "fn()",
        "a.b()",
        "a?.()",
        "fn?.()",
        "fn(x)!",
        "(fn)(x)",
        // empty parenthesized groups are parse errors
        "().x",
        "( ).x",
        // literals
        "123",
        "1.5",
        "0x10",
        "'str'",
        "\"str\"",
        // bare keyword literals (not identifiers/members; `undefined`
        // is explicitly rejected by the official compiler)
        "undefined",
        "this",
        "true",
        "false",
        "null",
        "(this)",
        // reserved words cannot start an expression
        "switch.x",
        "class.x",
        // malformed casts / unbalanced groups
        "a as",
        "(myValue as string",
        "a as string, b",
        "foo as X > y",
        // the type grammar ends at expression operators: the official compiler re-enters
        // expression context and rejects the resulting Binary/Logical/
        // Conditional expression
        "a as string + b",
        "a as T ? b : c",
        "a as any || b",
        "a as any && b",
        "a as T - b",
        "a as T U",
        "a as string bar",
        "foo as if while",
        "a as A |",
        "a as T = b",
        // malformed member syntax
        "a.",
        ".a",
        "a..b",
        "a[",
        "a]",
        "a[]",
        "a !== b",
        "a != b",
        // array/object literals
        "[a]",
        "{ a }",
        "...a",
    ];
    for expr in invalid {
        assert!(
            !super::is_member_expression(expr),
            "expected INVALID (official Vue rejects): {expr:?}"
        );
    }
}

#[test]
fn v_slot_dotted_name_includes_full_name_in_arg() {
    // v-slot:item.title should have arg_end covering "item.title", not just "item"
    let input = r#"<template><Comp><template v-slot:item.title="{ val }"><span>{{ val }}</span></template></Comp></template>"#;
    let parsed = parse_sfc(input);
    let ast = parsed.template_ast().expect("template AST");

    // Find the inner <template v-slot:item.title> element
    let comp_node = &ast.nodes[ast.root.content.as_ref().unwrap().children[0].0];
    let comp_el = match &comp_node.kind {
        AstNodeKind::Element(el) => el,
        _ => panic!("expected element"),
    };
    let inner_id = comp_el.content.as_ref().unwrap().children[0];
    let inner_node = &ast.nodes[inner_id.0];
    let inner_el = match &inner_node.kind {
        AstNodeKind::Element(el) => el,
        _ => panic!("expected element"),
    };

    let v_slot = inner_el.v_slot.as_ref().expect("v_slot should exist");
    let arg_start = v_slot.arg_start.expect("arg_start");
    let arg_end = v_slot.arg_end.expect("arg_end");
    let slot_name = &input[arg_start as usize..arg_end as usize];

    // Positive: full dotted name
    assert_eq!(
        slot_name, "item.title",
        "v-slot arg should include the full dotted name"
    );

    // Negative: modifiers should be empty (dots merged into arg)
    assert!(
        v_slot.modifiers.is_empty(),
        "v-slot modifiers should be empty after dot merging, got: {:?}",
        v_slot.modifiers
    );

    // No parse errors
    assert!(
        !parsed.has_errors(),
        "should have no errors for dotted slot names"
    );
}

#[test]
fn v_slot_shorthand_dotted_name() {
    // #item.title shorthand should also get the full name
    let input =
        r#"<template><Comp><template #item.title="{ val }"><span/></template></Comp></template>"#;
    let parsed = parse_sfc(input);
    let ast = parsed.template_ast().expect("template AST");

    let comp_node = &ast.nodes[ast.root.content.as_ref().unwrap().children[0].0];
    let comp_el = match &comp_node.kind {
        AstNodeKind::Element(el) => el,
        _ => panic!("expected element"),
    };
    let inner_id = comp_el.content.as_ref().unwrap().children[0];
    let inner_node = &ast.nodes[inner_id.0];
    let inner_el = match &inner_node.kind {
        AstNodeKind::Element(el) => el,
        _ => panic!("expected element"),
    };

    let v_slot = inner_el.v_slot.as_ref().expect("v_slot should exist");
    let arg_start = v_slot.arg_start.expect("arg_start");
    let arg_end = v_slot.arg_end.expect("arg_end");
    let slot_name = &input[arg_start as usize..arg_end as usize];

    assert_eq!(
        slot_name, "item.title",
        "#item.title shorthand should include full dotted name"
    );
    assert!(
        v_slot.modifiers.is_empty(),
        "shorthand v-slot modifiers should be empty"
    );
}
