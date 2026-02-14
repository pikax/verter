use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        plugins::code_gen::{
            template::vdom::helper::write_patch_flag_suffix, types::TemplateImportDependencies,
        },
        types::OxcCompiledElementClosed,
    },
    utils::vue::PatchFlags,
};

use super::super::{ChildInfo, ChildKind, StateStack};

/// Process the closing of an element.
///
/// **Close-phase children logic**: examines `state.children` to retroactively
/// insert separators via deferred `prepend_left` (collected in `pending_prepend_lefts`
/// for batch application):
///
/// - **All Text/Interpolation** → concatenation mode: `, ` before first, ` + ` between rest.
///   Adds TEXT patch flag if any interpolation is present.
/// - **Single non-text child** → `, ` before it.
/// - **Multiple mixed children** → array mode: `, [` before first, `, ` between rest, `]` in close.
///
/// **Slot mode** (when `state.slot_params` is Some): wraps children in
/// `{ default: _withCtx((params) => [ ... ]), _: 1 /* STABLE */ }`.
/// Text+interpolation children use `_createTextVNode(...)` inside slots.
///
/// Then emits patch flags, dynamic props, and closing paren.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_element_close<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    ev: &OxcCompiledElementClosed,
    state: &StateStack<'alloc>,
    is_production: bool,
    imports: &mut TemplateImportDependencies,
    pending_prepend_lefts: &mut Vec<(u32, &'alloc str)>,
    pending_overwrites: &mut Vec<(u32, u32, &'alloc str)>,
    pending_append_lefts: &mut Vec<(u32, &'alloc str)>,
    buf: &mut String,
) {
    let mut patch_flag = state.patch_flag;

    let has_children = !state.children.is_empty();
    let all_text_like = state
        .children
        .iter()
        .all(|c| matches!(c.kind, ChildKind::Text | ChildKind::Interpolation));
    let has_interpolation = state
        .children
        .iter()
        .any(|c| c.kind == ChildKind::Interpolation);
    let needs_array = has_children && !all_text_like && state.children.len() > 1;

    // Slot mode: children become `{ default: _withCtx((params) => [ ... ]), _: 1 }`
    let is_slot = state.slot_params.is_some();

    if is_slot && has_children {
        let params = state.slot_params.unwrap_or("");

        // Determine slot name: static ("default", "header", etc.) or dynamic ([expr])
        // Note: for dynamic slots, the arg span already includes brackets `[expr]`
        // from the tokenizer, so no extra wrapping is needed.
        let slot_key = state.slot_name.unwrap_or("default");

        // Build slot_open string
        buf.clear();
        buf.push_str(", {");
        buf.push_str(slot_key);
        buf.push_str(": _withCtx(");
        if !params.is_empty() {
            buf.push('(');
            buf.push_str(params);
            buf.push(')');
        } else {
            buf.push_str("()");
        }
        buf.push_str(" => [");
        let slot_open = code_transform.alloc_str(buf);

        // In slot mode, text+interpolation children are wrapped in _createTextVNode.
        // All children go into an array inside _withCtx.
        if all_text_like {
            // Wrap text+interp in _createTextVNode("text " + _toDisplayString(expr), 1)
            imports.add(TemplateImportDependencies::CREATE_TEXT_VNODE);
            let first_child = &state.children[0];

            // Build the text content prefix with _createTextVNode
            let text_flag = if has_interpolation {
                if is_production {
                    ", 1"
                } else {
                    ", 1 /* TEXT */"
                }
            } else {
                ""
            };

            buf.clear();
            buf.push_str(slot_open);
            buf.push_str("_createTextVNode(");
            buf.push_str(first_child.kind.content_prefix());
            let s = code_transform.alloc_str(buf);
            pending_prepend_lefts.push((first_child.start, s));

            // Join remaining text/interp children with " + "
            for child in state.children.iter().skip(1) {
                let prefix = child.kind.content_prefix();
                buf.clear();
                buf.push_str(" + ");
                buf.push_str(prefix);
                let s = code_transform.alloc_str(buf);
                pending_prepend_lefts.push((child.start, s));
            }

            let slot_stable = if state.slot_is_dynamic {
                if is_production {
                    "2"
                } else {
                    "2 /* DYNAMIC */"
                }
            } else if is_production {
                "1"
            } else {
                "1 /* STABLE */"
            };
            buf.clear();
            buf.push_str(text_flag);
            buf.push_str(")]), _: ");
            buf.push_str(slot_stable);
            buf.push('}');
            buf.push(')');
            write_patch_flag_suffix(buf, patch_flag, &state.dynamic_props, is_production);
            if state.is_block_root {
                buf.push(')');
            }

            if let Some(close_tag) = &ev.event.event_close_tag {
                let s = code_transform.alloc_str(buf);
                pending_overwrites.push((close_tag.start, close_tag.end, s));
            } else {
                let s = code_transform.alloc_str(buf);
                pending_append_lefts.push((state.open_tag_end, s));
            }
            return;
        } else {
            // Mixed or element-only children: each child is a separate array entry
            for (i, child) in state.children.iter().enumerate() {
                let prefix = child.kind.content_prefix();
                let scope = &child.scope_prefix;
                buf.clear();
                if i == 0 {
                    buf.push_str(slot_open);
                    buf.push_str(scope);
                    buf.push_str(prefix);
                } else {
                    buf.push_str(", ");
                    buf.push_str(scope);
                    buf.push_str(prefix);
                }
                let s = code_transform.alloc_str(buf);
                pending_prepend_lefts.push((child.start, s));
            }

            let slot_stable = if state.slot_is_dynamic {
                if is_production {
                    "2"
                } else {
                    "2 /* DYNAMIC */"
                }
            } else if is_production {
                "1"
            } else {
                "1 /* STABLE */"
            };
            buf.clear();
            buf.push_str("]), _: ");
            buf.push_str(slot_stable);
            buf.push_str("})");
            write_patch_flag_suffix(buf, patch_flag, &state.dynamic_props, is_production);
            if state.is_block_root {
                buf.push(')');
            }

            if let Some(close_tag) = &ev.event.event_close_tag {
                let s = code_transform.alloc_str(buf);
                pending_overwrites.push((close_tag.start, close_tag.end, s));
            } else {
                let s = code_transform.alloc_str(buf);
                pending_append_lefts.push((state.open_tag_end, s));
            }
            return;
        }
    }

    // Normal (non-slot) children handling
    if has_children {
        if all_text_like {
            // Concatenation mode: join with " + "
            for (i, child) in state.children.iter().enumerate() {
                let sep = if i == 0 { ", " } else { " + " };
                let s = child_separator_str(code_transform, sep, child, buf);
                pending_prepend_lefts.push((child.start, s));
            }
            if has_interpolation {
                patch_flag = patch_flag.add(PatchFlags::Text);
            }
        } else if state.children.len() == 1 {
            // Single non-text child: separator + scope_prefix + content_prefix
            let child = &state.children[0];
            let s = child_separator_str(code_transform, ", ", child, buf);
            pending_prepend_lefts.push((child.start, s));
        } else {
            // Multiple mixed children: array wrapping
            for (i, child) in state.children.iter().enumerate() {
                let sep = if i == 0 { ", [" } else { ", " };
                let s = child_separator_str(code_transform, sep, child, buf);
                pending_prepend_lefts.push((child.start, s));
            }
        }
    }

    // Build the closing string: optional array close + suffix + closing paren.
    // Block roots need an extra `)` to close the outer `(_openBlock(), ...)` grouping.
    // Fast path: use &'static str for common close strings to avoid bump allocation.
    let close_str: &'alloc str = if patch_flag.0 == 0 && !state.is_block_root {
        if needs_array {
            "])"
        } else {
            ")"
        }
    } else if patch_flag.0 == 0 && state.is_block_root {
        if needs_array {
            "]))"
        } else {
            "))"
        }
    } else {
        buf.clear();
        if needs_array {
            buf.push(']');
        }
        write_patch_flag_suffix(buf, patch_flag, &state.dynamic_props, is_production);
        buf.push(')');
        if state.is_block_root {
            buf.push(')');
        }
        code_transform.alloc_str(buf)
    };

    let close_pos = if let Some(close_tag) = &ev.event.event_close_tag {
        // Overwrite `</tagname>` with the close string
        pending_overwrites.push((close_tag.start, close_tag.end, close_str));
        close_tag.end
    } else {
        // Non-void element without close tag (shouldn't normally happen)
        pending_append_lefts.push((state.open_tag_end, close_str));
        state.open_tag_end
    };

    // withDirectives wrapping for runtime directives (v-model native, v-show, custom)
    emit_with_directives(code_transform, state, close_pos, pending_append_lefts, buf);
}

/// Build the separator+prefix string for a child in the close phase.
///
/// When `scope_prefix` is empty (the overwhelmingly common case), returns a
/// `&'static str` to avoid bump allocation entirely. Only allocates when
/// a non-empty scope prefix (v-if/v-for/v-once) is present.
#[inline]
fn child_separator_str<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    sep: &str,
    child: &ChildInfo<'alloc>,
    buf: &mut String,
) -> &'alloc str {
    let prefix = child.kind.content_prefix();
    let scope = &child.scope_prefix;

    if scope.is_empty() {
        // Fast path: use static strings for common combinations
        match (sep, prefix) {
            (", ", "") => ", ",
            (", ", "\"") => ", \"",
            (", ", "_toDisplayString") => ", _toDisplayString",
            (" + ", "\"") => " + \"",
            (" + ", "_toDisplayString") => " + _toDisplayString",
            (", [", "") => ", [",
            (", [", "\"") => ", [\"",
            (", [", "_toDisplayString") => ", [_toDisplayString",
            _ => {
                buf.clear();
                buf.push_str(sep);
                buf.push_str(prefix);
                code_transform.alloc_str(buf)
            }
        }
    } else {
        // Slow path: dynamic scope_prefix requires allocation
        buf.clear();
        buf.push_str(sep);
        buf.push_str(scope);
        buf.push_str(prefix);
        code_transform.alloc_str(buf)
    }
}

/// Emit the `, [[...]])` suffix for runtime directives.
fn emit_with_directives<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    state: &StateStack<'alloc>,
    close_pos: u32,
    pending_append_lefts: &mut Vec<(u32, &'alloc str)>,
    buf: &mut String,
) {
    if state.runtime_directives.is_empty() {
        return;
    }

    buf.clear();
    buf.push_str(", [");
    for (i, dir) in state.runtime_directives.iter().enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        buf.push('[');
        buf.push_str(dir.directive);
        if !dir.value.is_empty() || !dir.arg.is_empty() || !dir.modifiers.is_empty() {
            buf.push_str(", ");
            if dir.value.is_empty() {
                buf.push_str("void 0");
            } else {
                buf.push_str(dir.value);
            }
        }
        if !dir.arg.is_empty() || !dir.modifiers.is_empty() {
            buf.push_str(", ");
            if dir.arg.is_empty() {
                buf.push_str("void 0");
            } else {
                buf.push_str(dir.arg);
            }
        }
        if !dir.modifiers.is_empty() {
            buf.push_str(", ");
            buf.push_str(dir.modifiers);
        }
        buf.push(']');
    }
    buf.push_str("])");

    let s = code_transform.alloc_str(buf);
    pending_append_lefts.push((close_pos, s));
}

/// Close a self-closing/void element (e.g., `<br/>`, `<img/>`).
///
/// Void elements never receive an `OxcCompiledElementClosed` event,
/// so this is called inline from `handle_element_start`.
pub(crate) fn handle_element_close_self_closing<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    state: &StateStack<'alloc>,
    is_production: bool,
    pending_append_lefts: &mut Vec<(u32, &'alloc str)>,
    buf: &mut String,
) {
    // Fast path: use &'static str for common close strings (no patch flags).
    let s: &'alloc str = if state.patch_flag.0 == 0 {
        if state.is_block_root {
            "))"
        } else {
            ")"
        }
    } else {
        buf.clear();
        write_patch_flag_suffix(buf, state.patch_flag, &state.dynamic_props, is_production);
        buf.push(')');
        if state.is_block_root {
            buf.push(')');
        }
        code_transform.alloc_str(buf)
    };
    pending_append_lefts.push((state.open_tag_end, s));

    // withDirectives wrapping for self-closing elements (e.g., <input v-model="msg" />)
    emit_with_directives(
        code_transform,
        state,
        state.open_tag_end,
        pending_append_lefts,
        buf,
    );
}
