//! HTML payload construction and upward movement of plain-element rows.
use crate::ast::types::ElementNode;
pub(super) fn build_open_tag(element: &ElementNode, source: &str, html: &mut String) {
    html.push('<');
    let tag_name = &source[element.tag_open.start as usize + 1..element.tag_open.name_end as usize];
    html.push_str(tag_name);

    // Add static attributes to HTML
    for prop in &element.props {
        if prop.is_directive {
            continue; // Directives become effects, not HTML attributes
        }
        let name = &source[prop.start as usize..prop.name_end as usize];
        // Skip ref — handled as a template-ref statement, not HTML attribute
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

/// Close the HTML tag of every element except a void one.
///
/// Takes fields separately to avoid borrow conflict between `html` and `tag_name`.
pub(super) fn close_html_tag(html: &mut String, tag_name: &str, is_void: bool) {
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
///
/// Text after the last end tag is a node of its own (`<div><p>x</p> ` keeps
/// the space a text effect sets), so stripping stops at any text, whitespace
/// included.
pub(super) fn strip_trailing_close_tags(html: &mut String) {
    // Repeatedly strip closing tags from the end
    loop {
        let trimmed = html.as_str();
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

use super::counters::VaporCounters;
use super::effect::VaporEffect;
use super::ops::{NameKind, Nav, TextCreation};
use super::scope::VaporScope;
use super::state::VaporState;

/// A text run reached by navigation is emitted at its reserved statement position.
pub(super) fn finalize_text_parts<'c>(
    scope: &mut VaporScope<'c>,
    has_dynamic_text: bool,
    state: &mut VaporState<'c>,
) {
    if !has_dynamic_text || scope.text_parts.is_empty() || !scope.text_ref_generated {
        return;
    }
    if let Some(text_ref) = scope.text_node_ref {
        let effect = state.add_effect(VaporEffect::Text {
            text_ref,
            parts: std::mem::take(&mut scope.text_parts),
            generated: true,
        });
        scope.own_effects.push(effect);
    }
}

/// Move a plain child's rows upward; its HTML already belongs to the scope buffer.
///
/// The child's navigation declares the name its rows use: a child whose
/// node reference is named (by its effects, events, insertion points or own
/// children's navigation) is declared under that node name; a child read
/// only for its text gets a path name.
pub(super) fn merge_into_parent<'c>(
    mut child: VaporScope<'c>,
    parent: &mut VaporScope<'c>,
    counters: &mut VaporCounters,
    dom_child_index: u32,
    child_has_dynamic_text: bool,
) -> VaporScope<'c> {
    if child_has_dynamic_text || !child.own_effects.is_empty() || !child.child_effects.is_empty() {
        let container = parent.ensure_node_ref(counters);
        let from = match child.node_ref {
            Some(target) if child.node_named => (NameKind::Node, target),
            _ => (NameKind::Path, counters.next_path()),
        };
        parent.child_nav.push(Nav::At {
            target: from,
            container,
            index: dom_child_index,
            text: false,
        });
        if let Some(text) = child.text_node_ref {
            if child.text_ref_generated {
                parent
                    .child_text_creations
                    .push(TextCreation { text, from });
            }
        }
        parent.child_effects.append(&mut child.own_effects);
        parent.child_effects.append(&mut child.child_effects);
        parent.child_nav.append(&mut child.child_nav);
        parent
            .child_text_creations
            .append(&mut child.child_text_creations);
    }
    child
}
