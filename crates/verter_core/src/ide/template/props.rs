//! Prop/attribute → JSX conversion for TSX template codegen.
//!
//! Converts Vue template props and events to JSX equivalents:
//! - Static attributes: pass through
//! - `:prop="expr"` → `prop={expr}`
//! - `@event="handler"` → `onEvent={handler}`
//! - `v-bind="obj"` → `{...obj}` (spread)
//! - `v-on="{ ... }"` → `{...{ ... }}` (spread events, #49)

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, ObjectPropertyKind};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

use crate::ast::types::{ElementNode, TagType};
use crate::ide::{event_to_jsx_name, get_directive_name};
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::types::CodeGenOutput;
use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;
use crate::template::oxc::types::{OxcParsedElement, OxcParsedProp};
use crate::types::NodeProp;

/// Process all props on an element, converting to JSX syntax.
///
/// `condition_guard` is the accumulated condition text from v-if scopes
/// (parent + own), used for type narrowing guards in arrow function props.
#[allow(clippy::too_many_arguments)]
pub fn process_element_props<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
    condition_guard: Option<&str>,
    is_jsx: bool,
) {
    let v_if_guard = condition_guard;

    // Pre-scan: does this element have v-show? If so, :style will be handled
    // by emit_v_show and must be skipped here to avoid orphaned binding prepends.
    let has_v_show = el
        .props
        .iter()
        .any(|p| p.is_directive && &source[p.start as usize..p.name_end as usize] == "v-show");

    // Pre-scan: track which JSX event names appear more than once.
    // When the same event (e.g., @keydown.space + @keydown.enter → both onKeyDown)
    // appears twice, subsequent occurrences must use spread syntax to avoid TS17001.
    let mut event_name_counts: rustc_hash::FxHashMap<String, u8> = Default::default();
    for prop in &el.props {
        if prop.is_directive {
            let dn = get_directive_name(prop, source);
            if (dn == "on" || dn == "@") && prop.is_dynamic != Some(true) {
                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    let event = &source[as_ as usize..ae as usize];
                    let jsx = event_to_jsx_name(event);
                    *event_name_counts.entry(jsx).or_default() += 1;
                }
            }
        }
    }
    let mut event_seen: rustc_hash::FxHashSet<String> = Default::default();

    // Pre-scan for class/style merge: when both static and dynamic exist,
    // we merge them into a single normalizeClass/normalizeStyle call.
    let merge_class = el.needs_class_merge();
    let merge_style = el.needs_style_merge();

    // Find static class/style prop indices and values for merging
    let mut static_class_idx: Option<usize> = None;
    let mut static_style_idx: Option<usize> = None;
    let mut static_class_value: Option<&str> = None;
    let mut static_style_value: Option<&str> = None;

    if merge_class || merge_style {
        for (i, prop) in el.props.iter().enumerate() {
            if prop.is_directive {
                continue;
            }
            let name = &source[prop.start as usize..prop.name_end as usize];
            if merge_class && name == "class" {
                static_class_idx = Some(i);
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    static_class_value = Some(&source[vs as usize..ve as usize]);
                }
            } else if merge_style && name == "style" {
                static_style_idx = Some(i);
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    static_style_value = Some(&source[vs as usize..ve as usize]);
                }
            }
        }
    }

    for (i, prop) in el.props.iter().enumerate() {
        // Find corresponding OXC data for this prop
        let oxc_prop = oxc_el.and_then(|el| el.props.iter().find(|p| p.prop_index == i));

        // Skip structural directives (v-if, v-for, v-slot) — handled separately
        if is_structural_directive(prop, source) {
            // Remove the directive from output
            remove_prop(prop, source, out);
            continue;
        }

        // Skip v-show — handled separately by emit_v_show
        if is_builtin_directive(prop, source, "show") {
            continue;
        }

        // Skip :style binding when v-show is present on the same element.
        // emit_v_show merges the :style expression into its style output,
        // so processing :style here would produce orphaned binding prepends
        // that leak as stray text after the overwritten style attribute.
        if has_v_show && prop.is_directive {
            let dn = get_directive_name(prop, source);
            if (dn == "bind" || dn == ":") && prop.is_dynamic != Some(true) {
                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    if &source[as_ as usize..ae as usize] == "style" {
                        continue;
                    }
                }
            }
        }

        // v-model expansion: convert to modelValue/onUpdate:modelValue prop pair
        if is_builtin_directive(prop, source, "model") {
            process_v_model(prop, oxc_prop, el, source, out, resolver, is_jsx);
            continue;
        }

        if !prop.is_directive {
            // Static class/style props that need merging: remove from output
            // (they'll be merged into the dynamic binding's normalizeClass/normalizeStyle)
            if static_class_idx == Some(i) {
                let prop_end = get_prop_end(prop);
                out.overwrite(prop.start, prop_end, "");
                continue;
            }
            if static_style_idx == Some(i) {
                let prop_end = get_prop_end(prop);
                out.overwrite(prop.start, prop_end, "");
                continue;
            }
            // Static attribute — pass through
            continue;
        }

        let dir_name = get_directive_name(prop, source);

        // `<component :is="...">` is rewritten at the element level.
        // Skip prop-level bind transform to avoid overlapping rewrites.
        if el.tag_type == TagType::Component
            && &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize]
                == "component"
            && dir_name == "bind"
            && prop.is_dynamic != Some(true)
        {
            if let (Some(arg_start), Some(arg_end)) = (prop.arg_start, prop.arg_end) {
                if source[arg_start as usize..arg_end as usize].trim() == "is" {
                    continue;
                }
            }
        }

        // Check if this is a :class or :style that needs merging
        if dir_name == "bind" && prop.is_dynamic != Some(true) {
            if let (Some(arg_start), Some(arg_end)) = (prop.arg_start, prop.arg_end) {
                let arg_name = &source[arg_start as usize..arg_end as usize];
                if merge_class && arg_name == "class" {
                    if let Some(static_val) = static_class_value {
                        process_merged_class_or_style(
                            prop,
                            oxc_prop,
                            source,
                            out,
                            resolver,
                            "class",
                            "___VERTER___normalizeClass",
                            static_val,
                        );
                        continue;
                    }
                }
                if merge_style && arg_name == "style" {
                    if let Some(static_val) = static_style_value {
                        process_merged_class_or_style(
                            prop,
                            oxc_prop,
                            source,
                            out,
                            resolver,
                            "style",
                            "___VERTER___normalizeStyle",
                            static_val,
                        );
                        continue;
                    }
                }
            }
        }

        match dir_name {
            "bind" => process_v_bind(prop, oxc_prop, source, out, alloc, resolver, v_if_guard),
            "on" => {
                // Check if this event name has been seen before (duplicate handler)
                let use_spread = if prop.is_dynamic != Some(true) {
                    if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                        let event = &source[as_ as usize..ae as usize];
                        let jsx = event_to_jsx_name(event);
                        let is_dup = event_name_counts.get(&jsx).copied().unwrap_or(0) > 1;
                        let first_time = event_seen.insert(jsx);
                        is_dup && !first_time
                    } else {
                        false
                    }
                } else {
                    false
                };
                process_v_on(prop, oxc_prop, source, out, alloc, resolver, v_if_guard, use_spread);
            }
            "html" => process_v_html(prop, oxc_prop, source, out, resolver),
            "text" => process_v_text(prop, oxc_prop, source, out, resolver),
            _ => {
                // Unknown directive — remove it (TSX can't represent custom directives)
                remove_prop(prop, source, out);
            }
        }
    }
}

/// Merge a static class/style value with a dynamic binding into a single
/// `normalizeClass([dynamic, "static"])` or `normalizeStyle([dynamic, "static"])` call.
#[allow(clippy::too_many_arguments)]
fn process_merged_class_or_style<'alloc>(
    prop: &NodeProp,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
    attr_name: &str,
    helper_name: &str,
    static_value: &str,
) {
    let prop_end = get_prop_end(prop);
    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
        let value_expr = &source[vs as usize..ve as usize];
        // Patch-based: overwrite only boundaries, apply binding patches individually.
        // This preserves source map tokens for identifiers in the expression
        // (e.g., `props.x` in `:class="{ 'key': props.x }"`).
        let leading_ws = (value_expr.len() - value_expr.trim_start().len()) as u32;
        let trailing_ws = (value_expr.len() - value_expr.trim_end().len()) as u32;
        let tvs = vs + leading_ws;
        let tve = ve - trailing_ws;
        out.overwrite(
            prop.start,
            tvs,
            &format!("{}={{{}([", attr_name, helper_name),
        );
        // Escape newlines in the static value to avoid unterminated string literals
        // when the value spans multiple lines (e.g., multi-line static style attributes).
        let escaped_static = static_value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', " ")
            .replace('\r', "");
        out.overwrite(
            tve,
            prop_end,
            &format!(",\"{}\"])}}", escaped_static),
        );
        if let Some(oxc_p) = oxc_prop {
            if let Some(ref exp) = oxc_p.exp {
                if let Some(ref bindings) = exp.bindings {
                    resolver.collect_binding_patches(bindings, out);
                }
            }
        }
    } else {
        // No dynamic value — shouldn't happen if merge flag is set, but handle gracefully
        let escaped = static_value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', " ")
            .replace('\r', "");
        out.overwrite(
            prop.start,
            prop_end,
            &format!("{}=\"{}\"", attr_name, escaped),
        );
    }
}

/// Process `v-bind` / `:` directive.
///
/// - `:prop="expr"` → `prop={expr}`
/// - `v-bind:prop="expr"` → `prop={expr}`
/// - `v-bind="obj"` → `{...obj}` (spread)
/// - `:[key]="val"` → `{...{[key]: val}}` (dynamic key)
fn process_v_bind<'alloc>(
    prop: &NodeProp,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
    v_if_guard: Option<&str>,
) {
    let has_arg = prop.arg_start.is_some();
    let raw_name = &source[prop.start as usize..prop.name_end as usize];

    if !has_arg {
        // `.foo="bar"` shorthand for v-bind prop modifier
        if raw_name.starts_with('.') {
            let key = raw_name.trim_start_matches('.').trim();
            if key.is_empty() {
                return;
            }
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value_expr = &source[vs as usize..ve as usize];
                let resolved = resolve_prefixed_expr(value_expr, vs, oxc_prop, resolver);
                let prop_end = get_prop_end(prop);
                out.overwrite(prop.start, prop_end, &format!("{}={{{}}}", key, resolved));
            } else {
                let resolved = resolver.resolve_simple_expr(&kebab_to_camel_case(key));
                let prop_end = get_prop_end(prop);
                out.overwrite(prop.start, prop_end, &format!("{}={{{}}}", key, resolved));
            }
            return;
        }

        // v-bind="obj" → spread: `{...obj}`
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value_expr = &source[vs as usize..ve as usize];
            let resolved = resolve_prefixed_expr(value_expr, vs, oxc_prop, resolver);
            let prop_end = get_prop_end(prop);

            // When the resolved expression is the original with a prefix prepended
            // (e.g., "___VERTER___instance." + "$attrs"), split the overwrite to
            // preserve the original identifier's source map position for TSGO hover.
            let trimmed = value_expr.trim();
            if let Some(prefix) = resolved.strip_suffix(trimmed) {
                let leading_ws = (value_expr.len() - value_expr.trim_start().len()) as u32;
                let trailing_ws = (value_expr.len() - value_expr.trim_end().len()) as u32;
                let tvs = vs + leading_ws;
                let tve = ve - trailing_ws;
                out.overwrite(prop.start, tvs, &format!("{{...{}", prefix));
                out.overwrite(tve, prop_end, "}");
            } else {
                out.overwrite(prop.start, prop_end, &format!("{{...{}}}", resolved));
            }
        }
        return;
    }

    let arg_start = prop.arg_start.unwrap();
    let arg_end = prop.arg_end.unwrap();
    let arg_name = &source[arg_start as usize..arg_end as usize];

    // Dynamic key: :[key]="val" → {...{[key]: val}}
    if prop.is_dynamic == Some(true) {
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value_expr = &source[vs as usize..ve as usize];
            let value_resolved = resolve_prefixed_expr(value_expr, vs, oxc_prop, resolver);
            let arg_expr = arg_name
                .trim()
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or(arg_name)
                .trim();
            let arg_expr_start =
                arg_start + (arg_expr.as_ptr() as usize - arg_name.as_ptr() as usize) as u32;
            let arg_resolved =
                resolve_prefixed_dynamic_arg(arg_expr, arg_expr_start, oxc_prop, resolver);
            let prop_end = get_prop_end(prop);
            out.overwrite(
                prop.start,
                prop_end,
                &format!("{{...{{[{}]: {}}}}}", arg_resolved, value_resolved),
            );
        }
        return;
    }

    // Static key: :prop="expr" → prop={expr}
    let prop_end = get_prop_end(prop);
    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
        let value_expr = &source[vs as usize..ve as usize];
        let resolved = resolve_prefixed_expr(value_expr, vs, oxc_prop, resolver);

        // Inject type narrowing guard into function-typed props (Part C Step 8).
        let guarded = if let Some(guard) = v_if_guard {
            inject_function_guard(&resolved, guard, oxc_prop)
        } else {
            None
        };
        let final_expr = guarded.as_deref().unwrap_or(&resolved);

        // When the expression is unchanged, split the overwrite to preserve
        // the original expression span for source mapping (TSGO hover).
        let trimmed_expr = value_expr.trim();
        if final_expr == trimmed_expr {
            let leading_ws = value_expr.len() - value_expr.trim_start().len();
            let trailing_ws = value_expr.len() - value_expr.trim_end().len();
            let tvs = vs + leading_ws as u32;
            let tve = ve - trailing_ws as u32;
            out.overwrite(prop.start, arg_start, "");
            out.overwrite(arg_end, tvs, "={");
            out.overwrite(tve, prop_end, "}");
        } else if let Some(prefix) = final_expr.strip_suffix(trimmed_expr) {
            // Prefix-only change (e.g., "___VERTER___instance." + "$attrs").
            // Split overwrite to preserve the original identifier's source map position.
            let leading_ws = (value_expr.len() - value_expr.trim_start().len()) as u32;
            let trailing_ws = (value_expr.len() - value_expr.trim_end().len()) as u32;
            let tvs = vs + leading_ws;
            let tve = ve - trailing_ws;
            out.overwrite(prop.start, arg_start, "");
            out.overwrite(arg_end, tvs, &format!("={{{}", prefix));
            out.overwrite(tve, prop_end, "}");
        } else if guarded.is_some() {
            // Guard was injected — the expression was rewritten, can't patch individually
            out.overwrite(prop.start, arg_start, "");
            out.overwrite(arg_end, prop_end, &format!("={{{}}}", final_expr));
        } else {
            // Patch-based: overwrite only boundaries, apply binding patches individually.
            // This preserves source map tokens for each identifier in the expression,
            // enabling TSGO hover on sub-expressions (e.g., `props.x` in `:class="{ 'key': props.x }"`).
            let leading_ws = (value_expr.len() - value_expr.trim_start().len()) as u32;
            let trailing_ws = (value_expr.len() - value_expr.trim_end().len()) as u32;
            let tvs = vs + leading_ws;
            let tve = ve - trailing_ws;
            out.overwrite(prop.start, arg_start, "");
            out.overwrite(arg_end, tvs, "={");
            out.overwrite(tve, prop_end, "}");
            if let Some(oxc_p) = oxc_prop {
                if let Some(ref exp) = oxc_p.exp {
                    if let Some(ref bindings) = exp.bindings {
                        resolver.collect_binding_patches(bindings, out);
                    }
                }
            }
        }
    } else {
        // `:foo` shorthand → `foo={ctx.foo}`; `:foo-bar` uses camelCase lookup.
        let resolved = resolver.resolve_simple_expr(&kebab_to_camel_case(arg_name.trim()));
        out.overwrite(prop.start, arg_start, "");
        out.overwrite(arg_end, prop_end, &format!("={{{}}}", resolved));
    }
}

/// Process `v-on` / `@` directive.
///
/// - `@click="handler"` → `onClick={handler}`
/// - `@click="handler($event)"` → `onClick={($event) => handler($event)}`
/// - `v-on="{ mousedown: doThis }"` → `{...{ mousedown: doThis }}` (spread, #49)
fn process_v_on<'alloc>(
    prop: &NodeProp,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
    v_if_guard: Option<&str>,
    use_spread: bool,
) {
    let has_arg = prop.arg_start.is_some();

    if !has_arg {
        // v-on="{ mousedown: doThis }" → spread: {...{ mousedown: doThis }}
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value_expr = &source[vs as usize..ve as usize];
            let resolved = resolve_prefixed_expr(value_expr, vs, oxc_prop, resolver);
            let rewritten = rewrite_v_on_object_literal_expr(&resolved);
            let prop_end = get_prop_end(prop);
            out.overwrite(prop.start, prop_end, &format!("{{...{}}}", rewritten));
        }
        return;
    }

    let arg_start = prop.arg_start.unwrap();
    let arg_end = prop.arg_end.unwrap();
    let event_name = &source[arg_start as usize..arg_end as usize];

    // Dynamic event name: @[eventName]="handler" → {...{[`on${eventName}`]: handler}}
    if prop.is_dynamic == Some(true) {
        let raw_arg = event_name
            .trim()
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or(event_name)
            .trim();
        let raw_arg_start =
            arg_start + (raw_arg.as_ptr() as usize - event_name.as_ptr() as usize) as u32;
        let resolved_arg = resolve_prefixed_dynamic_arg(raw_arg, raw_arg_start, oxc_prop, resolver);
        let prop_end = get_prop_end(prop);

        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value_expr = &source[vs as usize..ve as usize];
            let resolved_value = resolve_prefixed_expr(value_expr, vs, oxc_prop, resolver);
            out.overwrite(
                prop.start,
                prop_end,
                &format!(
                    "{{...{{[`on${{{}}}` as any]: {}}}}}",
                    resolved_arg, resolved_value
                ),
            );
        } else {
            out.overwrite(prop.start, prop_end, "");
        }
        return;
    }

    // Convert event name to JSX: click → onClick, update:modelValue → onUpdate:modelValue
    let jsx_event_name = event_to_jsx_name(event_name);

    // When this event name was already emitted as a JSX attribute on this element
    // (e.g., @keydown.space + @keydown.enter → duplicate onKeyDown), use spread
    // syntax to avoid TS17001 "cannot have multiple attributes with same name".
    if use_spread {
        let prop_end = get_prop_end(prop);
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value_expr = &source[vs as usize..ve as usize];
            let resolved_expr = resolve_prefixed_expr(value_expr, vs, oxc_prop, resolver);
            let resolved_expr = resolved_expr.trim();
            let is_simple = crate::template::code_gen::binding::is_simple_ident(resolved_expr)
                || (resolved_expr.contains('.') && !resolved_expr.contains('('))
                || resolved_expr.starts_with("(")
                || resolved_expr.starts_with("function")
                || resolved_expr.contains("=>");
            if is_simple {
                out.overwrite(
                    prop.start,
                    prop_end,
                    &format!("{{...{{\"{}\": {}}}}}", jsx_event_name, resolved_expr),
                );
            } else {
                out.overwrite(
                    prop.start,
                    prop_end,
                    &format!(
                        "{{...{{\"{}\": () => {{{}}}}}}}",
                        jsx_event_name, resolved_expr
                    ),
                );
            }
        } else {
            out.overwrite(prop.start, prop_end, "");
        }
        return;
    }

    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
        let value_expr = &source[vs as usize..ve as usize];
        let resolved_expr = resolve_prefixed_expr(value_expr, vs, oxc_prop, resolver);
        let resolved_expr = resolved_expr.trim();

        // Determine if the handler needs wrapping
        let is_simple_ident = crate::template::code_gen::binding::is_simple_ident(resolved_expr);
        let is_member_expr = resolved_expr.contains('.') && !resolved_expr.contains('(');
        let is_fn_expr = resolved_expr.starts_with("(")
            || resolved_expr.starts_with("function")
            || resolved_expr.contains("=>");
        let is_object_expr = resolved_expr.starts_with('{') && resolved_expr.ends_with('}');
        let has_event_param = resolved_expr.contains("$event");

        // Build prop end position (including modifiers and quotes)
        let prop_end = get_prop_end(prop);

        // Check if binding resolution changed the expression text.
        // When unchanged (common for TSX inline mode), we split the overwrite
        // to preserve the original expression span, keeping source map tokens
        // for TSGO hover.
        let trimmed_expr = value_expr.trim();
        let expr_unchanged = resolved_expr == trimmed_expr;

        // Calculate trimmed expression boundaries within the source.
        // Leading/trailing whitespace must be included in the overwrite prefix/suffix
        // to avoid emitting raw whitespace between the JSX prop name and expression.
        let leading_ws = value_expr.len() - value_expr.trim_start().len();
        let trailing_ws = value_expr.len() - value_expr.trim_end().len();
        let trimmed_vs = vs + leading_ws as u32;
        let trimmed_ve = ve - trailing_ws as u32;

        if is_fn_expr || is_object_expr {
            // Explicit function/object expressions are already valid handlers.
            if expr_unchanged {
                out.overwrite(prop.start, trimmed_vs, &format!("{}={{", jsx_event_name));
                out.overwrite(trimmed_ve, prop_end, "}");
            } else {
                // Patch-based: preserve source map tokens for sub-expressions.
                out.overwrite(prop.start, trimmed_vs, &format!("{}={{", jsx_event_name));
                out.overwrite(trimmed_ve, prop_end, "}");
                if let Some(oxc_p) = oxc_prop {
                    if let Some(ref exp) = oxc_p.exp {
                        if let Some(ref bindings) = exp.bindings {
                            resolver.collect_binding_patches(bindings, out);
                        }
                    }
                }
            }
        } else if has_event_param {
            // $event can only exist inside a callback parameter scope.
            let guard_prefix = v_if_guard
                .map(|guard| format!("if (!({})) {{ return undefined; }} ", guard))
                .unwrap_or_default();
            if expr_unchanged {
                out.overwrite(
                    prop.start,
                    trimmed_vs,
                    &format!("{}={{($event) => {{{}", jsx_event_name, guard_prefix),
                );
                out.overwrite(trimmed_ve, prop_end, "}}");
            } else {
                // Patch-based: preserve source map tokens inside callback body.
                out.overwrite(
                    prop.start,
                    trimmed_vs,
                    &format!("{}={{($event) => {{{}", jsx_event_name, guard_prefix),
                );
                out.overwrite(trimmed_ve, prop_end, "}}");
                if let Some(oxc_p) = oxc_prop {
                    if let Some(ref exp) = oxc_p.exp {
                        if let Some(ref bindings) = exp.bindings {
                            resolver.collect_binding_patches(bindings, out);
                        }
                    }
                }
            }
        } else if is_simple_ident || is_member_expr {
            // Simple handler: @click="handler" → onClick={handler}
            if expr_unchanged {
                out.overwrite(prop.start, trimmed_vs, &format!("{}={{", jsx_event_name));
                out.overwrite(trimmed_ve, prop_end, "}");
            } else {
                // Patch-based: preserve source map tokens.
                out.overwrite(prop.start, trimmed_vs, &format!("{}={{", jsx_event_name));
                out.overwrite(trimmed_ve, prop_end, "}");
                if let Some(oxc_p) = oxc_prop {
                    if let Some(ref exp) = oxc_p.exp {
                        if let Some(ref bindings) = exp.bindings {
                            resolver.collect_binding_patches(bindings, out);
                        }
                    }
                }
            }
        } else {
            // Inline expression: @click="count++" → onClick={() => count++}
            let guard_prefix = v_if_guard
                .map(|guard| format!("if (!({})) {{ return undefined; }} ", guard))
                .unwrap_or_default();
            if expr_unchanged {
                out.overwrite(
                    prop.start,
                    trimmed_vs,
                    &format!("{}={{() => {{{}", jsx_event_name, guard_prefix),
                );
                out.overwrite(trimmed_ve, prop_end, "}}");
            } else {
                // Patch-based: preserve source map tokens inside callback body.
                out.overwrite(
                    prop.start,
                    trimmed_vs,
                    &format!("{}={{() => {{{}", jsx_event_name, guard_prefix),
                );
                out.overwrite(trimmed_ve, prop_end, "}}");
                if let Some(oxc_p) = oxc_prop {
                    if let Some(ref exp) = oxc_p.exp {
                        if let Some(ref bindings) = exp.bindings {
                            resolver.collect_binding_patches(bindings, out);
                        }
                    }
                }
            }
        }
    } else {
        // Event with no value — just remove
        let prop_end = get_prop_end(prop);
        out.overwrite(prop.start, prop_end, "");
    }
}

/// Process `v-model` directive → expand to prop + update event pair.
///
/// - `v-model="count"` → `modelValue={count} onUpdate:modelValue={($event) => (count = $event)}`
/// - `v-model:title="val"` → `title={val} onUpdate:title={($event) => (val = $event)}`
/// - Modifiers are emitted as a modifiers prop (e.g., `modelModifiers={{ trim: true }}`)
fn process_v_model<'alloc>(
    prop: &NodeProp,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
    el: &ElementNode,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
    is_jsx: bool,
) {
    let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) else {
        // v-model with no value — remove
        let prop_end = get_prop_end(prop);
        out.overwrite(prop.start, prop_end, "");
        return;
    };

    let raw_expr = &source[vs as usize..ve as usize];
    let resolved = resolve_prefixed_expr(raw_expr, vs, oxc_prop, resolver);
    let prop_end = get_prop_end(prop);

    // Determine prop name: default "modelValue" or named v-model:xxx
    let is_dynamic_arg = prop.is_dynamic == Some(true);
    let (value_prop, update_event, modifier_prop) = if let (Some(arg_s), Some(arg_e)) =
        (prop.arg_start, prop.arg_end)
    {
        let arg = &source[arg_s as usize..arg_e as usize];
        if is_dynamic_arg {
            // Dynamic arg: v-model:[expr]="val" → spread syntax
            let raw_arg = arg
                .trim()
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or(arg)
                .trim();
            let raw_arg_start = arg_s + (raw_arg.as_ptr() as usize - arg.as_ptr() as usize) as u32;
            let resolved_arg =
                resolve_prefixed_dynamic_arg(raw_arg, raw_arg_start, oxc_prop, resolver);
            (
                format!("[{}]", resolved_arg),
                format!("[`onUpdate:${{{}}}`]", resolved_arg),
                format!("[`${{{}}}Modifiers`]", resolved_arg),
            )
        } else {
            let camel_arg = kebab_to_camel_case(arg);
            (
                camel_arg.clone(),
                format!("onUpdate:{}", camel_arg),
                format!("{}Modifiers", camel_arg),
            )
        }
    } else {
        (
            "modelValue".to_string(),
            "onUpdate:modelValue".to_string(),
            "modelModifiers".to_string(),
        )
    };

    // Build the replacement.
    // For native HTML elements, use the actual DOM property (value/checked)
    // and a valid JSX event handler (onInput/onChange).
    // For components, use modelValue prop + spread for the event handler
    // (since "onUpdate:modelValue" is not a valid JSX attribute name).
    let is_native = el.tag_type.is_element();

    let event_param = if is_jsx { "$event" } else { "$event: any" };

    let mut replacement = if is_dynamic_arg {
        // Dynamic: always use spread syntax for computed prop names
        format!(
            "{{...{{{}:{}, \"{}\":({}) => (({}) = $event)}}}}",
            value_prop, resolved, update_event, event_param, resolved
        )
    } else if is_native {
        // Native element: use DOM property + native event handler
        let tag = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
        let (dom_prop, event_name) = native_vmodel_prop_and_event(el, source, tag);

        // Check if the element already has an explicit handler for the same event
        // (e.g., @change on a checkbox with v-model). If so, skip v-model's handler
        // to avoid duplicate JSX attributes (TS17001).
        let vue_event = event_name
            .strip_prefix("on")
            .map(|s| {
                let mut c = s.chars();
                match c.next() {
                    Some(ch) => {
                        let lower = ch.to_lowercase().to_string();
                        format!("{}{}", lower, c.as_str())
                    }
                    None => String::new(),
                }
            })
            .unwrap_or_default();
        let has_explicit_handler = el.props.iter().any(|p| {
            p.is_directive && {
                let dn = get_directive_name(p, source);
                (dn == "on" || dn == "@")
                    && p.arg_start
                        .zip(p.arg_end)
                        .map(|(a, b)| &source[a as usize..b as usize] == vue_event)
                        .unwrap_or(false)
            }
        });

        // Check if the element already has an explicit binding for the DOM prop
        // (e.g., :checked on a radio with v-model). If so, skip v-model's prop.
        let has_explicit_prop = el.props.iter().any(|p| {
            if p.is_directive {
                let dn = get_directive_name(p, source);
                (dn == "bind" || dn == ":")
                    && p.arg_start
                        .zip(p.arg_end)
                        .map(|(a, b)| &source[a as usize..b as usize] == dom_prop)
                        .unwrap_or(false)
            } else {
                let name = &source[p.start as usize..p.name_end as usize];
                name == dom_prop
            }
        });

        if has_explicit_prop && has_explicit_handler {
            // Both prop and handler already exist — v-model is redundant
            String::new()
        } else if has_explicit_prop {
            // Only emit the event handler
            format!(
                "{}={{({}) => (({}) = $event)}}",
                event_name, event_param, resolved
            )
        } else if has_explicit_handler {
            // Only emit the DOM property, skip the event handler
            format!("{}={{{}}}", dom_prop, resolved)
        } else {
            format!(
                "{}={{{}}} {}={{({}) => (({}) = $event)}}",
                dom_prop, resolved, event_name, event_param, resolved
            )
        }
    } else {
        // Component: modelValue + spread for event handler
        format!(
            "{}={{{}}} {{...{{\"{}\":({}) => (({}) = $event)}}}}",
            value_prop, resolved, update_event, event_param, resolved
        )
    };

    // Emit modifiers prop if present
    if !prop.modifiers.is_empty() {
        replacement.push_str(&format!(
            " {}={{{{ {} }}}}",
            modifier_prop,
            prop.modifiers
                .iter()
                .map(|m| {
                    let name = &source[m.start as usize..m.end as usize];
                    format!("{}: true", name)
                })
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    out.overwrite(prop.start, prop_end, &replacement);
}

/// Determine the DOM property and event handler for v-model on native elements.
/// Returns (prop_name, event_name) — both are valid JSX attribute identifiers.
fn native_vmodel_prop_and_event(
    el: &ElementNode,
    source: &str,
    tag: &str,
) -> (&'static str, &'static str) {
    match tag {
        "input" => {
            // Check for type="checkbox" or type="radio"
            for prop in &el.props {
                if !prop.is_directive {
                    let name = &source[prop.start as usize..prop.name_end as usize];
                    if name == "type" {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let type_value = &source[vs as usize..ve as usize];
                            if type_value == "checkbox" || type_value == "radio" {
                                return ("checked", "onChange");
                            }
                        }
                    }
                }
            }
            ("value", "onInput")
        }
        "select" => ("value", "onChange"),
        "textarea" => ("value", "onInput"),
        _ => ("value", "onInput"),
    }
}

/// Process `v-html="expr"` → `innerHTML={expr}`.
fn process_v_html<'alloc>(
    prop: &NodeProp,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
) {
    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
        let expr = &source[vs as usize..ve as usize];
        let resolved = resolve_prefixed_expr(expr, vs, oxc_prop, resolver);
        let prop_end = get_prop_end(prop);
        out.overwrite(prop.start, prop_end, &format!("innerHTML={{{}}}", resolved));
    }
}

/// Process `v-text="expr"` → `textContent={expr}`.
fn process_v_text<'alloc>(
    prop: &NodeProp,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
) {
    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
        let expr = &source[vs as usize..ve as usize];
        let resolved = resolve_prefixed_expr(expr, vs, oxc_prop, resolver);
        let prop_end = get_prop_end(prop);
        out.overwrite(
            prop.start,
            prop_end,
            &format!("textContent={{{}}}", resolved),
        );
    }
}

// ── Helpers ───────────────────────────────────────────────────────

// get_directive_name is imported from crate::ide (tsx/mod.rs)

/// Check if a prop is a structural directive (v-if, v-else-if, v-else, v-for, v-slot).
fn is_structural_directive(prop: &NodeProp, source: &str) -> bool {
    if !prop.is_directive {
        return false;
    }
    let name = get_directive_name(prop, source);
    matches!(name, "if" | "else-if" | "else" | "for" | "slot")
}

/// Check if a prop is a specific built-in directive.
fn is_builtin_directive(prop: &NodeProp, source: &str, dir: &str) -> bool {
    if !prop.is_directive {
        return false;
    }
    get_directive_name(prop, source) == dir
}

/// Remove a prop from output by overwriting with empty string.
/// Handles the full span including value and modifiers.
fn remove_prop(prop: &NodeProp, _source: &str, out: &mut CodeGenOutput<'_>) {
    let prop_end = get_prop_end(prop);
    out.overwrite(prop.start, prop_end, "");
}

/// Get the end position of a prop (including value and closing quote).
pub(crate) fn get_prop_end(prop: &NodeProp) -> u32 {
    // Priority: value_end + 1 (closing quote) > modifiers end > name_end
    if let Some(ve) = prop.value_end {
        ve + 1 // +1 for closing quote
    } else if !prop.modifiers.is_empty() {
        prop.modifiers
            .last()
            .map(|m| m.end)
            .unwrap_or(prop.name_end)
    } else if let Some(ae) = prop.arg_end {
        // Check for dynamic arg closing bracket
        ae
    } else {
        prop.name_end
    }
}

// event_to_jsx_name is imported from crate::ide (tsx/mod.rs)

fn rewrite_v_on_object_literal_expr(expr: &str) -> String {
    let trimmed = expr.trim();
    if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return expr.to_string();
    }

    let alloc = Allocator::new();
    let Ok(parsed) = Parser::new(&alloc, trimmed, SourceType::mjs()).parse_expression() else {
        return expr.to_string();
    };
    let Expression::ObjectExpression(obj) = parsed else {
        return expr.to_string();
    };

    let mut rebuilt = String::from("{");
    let mut first = true;

    for prop in &obj.properties {
        let piece = match prop {
            ObjectPropertyKind::SpreadProperty(spread) => {
                let span = spread.argument.span();
                if span.end <= span.start {
                    continue;
                }
                format!(
                    "...{}",
                    trimmed[span.start as usize..span.end as usize].trim()
                )
            }
            ObjectPropertyKind::ObjectProperty(p) => {
                if p.computed {
                    let key_span = p.key.span();
                    let value_span = p.value.span();
                    if key_span.end <= key_span.start || value_span.end <= value_span.start {
                        continue;
                    }
                    let key_src = trimmed[key_span.start as usize..key_span.end as usize].trim();
                    let value_src =
                        trimmed[value_span.start as usize..value_span.end as usize].trim();
                    format!("[{}]: {}", key_src, value_src)
                } else {
                    let key_span = p.key.span();
                    let value_span = p.value.span();
                    if key_span.end <= key_span.start || value_span.end <= value_span.start {
                        continue;
                    }

                    let raw_key = trimmed[key_span.start as usize..key_span.end as usize].trim();
                    let Some(event_key) = parse_static_event_key(raw_key) else {
                        return expr.to_string();
                    };
                    let mapped = event_to_jsx_name(event_key);
                    let key = if crate::template::code_gen::binding::is_simple_ident(&mapped) {
                        mapped
                    } else {
                        format!("\"{}\"", mapped)
                    };
                    let value_src =
                        trimmed[value_span.start as usize..value_span.end as usize].trim();
                    format!("{}: {}", key, value_src)
                }
            }
        };

        if !first {
            rebuilt.push_str(", ");
        }
        first = false;
        rebuilt.push_str(&piece);
    }

    rebuilt.push('}');
    rebuilt
}

fn parse_static_event_key(raw_key: &str) -> Option<&str> {
    let trimmed = raw_key.trim();
    if let Some(stripped) = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
        })
    {
        return Some(stripped.trim());
    }
    if crate::template::code_gen::binding::is_simple_ident(trimmed) {
        return Some(trimmed);
    }
    None
}

/// Inject a type narrowing guard into a function-typed v-bind expression.
///
/// Detects function types via OXC AST and injects appropriate guards:
/// - Arrow expression `() => expr` → `() => !(guard)?undefined:expr`
/// - Arrow block `() => { stmts }` → `() => {if(!(guard))return; stmts}`
/// - Function expression `function() { stmts }` → `function() {if(!(guard))return; stmts}`
/// - Non-function: returns None (no guard needed)
fn inject_function_guard(
    resolved: &str,
    guard: &str,
    oxc_prop: Option<&OxcParsedProp<'_>>,
) -> Option<String> {
    let oxc_p = oxc_prop?;
    let exp = oxc_p.exp.as_ref()?;
    let expression = exp.expression.as_ref()?;

    match expression {
        Expression::ArrowFunctionExpression(arrow) => {
            // Find `=>` in the resolved string
            let arrow_pos = resolved.find("=>")?;
            let after_arrow = arrow_pos + 2;

            if arrow.expression {
                // Arrow expression body: inject ternary guard before body
                // () => expr → () => !(guard)?undefined:expr
                let body_start = resolved[after_arrow..]
                    .find(|c: char| !c.is_whitespace())
                    .map(|i| after_arrow + i)
                    .unwrap_or(after_arrow);
                let mut result = String::with_capacity(resolved.len() + guard.len() + 30);
                result.push_str(&resolved[..body_start]);
                result.push_str(&crate::ide::condition::build_ternary_guard(guard));
                result.push_str(&resolved[body_start..]);
                Some(result)
            } else {
                // Arrow block body: inject block guard after opening {
                // () => { stmts } → () => {if(!(guard))return; stmts}
                let brace_offset = resolved[after_arrow..].find('{')?;
                let brace_pos = after_arrow + brace_offset;
                let mut result = String::with_capacity(resolved.len() + guard.len() + 30);
                result.push_str(&resolved[..=brace_pos]);
                result.push_str(&crate::ide::condition::build_block_guard(guard));
                result.push_str(&resolved[brace_pos + 1..]);
                Some(result)
            }
        }
        Expression::FunctionExpression(_) => {
            // Function expression: inject block guard after opening { of body
            // function() { stmts } → function() {if(!(guard))return; stmts}
            // Find the closing `)` of parameters, then the next `{`
            let mut depth = 0i32;
            let mut paren_close = None;
            for (i, ch) in resolved.char_indices() {
                if ch == '(' {
                    depth += 1;
                }
                if ch == ')' {
                    depth -= 1;
                    if depth == 0 {
                        paren_close = Some(i);
                        // Don't break — we want the FIRST balanced close
                        break;
                    }
                }
            }
            let paren_close = paren_close?;
            let brace_offset = resolved[paren_close..].find('{')?;
            let brace_pos = paren_close + brace_offset;
            let mut result = String::with_capacity(resolved.len() + guard.len() + 30);
            result.push_str(&resolved[..=brace_pos]);
            result.push_str(&crate::ide::condition::build_block_guard(guard));
            result.push_str(&resolved[brace_pos + 1..]);
            Some(result)
        }
        _ => None, // Non-function: no guard needed
    }
}

fn resolve_prefixed_expr(
    raw_expr: &str,
    expr_start: u32,
    oxc_prop: Option<&OxcParsedProp<'_>>,
    resolver: &BindingResolver<'_>,
) -> String {
    if let Some(oxc_p) = oxc_prop {
        if let Some(ref exp) = oxc_p.exp {
            return build_prefixed_expr(raw_expr, expr_start, exp, resolver, &[]);
        }
    }
    resolver.resolve_simple_expr(raw_expr)
}

fn resolve_prefixed_dynamic_arg(
    raw_arg: &str,
    raw_arg_start: u32,
    oxc_prop: Option<&OxcParsedProp<'_>>,
    resolver: &BindingResolver<'_>,
) -> String {
    if let Some(oxc_p) = oxc_prop {
        if let Some(ref arg) = oxc_p.arg {
            return build_prefixed_expr(raw_arg, raw_arg_start, arg, resolver, &[]);
        }
    }
    resolver.resolve_simple_expr(raw_arg)
}

fn kebab_to_camel_case(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut upper_next = false;
    for ch in input.chars() {
        if ch == '-' {
            upper_next = true;
            continue;
        }
        if upper_next {
            for uc in ch.to_uppercase() {
                out.push(uc);
            }
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_to_jsx_click() {
        assert_eq!(event_to_jsx_name("click"), "onClick");
    }

    #[test]
    fn event_to_jsx_update_model() {
        assert_eq!(
            event_to_jsx_name("update:modelValue"),
            "onUpdate:modelValue"
        );
    }

    #[test]
    fn event_to_jsx_kebab() {
        assert_eq!(event_to_jsx_name("custom-event"), "onCustomEvent");
    }

    #[test]
    fn event_to_jsx_simple() {
        assert_eq!(event_to_jsx_name("input"), "onInput");
        assert_eq!(event_to_jsx_name("change"), "onChange");
        assert_eq!(event_to_jsx_name("mousedown"), "onMousedown");
    }

    #[test]
    fn event_to_jsx_multi_segment_kebab() {
        assert_eq!(event_to_jsx_name("my-custom-event"), "onMyCustomEvent");
    }

    #[test]
    fn get_prop_end_with_value() {
        let prop = NodeProp {
            start: 0,
            name_end: 5,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            value_start: Some(7),
            value_end: Some(12),
            modifiers: smallvec::smallvec![],
            is_dynamic: None,
        };
        assert_eq!(get_prop_end(&prop), 13); // value_end + 1
    }

    #[test]
    fn get_prop_end_without_value() {
        let prop = NodeProp {
            start: 0,
            name_end: 5,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            value_start: None,
            value_end: None,
            modifiers: smallvec::smallvec![],
            is_dynamic: None,
        };
        assert_eq!(get_prop_end(&prop), 5); // name_end
    }
}
