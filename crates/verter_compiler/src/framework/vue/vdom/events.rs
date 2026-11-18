//! Event handlers: modifier classes, handler wrapping, cacheable inline
//! handlers, and the merge of duplicate listener keys.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::ast::types::ElementNode;
use crate::framework::vue::semantic::VueBindingView;
use crate::template::binding::prefixed_expr::find_prop_oxc_exp;
use crate::template::oxc::types::{OxcParsedElement, OxcParsedExpression};
use crate::template::vue_syntax::{
    classify_event_modifier, is_function_expression_value, is_member_expression,
    is_multi_statement_handler, trim_handler_body, EventModifierKind,
};

use crate::template::binding::prefixed_expr::resolve_expr;

/// Emit a single event handler's value into `buf`, with _withModifiers/_withKeys
/// wrapping as needed. Used to emit subsequent handlers in merged duplicate event
/// handler groups.
///
/// Returns `(uses_with_modifiers, uses_with_keys)`.
pub(super) fn emit_merged_event_handler_value(
    buf: &mut String,
    prop: &crate::types::NodeProp,
    prop_idx: usize,
    source: &str,
    oxc_el: Option<&OxcParsedElement<'_>>,
    resolver: &VueBindingView<'_>,
    force_js: bool,
) -> (bool, bool) {
    let mut uses_wm = false;
    let mut uses_wk = false;

    let dname = &source[prop.start as usize..prop.name_end as usize];
    let is_on = super::is_v_on(dname);

    // Classify modifiers (only for @event directives, not :onXxx bindings)
    let mut runtime_mods: Vec<&str> = Vec::new();
    let mut key_mods: Vec<&str> = Vec::new();

    if is_on {
        if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
            let arg_name = &source[as_ as usize..ae as usize];
            for modifier in &prop.modifiers {
                let m = &source[modifier.start as usize..modifier.end as usize];
                match classify_event_modifier(m, arg_name) {
                    EventModifierKind::Option => {} // already in key name
                    EventModifierKind::Runtime => runtime_mods.push(m),
                    EventModifierKind::Key => key_mods.push(m),
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
        } else if is_on {
            let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
            let resolved = resolve_expr(value, vs, oxc_exp, resolver, force_js);
            push_event_handler(buf, value, &resolved, oxc_exp);
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
                crate::template::vue_syntax::escape_js_string_into(buf, km);
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
                crate::template::vue_syntax::escape_js_string_into(buf, rm);
                buf.push('"');
            }
            buf.push_str("])");
        }
    }

    (uses_wm, uses_wk)
}

/// Write the function an inline `v-on` value stands for, as Vue writes it:
/// a function expression or a method reference is the handler itself, a
/// statement list becomes a block body and any other expression an
/// expression body. `resolved` is `value` with its bindings resolved. A
/// handler merged into a listener array is written the same way, so an
/// inline statement there runs on the event, not during render.
pub(super) fn push_event_handler(
    buf: &mut String,
    value: &str,
    resolved: &str,
    oxc_exp: Option<&OxcParsedExpression<'_>>,
) {
    if is_function_expression_value(value, oxc_exp) {
        // Already a function — emit as-is (no double-wrap no-op).
        buf.push_str(resolved);
    } else if is_multi_statement_handler(oxc_exp) {
        // A statement list needs a statement body; a single expression, an
        // assignment included, is an expression body.
        buf.push_str("$event => {");
        buf.push_str(resolved);
        buf.push('}');
    } else if !is_member_expression(value) {
        buf.push_str("$event => (");
        buf.push_str(trim_handler_body(resolved));
        buf.push(')');
    } else {
        buf.push_str(resolved);
    }
}

/// True when a `v-on` handler value is an inline statement/expression that
/// gets `$event => …` wrapped (a fresh function created on every render) —
/// exactly the case official Vue's `cacheHandlers` wraps in
/// `_cache[N] || (_cache[N] = …)` to keep a stable function identity. A
/// method reference (`@click="onClick"`, `@click="foo.bar"`) is already a
/// stable value and is neither `$event`-wrapped nor cached.
pub(crate) fn is_cacheable_inline_event_handler(
    value: &str,
    oxc_exp: Option<&OxcParsedExpression<'_>>,
) -> bool {
    !value.trim().is_empty()
        && !is_function_expression_value(value, oxc_exp)
        && (is_multi_statement_handler(oxc_exp) || !is_member_expression(value))
}

// Test-only instrumentation: counts how many times the duplicate-handler
// grouping maps are actually built. The merge gate is meant to skip that work
// whenever fewer than two candidate handlers exist (a collision is impossible),
// and this counter lets a test assert the gate directly rather than only
// through emitted shape. Compiled out of production builds.
#[cfg(test)]
thread_local! {
    static EVENT_MERGE_FULL_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Record that the duplicate-handler grouping path ran (gate did not short-circuit).
#[cfg(test)]
fn record_event_merge_full_scan() {
    EVENT_MERGE_FULL_SCANS.with(|c| c.set(c.get() + 1));
}

/// Reset the grouping-path counter on the current thread.
#[cfg(test)]
pub(crate) fn reset_event_merge_full_scan_count() {
    EVENT_MERGE_FULL_SCANS.with(|c| c.set(0));
}

/// Number of times the duplicate-handler grouping maps were built on the current
/// thread since the last [`reset_event_merge_full_scan_count`].
#[cfg(test)]
pub(crate) fn event_merge_full_scan_count() -> usize {
    EVENT_MERGE_FULL_SCANS.with(|c| c.get())
}

/// Result of pre-scanning for duplicate event handler keys.
pub(super) struct EventHandlerMerge {
    /// First handler index → all handler indices with the same key.
    pub(super) first_to_group: FxHashMap<usize, Vec<usize>>,
    /// Handler indices that are secondary (should be skipped in main loop).
    pub(super) secondary: FxHashSet<usize>,
}

/// Count directive props that could contribute a static handler key to the
/// duplicate-merge grouping (`v-on`, or `v-bind` whose camelized arg becomes an
/// `onX` key). Cheap and allocation-free: it never builds the handler key, so a
/// `v-bind` whose arg would *not* camelize to `onX` is counted optimistically.
///
/// Over-counting only triggers a redundant grouping pass that yields the same
/// (empty) result; it never under-counts a real collision, because every prop
/// that the grouping path would insert also satisfies these cheaper predicates.
fn count_merge_candidates(
    element: &ElementNode,
    source: &str,
    skip_prop_index: Option<usize>,
    merged_handler_indices: &FxHashSet<usize>,
) -> usize {
    let mut count = 0usize;
    for (idx, prop) in element.props.iter().enumerate() {
        if !prop.is_directive
            || skip_prop_index == Some(idx)
            || merged_handler_indices.contains(&idx)
        {
            continue;
        }
        let dname = &source[prop.start as usize..prop.name_end as usize];
        if !super::is_v_on(dname) && !super::is_v_bind(dname) {
            continue;
        }
        if prop.arg_start.is_none() || prop.arg_end.is_none() {
            continue;
        }
        if prop.is_dynamic == Some(true) {
            continue;
        }
        count += 1;
        if count >= 2 {
            return count;
        }
    }
    count
}

/// Pre-scan for duplicate event handler keys that need array merging.
/// Vue's `dedupeProperties` merges props with the same `isOn` key into arrays.
pub(super) fn pre_scan_event_handler_merge(
    element: &ElementNode,
    source: &str,
    skip_prop_index: Option<usize>,
    merged_handler_indices: &FxHashSet<usize>,
) -> EventHandlerMerge {
    // A duplicate handler key needs at least two candidate directives; with fewer
    // than two, `dedupeProperties` can never merge and the grouping maps would
    // always come back empty. Skip building them entirely in that common case.
    if count_merge_candidates(element, source, skip_prop_index, merged_handler_indices) < 2 {
        return EventHandlerMerge {
            first_to_group: FxHashMap::default(),
            secondary: FxHashSet::default(),
        };
    }

    #[cfg(test)]
    record_event_merge_full_scan();

    let mut event_key_groups: FxHashMap<String, Vec<usize>> = FxHashMap::default();
    for (idx, prop) in element.props.iter().enumerate() {
        if !prop.is_directive
            || skip_prop_index == Some(idx)
            || merged_handler_indices.contains(&idx)
        {
            continue;
        }
        let dname = &source[prop.start as usize..prop.name_end as usize];
        let is_on = super::is_v_on(dname);
        let is_bind = super::is_v_bind(dname);
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
            crate::template::vue_syntax::format_event_handler_key_into(&mut k, arg_name);
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
            let camelized = crate::template::vue_syntax::camelize(arg_name);
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

    let mut first_to_group = FxHashMap::default();
    let mut secondary = FxHashSet::default();
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
