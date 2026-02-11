use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        binding_types::BindingType,
        plugin::SyntaxPluginContext,
        plugins::code_gen::{
            template::helper::{
                build_patch_flag_suffix, capitalize_first, escape_js_string, patch_bindings,
            },
            types::TemplateImportDependencies,
        },
        types::{OxcCompiledElementClosed, OxcCompiledElementStart, PropKind},
    },
    utils::vue::PatchFlags,
};

use super::{ChildKind, StateStack};

/// Process the opening of an element.
///
/// Transforms the open tag source (`<tagname ...attrs... >`) into the beginning
/// of a VNode call: `_createElementVNode("tagname", {props}` for native elements,
/// or `_createVNode(TagName, {props}` for components.
///
/// Does NOT add a parent separator — the parent's close phase handles that
/// retroactively via `prepend_left`.
///
/// Mutates `state`: sets `is_component`, `patch_flag`, `dynamic_props`, `open_tag_end`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_element_open<'alloc>(
    code_transform: &mut CodeTransform<'alloc>,
    ev: &OxcCompiledElementStart<'alloc>,
    ctx: &SyntaxPluginContext<'alloc>,
    bindings: &FxHashMap<&'alloc str, BindingType>,
    is_production: bool,
    state: &mut StateStack,
    imports: &mut TemplateImportDependencies,
    resolved_components: &mut Vec<String>,
) {
    let open_tag = &ev.event.event_open_tag;
    let open_tag_end = &ev.event.event_open_tag_end;
    let is_component = open_tag.kind.is_component();

    state.is_component = is_component;
    state.open_tag_end = open_tag_end.end;

    // Inherit patch flags already estimated during syntax parsing
    state.patch_flag = open_tag.patch_flag;

    // Tag name from source bytes
    let tag_name = &ctx.input[open_tag.start as usize + 1..open_tag.name_end as usize];

    // For components, register for _resolveComponent and use the resolved variable name.
    // Vue pattern: const _component_MyComponent = _resolveComponent("MyComponent")
    // Then reference _component_MyComponent in _createBlock/_createVNode calls.
    let component_var = if is_component {
        let var_name = format!("_component_{}", tag_name);
        if !resolved_components.contains(&tag_name.to_string()) {
            resolved_components.push(tag_name.to_string());
            imports.add(TemplateImportDependencies::RESOLVE_COMPONENT);
        }
        Some(var_name)
    } else {
        None
    };

    // Build the VNode call prefix (no parent separator — close phase handles it)
    if state.is_block_root {
        // Block root: (_openBlock(), _createElementBlock("tag" or _createBlock(_component_Tag
        imports.add(TemplateImportDependencies::OPEN_BLOCK);
        if let Some(ref var) = component_var {
            imports.add(TemplateImportDependencies::CREATE_BLOCK);
            code_transform.overwrite(
                open_tag.start,
                open_tag.name_end,
                &format!("(_openBlock(), _createBlock({}", var),
            );
        } else {
            imports.add(TemplateImportDependencies::CREATE_ELEMENT_BLOCK);
            code_transform.overwrite(
                open_tag.start,
                open_tag.name_end,
                &format!("(_openBlock(), _createElementBlock(\"{}\"", tag_name),
            );
        }
    } else if let Some(ref var) = component_var {
        imports.add(TemplateImportDependencies::CREATE_VNODE);
        code_transform.overwrite(
            open_tag.start,
            open_tag.name_end,
            &format!("_createVNode({}", var),
        );
    } else {
        imports.add(TemplateImportDependencies::CREATE_ELEMENT_VNODE);
        code_transform.overwrite(
            open_tag.start,
            open_tag.name_end,
            &format!("_createElementVNode(\"{}\"", tag_name),
        );
    }

    // -- Props --
    if ev.props.is_empty() {
        // Remove the space+`>` region between tag name and open tag end
        // Replace with `, null`
        code_transform.overwrite(open_tag.name_end, open_tag_end.end, ", null");
    } else {
        // Overwrite the space between tag name and first prop with `, {`
        let first_prop_start = ev.props[0].event.start;
        code_transform.overwrite(open_tag.name_end, first_prop_start, ", {");

        for (i, prop) in ev.props.iter().enumerate() {
            let sep = if i > 0 { ", " } else { "" };

            match &prop.event.kind {
                PropKind::Value => {
                    // Static attribute: name="value"
                    let name = &ctx.input[prop.event.start as usize..prop.event.name_end as usize];
                    if let Some(val_span) = prop.event.value {
                        let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                        let escaped = escape_js_string(val);
                        code_transform.overwrite(
                            prop.event.start,
                            prop.event.end,
                            &format!("{}{}: \"{}\"", sep, name, escaped),
                        );
                    } else {
                        code_transform.overwrite(
                            prop.event.start,
                            prop.event.end,
                            &format!("{}{}: \"\"", sep, name),
                        );
                    }
                }

                PropKind::ClassValue => {
                    // Static class: class="foo bar"
                    if let Some(val_span) = prop.event.value {
                        let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                        let escaped = escape_js_string(val);
                        code_transform.overwrite(
                            prop.event.start,
                            prop.event.end,
                            &format!("{}class: \"{}\"", sep, escaped),
                        );
                    }
                }

                PropKind::StyleValue => {
                    // Static style: style="color: red"
                    if let Some(val_span) = prop.event.value {
                        let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                        let escaped = escape_js_string(val);
                        code_transform.overwrite(
                            prop.event.start,
                            prop.event.end,
                            &format!("{}style: \"{}\"", sep, escaped),
                        );
                    }
                }

                PropKind::Bind => {
                    // :prop="expr" → prop_name: expr
                    let prop_name = if let Some(arg_span) = prop.event.arg {
                        ctx.input[arg_span.start as usize..arg_span.end as usize].to_string()
                    } else {
                        "unknown".to_string()
                    };

                    // Patch bindings in the expression
                    if let Some(exp) = &prop.exp {
                        patch_bindings(code_transform, &exp.bindings, bindings, is_production);
                    }

                    if let Some(val_span) = prop.event.value {
                        let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                        code_transform.overwrite(
                            prop.event.start,
                            prop.event.end,
                            &format!("{}{}: {}", sep, prop_name, val),
                        );
                    } else {
                        code_transform.overwrite(
                            prop.event.start,
                            prop.event.end,
                            &format!("{}{}: undefined", sep, prop_name),
                        );
                    }

                    // Track dynamic prop for patch flag
                    state.dynamic_props.push(prop_name);
                    state.patch_flag = state.patch_flag.add(PatchFlags::Props);
                }

                PropKind::On => {
                    // @event="handler" → onEventName: handler
                    let event_name = if let Some(arg_span) = prop.event.arg {
                        let raw = &ctx.input[arg_span.start as usize..arg_span.end as usize];
                        format!("on{}", capitalize_first(raw))
                    } else {
                        "onClick".to_string()
                    };

                    if let Some(exp) = &prop.exp {
                        patch_bindings(code_transform, &exp.bindings, bindings, is_production);
                    }

                    if let Some(val_span) = prop.event.value {
                        let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                        code_transform.overwrite(
                            prop.event.start,
                            prop.event.end,
                            &format!("{}{}: {}", sep, event_name, val),
                        );
                    } else {
                        code_transform.overwrite(
                            prop.event.start,
                            prop.event.end,
                            &format!("{}{}: () => {{}}", sep, event_name),
                        );
                    }
                }

                PropKind::ClassBind => {
                    // :class="expr" → class: _normalizeClass(expr)
                    imports.add(TemplateImportDependencies::NORMALIZE_CLASS);

                    if let Some(exp) = &prop.exp {
                        patch_bindings(code_transform, &exp.bindings, bindings, is_production);
                    }

                    if let Some(val_span) = prop.event.value {
                        let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                        code_transform.overwrite(
                            prop.event.start,
                            prop.event.end,
                            &format!("{}class: _normalizeClass({})", sep, val),
                        );
                    }

                    state.patch_flag = state.patch_flag.add(PatchFlags::Class);
                }

                PropKind::StyleBind => {
                    // :style="expr" → style: _normalizeStyle(expr)
                    imports.add(TemplateImportDependencies::NORMALIZE_STYLE);

                    if let Some(exp) = &prop.exp {
                        patch_bindings(code_transform, &exp.bindings, bindings, is_production);
                    }

                    if let Some(val_span) = prop.event.value {
                        let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                        code_transform.overwrite(
                            prop.event.start,
                            prop.event.end,
                            &format!("{}style: _normalizeStyle({})", sep, val),
                        );
                    }

                    state.patch_flag = state.patch_flag.add(PatchFlags::Style);
                }

                // TODO: Model, Show, Html, Text, Directive, BindSpread, OnSpread
                _ => {
                    // Remove unhandled props for now
                    code_transform.overwrite(prop.event.start, prop.event.end, "");
                }
            }
        }

        // Close the props object: overwrite `>` with `}`
        let last_prop_end = ev.props.last().unwrap().event.end;
        code_transform.overwrite(last_prop_end, open_tag_end.end, "}");
    }
}

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
/// Then emits patch flags, dynamic props, and closing paren.
pub(crate) fn handle_element_close(
    code_transform: &mut CodeTransform,
    ev: &OxcCompiledElementClosed,
    state: &StateStack,
    is_production: bool,
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

    // Insert separators + scope prefix + child content prefixes based on children strategy.
    //
    // Each child's content_prefix() returns the text that needs to appear
    // immediately before the child's overwritten/original content:
    //   Text → `"` (opening quote; closing quote already added by text handler)
    //   Interpolation → `_toDisplayString` (function name; overwrites handle parens)
    //   Element/Comment → `` (overwrite already includes the full call prefix)
    //
    // Each child's scope_prefix contains any v-if/v-for prefix (e.g. `(show) ? `).
    //
    // We combine separator + scope_prefix + content_prefix into a single prepend_left
    // to ensure correct ordering (separator before scope before content).
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

    if let Some(close_tag) = &ev.event.event_close_tag {
        // Overwrite `</tagname>` with the close string
        code_transform.overwrite(close_tag.start, close_tag.end, &close_str);
    } else {
        // Non-void element without close tag (shouldn't normally happen)
        code_transform.append_left(state.open_tag_end, &close_str);
    }
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
}
