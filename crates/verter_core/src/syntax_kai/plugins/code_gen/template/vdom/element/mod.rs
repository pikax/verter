mod close;
pub(crate) use close::{handle_element_close, handle_element_close_self_closing};

use oxc_ast::ast::Expression;
use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        binding_types::BindingType,
        plugin::SyntaxPluginContext,
        plugins::code_gen::{
            template::shared::helper::{
                apply_dynamic_arg_prefix, build_prefixed_value, capitalize_first, escape_js_string,
                patch_bindings,
            },
            types::TemplateImportDependencies,
        },
        types::{OxcCompiledElementStart, PropKind},
    },
    utils::vue::PatchFlags,
};

use super::{DirectiveEntry, StateStack};

/// Check whether an event handler expression needs wrapping in `$event => (...)`.
///
/// Vue wraps expressions that are NOT simple identifiers, member accesses,
/// arrow functions, or function expressions. Call expressions like `fn($event)`
/// need wrapping because they execute immediately rather than deferring.
fn needs_event_handler_wrap(exp: &Option<oxc_ast::ast::Expression>) -> bool {
    match exp {
        None => false,
        Some(expr) => !matches!(
            expr,
            Expression::Identifier(_)
                | Expression::StaticMemberExpression(_)
                | Expression::ComputedMemberExpression(_)
                | Expression::ArrowFunctionExpression(_)
                | Expression::FunctionExpression(_)
        ),
    }
}

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
    resolved_directives: &mut Vec<String>,
    hoisted_constants: &mut Vec<String>,
) {
    let open_tag = &ev.event.event_open_tag;
    let open_tag_end = &ev.event.event_open_tag_end;
    let is_component = open_tag.kind.is_component();

    state.is_component = is_component;
    state.open_tag_start = open_tag.start;
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

    // Pre-scan: will this element need _withDirectives() wrapping?
    // Native v-model, v-show, and custom directives all produce runtime directives.
    // Component v-model is prop-based, not directive-based.
    let needs_with_directives = !is_component
        && ev.props.iter().any(|p| {
            matches!(
                p.event.kind,
                PropKind::Model | PropKind::Show | PropKind::Directive
            )
        });
    let wd_prefix = if needs_with_directives {
        "_withDirectives("
    } else {
        ""
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
                &format!("{}(_openBlock(), _createBlock({}", wd_prefix, var),
            );
        } else {
            imports.add(TemplateImportDependencies::CREATE_ELEMENT_BLOCK);
            code_transform.overwrite(
                open_tag.start,
                open_tag.name_end,
                &format!(
                    "{}(_openBlock(), _createElementBlock(\"{}\"",
                    wd_prefix, tag_name
                ),
            );
        }
    } else if let Some(ref var) = component_var {
        imports.add(TemplateImportDependencies::CREATE_VNODE);
        code_transform.overwrite(
            open_tag.start,
            open_tag.name_end,
            &format!("{}_createVNode({}", wd_prefix, var),
        );
    } else {
        imports.add(TemplateImportDependencies::CREATE_ELEMENT_VNODE);
        code_transform.overwrite(
            open_tag.start,
            open_tag.name_end,
            &format!("{}_createElementVNode(\"{}\"", wd_prefix, tag_name),
        );
    }

    // -- Props --
    let vif_key_prop = state.vif_branch_key.map(|k| format!("key: {}", k));

    if ev.props.is_empty() {
        // Replace with `, null` or `, { key: N }` for v-if branches
        if let Some(ref key_prop) = vif_key_prop {
            code_transform.overwrite(
                open_tag.name_end,
                open_tag_end.end,
                &format!(", {{ {} }}", key_prop),
            );
        } else {
            code_transform.overwrite(open_tag.name_end, open_tag_end.end, ", null");
        }
    } else {
        state.has_props = true;

        // Check if ALL props are static (hoistable).
        // Static prop kinds: Value, ClassValue, StyleValue.
        // Components never get props hoisted (Vue rule).
        let all_static = !is_component
            && ev.props.iter().all(|p| {
                matches!(
                    p.event.kind,
                    PropKind::Value | PropKind::ClassValue | PropKind::StyleValue
                )
            });

        if all_static {
            // Build the props object string for hoisting.
            let mut props_str = String::from("{ ");
            for (i, prop) in ev.props.iter().enumerate() {
                if i > 0 {
                    props_str.push_str(", ");
                }
                match &prop.event.kind {
                    PropKind::ClassValue => {
                        if let Some(val_span) = prop.event.value {
                            let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                            props_str.push_str(&format!("class: \"{}\"", escape_js_string(val)));
                        }
                    }
                    PropKind::StyleValue => {
                        if let Some(val_span) = prop.event.value {
                            let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                            props_str.push_str(&format!("style: \"{}\"", escape_js_string(val)));
                        }
                    }
                    PropKind::Value => {
                        let name =
                            &ctx.input[prop.event.start as usize..prop.event.name_end as usize];
                        if let Some(val_span) = prop.event.value {
                            let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                            props_str.push_str(&format!("{}: \"{}\"", name, escape_js_string(val)));
                        } else {
                            props_str.push_str(&format!("{}: \"\"", name));
                        }
                    }
                    _ => unreachable!("all_static check guarantees only static prop kinds"),
                }
            }
            props_str.push_str(" }");

            // Add to hoisted constants and emit reference.
            hoisted_constants.push(props_str);
            let hoist_id = hoisted_constants.len(); // 1-indexed
            state.has_all_static_props = true;

            // Overwrite entire props region (from after tag name to open_tag_end)
            // with `, _hoisted_N`
            code_transform.overwrite(
                open_tag.name_end,
                open_tag_end.end,
                &format!(", _hoisted_{}", hoist_id),
            );
        } else {
            state.has_all_static_props = false;

            // Pre-scan for class/style merging:
            // When both static (ClassValue/StyleValue) and dynamic (ClassBind/StyleBind)
            // exist, Vue merges them: class: _normalizeClass(["static", dynamic])
            let static_class: Option<String> = ev.props.iter().find_map(|p| {
                if p.event.kind == PropKind::ClassValue {
                    p.event
                        .value
                        .map(|v| escape_js_string(&ctx.input[v.start as usize..v.end as usize]))
                } else {
                    None
                }
            });
            let has_class_bind = ev.props.iter().any(|p| p.event.kind == PropKind::ClassBind);
            let merge_class = static_class.is_some() && has_class_bind;

            let static_style: Option<String> = ev.props.iter().find_map(|p| {
                if p.event.kind == PropKind::StyleValue {
                    p.event
                        .value
                        .map(|v| escape_js_string(&ctx.input[v.start as usize..v.end as usize]))
                } else {
                    None
                }
            });
            let has_style_bind = ev.props.iter().any(|p| p.event.kind == PropKind::StyleBind);
            let merge_style = static_style.is_some() && has_style_bind;

            // Check if all props are spread-like (BindSpread/OnSpread).
            // Spread props replace the entire props object (no `{}`).
            let is_spread_only = ev
                .props
                .iter()
                .all(|p| matches!(p.event.kind, PropKind::BindSpread | PropKind::OnSpread));

            // Normal inline props processing
            let first_prop_start = ev.props[0].event.start;
            if is_spread_only {
                code_transform.overwrite(open_tag.name_end, first_prop_start, ", ");
            } else if let Some(ref key_prop) = vif_key_prop {
                // Inject v-if branch key at the beginning of the props object
                code_transform.overwrite(
                    open_tag.name_end,
                    first_prop_start,
                    &format!(", {{{}, ", key_prop),
                );
            } else {
                code_transform.overwrite(open_tag.name_end, first_prop_start, ", {");
            }

            // Track how many props we've actually written (for separator logic).
            // Props that are skipped (e.g. ClassValue when merging) don't count.
            let mut written: usize = 0;

            for prop in ev.props.iter() {
                // When merging, skip the static ClassValue/StyleValue — they're folded
                // into the ClassBind/StyleBind handler below.
                if merge_class && prop.event.kind == PropKind::ClassValue {
                    code_transform.overwrite(prop.event.start, prop.event.end, "");
                    continue;
                }
                if merge_style && prop.event.kind == PropKind::StyleValue {
                    code_transform.overwrite(prop.event.start, prop.event.end, "");
                    continue;
                }

                let sep = if written > 0 { ", " } else { "" };

                match &prop.event.kind {
                    PropKind::Value => {
                        // Static attribute: name="value"
                        let name =
                            &ctx.input[prop.event.start as usize..prop.event.name_end as usize];
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
                        written += 1;
                    }

                    PropKind::ClassValue => {
                        // Static class (no merging — merge_class is false here)
                        if let Some(val_span) = prop.event.value {
                            let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                            let escaped = escape_js_string(val);
                            code_transform.overwrite(
                                prop.event.start,
                                prop.event.end,
                                &format!("{}class: \"{}\"", sep, escaped),
                            );
                        }
                        written += 1;
                    }

                    PropKind::StyleValue => {
                        // Static style (no merging — merge_style is false here)
                        if let Some(val_span) = prop.event.value {
                            let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                            let escaped = escape_js_string(val);
                            code_transform.overwrite(
                                prop.event.start,
                                prop.event.end,
                                &format!("{}style: \"{}\"", sep, escaped),
                            );
                        }
                        written += 1;
                    }

                    PropKind::Bind => {
                        // :prop="expr" → prop_name: expr
                        let prop_name = if let Some(arg_span) = prop.event.arg {
                            let raw = ctx.input[arg_span.start as usize..arg_span.end as usize]
                                .to_string();
                            if prop.event.has_dynamic_arg {
                                // Dynamic arg: :[foo]="value" → [_ctx.foo]: value
                                apply_dynamic_arg_prefix(
                                    &raw,
                                    arg_span.start,
                                    &prop.arg.as_ref().and_then(|a| a.bindings.clone()),
                                    bindings,
                                    is_production,
                                )
                            } else {
                                raw
                            }
                        } else {
                            "unknown".to_string()
                        };

                        if let Some(val_span) = prop.event.value {
                            code_transform.overwrite(
                                prop.event.start,
                                val_span.start,
                                &format!("{}{}: ", sep, prop_name),
                            );
                            code_transform.overwrite(val_span.end, prop.event.end, "");

                            if let Some(exp) = &prop.exp {
                                patch_bindings(
                                    code_transform,
                                    &exp.bindings,
                                    bindings,
                                    is_production,
                                );
                            }
                        } else {
                            code_transform.overwrite(
                                prop.event.start,
                                prop.event.end,
                                &format!("{}{}: undefined", sep, prop_name),
                            );
                        }

                        // If this is a :key prop inside a v-for, mark the fragment as keyed
                        if prop_name == "key" {
                            for close in state.pending_scope_closes.iter_mut() {
                                if let super::ScopeClose::For { is_keyed } = close {
                                    *is_keyed = true;
                                }
                            }
                        }

                        state.dynamic_props.push(prop_name);
                        state.patch_flag = state.patch_flag.add(PatchFlags::Props);
                        written += 1;
                    }

                    PropKind::On => {
                        // @event="handler" → onEventName: handler
                        // Modifiers: .stop/.prevent → _withModifiers, .enter etc → _withKeys
                        // .once/.capture/.passive → compile-time event name suffix
                        let raw_event = if let Some(arg_span) = prop.event.arg {
                            ctx.input[arg_span.start as usize..arg_span.end as usize].to_string()
                        } else {
                            "click".to_string()
                        };

                        // Classify modifiers
                        let modifiers: Vec<&str> = prop
                            .event
                            .modifiers
                            .as_ref()
                            .map(|m| {
                                m.iter()
                                    .map(|s| &ctx.input[s.start as usize..s.end as usize])
                                    .collect()
                            })
                            .unwrap_or_default();

                        // Compile-time modifiers: .once, .capture, .passive → event name suffix
                        let mut event_suffix = String::new();
                        let mut runtime_mods: Vec<&str> = Vec::new();
                        let mut key_mods: Vec<&str> = Vec::new();

                        for m in &modifiers {
                            match *m {
                                "once" => event_suffix.push_str("Once"),
                                "capture" => event_suffix.push_str("Capture"),
                                "passive" => event_suffix.push_str("Passive"),
                                // Key modifiers → _withKeys
                                "enter" | "tab" | "delete" | "esc" | "space" | "up" | "down"
                                | "left" | "right" => key_mods.push(m),
                                // Runtime modifiers → _withModifiers
                                _ => runtime_mods.push(m),
                            }
                        }

                        let event_name = if prop.event.has_dynamic_arg {
                            let arg_span = prop.event.arg.unwrap();
                            let raw = &ctx.input[arg_span.start as usize..arg_span.end as usize];
                            let prefixed = apply_dynamic_arg_prefix(
                                raw,
                                arg_span.start,
                                &prop.arg.as_ref().and_then(|a| a.bindings.clone()),
                                bindings,
                                is_production,
                            );
                            let inner = prefixed
                                .strip_prefix('[')
                                .and_then(|s| s.strip_suffix(']'))
                                .unwrap_or(&prefixed);
                            format!("[\"on\" + {}]", inner)
                        } else {
                            format!("on{}{}", capitalize_first(&raw_event), event_suffix)
                        };

                        if let Some(val_span) = prop.event.value {
                            let wrap = prop
                                .exp
                                .as_ref()
                                .map(|e| needs_event_handler_wrap(&e.expression))
                                .unwrap_or(false);

                            // Build handler prefix/suffix based on modifiers
                            let mut handler_prefix = String::new();
                            let mut handler_suffix = String::new();

                            if !runtime_mods.is_empty() {
                                imports.add(TemplateImportDependencies::WITH_MODIFIERS);
                                handler_prefix.push_str("_withModifiers(");
                                handler_suffix.push_str(&format!(
                                    ", [{}])",
                                    runtime_mods
                                        .iter()
                                        .map(|m| format!("\"{}\"", m))
                                        .collect::<Vec<_>>()
                                        .join(",")
                                ));
                            }
                            if !key_mods.is_empty() {
                                imports.add(TemplateImportDependencies::WITH_KEYS);
                                // Wrap around the existing (possibly already wrapped by withModifiers)
                                handler_prefix = format!("_withKeys({}", handler_prefix);
                                handler_suffix.push_str(&format!(
                                    ", [{}])",
                                    key_mods
                                        .iter()
                                        .map(|m| format!("\"{}\"", m))
                                        .collect::<Vec<_>>()
                                        .join(",")
                                ));
                            }

                            if wrap {
                                code_transform.overwrite(
                                    prop.event.start,
                                    val_span.start,
                                    &format!(
                                        "{}{}: {}$event => (",
                                        sep, event_name, handler_prefix
                                    ),
                                );
                                code_transform.overwrite(
                                    val_span.end,
                                    prop.event.end,
                                    &format!("){}", handler_suffix),
                                );
                            } else {
                                code_transform.overwrite(
                                    prop.event.start,
                                    val_span.start,
                                    &format!("{}{}: {}", sep, event_name, handler_prefix),
                                );
                                code_transform.overwrite(
                                    val_span.end,
                                    prop.event.end,
                                    &handler_suffix,
                                );
                            }

                            if let Some(exp) = &prop.exp {
                                patch_bindings(
                                    code_transform,
                                    &exp.bindings,
                                    bindings,
                                    is_production,
                                );
                            }
                        } else {
                            code_transform.overwrite(
                                prop.event.start,
                                prop.event.end,
                                &format!("{}{}: () => {{}}", sep, event_name),
                            );
                        }
                        // Events produce PROPS patch flag with event name in dynamic props.
                        // For dynamic args like @[event], event_name is `["on" + expr]`;
                        // strip the surrounding brackets so the expression goes into the
                        // dynamic props array verbatim (build_patch_flag_suffix will skip
                        // quoting it because it already starts with `"`).
                        let dp_entry = event_name
                            .strip_prefix('[')
                            .and_then(|s| s.strip_suffix(']'))
                            .unwrap_or(&event_name)
                            .to_string();
                        state.dynamic_props.push(dp_entry);
                        state.patch_flag = state.patch_flag.add(PatchFlags::Props);
                        written += 1;
                    }

                    PropKind::ClassBind => {
                        // :class="expr" → class: _normalizeClass(expr)
                        // With merge: class: _normalizeClass(["static", expr])
                        imports.add(TemplateImportDependencies::NORMALIZE_CLASS);

                        if let Some(val_span) = prop.event.value {
                            if merge_class {
                                let static_val = static_class.as_ref().unwrap();
                                code_transform.overwrite(
                                    prop.event.start,
                                    val_span.start,
                                    &format!("{}class: _normalizeClass([\"{}\", ", sep, static_val),
                                );
                                code_transform.overwrite(val_span.end, prop.event.end, "])");
                            } else {
                                code_transform.overwrite(
                                    prop.event.start,
                                    val_span.start,
                                    &format!("{}class: _normalizeClass(", sep),
                                );
                                code_transform.overwrite(val_span.end, prop.event.end, ")");
                            }

                            if let Some(exp) = &prop.exp {
                                patch_bindings(
                                    code_transform,
                                    &exp.bindings,
                                    bindings,
                                    is_production,
                                );
                            }
                        }

                        state.patch_flag = state.patch_flag.add(PatchFlags::Class);
                        written += 1;
                    }

                    PropKind::StyleBind => {
                        // :style="expr" → style: _normalizeStyle(expr)
                        // With merge: style: _normalizeStyle(["static", expr])
                        imports.add(TemplateImportDependencies::NORMALIZE_STYLE);

                        if let Some(val_span) = prop.event.value {
                            if merge_style {
                                let static_val = static_style.as_ref().unwrap();
                                code_transform.overwrite(
                                    prop.event.start,
                                    val_span.start,
                                    &format!("{}style: _normalizeStyle([\"{}\", ", sep, static_val),
                                );
                                code_transform.overwrite(val_span.end, prop.event.end, "])");
                            } else {
                                code_transform.overwrite(
                                    prop.event.start,
                                    val_span.start,
                                    &format!("{}style: _normalizeStyle(", sep),
                                );
                                code_transform.overwrite(val_span.end, prop.event.end, ")");
                            }

                            if let Some(exp) = &prop.exp {
                                patch_bindings(
                                    code_transform,
                                    &exp.bindings,
                                    bindings,
                                    is_production,
                                );
                            }
                        }

                        state.patch_flag = state.patch_flag.add(PatchFlags::Style);
                        written += 1;
                    }

                    PropKind::Model => {
                        // v-model on native elements → withDirectives + vModel*
                        // v-model on components → modelValue + onUpdate:modelValue props
                        let model_arg = prop.event.arg.map(|arg_span| {
                            ctx.input[arg_span.start as usize..arg_span.end as usize].to_string()
                        });
                        let model_name = model_arg.as_deref().unwrap_or("modelValue");

                        let model_modifiers: Vec<&str> = prop
                            .event
                            .modifiers
                            .as_ref()
                            .map(|m| {
                                m.iter()
                                    .map(|s| &ctx.input[s.start as usize..s.end as usize])
                                    .collect()
                            })
                            .unwrap_or_default();

                        if is_component {
                            // Component v-model: prop-based, no withDirectives
                            // `v-model="val"` → `modelValue: _ctx.val, "onUpdate:modelValue": $event => ((_ctx.val) = $event)`
                            // `v-model:title="val"` → `title: _ctx.val, "onUpdate:title": $event => ((_ctx.val) = $event)`
                            if let Some(val_span) = prop.event.value {
                                let update_event = format!("\"onUpdate:{}\"", model_name);
                                let val_text =
                                    &ctx.input[val_span.start as usize..val_span.end as usize];
                                let prefixed_val = if let Some(exp) = &prop.exp {
                                    build_prefixed_value(
                                        val_text,
                                        val_span.start,
                                        &exp.bindings,
                                        bindings,
                                        is_production,
                                    )
                                } else {
                                    val_text.to_string()
                                };

                                let mut replacement = format!(
                                    "{}{}: {}, {}: $event => (({}) = $event)",
                                    sep, model_name, prefixed_val, update_event, prefixed_val
                                );

                                // Component v-model modifiers → modelModifiers prop
                                if !model_modifiers.is_empty() {
                                    let mods_obj = model_modifiers
                                        .iter()
                                        .map(|m| format!("{}: true", m))
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    let mods_prop_name = if model_name == "modelValue" {
                                        "modelModifiers".to_string()
                                    } else {
                                        format!("{}Modifiers", model_name)
                                    };
                                    replacement.push_str(&format!(
                                        ", {}: {{ {} }}",
                                        mods_prop_name, mods_obj
                                    ));
                                }

                                code_transform.overwrite(
                                    prop.event.start,
                                    prop.event.end,
                                    &replacement,
                                );

                                state.dynamic_props.push(model_name.to_string());
                                state.dynamic_props.push(format!("onUpdate:{}", model_name));
                                state.patch_flag = state.patch_flag.add(PatchFlags::Props);
                            }
                        } else {
                            // Native v-model: directive-based (withDirectives)
                            // Determine which vModel directive to use based on element type
                            let type_attr = ev.props.iter().find_map(|p| {
                                if p.event.kind == PropKind::Value {
                                    let name = &ctx.input
                                        [p.event.start as usize..p.event.name_end as usize];
                                    if name == "type" {
                                        p.event.value.map(|v| {
                                            ctx.input[v.start as usize..v.end as usize].to_string()
                                        })
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            });

                            let (directive_name, import_flag) =
                                match (tag_name, type_attr.as_deref()) {
                                    ("select", _) => (
                                        "_vModelSelect",
                                        TemplateImportDependencies::V_MODEL_SELECT,
                                    ),
                                    (_, Some("checkbox")) => (
                                        "_vModelCheckbox",
                                        TemplateImportDependencies::V_MODEL_CHECKBOX,
                                    ),
                                    (_, Some("radio")) => {
                                        ("_vModelRadio", TemplateImportDependencies::V_MODEL_RADIO)
                                    }
                                    _ => ("_vModelText", TemplateImportDependencies::V_MODEL_TEXT),
                                };
                            imports.add(import_flag);
                            imports.add(TemplateImportDependencies::WITH_DIRECTIVES);

                            // Emit the onUpdate:modelValue prop
                            if let Some(val_span) = prop.event.value {
                                let val_text =
                                    &ctx.input[val_span.start as usize..val_span.end as usize];
                                let prefixed_val = if let Some(exp) = &prop.exp {
                                    build_prefixed_value(
                                        val_text,
                                        val_span.start,
                                        &exp.bindings,
                                        bindings,
                                        is_production,
                                    )
                                } else {
                                    val_text.to_string()
                                };

                                code_transform.overwrite(
                                    prop.event.start,
                                    prop.event.end,
                                    &format!(
                                        "{}\"onUpdate:modelValue\": $event => (({}) = $event)",
                                        sep, prefixed_val
                                    ),
                                );

                                // Add directive entry for withDirectives wrapping
                                let mut dir_entry = DirectiveEntry {
                                    directive: directive_name.to_string(),
                                    value: prefixed_val,
                                    arg: String::new(),
                                    modifiers: String::new(),
                                };
                                if !model_modifiers.is_empty() {
                                    dir_entry.modifiers = format!(
                                        "{{ {} }}",
                                        model_modifiers
                                            .iter()
                                            .map(|m| format!("{}: true", m))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    );
                                }
                                state.runtime_directives.push(dir_entry);
                            }

                            state.dynamic_props.push("onUpdate:modelValue".to_string());
                            state.patch_flag = state.patch_flag.add(PatchFlags::Props);
                        }
                        written += 1;
                    }

                    PropKind::Show => {
                        // v-show="expr" → withDirectives(vnode, [[vShow, expr]])
                        // No prop emitted — just add directive entry + NEED_PATCH flag
                        imports.add(TemplateImportDependencies::V_SHOW);
                        imports.add(TemplateImportDependencies::WITH_DIRECTIVES);

                        if let Some(val_span) = prop.event.value {
                            let val_text =
                                &ctx.input[val_span.start as usize..val_span.end as usize];
                            let prefixed_val = if let Some(exp) = &prop.exp {
                                build_prefixed_value(
                                    val_text,
                                    val_span.start,
                                    &exp.bindings,
                                    bindings,
                                    is_production,
                                )
                            } else {
                                val_text.to_string()
                            };

                            state.runtime_directives.push(DirectiveEntry {
                                directive: "_vShow".to_string(),
                                value: prefixed_val,
                                arg: String::new(),
                                modifiers: String::new(),
                            });
                        }
                        // Remove v-show from props output
                        code_transform.overwrite(prop.event.start, prop.event.end, "");
                        state.patch_flag = state.patch_flag.add(PatchFlags::NeedPatch);
                    }

                    PropKind::Html => {
                        // v-html="expr" → innerHTML: expr (as prop, no directive)
                        if let Some(val_span) = prop.event.value {
                            code_transform.overwrite(
                                prop.event.start,
                                val_span.start,
                                &format!("{}innerHTML: ", sep),
                            );
                            code_transform.overwrite(val_span.end, prop.event.end, "");
                            if let Some(exp) = &prop.exp {
                                patch_bindings(
                                    code_transform,
                                    &exp.bindings,
                                    bindings,
                                    is_production,
                                );
                            }
                        }
                        state.dynamic_props.push("innerHTML".to_string());
                        state.patch_flag = state.patch_flag.add(PatchFlags::Props);
                        written += 1;
                    }

                    PropKind::Text => {
                        // v-text="expr" → textContent: _toDisplayString(expr)
                        imports.add(TemplateImportDependencies::TO_DISPLAY_STRING);
                        if let Some(val_span) = prop.event.value {
                            code_transform.overwrite(
                                prop.event.start,
                                val_span.start,
                                &format!("{}textContent: _toDisplayString(", sep),
                            );
                            code_transform.overwrite(val_span.end, prop.event.end, ")");
                            if let Some(exp) = &prop.exp {
                                patch_bindings(
                                    code_transform,
                                    &exp.bindings,
                                    bindings,
                                    is_production,
                                );
                            }
                        }
                        state.dynamic_props.push("textContent".to_string());
                        state.patch_flag = state.patch_flag.add(PatchFlags::Props);
                        written += 1;
                    }

                    PropKind::BindSpread => {
                        // v-bind="obj" → _normalizeProps(_guardReactiveProps(expr))
                        imports.add(TemplateImportDependencies::NORMALIZE_PROPS);
                        imports.add(TemplateImportDependencies::GUARD_REACTIVE_PROPS);
                        // BindSpread replaces the entire props object, not a single prop.
                        // We mark this and handle it specially after the loop.
                        // For now: overwrite with _normalizeProps wrapper
                        if let Some(val_span) = prop.event.value {
                            code_transform.overwrite(
                                prop.event.start,
                                val_span.start,
                                &format!("{}_normalizeProps(_guardReactiveProps(", sep),
                            );
                            code_transform.overwrite(val_span.end, prop.event.end, "))");
                            if let Some(exp) = &prop.exp {
                                patch_bindings(
                                    code_transform,
                                    &exp.bindings,
                                    bindings,
                                    is_production,
                                );
                            }
                        }
                        state.patch_flag = state.patch_flag.add(PatchFlags::FullProps);
                        written += 1;
                    }

                    PropKind::OnSpread => {
                        // v-on="handlers" → _toHandlers(expr, true)
                        // TODO: add TO_HANDLERS to imports when available
                        if let Some(val_span) = prop.event.value {
                            code_transform.overwrite(
                                prop.event.start,
                                val_span.start,
                                &format!("{}_toHandlers(", sep),
                            );
                            code_transform.overwrite(val_span.end, prop.event.end, ", true)");
                            if let Some(exp) = &prop.exp {
                                patch_bindings(
                                    code_transform,
                                    &exp.bindings,
                                    bindings,
                                    is_production,
                                );
                            }
                        }
                        state.patch_flag = state.patch_flag.add(PatchFlags::FullProps);
                        written += 1;
                    }

                    PropKind::Directive => {
                        // Custom directive: v-my-directive:arg.mod="val"
                        // → resolveDirective + withDirectives
                        let dir_raw_name =
                            &ctx.input[prop.event.start as usize..prop.event.name_end as usize];
                        // Strip "v-" prefix for resolve
                        let dir_name = dir_raw_name.strip_prefix("v-").unwrap_or(dir_raw_name);
                        let dir_var = format!("_directive_{}", dir_name.replace('-', "_"));
                        // Register for _resolveDirective declaration (deduped)
                        if !resolved_directives.contains(&dir_name.to_string()) {
                            resolved_directives.push(dir_name.to_string());
                        }
                        imports.add(TemplateImportDependencies::RESOLVE_DIRECTIVE);
                        imports.add(TemplateImportDependencies::WITH_DIRECTIVES);

                        let value = if let Some(val_span) = prop.event.value {
                            let val_text =
                                &ctx.input[val_span.start as usize..val_span.end as usize];
                            if let Some(exp) = &prop.exp {
                                build_prefixed_value(
                                    val_text,
                                    val_span.start,
                                    &exp.bindings,
                                    bindings,
                                    is_production,
                                )
                            } else {
                                val_text.to_string()
                            }
                        } else {
                            String::new()
                        };

                        let arg = prop
                            .event
                            .arg
                            .map(|arg_span| {
                                let raw =
                                    &ctx.input[arg_span.start as usize..arg_span.end as usize];
                                if prop.event.has_dynamic_arg {
                                    apply_dynamic_arg_prefix(
                                        raw,
                                        arg_span.start,
                                        &prop.arg.as_ref().and_then(|a| a.bindings.clone()),
                                        bindings,
                                        is_production,
                                    )
                                } else {
                                    format!("\"{}\"", raw)
                                }
                            })
                            .unwrap_or_default();

                        let dir_modifiers: Vec<&str> = prop
                            .event
                            .modifiers
                            .as_ref()
                            .map(|m| {
                                m.iter()
                                    .map(|s| &ctx.input[s.start as usize..s.end as usize])
                                    .collect()
                            })
                            .unwrap_or_default();

                        let mods = if dir_modifiers.is_empty() {
                            String::new()
                        } else {
                            format!(
                                "{{ {} }}",
                                dir_modifiers
                                    .iter()
                                    .map(|m| format!("{}: true", m))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        };

                        state.runtime_directives.push(DirectiveEntry {
                            directive: dir_var,
                            value,
                            arg,
                            modifiers: mods,
                        });

                        // Remove from props output
                        code_transform.overwrite(prop.event.start, prop.event.end, "");
                        state.patch_flag = state.patch_flag.add(PatchFlags::NeedPatch);
                    }

                    // v-if, v-else-if, v-else, v-for, v-slot, v-once are handled
                    // as scopes in directives/mod.rs, not as prop kinds here.
                    _ => {
                        code_transform.overwrite(prop.event.start, prop.event.end, "");
                    }
                }
            }

            // Close the props object: overwrite `>` with `}` (or empty for spread-only)
            let last_prop_end = ev.props.last().unwrap().event.end;
            if is_spread_only {
                code_transform.overwrite(last_prop_end, open_tag_end.end, "");
            } else {
                code_transform.overwrite(last_prop_end, open_tag_end.end, "}");
            }
        }
    }
}
