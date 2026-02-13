//! Extracted per-prop-kind handlers for VDOM element open processing.
//!
//! These functions handle the complex match arms of `handle_element_open`,
//! keeping the main function focused on coordination logic.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        binding_types::BindingType,
        plugin::SyntaxPluginContext,
        plugins::code_gen::template::shared::helper::{
            build_prefixed_value_into, capitalize_first_into, classify_modifier,
            collect_binding_patches, ModifierKind,
        },
        types::{OxcProp, PropKind},
    },
    utils::vue::PatchFlags,
};

use super::{needs_event_handler_wrap, DirectiveEntry, StateStack};
use crate::syntax_kai::plugins::code_gen::types::TemplateImportDependencies;

/// Handle `@event="handler"` props (PropKind::On).
///
/// Processes event modifiers (runtime, key filters, listener options),
/// builds event name with suffix, wraps handler with `_withModifiers`/`_withKeys`
/// as needed, and adds the event to dynamic props.
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
        capitalize_first_into(raw_event, buf);
        buf.push_str(&event_suffix);
        code_transform.alloc_str(buf)
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

        if wrap {
            buf.clear();
            buf.push_str(sep);
            buf.push_str(event_name);
            buf.push_str(": ");
            buf.push_str(&handler_prefix);
            buf.push_str("$event => (");
            let s = code_transform.alloc_str(buf);
            pending_overwrites.push((prop.event.start, val_span.start, s));
            buf.clear();
            buf.push(')');
            buf.push_str(&handler_suffix);
            let s = code_transform.alloc_str(buf);
            pending_overwrites.push((val_span.end, prop.event.end, s));
        } else {
            buf.clear();
            buf.push_str(sep);
            buf.push_str(event_name);
            buf.push_str(": ");
            buf.push_str(&handler_prefix);
            let s = code_transform.alloc_str(buf);
            pending_overwrites.push((prop.event.start, val_span.start, s));
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
    } else {
        buf.clear();
        buf.push_str(sep);
        buf.push_str(event_name);
        buf.push_str(": () => {}");
        let s = code_transform.alloc_str(buf);
        pending_overwrites.push((prop.event.start, prop.event.end, s));
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

/// Handle `v-model` props (PropKind::Model).
///
/// Component v-model: prop-based (`modelValue` + `onUpdate:modelValue` props).
/// Native v-model: directive-based (`withDirectives` + `vModel*` directive).
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

            buf.clear();
            buf.push_str(sep);
            buf.push_str(model_name);
            buf.push_str(": ");
            buf.push_str(prefixed_val);
            buf.push_str(", \"onUpdate:");
            buf.push_str(model_name);
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
                if mods_prop_name.is_empty() {
                    buf.push_str(model_name);
                    buf.push_str("Modifiers");
                } else {
                    buf.push_str(mods_prop_name);
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
            buf.push_str(model_name);
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
