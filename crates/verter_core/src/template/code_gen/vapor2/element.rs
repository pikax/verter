//! Vapor2 element code generation.
//!
//! Static HTML template collection, node navigation, and root finalization.
//! Uses NodeId-based variable naming — no counters.

use crate::ast::types::{AstNodeKind, ElementNode, TemplateAst};
use crate::template::code_gen::shared::helpers::{self, push_u32, VaporHelper};
use crate::template::code_gen::types::CodeGenOutput;
use crate::types::NodeId;

/// Build the HTML open tag from an element node.
///
/// Appends `<tag_name` plus static attributes to `html_buf`.
/// Dynamic attributes (`:class`, `@click`, etc.) are skipped — they become effects.
pub fn build_open_tag_html(element: &ElementNode, source: &str, html_buf: &mut String) {
    html_buf.push('<');
    let tag_name = &source[element.tag_open.start as usize + 1..element.tag_open.name_end as usize];
    html_buf.push_str(tag_name);

    for prop in &element.props {
        if prop.is_directive {
            continue; // Directives become effects, not HTML attributes
        }
        let name = &source[prop.start as usize..prop.name_end as usize];
        html_buf.push(' ');
        html_buf.push_str(name);
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value = &source[vs as usize..ve as usize];
            html_buf.push_str("=\"");
            html_buf.push_str(value);
            html_buf.push('"');
        }
    }

    if element.is_self_closing {
        html_buf.push_str(" />");
    } else {
        html_buf.push('>');
    }
}

/// Close the HTML tag for a non-void, non-self-closing element.
pub fn close_html_tag(html_buf: &mut String, tag_name: &str, is_void: bool) {
    if !is_void {
        html_buf.push_str("</");
        html_buf.push_str(tag_name);
        html_buf.push('>');
    }
}

/// Compute the 0-based DOM child index for a node within its parent element.
///
/// Adjacent Text/Interpolation nodes coalesce into a single DOM child.
/// Elements each count as one DOM child.
/// Comments count as a DOM child only when `comments` is true.
pub fn compute_dom_child_index(ast: &TemplateAst, child_id: NodeId, comments: bool) -> u32 {
    let child_node = &ast.nodes[child_id.0];
    let parent_id = child_node
        .parent
        .expect("root elements don't call compute_dom_child_index");
    let parent_el = match &ast.nodes[parent_id.0].kind {
        AstNodeKind::Element(el) => el,
        _ => unreachable!("parent of an element must be an element"),
    };
    let siblings = &parent_el
        .content
        .as_ref()
        .expect("parent must have content")
        .children;

    let mut dom_idx = 0u32;
    let mut in_text = false;
    for &sib_id in siblings {
        if sib_id == child_id {
            break;
        }
        match &ast.nodes[sib_id.0].kind {
            AstNodeKind::Text(_) | AstNodeKind::Interpolation(_) => {
                if !in_text {
                    in_text = true;
                    dom_idx += 1;
                }
            }
            AstNodeKind::Comment(_) => {
                if comments {
                    in_text = false;
                    dom_idx += 1;
                }
            }
            AstNodeKind::Element(_) => {
                in_text = false;
                dom_idx += 1;
            }
        }
    }
    dom_idx
}

/// Emit a navigation instruction to reach a non-root dynamic element.
///
/// Uses the NodeId as the variable name suffix. Returns the generated
/// variable name (e.g., `"n5"`) and pushes the navigation line to `body_lines`.
pub fn emit_navigation<'alloc>(
    id: NodeId,
    parent_id: NodeId,
    dom_child_index: u32,
    body_lines: &mut Vec<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) -> String {
    let mut var = String::with_capacity(8);
    var.push('n');
    push_u32(&mut var, id.0 as u32);

    let mut nav = String::with_capacity(40);
    nav.push_str("  const ");
    nav.push_str(&var);
    if dom_child_index == 0 {
        nav.push_str(" = _child(n");
        push_u32(&mut nav, parent_id.0 as u32);
        nav.push(')');
        out.add_vapor_import(VaporHelper::Child);
    } else {
        nav.push_str(" = _next(_child(n");
        push_u32(&mut nav, parent_id.0 as u32);
        nav.push_str("), ");
        push_u32(&mut nav, dom_child_index);
        nav.push(')');
        out.add_vapor_import(VaporHelper::Child);
        out.add_vapor_import(VaporHelper::Next);
    }

    body_lines.push(out.alloc_str(&nav));
    var
}

/// Write a template declaration into the template_decls buffer.
///
/// Generates: `const t{id} = _template("<html>", true)`
pub fn write_template_decl<'alloc>(
    id: NodeId,
    html: &str,
    is_single_root: bool,
    template_decls: &mut Vec<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) {
    let mut buf = String::with_capacity(html.len() + 40);
    buf.push_str("const t");
    push_u32(&mut buf, id.0 as u32);
    buf.push_str(" = _template(\"");
    helpers::escape_js_string_into(&mut buf, html);
    buf.push('"');
    if is_single_root {
        buf.push_str(", true");
    }
    buf.push(')');
    template_decls.push(out.alloc_str(&buf));
    out.add_vapor_import(VaporHelper::Template);
}

/// Write template instantiation line: `  const n{id} = t{id}()`
#[allow(dead_code)]
pub fn write_template_instantiation<'alloc>(
    id: NodeId,
    body_lines: &mut Vec<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) {
    body_lines.push(make_template_instantiation(id, out));
}

/// Create template instantiation string: `  const n{id} = t{id}()`
///
/// Returns a bump-allocated `&'alloc str`. Useful when inserting at a
/// specific index in `body_lines` (e.g., before child navigation).
pub fn make_template_instantiation<'alloc>(
    id: NodeId,
    out: &mut CodeGenOutput<'alloc>,
) -> &'alloc str {
    let mut buf = String::with_capacity(24);
    buf.push_str("  const n");
    push_u32(&mut buf, id.0 as u32);
    buf.push_str(" = t");
    push_u32(&mut buf, id.0 as u32);
    buf.push_str("()");
    out.alloc_str(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::types::*;
    use crate::types::NodeTag;
    use smallvec::SmallVec;

    fn make_tag(start: u32, end: u32, name_end: u32) -> NodeTag {
        NodeTag {
            start,
            end,
            name_end,
        }
    }

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
            is_fully_static: false,
        };
        let mut html = String::new();
        build_open_tag_html(&element, source, &mut html);
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
        build_open_tag_html(&element, source, &mut html);
        assert_eq!(html, r#"<div class="foo">"#);
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
        build_open_tag_html(&element, source, &mut html);
        assert_eq!(html, "<br />");
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
        build_open_tag_html(&element, source, &mut html);
        assert_eq!(html, r#"<div id="x">"#);
    }

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
        assert_eq!(html, "<br />");
    }
}
