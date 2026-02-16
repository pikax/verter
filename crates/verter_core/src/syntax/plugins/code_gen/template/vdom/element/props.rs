//! Extracted per-prop-kind handlers for VDOM element open processing.
//!
//! These functions handle the complex match arms of `handle_element_open`,
//! keeping the main function focused on coordination logic.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    code_transform::CodeTransform,
    syntax::{
        binding_types::BindingType,
        plugin::SyntaxPluginContext,
        plugins::code_gen::template::shared::helper::{
            build_prefixed_value_into, camelize_capitalize_into, camelize_into,
            capitalize_first_into, classify_modifier, collect_binding_patches,
            is_valid_js_prop_key, ModifierKind,
        },
        types::{OxcProp, PropKind},
    },
    utils::vue::PatchFlags,
};

use super::{needs_event_handler_wrap, DirectiveEntry, StateStack};
use crate::syntax::plugins::code_gen::types::TemplateImportDependencies;

/// Handle `@event="handler"` props (PropKind::On).
///
/// Processes event modifiers (runtime, key filters, listener options),
/// builds event name with suffix, wraps handler with `_withModifiers`/`_withKeys`
/// as needed, and adds the event to dynamic props.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_prop_on<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    prop: &OxcProp<'alloc>,
    sep: &str,
    ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack<'alloc>,
    bindings: &FxHashMap<&'alloc str, BindingType>,
    is_production: bool,
    imports: &mut TemplateImportDependencies,
    buf: &mut String,
    binding_patches: &mut Vec<(u32, &'alloc str)>,
    pending_overwrites: &mut Vec<(u32, u32, &'alloc str)>,
) -> usize {
    let raw_event: &str = if let Some(arg_span) = prop.event.arg {
        &ctx.input[arg_span.start as usize..arg_span.end as usize]
    } else {
        "click"
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

    let mut event_suffix = String::new();
    let mut runtime_mods: Vec<&str> = Vec::new();
    let mut key_mods: Vec<&str> = Vec::new();

    for m in &modifiers {
        match classify_modifier(m) {
            ModifierKind::ListenerOption => {
                capitalize_first_into(m, &mut event_suffix);
            }
            ModifierKind::KeyFilter => key_mods.push(m),
            ModifierKind::Runtime => runtime_mods.push(m),
        }
    }

    // Build camelized event name in buffer (needed for dynamic_props in all cases)
    let event_name: &'alloc str = if prop.event.has_dynamic_arg {
        let arg_span = prop.event.arg.unwrap();
        let raw = &ctx.input[arg_span.start as usize..arg_span.end as usize];
        let saved = buf.len();
        build_prefixed_value_into(
            buf,
            raw,
            arg_span.start,
            prop.arg.as_ref().and_then(|a| a.bindings.as_ref()),
            bindings,
            is_production,
            &[],
        );
        let prefixed = code_transform.alloc_str(&buf[saved..]);
        buf.truncate(saved);
        let inner = prefixed
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or(prefixed);
        buf.clear();
        buf.push_str("[\"on\" + ");
        buf.push_str(inner);
        buf.push(']');
        code_transform.alloc_str(buf)
    } else {
        buf.clear();
        buf.push_str("on");
        camelize_capitalize_into(raw_event, buf);
        buf.push_str(&event_suffix);
        code_transform.alloc_str(buf)
    };

    // Build handler prefix/suffix from modifiers (shared by all branches)
    let mut handler_prefix = String::new();
    let mut handler_suffix = String::new();

    if !runtime_mods.is_empty() {
        imports.add(TemplateImportDependencies::WITH_MODIFIERS);
        handler_prefix.push_str("_withModifiers(");
        handler_suffix.push_str(", [");
        for (i, m) in runtime_mods.iter().enumerate() {
            if i > 0 {
                handler_suffix.push(',');
            }
            handler_suffix.push('"');
            handler_suffix.push_str(m);
            handler_suffix.push('"');
        }
        handler_suffix.push_str("])");
    }
    if !key_mods.is_empty() {
        imports.add(TemplateImportDependencies::WITH_KEYS);
        let old_prefix = std::mem::take(&mut handler_prefix);
        handler_prefix.push_str("_withKeys(");
        handler_prefix.push_str(&old_prefix);
        handler_suffix.push_str(", [");
        for (i, m) in key_mods.iter().enumerate() {
            if i > 0 {
                handler_suffix.push(',');
            }
            handler_suffix.push('"');
            handler_suffix.push_str(m);
            handler_suffix.push('"');
        }
        handler_suffix.push_str("])");
    }

    // For non-dynamic events with arg_span: use split overwrites to transform
    // the event name in-place (capitalize first char, camelize hyphens, prepend "on").
    if !prop.event.has_dynamic_arg {
        if let Some(arg_span) = prop.event.arg {
            // Event names with colons (e.g., update:model-value → onUpdate:modelValue)
            // must be quoted as string keys in JavaScript object literals.
            let needs_key_quote = raw_event.contains(':');

            // 1. Before name: replace @ prefix with sep + "on" (or quoted variant)
            let before: &'static str = match (sep.is_empty(), needs_key_quote) {
                (true, false) => "on",
                (true, true) => "\"on",
                (false, false) => ", on",
                (false, true) => ", \"on",
            };
            pending_overwrites.push((prop.event.start, arg_span.start, before));

            // 2. Uppercase first character
            let bytes = raw_event.as_bytes();
            if let Some(&first) = bytes.first() {
                if first.is_ascii_lowercase() {
                    let saved = buf.len();
                    buf.push(first.to_ascii_uppercase() as char);
                    let s = code_transform.alloc_str(&buf[saved..]);
                    buf.truncate(saved);
                    pending_overwrites.push((arg_span.start, arg_span.start + 1, s));
                }
            }

            // 3. Camelize hyphens: -x → X
            for i in 0..bytes.len().saturating_sub(1) {
                if bytes[i] == b'-' && bytes[i + 1].is_ascii_alphabetic() {
                    let saved = buf.len();
                    buf.push(bytes[i + 1].to_ascii_uppercase() as char);
                    let s = code_transform.alloc_str(&buf[saved..]);
                    buf.truncate(saved);
                    pending_overwrites.push((
                        arg_span.start + i as u32,
                        arg_span.start + i as u32 + 2,
                        s,
                    ));
                }
            }

            // 4. After name: [closing quote] + event_suffix + ": " + handler_prefix [+ "$event => ("]
            let quote_close = if needs_key_quote { "\"" } else { "" };

            if let Some(val_span) = prop.event.value {
                // Empty value (e.g., @click.stop="") — treat like no-value
                // but still wrap with modifier helpers.
                // Vue compiler emits `_withModifiers(() => {}, ["stop"])`.
                let is_empty_value = val_span.start == val_span.end;

                if is_empty_value {
                    buf.clear();
                    buf.push_str(&event_suffix);
                    buf.push_str(quote_close);
                    buf.push_str(": ");
                    buf.push_str(&handler_prefix);
                    buf.push_str("() => {}");
                    buf.push_str(&handler_suffix);
                    let s = code_transform.alloc_str(buf);
                    pending_overwrites.push((arg_span.end, prop.event.end, s));
                } else {
                    let wrap = prop
                        .exp
                        .as_ref()
                        .map(|e| needs_event_handler_wrap(&e.expression))
                        .unwrap_or(false);

                    if event_suffix.is_empty() && handler_prefix.is_empty() && !needs_key_quote {
                        let after: &'static str = if wrap { ": $event => (" } else { ": " };
                        pending_overwrites.push((arg_span.end, val_span.start, after));
                    } else {
                        buf.clear();
                        buf.push_str(&event_suffix);
                        buf.push_str(quote_close);
                        buf.push_str(": ");
                        buf.push_str(&handler_prefix);
                        if wrap {
                            buf.push_str("$event => (");
                        }
                        let s = code_transform.alloc_str(buf);
                        pending_overwrites.push((arg_span.end, val_span.start, s));
                    }

                    // Suffix overwrite after value
                    if wrap {
                        buf.clear();
                        buf.push(')');
                        buf.push_str(&handler_suffix);
                        let s = code_transform.alloc_str(buf);
                        pending_overwrites.push((val_span.end, prop.event.end, s));
                    } else {
                        let s = code_transform.alloc_str(&handler_suffix);
                        pending_overwrites.push((val_span.end, prop.event.end, s));
                    }

                    if let Some(exp) = &prop.exp {
                        collect_binding_patches(
                            exp.bindings.as_ref(),
                            bindings,
                            is_production,
                            binding_patches,
                        );
                    }
                }
            } else {
                // No value: [closing quote] + event_suffix + ": () => {}"
                buf.clear();
                buf.push_str(&event_suffix);
                buf.push_str(quote_close);
                buf.push_str(": ");
                buf.push_str(&handler_prefix);
                buf.push_str("() => {}");
                buf.push_str(&handler_suffix);
                let s = code_transform.alloc_str(buf);
                pending_overwrites.push((arg_span.end, prop.event.end, s));
            }
        } else {
            // No arg_span (default "click"): use buffer approach
            emit_event_buffer(
                code_transform,
                prop,
                sep,
                event_name,
                &event_suffix,
                &handler_prefix,
                &handler_suffix,
                buf,
                binding_patches,
                bindings,
                is_production,
                pending_overwrites,
            );
        }
    } else {
        // Dynamic event: use buffer approach (current)
        emit_event_buffer(
            code_transform,
            prop,
            sep,
            event_name,
            &event_suffix,
            &handler_prefix,
            &handler_suffix,
            buf,
            binding_patches,
            bindings,
            is_production,
            pending_overwrites,
        );
    }

    // Events produce PROPS patch flag with event name in dynamic props.
    let dp_entry: &'alloc str = event_name
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(event_name);
    state.dynamic_props.push(dp_entry);
    state.patch_flag = state.patch_flag.add(PatchFlags::Props);
    1
}

/// Emit event overwrites using the buffer approach (for dynamic args or no arg_span).
#[allow(clippy::too_many_arguments)]
fn emit_event_buffer<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    prop: &OxcProp<'alloc>,
    sep: &str,
    event_name: &'alloc str,
    _event_suffix: &str,
    handler_prefix: &str,
    handler_suffix: &str,
    buf: &mut String,
    binding_patches: &mut Vec<(u32, &'alloc str)>,
    bindings: &FxHashMap<&'alloc str, BindingType>,
    is_production: bool,
    pending_overwrites: &mut Vec<(u32, u32, &'alloc str)>,
) {
    // Quote the key if it contains characters invalid as a bare JS identifier (e.g., colon).
    // Computed property keys (starting with `[`) are already bracketed and must not be quoted.
    let needs_quote = !event_name.starts_with('[') && !is_valid_js_prop_key(event_name);

    if let Some(val_span) = prop.event.value {
        // Empty value (e.g., @click.stop="") — emit `() => {}` as the handler,
        // still wrapped with any modifier helpers.
        let is_empty_value = val_span.start == val_span.end;

        if is_empty_value {
            buf.clear();
            buf.push_str(sep);
            if needs_quote {
                buf.push('"');
            }
            buf.push_str(event_name);
            if needs_quote {
                buf.push('"');
            }
            buf.push_str(": ");
            buf.push_str(handler_prefix);
            buf.push_str("() => {}");
            buf.push_str(handler_suffix);
            let s = code_transform.alloc_str(buf);
            pending_overwrites.push((prop.event.start, prop.event.end, s));
        } else {
            let wrap = prop
                .exp
                .as_ref()
                .map(|e| needs_event_handler_wrap(&e.expression))
                .unwrap_or(false);

            buf.clear();
            buf.push_str(sep);
            if needs_quote {
                buf.push('"');
            }
            buf.push_str(event_name);
            if needs_quote {
                buf.push('"');
            }
            buf.push_str(": ");
            buf.push_str(handler_prefix);
            if wrap {
                buf.push_str("$event => (");
            }
            let s = code_transform.alloc_str(buf);
            pending_overwrites.push((prop.event.start, val_span.start, s));

            if wrap {
                buf.clear();
                buf.push(')');
                buf.push_str(handler_suffix);
                let s = code_transform.alloc_str(buf);
                pending_overwrites.push((val_span.end, prop.event.end, s));
            } else {
                let s = code_transform.alloc_str(handler_suffix);
                pending_overwrites.push((val_span.end, prop.event.end, s));
            }

            if let Some(exp) = &prop.exp {
                collect_binding_patches(
                    exp.bindings.as_ref(),
                    bindings,
                    is_production,
                    binding_patches,
                );
            }
        }
    } else {
        buf.clear();
        buf.push_str(sep);
        if needs_quote {
            buf.push('"');
        }
        buf.push_str(event_name);
        if needs_quote {
            buf.push('"');
        }
        buf.push_str(": ");
        buf.push_str(handler_prefix);
        buf.push_str("() => {}");
        buf.push_str(handler_suffix);
        let s = code_transform.alloc_str(buf);
        pending_overwrites.push((prop.event.start, prop.event.end, s));
    }
}

/// Handle `v-model` props (PropKind::Model).
///
/// Component v-model: prop-based (`modelValue` + `onUpdate:modelValue` props).
/// Native v-model: directive-based (`withDirectives` + `vModel*` directive).
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_prop_model<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    prop: &OxcProp<'alloc>,
    sep: &str,
    ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack<'alloc>,
    bindings: &FxHashMap<&'alloc str, BindingType>,
    is_production: bool,
    imports: &mut TemplateImportDependencies,
    tag_name: &str,
    is_component: bool,
    ev_props: &[OxcProp<'alloc>],
    buf: &mut String,
    pending_overwrites: &mut Vec<(u32, u32, &'alloc str)>,
) -> usize {
    let model_name: &str = prop
        .event
        .arg
        .map(|arg_span| &ctx.input[arg_span.start as usize..arg_span.end as usize])
        .unwrap_or("modelValue");

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
        if let Some(val_span) = prop.event.value {
            let val_text = &ctx.input[val_span.start as usize..val_span.end as usize];
            let prefixed_val: &'alloc str = if let Some(exp) = &prop.exp {
                let saved = buf.len();
                build_prefixed_value_into(
                    buf,
                    val_text,
                    val_span.start,
                    exp.bindings.as_ref(),
                    bindings,
                    is_production,
                    &[],
                );
                let result = code_transform.alloc_str(&buf[saved..]);
                buf.truncate(saved);
                result
            } else {
                val_text
            };

            let needs_quote = !is_valid_js_prop_key(model_name);

            buf.clear();
            buf.push_str(sep);
            // Prop key: quote if hyphenated
            if needs_quote {
                buf.push('"');
            }
            buf.push_str(model_name);
            if needs_quote {
                buf.push('"');
            }
            buf.push_str(": ");
            buf.push_str(prefixed_val);
            // Event key: camelize model name within quoted "onUpdate:..."
            buf.push_str(", \"onUpdate:");
            if needs_quote {
                camelize_into(model_name, buf);
            } else {
                buf.push_str(model_name);
            }
            buf.push_str("\": $event => ((");
            buf.push_str(prefixed_val);
            buf.push_str(") = $event)");

            // Component v-model modifiers → modelModifiers prop
            if !model_modifiers.is_empty() {
                let mods_prop_name = if model_name == "modelValue" {
                    "modelModifiers"
                } else {
                    // Need to build the name dynamically
                    ""
                };
                buf.push_str(", ");
                // Modifiers prop name needs quoting if model_name has hyphens
                if needs_quote {
                    buf.push('"');
                }
                if mods_prop_name.is_empty() {
                    buf.push_str(model_name);
                    buf.push_str("Modifiers");
                } else {
                    buf.push_str(mods_prop_name);
                }
                if needs_quote {
                    buf.push('"');
                }
                buf.push_str(": { ");
                for (i, m) in model_modifiers.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(", ");
                    }
                    buf.push_str(m);
                    buf.push_str(": true");
                }
                buf.push_str(" }");
            }

            let s = code_transform.alloc_str(buf);
            pending_overwrites.push((prop.event.start, prop.event.end, s));

            state.dynamic_props.push(model_name);
            buf.clear();
            buf.push_str("onUpdate:");
            if needs_quote {
                camelize_into(model_name, buf);
            } else {
                buf.push_str(model_name);
            }
            state.dynamic_props.push(code_transform.alloc_str(buf));
            state.patch_flag = state.patch_flag.add(PatchFlags::Props);
        }
    } else {
        // Native v-model: directive-based (withDirectives)
        let type_attr: Option<&str> = ev_props.iter().find_map(|p| {
            if p.event.kind == PropKind::Value {
                let name = &ctx.input[p.event.start as usize..p.event.name_end as usize];
                if name == "type" {
                    p.event
                        .value
                        .map(|v| &ctx.input[v.start as usize..v.end as usize])
                } else {
                    None
                }
            } else {
                None
            }
        });

        let (directive_name, import_flag) = match (tag_name, type_attr) {
            ("select", _) => ("_vModelSelect", TemplateImportDependencies::V_MODEL_SELECT),
            (_, Some("checkbox")) => (
                "_vModelCheckbox",
                TemplateImportDependencies::V_MODEL_CHECKBOX,
            ),
            (_, Some("radio")) => ("_vModelRadio", TemplateImportDependencies::V_MODEL_RADIO),
            _ => ("_vModelText", TemplateImportDependencies::V_MODEL_TEXT),
        };
        imports.add(import_flag);
        imports.add(TemplateImportDependencies::WITH_DIRECTIVES);

        // Emit the onUpdate:modelValue prop
        if let Some(val_span) = prop.event.value {
            let val_text = &ctx.input[val_span.start as usize..val_span.end as usize];
            let prefixed_val: &'alloc str = if let Some(exp) = &prop.exp {
                let saved = buf.len();
                build_prefixed_value_into(
                    buf,
                    val_text,
                    val_span.start,
                    exp.bindings.as_ref(),
                    bindings,
                    is_production,
                    &[],
                );
                let result = code_transform.alloc_str(&buf[saved..]);
                buf.truncate(saved);
                result
            } else {
                val_text
            };

            buf.clear();
            buf.push_str(sep);
            buf.push_str("\"onUpdate:modelValue\": $event => ((");
            buf.push_str(prefixed_val);
            buf.push_str(") = $event)");
            let s = code_transform.alloc_str(buf);
            pending_overwrites.push((prop.event.start, prop.event.end, s));

            // Add directive entry for withDirectives wrapping
            let mut dir_entry = DirectiveEntry {
                directive: directive_name,
                value: prefixed_val,
                arg: "",
                modifiers: "",
            };
            if !model_modifiers.is_empty() {
                buf.clear();
                buf.push_str("{ ");
                for (i, m) in model_modifiers.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(", ");
                    }
                    buf.push_str(m);
                    buf.push_str(": true");
                }
                buf.push_str(" }");
                dir_entry.modifiers = code_transform.alloc_str(buf);
            }
            state.runtime_directives.push(dir_entry);
        }

        state.dynamic_props.push("onUpdate:modelValue");
        state.patch_flag = state.patch_flag.add(PatchFlags::Props);
    }
    1
}

/// Handle custom directive props (PropKind::Directive).
///
/// Resolves the directive name, extracts arg and modifiers, and adds
/// a `DirectiveEntry` to `state.runtime_directives` for `_withDirectives` wrapping.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_prop_directive<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    prop: &OxcProp<'alloc>,
    ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack<'alloc>,
    bindings: &FxHashMap<&'alloc str, BindingType>,
    is_production: bool,
    imports: &mut TemplateImportDependencies,
    resolved_directives: &mut Vec<&'alloc str>,
    resolved_directives_set: &mut FxHashSet<&'alloc str>,
    buf: &mut String,
    pending_overwrites: &mut Vec<(u32, u32, &'alloc str)>,
) {
    let dir_raw_name = &ctx.input[prop.event.start as usize..prop.event.name_end as usize];
    // Strip "v-" prefix for resolve
    let dir_name = dir_raw_name.strip_prefix("v-").unwrap_or(dir_raw_name);
    buf.clear();
    buf.push_str("_directive_");
    // Replace '-' with '_' inline instead of creating a temporary String
    for ch in dir_name.chars() {
        if ch == '-' {
            buf.push('_');
        } else {
            buf.push(ch);
        }
    }
    let dir_var = code_transform.alloc_str(buf);
    // Register for _resolveDirective declaration (deduped)
    if resolved_directives_set.insert(dir_name) {
        resolved_directives.push(dir_name);
    }
    imports.add(TemplateImportDependencies::RESOLVE_DIRECTIVE);
    imports.add(TemplateImportDependencies::WITH_DIRECTIVES);

    let value: &'alloc str = if let Some(val_span) = prop.event.value {
        let val_text = &ctx.input[val_span.start as usize..val_span.end as usize];
        if let Some(exp) = &prop.exp {
            let saved = buf.len();
            build_prefixed_value_into(
                buf,
                val_text,
                val_span.start,
                exp.bindings.as_ref(),
                bindings,
                is_production,
                &[],
            );
            let result = code_transform.alloc_str(&buf[saved..]);
            buf.truncate(saved);
            result
        } else {
            val_text
        }
    } else {
        ""
    };

    let arg: &'alloc str = prop
        .event
        .arg
        .map(|arg_span| {
            let raw = &ctx.input[arg_span.start as usize..arg_span.end as usize];
            if prop.event.has_dynamic_arg {
                let saved = buf.len();
                build_prefixed_value_into(
                    buf,
                    raw,
                    arg_span.start,
                    prop.arg.as_ref().and_then(|a| a.bindings.as_ref()),
                    bindings,
                    is_production,
                    &[],
                );
                let result = code_transform.alloc_str(&buf[saved..]);
                buf.truncate(saved);
                result
            } else {
                buf.clear();
                buf.push('"');
                buf.push_str(raw);
                buf.push('"');
                code_transform.alloc_str(buf)
            }
        })
        .unwrap_or("");

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

    let mods: &'alloc str = if dir_modifiers.is_empty() {
        ""
    } else {
        buf.clear();
        buf.push_str("{ ");
        for (i, m) in dir_modifiers.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            buf.push_str(m);
            buf.push_str(": true");
        }
        buf.push_str(" }");
        code_transform.alloc_str(buf)
    };

    state.runtime_directives.push(DirectiveEntry {
        directive: dir_var,
        value,
        arg,
        modifiers: mods,
    });

    // Remove from props output
    pending_overwrites.push((prop.event.start, prop.event.end, ""));
    state.patch_flag = state.patch_flag.add(PatchFlags::NeedPatch);
}
