//! The `<svelte:*>` HOST / RENDERABLE-SPECIAL classifiers, extracted from `client_surface.rs`
//! to keep the default-deny classifier under the file-size guard.
//!
//! These are the per-special-kind classification entry points the node walk
//! ([`classify_node`](super::client_surface)) dispatches to: [`classify_special_host`] (the
//! global `<svelte:window|document|body>` event/bind hosts), [`classify_svelte_element`] (the
//! `<svelte:element>` dynamic element), and [`classify_svelte_boundary`] (`<svelte:boundary>`).
//! Each records the accepted event/bind SHAPE facts into the shared [`SurfaceFacts`] or fails
//! closed (a wrong-host / invalid bind name, an unsupported attribute / directive).

use std::cell::RefCell;

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_shapes;
use super::client_surface::{refuse_invalid_event_modifiers, SurfaceFacts};
use super::client_surface_refuse::special_label;
use super::ir::{AttrIr, EventOrigin, NodeId, SpecialKind, SvelteRuntimeIr};

/// Classify a GLOBAL-host special (`<svelte:window|document|body>`): each attribute is an
/// EVENT (the global-host event surface — `$.event('<type>', <host>, handler)`, recorded
/// host-keyed so the op projection finds the shape without a DOM node id) OR a `bind:`
/// resolved through the HOST-SCOPED bind contract (`bind:innerWidth` is window-only,
/// `bind:activeElement` document-only, `bind:clientWidth` body-only, `bind:this` /
/// `bind:focused` any host). Every OTHER attribute form — a static / dynamic / mixed
/// attribute, a spread, a `class:` / `style:` / `use:` / `transition:` / `let:` directive —
/// is NOT a supported host-special surface and fails closed.
///
/// A WRONG-HOST or UNKNOWN bind name (`<svelte:body bind:scrollX>`, `<svelte:document
/// bind:innerWidth>`, `<svelte:window bind:fooBar>`) fails closed at the bind classifier (the
/// bind never emits — the official `bind_invalid_target` / `bind_invalid_name` reject; the
/// EXACT diagnostic code/order is the D-29 deferral, Verter routes to the generic refusal).
pub(super) fn classify_special_host(
    ir: &SvelteRuntimeIr,
    node_id: NodeId,
    special: &super::ir::SpecialElementIr,
    declared_root_names: &rustc_hash::FxHashSet<String>,
    facts: &RefCell<SurfaceFacts>,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    let host_token = match special.kind {
        SpecialKind::Window => "svelte:window",
        SpecialKind::Document => "svelte:document",
        SpecialKind::Body => "svelte:body",
        // Unreachable: the gate routes only Window/Document/Body here.
        _ => {
            return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: special_label(special.kind),
                span: special.span,
            })
        }
    };
    // A GLOBAL host renders NO DOM, so it can have NO child content — the official
    // `svelte_meta_invalid_content` error (`<svelte:window> cannot have children`). Fail
    // closed for ANY child (text / interpolation / element / block) rather than SILENTLY
    // dropping it through the no-DOM path.
    if !special.children.is_empty() {
        return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
            construct: special_label(special.kind),
            span: special.span,
        });
    }
    for attr in &special.attrs {
        match attr {
            AttrIr::Event {
                event_type,
                handler,
                delegated,
                modifiers,
                ..
            } => {
                // A global host event is a DIRECT `$.event(...)` registration (the lowering
                // records `delegated: false` for the host targets) — classify the handler
                // against the direct-event surface (any non-async inline arrow / fn).
                refuse_invalid_event_modifiers(modifiers, event_type, special.span)?;
                let analyzed = ir.analysis.expressions.get(*handler);
                let shape = client_shapes::classify_event_handler_shape(
                    analyzed.source,
                    event_type,
                    special.span,
                    analyzed.scope,
                    &ir.analysis.bindings,
                    &ir.analysis.scopes,
                    !*delegated,
                    // Special-host bare-identifier handlers stay fail-closed: the
                    // top-level-function referent set is scoped to REGULAR
                    // elements, so the special hosts pass the empty set.
                    &rustc_hash::FxHashSet::default(),
                )?;
                facts.borrow_mut().event_shapes.push((
                    node_id,
                    event_type.clone(),
                    *handler,
                    shape,
                ));
            }
            AttrIr::Bind { target, expr } => {
                let analyzed = expr.map(|e| ir.analysis.expressions.get(e));
                let scope = analyzed
                    .map(|a| a.scope)
                    .unwrap_or_else(|| ir.root_scope().scope);
                // The host token (`svelte:window` / …) is the bind classifier's host key —
                // the HOST-SCOPED `resolve_runtime_bind(name, host_token)` decides validity +
                // routing, so a wrong-host bind fails closed.
                let shape = client_shapes::classify_bind_shape(
                    target,
                    host_token,
                    &special.attrs,
                    analyzed,
                    scope,
                    &ir.analysis.bindings,
                    &ir.analysis.scopes,
                    declared_root_names,
                    special.span,
                )?;
                facts
                    .borrow_mut()
                    .bind_shapes
                    .push((node_id, target.clone(), shape));
            }
            // Every other attribute/directive on a global host is not a supported surface.
            other => {
                let name = match other {
                    AttrIr::Static { name, .. }
                    | AttrIr::Dynamic { name, .. }
                    | AttrIr::Mixed { name, .. } => name.clone(),
                    AttrIr::Class { name, .. } => format!("class:{name}"),
                    AttrIr::Style { property, .. } => format!("style:{property}"),
                    AttrIr::Spread { .. } => "spread".to_string(),
                    AttrIr::Use { .. } => "use:".to_string(),
                    AttrIr::Transition { name, .. } => name.clone(),
                    AttrIr::Animate { name, .. } => format!("animate:{name}"),
                    AttrIr::Attach { .. } => "{@attach}".to_string(),
                    AttrIr::Let { name, .. } => format!("let:{name}"),
                    AttrIr::Event { .. } | AttrIr::Bind { .. } => unreachable!(),
                };
                return Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                    name,
                    span: special.span,
                });
            }
        }
    }
    Ok(())
}

/// Classify a `<svelte:element this={…}>` dynamic element: validate the `this` selector is
/// present, classify each `bind:` against the HOST-SCOPED `svelte:element` generic-element
/// host (an invalid/wrong-host bind name fails closed — `bind:value` / `bind:devicePixelRatio`
/// are §1.8 negatives), and validate each event handler against the inline-handler surface.
/// Plain attributes (static / dynamic / mixed / `class:` / `style:` / spread) are ACCEPTED —
/// they fold into the runtime `$.attribute_effect` (NO static-attr allowlist; the tag is
/// dynamic). A `slot` attribute reaches the fold ONLY on a DIRECT slot-declaring component
/// child (the unified slot choke-point at `classify_node` entry accepts exactly that
/// filler placement and official folds it — `$.attribute_effect($$element, () => ({ slot:
/// 'x' }))`); a top-level / non-direct / dynamic `slot` was refused upstream. A `use:` /
/// `transition:` / `let:` directive is a 5f-c surface and fails closed. The children are
/// the element's OWN body region (classified independently by the scope loop), so they are
/// not recursed here.
pub(super) fn classify_svelte_element(
    ir: &SvelteRuntimeIr,
    node_id: NodeId,
    special: &super::ir::SpecialElementIr,
    declared_root_names: &rustc_hash::FxHashSet<String>,
    facts: &RefCell<SurfaceFacts>,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    // A `<svelte:element>` REQUIRES a `this` selector (a parse error otherwise) — fail
    // closed defensively if the lowering recorded neither a dynamic nor a static tag.
    if special.this_expr.is_none() && special.static_tag.is_none() {
        return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
            construct: "svelte:element without this",
            span: special.span,
        });
    }
    for attr in &special.attrs {
        match attr {
            AttrIr::Bind { target, expr } => {
                let analyzed = expr.map(|e| ir.analysis.expressions.get(e));
                let scope = analyzed
                    .map(|a| a.scope)
                    .unwrap_or_else(|| ir.root_scope().scope);
                let shape = client_shapes::classify_bind_shape(
                    target,
                    "svelte:element",
                    &special.attrs,
                    analyzed,
                    scope,
                    &ir.analysis.bindings,
                    &ir.analysis.scopes,
                    declared_root_names,
                    special.span,
                )?;
                facts
                    .borrow_mut()
                    .bind_shapes
                    .push((node_id, target.clone(), shape));
            }
            AttrIr::Event {
                event_type,
                handler,
                modifiers,
                ..
            } => {
                // A dynamic-element event is a `$.attribute_effect` fold entry (NOT a delegated
                // listener) — validate the handler is an accepted inline arrow / function.
                refuse_invalid_event_modifiers(modifiers, event_type, special.span)?;
                let analyzed = ir.analysis.expressions.get(*handler);
                client_shapes::classify_event_handler_shape(
                    analyzed.source,
                    event_type,
                    special.span,
                    analyzed.scope,
                    &ir.analysis.bindings,
                    &ir.analysis.scopes,
                    true,
                    // Special-host bare-identifier handlers stay fail-closed: the
                    // top-level-function referent set is scoped to REGULAR
                    // elements, so the special hosts pass the empty set.
                    &rustc_hash::FxHashSet::default(),
                )?;
            }
            // Plain attributes fold into `$.attribute_effect` (a dynamic tag accepts any
            // attribute name — no static-attr allowlist).
            AttrIr::Static { .. }
            | AttrIr::Dynamic { .. }
            | AttrIr::Mixed { .. }
            | AttrIr::Class { .. }
            | AttrIr::Style { .. }
            | AttrIr::Spread { .. } => {}
            // A lifecycle directive (`use:` / `transition:` / `animate:` / `{@attach}`)
            // on a dynamic element is the DEFERRED host-lifecycle surface (ledger D-39,
            // official ACCEPTS it against `$$element`); `let:` stays the slot-prop
            // refusal. All fail closed.
            AttrIr::Use { .. }
            | AttrIr::Transition { .. }
            | AttrIr::Animate { .. }
            | AttrIr::Attach { .. }
            | AttrIr::Let { .. } => {
                return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "directive",
                    span: special.span,
                });
            }
        }
    }
    Ok(())
}

/// Classify a `<svelte:head>`: it renders no DOM node itself (it emits `$.head('<hash>', …)`),
/// its `<title>` drives `$.document.title`, and its non-title children (`<meta>` / `<link>` / a
/// text run / …) are its OWN body region (classified independently by the scope loop, so they
/// are not recursed here). Official REJECTS any attribute or directive on a `<svelte:head>`
/// (its `SvelteHead` analyze visitor throws `svelte_head_illegal_attribute` on every attribute),
/// so Verter's fail-close on a head-borne attribute is PARITY with official.
pub(super) fn classify_svelte_head(
    special: &super::ir::SpecialElementIr,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    // A `<title>` may contain ONLY text + interpolations (the official `TitleElement`
    // `title_invalid_content` error). A nested element / comment / block title child is
    // recorded at lowering as `invalid_content`; fail the head closed rather than SILENTLY
    // dropping the unsupported child.
    if let Some(title) = &special.head_title {
        if let Some(span) = title.invalid_content {
            return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "svelte:head <title> non-text content",
                span,
            });
        }
    }
    // Official's `SvelteHead` analyze visitor rejects EVERY attribute / directive with
    // `svelte_head_illegal_attribute` ("`<svelte:head>` cannot have attributes nor directives").
    // Verter fails closed on any attribute on the attribute-less `<svelte:head>` — PARITY with
    // official's reject, not a deviation.
    if special.attrs.is_empty() {
        Ok(())
    } else {
        Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
            construct: "svelte:head attribute",
            span: special.span,
        })
    }
}

/// Classify a `<svelte:boundary>`: the MODERN `onerror={…}` handler (validated against the
/// inline-handler surface — an ASYNC `onerror` fails closed via `ExperimentalAsync`) and the
/// MODERN `failed={…}` / `pending={…}` ATTRIBUTE-expression forms are accepted; every other
/// attribute fails closed. The body + the `failed` / `pending` `{#snippet}` CHILD defs are the
/// boundary's body region + snippet defs, classified independently by the scope loop (so they are
/// not recursed here).
///
/// Official gates boundary attributes on `type === "Attribute" && name ∈ {onerror, failed,
/// pending}` (the `SvelteBoundary` visitor's accept-list), with the value required to be exactly
/// one `ExpressionTag` — anything else is `svelte_boundary_invalid_attribute` /
/// `svelte_boundary_invalid_attribute_value`. A LEGACY `on:error` is an `OnDirective`, never an
/// `Attribute`, so official REJECTS every `on:` form (including a bare `on:error`). Because a bare
/// `on:error` collapses to the SAME `AttrIr::Event` shape as the modern `onerror` (no modifiers /
/// capture / passive), the lowering-recorded [`EventOrigin`] is the sole faithful discriminator —
/// NOT a modifier-presence heuristic. `failed` / `pending` (and the shorthand `{failed}` /
/// `{pending}`) lower to an `AttrIr::Dynamic` (exactly one expression) and are accepted; a
/// valueless / string-literal / mixed `failed` lowers to `AttrIr::Static` / `AttrIr::Mixed`, NOT
/// `Dynamic`, so it falls through to the reject arm (official's invalid-attribute-value error).
pub(super) fn classify_svelte_boundary(
    ir: &SvelteRuntimeIr,
    special: &super::ir::SpecialElementIr,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    for attr in &special.attrs {
        match attr {
            // The MODERN `onerror={…}` attribute — a modern-origin `error` event with NO capture
            // suffix / modifiers / passive. `onerrorcapture` (capture: true) is NOT the exact
            // `onerror` name in official's `Pw`, so it falls through to the reject arm below,
            // matching official.
            AttrIr::Event {
                event_type,
                handler,
                origin: EventOrigin::ModernAttribute,
                capture: false,
                modifiers,
                passive: None,
                ..
            } if event_type == "error" && modifiers.is_empty() => {
                let analyzed = ir.analysis.expressions.get(*handler);
                client_shapes::classify_event_handler_shape(
                    analyzed.source,
                    event_type,
                    special.span,
                    analyzed.scope,
                    &ir.analysis.bindings,
                    &ir.analysis.scopes,
                    true,
                    // Special-host bare-identifier handlers stay fail-closed: the
                    // top-level-function referent set is scoped to REGULAR
                    // elements, so the special hosts pass the empty set.
                    &rustc_hash::FxHashSet::default(),
                )?;
            }
            // The MODERN `failed={…}` / `pending={…}` ATTRIBUTE-expression form (and the shorthand
            // `{failed}` / `{pending}`) — a plain `AttrIr::Dynamic` carrying exactly one
            // expression. Official accepts these alongside `onerror`; Verter routes them into the
            // boundary props as the getter accessor `get name() { return <expr>; }` (the emitter's
            // `has_state ? get : init` decision). Nothing to validate at classify time — any
            // single-expression value is legal per official (the value shape was already narrowed
            // to `Dynamic` by lowering; a valueless / string / mixed value is a different
            // `AttrIr` variant that falls through to the reject arm below).
            AttrIr::Dynamic { name, .. } if name == "failed" || name == "pending" => {}
            // A LEGACY `on:` directive on a boundary (bare `on:error`, `on:error|preventDefault`,
            // `on:click`, …) is an `OnDirective` official never treats as a valid boundary
            // attribute — fail closed on the LEGACY origin. This is the faithful reject the
            // pre-fix modifier heuristic missed for the BARE `on:error` form.
            AttrIr::Event {
                origin: EventOrigin::LegacyDirective,
                ..
            } => {
                return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "svelte:boundary legacy on: directive",
                    span: special.span,
                });
            }
            // Every other attribute / directive — a modern non-`onerror` event (incl.
            // `onerrorcapture`), a `failed` / `pending` / other plain attribute, a spread, a
            // `bind:` / `class:` / … directive — is not a supported boundary surface.
            _ => {
                return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "svelte:boundary attribute",
                    span: special.span,
                });
            }
        }
    }
    Ok(())
}
