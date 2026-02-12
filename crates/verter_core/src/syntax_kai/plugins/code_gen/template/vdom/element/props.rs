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
            apply_dynamic_arg_prefix, build_prefixed_value, capitalize_first, classify_modifier,
            patch_bindings, ModifierKind,
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
    code_transform: &mut CodeTransform<'alloc>,
    prop: &OxcProp<'alloc>,
    sep: &str,
    ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack,
    bindings: &FxHashMap<&'alloc str, BindingType>,
    is_production: bool,
    imports: &mut TemplateImportDependencies,
) -> usize {
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

    let mut event_suffix = String::new();
    let mut runtime_mods: Vec<&str> = Vec::new();
    let mut key_mods: Vec<&str> = Vec::new();

    for m in &modifiers {
        match classify_modifier(m) {
            ModifierKind::ListenerOption => {
                event_suffix.push_str(&capitalize_first(m));
            }
            ModifierKind::KeyFilter => key_mods.push(m),
            ModifierKind::Runtime => runtime_mods.push(m),
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
                &format!("{}{}: {}$event => (", sep, event_name, handler_prefix),
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
            code_transform.overwrite(val_span.end, prop.event.end, &handler_suffix);
        }

        if let Some(exp) = &prop.exp {
            patch_bindings(code_transform, &exp.bindings, bindings, is_production);
        }
    } else {
        code_transform.overwrite(
            prop.event.start,
            prop.event.end,
            &format!("{}{}: () => {{}}", sep, event_name),
        );
    }

    // Events produce PROPS patch flag with event name in dynamic props.
    let dp_entry = event_name
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(&event_name)
        .to_string();
    state.dynamic_props.push(dp_entry);
    state.patch_flag = state.patch_flag.add(PatchFlags::Props);
    1
}

/// Handle `v-model` props (PropKind::Model).
///
/// Component v-model: prop-based (`modelValue` + `onUpdate:modelValue` props).
/// Native v-model: directive-based (`withDirectives` + `vModel*` directive).
pub(super) fn handle_prop_model<'alloc>(
    code_transform: &mut CodeTransform<'alloc>,
    prop: &OxcProp<'alloc>,
    sep: &str,
    ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack,
    bindings: &FxHashMap<&'alloc str, BindingType>,
    is_production: bool,
    imports: &mut TemplateImportDependencies,
    tag_name: &str,
    is_component: bool,
    ev_props: &[OxcProp<'alloc>],
) -> usize {
    let model_arg = prop
        .event
        .arg
        .map(|arg_span| ctx.input[arg_span.start as usize..arg_span.end as usize].to_string());
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
        if let Some(val_span) = prop.event.value {
            let update_event = format!("\"onUpdate:{}\"", model_name);
            let val_text = &ctx.input[val_span.start as usize..val_span.end as usize];
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
                replacement.push_str(&format!(", {}: {{ {} }}", mods_prop_name, mods_obj));
            }

            code_transform.overwrite(prop.event.start, prop.event.end, &replacement);

            state.dynamic_props.push(model_name.to_string());
            state.dynamic_props.push(format!("onUpdate:{}", model_name));
            state.patch_flag = state.patch_flag.add(PatchFlags::Props);
        }
    } else {
        // Native v-model: directive-based (withDirectives)
        let type_attr = ev_props.iter().find_map(|p| {
            if p.event.kind == PropKind::Value {
                let name = &ctx.input[p.event.start as usize..p.event.name_end as usize];
                if name == "type" {
                    p.event
                        .value
                        .map(|v| ctx.input[v.start as usize..v.end as usize].to_string())
                } else {
                    None
                }
            } else {
                None
            }
        });

        let (directive_name, import_flag) = match (tag_name, type_attr.as_deref()) {
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
    1
}

/// Handle custom directive props (PropKind::Directive).
///
/// Resolves the directive name, extracts arg and modifiers, and adds
/// a `DirectiveEntry` to `state.runtime_directives` for `_withDirectives` wrapping.
pub(super) fn handle_prop_directive<'alloc>(
    code_transform: &mut CodeTransform<'alloc>,
    prop: &OxcProp<'alloc>,
    ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack,
    bindings: &FxHashMap<&'alloc str, BindingType>,
    is_production: bool,
    imports: &mut TemplateImportDependencies,
    resolved_directives: &mut Vec<String>,
    resolved_directives_set: &mut FxHashSet<String>,
) {
    let dir_raw_name = &ctx.input[prop.event.start as usize..prop.event.name_end as usize];
    // Strip "v-" prefix for resolve
    let dir_name = dir_raw_name.strip_prefix("v-").unwrap_or(dir_raw_name);
    let dir_var = format!("_directive_{}", dir_name.replace('-', "_"));
    // Register for _resolveDirective declaration (deduped)
    if resolved_directives_set.insert(dir_name.to_string()) {
        resolved_directives.push(dir_name.to_string());
    }
    imports.add(TemplateImportDependencies::RESOLVE_DIRECTIVE);
    imports.add(TemplateImportDependencies::WITH_DIRECTIVES);

    let value = if let Some(val_span) = prop.event.value {
        let val_text = &ctx.input[val_span.start as usize..val_span.end as usize];
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
            let raw = &ctx.input[arg_span.start as usize..arg_span.end as usize];
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
