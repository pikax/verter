//! Vapor element code generation.
//!
//! Static HTML template collection, node navigation (`_child`, `_next`).
//!
//! Element processing in Vapor mode:
//! 1. **Enter**: Start accumulating HTML (`<tag attrs...>`).
//! 2. **Children**: Text/interpolation/comment append to HTML buffer.
//! 3. **Leave**: Close the HTML tag, finalize effects, and for root elements
//!    register the template and emit navigation + effect code.

use crate::new_impl::ast::types::ElementNode;
use crate::new_impl::template::code_gen::shared::helpers::VaporHelper;
use crate::new_impl::template::code_gen::types::{
    CodeGenOutput, VaporCounters, VaporEffect, VaporElementState, VaporRootElement,
};

/// Build the HTML open tag from an element node.
///
/// Appends `<tag_name` plus static attributes to the parent's HTML buffer.
/// Dynamic attributes (`:class`, `@click`, etc.) are skipped — they become effects.
///
/// Vue 3.6 Vapor HTML minimization rules:
/// - Self-closing tags like `<br/>` become `<br>` (no slash)
/// - Attribute values without spaces are unquoted: `id=app` not `id="app"`
pub fn build_open_tag(element: &ElementNode, source: &str, state: &mut VaporElementState<'_>) {
    state.html.push('<');
    let tag_name = &source[element.tag_open.start as usize + 1..element.tag_open.name_end as usize];
    state.html.push_str(tag_name);

    // Add static attributes to HTML
    for prop in &element.props {
        if prop.is_directive {
            continue; // Directives become effects, not HTML attributes
        }
        let name = &source[prop.start as usize..prop.name_end as usize];
        // Skip ref — handled as _setTemplateRef statement, not HTML attribute
        if name == "ref" {
            continue;
        }
        state.html.push(' ');
        state.html.push_str(name);
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value = &source[vs as usize..ve as usize];
            // Vue 3.6: unquote attr values that don't contain spaces or special chars
            if needs_attr_quoting(value) {
                state.html.push_str("=\"");
                state.html.push_str(value);
                state.html.push('"');
            } else {
                state.html.push('=');
                state.html.push_str(value);
            }
        }
    }

    // Vue 3.6: self-closing tags always use `>` not ` />` in Vapor HTML
    state.html.push('>');
}

/// Check if an attribute value needs quoting in HTML.
///
/// Values need quoting if they contain spaces, quotes, `=`, `<`, `>`, `` ` ``,
/// or are empty. Vue 3.6 uses unquoted values when safe.
fn needs_attr_quoting(value: &str) -> bool {
    value.is_empty()
        || value
            .bytes()
            .any(|b| matches!(b, b' ' | b'"' | b'\'' | b'=' | b'<' | b'>' | b'`'))
}

/// Close the HTML tag for a non-void, non-self-closing element.
///
/// Takes fields separately to avoid borrow conflict between `html` and `tag_name`.
pub fn close_html_tag(html: &mut String, tag_name: &str, is_void: bool) {
    if !is_void {
        html.push_str("</");
        html.push_str(tag_name);
        html.push('>');
    }
}

/// Strip trailing close tags from an HTML string (Vue 3.6 Vapor minimization).
///
/// Vue 3.6 drops all trailing `</tag>` sequences from the template HTML.
/// For example:
/// - `<div>hello</div>` → `<div>hello`
/// - `<div><span>inner</span></div>` → `<div><span>inner`
/// - `<div><span>a</span><span>b</span></div>` → `<div><span>a</span><span>b`
pub fn strip_trailing_close_tags(html: &mut String) {
    // Repeatedly strip closing tags from the end
    loop {
        let trimmed = html.trim_end();
        if !trimmed.ends_with('>') {
            break;
        }
        // Check if it ends with a closing tag: </tagname>
        if let Some(open_bracket) = trimmed.rfind("</") {
            let close_tag_start = open_bracket;
            // Verify the rest is a valid tag name followed by >
            let between = &trimmed[close_tag_start + 2..trimmed.len() - 1];
            if !between.is_empty()
                && between
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                html.truncate(close_tag_start);
                continue;
            }
        }
        break;
    }
}

/// Finalize text parts on an element when leaving.
///
/// If the element has dynamic text (derived from `ChildrenFlags::HasInterpolation`),
/// creates a `SetText` effect from the accumulated text parts.
/// The `has_dynamic_text` flag is computed from the AST by the caller.
pub fn finalize_text_parts(state: &mut VaporElementState<'_>, has_dynamic_text: bool) {
    if !has_dynamic_text || state.text_parts.is_empty() {
        return;
    }

    if let Some(text_ref) = state.text_node_ref {
        let parts = std::mem::take(&mut state.text_parts);
        state
            .own_effects
            .push(VaporEffect::SetText { text_ref, parts });
    }
}

/// Build navigation instructions for accessing a child element's node.
///
/// Returns the navigation instruction string and the variable name for the child.
/// Uses `_child(parent)` for the first child, `_next(prev_sibling)` for subsequent.
pub fn build_child_nav(
    parent_var: &str,
    child_index: u32,
    counters: &mut VaporCounters,
) -> (String, String) {
    use crate::new_impl::template::code_gen::shared::helpers::push_u32;

    let path_idx = counters.next_path();
    let mut var = String::with_capacity(4);
    var.push('p');
    push_u32(&mut var, path_idx);

    let mut nav = String::with_capacity(32);
    nav.push_str("const ");
    nav.push_str(&var);
    if child_index == 0 {
        nav.push_str(" = _child(");
        nav.push_str(parent_var);
        nav.push(')');
    } else {
        nav.push_str(" = _next(");
        nav.push_str(parent_var);
        nav.push_str(", ");
        push_u32(&mut nav, child_index);
        nav.push(')');
    }

    (nav, var)
}

/// Process a completed root element, producing a `VaporRootElement`.
///
/// This is called when a root-level element's leave phase completes.
/// It finalizes the HTML template, assigns node refs, and collects all
/// effects and navigation.
///
/// `has_dynamic_text` is derived from `ChildrenFlags::HasInterpolation` by the caller.
pub fn finalize_root_element<'a>(
    mut state: VaporElementState<'a>,
    counters: &mut VaporCounters,
    out: &mut CodeGenOutput<'_>,
    has_dynamic_text: bool,
) -> VaporRootElement<'a> {
    // Finalize text parts into effects
    finalize_text_parts(&mut state, has_dynamic_text);

    // Vue 3.6: strip trailing close tags from HTML
    strip_trailing_close_tags(&mut state.html);

    // Register template
    let template_idx = counters.next_template();
    let node_ref = state.ensure_node_ref(counters);

    // Collect all effects (own + child)
    let mut all_effects: Vec<VaporEffect<'a>> = Vec::new();
    all_effects.append(&mut state.own_effects);
    all_effects.append(&mut state.child_effects);

    // Add imports
    out.add_vapor_import(VaporHelper::Template);
    if !state.child_nav.is_empty() {
        out.add_vapor_import(VaporHelper::Child);
        out.add_vapor_import(VaporHelper::Next);
    }
    if !state.child_text_creations.is_empty() {
        out.add_vapor_import(VaporHelper::Txt);
        out.add_vapor_import(VaporHelper::SetText);
    }
    if !all_effects.is_empty() {
        out.add_vapor_import(VaporHelper::RenderEffect);
    }

    VaporRootElement {
        html: state.html,
        template_idx: Some(template_idx),
        node_ref,
        nav: state.child_nav,
        text_creations: state.child_text_creations,
        effects: all_effects,
        statements: state.child_statements,
        v_once: false,
    }
}

/// Merge a child element's state into its parent when the child is not a root element.
///
/// The child's HTML is appended to the parent's HTML buffer.
/// The child's effects, navigation, and text creations bubble up to the parent.
///
/// `dom_child_index` is the child's 0-based DOM index within the parent,
/// computed from the AST by `compute_dom_child_index`.
/// `child_has_dynamic_text` is derived from `ChildrenFlags::HasInterpolation`.
/// Returns the consumed child state for optional recycling into a pool.
pub fn merge_into_parent<'a>(
    mut child: VaporElementState<'a>,
    parent: &mut VaporElementState<'a>,
    counters: &mut VaporCounters,
    dom_child_index: u32,
    child_has_dynamic_text: bool,
    out: &CodeGenOutput<'a>,
) -> VaporElementState<'a> {
    // Append child's HTML to parent
    parent.html.push_str(&child.html);

    // If child has dynamic content, we need navigation to reach it
    if child_has_dynamic_text || !child.own_effects.is_empty() || !child.child_effects.is_empty() {
        use crate::new_impl::template::code_gen::shared::helpers::push_u32;

        // Build parent_var without format!
        let ref_idx = parent
            .node_ref
            .unwrap_or_else(|| parent.ensure_node_ref(counters));
        let mut parent_var = String::with_capacity(4);
        parent_var.push('n');
        push_u32(&mut parent_var, ref_idx);

        let (nav_instruction, child_var) = build_child_nav(&parent_var, dom_child_index, counters);
        parent.child_nav.push(out.alloc_str(&nav_instruction));

        // If child has dynamic text, create _txt() call (avoid format!)
        if let Some(text_ref) = child.text_node_ref {
            let mut tc = String::with_capacity(24);
            tc.push_str("const x");
            push_u32(&mut tc, text_ref);
            tc.push_str(" = _txt(");
            tc.push_str(&child_var);
            tc.push(')');
            parent.child_text_creations.push(out.alloc_str(&tc));
        }

        // Bubble up child's own effects (append drains child vec, capacity retained)
        parent.child_effects.append(&mut child.own_effects);

        // Bubble up grandchild effects
        parent.child_effects.append(&mut child.child_effects);

        // Bubble up grandchild navigation
        parent.child_nav.append(&mut child.child_nav);

        // Bubble up grandchild text creations
        parent
            .child_text_creations
            .append(&mut child.child_text_creations);
    }

    child
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::new_impl::ast::types::*;
    use crate::new_impl::template::code_gen::types::VaporTextPart;
    use crate::new_impl::types::NodeTag;
    use oxc_allocator::Allocator;
    use smallvec::SmallVec;

    fn make_tag(start: u32, end: u32, name_end: u32) -> NodeTag {
        NodeTag {
            start,
            end,
            name_end,
        }
    }

    // ==================== build_open_tag ====================

    #[test]
    fn open_tag_simple_div() {
        let source = "<div></div>";
        let element = ElementNode {
            tag_open: make_tag(0, 5, 4),
            tag_close: Some(make_tag(5, 11, 10)),
            tag_type: TagType::Element,
            is_self_closing: false,
            props: Vec::new(),
            content: Some(ElementContent {
                start: 5,
                end: 5,
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
        let mut state = VaporElementState::new();
        build_open_tag(&element, source, &mut state);

        assert_eq!(state.html, "<div>");
    }

    #[test]
    fn open_tag_with_static_attr() {
        let source = r#"<div class="foo"></div>"#;
        let element = ElementNode {
            tag_open: make_tag(0, 17, 4),
            tag_close: Some(make_tag(17, 23, 22)),
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![crate::new_impl::types::NodeProp {
                start: 5,
                name_end: 10,
                is_directive: false,
                arg_start: None,
                arg_end: None,
                is_dynamic: None,
                value_start: Some(12),
                value_end: Some(15),
                modifiers: SmallVec::new(),
            }],
            content: Some(ElementContent {
                start: 17,
                end: 17,
                children: SmallVec::new(),
            }),
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty().add(PropFlags::HasStaticClass),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
        };
        let mut state = VaporElementState::new();
        build_open_tag(&element, source, &mut state);

        assert_eq!(state.html, "<div class=foo>");
    }

    #[test]
    fn open_tag_self_closing() {
        let source = "<br />";
        let element = ElementNode {
            tag_open: make_tag(0, 6, 3),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: true,
            props: Vec::new(),
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
        };
        let mut state = VaporElementState::new();
        build_open_tag(&element, source, &mut state);

        assert_eq!(state.html, "<br>");
    }

    #[test]
    fn open_tag_skips_directives() {
        let source = r#"<div :class="cls" id="x"></div>"#;
        let element = ElementNode {
            tag_open: make_tag(0, 25, 4),
            tag_close: Some(make_tag(25, 31, 30)),
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![
                // :class="cls" — directive, should be skipped
                crate::new_impl::types::NodeProp {
                    start: 5,
                    name_end: 11,
                    is_directive: true,
                    arg_start: Some(6),
                    arg_end: Some(11),
                    is_dynamic: None,
                    value_start: Some(13),
                    value_end: Some(16),
                    modifiers: SmallVec::new(),
                },
                // id="x" — static attribute, should be included
                crate::new_impl::types::NodeProp {
                    start: 18,
                    name_end: 20,
                    is_directive: false,
                    arg_start: None,
                    arg_end: None,
                    is_dynamic: None,
                    value_start: Some(22),
                    value_end: Some(23),
                    modifiers: SmallVec::new(),
                },
            ],
            content: Some(ElementContent {
                start: 25,
                end: 25,
                children: SmallVec::new(),
            }),
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty().add(PropFlags::HasDynamicClass),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
        };
        let mut state = VaporElementState::new();
        build_open_tag(&element, source, &mut state);

        // Only static id should be in HTML, not :class
        assert_eq!(state.html, "<div id=x>");
    }

    // ==================== close_html_tag ====================

    #[test]
    fn close_tag_non_void() {
        let mut html = "<div>hello".to_string();
        close_html_tag(&mut html, "div", false);
        assert_eq!(html, "<div>hello</div>");
    }

    #[test]
    fn close_tag_void_noop() {
        let mut html = "<br />".to_string();
        close_html_tag(&mut html, "br", true);
        assert_eq!(html, "<br />"); // No close tag for void
    }

    // ==================== finalize_text_parts ====================

    #[test]
    fn finalize_text_parts_creates_effect() {
        let mut state = VaporElementState::new();
        state.text_node_ref = Some(0);
        state.text_parts = vec![
            VaporTextPart::Static("\"hello \""),
            VaporTextPart::Dynamic("_toDisplayString(_ctx.msg)"),
        ];

        finalize_text_parts(&mut state, true);

        assert_eq!(state.own_effects.len(), 1);
        let effect = &state.own_effects[0];
        assert_eq!(
            effect.to_code(),
            "_setText(x0, \"hello \" + _toDisplayString(_ctx.msg))"
        );
    }

    #[test]
    fn finalize_text_parts_no_dynamic_noop() {
        let mut state = VaporElementState::new();
        state.text_parts = vec![VaporTextPart::Static("\"hello\"")];

        finalize_text_parts(&mut state, false);
        assert!(state.own_effects.is_empty());
    }

    // ==================== build_child_nav ====================

    #[test]
    fn nav_first_child() {
        let mut counters = VaporCounters::default();
        let (nav, var) = build_child_nav("n0", 0, &mut counters);
        assert_eq!(nav, "const p0 = _child(n0)");
        assert_eq!(var, "p0");
    }

    #[test]
    fn nav_nth_child() {
        let mut counters = VaporCounters::default();
        let (nav, var) = build_child_nav("p0", 2, &mut counters);
        assert_eq!(nav, "const p0 = _next(p0, 2)");
        assert_eq!(var, "p0");
    }

    // ==================== finalize_root_element ====================

    #[test]
    fn finalize_root_static_only() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();

        let mut state = VaporElementState::new();
        state.html = "<div>hello</div>".to_string();

        let root = finalize_root_element(state, &mut counters, &mut out, false);

        assert_eq!(root.html, "<div>hello");
        assert_eq!(root.template_idx, Some(0));
        assert_eq!(root.node_ref, 0);
        assert!(root.effects.is_empty());
        assert!(root.nav.is_empty());
        assert!(out.vapor_imports().has(VaporHelper::Template));
    }

    #[test]
    fn finalize_root_with_dynamic_text() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();

        let mut state = VaporElementState::new();
        state.html = "<div> </div>".to_string();
        state.text_node_ref = Some(counters.next_text());
        state.text_parts = vec![VaporTextPart::Dynamic("_toDisplayString(_ctx.msg)")];

        let root = finalize_root_element(state, &mut counters, &mut out, true);

        assert_eq!(root.effects.len(), 1);
        assert!(out.vapor_imports().has(VaporHelper::RenderEffect));
    }

    // ==================== merge_into_parent ====================

    #[test]
    fn merge_static_child_into_parent() {
        let alloc = Allocator::default();
        let out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut parent = VaporElementState::new();
        parent.html = "<div>".to_string();

        let mut child = VaporElementState::new();
        child.html = "<span>text</span>".to_string();

        // dom_child_index=0 (first child), no dynamic text
        let _ = merge_into_parent(child, &mut parent, &mut counters, 0, false, &out);

        assert_eq!(parent.html, "<div><span>text</span>");
        assert!(parent.child_nav.is_empty()); // No navigation needed for static
    }

    #[test]
    fn merge_dynamic_child_uses_dom_index_for_nav() {
        let alloc = Allocator::default();
        let out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut parent = VaporElementState::new();
        parent.html = "<div>".to_string();

        // First static child at dom_child_index=0
        let mut static_child = VaporElementState::new();
        static_child.html = "<p>text</p>".to_string();
        let _ = merge_into_parent(static_child, &mut parent, &mut counters, 0, false, &out);

        // Second dynamic child at dom_child_index=1
        let mut dynamic_child = VaporElementState::new();
        dynamic_child.html = "<span> </span>".to_string();
        dynamic_child.text_node_ref = Some(counters.next_text());
        dynamic_child.own_effects = vec![VaporEffect::SetText {
            text_ref: 0,
            parts: vec![VaporTextPart::Dynamic("_toDisplayString(_ctx.msg)")],
        }];

        let _ = merge_into_parent(dynamic_child, &mut parent, &mut counters, 1, true, &out);

        assert!(!parent.child_nav.is_empty());
        // dom_child_index=1 → _next(n0, 1)
        assert!(
            parent.child_nav[0].contains(", 1)"),
            "Expected _next with index 1, got: {}",
            parent.child_nav[0]
        );
    }

    #[test]
    fn merge_dynamic_child_into_parent() {
        let alloc = Allocator::default();
        let out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut parent = VaporElementState::new();
        parent.html = "<div>".to_string();

        let mut child = VaporElementState::new();
        child.html = "<span> </span>".to_string();
        child.text_node_ref = Some(counters.next_text());
        child.own_effects = vec![VaporEffect::SetText {
            text_ref: 0,
            parts: vec![VaporTextPart::Dynamic("_toDisplayString(_ctx.msg)")],
        }];

        // dom_child_index=0 (first child), has dynamic text
        let _ = merge_into_parent(child, &mut parent, &mut counters, 0, true, &out);

        assert_eq!(parent.html, "<div><span> </span>");
        assert!(!parent.child_nav.is_empty());
        assert!(!parent.child_text_creations.is_empty());
        assert!(!parent.child_effects.is_empty());
    }
}
