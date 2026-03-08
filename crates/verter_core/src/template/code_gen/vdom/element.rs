//! VDOM element code generation.
//!
//! Handles element open/close tag transformation, props object generation,
//! whitespace resolution, and patch flag emission. Children separators and
//! text-run wrapping live in [`super::children`]; component tag resolution
//! and dynamic component handling live in [`super::component`].
//!
//! The main entry point is [`process_element_leave`], called from
//! `VdomCodeGen::leave_element` after all children have been visited.

use crate::ast::types::{ChildrenMode, ElementNode, TemplateAst};
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::vapor::find_prop_oxc_exp;
use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;
use crate::template::oxc::types::{ExpressionFlag, OxcParsedElement, OxcParsedExpression};

use super::super::shared::helpers::{self, is_member_expression, VdomHelper};
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

/// Emit a single event handler's value into `buf`, with _withModifiers/_withKeys
/// wrapping as needed. Used to emit subsequent handlers in merged duplicate event
/// handler groups.
///
/// Returns `(uses_with_modifiers, uses_with_keys)`.
fn emit_merged_event_handler_value(
    buf: &mut String,
    prop: &crate::types::NodeProp,
    prop_idx: usize,
    source: &str,
    oxc_el: Option<&OxcParsedElement<'_>>,
    resolver: &BindingResolver<'_>,
    force_js: bool,
) -> (bool, bool) {
    let mut uses_wm = false;
    let mut uses_wk = false;

    let dname = &source[prop.start as usize..prop.name_end as usize];
    let is_on = dname == "@" || dname == "v-on";

    // Classify modifiers (only for @event directives, not :onXxx bindings)
    let mut runtime_mods: Vec<&str> = Vec::new();
    let mut key_mods: Vec<&str> = Vec::new();

    if is_on {
        if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
            let arg_name = &source[as_ as usize..ae as usize];
            for modifier in &prop.modifiers {
                let m = &source[modifier.start as usize..modifier.end as usize];
                match m {
                    "capture" | "once" | "passive" => {} // already in key name
                    "enter" | "tab" | "delete" | "esc" | "space" | "up" | "down" | "left"
                    | "right" => {
                        if (m == "left" || m == "right") && !arg_name.starts_with("key") {
                            runtime_mods.push(m);
                        } else {
                            key_mods.push(m);
                        }
                    }
                    _ => runtime_mods.push(m),
                }
            }
        }
    }

    let has_mods = !runtime_mods.is_empty() || !key_mods.is_empty();
    let vsp = if has_mods { Some(buf.len()) } else { None };

    // Emit the handler value
    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
        let value = &source[vs as usize..ve as usize];
        if is_on && value.trim().is_empty() {
            buf.push_str("() => {}");
        } else if is_on && (value.contains(';') || contains_assignment_operator(value)) {
            let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
            let resolved = resolve_expr(value, vs, oxc_exp, resolver, force_js);
            buf.push_str("$event => {");
            buf.push_str(&resolved);
            buf.push('}');
        } else if is_on && !is_member_expression(value) {
            let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
            let resolved = resolve_expr(value, vs, oxc_exp, resolver, force_js);
            buf.push_str("$event => (");
            buf.push_str(&resolved);
            buf.push(')');
        } else {
            let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
            let resolved = resolve_expr(value, vs, oxc_exp, resolver, force_js);
            buf.push_str(&resolved);
        }
    } else if is_on {
        buf.push_str("() => {}");
    }

    // Apply modifier wrapping
    if let Some(vsp) = vsp {
        let handler_value = buf.split_off(vsp);
        if !key_mods.is_empty() {
            uses_wk = true;
            buf.push_str("_withKeys(");
            buf.push_str(&handler_value);
            buf.push_str(", [");
            for (i, km) in key_mods.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                buf.push('"');
                helpers::escape_js_string_into(buf, km);
                buf.push('"');
            }
            buf.push_str("])");
        } else {
            buf.push_str(&handler_value);
        }
        if !runtime_mods.is_empty() {
            uses_wm = true;
            let inner = buf.split_off(vsp);
            buf.push_str("_withModifiers(");
            buf.push_str(&inner);
            buf.push_str(", [");
            for (i, rm) in runtime_mods.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                buf.push('"');
                helpers::escape_js_string_into(buf, rm);
                buf.push('"');
            }
            buf.push_str("])");
        }
    }

    (uses_wm, uses_wk)
}

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
/// Resolve an expression using OXC binding data when available, falling back
/// to simple identifier resolution.
///
/// Decodes HTML entities in the expression value first (e.g., `&quot;` → `"`),
/// since template attribute values may contain HTML-encoded characters from
/// preprocessor plugins (markdown, docs blocks, etc.).
pub(crate) fn resolve_expr(
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
/// Info about a v-model directive on a native element that needs
/// `_withDirectives()` wrapping in the final output.
pub struct NativeVModel {
    /// The resolved expression (e.g. `$setup.msg`)
    pub resolved_value: String,
    /// The Vue runtime directive helper (e.g. `_vModelText`, `_vModelCheckbox`)
    pub directive_helper: VdomHelper,
    /// Modifier object string (e.g. `{ trim: true, number: true }`), empty if none
    pub modifiers: String,
}

pub struct PropsResult {
    pub dynamic_props: Vec<String>,
    pub uses_merge: bool,
    pub uses_normalize_class: bool,
    pub uses_normalize_style: bool,
    pub uses_with_modifiers: bool,
    pub uses_with_keys: bool,
    pub uses_normalize_props: bool,
    pub uses_guard_reactive_props: bool,
    /// v-model on a native element (input/textarea/select) that needs
    /// `_withDirectives()` wrapping after the element VNode is created.
    pub native_vmodel: Option<NativeVModel>,
}

/// Determine the appropriate vModel directive helper for a native element.
///
/// - `<select>` → `_vModelSelect`
/// - `<textarea>` → `_vModelText`
/// - `<input type="checkbox">` → `_vModelCheckbox`
/// - `<input type="radio">` → `_vModelRadio`
/// - `<input :type="dynamic">` → `_vModelDynamic`
/// - `<input>` (text, default) → `_vModelText`
fn determine_native_vmodel_directive(
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
                    let is_bind = dname == ":" || dname == "v-bind";
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

// ── Pre-scan helpers for build_props_object_into ──────────────────

/// Result of pre-scanning for class/style merge requirements.
struct ClassStyleMerge<'a> {
    /// Static class value when both static `class` and dynamic `:class` exist.
    merge_class: Option<&'a str>,
    /// Static style value when both static `style` and dynamic `:style` exist.
    merge_style: Option<&'a str>,
}

/// Pre-scan element props to detect if both static and dynamic class/style exist.
/// When both exist, they must be merged via `_normalizeClass()`/`_normalizeStyle()`.
fn pre_scan_class_style_merge<'a>(
    element_props: &[crate::types::NodeProp],
    source: &'a str,
) -> ClassStyleMerge<'a> {
    let mut static_class_value: Option<&str> = None;
    let mut has_dynamic_class = false;
    let mut static_style_value: Option<&str> = None;
    let mut has_dynamic_style = false;
    for prop in element_props {
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
    ClassStyleMerge {
        merge_class: if has_dynamic_class {
            static_class_value
        } else {
            None
        },
        merge_style: if has_dynamic_style {
            static_style_value
        } else {
            None
        },
    }
}

/// Pre-scan for v-model directives paired with explicit `@update:<name>` handlers.
/// Returns `(vmodel_prop_idx, handler_prop_idx)` pairs that need merging into arrays.
fn pre_scan_vmodel_handler_merge(element: &ElementNode, source: &str) -> Vec<(usize, usize)> {
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

    let mut targets = Vec::with_capacity(2);
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
            let normalized = format!(
                "update:{}",
                props::camelize(arg.strip_prefix("update:").unwrap_or(arg))
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

/// Result of pre-scanning for duplicate event handler keys.
struct EventHandlerMerge {
    /// First handler index → all handler indices with the same key.
    first_to_group: std::collections::HashMap<usize, Vec<usize>>,
    /// Handler indices that are secondary (should be skipped in main loop).
    secondary: std::collections::HashSet<usize>,
}

/// Pre-scan for duplicate event handler keys that need array merging.
/// Vue's `dedupeProperties` merges props with the same `isOn` key into arrays.
fn pre_scan_event_handler_merge(
    element: &ElementNode,
    source: &str,
    skip_prop_index: Option<usize>,
    merged_handler_indices: &std::collections::HashSet<usize>,
) -> EventHandlerMerge {
    let mut event_key_groups: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (idx, prop) in element.props.iter().enumerate() {
        if !prop.is_directive
            || skip_prop_index == Some(idx)
            || merged_handler_indices.contains(&idx)
        {
            continue;
        }
        let dname = &source[prop.start as usize..prop.name_end as usize];
        let is_on = dname == "@" || dname == "v-on";
        let is_bind = dname == ":" || dname == "v-bind";
        if !is_on && !is_bind {
            continue;
        }
        let (arg_start, arg_end) = match (prop.arg_start, prop.arg_end) {
            (Some(a), Some(b)) => (a, b),
            _ => continue,
        };
        if prop.is_dynamic == Some(true) {
            continue;
        }
        let arg_name = &source[arg_start as usize..arg_end as usize];
        let key = if is_on {
            let mut k = String::with_capacity(arg_name.len() + 10);
            props::format_event_handler_key_into(&mut k, arg_name);
            for modifier in &prop.modifiers {
                let m = &source[modifier.start as usize..modifier.end as usize];
                if matches!(m, "capture" | "once" | "passive") {
                    let first_char = m.as_bytes()[0].to_ascii_uppercase() as char;
                    k.push(first_char);
                    k.push_str(&m[1..]);
                }
            }
            k
        } else {
            let camelized = props::camelize(arg_name);
            if camelized.len() > 2
                && camelized.starts_with("on")
                && camelized.as_bytes()[2].is_ascii_uppercase()
            {
                camelized.to_string()
            } else {
                continue;
            }
        };
        event_key_groups.entry(key).or_default().push(idx);
    }

    let mut first_to_group = std::collections::HashMap::new();
    let mut secondary = std::collections::HashSet::new();
    for indices in event_key_groups.into_values() {
        if indices.len() > 1 {
            for &idx in &indices[1..] {
                secondary.insert(idx);
            }
            first_to_group.insert(indices[0], indices);
        }
    }
    EventHandlerMerge {
        first_to_group,
        secondary,
    }
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
    let mut has_dynamic_key = false;
    let mut uses_normalize_class = false;
    let mut uses_normalize_style = false;
    let mut uses_with_modifiers = false;
    let mut uses_with_keys = false;
    let mut native_vmodel: Option<NativeVModel> = None;

    let ClassStyleMerge {
        merge_class,
        merge_style,
    } = pre_scan_class_style_merge(&element.props, source);

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
                if prop.is_dynamic == Some(true) {
                    has_dynamic_key = true;
                }
            }
            // v-model on components or native elements expands to props
            if directive_name == "v-model" {
                has_regular_props = true;
            }
            // Directives with no arg and not v-bind/v-on (e.g., v-ripple, v-show)
            // are not rendered as props — they use withDirectives() wrapping.
            // (v-model is handled in the second pass below)
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

    // Dynamic key names (:[expr]) need _normalizeProps wrapping when not using _mergeProps
    let wrap_normalize_for_dynamic_key = has_dynamic_key && !use_merge;

    // Emit regular props object
    if has_regular_props || spreads.is_empty() {
        if wrap_normalize_for_dynamic_key {
            buf.push_str("_normalizeProps(");
        }
        buf.push_str("{ ");
        let mut first = true;

        // Emit ref prop first (cached in v_ref, not in props vec)
        if let Some(ref_prop) = &element.v_ref {
            buf.push_str("ref: ");
            if let (Some(vs), Some(ve)) = (ref_prop.value_start, ref_prop.value_end) {
                let ref_value = &source[vs as usize..ve as usize];
                buf.push('"');
                helpers::escape_js_string_into(buf, ref_value);
                buf.push('"');
            } else {
                // ref without value (rare, but handle gracefully)
                buf.push_str("\"\"");
            }
            first = false;
        }

        let vmodel_merge_targets = pre_scan_vmodel_handler_merge(element, source);
        let merged_handler_indices: std::collections::HashSet<usize> =
            vmodel_merge_targets.iter().map(|&(_, hi)| hi).collect();

        let EventHandlerMerge {
            first_to_group: event_merge_first_to_group,
            secondary: event_merge_secondary,
        } = pre_scan_event_handler_merge(element, source, skip_prop_index, &merged_handler_indices);

        for (prop_idx, prop) in element.props.iter().enumerate() {
            if skip_prop_index == Some(prop_idx) {
                continue;
            }
            // Skip explicit @update handlers that have been merged into a v-model array
            if merged_handler_indices.contains(&prop_idx) {
                continue;
            }
            // Skip secondary event handlers — they'll be merged into the first handler's array
            if event_merge_secondary.contains(&prop_idx) {
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
                    } else if prop.is_dynamic == Some(true) && is_bind {
                        // Dynamic bind key: :[attrName]="value"
                        // Emit as computed property key: [resolvedExpr || ""]
                        // Strip brackets from arg_name
                        let inner = if arg_name.starts_with('[') && arg_name.ends_with(']') {
                            &arg_name[1..arg_name.len() - 1]
                        } else {
                            arg_name
                        };
                        let resolved = resolver.resolve_simple_expr(inner);
                        buf.push('[');
                        buf.push_str(&resolved);
                        buf.push_str(" || \"\"");
                        buf.push(']');
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
                            // Cross-file optimization: skip adding to dynamic_props
                            // when all bindings are const props (proven constant across
                            // all parent call sites). Only active with const_props data.
                            let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
                            let expr_bindings = oxc_exp.and_then(|e| e.bindings.as_ref());
                            if !resolver.all_bindings_const_props(expr_bindings) {
                                dynamic_props.push(key.to_string());
                            }
                        }
                        if props::needs_quoted_key(&key) {
                            buf.push('"');
                            helpers::escape_js_string_into(buf, &key);
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

                    // v-model on native element: emit "onUpdate:modelValue" prop
                    // and collect directive info for _withDirectives() wrapping.
                    if is_vmodel && !element.tag_type.is_component() {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let raw_value = &source[vs as usize..ve as usize];
                            let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
                            let resolved = resolve_expr(raw_value, vs, oxc_exp, resolver, force_js);

                            if !first {
                                buf.push_str(", ");
                            }
                            first = false;

                            // Emit: "onUpdate:modelValue": $event => ((<resolved>) = $event)
                            buf.push_str("\"onUpdate:modelValue\": $event => ((");
                            buf.push_str(&resolved);
                            buf.push_str(") = $event)");

                            // Determine the directive helper based on tag and type attribute
                            let tag_name = &source[element.tag_open.start as usize + 1
                                ..element.tag_open.name_end as usize];
                            let directive_helper =
                                determine_native_vmodel_directive(element, source, tag_name);

                            // Build modifiers object string
                            let modifiers = if prop.modifiers.is_empty() {
                                String::new()
                            } else {
                                let mut mods = String::from("{ ");
                                for (mi, modifier) in prop.modifiers.iter().enumerate() {
                                    if mi > 0 {
                                        mods.push_str(", ");
                                    }
                                    let mod_name =
                                        &source[modifier.start as usize..modifier.end as usize];
                                    mods.push_str(mod_name);
                                    mods.push_str(": true");
                                }
                                mods.push_str(" }");
                                mods
                            };

                            native_vmodel = Some(NativeVModel {
                                resolved_value: resolved,
                                directive_helper,
                                modifiers,
                            });
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

            // Track position before the value for potential array merge wrapping
            let handler_value_start = buf.len();

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
                        helpers::escape_js_string_into(buf, km);
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
                        helpers::escape_js_string_into(buf, rm);
                        buf.push('"');
                    }
                    buf.push_str("])");
                }
            }

            // If this prop is the first handler in a merge group, wrap value in array
            // and emit remaining handlers' values.
            if let Some(group) = event_merge_first_to_group.get(&prop_idx) {
                // Insert `[` before the first handler's value
                buf.insert(handler_value_start, '[');
                // Emit subsequent handlers' values
                for &other_idx in &group[1..] {
                    buf.push_str(", ");
                    let other_prop = &element.props[other_idx];
                    let (uwm, uwk) = emit_merged_event_handler_value(
                        buf, other_prop, other_idx, source, oxc_el, resolver, force_js,
                    );
                    if uwm {
                        uses_with_modifiers = true;
                    }
                    if uwk {
                        uses_with_keys = true;
                    }
                }
                buf.push(']');
            }
        }
        buf.push_str(" }");
        if wrap_normalize_for_dynamic_key {
            buf.push(')');
        }
    }
    let mut uses_normalize_props = wrap_normalize_for_dynamic_key;
    let mut uses_guard_reactive_props = false;

    if use_merge {
        // Spreads mixed with regular props → _mergeProps({...}, spread1, spread2)
        // No normalizeProps/guardReactiveProps needed — _mergeProps handles normalization
        for spread in &spreads {
            buf.push_str(", ");
            buf.push_str(spread);
        }
        buf.push(')');
    } else if !spreads.is_empty() && !has_regular_props {
        if spreads.len() == 1 {
            // Single spread, no regular props → _normalizeProps(_guardReactiveProps(expr))
            buf.push_str("_normalizeProps(_guardReactiveProps(");
            buf.push_str(&spreads[0]);
            buf.push_str("))");
            uses_normalize_props = true;
            uses_guard_reactive_props = true;
        } else {
            // Multiple spreads, no regular props → _mergeProps(expr1, expr2)
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
        uses_normalize_props,
        uses_guard_reactive_props,
        native_vmodel,
    }
}

/// Process the leave phase of a VDOM element node.
///
/// 1. Resolves whitespace in children
/// 2. Constructs VNode call via overwrites
/// 3. Handles props, children, patch flags
/// 4. Returns a ChildRecord for the parent's children list
///
/// `is_block_root` indicates this element is at a block-tree root position
/// (template single root, v-if branch, or v-for item). Block roots use
/// `_createElementBlock`/`_createBlock` and get `(_openBlock(), ...)` wrapping.
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
    ast: &TemplateAst,
    is_block_root: bool,
) -> ChildRecord {
    let tag_open = &element.tag_open;
    debug_assert!((tag_open.start as usize + 1) <= source.len());
    debug_assert!((tag_open.name_end as usize) <= source.len());
    let tag_name = &source[tag_open.start as usize + 1..tag_open.name_end as usize];
    let helper = vnode_helper(element, is_block_root);

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

    // Pre-scan: detect v-model on native elements for _withDirectives() wrapping.
    // We need to know this before building the open tag so we can prepend the wrapper.
    let has_native_vmodel = !element.tag_type.is_component()
        && element.props.iter().any(|p| {
            p.is_directive && {
                let dname = &source[p.start as usize..p.name_end as usize];
                dname == "v-model"
            }
        });

    // Step 3: Build open tag overwrite (reusing caller's buffer)
    buf.clear();
    // Include v-for prefix (e.g., `(_openBlock(true), _createElementBlock(_Fragment, null,
    // _renderList(items, (item) => {return `) at the start of the overwrite. This ensures
    // it appears AFTER any sibling text node closing markers that are prepended at the
    // same position (overwrites come after prepends at the same position).
    if let Some(prefix) = v_for_prefix {
        buf.push_str(prefix);
    }
    // Wrap with _withDirectives() for native v-model
    if has_native_vmodel {
        buf.push_str("_withDirectives(");
    }
    // Block root elements (v-for items, v-if branches) need their own block scope
    // so that dynamic children are tracked in dynamicChildren and patched correctly.
    // Template root: wrapping is handled by leave_template, not here.
    let needs_block_wrapper =
        is_block_root && (element.v_for.is_some() || element.v_condition.is_some());
    if needs_block_wrapper {
        buf.push_str("(_openBlock(), ");
        out.add_vdom_import(VdomHelper::OpenBlock);
    }
    buf.push_str(helper.name());
    out.add_vdom_import(helper);
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
        helpers::escape_js_string_into(buf, tag_name);
        buf.push('"');
    }

    // Adjust has_props to account for the skipped :is prop
    let has_props = if skip_is_prop.is_some() {
        element.props.len() > 1 || element.v_ref.is_some()
    } else {
        has_props
    };

    // Props
    let (dynamic_props, native_vmodel) = if has_props {
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
        if props_result.uses_normalize_props {
            out.add_vdom_import(VdomHelper::NormalizeProps);
        }
        if props_result.uses_guard_reactive_props {
            out.add_vdom_import(VdomHelper::GuardReactiveProps);
        }
        (props_result.dynamic_props, props_result.native_vmodel)
    } else {
        if has_children || patch_flag != 0 {
            // Need null placeholder for props when there are children or patch flags
            buf.push_str(", null");
        }
        (Vec::new(), None)
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
            ChildrenMode::Mixed | ChildrenMode::MultiElement | ChildrenMode::SingleElement => {
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
                helpers::escape_js_string_into(buf, key);
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
        // Close the outer (_openBlock(), ...) wrapper for block root elements
        if needs_block_wrapper {
            buf.push(')');
        }
        // Close _withDirectives() wrapper for native v-model
        if let Some(ref nvm) = native_vmodel {
            buf.push_str(", [[");
            buf.push_str(nvm.directive_helper.name());
            buf.push_str(", ");
            buf.push_str(&nvm.resolved_value);
            if !nvm.modifiers.is_empty() {
                buf.push_str(", void 0, ");
                buf.push_str(&nvm.modifiers);
            }
            buf.push_str("]])");
            out.add_vdom_import(VdomHelper::WithDirectives);
            out.add_vdom_import(nvm.directive_helper);
        }
        out.overwrite(tag_open.start, tag_open.end, buf);

        return ChildRecord {
            start: tag_open.start,
            end: tag_open.end,
            kind: ChildKind::Element,
            condition: None,
            condition_prefix: None,
            condition_expr_start: None,
            condition_binding_prefix_len: 0,
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
    let ast_children = element
        .content
        .as_ref()
        .map(|c| c.children.as_slice())
        .unwrap_or(&[]);
    add_children_separators(
        children,
        element.children_mode,
        out,
        options,
        source,
        ast,
        ast_children,
    );

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
            ChildrenMode::Mixed | ChildrenMode::MultiElement | ChildrenMode::SingleElement => {
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
    // Close the outer (_openBlock(), ...) wrapper for block root elements
    if needs_block_wrapper {
        buf.push(')');
    }
    // Close _withDirectives() wrapper for native v-model
    if let Some(ref nvm) = native_vmodel {
        buf.push_str(", [[");
        buf.push_str(nvm.directive_helper.name());
        buf.push_str(", ");
        buf.push_str(&nvm.resolved_value);
        if !nvm.modifiers.is_empty() {
            buf.push_str(", void 0, ");
            buf.push_str(&nvm.modifiers);
        }
        buf.push_str("]])");
        out.add_vdom_import(VdomHelper::WithDirectives);
        out.add_vdom_import(nvm.directive_helper);
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
        condition_expr_start: None,
        condition_binding_prefix_len: 0,
    }
}

#[cfg(test)]
#[path = "element_tests.rs"]
mod tests;
