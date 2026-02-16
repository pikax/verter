use crate::{
    code_transform::CodeTransform,
    syntax::{
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
    // Slot outlets: _renderSlot($slots, "name") — just close the paren.
    // With fallback content: _renderSlot($slots, "name", {}, () => [children])
    if state.is_slot_outlet {
        if state.children.is_empty() {
            // No fallback content — just close the paren
            if let Some(close_tag) = &ev.event.event_close_tag {
                pending_overwrites.push((close_tag.start, close_tag.end, ")"));
            } else {
                pending_append_lefts.push((state.open_tag_end, ")"));
            }
        } else {
            // Fallback content present — emit: , {}, () => [children])
            let all_text_like = state
                .children
                .iter()
                .all(|c| matches!(c.kind, ChildKind::Text | ChildKind::Interpolation));
            let has_interpolation = state
                .children
                .iter()
                .any(|c| c.kind == ChildKind::Interpolation);

            if all_text_like {
                // Text/interpolation fallback: wrap in _createTextVNode
                imports.add(TemplateImportDependencies::CREATE_TEXT_VNODE);
                let first_child = &state.children[0];
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
                buf.push_str(", {}, () => [_createTextVNode(");
                buf.push_str(first_child.kind.content_prefix());
                let s = code_transform.alloc_str(buf);
                pending_prepend_lefts.push((first_child.start, s));

                for child in state.children.iter().skip(1) {
                    buf.clear();
                    buf.push_str(" + ");
                    buf.push_str(child.kind.content_prefix());
                    let s = code_transform.alloc_str(buf);
                    pending_prepend_lefts.push((child.start, s));
                }

                buf.clear();
                buf.push_str(text_flag);
                buf.push_str(")])");
            } else {
                // Mixed or element-only fallback children
                for (i, child) in state.children.iter().enumerate() {
                    buf.clear();
                    if i == 0 {
                        buf.push_str(", {}, () => [");
                        buf.push_str(child.scope_prefix);
                        buf.push_str(child.kind.content_prefix());
                    } else {
                        buf.push_str(", ");
                        buf.push_str(child.scope_prefix);
                        buf.push_str(child.kind.content_prefix());
                    }
                    let s = code_transform.alloc_str(buf);
                    pending_prepend_lefts.push((child.start, s));
                }

                buf.clear();
                buf.push_str("])");
            }

            if let Some(close_tag) = &ev.event.event_close_tag {
                let s = code_transform.alloc_str(buf);
                pending_overwrites.push((close_tag.start, close_tag.end, s));
            } else {
                let s = code_transform.alloc_str(buf);
                pending_append_lefts.push((state.open_tag_end, s));
            }
        }
        return;
    }

    // Named slot template: <template #name> inside a component.
    // Emits just the slot entry `name: _withCtx((params) => [children])` without
    // VNode wrapper. The parent component will wrap all entries in `{ ... _: 1 }`.
    if state.is_named_slot_template {
        let has_children = !state.children.is_empty();
        let params = state.slot_params.unwrap_or("");
        let slot_key = state.slot_name.unwrap_or("default");

        // Build slot entry prefix: `name: _withCtx((params) => [`
        buf.clear();
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

        if has_children {
            let all_text_like = state
                .children
                .iter()
                .all(|c| matches!(c.kind, ChildKind::Text | ChildKind::Interpolation));
            let has_interpolation = state
                .children
                .iter()
                .any(|c| c.kind == ChildKind::Interpolation);

            if all_text_like {
                imports.add(TemplateImportDependencies::CREATE_TEXT_VNODE);
                let first_child = &state.children[0];
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

                for child in state.children.iter().skip(1) {
                    buf.clear();
                    buf.push_str(" + ");
                    buf.push_str(child.kind.content_prefix());
                    let s = code_transform.alloc_str(buf);
                    pending_prepend_lefts.push((child.start, s));
                }

                buf.clear();
                buf.push_str(text_flag);
                buf.push_str(")])");
            } else {
                // Mixed or element-only children
                for (i, child) in state.children.iter().enumerate() {
                    buf.clear();
                    if i == 0 {
                        buf.push_str(slot_open);
                        buf.push_str(child.scope_prefix);
                        buf.push_str(child.kind.content_prefix());
                    } else {
                        buf.push_str(", ");
                        buf.push_str(child.scope_prefix);
                        buf.push_str(child.kind.content_prefix());
                    }
                    let s = code_transform.alloc_str(buf);
                    pending_prepend_lefts.push((child.start, s));
                }

                buf.clear();
                buf.push_str("])");
            }
        } else {
            buf.clear();
            buf.push_str(slot_open);
            buf.push_str("])");
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
    // Vue's runtime expects non-text children to ALWAYS be array-wrapped.
    // `_createElementVNode("div", props, [child])` — even for a single child.
    // Without the array, `mountChildren` sees `children.length === undefined`
    // and never mounts the child, leaving its `.el` null.
    let needs_array = has_children && !all_text_like;

    // Named slot children: component has <template #name> children.
    // Children become `{ first: _withCtx(...), second: _withCtx(...), _: 1 }`.
    //
    // When a component has BOTH named slot templates (`<template #name>`) and
    // non-named-slot content (e.g., `<template v-if>`, bare elements, text),
    // the non-named-slot children must be wrapped in `default: _withCtx(() => [...])`.
    // Named slot children already emit their own `name: _withCtx(...)` during their
    // own close phase (is_named_slot_template handling above).
    if state.has_named_slot_children && has_children {
        imports.add(TemplateImportDependencies::WITH_CTX);

        // Check if any children are NOT named slot templates.
        let has_implicit_default = state.children.iter().any(|c| !c.is_named_slot);

        if !has_implicit_default {
            // All children are named slot templates — simple case.
            for (i, child) in state.children.iter().enumerate() {
                let sep = if i == 0 { ", {" } else { ", " };
                let s = child_separator_str(code_transform, sep, child, buf);
                pending_prepend_lefts.push((child.start, s));
            }
        } else {
            // Mixed: some children are named slots, some are implicit default slot content.
            // Group non-named-slot children into `default: _withCtx(() => [...])`.
            //
            // Strategy: use the separator before each child to handle transitions.
            // - When entering default slot content: prepend `default: _withCtx(() => [`
            // - When transitioning default→named: prepend `]), ` before the named slot child
            // - When the last child is default: close `])` in the close string
            let mut first_in_object = true;
            let mut in_default_slot = false;

            for child in state.children.iter() {
                if child.is_named_slot {
                    if in_default_slot {
                        // Transition: default slot → named slot.
                        // Close the default slot wrapper and start the named slot separator.
                        buf.clear();
                        buf.push_str("]), ");
                        buf.push_str(child.scope_prefix);
                        buf.push_str(child.kind.content_prefix());
                        let s = code_transform.alloc_str(buf);
                        pending_prepend_lefts.push((child.start, s));
                        in_default_slot = false;
                    } else {
                        // Named slot entry — already emits `name: _withCtx(...)` on its own
                        let sep = if first_in_object { ", {" } else { ", " };
                        let s = child_separator_str(code_transform, sep, child, buf);
                        pending_prepend_lefts.push((child.start, s));
                    }
                    first_in_object = false;
                } else {
                    // Implicit default slot content
                    if !in_default_slot {
                        // Start the default slot wrapper: `default: _withCtx(() => [`
                        buf.clear();
                        if first_in_object {
                            buf.push_str(", {default: _withCtx(() => [");
                        } else {
                            buf.push_str(", default: _withCtx(() => [");
                        }
                        buf.push_str(child.scope_prefix);
                        buf.push_str(child.kind.content_prefix());
                        let s = code_transform.alloc_str(buf);
                        pending_prepend_lefts.push((child.start, s));
                        first_in_object = false;
                        in_default_slot = true;
                    } else {
                        // Continue inside the default slot array
                        buf.clear();
                        buf.push_str(", ");
                        buf.push_str(child.scope_prefix);
                        buf.push_str(child.kind.content_prefix());
                        let s = code_transform.alloc_str(buf);
                        pending_prepend_lefts.push((child.start, s));
                    }
                }
            }
        }

        // Build close: `, _: flag})`
        let slot_flag = if state.any_dynamic_slots {
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
        // If the last child was implicit default slot content, close the _withCtx wrapper
        if has_implicit_default {
            let last_is_default = state
                .children
                .last()
                .map(|c| !c.is_named_slot)
                .unwrap_or(false);
            if last_is_default {
                buf.push_str("])");
            }
        }
        buf.push_str(", _: ");
        buf.push_str(slot_flag);
        buf.push('}');
        write_patch_flag_suffix(buf, patch_flag, &state.dynamic_props, is_production);
        buf.push(')');
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
            write_patch_flag_suffix(buf, patch_flag, &state.dynamic_props, is_production);
            buf.push(')');
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
            buf.push('}');
            write_patch_flag_suffix(buf, patch_flag, &state.dynamic_props, is_production);
            buf.push(')');
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
        } else {
            // Non-text children: always array-wrap (single or multiple).
            // Vue's runtime requires `[child]` even for a single child.
            //
            // Text/interpolation children mixed with elements must be wrapped
            // in `_createTextVNode(...)` so they become proper VNodes. Without
            // this, Vue's optimized `mountChildren` (in block mode) skips
            // `normalizeVNode` and raw strings never mount.
            //
            // Consecutive text-like children form "runs" that are grouped into
            // a single `_createTextVNode("text " + _toDisplayString(expr), 1)`.
            emit_mixed_children(
                code_transform,
                state,
                is_production,
                imports,
                pending_prepend_lefts,
                pending_append_lefts,
                buf,
            );
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
        } else if !has_children {
            // When there are no children but we have patchFlag, emit `null` as
            // the children argument so patchFlag lands in the correct position.
            // e.g. _createVNode(Comp, props, null, 8, ["msg"])
            buf.push_str(", null");
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

/// Emit mixed children with text-like runs wrapped in `_createTextVNode()`.
///
/// Identifies consecutive "runs" of text/interpolation children and wraps each
/// run in `_createTextVNode(...)`. Within a run, children are concatenated with
/// ` + ` (same as the all-text-like path). Runs containing interpolation get
/// a TEXT patch flag (`1`).
///
/// Non-text children (elements, comments) are emitted with normal separators.
#[allow(clippy::too_many_arguments)]
fn emit_mixed_children<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    state: &StateStack<'alloc>,
    is_production: bool,
    imports: &mut TemplateImportDependencies,
    pending_prepend_lefts: &mut Vec<(u32, &'alloc str)>,
    pending_append_lefts: &mut Vec<(u32, &'alloc str)>,
    buf: &mut String,
) {
    let children = &state.children;
    let mut i = 0;
    // Track whether we've emitted the first child (for `, [` vs `, ` separator).
    let mut is_first_in_array = true;

    while i < children.len() {
        let child = &children[i];
        let is_text_like = matches!(child.kind, ChildKind::Text | ChildKind::Interpolation);

        if !is_text_like {
            // Non-text child: emit with normal separator.
            let sep = if is_first_in_array { ", [" } else { ", " };
            let s = child_separator_str(code_transform, sep, child, buf);
            pending_prepend_lefts.push((child.start, s));
            is_first_in_array = false;
            i += 1;
            continue;
        }

        // Start of a text-like run. Find the end of this run.
        let run_start = i;
        let mut run_has_interp = false;
        while i < children.len() {
            let c = &children[i];
            if !matches!(c.kind, ChildKind::Text | ChildKind::Interpolation) {
                break;
            }
            if c.kind == ChildKind::Interpolation {
                run_has_interp = true;
            }
            i += 1;
        }
        let run_end = i; // exclusive

        imports.add(TemplateImportDependencies::CREATE_TEXT_VNODE);

        // Emit the run.
        // First child in the run: separator + `_createTextVNode(` + content_prefix
        let first_child = &children[run_start];
        let array_sep = if is_first_in_array { ", [" } else { ", " };
        buf.clear();
        buf.push_str(array_sep);
        buf.push_str(first_child.scope_prefix);
        buf.push_str("_createTextVNode(");
        buf.push_str(first_child.kind.content_prefix());
        let s = code_transform.alloc_str(buf);
        pending_prepend_lefts.push((first_child.start, s));

        // Remaining children in the run: ` + ` concatenation
        for c in children.iter().take(run_end).skip(run_start + 1) {
            buf.clear();
            buf.push_str(" + ");
            buf.push_str(c.kind.content_prefix());
            let s = code_transform.alloc_str(buf);
            pending_prepend_lefts.push((c.start, s));
        }

        // Close the _createTextVNode call after the last child in the run.
        let last_child = &children[run_end - 1];
        let close_str = if run_has_interp {
            if is_production {
                ", 1)"
            } else {
                ", 1 /* TEXT */)"
            }
        } else {
            ")"
        };
        pending_append_lefts.push((last_child.end, close_str));

        is_first_in_array = false;
    }
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
    // Slot outlets: _renderSlot($slots, "name") — just close the paren.
    if state.is_slot_outlet {
        pending_append_lefts.push((state.open_tag_end, ")"));
        return;
    }

    // Fast path: use &'static str for common close strings (no patch flags).
    let s: &'alloc str = if state.patch_flag.0 == 0 {
        if state.is_block_root {
            "))"
        } else {
            ")"
        }
    } else {
        buf.clear();
        // Self-closing elements never have children. Emit `null` so patchFlag
        // lands in the correct argument position.
        // e.g. _createElementVNode("img", {src: url}, null, 8, ["src"])
        buf.push_str(", null");
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
