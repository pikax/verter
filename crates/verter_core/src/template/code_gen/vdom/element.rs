//! VDOM element code generation.
//!
//! Handles element open/close tag transformation, props object generation,
//! whitespace resolution, and patch flag emission. Children separators and
//! text-run wrapping live in [`super::children`]; component tag resolution
//! and dynamic component handling live in [`super::component`].
//!
//! The main entry point is [`process_element_leave`], called from
//! `VdomCodeGen::leave_element` after all children have been visited.

use crate::ast::types::{ChildrenMode, ElementNode};
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::vapor::find_prop_oxc_exp;
use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;
use crate::template::oxc::types::{ExpressionFlag, OxcParsedElement, OxcParsedExpression};

use super::super::shared::helpers::{self, VdomHelper};
use super::super::types::{ChildKind, ChildRecord, CodeGenOutput};
use super::super::TemplateCodeGenOptions;
use super::children::add_children_separators;
use super::component::{resolve_component_tag, resolve_dynamic_component, vnode_helper};
use super::props;

// Re-export moved function so external callers (e.g., compile.rs)
// can continue to use `vdom::element::to_pascal_case`.
pub(crate) use super::component::to_pascal_case;

// ======================== Whitespace resolution ========================

/// Resolve whitespace candidate children based on Vue's condense mode rules.
///
/// - Leading/trailing whitespace-only nodes → removed (overwritten to "")
/// - Interior `WhitespaceNewline` between two elements/comments → removed
/// - Interior `WhitespaceNewline` adjacent to text/interpolation → single space
/// - Interior `WhitespaceSpace` → converted to single-space Text
pub fn resolve_whitespace<'alloc>(
    children: &mut Vec<ChildRecord>,
    out: &mut CodeGenOutput<'alloc>,
    emit_removal_overwrites: bool,
) {
    // Remove leading whitespace (drain-based, O(n) instead of O(n^2))
    let leading = children
        .iter()
        .take_while(|c| is_whitespace_kind(c.kind))
        .count();
    if emit_removal_overwrites {
        for removed in children.drain(..leading) {
            out.overwrite(removed.start, removed.end, "");
        }
    } else {
        children.drain(..leading);
    }

    // Remove trailing whitespace
    while children.last().is_some_and(|c| is_whitespace_kind(c.kind)) {
        let removed = children.pop().unwrap();
        if emit_removal_overwrites {
            out.overwrite(removed.start, removed.end, "");
        }
    }

    // Resolve interior whitespace.
    // Vue's condense rules for WhitespaceNewline:
    //   - Remove when both adjacent siblings are elements or comments
    //   - Keep as single space otherwise (e.g., between element and
    //     interpolation, between two interpolations, etc.)
    let mut i = 0;
    while i < children.len() {
        match children[i].kind {
            ChildKind::WhitespaceNewline => {
                let prev_is_element = i > 0 && is_element_or_comment(children[i - 1].kind);
                let next_is_element =
                    i + 1 < children.len() && is_element_or_comment(children[i + 1].kind);

                if prev_is_element && next_is_element {
                    // Both neighbors are elements/comments → remove entirely
                    let removed = children.remove(i);
                    if emit_removal_overwrites {
                        out.overwrite(removed.start, removed.end, "");
                    }
                } else {
                    // At least one neighbor is text/interpolation → keep as space
                    // (always emit — this is a modification, not a removal)
                    out.overwrite(children[i].start, children[i].end, " ");
                    children[i].kind = ChildKind::Text;
                    i += 1;
                }
            }
            ChildKind::WhitespaceSpace => {
                // Always emit — this is a modification, not a removal
                out.overwrite(children[i].start, children[i].end, " ");
                children[i].kind = ChildKind::Text;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
}

/// Check if a child kind is an element or comment (for whitespace resolution).
#[inline]
fn is_element_or_comment(kind: ChildKind) -> bool {
    matches!(kind, ChildKind::Element | ChildKind::Comment)
}

#[inline]
pub(crate) fn is_whitespace_kind(kind: ChildKind) -> bool {
    matches!(
        kind,
        ChildKind::WhitespaceNewline | ChildKind::WhitespaceSpace
    )
}

// ======================== Condition chain cleanup ========================

/// Remove non-element children (comments, text) between v-if chain members.
///
/// Vue's compiler strips nodes between v-if/v-else-if/v-else branches.
/// For example, comments between `<div v-if>` and `<div v-else>` are
/// discarded and not rendered.
///
/// Scans backwards: when a Continuation element is found, removes all
/// preceding non-Element children until the previous Element is reached.
pub fn strip_interstitial_condition_nodes<'alloc>(
    children: &mut Vec<ChildRecord>,
    out: &mut CodeGenOutput<'alloc>,
    emit_removal_overwrites: bool,
) {
    let mut i = children.len();
    while i > 0 {
        i -= 1;
        if children[i].condition == Some(super::super::types::ConditionChainRole::Continuation) {
            // Remove all non-element children immediately before this continuation
            while i > 0 && children[i - 1].kind != ChildKind::Element {
                let removed = children.remove(i - 1);
                if emit_removal_overwrites {
                    out.overwrite(removed.start, removed.end, "");
                }
                i -= 1;
            }
        }
    }
}

// ======================== VNode construction ========================

/// Check if a string contains an assignment operator (`=`, `+=`, `-=`, etc.)
/// but NOT comparison operators (`===`, `!==`, `==`, `>=`, `<=`) or arrows (`=>`).
///
/// Used to detect event handler values like `dialog = true` that need wrapping
/// in `$event => { ... }` to be valid inside an object literal.
fn contains_assignment_operator(s: &str) -> bool {
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'=' {
            // Skip if preceded by !, >, <, = (comparison operators like !=, >=, <=, ==)
            if i > 0 && matches!(bytes[i - 1], b'!' | b'>' | b'<' | b'=') {
                continue;
            }
            // Skip if followed by = or > (== or =>)
            if i + 1 < bytes.len() && matches!(bytes[i + 1], b'=' | b'>') {
                continue;
            }
            return true;
        }
    }
    false
}
/// Check whether an expression is a simple member expression (identifier or
/// dot-separated property access like `foo`, `foo.bar`, `_ctx.onClick`).
/// Used to distinguish event handler references from inline handlers.
fn is_member_expression(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.')
}

/// Resolve an expression using OXC binding data when available, falling back
/// to simple identifier resolution.
///
/// Decodes HTML entities in the expression value first (e.g., `&quot;` → `"`),
/// since template attribute values may contain HTML-encoded characters from
/// preprocessor plugins (markdown, docs blocks, etc.).
pub(super) fn resolve_expr(
    expr: &str,
    value_start: u32,
    oxc_exp: Option<&OxcParsedExpression<'_>>,
    resolver: &BindingResolver<'_>,
    force_js: bool,
) -> String {
    // Compute TS removal spans when force_js is set and an OXC expression is available.
    let ts_skip_ranges: Vec<(u32, u32)> = if force_js {
        oxc_exp
            .and_then(|oxc| oxc.expression.as_ref())
            .map(|expr| crate::strip_types::typescript::collect_ts_removal_spans(expr))
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // When HTML entities are present (e.g., `&quot;` in template literal attributes),
    // we must keep binding positions aligned with the expression string passed to
    // build_prefixed_expr. OXC binding positions are relative to the original source
    // (with entities), so we pass the original expression for prefix insertion, then
    // decode entities in the final result.
    if helpers::has_html_entities(expr) {
        if let Some(oxc) = oxc_exp {
            // Build prefixed output from original source (positions are correct)
            let prefixed = build_prefixed_expr(expr, value_start, oxc, resolver, &ts_skip_ranges);
            // Decode entities in the result
            let mut decoded = String::with_capacity(prefixed.len());
            helpers::decode_html_entities_into(&mut decoded, &prefixed);
            decoded
        } else {
            let mut decoded = String::with_capacity(expr.len());
            helpers::decode_html_entities_into(&mut decoded, expr);
            resolver.resolve_simple_expr(&decoded)
        }
    } else if let Some(oxc) = oxc_exp {
        build_prefixed_expr(expr, value_start, oxc, resolver, &ts_skip_ranges)
    } else {
        resolver.resolve_simple_expr(expr)
    }
}

/// Result from [`build_props_object_into`].
pub struct PropsResult {
    pub dynamic_props: Vec<String>,
    pub uses_merge: bool,
    pub uses_normalize_class: bool,
    pub uses_normalize_style: bool,
    pub uses_with_modifiers: bool,
    pub uses_with_keys: bool,
}

/// Build the static props object into a buffer.
///
/// Handles static attributes and directive values with binding resolution.
/// Wraps `:class` values with `_normalizeClass()` and `:style` with `_normalizeStyle()`.
/// When `skip_prop_index` is `Some(i)`, prop at index `i` is excluded
/// (used for `:is` on `<component>` elements).
pub(crate) fn build_props_object_into(
    buf: &mut String,
    element: &ElementNode,
    source: &str,
    resolver: &BindingResolver<'_>,
    oxc_el: Option<&OxcParsedElement<'_>>,
    skip_prop_index: Option<usize>,
    force_js: bool,
) -> PropsResult {
    let mut dynamic_props: Vec<String> = Vec::with_capacity(4);
    // Collect spread expressions from v-bind/v-on without args.
    // These need _mergeProps() wrapping when mixed with regular props.
    let mut spreads: Vec<String> = Vec::with_capacity(2);
    let mut has_regular_props = false;
    let mut uses_normalize_class = false;
    let mut uses_normalize_style = false;
    let mut uses_with_modifiers = false;
    let mut uses_with_keys = false;

    // Pre-scan: detect if both static `class`/`style` and dynamic `:class`/`:style` exist.
    // When both exist, we must merge them into a single `class:`/`style:` prop using
    // _normalizeClass / _normalizeStyle, otherwise the second key overwrites the first.
    let mut static_class_value: Option<&str> = None;
    let mut has_dynamic_class = false;
    let mut static_style_value: Option<&str> = None;
    let mut has_dynamic_style = false;
    for prop in &element.props {
        if prop.is_directive {
            let directive_name = &source[prop.start as usize..prop.name_end as usize];
            let is_bind = directive_name == ":" || directive_name == "v-bind";
            if is_bind {
                if let (Some(arg_s), Some(arg_e)) = (prop.arg_start, prop.arg_end) {
                    let arg = &source[arg_s as usize..arg_e as usize];
                    if arg == "class" {
                        has_dynamic_class = true;
                    } else if arg == "style" {
                        has_dynamic_style = true;
                    }
                }
            }
        } else {
            let name = &source[prop.start as usize..prop.name_end as usize];
            if name == "class" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    static_class_value = Some(&source[vs as usize..ve as usize]);
                }
            } else if name == "style" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    static_style_value = Some(&source[vs as usize..ve as usize]);
                }
            }
        }
    }
    // Only keep the merge values when both static and dynamic co-exist
    let merge_class = if has_dynamic_class {
        static_class_value
    } else {
        None
    };
    let merge_style = if has_dynamic_style {
        static_style_value
    } else {
        None
    };

    for (prop_idx, prop) in element.props.iter().enumerate() {
        if skip_prop_index == Some(prop_idx) {
            continue;
        }
        if prop.is_directive {
            let directive_name = &source[prop.start as usize..prop.name_end as usize];
            let is_bind = directive_name == ":" || directive_name == "v-bind";
            let is_on = directive_name == "@" || directive_name == "v-on";

            if (is_bind || is_on) && prop.arg_start.is_none() {
                // v-bind="expr" or v-on="expr" without arg → spread
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let raw = &source[vs as usize..ve as usize];
                    let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
                    spreads.push(resolve_expr(raw, vs, oxc_exp, resolver, force_js));
                }
                continue;
            }

            if prop.arg_start.is_some() {
                has_regular_props = true;
            }
            // v-model on components expands to prop-based modelValue + onUpdate:modelValue
            if directive_name == "v-model" && element.tag_type.is_component() {
                has_regular_props = true;
            }
            // Directives with no arg and not v-bind/v-on (e.g., v-ripple, v-show, v-model native)
            // are not rendered as props — they use withDirectives() wrapping.
        } else {
            // Skip static class/style when it will be merged with dynamic `:class`/`:style`
            let name = &source[prop.start as usize..prop.name_end as usize];
            if (name == "class" && merge_class.is_some())
                || (name == "style" && merge_style.is_some())
            {
                continue;
            }
            has_regular_props = true;
        }
    }

    // Include v_ref in regular props count
    let has_ref = element.v_ref.is_some();
    if has_ref {
        has_regular_props = true;
    }

    // If we have spreads and regular props, use _mergeProps
    let use_merge = !spreads.is_empty() && has_regular_props;
    if use_merge {
        buf.push_str("_mergeProps(");
    }

    // Emit regular props object
    if has_regular_props || spreads.is_empty() {
        buf.push_str("{ ");
        let mut first = true;

        // Emit ref prop first (cached in v_ref, not in props vec)
        if let Some(ref_prop) = &element.v_ref {
            buf.push_str("ref: ");
            if let (Some(vs), Some(ve)) = (ref_prop.value_start, ref_prop.value_end) {
                let ref_value = &source[vs as usize..ve as usize];
                buf.push('"');
                buf.push_str(ref_value);
                buf.push('"');
            } else {
                // ref without value (rare, but handle gracefully)
                buf.push_str("\"\"");
            }
            first = false;
        }

        // Pre-scan: detect v-model + explicit @update:<name> on the same element.
        // When both exist, we must merge them into an array (like Vue does)
        // instead of emitting duplicate object keys.
        let mut vmodel_merge_targets: Vec<(usize, usize)> = Vec::with_capacity(2); // (vmodel_idx, handler_idx)
        {
            // Collect v-model prop indices and their event key names
            let mut vmodel_keys: Vec<(usize, String)> = Vec::with_capacity(2);
            for (idx, prop) in element.props.iter().enumerate() {
                if !prop.is_directive {
                    continue;
                }
                let dname = &source[prop.start as usize..prop.name_end as usize];
                if dname != "v-model" || !element.tag_type.is_component() {
                    continue;
                }
                let event_key = if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    let arg = &source[as_ as usize..ae as usize];
                    format!("update:{}", props::camelize(arg))
                } else {
                    "update:modelValue".to_string()
                };
                vmodel_keys.push((idx, event_key));
            }
            // Find matching explicit @update:<name> handlers
            for (idx, prop) in element.props.iter().enumerate() {
                if !prop.is_directive {
                    continue;
                }
                let dname = &source[prop.start as usize..prop.name_end as usize];
                if dname != "@" && dname != "v-on" {
                    continue;
                }
                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    let arg = &source[as_ as usize..ae as usize];
                    // Normalize: "update:model-value" → "update:modelValue"
                    let normalized = format!(
                        "update:{}",
                        props::camelize(arg.strip_prefix("update:").unwrap_or(arg))
                    );
                    for &(vmodel_idx, ref vmodel_key) in &vmodel_keys {
                        if normalized == *vmodel_key {
                            vmodel_merge_targets.push((vmodel_idx, idx));
                        }
                    }
                }
            }
        }
        // Build a set of handler prop indices that should be skipped
        // (they'll be merged into the v-model array instead)
        let merged_handler_indices: std::collections::HashSet<usize> =
            vmodel_merge_targets.iter().map(|&(_, hi)| hi).collect();

        for (prop_idx, prop) in element.props.iter().enumerate() {
            if skip_prop_index == Some(prop_idx) {
                continue;
            }
            // Skip explicit @update handlers that have been merged into a v-model array
            if merged_handler_indices.contains(&prop_idx) {
                continue;
            }
            // Track prop kind for value wrapping decisions.
            let mut prop_is_event = false;
            let mut prop_is_class = false;
            let mut prop_is_style = false;
            let mut is_static_style = false;
            // Event modifier categories (populated when prop_is_event is true)
            let mut event_runtime_modifiers: Vec<&str> = Vec::new();
            let mut event_key_modifiers: Vec<&str> = Vec::new();

            if prop.is_directive {
                let directive_name = &source[prop.start as usize..prop.name_end as usize];
                let is_bind = directive_name == ":" || directive_name == "v-bind";
                let is_on = directive_name == "@" || directive_name == "v-on";

                if (is_bind || is_on) && prop.arg_start.is_none() {
                    continue; // Handled as spread above
                }

                if let (Some(arg_start), Some(arg_end)) = (prop.arg_start, prop.arg_end) {
                    let arg_name = &source[arg_start as usize..arg_end as usize];

                    if !first {
                        buf.push_str(", ");
                    }
                    first = false;

                    // v-model:arg on a component: expand to named model prop + update handler
                    if directive_name == "v-model" && element.tag_type.is_component() {
                        let model_name = props::camelize(arg_name);
                        let needs_quote = props::needs_quoted_key(&model_name);
                        if needs_quote {
                            buf.push('"');
                        }
                        buf.push_str(&model_name);
                        if needs_quote {
                            buf.push('"');
                        }
                        buf.push_str(": ");

                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let raw_value = &source[vs as usize..ve as usize];
                            let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
                            let resolved = resolve_expr(raw_value, vs, oxc_exp, resolver, force_js);
                            buf.push_str(&resolved);

                            // Emit: "onUpdate:<name>": ...
                            buf.push_str(", \"onUpdate:");
                            buf.push_str(&model_name);
                            buf.push_str("\": ");

                            // Check if an explicit @update:<name> handler needs merging
                            let merge_handler_idx = vmodel_merge_targets
                                .iter()
                                .find(|&&(vi, _)| vi == prop_idx)
                                .map(|&(_, hi)| hi);

                            if let Some(hi) = merge_handler_idx {
                                // Merge: emit as array [vmodelHandler, explicitHandler]
                                buf.push_str("[$event => ((");
                                buf.push_str(&resolved);
                                buf.push_str(") = $event), ");
                                let handler_prop = &element.props[hi];
                                if let (Some(hvs), Some(hve)) =
                                    (handler_prop.value_start, handler_prop.value_end)
                                {
                                    let handler_raw = &source[hvs as usize..hve as usize];
                                    let handler_oxc = find_prop_oxc_exp(oxc_el, hi);
                                    let handler_resolved = resolve_expr(
                                        handler_raw,
                                        hvs,
                                        handler_oxc,
                                        resolver,
                                        force_js,
                                    );
                                    buf.push_str(&handler_resolved);
                                } else {
                                    buf.push_str("() => {}");
                                }
                                buf.push(']');
                            } else {
                                // No merge needed — single handler
                                buf.push_str("$event => ((");
                                buf.push_str(&resolved);
                                buf.push_str(") = $event)");
                            }

                            dynamic_props.push(model_name.to_string());
                            dynamic_props.push(format!("onUpdate:{}", model_name));

                            // Emit modifiers prop if present
                            if !prop.modifiers.is_empty() {
                                let mods_prop = if model_name == "modelValue" {
                                    "modelModifiers".to_string()
                                } else {
                                    format!("{}Modifiers", model_name)
                                };
                                buf.push_str(", ");
                                buf.push_str(&mods_prop);
                                buf.push_str(": { ");
                                for (mi, modifier) in prop.modifiers.iter().enumerate() {
                                    if mi > 0 {
                                        buf.push_str(", ");
                                    }
                                    let mod_name =
                                        &source[modifier.start as usize..modifier.end as usize];
                                    buf.push_str(mod_name);
                                    buf.push_str(": true");
                                }
                                buf.push_str(" }");
                            }
                        } else {
                            buf.push_str("true");
                        }
                        continue;
                    }

                    if is_on || directive_name == "@" {
                        prop_is_event = true;
                        // Event directive: @click → onClick, @update:modelValue → "onUpdate:modelValue"
                        // Build key in temp position, then check if quoting is needed
                        let key_start = buf.len();
                        props::format_event_handler_key_into(buf, arg_name);

                        // Classify modifiers into option (key suffix), runtime, and key categories
                        let mut runtime_modifiers: Vec<&str> = Vec::new();
                        let mut key_modifiers: Vec<&str> = Vec::new();
                        for modifier in &prop.modifiers {
                            let mod_name = &source[modifier.start as usize..modifier.end as usize];
                            match mod_name {
                                // Option modifiers: appended to key name
                                "capture" | "once" | "passive" => {
                                    // Capitalize and append to key name (e.g., onClick → onClickCapture)
                                    let first_char =
                                        mod_name.as_bytes()[0].to_ascii_uppercase() as char;
                                    buf.push(first_char);
                                    buf.push_str(&mod_name[1..]);
                                }
                                // Key modifiers: wrapped with _withKeys
                                "enter" | "tab" | "delete" | "esc" | "space" | "up" | "down"
                                | "left" | "right" => {
                                    // "left" and "right" are key modifiers on keyboard events,
                                    // but runtime modifiers on mouse events.
                                    if (mod_name == "left" || mod_name == "right")
                                        && !arg_name.starts_with("key")
                                    {
                                        runtime_modifiers.push(mod_name);
                                    } else {
                                        key_modifiers.push(mod_name);
                                    }
                                }
                                // Runtime modifiers: wrapped with _withModifiers
                                _ => {
                                    runtime_modifiers.push(mod_name);
                                }
                            }
                        }

                        let key_name = buf[key_start..].to_string();
                        dynamic_props.push(key_name);
                        if props::needs_quoted_key(&buf[key_start..]) {
                            buf.insert(key_start, '"');
                            buf.push('"');
                        }

                        // Store modifier info for use during value emission
                        event_runtime_modifiers = runtime_modifiers;
                        event_key_modifiers = key_modifiers;
                    } else {
                        // Track :class and :style for _normalizeClass/_normalizeStyle wrapping
                        if is_bind && arg_name == "class" {
                            prop_is_class = true;
                        } else if is_bind && arg_name == "style" {
                            prop_is_style = true;
                        }
                        // data-* and aria-* attributes must NOT be camelized —
                        // they are standard HTML attributes that preserve hyphens.
                        let skip_camelize =
                            arg_name.starts_with("data-") || arg_name.starts_with("aria-");
                        let key: std::borrow::Cow<'_, str> = if skip_camelize {
                            std::borrow::Cow::Borrowed(arg_name)
                        } else {
                            props::camelize(arg_name)
                        };
                        // For plain elements, :class and :style have dedicated
                        // patch flags (PATCH_CLASS / PATCH_STYLE) and must NOT
                        // appear in dynamic_props (which triggers PATCH_PROPS).
                        // Components need them in dynamic_props for
                        // shouldUpdateComponent checking.
                        let is_class_or_style =
                            is_bind && (arg_name == "class" || arg_name == "style");
                        if !is_class_or_style || element.tag_type.is_component() {
                            dynamic_props.push(key.to_string());
                        }
                        if props::needs_quoted_key(&key) {
                            buf.push('"');
                            buf.push_str(&key);
                            buf.push('"');
                        } else {
                            buf.push_str(&key);
                        }
                    }
                } else {
                    // v-model on a component: expand to modelValue + onUpdate:modelValue props
                    let is_vmodel = directive_name == "v-model";
                    if is_vmodel && element.tag_type.is_component() {
                        // Plain v-model (no arg) uses "modelValue" as the model name
                        let model_name = "modelValue";

                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let raw_value = &source[vs as usize..ve as usize];
                            let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
                            let resolved = resolve_expr(raw_value, vs, oxc_exp, resolver, force_js);

                            if !first {
                                buf.push_str(", ");
                            }
                            first = false;

                            // Emit: modelValue: <resolved>
                            buf.push_str(model_name);
                            buf.push_str(": ");
                            buf.push_str(&resolved);

                            // Emit: "onUpdate:modelValue": ...
                            buf.push_str(", \"onUpdate:");
                            buf.push_str(model_name);
                            buf.push_str("\": ");

                            // Check if an explicit @update:modelValue handler needs merging
                            let merge_handler_idx = vmodel_merge_targets
                                .iter()
                                .find(|&&(vi, _)| vi == prop_idx)
                                .map(|&(_, hi)| hi);

                            if let Some(hi) = merge_handler_idx {
                                // Merge: emit as array [vmodelHandler, explicitHandler]
                                buf.push_str("[$event => ((");
                                buf.push_str(&resolved);
                                buf.push_str(") = $event), ");
                                let handler_prop = &element.props[hi];
                                if let (Some(hvs), Some(hve)) =
                                    (handler_prop.value_start, handler_prop.value_end)
                                {
                                    let handler_raw = &source[hvs as usize..hve as usize];
                                    let handler_oxc = find_prop_oxc_exp(oxc_el, hi);
                                    let handler_resolved = resolve_expr(
                                        handler_raw,
                                        hvs,
                                        handler_oxc,
                                        resolver,
                                        force_js,
                                    );
                                    buf.push_str(&handler_resolved);
                                } else {
                                    buf.push_str("() => {}");
                                }
                                buf.push(']');
                            } else {
                                // No merge needed — single handler
                                buf.push_str("$event => ((");
                                buf.push_str(&resolved);
                                buf.push_str(") = $event)");
                            }

                            dynamic_props.push(model_name.to_string());
                            dynamic_props.push(format!("onUpdate:{}", model_name));

                            // Emit modifiers prop if present
                            if !prop.modifiers.is_empty() {
                                let mods_prop = if model_name == "modelValue" {
                                    "modelModifiers".to_string()
                                } else {
                                    format!("{}Modifiers", model_name)
                                };
                                buf.push_str(", ");
                                buf.push_str(&mods_prop);
                                buf.push_str(": { ");
                                for (mi, modifier) in prop.modifiers.iter().enumerate() {
                                    if mi > 0 {
                                        buf.push_str(", ");
                                    }
                                    let mod_name =
                                        &source[modifier.start as usize..modifier.end as usize];
                                    buf.push_str(mod_name);
                                    buf.push_str(": true");
                                }
                                buf.push_str(" }");
                            }
                        }
                        continue;
                    }

                    // Other directives with no arg (v-ripple, v-show, etc.) — skip in props
                    continue;
                }
            } else {
                // Static attribute — skip if being merged with dynamic `:class`/`:style`
                let name = &source[prop.start as usize..prop.name_end as usize];
                if (name == "class" && merge_class.is_some())
                    || (name == "style" && merge_style.is_some())
                {
                    continue;
                }

                is_static_style = name == "style";

                if !first {
                    buf.push_str(", ");
                }
                first = false;

                if props::needs_quoted_key(name) {
                    buf.push('"');
                    buf.push_str(name);
                    buf.push('"');
                } else {
                    buf.push_str(name);
                }
            }

            buf.push_str(": ");

            // Track whether this event handler needs modifier wrapping
            let has_event_modifiers = prop_is_event
                && (!event_runtime_modifiers.is_empty() || !event_key_modifiers.is_empty());

            // For event modifiers, we need to wrap the handler value.
            // Record the position before the handler so we can insert prefixes.
            let value_start_pos = if has_event_modifiers {
                Some(buf.len())
            } else {
                None
            };

            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];
                if prop.is_directive {
                    if prop_is_event && value.trim().is_empty() {
                        // Empty string event handler (e.g., @click.stop="")
                        buf.push_str("() => {}");
                    } else if prop_is_event
                        && (value.contains(';') || contains_assignment_operator(value))
                    {
                        // Multi-statement handlers (`;`) or assignment expressions
                        // (`dialog = true`) need wrapping to be valid in an
                        // object literal. Vue's official compiler does the same.
                        let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
                        let resolved = resolve_expr(value, vs, oxc_exp, resolver, force_js);
                        buf.push_str("$event => {");
                        buf.push_str(&resolved);
                        buf.push('}');
                    } else {
                        let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
                        let resolved = resolve_expr(value, vs, oxc_exp, resolver, force_js);
                        if prop_is_class {
                            buf.push_str("_normalizeClass(");
                            if let Some(static_cls) = merge_class {
                                // Merge static + dynamic: _normalizeClass(["static", dynamic])
                                buf.push_str("[\"");
                                helpers::escape_js_string_into(buf, static_cls);
                                buf.push_str("\", ");
                                buf.push_str(&resolved);
                                buf.push(']');
                            } else {
                                buf.push_str(&resolved);
                            }
                            buf.push(')');
                            uses_normalize_class = true;
                        } else if prop_is_style {
                            buf.push_str("_normalizeStyle(");
                            if let Some(static_sty) = merge_style {
                                // Merge static + dynamic: _normalizeStyle([{obj}, dynamic])
                                buf.push('[');
                                props::emit_static_style_object(buf, static_sty);
                                buf.push_str(", ");
                                buf.push_str(&resolved);
                                buf.push(']');
                            } else {
                                buf.push_str(&resolved);
                            }
                            buf.push(')');
                            uses_normalize_style = true;
                        } else if prop_is_event && !is_member_expression(value) {
                            // Inline event handler: @click="onClick(tab)"
                            // needs wrapping so it fires on click, not during render.
                            buf.push_str("$event => (");
                            buf.push_str(&resolved);
                            buf.push(')');
                        } else {
                            buf.push_str(&resolved);
                        }
                    }
                } else if is_static_style {
                    // Static style: emit as JS object for SSR normalization parity with Vue
                    let style_val = if helpers::has_html_entities(value) {
                        let mut decoded = String::with_capacity(value.len());
                        helpers::decode_html_entities_into(&mut decoded, value);
                        decoded
                    } else {
                        value.to_string()
                    };
                    props::emit_static_style_object(buf, &style_val);
                } else {
                    buf.push('"');
                    if helpers::has_html_entities(value) {
                        let mut decoded = String::with_capacity(value.len());
                        helpers::decode_html_entities_into(&mut decoded, value);
                        helpers::escape_js_string_into(buf, &decoded);
                    } else {
                        helpers::escape_js_string_into(buf, value);
                    }
                    buf.push('"');
                }
            } else if prop_is_event {
                // Event handler with no value (e.g., @contextmenu.prevent)
                buf.push_str("() => {}");
            } else {
                // Boolean attribute (no value)
                buf.push_str("\"\"");
            }

            // Wrap event handler with _withModifiers/_withKeys if modifiers are present
            if let Some(vsp) = value_start_pos {
                // Extract the handler value that was just written (split_off avoids clone)
                let handler_value = buf.split_off(vsp);

                // Innermost: _withKeys wrapping (if key modifiers present)
                if !event_key_modifiers.is_empty() {
                    uses_with_keys = true;
                    buf.push_str("_withKeys(");
                    buf.push_str(&handler_value);
                    buf.push_str(", [");
                    for (i, km) in event_key_modifiers.iter().enumerate() {
                        if i > 0 {
                            buf.push_str(", ");
                        }
                        buf.push('"');
                        buf.push_str(km);
                        buf.push('"');
                    }
                    buf.push_str("])");
                } else {
                    buf.push_str(&handler_value);
                }

                // Outermost: _withModifiers wrapping (if runtime modifiers present)
                if !event_runtime_modifiers.is_empty() {
                    uses_with_modifiers = true;
                    // Save the inner result, wrap with _withModifiers
                    let inner = buf.split_off(vsp);
                    buf.push_str("_withModifiers(");
                    buf.push_str(&inner);
                    buf.push_str(", [");
                    for (i, rm) in event_runtime_modifiers.iter().enumerate() {
                        if i > 0 {
                            buf.push_str(", ");
                        }
                        buf.push('"');
                        buf.push_str(rm);
                        buf.push('"');
                    }
                    buf.push_str("])");
                }
            }
        }
        buf.push_str(" }");
    }

    // Emit spread expressions
    for spread in &spreads {
        if has_regular_props || use_merge {
            buf.push_str(", ");
        }
        buf.push_str(spread);
    }

    if use_merge {
        buf.push(')');
    } else if !spreads.is_empty() && !has_regular_props {
        // Only spreads, no regular props — just emit the first spread
        // (if multiple, use _mergeProps)
        if spreads.len() > 1 {
            // Rewrite: wrap all spreads in _mergeProps
            // This case is unlikely but handle it properly
            let content =
                buf.split_off(buf.len() - spreads.iter().map(|s| s.len() + 2).sum::<usize>() + 2);
            let _ = content;
            buf.push_str("_mergeProps(");
            for (i, spread) in spreads.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                buf.push_str(spread);
            }
            buf.push(')');
        }
    }

    PropsResult {
        dynamic_props,
        uses_merge: use_merge,
        uses_normalize_class,
        uses_normalize_style,
        uses_with_modifiers,
        uses_with_keys,
    }
}

/// Process the leave phase of a VDOM element node.
///
/// 1. Resolves whitespace in children
/// 2. Constructs VNode call via overwrites
/// 3. Handles props, children, patch flags
/// 4. Returns a ChildRecord for the parent's children list
#[allow(clippy::too_many_arguments)]
pub fn process_element_leave<'alloc>(
    element: &ElementNode,
    oxc: Option<&OxcParsedElement<'alloc>>,
    children: &mut Vec<ChildRecord>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    options: &TemplateCodeGenOptions,
    resolver: &BindingResolver<'alloc>,
    buf: &mut String,
    v_for_prefix: Option<&str>,
) -> ChildRecord {
    let tag_open = &element.tag_open;
    debug_assert!((tag_open.start as usize + 1) <= source.len());
    debug_assert!((tag_open.name_end as usize) <= source.len());
    let tag_name = &source[tag_open.start as usize + 1..tag_open.name_end as usize];
    let helper = vnode_helper(element);

    // Step 1: Resolve whitespace and strip interstitial condition nodes
    // Pass false: tag extension + gap-filling below cover all removed regions,
    // so emitting removal overwrites here would create overlapping ranges.
    resolve_whitespace(children, out, false);
    strip_interstitial_condition_nodes(children, out, false);

    let has_props = !element.props.is_empty() || element.v_ref.is_some();
    let has_children = !children.is_empty();

    // Step 2: Compute patch flags
    let expr_flag = oxc
        .map(|o| o.expression_flag)
        .unwrap_or(ExpressionFlag::empty());
    let mut patch_flag =
        props::compute_patch_flags(element.prop_flag, expr_flag, element.children_mode);

    // Step 3: Build open tag overwrite (reusing caller's buffer)
    buf.clear();
    // Include v-for prefix (e.g., `(_openBlock(true), _createElementBlock(_Fragment, null,
    // _renderList(items, (item) => {return `) at the start of the overwrite. This ensures
    // it appears AFTER any sibling text node closing markers that are prepended at the
    // same position (overwrites come after prepends at the same position).
    if let Some(prefix) = v_for_prefix {
        buf.push_str(prefix);
    }
    // v-for direct children need their own block scope so that dynamic
    // component children are tracked in dynamicChildren and patched correctly.
    let is_vfor_child = element.v_for.is_some() && !element.tag_type.is_component();
    if is_vfor_child {
        buf.push_str("(_openBlock(), ");
        out.add_vdom_import(VdomHelper::OpenBlock);
        buf.push_str(VdomHelper::CreateElementBlock.name());
        out.add_vdom_import(VdomHelper::CreateElementBlock);
    } else {
        buf.push_str(helper.name());
    }
    // Check for <component :is="expr"> dynamic component (childless path)
    let dynamic_is =
        resolve_dynamic_component(element, source, oxc, resolver, out, options.force_js);
    let skip_is_prop = dynamic_is.as_ref().map(|(_, idx)| *idx);

    buf.push('(');
    if element.tag_type.is_component() {
        if let Some((ref resolved_tag, _)) = dynamic_is {
            // Dynamic component: <component :is="expr"> → _resolveDynamicComponent(expr)
            buf.push_str(resolved_tag);
        } else {
            // Static component: resolve through bindings or _resolveComponent
            let resolved = resolve_component_tag(tag_name, resolver, out, &options.self_name);
            buf.push_str(&resolved);
        }
    } else {
        // Plain element: string literal
        buf.push('"');
        buf.push_str(tag_name);
        buf.push('"');
    }

    // Add import for the helper
    out.add_vdom_import(helper);

    // Adjust has_props to account for the skipped :is prop
    let has_props = if skip_is_prop.is_some() {
        element.props.len() > 1 || element.v_ref.is_some()
    } else {
        has_props
    };

    // Props
    let dynamic_props = if has_props {
        buf.push_str(", ");
        let props_result = build_props_object_into(
            buf,
            element,
            source,
            resolver,
            oxc,
            skip_is_prop,
            options.force_js,
        );
        if props_result.uses_merge {
            out.add_vdom_import(VdomHelper::MergeProps);
        }
        if props_result.uses_normalize_class {
            out.add_vdom_import(VdomHelper::NormalizeClass);
        }
        if props_result.uses_normalize_style {
            out.add_vdom_import(VdomHelper::NormalizeStyle);
        }
        if props_result.uses_with_modifiers {
            out.add_vdom_import(VdomHelper::WithModifiers);
        }
        if props_result.uses_with_keys {
            out.add_vdom_import(VdomHelper::WithKeys);
        }
        props_result.dynamic_props
    } else {
        if has_children || patch_flag != 0 {
            // Need null placeholder for props when there are children or patch flags
            buf.push_str(", null");
        }
        Vec::new()
    };
    // For components with dynamic bound props, add PATCH_PROPS so that
    // shouldUpdateComponent can check listed dynamic props.
    // For plain elements, compute_patch_flags already handles PATCH_PROPS
    // based on PropFlags::HasDynamicBinding / HasEventListener.
    if !dynamic_props.is_empty() {
        patch_flag |= helpers::PATCH_PROPS;
    }

    // Children opening
    if has_children {
        buf.push_str(", ");
        match element.children_mode {
            ChildrenMode::TextOnlyStatic | ChildrenMode::TextOnlyDynamic => {
                // First child prefix
                if children[0].kind == ChildKind::Text {
                    buf.push('"');
                } else if children[0].kind == ChildKind::Interpolation {
                    buf.push_str("_toDisplayString");
                    out.add_vdom_import(VdomHelper::ToDisplayString);
                }
            }
            ChildrenMode::Mixed | ChildrenMode::MultiElement => {
                buf.push('[');
            }
            _ => {}
        }
    }

    // Pre-build patch flag suffix (flag + dynamic props array) once, reuse in both
    // self-closing and close-tag paths.
    let patch_suffix: &str = if patch_flag != 0 {
        let saved = buf.len();
        buf.push_str(", ");
        let flag_str =
            helpers::format_patch_flag(patch_flag, options.is_production, |s| out.alloc_str(s));
        buf.push_str(flag_str);
        // Dynamic props array — required when PATCH_PROPS (8) is set.
        // Vue's runtime reads `n2.dynamicProps` to know which props changed;
        // omitting this causes `Cannot read properties of null (reading 'length')`.
        if (patch_flag & helpers::PATCH_PROPS) != 0 && !dynamic_props.is_empty() {
            buf.push_str(", [");
            for (i, key) in dynamic_props.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                buf.push('"');
                buf.push_str(key);
                buf.push('"');
            }
            buf.push(']');
        }
        let suffix = out.alloc_str(&buf[saved..]);
        buf.truncate(saved);
        suffix
    } else {
        ""
    };

    // Self-closing or no close tag: single overwrite for entire tag
    if element.is_self_closing || element.tag_close.is_none() {
        // Emit PatchFlags for self-closing elements (before closing paren)
        if patch_flag != 0 {
            // Need null children placeholder before patch flags
            if !has_children {
                buf.push_str(", null");
            }
            buf.push_str(patch_suffix);
        }
        buf.push(')');
        // Close the outer (_openBlock(), ...) wrapper for v-for children
        if is_vfor_child {
            buf.push(')');
        }
        out.overwrite(tag_open.start, tag_open.end, buf);

        return ChildRecord {
            start: tag_open.start,
            end: tag_open.end,
            kind: ChildKind::Element,
            condition: None,
            condition_prefix: None,
        };
    }

    // Overwrite open tag.
    // Extend the overwrite end to children[0].start (if children exist) to
    // consume any whitespace gap between `>` and the first child. The tokenizer
    // silently drops all-whitespace text segments before entities, so these
    // gaps aren't represented in the AST but still exist in the source.
    let open_end = if has_children {
        children[0].start
    } else {
        tag_open.end
    };
    out.overwrite(tag_open.start, open_end, buf);

    // Step 3b: Remove whitespace gaps between consecutive children.
    // The tokenizer silently drops all-whitespace text segments before entities,
    // so gaps can exist between any adjacent children (e.g., between &gt; and
    // &lt; entities separated by a newline). Overwrite these gaps to "".
    for i in 1..children.len() {
        let prev_end = children[i - 1].end;
        let next_start = children[i].start;
        if next_start > prev_end {
            out.overwrite(prev_end, next_start, "");
        }
    }

    // Step 4: Add children separators (and text run wrapping for array modes)
    add_children_separators(children, element.children_mode, out, options);

    // Step 5: Build close tag overwrite (reuse buffer)
    buf.clear();
    let tag_close = element.tag_close.as_ref().unwrap();

    // Children closing
    if has_children {
        match element.children_mode {
            ChildrenMode::TextOnlyStatic | ChildrenMode::TextOnlyDynamic => {
                // Last child suffix
                let last_kind = children.last().map(|c| c.kind).unwrap_or(ChildKind::Text);
                if last_kind == ChildKind::Text {
                    buf.push('"');
                }
                // Interpolation already ends with ) from interpolation.rs
            }
            ChildrenMode::Mixed | ChildrenMode::MultiElement => {
                buf.push(']');
            }
            _ => {}
        }
    }

    // Patch flag (only if non-zero) — reuse pre-built suffix
    if patch_flag != 0 {
        buf.push_str(patch_suffix);
    }

    buf.push(')');
    // Close the outer (_openBlock(), ...) wrapper for v-for children
    if is_vfor_child {
        buf.push(')');
    }
    // Extend the close tag overwrite start back to the last child's end
    // to consume trailing whitespace gaps (same reason as open tag above).
    let close_start = if has_children {
        children.last().unwrap().end
    } else {
        tag_close.start
    };
    out.overwrite(close_start, tag_close.end, buf);

    // Return child record for the parent
    ChildRecord {
        start: tag_open.start,
        end: tag_close.end,
        kind: ChildKind::Element,
        condition: None,
        condition_prefix: None,
    }
}

#[cfg(test)]
#[path = "element_tests.rs"]
mod tests;
