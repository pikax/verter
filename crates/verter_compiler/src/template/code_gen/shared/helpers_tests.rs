use super::*;
use smallvec::SmallVec;

// ==================== parse_v_for_expression ====================

#[test]
fn shared_parse_v_for_simple() {
    let (params, iterable) = parse_v_for_expression("item in items");
    assert_eq!(params, "item");
    assert_eq!(iterable, "items");
}

#[test]
fn shared_parse_v_for_destructured() {
    let (params, iterable) = parse_v_for_expression("(item, index) in items");
    assert_eq!(params, "item, index");
    assert_eq!(iterable, "items");
}

#[test]
fn shared_parse_v_for_of() {
    let (params, iterable) = parse_v_for_expression("item of items");
    assert_eq!(params, "item");
    assert_eq!(iterable, "items");
}

#[test]
fn shared_parse_v_for_complex_iterable() {
    let (params, iterable) = parse_v_for_expression("item in items.filter(x => x.active)");
    assert_eq!(params, "item");
    assert_eq!(iterable, "items.filter(x => x.active)");
}

// ==================== extract_directive_value ====================

#[test]
fn shared_extract_directive_value_with_span() {
    let prop = NodeProp {
        start: 0,
        name_end: 4,
        is_directive: true,
        arg_start: None,
        arg_end: None,
        is_dynamic: None,
        value_start: Some(6),
        value_end: Some(10),
        modifiers: SmallVec::new(),
    };
    assert_eq!(extract_directive_value(&prop, "v-if=\"show\""), "show");
}

#[test]
fn shared_extract_directive_value_no_span() {
    let prop = NodeProp {
        start: 0,
        name_end: 4,
        is_directive: true,
        arg_start: None,
        arg_end: None,
        is_dynamic: None,
        value_start: None,
        value_end: None,
        modifiers: SmallVec::new(),
    };
    assert_eq!(extract_directive_value(&prop, "v-else"), "");
}

// ==================== Patch flags ====================

#[test]
fn format_patch_flag_production() {
    let result = format_patch_flag(1, true, |s| {
        // Simulate allocation — in tests we just leak
        Box::leak(s.to_string().into_boxed_str())
    });
    assert_eq!(result, "1");
}

#[test]
fn format_patch_flag_dev_single() {
    let result = format_patch_flag(1, false, |s| Box::leak(s.to_string().into_boxed_str()));
    assert_eq!(result, "1 /* TEXT */");
}

#[test]
fn format_patch_flag_dev_combined() {
    let result = format_patch_flag(PATCH_TEXT | PATCH_PROPS, false, |s| {
        Box::leak(s.to_string().into_boxed_str())
    });
    assert_eq!(result, "9 /* TEXT, PROPS */");
}

#[test]
fn format_patch_flag_dev_class_style() {
    let result = format_patch_flag(PATCH_CLASS | PATCH_STYLE, false, |s| {
        Box::leak(s.to_string().into_boxed_str())
    });
    assert_eq!(result, "6 /* CLASS, STYLE */");
}

// ==================== JS string escaping ====================

#[test]
fn needs_js_escaping_plain_text() {
    assert!(!needs_js_escaping("hello world"));
}

#[test]
fn needs_js_escaping_with_quote() {
    assert!(needs_js_escaping("say \"hi\""));
}

#[test]
fn needs_js_escaping_with_backslash() {
    assert!(needs_js_escaping("path\\to\\file"));
}

#[test]
fn needs_js_escaping_with_newline() {
    assert!(needs_js_escaping("line1\nline2"));
}

#[test]
fn needs_js_escaping_with_control_char() {
    assert!(needs_js_escaping("bell\x07"));
}

#[test]
fn escape_js_string_no_escaping() {
    assert_eq!(escape_js_string("hello"), "hello");
}

#[test]
fn escape_js_string_quotes_and_backslash() {
    assert_eq!(escape_js_string(r#"a"b\c"#), r#"a\"b\\c"#);
}

#[test]
fn escape_js_string_newlines() {
    assert_eq!(escape_js_string("a\nb\rc"), "a\\nb\\rc");
}

#[test]
fn escape_js_string_null_and_tab() {
    assert_eq!(escape_js_string("a\0b\tc"), "a\\0b\\tc");
}

#[test]
fn escape_js_string_unicode_line_separators() {
    assert_eq!(escape_js_string("a\u{2028}b\u{2029}c"), "a\\u2028b\\u2029c");
}

#[test]
fn escape_js_string_ascii_control() {
    assert_eq!(escape_js_string("a\x07b"), "a\\x07b");
}

#[test]
fn escape_js_string_into_appends() {
    let mut buf = String::from("prefix:");
    escape_js_string_into(&mut buf, "a\"b");
    assert_eq!(buf, "prefix:a\\\"b");
}

// ==================== Vapor HTML helpers ====================

#[test]
fn format_template_declaration_single_root() {
    let result = format_template_declaration(0, "<div>hello</div>", true);
    assert_eq!(result, "const t0 = _template(\"<div>hello</div>\", true)");
}

#[test]
fn format_template_declaration_multi_root() {
    let result = format_template_declaration(1, "<div>text</div>", false);
    assert_eq!(result, "const t1 = _template(\"<div>text</div>\")");
}

#[test]
fn format_template_declaration_escapes_quotes() {
    let result = format_template_declaration(0, r#"<div class="foo">text</div>"#, true);
    assert_eq!(
        result,
        r#"const t0 = _template("<div class=\"foo\">text</div>", true)"#
    );
}

#[test]
fn format_render_effect_single() {
    let result = format_render_effect(&["_setClass(n0, _ctx.cls)".to_string()]);
    assert_eq!(
        result,
        "_renderEffect(() => {\n  _setClass(n0, _ctx.cls)\n})"
    );
}

#[test]
fn format_render_effect_multiple() {
    let result = format_render_effect(&[
        "_setText(x0, _toDisplayString(_ctx.msg))".to_string(),
        "_setClass(n0, _ctx.cls)".to_string(),
    ]);
    assert_eq!(
        result,
        "_renderEffect(() => {\n  _setText(x0, _toDisplayString(_ctx.msg))\n  _setClass(n0, _ctx.cls)\n})"
    );
}

#[test]
fn format_render_effect_empty() {
    let result = format_render_effect(&[]);
    assert_eq!(result, "");
}

// ==================== VdomHelperFlags ====================

#[test]
fn vdom_flags_empty() {
    let flags = VdomHelperFlags::empty();
    assert!(flags.is_empty());
    assert!(!flags.has(VdomHelper::CreateElementVNode));
    assert!(flags.to_imports().is_empty());
}

#[test]
fn vdom_flags_add_single() {
    let flags = VdomHelperFlags::empty().add(VdomHelper::ToDisplayString);
    assert!(!flags.is_empty());
    assert!(flags.has(VdomHelper::ToDisplayString));
    assert!(!flags.has(VdomHelper::Fragment));
    assert_eq!(flags.to_imports(), vec!["_toDisplayString"]);
}

#[test]
fn vdom_flags_add_deduplicates() {
    let flags = VdomHelperFlags::empty()
        .add(VdomHelper::OpenBlock)
        .add(VdomHelper::OpenBlock);
    assert_eq!(flags.to_imports().len(), 1);
}

#[test]
fn vdom_flags_multiple() {
    let flags = VdomHelperFlags::empty()
        .add(VdomHelper::CreateElementVNode)
        .add(VdomHelper::Fragment)
        .add(VdomHelper::OpenBlock);
    assert!(flags.has(VdomHelper::CreateElementVNode));
    assert!(flags.has(VdomHelper::Fragment));
    assert!(flags.has(VdomHelper::OpenBlock));
    let imports = flags.to_imports();
    assert_eq!(imports.len(), 3);
    // Ordered by bit position
    assert_eq!(imports[0], "_createElementVNode");
    assert_eq!(imports[1], "_openBlock");
    assert_eq!(imports[2], "_Fragment");
}

#[test]
fn vdom_flags_union() {
    let a = VdomHelperFlags::empty()
        .add(VdomHelper::CreateVNode)
        .add(VdomHelper::Fragment);
    let b = VdomHelperFlags::empty()
        .add(VdomHelper::Fragment)
        .add(VdomHelper::VShow);
    let merged = a.union(b);
    assert!(merged.has(VdomHelper::CreateVNode));
    assert!(merged.has(VdomHelper::Fragment));
    assert!(merged.has(VdomHelper::VShow));
    assert_eq!(merged.to_imports().len(), 3);
}

#[test]
fn vdom_helper_name_roundtrip() {
    // Verify every variant's name matches the corresponding const
    assert_eq!(VdomHelper::CreateElementVNode.name(), CREATE_ELEMENT_VNODE);
    assert_eq!(VdomHelper::CreateElementBlock.name(), CREATE_ELEMENT_BLOCK);
    assert_eq!(VdomHelper::CreateVNode.name(), CREATE_VNODE);
    assert_eq!(VdomHelper::CreateBlock.name(), CREATE_BLOCK);
    assert_eq!(VdomHelper::CreateCommentVNode.name(), CREATE_COMMENT_VNODE);
    assert_eq!(VdomHelper::CreateTextVNode.name(), CREATE_TEXT_VNODE);
    assert_eq!(VdomHelper::OpenBlock.name(), OPEN_BLOCK);
    assert_eq!(VdomHelper::Fragment.name(), FRAGMENT);
    assert_eq!(VdomHelper::ToDisplayString.name(), TO_DISPLAY_STRING);
    assert_eq!(VdomHelper::RenderList.name(), RENDER_LIST);
    assert_eq!(VdomHelper::WithCtx.name(), WITH_CTX);
    assert_eq!(VdomHelper::WithDirectives.name(), WITH_DIRECTIVES);
    assert_eq!(VdomHelper::WithModifiers.name(), WITH_MODIFIERS);
    assert_eq!(VdomHelper::WithKeys.name(), WITH_KEYS);
    assert_eq!(VdomHelper::ResolveComponent.name(), RESOLVE_COMPONENT);
    assert_eq!(VdomHelper::ResolveDirective.name(), RESOLVE_DIRECTIVE);
    assert_eq!(VdomHelper::SetBlockTracking.name(), SET_BLOCK_TRACKING);
    assert_eq!(VdomHelper::VModelText.name(), V_MODEL_TEXT);
    assert_eq!(VdomHelper::VModelCheckbox.name(), V_MODEL_CHECKBOX);
    assert_eq!(VdomHelper::VModelRadio.name(), V_MODEL_RADIO);
    assert_eq!(VdomHelper::VModelSelect.name(), V_MODEL_SELECT);
    assert_eq!(VdomHelper::VModelDynamic.name(), V_MODEL_DYNAMIC);
    assert_eq!(VdomHelper::VShow.name(), V_SHOW);
    assert_eq!(VdomHelper::RenderSlot.name(), RENDER_SLOT);
    assert_eq!(VdomHelper::CreateSlots.name(), CREATE_SLOTS);
    assert_eq!(VdomHelper::MergeProps.name(), MERGE_PROPS);
    assert_eq!(VdomHelper::NormalizeClass.name(), NORMALIZE_CLASS);
    assert_eq!(VdomHelper::NormalizeStyle.name(), NORMALIZE_STYLE);
    assert_eq!(
        VdomHelper::ResolveDynamicComponent.name(),
        RESOLVE_DYNAMIC_COMPONENT
    );
}

// ==================== VaporHelperFlags ====================

#[test]
fn vapor_flags_empty() {
    let flags = VaporHelperFlags::empty();
    assert!(flags.is_empty());
    assert!(flags.to_imports().is_empty());
}

#[test]
fn vapor_flags_add_single() {
    let flags = VaporHelperFlags::empty().add(VaporHelper::Template);
    assert!(flags.has(VaporHelper::Template));
    assert_eq!(flags.to_imports(), vec!["_template"]);
}

#[test]
fn vapor_flags_add_deduplicates() {
    let flags = VaporHelperFlags::empty()
        .add(VaporHelper::RenderEffect)
        .add(VaporHelper::RenderEffect);
    assert_eq!(flags.to_imports().len(), 1);
}

#[test]
fn vapor_flags_multiple() {
    let flags = VaporHelperFlags::empty()
        .add(VaporHelper::Template)
        .add(VaporHelper::SetText)
        .add(VaporHelper::RenderEffect);
    let imports = flags.to_imports();
    assert_eq!(imports.len(), 3);
    assert_eq!(imports[0], "_template");
    assert_eq!(imports[1], "_setText");
    assert_eq!(imports[2], "_renderEffect");
}

#[test]
fn vapor_flags_union() {
    let a = VaporHelperFlags::empty()
        .add(VaporHelper::Child)
        .add(VaporHelper::SetClass);
    let b = VaporHelperFlags::empty()
        .add(VaporHelper::SetClass)
        .add(VaporHelper::CreateFor);
    let merged = a.union(b);
    assert_eq!(merged.to_imports().len(), 3);
}

#[test]
fn vapor_helper_name_roundtrip() {
    assert_eq!(VaporHelper::Template.name(), TEMPLATE);
    assert_eq!(VaporHelper::Txt.name(), TXT);
    assert_eq!(VaporHelper::SetText.name(), SET_TEXT);
    assert_eq!(VaporHelper::SetClass.name(), SET_CLASS);
    assert_eq!(VaporHelper::SetStyle.name(), SET_STYLE);
    assert_eq!(VaporHelper::SetProp.name(), SET_PROP);
    assert_eq!(VaporHelper::SetAttr.name(), SET_ATTR);
    assert_eq!(VaporHelper::SetHtml.name(), SET_HTML);
    assert_eq!(VaporHelper::SetDynamicProps.name(), SET_DYNAMIC_PROPS);
    assert_eq!(VaporHelper::Child.name(), CHILD);
    assert_eq!(VaporHelper::Next.name(), NEXT);
    assert_eq!(VaporHelper::RenderEffect.name(), RENDER_EFFECT);
    assert_eq!(VaporHelper::DelegateEvents.name(), DELEGATE_EVENTS);
    assert_eq!(VaporHelper::On.name(), ON);
    assert_eq!(VaporHelper::CreateInvoker.name(), CREATE_INVOKER);
    assert_eq!(VaporHelper::CreateIf.name(), CREATE_IF);
    assert_eq!(VaporHelper::CreateFor.name(), CREATE_FOR);
    assert_eq!(VaporHelper::CreateSlot.name(), CREATE_SLOT);
    assert_eq!(VaporHelper::CreateComponent.name(), CREATE_COMPONENT);
    assert_eq!(VaporHelper::ToDisplayString.name(), VAPOR_TO_DISPLAY_STRING);
}

// ==================== escape_template_literal_into ====================

/// @ai-generated — No escaping needed
#[test]
fn escape_template_literal_no_escape() {
    let mut buf = String::new();
    escape_template_literal_into(&mut buf, "<div class=\"foo\">text</div>");
    assert_eq!(buf, "<div class=\"foo\">text</div>");
}

/// @ai-generated — Backtick gets escaped
#[test]
fn escape_template_literal_backtick() {
    let mut buf = String::new();
    escape_template_literal_into(&mut buf, "a`b");
    assert_eq!(buf, "a\\`b");
}

/// @ai-generated — Backslash gets escaped
#[test]
fn escape_template_literal_backslash() {
    let mut buf = String::new();
    escape_template_literal_into(&mut buf, "a\\b");
    assert_eq!(buf, "a\\\\b");
}

/// @ai-generated — ${ gets escaped
#[test]
fn escape_template_literal_dollar_brace() {
    let mut buf = String::new();
    escape_template_literal_into(&mut buf, "a${b}");
    assert_eq!(buf, "a\\${b}");
}

/// @ai-generated — $ without { is NOT escaped
#[test]
fn escape_template_literal_dollar_no_brace() {
    let mut buf = String::new();
    escape_template_literal_into(&mut buf, "a$b");
    assert_eq!(buf, "a$b");
}

/// @ai-generated — Empty string
#[test]
fn escape_template_literal_empty() {
    let mut buf = String::new();
    escape_template_literal_into(&mut buf, "");
    assert_eq!(buf, "");
}

// ==================== build_static_html_with_scope ====================

/// @ai-generated — No scope ID injection
#[test]
fn build_static_html_no_scope() {
    let source = "<div>hello</div>";
    let mut buf = String::new();
    build_static_html_with_scope(source, 0, source.len() as u32, "data-v-abc", &[], &mut buf);
    assert_eq!(buf, "<div>hello</div>");
}

/// @ai-generated — Single injection point
#[test]
fn build_static_html_single_injection() {
    let source = "<div>hello</div>";
    // Inject before '>' at position 4
    let mut buf = String::new();
    build_static_html_with_scope(source, 0, source.len() as u32, "data-v-abc", &[4], &mut buf);
    assert_eq!(buf, "<div data-v-abc>hello</div>");
}

/// @ai-generated — Multiple injection points
#[test]
fn build_static_html_multiple_injections() {
    let source = "<div><span>text</span></div>";
    // '<div>' ends at 5, inject at 4 (before >)
    // '<span>' starts at 5, ends at 11, inject at 10 (before >)
    let mut buf = String::new();
    build_static_html_with_scope(
        source,
        0,
        source.len() as u32,
        "data-v-abc",
        &[4, 10],
        &mut buf,
    );
    assert_eq!(buf, "<div data-v-abc><span data-v-abc>text</span></div>");
}

// ==================== CreateStaticVNode helper ====================

/// @ai-generated — VdomHelper::CreateStaticVNode name
#[test]
fn vdom_helper_create_static_vnode_name() {
    assert_eq!(VdomHelper::CreateStaticVNode.name(), CREATE_STATIC_VNODE);
    assert_eq!(VdomHelper::CreateStaticVNode.name(), "_createStaticVNode");
}

/// @ai-generated — VdomHelperFlags includes CreateStaticVNode
#[test]
fn vdom_helper_flags_create_static_vnode() {
    let flags = VdomHelperFlags::empty().add(VdomHelper::CreateStaticVNode);
    assert!(flags.has(VdomHelper::CreateStaticVNode));
    let imports = flags.to_imports();
    assert!(imports.contains(&"_createStaticVNode"));
}
