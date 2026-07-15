//! Bind-target NAME collection from the typed runtime IR — the input sets the
//! instance-script item allowlist consults to decide which bare-local / function
//! declarations a `bind:` directive references.
//!
//! Each collector walks the typed `AttrIr` bind inventory + the parsed bind-expression
//! node (never a raw source scan) and returns the names that a top-level declaration must
//! match to be admitted by the strict script-item allowlist:
//! - [`collect_bind_this_targets`] — `bind:this={el}` clone-root locals (admit `let el;`);
//! - [`collect_bind_lvalue_roots`] — every non-`this` DOM bind-target lvalue ROOT (admit a
//!   plain-local `let v;` / `let v = <literal>;` bind target);
//! - [`collect_bind_function_pair_names`] — the bare-identifier names referenced by a DOM
//!   function-pair bind `bind:value={get, set}` (admit the named `function get(){…}` /
//!   `function set(next){…}` declarations).
//!
//! Plus [`collect_event_handler_fn_referents`] — the bare-identifier EVENT-handler
//! referents (`onclick={inc}` / `on:click={inc}`) that name a top-level `function`
//! declaration (admit `function inc(){…}` — the handler passes it by reference) —
//! and [`default_attr_has_matching_bind`] — the bind-relationship query that decides
//! whether a static `defaultValue` / `defaultChecked` attribute is co-located with its
//! matching `bind:value` / `bind:checked` (the form-default property-write acceptance).

use super::client_allowlist::SupportedHtmlElement;
use super::ir::{AttrIr, IrNode, NodeId, SvelteRuntimeIr};

/// Collect the local names used as a supported `bind:this` target, resolved from the
/// IR's bind attributes. A bare `let el;` instance-script declaration is admitted by
/// the script-item allowlist ONLY when its name is in this set (so an UNUSED bare
/// local fails closed). Driven from the typed `AttrIr` inventory + the analyzed
/// bind-expression source, never a raw scan.
pub(super) fn collect_bind_this_targets(ir: &SvelteRuntimeIr) -> Vec<String> {
    let mut targets = Vec::new();
    for node in &ir.nodes {
        // An element `bind:this={el}` OR a component `bind:this={ref}` (the
        // component-reference bind) contributes its target identifier.
        let attrs = match node {
            IrNode::Element(el) => &el.attrs,
            IrNode::Component(c) => &c.attrs,
            IrNode::Special(s) => &s.attrs,
            _ => continue,
        };
        for attr in attrs {
            let AttrIr::Bind {
                target,
                expr: Some(expr_id),
            } = attr
            else {
                continue;
            };
            if target != "this" {
                continue;
            }
            let analyzed = ir.analysis.expressions.get(*expr_id);
            // Only a bare-IDENTIFIER `bind:this={el}` / `bind:this={(el)}` target contributes
            // a name (a member `bind:this={refs[0]}` is refused at the bind classifier). The
            // root comes from the typed fact (`kind` + `root_ident`), NOT `source.trim()` /
            // `is_plain_identifier` — so a parenthesized identifier resolves its root `el`.
            if matches!(
                analyzed.bind_target.kind,
                Some(super::expr::BindTargetKind::Identifier)
            ) {
                if let Some(root) = &analyzed.bind_target.root_ident {
                    targets.push(root.clone());
                }
            }
        }
    }
    targets
}

/// Collect the ROOT identifier names of every DOM bind TARGET (a `bind:value={v}`
/// ident → `v`; a `bind:value={o.x}` member → `o`), excluding `bind:this` (handled by
/// [`collect_bind_this_targets`]). A plain-local `let name = <literal>;` / `let name;`
/// instance-script declaration is admitted by the script-item allowlist as a DOM
/// bind-target lvalue root ONLY when its name is in this set (so an UNUSED plain local
/// fails closed).
///
/// This OVER-collects deliberately: it records every non-`this` bind-target root
/// regardless of binding kind (the kind is not finalized at this point). A `$state`
/// root routes to the `$state` shape (not the plain-local arm), so a signal root in
/// this set is harmless — the plain-local arm only fires for a genuine plain `let`. A
/// function-pair target contributes no root (the user owns the get/set; its referenced
/// signals are handled by the general state analysis). Driven from the typed `AttrIr`
/// inventory + the parsed bind-expression node, never a raw text scan.
pub(super) fn collect_bind_lvalue_roots(ir: &SvelteRuntimeIr) -> Vec<String> {
    use super::expr::BindTargetKind;
    let mut roots = Vec::new();
    for node in &ir.nodes {
        let IrNode::Element(el) = node else {
            continue;
        };
        for attr in &el.attrs {
            let AttrIr::Bind {
                target,
                expr: Some(expr_id),
            } = attr
            else {
                continue;
            };
            if target == "this" {
                continue;
            }
            let analyzed = ir.analysis.expressions.get(*expr_id);
            // Only an IDENTIFIER or MEMBER target has a single lvalue root; a
            // function-pair / non-lvalue contributes none. Read the shared bind-target
            // fact (computed once at analysis time) — no per-call reparse.
            if let Some(BindTargetKind::Identifier | BindTargetKind::Member) =
                analyzed.bind_target.kind
            {
                if let Some(root) = &analyzed.bind_target.root_ident {
                    roots.push(root.clone());
                }
            }
        }
    }
    roots
}

/// Collect the bare-identifier names referenced by every FUNCTION-PAIR bind — a DOM
/// value/property bind (`bind:value={get, set}`) OR an element `bind:this={get, set}`
/// (the host-instance getter/setter pair) — the get/set element sources that are a PLAIN
/// IDENTIFIER (a named function reference). A top-level `function name(...) {}` declaration is
/// admitted by the script-item allowlist as a lowered function ONLY when its name is in
/// this set (so a function nothing binds still fails closed).
///
/// Each function-pair target's two element sources are read from the shared
/// [`BindTargetFact::function_pair`](super::expr::BindTargetFact) (the default-closed
/// plain-Svelte-JS slices, computed once at analysis time); an element that is a bare plain identifier
/// (e.g. `get` / `set`) contributes its name, while an INLINE arrow / call / any
/// non-identifier element contributes nothing (an inline pair declares no top-level
/// function, so it needs no admission). A TS-bearing pair (refused by the lane) yields
/// no names — consistent with the bind itself failing closed. Driven from the typed
/// `AttrIr` inventory + the parsed bind-expression node, never a raw text scan.
pub(super) fn collect_bind_function_pair_names(ir: &SvelteRuntimeIr) -> Vec<String> {
    use super::expr::BindTargetKind;
    let mut names = Vec::new();
    for node in &ir.nodes {
        let IrNode::Element(el) = node else {
            continue;
        };
        for attr in &el.attrs {
            let AttrIr::Bind {
                target: _,
                expr: Some(expr_id),
            } = attr
            else {
                continue;
            };
            // A function-pair target is admitted for EVERY bind name — including
            // `bind:this={get, set}` (the element host-instance getter/setter pair), whose
            // named `get`/`set` function declarations this set admits. A NON-function-pair
            // target (the identifier `bind:this={el}`, a member, …) is skipped by the kind
            // gate below.
            let analyzed = ir.analysis.expressions.get(*expr_id);
            if !matches!(
                analyzed.bind_target.kind,
                Some(BindTargetKind::FunctionPair)
            ) {
                continue;
            }
            // The two element sources of the function pair (read from the shared fact); a
            // plain-identifier element is a named function reference whose declaration this
            // admits.
            if let Some((get_src, set_src)) = &analyzed.bind_target.function_pair {
                for src in [get_src, set_src] {
                    let trimmed = src.trim();
                    if is_plain_identifier(trimmed) {
                        names.push(trimmed.to_string());
                    }
                }
            }
        }
    }
    names
}

/// Collect the BARE-IDENTIFIER event-handler referents that name a top-level
/// instance-script `function` declaration (`onclick={inc}` / `on:click={inc}` with
/// `function inc(){…}`). A top-level function declaration is admitted by the
/// script-item allowlist when its name is in this set (the handler passes the
/// reference through — `$.delegated('click', button, inc)`). An inline arrow /
/// call / member handler contributes nothing, and a bare identifier that is NOT a
/// top-level function name contributes nothing (its handler classification fails
/// closed independently). Driven from the typed `AttrIr` event inventory + the
/// analyzed handler source, never a raw scan.
pub(super) fn collect_event_handler_fn_referents<'a>(
    ir: &'a SvelteRuntimeIr,
    fn_decl_names: &rustc_hash::FxHashSet<String>,
) -> Vec<&'a str> {
    let mut names = Vec::new();
    for node in &ir.nodes {
        let IrNode::Element(el) = node else {
            continue;
        };
        for attr in &el.attrs {
            let AttrIr::Event { handler, .. } = attr else {
                continue;
            };
            let analyzed = ir.analysis.expressions.get(*handler);
            let trimmed = analyzed.source.trim();
            if is_plain_identifier(trimmed) && fn_decl_names.contains(trimmed) {
                names.push(trimmed);
            }
        }
    }
    names
}

/// Whether a static `defaultValue` / `defaultChecked` attribute on the element at
/// `node_id` is CO-LOCATED with its MATCHING two-way bind — the form-default surface
/// the official compiler emits as a property write before the bind. `defaultValue`
/// pairs with `bind:value` (on an `<input>` OR `<textarea>` bind host); `defaultChecked`
/// pairs with `bind:checked` (on an `<input>`). A non-default attribute name, or a
/// default attribute WITHOUT its matching bind (standalone, or mismatched — e.g.
/// `defaultChecked` alongside `bind:value`), returns `false`, so it stays the
/// form-default deferral and fails closed at the static-attr allowlist. The decision is
/// structural over the typed `AttrIr` bind directives, never a source scan.
pub(super) fn default_attr_has_matching_bind(
    name: &str,
    element: SupportedHtmlElement,
    ir: &SvelteRuntimeIr,
    node_id: NodeId,
) -> bool {
    let bind_target = match name {
        // `defaultValue` is valid on a `bind:value` host (`<input>` or `<textarea>`).
        "defaultValue"
            if matches!(
                element,
                SupportedHtmlElement::Input | SupportedHtmlElement::Textarea
            ) =>
        {
            "value"
        }
        // `defaultChecked` is valid on a `bind:checked` `<input>`.
        "defaultChecked" if element == SupportedHtmlElement::Input => "checked",
        _ => return false,
    };
    matches!(ir.node(node_id), IrNode::Element(el)
        if el.attrs.iter().any(|a| matches!(a, AttrIr::Bind { target, .. } if target == bind_target)))
}

/// Whether a string is a single plain JS identifier (`/^[A-Za-z_$][A-Za-z0-9_$]*$/`),
/// the only `bind:this` target shape that names a supported bare-local declaration.
fn is_plain_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}
