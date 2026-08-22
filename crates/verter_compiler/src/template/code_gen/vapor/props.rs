//! Vapor prop setters code generation.
//!
//! `_setClass`, `_setProp`, `_renderEffect` for dynamic props.
//!
//! In Vapor mode, static props are baked into the HTML template string.
//! Dynamic props (`:class`, `:style`, `:attr`, `v-bind`) become effects
//! that update the DOM via setter functions inside `_renderEffect`.

use crate::ast::types::{ElementNode, PropFlags};
use crate::template::code_gen::binding::{BindingResolver, BindingType};
use crate::template::code_gen::shared::helpers::{
    self, is_member_expression, is_multi_statement_handler, VaporHelper, DELEGATABLE_EVENTS,
};
use crate::template::code_gen::types::{
    CodeGenOutput, VaporCounters, VaporEffect, VaporElementState,
};
use crate::template::oxc::types::{OxcParsedElement, OxcParsedExpression};

use super::{find_prop_oxc_exp, resolve_expr};

/// Shared context for vapor prop processing functions.
///
/// Bundles the ambient state passed through `process_dynamic_props`,
/// `process_event`, and `process_v_model` to reduce parameter counts.
pub struct VaporPropsContext<'a, 'alloc> {
    pub source: &'alloc str,
    pub resolver: &'a BindingResolver<'a>,
    pub state: &'a mut VaporElementState<'alloc>,
    pub counters: &'a mut VaporCounters,
    pub out: &'a mut CodeGenOutput<'alloc>,
    pub delegated_events: &'a mut Vec<&'alloc str>,
    pub delegated_events_set: &'a mut rustc_hash::FxHashSet<&'alloc str>,
    pub force_js: bool,
}

/// Runtime modifier names (wrapped via _withModifiers).
const RUNTIME_MODIFIERS: &[&str] = &[
    "stop", "prevent", "self", "ctrl", "shift", "alt", "meta", "left", "middle", "right", "exact",
];

/// Key modifier names (wrapped via _withKeys).
const KEY_MODIFIERS: &[&str] = &[
    "enter", "tab", "delete", "esc", "space", "up", "down", "left", "right",
];

/// Process dynamic props on an element, generating effects and statements.
///
/// Handles all directive types:
/// - `:class="expr"` → `_setClass(nN, expr)` (effect)
/// - `:style="expr"` → `_setStyle(nN, expr)` (effect)
/// - `:attr="expr"` → `_setProp(nN, "attr", expr)` (effect)
/// - `@event="handler"` → delegated or `_on()` (statement)
/// - `v-show="expr"` → `_applyVShow(nN, () => (expr))` (statement)
/// - `v-model="expr"` → `_applyTextModel(nN, ...)` (statement)
/// - `v-html="expr"` → `_setHtml(nN, expr)` (effect)
/// - `v-bind="obj"` → `_setDynamicProps(nN, [obj])` (effect)
///
/// Static props are already in the HTML template (handled by `build_open_tag`).
pub fn process_dynamic_props(
    element: &ElementNode,
    ctx: &mut VaporPropsContext<'_, '_>,
    oxc_el: Option<&OxcParsedElement<'_>>,
) {
    // Process template ref (ref="name") — emits _setTemplateRef
    if let Some(ref_prop) = &element.v_ref {
        process_template_ref(ref_prop, ctx.source, ctx.state, ctx.counters, ctx.out);
    }

    // Quick check: skip if no directive props exist at all.
    if !element.props.iter().any(|p| p.is_directive) {
        return;
    }

    for (prop_idx, prop) in element.props.iter().enumerate() {
        if !prop.is_directive {
            continue;
        }

        let name = &ctx.source[prop.start as usize..prop.name_end as usize];

        // ===== Event listeners: @event or v-on:event =====
        if name.starts_with('@') || name == "v-on" {
            let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
            process_event(prop, name, ctx, oxc_exp);
            continue;
        }

        let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
        let (value, value_start) = match (prop.value_start, prop.value_end) {
            (Some(vs), Some(ve)) => (&ctx.source[vs as usize..ve as usize], vs),
            _ => ("", 0),
        };

        // ===== v-show =====
        if name == "v-show" {
            let node_ref = ctx.state.ensure_node_ref(ctx.counters);
            let resolved = resolve_expr(value, value_start, oxc_exp, ctx.resolver, ctx.force_js);
            let mut stmt = String::with_capacity(48);
            stmt.push_str("_applyVShow(n");
            helpers::push_u32(&mut stmt, node_ref);
            stmt.push_str(", () => (");
            stmt.push_str(&resolved);
            stmt.push_str("))");
            ctx.state
                .child_statements
                .push((ctx.out.alloc_str(&stmt), &[]));
            ctx.out.add_vapor_import(VaporHelper::ApplyVShow);
            continue;
        }

        // ===== v-model =====
        if name == "v-model" {
            process_v_model(element, prop, value, value_start, ctx, oxc_exp);
            continue;
        }

        // ===== v-html =====
        if name == "v-html" {
            let node_ref = ctx.state.ensure_node_ref(ctx.counters);
            let resolved = resolve_expr(value, value_start, oxc_exp, ctx.resolver, ctx.force_js);
            ctx.state.own_effects.push(VaporEffect::SetHtml {
                node_ref,
                expr: ctx.out.alloc_str(&resolved),
            });
            ctx.out.add_vapor_import(VaporHelper::SetHtml);
            continue;
        }

        // ===== v-text: handled via interpolation mechanism =====
        if name == "v-text" {
            continue;
        }

        // ===== v-memo: handled at the root element level, not as a prop =====
        if name == "v-memo" {
            continue;
        }

        // ===== v-bind (with or without arg) =====
        // Determine argument if present
        let arg = match (prop.arg_start, prop.arg_end) {
            (Some(as_), Some(ae)) => Some(&ctx.source[as_ as usize..ae as usize]),
            _ => None,
        };

        // `:key` (or `v-bind:key`) is VNode-identity metadata, never a real
        // DOM attribute — official never emits a prop-setter for it.
        // `build_v_for_root`'s `extract_key_expr` already reads it
        // separately for `_createFor`'s trailing key callback
        // (`(item) => (item)`); processing it again here as an ordinary
        // dynamic prop double-handles it, emitting a bogus
        // `_setProp(nN, "key", …)` that renders a literal `key="…"` DOM
        // attribute (confirmed as the exact cause of the runtime HTML
        // mismatch — `<li key="[object Object]">` — on a nested `v-for`
        // with `:key`).
        if (name == ":" || name == "v-bind") && arg == Some("key") {
            continue;
        }

        // Dynamic bind key: `:[attrName]="value"` / `v-bind:[attrName]="value"`.
        // The key itself isn't known until runtime, so it can't be a literal
        // `_setProp(nN, "attr", …)` attr name — official routes it through
        // `_setDynamicProps` with a single computed-key object, mirroring
        // VDOM's `_normalizeProps({ [key]: value })` handling.
        if (name == ":" || name == "v-bind") && prop.is_dynamic == Some(true) {
            if let Some(raw_arg) = arg {
                let inner = raw_arg
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .unwrap_or(raw_arg);
                let key_expr = ctx.resolver.resolve_simple_expr(inner);
                let value_resolved =
                    resolve_expr(value, value_start, oxc_exp, ctx.resolver, ctx.force_js);
                let node_ref = ctx.state.ensure_node_ref(ctx.counters);
                let obj_expr = format!("{{ [{key_expr}]: {value_resolved} }}");
                ctx.state.own_effects.push(VaporEffect::SetDynamicProps {
                    node_ref,
                    expr: ctx.out.alloc_str(&obj_expr),
                });
                ctx.out.add_vapor_import(VaporHelper::SetDynamicProps);
            }
            continue;
        }

        if value.is_empty() && arg.is_none() {
            continue; // Skip directives without values
        }

        // v-bind="obj" spread
        if name == "v-bind" && arg.is_none() {
            let node_ref = ctx.state.ensure_node_ref(ctx.counters);
            let resolved = resolve_expr(value, value_start, oxc_exp, ctx.resolver, ctx.force_js);
            ctx.state.own_effects.push(VaporEffect::SetDynamicProps {
                node_ref,
                expr: ctx.out.alloc_str(&resolved),
            });
            ctx.out.add_vapor_import(VaporHelper::SetDynamicProps);
            continue;
        }

        // Ensure node ref for effects
        let node_ref = ctx.state.ensure_node_ref(ctx.counters);

        // Vue 3.4 same-name shorthand: `:id` with NO `="..."` at all
        // (`prop.value_start.is_none()`) binds to a setup binding named
        // after the (camelized) arg itself (`:id` == `:id="id"`) — resolved
        // as a bare identifier, mirroring VDOM's `bind_shorthand_arg`
        // handling. No OXC expression data exists for a value that was
        // never authored. An explicit but BLANK value (`:id=""`, `:id="  "`)
        // is a DIFFERENT authored shape — official rejects it outright
        // (`X_V_BIND_NO_EXPRESSION`, parser-diagnosed) rather than treating
        // it as shorthand; its own recovery falls back to a literal
        // empty-string expression, which this mirrors instead of silently
        // aliasing the authored-empty value to the shorthand binding.
        let resolved = if prop.value_start.is_none() {
            match arg {
                Some(attr_name) => {
                    let camelized = crate::template::code_gen::vdom::props::camelize(attr_name);
                    ctx.resolver.resolve_simple_expr(&camelized)
                }
                None => String::new(),
            }
        } else if value.trim().is_empty() {
            "\"\"".to_string()
        } else {
            resolve_expr(value, value_start, oxc_exp, ctx.resolver, ctx.force_js)
        };
        let resolved_expr = ctx.out.alloc_str(&resolved);

        // Cross-file optimization: if all bindings in the expression are const props,
        // emit the setter as a one-time direct statement instead of a reactive effect.
        let expr_bindings = oxc_exp.and_then(|e| e.bindings.as_ref());
        let is_const = ctx.resolver.all_bindings_const_props(expr_bindings);

        let mut has_prop_modifier = false;
        let mut has_attr_modifier = false;
        for modifier in &prop.modifiers {
            let mod_name = &ctx.source[modifier.start as usize..modifier.end as usize];
            match mod_name {
                "prop" => has_prop_modifier = true,
                "attr" => has_attr_modifier = true,
                _ => {}
            }
        }

        match classify_directive(
            arg,
            &element.prop_flag,
            has_prop_modifier,
            has_attr_modifier,
        ) {
            DirectiveKind::Class => {
                let effect = VaporEffect::SetClass {
                    node_ref,
                    expr: resolved_expr,
                };
                if is_const {
                    ctx.state
                        .child_statements
                        .push((ctx.out.alloc_str(&effect.to_statement()), &[]));
                } else {
                    ctx.state.own_effects.push(effect);
                }
                ctx.out.add_vapor_import(VaporHelper::SetClass);
            }
            DirectiveKind::Style => {
                let effect = VaporEffect::SetStyle {
                    node_ref,
                    expr: resolved_expr,
                };
                if is_const {
                    ctx.state
                        .child_statements
                        .push((ctx.out.alloc_str(&effect.to_statement()), &[]));
                } else {
                    ctx.state.own_effects.push(effect);
                }
                ctx.out.add_vapor_import(VaporHelper::SetStyle);
            }
            DirectiveKind::Prop(attr) => {
                let effect = VaporEffect::SetProp {
                    node_ref,
                    attr,
                    expr: resolved_expr,
                };
                if is_const {
                    ctx.state
                        .child_statements
                        .push((ctx.out.alloc_str(&effect.to_statement()), &[]));
                } else {
                    ctx.state.own_effects.push(effect);
                }
                ctx.out.add_vapor_import(VaporHelper::SetProp);
            }
            DirectiveKind::Attr(attr) => {
                let effect = VaporEffect::SetAttr {
                    node_ref,
                    attr,
                    expr: resolved_expr,
                };
                if is_const {
                    ctx.state
                        .child_statements
                        .push((ctx.out.alloc_str(&effect.to_statement()), &[]));
                } else {
                    ctx.state.own_effects.push(effect);
                }
                ctx.out.add_vapor_import(VaporHelper::SetAttr);
            }
            DirectiveKind::DomProp(attr) => {
                let effect = VaporEffect::SetDomProp {
                    node_ref,
                    attr,
                    expr: resolved_expr,
                };
                if is_const {
                    ctx.state
                        .child_statements
                        .push((ctx.out.alloc_str(&effect.to_statement()), &[]));
                } else {
                    ctx.state.own_effects.push(effect);
                }
                ctx.out.add_vapor_import(VaporHelper::SetDomProp);
            }
            DirectiveKind::Unknown => {}
        }
    }
}

/// Process a single event listener directive.
fn process_event(
    prop: &crate::types::NodeProp,
    name: &str,
    ctx: &mut VaporPropsContext<'_, '_>,
    oxc_exp: Option<&OxcParsedExpression<'_>>,
) {
    // Extract event name
    let event_name = if let Some(after_at) = name.strip_prefix('@') {
        if after_at.is_empty() {
            if let (Some(s), Some(e)) = (prop.arg_start, prop.arg_end) {
                &ctx.source[s as usize..e as usize]
            } else {
                return;
            }
        } else {
            after_at
        }
    } else if name == "v-on" {
        if let (Some(s), Some(e)) = (prop.arg_start, prop.arg_end) {
            &ctx.source[s as usize..e as usize]
        } else {
            return; // v-on="obj" spread
        }
    } else {
        return;
    };

    let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) else {
        return;
    };
    let handler = &ctx.source[vs as usize..ve as usize];
    let node_ref = ctx.state.ensure_node_ref(ctx.counters);

    // Collect modifiers
    let mut runtime_mods: Vec<&str> = Vec::new();
    let mut key_mods: Vec<&str> = Vec::new();
    // Listener-option modifiers (`capture`/`passive`/`once`), kept in AUTHORED
    // order — the official compiler emits the `_on(...)` options object in
    // the order the modifiers were written (`@click.once.capture` → `{ once:
    // true, capture: true }`), not a fixed schema order.
    let mut option_mods: Vec<&str> = Vec::new();
    let mut has_capture = false;
    let mut has_passive = false;
    let mut has_once = false;
    let mut has_delegate = false;

    for modifier in &prop.modifiers {
        let mod_name = &ctx.source[modifier.start as usize..modifier.end as usize];
        match mod_name {
            "capture" => {
                has_capture = true;
                option_mods.push("capture");
            }
            "passive" => {
                has_passive = true;
                option_mods.push("passive");
            }
            "once" => {
                has_once = true;
                option_mods.push("once");
            }
            "delegate" => has_delegate = true,
            m if RUNTIME_MODIFIERS.contains(&m) => runtime_mods.push(m),
            m if KEY_MODIFIERS.contains(&m) => key_mods.push(m),
            _ => {}
        }
    }

    // Register the modifier-wrapper helpers the handler will actually call —
    // `write_handler_expression` below emits the `_withModifiers`/`_withKeys`
    // call syntax but never wires its own import (VDOM/SSR wire theirs at
    // their own call sites; Vapor must do the same here).
    if !runtime_mods.is_empty() {
        ctx.out.add_vapor_import(VaporHelper::WithModifiers);
    }
    if !key_mods.is_empty() {
        ctx.out.add_vapor_import(VaporHelper::WithKeys);
    }

    let is_dynamic_arg = prop.is_dynamic == Some(true);
    let non_delegatable = has_capture || has_passive || has_once || is_dynamic_arg;

    // Extract resolver and force_js before mutable borrows for write_handler_expression
    let resolver = ctx.resolver;
    let force_js = ctx.force_js;

    // Official rc.5 (`@vue/compiler-vapor` `transformVOn`): delegation is now
    // OPT-IN via an explicit `.delegate` modifier (`isDelegatableEvent =
    // !!delegateModifier && arg.isStatic && delegatedEvents(arg.content)`) —
    // a bare `@click="handler"` with no modifier binds directly through
    // `_on()`, matching the rc.5 seed goldens exactly. Delegation is no
    // longer automatic for known-delegatable event names.
    if has_delegate && !non_delegatable && DELEGATABLE_EVENTS.contains(&event_name) {
        // Delegatable event: n{ref}.$evt{event} = _createInvoker(handler)
        let event_alloc = ctx.out.alloc_str(event_name);
        if ctx.delegated_events_set.insert(event_alloc) {
            ctx.delegated_events.push(event_alloc);
        }

        let mut line = String::with_capacity(64);
        line.push('n');
        helpers::push_u32(&mut line, node_ref);
        line.push_str(".$evt");
        line.push_str(event_name);
        line.push_str(" = _createInvoker(");
        write_handler_expression(
            &mut line,
            handler,
            prop.value_start.unwrap_or(0),
            resolver,
            &runtime_mods,
            &key_mods,
            oxc_exp,
            force_js,
        );
        line.push(')');
        ctx.state
            .child_statements
            .push((ctx.out.alloc_str(&line), &[]));
        ctx.out.add_vapor_import(VaporHelper::DelegateEvents);
        ctx.out.add_vapor_import(VaporHelper::CreateInvoker);
    } else {
        // Non-delegatable: _on(n{ref}, "event", handler, options?)
        let mut line = String::with_capacity(64);
        line.push_str("_on(n");
        helpers::push_u32(&mut line, node_ref);
        line.push_str(", \"");
        line.push_str(event_name);
        line.push_str("\", ");
        write_handler_expression(
            &mut line,
            handler,
            prop.value_start.unwrap_or(0),
            resolver,
            &runtime_mods,
            &key_mods,
            oxc_exp,
            force_js,
        );

        if !option_mods.is_empty() {
            line.push_str(", { ");
            for (i, m) in option_mods.iter().enumerate() {
                if i > 0 {
                    line.push_str(", ");
                }
                line.push_str(m);
                line.push_str(": true");
            }
            line.push_str(" }");
        }

        line.push(')');
        ctx.state
            .child_statements
            .push((ctx.out.alloc_str(&line), &[]));
        ctx.out.add_vapor_import(VaporHelper::On);
    }
}

/// Write a handler expression, wrapping with _withModifiers/_withKeys if needed.
#[allow(clippy::too_many_arguments)]
fn write_handler_expression(
    buf: &mut String,
    handler: &str,
    value_start: u32,
    resolver: &BindingResolver<'_>,
    runtime_mods: &[&str],
    key_mods: &[&str],
    oxc_exp: Option<&OxcParsedExpression<'_>>,
    force_js: bool,
) {
    // Check if handler is a simple identifier (method reference)
    let is_member = is_member_expression(handler);
    let resolved = resolve_expr(handler, value_start, oxc_exp, resolver, force_js);

    // Official (`genEventHandler`'s `isConstantBinding`, confirmed directly
    // against the vendored rc.5 source): the arrow-wrap is skipped ONLY for
    // a BARE identifier (no dots — `value.ast === null`; a genuine dotted
    // path like `foo.bar` always parses a sub-expression and is therefore
    // ALWAYS wrapped, regardless of `foo`'s own binding type) that resolves
    // to a `SETUP_CONST` binding — Vue's own `analyzeScriptBindings`
    // classifies function/class/enum declarations and imports as
    // `SETUP_CONST`, so `@click="onClick"` referencing a `function onClick
    // () {}` declaration emits the bare `_ctx.onClick`, never a wrapper —
    // confirmed against the pinned rc.5 golden for `props-emit.vue`.
    let is_constant_binding =
        !handler.contains('.') && resolver.get(handler) == Some(BindingType::SetupConst);
    let should_wrap = is_member && !is_constant_binding;

    // Whether the handler body is a statement LIST, read from the parse fact —
    // never probed out of the raw source text.
    let is_statement_list = is_multi_statement_handler(oxc_exp);

    if runtime_mods.is_empty() && key_mods.is_empty() {
        if should_wrap {
            buf.push_str("e => ");
            buf.push_str(&resolved);
            buf.push_str("(e)");
        } else if is_member {
            // Constant-binding member reference — bare, unwrapped.
            buf.push_str(&resolved);
        } else {
            push_inline_handler(buf, &resolved, is_statement_list);
        }
        return;
    }

    if !key_mods.is_empty() {
        buf.push_str("_withKeys(");
    }
    if !runtime_mods.is_empty() {
        buf.push_str("_withModifiers(");
        if should_wrap {
            buf.push_str("e => ");
            buf.push_str(&resolved);
            buf.push_str("(e)");
        } else if is_member {
            buf.push_str(&resolved);
        } else {
            push_inline_handler(buf, &resolved, is_statement_list);
        }
        buf.push_str(", [");
        for (i, m) in runtime_mods.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            buf.push('"');
            helpers::escape_js_string_into(buf, m);
            buf.push('"');
        }
        buf.push_str("])");
    } else if should_wrap {
        buf.push_str("e => ");
        buf.push_str(&resolved);
        buf.push_str("(e)");
    } else if is_member {
        buf.push_str(&resolved);
    } else {
        push_inline_handler(buf, &resolved, is_statement_list);
    }
    if !key_mods.is_empty() {
        buf.push_str(", [");
        for (i, m) in key_mods.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            buf.push('"');
            buf.push_str(m);
            buf.push('"');
        }
        buf.push_str("])");
    }
}

/// Push an inline handler expression, giving a statement LIST a block body and a
/// single expression a parenthesised one.
///
/// `is_statement_list` is the parse fact from
/// [`is_multi_statement_handler`][helpers::is_multi_statement_handler], never a
/// probe of the handler text.
fn push_inline_handler(buf: &mut String, resolved: &str, is_statement_list: bool) {
    let trimmed = helpers::trim_handler_body(resolved);
    if trimmed.is_empty() {
        // Empty handler: @event="" → no-op
        buf.push_str("$event => {}");
        return;
    }
    // Official Vue omits the `$event` parameter entirely when the handler
    // body never references it — `@click="count++"` compiles to `() =>
    // (count++)`, not `$event => (...)` (verified against the real
    // compiler). `$event` is a compiler-synthesized pseudo-identifier
    // meaningful only inside an inline handler body, not a real binding —
    // official's own `transformOn` makes this same call from the raw
    // handler text, so a text check here is not a resolver-core violation.
    let param = if references_event_param(trimmed) {
        "$event"
    } else {
        "()"
    };
    if is_statement_list {
        buf.push_str(param);
        buf.push_str(" => { ");
        buf.push_str(trimmed);
        buf.push_str(" }");
    } else {
        buf.push_str(param);
        buf.push_str(" => (");
        buf.push_str(trimmed);
        buf.push(')');
    }
}

/// Whether `body` references the `$event` pseudo-identifier as a whole
/// word (not merely as a substring of a longer identifier).
fn references_event_param(body: &str) -> bool {
    const NEEDLE: &str = "$event";
    let bytes = body.as_bytes();
    let mut start = 0;
    while let Some(pos) = body[start..].find(NEEDLE) {
        let idx = start + pos;
        let before_ok = idx == 0
            || !(bytes[idx - 1].is_ascii_alphanumeric()
                || bytes[idx - 1] == b'_'
                || bytes[idx - 1] == b'$');
        let after = idx + NEEDLE.len();
        let after_ok =
            after >= bytes.len() || !(bytes[after].is_ascii_alphanumeric() || bytes[after] == b'_');
        if before_ok && after_ok {
            return true;
        }
        start = idx + NEEDLE.len();
    }
    false
}

/// Process v-model directive.
fn process_v_model(
    element: &ElementNode,
    prop: &crate::types::NodeProp,
    value: &str,
    value_start: u32,
    ctx: &mut VaporPropsContext<'_, '_>,
    oxc_exp: Option<&OxcParsedExpression<'_>>,
) {
    let node_ref = ctx.state.ensure_node_ref(ctx.counters);
    let tag_name =
        &ctx.source[element.tag_open.start as usize + 1..element.tag_open.name_end as usize];

    // Determine model helper based on input type and tag
    let helper = determine_model_helper(element, tag_name, ctx.source);

    // Collect v-model modifiers (trim, number, lazy)
    let mut modifiers: Vec<&str> = Vec::new();
    for modifier in &prop.modifiers {
        let mod_name = &ctx.source[modifier.start as usize..modifier.end as usize];
        match mod_name {
            "trim" | "number" | "lazy" => modifiers.push(mod_name),
            _ => {}
        }
    }

    let resolved = resolve_expr(value, value_start, oxc_exp, ctx.resolver, ctx.force_js);
    let mut stmt = String::with_capacity(128);
    stmt.push_str(helper);
    stmt.push_str("(n");
    helpers::push_u32(&mut stmt, node_ref);
    stmt.push_str(", () => (");
    stmt.push_str(&resolved);
    stmt.push_str("), _value => (");
    stmt.push_str(&resolved);
    stmt.push_str(" = _value)");

    // Append modifiers object if present
    if !modifiers.is_empty() {
        stmt.push_str(", { ");
        for (i, m) in modifiers.iter().enumerate() {
            if i > 0 {
                stmt.push_str(", ");
            }
            stmt.push_str(m);
            stmt.push_str(": true");
        }
        stmt.push_str(" }");
    }

    stmt.push(')');

    ctx.state
        .child_statements
        .push((ctx.out.alloc_str(&stmt), &[]));

    match helper {
        "_applyTextModel" => ctx.out.add_vapor_import(VaporHelper::ApplyTextModel),
        "_applyCheckboxModel" => ctx.out.add_vapor_import(VaporHelper::ApplyCheckboxModel),
        "_applyRadioModel" => ctx.out.add_vapor_import(VaporHelper::ApplyRadioModel),
        "_applySelectModel" => ctx.out.add_vapor_import(VaporHelper::ApplySelectModel),
        _ => ctx.out.add_vapor_import(VaporHelper::ApplyTextModel),
    }
}

/// Determine which v-model helper to use based on element type.
fn determine_model_helper(element: &ElementNode, tag_name: &str, source: &str) -> &'static str {
    match tag_name {
        "select" => "_applySelectModel",
        "textarea" => "_applyTextModel",
        "input" => {
            // Check for type attribute
            for prop in &element.props {
                if prop.is_directive {
                    continue;
                }
                let attr_name = &source[prop.start as usize..prop.name_end as usize];
                if attr_name == "type" {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let type_val = &source[vs as usize..ve as usize];
                        return match type_val {
                            "checkbox" => "_applyCheckboxModel",
                            "radio" => "_applyRadioModel",
                            _ => "_applyTextModel",
                        };
                    }
                }
            }
            "_applyTextModel"
        }
        _ => "_applyTextModel",
    }
}

/// Process a cached template ref attribute (`ref="name"`) → `_setTemplateRef(nN, "name")`.
///
/// Uses the pre-cached `ElementNode::v_ref` field (populated by the syntax layer)
/// instead of scanning the props list. This mirrors how `v_for`, `v_once`, etc. work.
fn process_template_ref<'a>(
    ref_prop: &crate::types::NodeProp,
    source: &'a str,
    state: &mut VaporElementState<'a>,
    counters: &mut VaporCounters,
    out: &mut CodeGenOutput<'a>,
) {
    if let (Some(vs), Some(ve)) = (ref_prop.value_start, ref_prop.value_end) {
        let ref_name = &source[vs as usize..ve as usize];
        let node_ref = state.ensure_node_ref(counters);
        let mut stmt = String::with_capacity(40);
        stmt.push_str("_setTemplateRef(n");
        helpers::push_u32(&mut stmt, node_ref);
        stmt.push_str(", \"");
        stmt.push_str(ref_name);
        stmt.push_str("\")");
        state.child_statements.push((out.alloc_str(&stmt), &[]));
        out.add_vapor_import(VaporHelper::CreateTemplateRefSetter);
    }
}

/// Classification of a directive for Vapor prop handling.
enum DirectiveKind<'a> {
    /// `:class` binding.
    Class,
    /// `:style` binding.
    Style,
    /// `:attr` binding — DOM prop (e.g., `id`, `value`, `textContent`).
    Prop(&'a str),
    /// `:attr` binding — HTML attribute (e.g., `data-*`, `aria-*`, hyphenated).
    Attr(&'a str),
    /// `:attr.prop` binding — explicitly forced DOM property regardless of
    /// hyphenation (e.g. `:text-content.prop`).
    DomProp(&'a str),
    /// Unknown directive (skip).
    Unknown,
}

/// Check if an attribute name should use `_setAttr` instead of `_setProp`.
///
/// Vue 3.6 uses `_setAttr` for:
/// - `data-*` attributes
/// - `aria-*` attributes
/// - Hyphenated attributes (e.g., `my-custom-attr`)
/// - Non-standard HTML attributes
///
/// Standard DOM properties (e.g., `id`, `value`, `textContent`) use `_setProp`.
fn is_attr_not_prop(name: &str) -> bool {
    name.starts_with("data-") || name.starts_with("aria-") || name.contains('-')
}

/// Classify a directive arg + prop flags into a `DirectiveKind`.
///
/// `has_prop_modifier`/`has_attr_modifier` are the explicit `.prop`/`.attr`
/// modifiers read off the directive (`:text-content.prop`, `:data-x.attr`) —
/// they override the implicit hyphenation-based classification below, since
/// the official Vapor compiler forces DOM-property or attribute routing
/// regardless of the attribute name's shape when the modifier is present.
fn classify_directive<'a>(
    arg: Option<&'a str>,
    prop_flag: &crate::ast::types::PropFlag,
    has_prop_modifier: bool,
    has_attr_modifier: bool,
) -> DirectiveKind<'a> {
    // v-bind shorthand: :class, :style, :attr
    if let Some(attr_name) = arg {
        if attr_name == "class" && prop_flag.has(PropFlags::HasDynamicClass) {
            return DirectiveKind::Class;
        }
        if attr_name == "style" && prop_flag.has(PropFlags::HasDynamicStyle) {
            return DirectiveKind::Style;
        }
        if has_prop_modifier {
            return DirectiveKind::DomProp(attr_name);
        }
        if has_attr_modifier {
            return DirectiveKind::Attr(attr_name);
        }
        // Vue 3.6: data-*, aria-*, and hyphenated attrs use _setAttr; DOM props use _setProp
        if is_attr_not_prop(attr_name) {
            return DirectiveKind::Attr(attr_name);
        }
        return DirectiveKind::Prop(attr_name);
    }

    // Named directives without args
    // v-bind="obj" (spread) — handled separately
    // v-show, v-model — handled separately
    DirectiveKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::types::*;
    use crate::types::{NodeProp, NodeTag};
    use oxc_allocator::Allocator;
    use rustc_hash::{FxHashMap, FxHashSet};
    use smallvec::SmallVec;

    fn make_resolver() -> BindingResolver<'static> {
        BindingResolver::new(FxHashMap::default(), false)
    }

    fn make_resolver_with(
        entries: &[(
            &'static str,
            crate::template::code_gen::binding::BindingType,
        )],
        is_inline: bool,
    ) -> BindingResolver<'static> {
        let mut map = FxHashMap::default();
        for &(name, bt) in entries {
            map.insert(name, bt);
        }
        BindingResolver::new(map, is_inline)
    }

    fn make_tag(start: u32, end: u32, name_end: u32) -> NodeTag {
        NodeTag {
            start,
            end,
            name_end,
        }
    }

    fn make_directive_prop(
        start: u32,
        name_end: u32,
        arg_start: Option<u32>,
        arg_end: Option<u32>,
        value_start: Option<u32>,
        value_end: Option<u32>,
    ) -> NodeProp {
        make_directive_prop_with_modifiers(
            start,
            name_end,
            arg_start,
            arg_end,
            value_start,
            value_end,
            &[],
        )
    }

    /// Like [`make_directive_prop`] but with modifier spans sliced from
    /// `ctx.source` at the given byte ranges (matching `.modName` in the
    /// authored source).
    #[allow(clippy::too_many_arguments)]
    fn make_directive_prop_with_modifiers(
        start: u32,
        name_end: u32,
        arg_start: Option<u32>,
        arg_end: Option<u32>,
        value_start: Option<u32>,
        value_end: Option<u32>,
        modifiers: &[(u32, u32)],
    ) -> NodeProp {
        NodeProp {
            start,
            name_end,
            is_directive: true,
            arg_start,
            arg_end,
            is_dynamic: None,
            value_start,
            value_end,
            modifiers: modifiers
                .iter()
                .map(|&(s, e)| crate::common::Span::new(s, e))
                .collect(),
        }
    }

    #[test]
    fn dynamic_class_creates_set_class_effect() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        let source = r#":class="cls""#;
        let element = ElementNode {
            tag_open: make_tag(0, 20, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop(
                0,
                6,
                Some(1),
                Some(6),
                Some(8),
                Some(11),
            )],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty().add(PropFlags::HasDynamicClass),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver();
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(state.own_effects[0].to_code(), "_setClass(n0, _ctx.cls)");
        assert!(out.vapor_imports().has(VaporHelper::SetClass));
    }

    /// `:id` with NO `="..."` at all is the Vue 3.4+ same-name shorthand —
    /// resolves to the identically-named binding.
    #[test]
    fn bind_shorthand_no_value_resolves_to_binding() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        let source = ":id";
        let element = ElementNode {
            tag_open: make_tag(0, 20, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop(0, 3, Some(1), Some(3), None, None)],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver();
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(
            state.own_effects[0].to_code(),
            "_setProp(n0, \"id\", _ctx.id)"
        );
    }

    /// `:id=""` is a DIFFERENT authored shape from `:id` — an explicit but
    /// blank value, not the same-name shorthand. It must NOT silently
    /// resolve to the `id` binding (the pre-fix conflation): official
    /// rejects this outright (`X_V_BIND_NO_EXPRESSION`), and its own
    /// recovery falls back to a literal empty-string expression.
    #[test]
    fn bind_explicit_blank_value_does_not_alias_to_shorthand() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        let source = ":id=\"\"";
        let element = ElementNode {
            tag_open: make_tag(0, 20, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop(
                0,
                3,
                Some(1),
                Some(3),
                Some(5),
                Some(5),
            )],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        // `id` IS a real binding here — if the fix regressed to the old
        // `value.is_empty()` conflation, this would silently resolve to
        // `_ctx.id`/`$setup.id` instead of the literal empty string.
        let resolver = make_resolver_with(
            &[(
                "id",
                crate::template::code_gen::binding::BindingType::SetupRef,
            )],
            false,
        );
        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        let code = state
            .own_effects
            .first()
            .map(|e| e.to_code())
            .or_else(|| {
                state
                    .child_statements
                    .first()
                    .map(|(s, _)| (*s).to_string())
            })
            .expect("blank value still emits a setter, just with a literal empty expression");
        assert!(
            !code.contains("id.value") && !code.contains("_ctx.id") && !code.contains("$setup.id"),
            "an explicit blank value must NOT alias to the same-name shorthand binding, got: {code}"
        );
        assert_eq!(code, "_setProp(n0, \"id\", \"\")");
    }

    #[test]
    fn dynamic_style_creates_set_style_effect() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        let source = r#":style="sty""#;
        let element = ElementNode {
            tag_open: make_tag(0, 20, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop(
                0,
                6,
                Some(1),
                Some(6),
                Some(8),
                Some(11),
            )],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty().add(PropFlags::HasDynamicStyle),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver();
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(state.own_effects[0].to_code(), "_setStyle(n0, _ctx.sty)");
        assert!(out.vapor_imports().has(VaporHelper::SetStyle));
    }

    #[test]
    fn dynamic_prop_creates_set_prop_effect() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        let source = r#":title="val""#;
        let element = ElementNode {
            tag_open: make_tag(0, 20, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop(
                0,
                6,
                Some(1),
                Some(6),
                Some(8),
                Some(11),
            )],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty().add(PropFlags::HasDynamicKey),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver();
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(
            state.own_effects[0].to_code(),
            r#"_setProp(n0, "title", _ctx.val)"#
        );
        assert!(out.vapor_imports().has(VaporHelper::SetProp));
    }

    #[test]
    fn static_only_element_no_effects() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        let element = ElementNode {
            tag_open: make_tag(0, 5, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: Vec::new(),
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver();
        let mut ctx = VaporPropsContext {
            source: "<div>",
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert!(state.own_effects.is_empty());
        assert!(state.node_ref.is_none());
    }

    #[test]
    fn static_class_only_no_effects() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        let element = ElementNode {
            tag_open: make_tag(0, 17, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![NodeProp {
                start: 5,
                name_end: 10,
                is_directive: false,
                arg_start: None,
                arg_end: None,
                is_dynamic: None,
                value_start: Some(12),
                value_end: Some(15),
                modifiers: SmallVec::new(),
            }],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty().add(PropFlags::HasStaticClass),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver();
        let mut ctx = VaporPropsContext {
            source: r#"<div class="foo">"#,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert!(state.own_effects.is_empty());
    }

    #[test]
    fn multiple_dynamic_props() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        let source = r#":class="cls" :title="ttl""#;
        let element = ElementNode {
            tag_open: make_tag(0, 30, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![
                make_directive_prop(0, 6, Some(1), Some(6), Some(8), Some(11)),
                make_directive_prop(13, 19, Some(14), Some(19), Some(21), Some(24)),
            ],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty()
                .add(PropFlags::HasDynamicClass)
                .add(PropFlags::HasDynamicKey),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver();
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 2);
        assert_eq!(state.own_effects[0].to_code(), "_setClass(n0, _ctx.cls)");
        assert_eq!(
            state.own_effects[1].to_code(),
            r#"_setProp(n0, "title", _ctx.ttl)"#
        );
        // Same node ref for both
        assert_eq!(state.node_ref, Some(0));
    }

    // ==================== Modifier helper-routing tests ====================

    #[test]
    fn event_runtime_modifier_wires_with_modifiers_import() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        //                0123456789012345678901
        let source = r#"@click.stop="handler""#;
        let element = ElementNode {
            tag_open: make_tag(0, 30, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop_with_modifiers(
                0,
                6,
                None,
                None,
                Some(13),
                Some(20),
                &[(7, 11)],
            )],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver();
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.child_statements.len(), 1);
        assert_eq!(
            state.child_statements[0].0,
            r#"_on(n0, "click", _withModifiers(e => _ctx.handler(e), ["stop"]))"#
        );
        assert!(out.vapor_imports().has(VaporHelper::WithModifiers));
        assert!(!out.vapor_imports().has(VaporHelper::WithKeys));
    }

    #[test]
    fn event_key_modifier_wires_with_keys_import() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        //                012345678901234567890123
        let source = r#"@keyup.enter="onEnter""#;
        let element = ElementNode {
            tag_open: make_tag(0, 30, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop_with_modifiers(
                0,
                6,
                None,
                None,
                Some(14),
                Some(21),
                &[(7, 12)],
            )],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver();
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.child_statements.len(), 1);
        assert_eq!(
            state.child_statements[0].0,
            r#"_on(n0, "keyup", _withKeys(e => _ctx.onEnter(e), ["enter"]))"#
        );
        assert!(out.vapor_imports().has(VaporHelper::WithKeys));
        assert!(!out.vapor_imports().has(VaporHelper::WithModifiers));
    }

    #[test]
    fn prop_modifier_forces_set_dom_prop_even_when_hyphenated() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        //                0         1         2
        //                0123456789012345678901234
        let source = r#":text-content.prop="text""#;
        let element = ElementNode {
            tag_open: make_tag(0, 30, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop_with_modifiers(
                0,
                13,
                Some(1),
                Some(13),
                Some(20),
                Some(24),
                &[(14, 18)],
            )],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver();
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(
            state.own_effects[0].to_code(),
            r#"_setDOMProp(n0, "text-content", _ctx.text)"#
        );
        assert!(out.vapor_imports().has(VaporHelper::SetDomProp));
        assert!(!out.vapor_imports().has(VaporHelper::SetAttr));
    }

    #[test]
    fn attr_modifier_forces_set_attr_even_when_not_hyphenated() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        //                0         1
        //                012345678901234567
        let source = r#":title.attr="ttl""#;
        let element = ElementNode {
            tag_open: make_tag(0, 20, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop_with_modifiers(
                0,
                6,
                Some(1),
                Some(6),
                Some(13),
                Some(16),
                &[(7, 11)],
            )],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver();
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(
            state.own_effects[0].to_code(),
            r#"_setAttr(n0, "title", _ctx.ttl)"#
        );
        assert!(out.vapor_imports().has(VaporHelper::SetAttr));
        assert!(!out.vapor_imports().has(VaporHelper::SetProp));
        assert!(!out.vapor_imports().has(VaporHelper::SetDomProp));
    }

    /// Negative control (explicit-request §5): a plain hyphenated attribute
    /// WITHOUT `.prop` must NOT flip to the new DOM-property helper — the
    /// implicit hyphen-based `_setAttr` routing is unaffected.
    #[test]
    fn hyphenated_attr_without_prop_modifier_stays_set_attr() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        let source = r#":data-x="dataX""#;
        let element = ElementNode {
            tag_open: make_tag(0, 20, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop(
                0,
                7,
                Some(1),
                Some(7),
                Some(9),
                Some(14),
            )],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver();
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(
            state.own_effects[0].to_code(),
            r#"_setAttr(n0, "data-x", _ctx.dataX)"#
        );
        assert!(out.vapor_imports().has(VaporHelper::SetAttr));
        assert!(!out.vapor_imports().has(VaporHelper::SetDomProp));
    }

    // ==================== Binding resolution tests ====================

    #[test]
    fn v_html_resolves_expression() {
        use crate::template::code_gen::binding::BindingType;

        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        //                0123456789012345
        let source = r#"v-html="rawHtml""#;
        let element = ElementNode {
            tag_open: make_tag(0, 20, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop(0, 6, None, None, Some(8), Some(15))],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();

        // SetupRef inline → should resolve to "rawHtml.value"
        let resolver = make_resolver_with(&[("rawHtml", BindingType::SetupRef)], true);
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(
            state.own_effects[0].to_code(),
            "_setHtml(n0, rawHtml.value)"
        );
    }

    #[test]
    fn v_html_unresolved_gets_ctx_prefix() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        let source = r#"v-html="rawHtml""#;
        let element = ElementNode {
            tag_open: make_tag(0, 20, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop(0, 6, None, None, Some(8), Some(15))],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();

        // Unresolved → should resolve to "_ctx.rawHtml"
        let resolver = make_resolver();
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(state.own_effects[0].to_code(), "_setHtml(n0, _ctx.rawHtml)");
    }

    #[test]
    fn v_bind_spread_resolves_expression() {
        use crate::template::code_gen::binding::BindingType;

        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        //                01234567890123
        let source = r#"v-bind="attrs""#;
        let element = ElementNode {
            tag_open: make_tag(0, 20, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop(0, 6, None, None, Some(8), Some(13))],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();

        // SetupRef inline → should resolve to "attrs.value"
        let resolver = make_resolver_with(&[("attrs", BindingType::SetupRef)], true);
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(
            state.own_effects[0].to_code(),
            "_setDynamicProps(n0, [attrs.value])"
        );
    }

    #[test]
    fn dynamic_class_with_setup_ref_inline() {
        use crate::template::code_gen::binding::BindingType;

        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        let source = r#":class="cls""#;
        let element = ElementNode {
            tag_open: make_tag(0, 20, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop(
                0,
                6,
                Some(1),
                Some(6),
                Some(8),
                Some(11),
            )],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty().add(PropFlags::HasDynamicClass),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver_with(&[("cls", BindingType::SetupRef)], true);
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(state.own_effects[0].to_code(), "_setClass(n0, cls.value)");
    }

    #[test]
    fn dynamic_style_with_props_inline() {
        use crate::template::code_gen::binding::BindingType;

        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        let source = r#":style="sty""#;
        let element = ElementNode {
            tag_open: make_tag(0, 20, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop(
                0,
                6,
                Some(1),
                Some(6),
                Some(8),
                Some(11),
            )],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty().add(PropFlags::HasDynamicStyle),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver_with(&[("sty", BindingType::Props)], true);
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(state.own_effects[0].to_code(), "_setStyle(n0, __props.sty)");
    }

    #[test]
    fn dynamic_prop_with_setup_const_standalone() {
        use crate::template::code_gen::binding::BindingType;

        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        let source = r#":title="val""#;
        let element = ElementNode {
            tag_open: make_tag(0, 20, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop(
                0,
                6,
                Some(1),
                Some(6),
                Some(8),
                Some(11),
            )],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty().add(PropFlags::HasDynamicKey),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver_with(&[("val", BindingType::SetupConst)], false);
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(
            state.own_effects[0].to_code(),
            r#"_setProp(n0, "title", $setup.val)"#
        );
    }

    #[test]
    fn dynamic_attr_with_setup_ref_inline() {
        use crate::template::code_gen::binding::BindingType;

        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();

        //                   0         1
        //                   0123456789012345678
        let source = r#":data-id="dataId""#;
        let element = ElementNode {
            tag_open: make_tag(0, 30, 4),
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: false,
            props: vec![make_directive_prop(
                0,
                8,
                Some(1),
                Some(8),
                Some(10),
                Some(16),
            )],
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty().add(PropFlags::HasDynamicKey),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        };

        let mut del_events: Vec<&str> = Vec::new();
        let mut del_set: FxHashSet<&str> = FxHashSet::default();
        let resolver = make_resolver_with(&[("dataId", BindingType::SetupRef)], true);
        let mut ctx = VaporPropsContext {
            source,
            resolver: &resolver,
            state: &mut state,
            counters: &mut counters,
            out: &mut out,
            delegated_events: &mut del_events,
            delegated_events_set: &mut del_set,
            force_js: false,
        };
        process_dynamic_props(&element, &mut ctx, None);

        assert_eq!(state.own_effects.len(), 1);
        assert_eq!(
            state.own_effects[0].to_code(),
            r#"_setAttr(n0, "data-id", dataId.value)"#
        );
    }
}
