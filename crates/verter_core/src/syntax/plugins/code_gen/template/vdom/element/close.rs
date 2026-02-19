use crate::{
    code_transform::CodeTransform,
    syntax::{
        plugins::code_gen::{
            template::{
                shared::helper::is_valid_js_prop_key, vdom::helper::write_patch_flag_suffix,
            },
            types::TemplateImportDependencies,
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
    parent_any_dynamic_slots: bool,
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
    //
    // The name format prefix is emitted as a SEPARATE prepend_left entry so it
    // can be retroactively patched from static to dynamic format if a later
    // sibling named slot has v-if (which sets parent.any_dynamic_slots = true
    // after this slot has already closed).
    //
    // Static slots (parent_any_dynamic_slots=false):
    //   `name: _withCtx((params) => [children])`
    //   Parent wraps all entries in `{ ... _: 1 }`.
    //
    // Dynamic slots (parent_any_dynamic_slots=true):
    //   `{ name: "slotName", fn: _withCtx((params) => [children]) }`
    //   Parent wraps in `_createSlots({ _: 2 }, [...])`.
    if state.is_named_slot_template {
        let has_children = !state.children.is_empty();
        let params = state.slot_params.unwrap_or("");
        let slot_key = state.slot_name.unwrap_or("default");
        let is_dynamic_name = slot_key.starts_with('[');
        let needs_slot_quote = !is_dynamic_name && !is_valid_js_prop_key(slot_key);

        // Build name format prefix — emitted as a separate entry for retroactive patching.
        let name_format: &'alloc str = if parent_any_dynamic_slots {
            build_dynamic_slot_name_format(code_transform, slot_key, is_dynamic_name, buf)
        } else {
            build_static_slot_name_format(code_transform, slot_key, needs_slot_quote, buf)
        };

        // Build _withCtx open (format-independent: `_withCtx((params) => [`)
        buf.clear();
        buf.push_str("_withCtx(");
        if !params.is_empty() {
            buf.push('(');
            buf.push_str(params);
            buf.push(')');
        } else {
            buf.push_str("()");
        }
        buf.push_str(" => [");
        let withctx_open = code_transform.alloc_str(buf);

        // Closing suffix: `])` for static, `]) }` for dynamic (_createSlots format)
        let slot_close_suffix = if parent_any_dynamic_slots {
            "]) }"
        } else {
            "])"
        };

        // Position where the name format prepend_left will be added.
        // This is the position of the first child (or close tag for empty slots).
        let name_format_pos: u32;

        if has_children {
            let all_text_like = state
                .children
                .iter()
                .all(|c| matches!(c.kind, ChildKind::Text | ChildKind::Interpolation));
            let has_interpolation = state
                .children
                .iter()
                .any(|c| c.kind == ChildKind::Interpolation);

            name_format_pos = state.children[0].start;

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

                // Entry 1: name format (patchable)
                pending_prepend_lefts.push((first_child.start, name_format));
                // Entry 2: _withCtx + children prefix
                buf.clear();
                buf.push_str(withctx_open);
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
                buf.push(')'); // close _createTextVNode(
                buf.push_str(slot_close_suffix);
            } else {
                // Mixed or element-only children
                for (i, child) in state.children.iter().enumerate() {
                    buf.clear();
                    if i == 0 {
                        // Entry 1: name format (patchable)
                        pending_prepend_lefts.push((child.start, name_format));
                        // Entry 2: _withCtx + first child prefix
                        buf.push_str(withctx_open);
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
                buf.push_str(slot_close_suffix);
            }
        } else {
            // Empty slot: name_format + withctx_open + close in one overwrite/append
            name_format_pos = ev
                .event
                .event_close_tag
                .as_ref()
                .map(|c| c.start)
                .unwrap_or(state.open_tag_end);

            // Entry 1: name format (patchable) at the close tag position
            pending_prepend_lefts.push((name_format_pos, name_format));

            buf.clear();
            buf.push_str(withctx_open);
            buf.push_str(slot_close_suffix);
        }

        if let Some(close_tag) = &ev.event.event_close_tag {
            let s = code_transform.alloc_str(buf);
            pending_overwrites.push((close_tag.start, close_tag.end, s));
        } else {
            let s = code_transform.alloc_str(buf);
            pending_append_lefts.push((state.open_tag_end, s));
        }

        // Store the name format prepend index for retroactive patching.
        // The caller (mod.rs) will save this on the parent's ChildInfo.
        // We use the `name_format_pos` field of `buf` indirectly — the index
        // is computed by the caller from pending_prepend_lefts length.
        let _ = name_format_pos; // used above

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
    //
    // Static slots (any_dynamic_slots=false):
    //   `{ first: _withCtx(...), second: _withCtx(...), _: 1 /* STABLE */ }`
    //
    // Dynamic slots (any_dynamic_slots=true, e.g. v-if on named slots):
    //   `_createSlots({ _: 2 /* DYNAMIC */ }, [entries...])`
    //   where entries are `{ name: "x", fn: _withCtx(...) }` or ternary chains.
    //
    // When a component has BOTH named slot templates and non-named-slot content,
    // the non-named-slot children must be wrapped in `default: _withCtx(() => [...])`.
    // Named slot children already emit their own entry during their close phase.
    if state.has_named_slot_children && has_children {
        imports.add(TemplateImportDependencies::WITH_CTX);

        // Check if any children are NOT named slot templates.
        let has_implicit_default = state.children.iter().any(|c| !c.is_named_slot);

        if state.any_dynamic_slots {
            // Dynamic slots: use _createSlots({ _: 2 }, [entries...])
            imports.add(TemplateImportDependencies::CREATE_SLOTS);

            // Retroactively patch any named slot entries that were emitted in static
            // format before any_dynamic_slots was set (e.g. #content closed before
            // a sibling v-if #default opened and set the flag).
            for child in state.children.iter() {
                if let Some(prefix_idx) = child.slot_format_prepend_idx {
                    // Replace static name format (`slotName: `) with dynamic format
                    // (`{ name: "slotName", fn: `) in the already-emitted prepend_left.
                    let (pos, _old_prefix) = pending_prepend_lefts[prefix_idx];
                    let is_dynamic_name = child.slot_name.starts_with('[');
                    let new_prefix = build_dynamic_slot_name_format(
                        code_transform,
                        child.slot_name,
                        is_dynamic_name,
                        buf,
                    );
                    pending_prepend_lefts[prefix_idx] = (pos, new_prefix);
                    // Append ` }` after the slot close to complete `{ name, fn: ... }`
                    pending_append_lefts.push((child.slot_close_tag_end, " }"));
                }
            }

            if !has_implicit_default {
                // All children are named slot templates — all go in the array.
                for (i, child) in state.children.iter().enumerate() {
                    let sep = if i == 0 {
                        ", _createSlots({ _: 2 /* DYNAMIC */ }, ["
                    } else {
                        ", "
                    };
                    let s = child_separator_str(code_transform, sep, child, buf);
                    pending_prepend_lefts.push((child.start, s));
                }
            } else {
                // Mixed: default slot children + dynamic named slots.
                // Default children → `{ name: "default", fn: _withCtx(() => [...]) }`
                // Named slot children already emit `{ name: "x", fn: ... }` format.
                let mut first_in_array = true;
                let mut in_default_slot = false;

                for child in state.children.iter() {
                    if child.is_named_slot {
                        if in_default_slot {
                            // Close default slot, then separator for named slot
                            buf.clear();
                            buf.push_str("]) }, ");
                            buf.push_str(child.scope_prefix);
                            buf.push_str(child.kind.content_prefix());
                            let s = code_transform.alloc_str(buf);
                            pending_prepend_lefts.push((child.start, s));
                            in_default_slot = false;
                        } else {
                            let sep = if first_in_array {
                                ", _createSlots({ _: 2 /* DYNAMIC */ }, ["
                            } else {
                                ", "
                            };
                            let s = child_separator_str(code_transform, sep, child, buf);
                            pending_prepend_lefts.push((child.start, s));
                        }
                        first_in_array = false;
                    } else {
                        // Implicit default slot content
                        if !in_default_slot {
                            buf.clear();
                            if first_in_array {
                                buf.push_str(
                                    ", _createSlots({ _: 2 /* DYNAMIC */ }, [{ name: \"default\", fn: _withCtx(() => [",
                                );
                            } else {
                                buf.push_str(", { name: \"default\", fn: _withCtx(() => [");
                            }
                            buf.push_str(child.scope_prefix);
                            buf.push_str(child.kind.content_prefix());
                            let s = code_transform.alloc_str(buf);
                            pending_prepend_lefts.push((child.start, s));
                            first_in_array = false;
                            in_default_slot = true;
                        } else {
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

            // Build close: `])`
            buf.clear();
            if has_implicit_default {
                let last_is_default = state
                    .children
                    .last()
                    .map(|c| !c.is_named_slot)
                    .unwrap_or(false);
                if last_is_default {
                    buf.push_str("]) }");
                }
            }
            buf.push_str("])");
        } else {
            // Static slots: use plain object `{ name: _withCtx(...), _: 1 }`
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
                let mut first_in_object = true;
                let mut in_default_slot = false;

                for child in state.children.iter() {
                    if child.is_named_slot {
                        if in_default_slot {
                            buf.clear();
                            buf.push_str("]), ");
                            buf.push_str(child.scope_prefix);
                            buf.push_str(child.kind.content_prefix());
                            let s = code_transform.alloc_str(buf);
                            pending_prepend_lefts.push((child.start, s));
                            in_default_slot = false;
                        } else {
                            let sep = if first_in_object { ", {" } else { ", " };
                            let s = child_separator_str(code_transform, sep, child, buf);
                            pending_prepend_lefts.push((child.start, s));
                        }
                        first_in_object = false;
                    } else if !in_default_slot {
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
                        buf.clear();
                        buf.push_str(", ");
                        buf.push_str(child.scope_prefix);
                        buf.push_str(child.kind.content_prefix());
                        let s = code_transform.alloc_str(buf);
                        pending_prepend_lefts.push((child.start, s));
                    }
                }
            }

            // Build close: `, _: flag})`
            let slot_flag = if is_production { "1" } else { "1 /* STABLE */" };
            buf.clear();
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
        }
        write_patch_flag_suffix(buf, patch_flag, &state.dynamic_props, is_production);
        buf.push(')');
        if state.is_block_root {
            buf.push(')');
        }

        let close_pos = if let Some(close_tag) = &ev.event.event_close_tag {
            let s = code_transform.alloc_str(buf);
            pending_overwrites.push((close_tag.start, close_tag.end, s));
            close_tag.end
        } else {
            let s = code_transform.alloc_str(buf);
            pending_append_lefts.push((state.open_tag_end, s));
            state.open_tag_end
        };
        emit_with_directives(code_transform, state, close_pos, pending_append_lefts, buf);
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

        // Slot names with non-identifier characters (hyphens, colons, etc.) must be quoted.
        let needs_slot_quote = !slot_key.starts_with('[') && !is_valid_js_prop_key(slot_key);

        // Build slot_open string
        buf.clear();
        buf.push_str(", {");
        if needs_slot_quote {
            buf.push('"');
        }
        buf.push_str(slot_key);
        if needs_slot_quote {
            buf.push('"');
        }
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

            let close_pos = if let Some(close_tag) = &ev.event.event_close_tag {
                let s = code_transform.alloc_str(buf);
                pending_overwrites.push((close_tag.start, close_tag.end, s));
                close_tag.end
            } else {
                let s = code_transform.alloc_str(buf);
                pending_append_lefts.push((state.open_tag_end, s));
                state.open_tag_end
            };
            emit_with_directives(code_transform, state, close_pos, pending_append_lefts, buf);
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

            let close_pos = if let Some(close_tag) = &ev.event.event_close_tag {
                let s = code_transform.alloc_str(buf);
                pending_overwrites.push((close_tag.start, close_tag.end, s));
                close_tag.end
            } else {
                let s = code_transform.alloc_str(buf);
                pending_append_lefts.push((state.open_tag_end, s));
                state.open_tag_end
            };
            emit_with_directives(code_transform, state, close_pos, pending_append_lefts, buf);
            return;
        }
    }

    // Normal (non-slot) children handling.
    // Components without explicit v-slot still need their children wrapped in a
    // default slot function: `{ default: _withCtx(() => [...]), _: 1 }`.
    // Native elements emit children as direct args (text concat or array).
    if has_children && state.is_component {
        // Component implicit default slot: wrap in _withCtx function.
        imports.add(TemplateImportDependencies::WITH_CTX);

        let slot_stable = if is_production { "1" } else { "1 /* STABLE */" };

        if all_text_like {
            // Text-only default slot: _createTextVNode inside _withCtx
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
            buf.push_str(", {default: _withCtx(() => [_createTextVNode(");
            buf.push_str(first_child.kind.content_prefix());
            let s = code_transform.alloc_str(buf);
            pending_prepend_lefts.push((first_child.start, s));

            for child in state.children.iter().skip(1) {
                let prefix = child.kind.content_prefix();
                buf.clear();
                buf.push_str(" + ");
                buf.push_str(prefix);
                let s = code_transform.alloc_str(buf);
                pending_prepend_lefts.push((child.start, s));
            }

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

            let close_pos = if let Some(close_tag) = &ev.event.event_close_tag {
                let s = code_transform.alloc_str(buf);
                pending_overwrites.push((close_tag.start, close_tag.end, s));
                close_tag.end
            } else {
                let s = code_transform.alloc_str(buf);
                pending_append_lefts.push((state.open_tag_end, s));
                state.open_tag_end
            };
            emit_with_directives(code_transform, state, close_pos, pending_append_lefts, buf);
            return;
        } else {
            // Mixed/element default slot content: each child is a separate array entry
            // inside `{ default: _withCtx(() => [...]), _: STABLE }`.
            // Text/interpolation runs must be wrapped in _createTextVNode — bare strings
            // and _toDisplayString results are not valid VNodes in slot arrays.
            let slot_open = {
                buf.clear();
                buf.push_str(", {default: _withCtx(() => [");
                code_transform.alloc_str(buf)
            };

            emit_mixed_children_with_prefix(
                code_transform,
                state,
                is_production,
                imports,
                pending_prepend_lefts,
                pending_append_lefts,
                buf,
                slot_open,
            );

            buf.clear();
            buf.push_str("]), _: ");
            buf.push_str(slot_stable);
            buf.push('}');
            write_patch_flag_suffix(buf, patch_flag, &state.dynamic_props, is_production);
            buf.push(')');
            if state.is_block_root {
                buf.push(')');
            }

            let close_pos = if let Some(close_tag) = &ev.event.event_close_tag {
                let s = code_transform.alloc_str(buf);
                pending_overwrites.push((close_tag.start, close_tag.end, s));
                close_tag.end
            } else {
                let s = code_transform.alloc_str(buf);
                pending_append_lefts.push((state.open_tag_end, s));
                state.open_tag_end
            };
            emit_with_directives(code_transform, state, close_pos, pending_append_lefts, buf);
            return;
        }
    } else if has_children {
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
///
/// `first_prefix` controls the separator for the very first child:
/// - For element children: `", ["` (standard array open)
/// - For slot children: `", {default: _withCtx(() => ["` (slot wrapper)
#[allow(clippy::too_many_arguments)]
fn emit_mixed_children_impl<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    state: &StateStack<'alloc>,
    is_production: bool,
    imports: &mut TemplateImportDependencies,
    pending_prepend_lefts: &mut Vec<(u32, &'alloc str)>,
    pending_append_lefts: &mut Vec<(u32, &'alloc str)>,
    buf: &mut String,
    first_prefix: &'alloc str,
) {
    let children = &state.children;
    let mut i = 0;
    // Track whether we've emitted the first child (for custom prefix vs `, ` separator).
    let mut is_first_in_array = true;

    while i < children.len() {
        let child = &children[i];
        let is_text_like = matches!(child.kind, ChildKind::Text | ChildKind::Interpolation);

        if !is_text_like {
            // Non-text child: emit with normal separator.
            let sep = if is_first_in_array {
                first_prefix
            } else {
                ", "
            };
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
        let array_sep = if is_first_in_array {
            first_prefix
        } else {
            ", "
        };
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

/// Emit mixed children for element arrays (uses `", ["` as first separator).
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
    emit_mixed_children_impl(
        code_transform,
        state,
        is_production,
        imports,
        pending_prepend_lefts,
        pending_append_lefts,
        buf,
        ", [",
    );
}

/// Emit mixed children for component implicit default slots.
/// Uses a custom `first_prefix` (e.g. `", {default: _withCtx(() => ["`) instead of `", ["`.
#[allow(clippy::too_many_arguments)]
fn emit_mixed_children_with_prefix<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    state: &StateStack<'alloc>,
    is_production: bool,
    imports: &mut TemplateImportDependencies,
    pending_prepend_lefts: &mut Vec<(u32, &'alloc str)>,
    pending_append_lefts: &mut Vec<(u32, &'alloc str)>,
    buf: &mut String,
    first_prefix: &'alloc str,
) {
    emit_mixed_children_impl(
        code_transform,
        state,
        is_production,
        imports,
        pending_prepend_lefts,
        pending_append_lefts,
        buf,
        first_prefix,
    );
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

/// Build the static slot name format: `slotName: ` or `"slot-name": `.
fn build_static_slot_name_format<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    slot_key: &str,
    needs_quote: bool,
    buf: &mut String,
) -> &'alloc str {
    buf.clear();
    if needs_quote {
        buf.push('"');
    }
    buf.push_str(slot_key);
    if needs_quote {
        buf.push('"');
    }
    buf.push_str(": ");
    code_transform.alloc_str(buf)
}

/// Build the dynamic slot name format: `{ name: "slotName", fn: ` or `{ name: [expr], fn: `.
pub(crate) fn build_dynamic_slot_name_format<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    slot_key: &str,
    is_dynamic_name: bool,
    buf: &mut String,
) -> &'alloc str {
    buf.clear();
    buf.push_str("{ name: ");
    if is_dynamic_name {
        buf.push_str(slot_key);
    } else {
        buf.push('"');
        buf.push_str(slot_key);
        buf.push('"');
    }
    buf.push_str(", fn: ");
    code_transform.alloc_str(buf)
}
