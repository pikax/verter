use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        plugins::code_gen::{
            template::vdom::helper::build_patch_flag_suffix, types::TemplateImportDependencies,
        },
        types::OxcCompiledElementClosed,
    },
    utils::vue::PatchFlags,
};

use super::super::{ChildKind, StateStack};

/// Process the closing of an element.
///
/// **Close-phase children logic**: examines `state.children` to retroactively
/// insert separators via `prepend_left`:
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
pub(crate) fn handle_element_close(
    code_transform: &mut CodeTransform,
    ev: &OxcCompiledElementClosed,
    state: &StateStack,
    is_production: bool,
    imports: &mut TemplateImportDependencies,
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
        let params = state.slot_params.as_deref().unwrap_or("");

        // Determine slot name: static ("default", "header", etc.) or dynamic ([expr])
        // Note: for dynamic slots, the arg span already includes brackets `[expr]`
        // from the tokenizer, so no extra wrapping is needed.
        let slot_key = state.slot_name.as_deref().unwrap_or("default").to_string();

        let slot_open = if params.is_empty() {
            format!(", {{{}: _withCtx(() => [", slot_key)
        } else {
            format!(", {{{}: _withCtx(({}) => [", slot_key, params)
        };

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

            code_transform.prepend_left(
                first_child.start,
                &format!(
                    "{}_createTextVNode({}",
                    slot_open,
                    first_child.kind.content_prefix()
                ),
            );

            // Join remaining text/interp children with " + "
            for child in state.children.iter().skip(1) {
                let prefix = child.kind.content_prefix();
                code_transform.prepend_left(child.start, &format!(" + {}", prefix));
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
            let suffix = build_patch_flag_suffix(patch_flag, &state.dynamic_props, is_production);
            let block_close = if state.is_block_root { ")" } else { "" };
            // Target: `TEXT_FLAG) ]) , _: SLOT_STABLE } ) SUFFIX BLOCK_CLOSE`
            //   TEXT_FLAG)  → close _createTextVNode (e.g. `, 1 /* TEXT */`)`)
            //   ])          → close array, close _withCtx
            //   , _: STABLE → slot stability marker
            //   }           → close slot object
            //   )           → close _createVNode
            let mut close_str = String::new();
            close_str.push_str(text_flag);
            close_str.push_str(")]), _: ");
            close_str.push_str(slot_stable);
            close_str.push('}');
            close_str.push(')');
            close_str.push_str(&suffix);
            close_str.push_str(block_close);

            if let Some(close_tag) = &ev.event.event_close_tag {
                code_transform.overwrite(close_tag.start, close_tag.end, &close_str);
            } else {
                code_transform.append_left(state.open_tag_end, &close_str);
            }
            return;
        } else {
            // Mixed or element-only children: each child is a separate array entry
            for (i, child) in state.children.iter().enumerate() {
                let prefix = child.kind.content_prefix();
                let scope = &child.scope_prefix;
                if i == 0 {
                    code_transform
                        .prepend_left(child.start, &format!("{}{}{}", slot_open, scope, prefix));
                } else {
                    code_transform.prepend_left(child.start, &format!(", {}{}", scope, prefix));
                }
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
            let suffix = build_patch_flag_suffix(patch_flag, &state.dynamic_props, is_production);
            let block_close = if state.is_block_root { ")" } else { "" };
            let mut close_str = String::from("]), _: ");
            close_str.push_str(slot_stable);
            close_str.push_str("})");
            close_str.push_str(&suffix);
            close_str.push_str(block_close);

            if let Some(close_tag) = &ev.event.event_close_tag {
                code_transform.overwrite(close_tag.start, close_tag.end, &close_str);
            } else {
                code_transform.append_left(state.open_tag_end, &close_str);
            }
            return;
        }
    }

    // Normal (non-slot) children handling
    if has_children {
        if all_text_like {
            // Concatenation mode: join with " + "
            for (i, child) in state.children.iter().enumerate() {
                let prefix = child.kind.content_prefix();
                let scope = &child.scope_prefix;
                if i == 0 {
                    code_transform.prepend_left(child.start, &format!(", {}{}", scope, prefix));
                } else {
                    code_transform.prepend_left(child.start, &format!(" + {}{}", scope, prefix));
                }
            }
            if has_interpolation {
                patch_flag = patch_flag.add(PatchFlags::Text);
            }
        } else if state.children.len() == 1 {
            // Single non-text child: separator + scope_prefix + content_prefix
            let child = &state.children[0];
            let prefix = child.kind.content_prefix();
            let scope = &child.scope_prefix;
            code_transform.prepend_left(child.start, &format!(", {}{}", scope, prefix));
        } else {
            // Multiple mixed children: array wrapping
            for (i, child) in state.children.iter().enumerate() {
                let prefix = child.kind.content_prefix();
                let scope = &child.scope_prefix;
                if i == 0 {
                    code_transform.prepend_left(child.start, &format!(", [{}{}", scope, prefix));
                } else {
                    code_transform.prepend_left(child.start, &format!(", {}{}", scope, prefix));
                }
            }
        }
    }

    let suffix = build_patch_flag_suffix(patch_flag, &state.dynamic_props, is_production);

    // Build the closing string: optional array close + suffix + closing paren.
    // Block roots need an extra `)` to close the outer `(_openBlock(), ...)` grouping.
    let block_close = if state.is_block_root { ")" } else { "" };
    let close_str = if needs_array {
        format!("]{}{}{}", suffix, ")", block_close)
    } else {
        format!("{}){}", suffix, block_close)
    };

    let close_pos = if let Some(close_tag) = &ev.event.event_close_tag {
        // Overwrite `</tagname>` with the close string
        code_transform.overwrite(close_tag.start, close_tag.end, &close_str);
        close_tag.end
    } else {
        // Non-void element without close tag (shouldn't normally happen)
        code_transform.append_left(state.open_tag_end, &close_str);
        state.open_tag_end
    };

    // withDirectives wrapping for runtime directives (v-model native, v-show, custom)
    emit_with_directives(code_transform, state, close_pos);
}

/// Emit the `, [[...]])` suffix for runtime directives.
fn emit_with_directives(code_transform: &mut CodeTransform, state: &StateStack, close_pos: u32) {
    if state.runtime_directives.is_empty() {
        return;
    }

    let mut dirs = String::from(", [");
    for (i, dir) in state.runtime_directives.iter().enumerate() {
        if i > 0 {
            dirs.push_str(", ");
        }
        dirs.push('[');
        dirs.push_str(&dir.directive);
        if !dir.value.is_empty() || !dir.arg.is_empty() || !dir.modifiers.is_empty() {
            dirs.push_str(", ");
            if dir.value.is_empty() {
                dirs.push_str("void 0");
            } else {
                dirs.push_str(&dir.value);
            }
        }
        if !dir.arg.is_empty() || !dir.modifiers.is_empty() {
            dirs.push_str(", ");
            if dir.arg.is_empty() {
                dirs.push_str("void 0");
            } else {
                dirs.push_str(&dir.arg);
            }
        }
        if !dir.modifiers.is_empty() {
            dirs.push_str(", ");
            dirs.push_str(&dir.modifiers);
        }
        dirs.push(']');
    }
    dirs.push_str("])");

    code_transform.append_left(close_pos, &dirs);
}

/// Close a self-closing/void element (e.g., `<br/>`, `<img/>`).
///
/// Void elements never receive an `OxcCompiledElementClosed` event,
/// so this is called inline from `handle_element_start`.
pub(crate) fn handle_element_close_self_closing(
    code_transform: &mut CodeTransform,
    state: &StateStack,
    is_production: bool,
) {
    let suffix = build_patch_flag_suffix(state.patch_flag, &state.dynamic_props, is_production);
    let block_close = if state.is_block_root { ")" } else { "" };
    let close_str = format!("{}){}", suffix, block_close);
    code_transform.append_left(state.open_tag_end, &close_str);

    // withDirectives wrapping for self-closing elements (e.g., <input v-model="msg" />)
    emit_with_directives(code_transform, state, state.open_tag_end);
}
