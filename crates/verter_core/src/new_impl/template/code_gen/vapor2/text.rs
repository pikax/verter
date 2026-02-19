//! Vapor2 text processing.
//!
//! Processes accumulated text parts into `_setText` calls.

use crate::new_impl::template::code_gen::shared::helpers::{push_u32, VaporHelper};
use crate::new_impl::template::code_gen::types::{CodeGenOutput, VaporTextPart};
use crate::new_impl::types::NodeId;

/// Process accumulated text parts for an element with dynamic text.
///
/// Emits `_setText(n{parent_id}, parts...)` inside the render effect body,
/// and `const x{parent_id} = _txt(n{parent_id})` as a creation statement.
///
/// Returns `true` if any text parts were emitted (caller should set `has_render_effect`).
pub fn process_text_parts<'alloc>(
    parent_id: NodeId,
    text_parts: &mut Vec<VaporTextPart<'alloc>>,
    body_lines: &mut Vec<&'alloc str>,
    effect_lines: &mut Vec<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) -> bool {
    if text_parts.is_empty() {
        return false;
    }

    // Emit: const x{id} = _txt(n{id})
    let mut txt_line = String::with_capacity(32);
    txt_line.push_str("  const x");
    push_u32(&mut txt_line, parent_id.0 as u32);
    txt_line.push_str(" = _txt(n");
    push_u32(&mut txt_line, parent_id.0 as u32);
    txt_line.push(')');
    body_lines.push(out.alloc_str(&txt_line));
    out.add_vapor_import(VaporHelper::Txt);

    // Emit: _setText(x{id}, parts...)
    let mut set_line = String::with_capacity(64);
    set_line.push_str("    _setText(x");
    push_u32(&mut set_line, parent_id.0 as u32);
    set_line.push_str(", ");
    for (i, part) in text_parts.iter().enumerate() {
        if i > 0 {
            set_line.push_str(" + ");
        }
        set_line.push_str(part.to_js());
    }
    set_line.push(')');
    effect_lines.push(out.alloc_str(&set_line));
    out.add_vapor_import(VaporHelper::SetText);

    text_parts.clear();
    true
}
