//! Prop/attribute → JSX conversion for TSX template codegen.
//!
//! Converts Vue template props and events to JSX equivalents:
//! - Static attributes: pass through
//! - `:prop="expr"` → `prop={expr}`
//! - `@event="handler"` → `onEvent={handler}`
//! - `v-bind="obj"` → `{...obj}` (spread)
//! - `v-on="{ ... }"` → `{...{ ... }}` (spread events, #49)

use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;

use verter_span::{SourceByteOffset, SourceByteRange};

use crate::ast::types::{ElementNode, TagType};
use crate::ide::template::emit::{emit_op, EmitOp, EmitText};
use crate::ide::{event_to_jsx_name, get_directive_name};
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::types::CodeGenOutput;
use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;
use crate::template::oxc::types::{OxcParsedElement, OxcParsedProp};
use crate::types::NodeProp;

/// A custom directive collected for `v-directive` emission in TSX output.
pub struct CollectedDirective {
    /// CamelCase directive name: `"vFocus"`, `"vClickOutside"`
    pub camel_name: String,
    /// Resolved value expression, or `"true"` for no-value directives
    pub value: String,
    /// Argument: `"\"foo\""` (quoted static), resolved expression (dynamic), or `"undefined"`
    pub arg: String,
    /// Modifiers object: `{"bar":true}` or `{}`
    pub modifiers: String,
}

/// Convert a directive name (without `v-` prefix) to camelCase with `v` prefix.
///
/// - `"focus"` → `"vFocus"`
/// - `"click-outside"` → `"vClickOutside"`
pub fn directive_name_to_camel(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 1);
    result.push('v');
    let mut capitalize_next = true;
    for ch in name.chars() {
        if ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            for upper in ch.to_uppercase() {
                result.push(upper);
            }
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

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
) -> Vec<CollectedDirective> {
    let mut collected_directives: Vec<CollectedDirective> = Vec::new();
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
            super::vmodel::process_v_model(prop, oxc_prop, el, source, out, resolver, is_jsx);
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
            "bind" => process_v_bind(
                prop, oxc_prop, source, out, alloc, resolver, v_if_guard, is_jsx,
            ),
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
                super::von::process_v_on(
                    prop, oxc_prop, source, out, alloc, resolver, v_if_guard, use_spread,
                );
            }
            "html" => process_v_html(prop, oxc_prop, source, out, resolver),
            "text" => process_v_text(prop, oxc_prop, source, out, resolver),
            _ => {
                // Custom directive — collect for v-directive emission (TS mode only).
                // Skip built-ins that are handled elsewhere.
                if matches!(dir_name, "show" | "model" | "cloak" | "memo" | "pre" | "is") {
                    remove_prop(prop, source, out);
                } else if is_jsx {
                    // JSX mode: strip custom directives (no TS-only v-directive support)
                    remove_prop(prop, source, out);
                } else {
                    // Build CollectedDirective
                    let camel_name = directive_name_to_camel(dir_name);

                    // Value — the custom-directive expression is relocated into a
                    // synthetic `___VERTER___runCustomDirective(...)` call, so it is
                    // resolved to a flat string via the shared `build_prefixed_expr`
                    // helper (no in-place mapping is possible here).
                    let value = if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let raw = &source[vs as usize..ve as usize];
                        match oxc_prop.and_then(|p| p.exp.as_ref()) {
                            Some(exp) => build_prefixed_expr(raw, vs, exp, resolver, &[]),
                            None => resolver.resolve_simple_expr(raw),
                        }
                    } else {
                        "true".to_string()
                    };

                    // Arg
                    let arg = if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                        let raw_arg = &source[as_ as usize..ae as usize];
                        if prop.is_dynamic == Some(true) {
                            // Dynamic arg: resolve as expression, strip brackets if present
                            let inner = raw_arg
                                .strip_prefix('[')
                                .and_then(|s| s.strip_suffix(']'))
                                .unwrap_or(raw_arg);
                            resolver.resolve_simple_expr(inner)
                        } else {
                            // Static arg: quote it
                            format!("\"{}\"", raw_arg)
                        }
                    } else {
                        "undefined".to_string()
                    };

                    // Modifiers
                    let modifiers = if prop.modifiers.is_empty() {
                        "{}".to_string()
                    } else {
                        let mut m = String::from("{");
                        for (i, modifier) in prop.modifiers.iter().enumerate() {
                            if i > 0 {
                                m.push(',');
                            }
                            let mod_name = &source[modifier.start as usize..modifier.end as usize];
                            m.push_str(&format!("\"{}\":true", mod_name));
                        }
                        m.push('}');
                        m
                    };

                    collected_directives.push(CollectedDirective {
                        camel_name,
                        value,
                        arg,
                        modifiers,
                    });

                    // Remove the directive from raw output
                    remove_prop(prop, source, out);
                }
            }
        }
    }

    collected_directives
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
        out.overwrite(tve, prop_end, &format!(",\"{}\"])}}", escaped_static));
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
#[allow(clippy::too_many_arguments)]
fn process_v_bind<'alloc>(
    prop: &NodeProp,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
    v_if_guard: Option<&str>,
    is_jsx: bool,
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
                // `bar` is preserved in place; `key={` / `}` are unmapped boundaries.
                emit_in_place_jsx_value(
                    prop,
                    oxc_prop,
                    source,
                    out,
                    resolver,
                    vs,
                    ve,
                    &format!("{}={{", key),
                    "}",
                );
            } else {
                let resolved = resolver.resolve_simple_expr(&kebab_to_camel_case(key));
                let prop_end = get_prop_end(prop);
                out.overwrite(prop.start, prop_end, &format!("{}={{{}}}", key, resolved));
            }
            return;
        }

        // v-bind="obj" → spread: `{...obj}`. `obj` is preserved in place (each
        // identifier maps back via binding patches); `{...` / `}` are unmapped.
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            emit_in_place_jsx_value(prop, oxc_prop, source, out, resolver, vs, ve, "{...", "}");
        }
        return;
    }

    let arg_start = prop.arg_start.unwrap();
    let arg_end = prop.arg_end.unwrap();
    let arg_name = &source[arg_start as usize..arg_end as usize];

    // Dynamic key: :[key]="val" → {...{[key]: val}}
    // Both the arg (`key`) and value (`val`) identifiers are preserved IN PLACE
    // and map back to their source spans; the punctuation `{...{[`, `]: `, `}}}`
    // is unmapped synthetic text.
    if prop.is_dynamic == Some(true) {
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value_expr = &source[vs as usize..ve as usize];
            let arg_expr = arg_name
                .trim()
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or(arg_name)
                .trim();
            let arg_expr_start =
                arg_start + (arg_expr.as_ptr() as usize - arg_name.as_ptr() as usize) as u32;
            let arg_expr_end = arg_expr_start + arg_expr.len() as u32;

            let value_leading_ws = (value_expr.len() - value_expr.trim_start().len()) as u32;
            let value_trailing_ws = (value_expr.len() - value_expr.trim_end().len()) as u32;
            let tvs = vs + value_leading_ws;
            let tve = ve - value_trailing_ws;
            let prop_end = get_prop_end(prop);

            // `{...{[` before the arg identifier.
            emit_op(
                out,
                &EmitOp::OverwriteSyntheticBoundary {
                    source: SourceByteRange::new(
                        SourceByteOffset(prop.start),
                        SourceByteOffset(arg_expr_start),
                    ),
                    text: EmitText::Static("{...{["),
                    anchor: None,
                },
            );
            // `key` preserved in place; per-identifier prefixes via arg bindings.
            if let Some(oxc_p) = oxc_prop {
                if let Some(ref arg) = oxc_p.arg {
                    if let Some(ref bindings) = arg.bindings {
                        resolver.collect_binding_patches(bindings, out);
                    }
                }
            }
            // `]: ` between the arg and the value identifier.
            emit_op(
                out,
                &EmitOp::OverwriteSyntheticBoundary {
                    source: SourceByteRange::new(
                        SourceByteOffset(arg_expr_end),
                        SourceByteOffset(tvs),
                    ),
                    text: EmitText::Static("]: "),
                    anchor: None,
                },
            );
            // `val` preserved in place; per-identifier prefixes via value bindings.
            if let Some(oxc_p) = oxc_prop {
                if let Some(ref exp) = oxc_p.exp {
                    if let Some(ref bindings) = exp.bindings {
                        resolver.collect_binding_patches(bindings, out);
                    }
                }
            }
            // `}}` closing the computed-key object (`{[key]: val}`) + the spread
            // (`{...}`).
            emit_op(
                out,
                &EmitOp::OverwriteSyntheticBoundary {
                    source: SourceByteRange::new(SourceByteOffset(tve), SourceByteOffset(prop_end)),
                    text: EmitText::Static("}}"),
                    anchor: None,
                },
            );
        }
        return;
    }

    // Static key: :prop="expr" → prop={expr}
    let prop_end = get_prop_end(prop);

    // CSSProperties satisfies annotation for :style with object literal (#31).
    let is_style_object = arg_name == "style" && {
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            source[vs as usize..ve as usize].trim().starts_with('{')
        } else {
            false
        }
    };
    let close_brace = if is_style_object && !is_jsx {
        " satisfies import('vue').CSSProperties}"
    } else {
        "}"
    };

    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
        let value_expr = &source[vs as usize..ve as usize];
        // Flat resolution drives guard-injection detection + the in-place split
        // branch decisions below; the identifier itself stays preserved in place.
        let resolved = match oxc_prop.and_then(|p| p.exp.as_ref()) {
            Some(exp) => build_prefixed_expr(value_expr, vs, exp, resolver, &[]),
            None => resolver.resolve_simple_expr(value_expr),
        };

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
            out.overwrite(tve, prop_end, close_brace);
        } else if let Some(prefix) = final_expr.strip_suffix(trimmed_expr) {
            // Prefix-only change (e.g., "___VERTER___instance." + "$attrs").
            // Split overwrite to preserve the original identifier's source map position.
            let leading_ws = (value_expr.len() - value_expr.trim_start().len()) as u32;
            let trailing_ws = (value_expr.len() - value_expr.trim_end().len()) as u32;
            let tvs = vs + leading_ws;
            let tve = ve - trailing_ws;
            out.overwrite(prop.start, arg_start, "");
            out.overwrite(arg_end, tvs, &format!("={{{}", prefix));
            out.overwrite(tve, prop_end, close_brace);
        } else if guarded.is_some() {
            // Guard was injected — the expression was rewritten, can't patch individually
            let close = if is_style_object && !is_jsx {
                format!("={{{} satisfies import('vue').CSSProperties}}", final_expr)
            } else {
                format!("={{{}}}", final_expr)
            };
            out.overwrite(prop.start, arg_start, "");
            out.overwrite(arg_end, prop_end, &close);
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
            out.overwrite(tve, prop_end, close_brace);
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

/// Emit a directive whose value is a single JSX-attribute expression kept
/// IN PLACE — `lead`/`trail` are the synthetic JSX boundaries (`innerHTML={` …
/// `}`) and the user expression bytes are preserved 1:1 so the identifier keeps
/// its exact source-map mapping.
///
/// Lowering (typed [`EmitOp`] discipline):
/// - `OverwriteSyntheticBoundary(prop.start..tvs, lead)` — delete + unmapped insert.
/// - `PreserveOriginal(tvs..tve)` — pure no-op; bytes stay an `Original` chunk.
/// - per-identifier prefix/suffix via `collect_binding_patches` (in-place prepends,
///   maps each sub-identifier).
/// - `OverwriteSyntheticBoundary(tve..prop_end, trail)` — delete + unmapped insert.
#[allow(clippy::too_many_arguments)]
fn emit_in_place_jsx_value<'alloc>(
    prop: &NodeProp,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
    vs: u32,
    ve: u32,
    lead: &str,
    trail: &str,
) {
    let value_expr = &source[vs as usize..ve as usize];
    let leading_ws = (value_expr.len() - value_expr.trim_start().len()) as u32;
    let trailing_ws = (value_expr.len() - value_expr.trim_end().len()) as u32;
    let tvs = vs + leading_ws;
    let tve = ve - trailing_ws;
    let prop_end = get_prop_end(prop);

    let lead_op = EmitOp::OverwriteSyntheticBoundary {
        source: SourceByteRange::new(SourceByteOffset(prop.start), SourceByteOffset(tvs)),
        text: EmitText::Borrowed(lead),
        anchor: None,
    };
    let preserve = EmitOp::PreserveOriginal {
        source: SourceByteRange::new(SourceByteOffset(tvs), SourceByteOffset(tve)),
    };
    let trail_op = EmitOp::OverwriteSyntheticBoundary {
        source: SourceByteRange::new(SourceByteOffset(tve), SourceByteOffset(prop_end)),
        text: EmitText::Borrowed(trail),
        anchor: None,
    };

    emit_op(out, &lead_op);
    emit_op(out, &preserve);
    // Per-identifier accessor prefixes/suffixes (e.g. `___VERTER___instance.`,
    // `.value`) applied in place — each maps its identifier back to source.
    if let Some(oxc_p) = oxc_prop {
        if let Some(ref exp) = oxc_p.exp {
            if let Some(ref bindings) = exp.bindings {
                resolver.collect_binding_patches(bindings, out);
            }
        }
    }
    emit_op(out, &trail_op);
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
        emit_in_place_jsx_value(
            prop,
            oxc_prop,
            source,
            out,
            resolver,
            vs,
            ve,
            "innerHTML={",
            "}",
        );
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
        emit_in_place_jsx_value(
            prop,
            oxc_prop,
            source,
            out,
            resolver,
            vs,
            ve,
            "textContent={",
            "}",
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

pub(super) fn kebab_to_camel_case(input: &str) -> String {
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
        // Kebab-case events should preserve hyphens (not camelize)
        assert_eq!(event_to_jsx_name("custom-event"), "onCustom-event");
    }

    #[test]
    fn event_to_jsx_simple() {
        assert_eq!(event_to_jsx_name("input"), "onInput");
        assert_eq!(event_to_jsx_name("change"), "onChange");
        assert_eq!(event_to_jsx_name("mousedown"), "onMousedown");
    }

    #[test]
    fn event_to_jsx_multi_segment_kebab() {
        // Multi-segment kebab should also preserve hyphens
        assert_eq!(event_to_jsx_name("my-custom-event"), "onMy-custom-event");
    }

    #[test]
    fn event_to_jsx_camel_case_preserved() {
        assert_eq!(event_to_jsx_name("myEvent"), "onMyEvent");
        assert_eq!(event_to_jsx_name("customHandler"), "onCustomHandler");
    }

    #[test]
    fn event_to_jsx_pascal_case_preserved() {
        assert_eq!(event_to_jsx_name("MyEvent"), "onMyEvent");
    }

    #[test]
    fn event_to_jsx_snake_case_preserved() {
        assert_eq!(event_to_jsx_name("my_event"), "onMy_event");
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

    #[test]
    fn directive_name_to_camel_simple() {
        assert_eq!(directive_name_to_camel("focus"), "vFocus");
    }

    #[test]
    fn directive_name_to_camel_hyphenated() {
        assert_eq!(directive_name_to_camel("click-outside"), "vClickOutside");
    }

    #[test]
    fn directive_name_to_camel_multi_hyphen() {
        assert_eq!(directive_name_to_camel("my-long-dir"), "vMyLongDir");
    }

    #[test]
    fn directive_name_to_camel_single_char() {
        assert_eq!(directive_name_to_camel("a"), "vA");
    }
}
