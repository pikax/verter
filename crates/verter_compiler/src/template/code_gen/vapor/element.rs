//! Vapor element code generation.
//!
//! Static HTML template collection, node navigation (`_child`, `_next`).
//!
//! Element processing in Vapor mode:
//! 1. **Enter**: Start accumulating HTML (`<tag attrs...>`).
//! 2. **Children**: Text/interpolation/comment append to HTML buffer.
//! 3. **Leave**: Close the HTML tag, finalize effects, and for root elements
//!    register the template and emit navigation + effect code.

use crate::ast::types::ElementNode;
use crate::template::code_gen::shared::helpers::VaporHelper;
use crate::template::code_gen::types::{
    CodeGenOutput, VaporCounters, VaporEffect, VaporElementState, VaporRootElement,
};

/// Build the HTML open tag from an element node into the current scope buffer.
///
/// Appends `<tag_name` plus static attributes to `html` — the single HTML
/// buffer for the element's template scope, into which every plain descendant
/// writes directly. Dynamic attributes (`:class`, `@click`, etc.) are skipped —
/// they become effects.
///
/// Vue 3.6 Vapor HTML minimization rules:
/// - Self-closing tags like `<br/>` become `<br>` (no slash)
/// - Attribute values without spaces are unquoted: `id=app` not `id="app"`
pub fn build_open_tag(element: &ElementNode, source: &str, html: &mut String) {
    html.push('<');
    let tag_name = &source[element.tag_open.start as usize + 1..element.tag_open.name_end as usize];
    html.push_str(tag_name);

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
        html.push(' ');
        html.push_str(name);
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value = &source[vs as usize..ve as usize];
            // Vue 3.6: unquote attr values that don't contain spaces or special chars
            if needs_attr_quoting(value) {
                html.push_str("=\"");
                html.push_str(value);
                html.push('"');
            } else {
                html.push('=');
                html.push_str(value);
            }
        }
    }

    // Vue 3.6: self-closing tags always use `>` not ` />` in Vapor HTML
    html.push('>');
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
        v_memo_expr: None,
    }
}

/// Bubble a non-root child element's render-side state into its parent.
///
/// The child's static HTML has already been written straight into the shared
/// scope buffer during the DFS, so nothing is copied here. What bubbles up is
/// the render-side work: navigation to reach a dynamic child, its text-node
/// creation, and the child's (and grandchildren's) effects and nav.
///
/// `dom_child_index` is the child's 0-based DOM index within the parent,
/// taken from the parent's running child cursor (`observe_dom_element`).
/// `child_has_dynamic_text` is derived from `ChildrenFlags::HasInterpolation`.
/// Returns the consumed child state for optional recycling into a pool.
pub fn merge_into_parent<'a>(
    mut child: VaporElementState<'a>,
    parent: &mut VaporElementState<'a>,
    counters: &mut VaporCounters,
    dom_child_index: u32,
    child_has_dynamic_text: bool,
    out: &mut CodeGenOutput<'a>,
) -> VaporElementState<'a> {
    // If child has dynamic content, we need navigation to reach it
    if child_has_dynamic_text || !child.own_effects.is_empty() || !child.child_effects.is_empty() {
        // Navigation and text-creation lines are assembled directly through
        // the reusable format-sink scratch (`alloc_fmt` → one bump per line),
        // instead of a fresh heap `String` per line plus a separate arena copy.
        let ref_idx = parent
            .node_ref
            .unwrap_or_else(|| parent.ensure_node_ref(counters));
        // First child reaches via `_child(parent)`, later siblings via
        // `_next(parent, dom_index)` — `pN` is the navigation path variable.
        let path_idx = counters.next_path();
        let nav = if dom_child_index == 0 {
            out.alloc_fmt(format_args!("const p{path_idx} = _child(n{ref_idx})"))
        } else {
            out.alloc_fmt(format_args!(
                "const p{path_idx} = _next(n{ref_idx}, {dom_child_index})"
            ))
        };
        parent.child_nav.push(nav);

        // If child has dynamic text, create its `_txt()` text node off `pN`.
        if let Some(text_ref) = child.text_node_ref {
            let tc = out.alloc_fmt(format_args!("const x{text_ref} = _txt(p{path_idx})"));
            parent.child_text_creations.push(tc);
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
    use crate::ast::types::*;
    use crate::template::code_gen::types::VaporTextPart;
    use crate::types::NodeTag;
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
        let mut html = String::new();
        build_open_tag(&element, source, &mut html);

        assert_eq!(html, "<div>");
    }

    #[test]
    fn open_tag_with_static_attr() {
        let source = r#"<div class="foo"></div>"#;
        let element = ElementNode {
            tag_open: make_tag(0, 17, 4),
            tag_close: Some(make_tag(17, 23, 22)),
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![crate::types::NodeProp {
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
                v_if_chains: SmallVec::new(),
            }),
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty().add(PropFlags::HasStaticClass),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };
        let mut html = String::new();
        build_open_tag(&element, source, &mut html);

        assert_eq!(html, "<div class=foo>");
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
            is_fully_static: false,
        };
        let mut html = String::new();
        build_open_tag(&element, source, &mut html);

        assert_eq!(html, "<br>");
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
                crate::types::NodeProp {
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
                crate::types::NodeProp {
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
                v_if_chains: SmallVec::new(),
            }),
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty().add(PropFlags::HasDynamicClass),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };
        let mut html = String::new();
        build_open_tag(&element, source, &mut html);

        // Only static id should be in HTML, not :class
        assert_eq!(html, "<div id=x>");
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
    fn merge_static_child_does_not_copy_html_or_emit_nav() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut parent = VaporElementState::new();
        parent.html = "<div>".to_string();

        // The child's HTML lives in the shared scope buffer already; its own
        // `html` field is irrelevant to the merge. Set it to a sentinel to
        // prove the merge never appends it into the parent.
        let mut child = VaporElementState::new();
        child.html = "<span>text</span>".to_string();

        // dom_child_index=0 (first child), no dynamic text
        let _ = merge_into_parent(child, &mut parent, &mut counters, 0, false, &mut out);

        // Negative: the merge must NOT copy child HTML into the parent.
        assert_eq!(parent.html, "<div>");
        assert!(parent.child_nav.is_empty()); // No navigation needed for static
    }

    #[test]
    fn merge_dynamic_child_uses_dom_index_for_nav() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut parent = VaporElementState::new();
        parent.html = "<div>".to_string();

        // First static child at dom_child_index=0 (no nav contribution).
        let static_child = VaporElementState::new();
        let _ = merge_into_parent(static_child, &mut parent, &mut counters, 0, false, &mut out);

        // Second dynamic child at dom_child_index=1
        let mut dynamic_child = VaporElementState::new();
        dynamic_child.text_node_ref = Some(counters.next_text());
        dynamic_child.own_effects = vec![VaporEffect::SetText {
            text_ref: 0,
            parts: vec![VaporTextPart::Dynamic("_toDisplayString(_ctx.msg)")],
        }];

        let _ = merge_into_parent(dynamic_child, &mut parent, &mut counters, 1, true, &mut out);

        assert!(!parent.child_nav.is_empty());
        // dom_child_index=1 → _next(n0, 1)
        assert!(
            parent.child_nav[0].contains(", 1)"),
            "Expected _next with index 1, got: {}",
            parent.child_nav[0]
        );
        // The merge bubbles render-side work only, never HTML.
        assert_eq!(parent.html, "<div>");
    }

    #[test]
    fn merge_dynamic_child_bubbles_nav_text_and_effects() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
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
        let _ = merge_into_parent(child, &mut parent, &mut counters, 0, true, &mut out);

        // Negative: HTML is untouched (it lives in the shared scope buffer).
        assert_eq!(parent.html, "<div>");
        // Render-side state bubbles up: nav, text-node creation, and effects.
        assert!(!parent.child_nav.is_empty());
        assert!(!parent.child_text_creations.is_empty());
        assert!(!parent.child_effects.is_empty());
    }
}
