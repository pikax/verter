//! `v-model`: the native directive helper, the update handler, and the
//! pairing of a component model with an explicit `@update:*` listener.

use crate::ast::types::ElementNode;
use crate::framework::vue::semantic::{is_simple_ident, VueBindingView};
use crate::template::code_gen::shared::helpers::VdomHelper;
use crate::types::BindingType;

/// Append the update handler `$event => (<target> = $event)` for a model
/// bound to `raw` (the authored expression), whose resolved text is
/// `resolved`. Returns whether the handler calls the runtime `isRef`.
///
/// In inline mode a setup binding the script frontend could not prove to be
/// a ref (a `let`, or a value that may hold a ref) is written through the
/// ref when it is one, as the official compiler does:
/// `$event => (_isRef(x) ? (x).value = $event : x = $event)` for a `let`,
/// `… : null)` otherwise. The binding class is the one the script frontend
/// reported; this function decides nothing about it.
pub(super) fn push_model_update(
    buf: &mut String,
    raw: &str,
    resolved: &str,
    resolver: &VueBindingView<'_>,
) -> bool {
    let target = raw.trim();
    let maybe_ref = resolver.is_inline() && is_simple_ident(target);
    match maybe_ref.then(|| resolver.get(target)).flatten() {
        Some(binding @ (BindingType::SetupLet | BindingType::SetupMaybeRef)) => {
            buf.push_str("$event => (_isRef(");
            buf.push_str(target);
            buf.push_str(") ? (");
            buf.push_str(target);
            buf.push_str(").value = $event : ");
            if binding == BindingType::SetupLet {
                buf.push_str(target);
                buf.push_str(" = $event)");
            } else {
                buf.push_str("null)");
            }
            true
        }
        _ => {
            buf.push_str("$event => ((");
            buf.push_str(resolved);
            buf.push_str(") = $event)");
            false
        }
    }
}

/// Determine the appropriate vModel directive helper for a native element.
///
/// - `<select>` → `_vModelSelect`
/// - `<textarea>` → `_vModelText`
/// - `<input type="checkbox">` → `_vModelCheckbox`
/// - `<input type="radio">` → `_vModelRadio`
/// - `<input :type="dynamic">` → `_vModelDynamic`
/// - `<input>` (text, default) → `_vModelText`
pub(super) fn determine_native_vmodel_directive(
    element: &ElementNode,
    source: &str,
    tag_name: &str,
) -> VdomHelper {
    match tag_name {
        "select" => VdomHelper::VModelSelect,
        "textarea" => VdomHelper::VModelText,
        "input" => {
            // Check for static type="checkbox"|"radio" or dynamic :type
            for prop in &element.props {
                if prop.is_directive {
                    let dname = &source[prop.start as usize..prop.name_end as usize];
                    let is_bind = super::is_v_bind(dname);
                    if is_bind {
                        if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                            let arg = &source[as_ as usize..ae as usize];
                            if arg == "type" {
                                // Dynamic type binding → use _vModelDynamic
                                return VdomHelper::VModelDynamic;
                            }
                        }
                    }
                } else {
                    let name = &source[prop.start as usize..prop.name_end as usize];
                    if name == "type" {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let type_value = &source[vs as usize..ve as usize];
                            return match type_value {
                                "checkbox" => VdomHelper::VModelCheckbox,
                                "radio" => VdomHelper::VModelRadio,
                                _ => VdomHelper::VModelText,
                            };
                        }
                    }
                }
            }
            VdomHelper::VModelText
        }
        _ => VdomHelper::VModelText,
    }
}

/// Pre-scan for v-model directives paired with explicit `@update:<name>` handlers.
/// Returns `(vmodel_prop_idx, handler_prop_idx)` pairs that need merging into arrays.
pub(super) fn pre_scan_vmodel_handler_merge(
    element: &ElementNode,
    source: &str,
) -> Vec<(usize, usize)> {
    // `v-model` expands to a paired `onUpdate:*` handler only on component
    // elements; native `v-model` never merges with an explicit `@update:*`.
    // Bail before touching props when no component v-model is possible.
    if !element.tag_type.is_component() {
        return Vec::new();
    }

    let mut vmodel_keys: Vec<(usize, String)> = Vec::new();
    for (idx, prop) in element.props.iter().enumerate() {
        if !prop.is_directive {
            continue;
        }
        let dname = &source[prop.start as usize..prop.name_end as usize];
        if dname != "v-model" {
            continue;
        }
        let event_key = if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
            let arg = &source[as_ as usize..ae as usize];
            format!("update:{}", crate::template::vue_syntax::camelize(arg))
        } else {
            "update:modelValue".to_string()
        };
        vmodel_keys.push((idx, event_key));
    }

    // No v-model directive ⇒ nothing for an `@update:*` handler to pair with.
    // Returning early skips the second scan's per-handler key formatting.
    if vmodel_keys.is_empty() {
        return Vec::new();
    }

    let mut targets = Vec::new();
    for (idx, prop) in element.props.iter().enumerate() {
        if !prop.is_directive {
            continue;
        }
        let dname = &source[prop.start as usize..prop.name_end as usize];
        if !super::is_v_on(dname) {
            continue;
        }
        if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
            let arg = &source[as_ as usize..ae as usize];
            let normalized = format!(
                "update:{}",
                crate::template::vue_syntax::camelize(arg.strip_prefix("update:").unwrap_or(arg))
            );
            for &(vmodel_idx, ref vmodel_key) in &vmodel_keys {
                if normalized == *vmodel_key {
                    targets.push((vmodel_idx, idx));
                }
            }
        }
    }
    targets
}
