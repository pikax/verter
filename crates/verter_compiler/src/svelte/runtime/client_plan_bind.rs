//! The `bind:` / event OP-PROJECTION half of the Svelte client plan builder.
//!
//! Extracted from `client_plan.rs` (the file-size guard boundary): these are the
//! [`SupportedClientIr`] methods that project a `bind:` directive
//! ([`project_bind_op`](SupportedClientIr::project_bind_op)) or a delegated event
//! ([`project_event_op`](SupportedClientIr::project_event_op)) into its narrow
//! [`ClientRuntimeOp`], plus the shared getter/setter derivation
//! ([`bind_getter_setter`](SupportedClientIr::bind_getter_setter)) that shapes a
//! bind's get/set bodies from the target's STRUCTURAL lvalue kind (signal / plain /
//! member / function-pair) through the FALLIBLE source-preserving rewriter — never a
//! raw-text assignment. They consume the classifier's accepted shape facts
//! (`ClientBindShape` / `ClientEventHandlerShape`); a node with no recorded shape
//! fails closed defensively.

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_codegen_helpers::is_signal_kind;
use super::client_plan::SupportedClientIr;
use super::client_plan_types::{
    ClientNodeId, ClientRuntimeOp, EventEmit, EventEmitTarget, EventMode, EventWrapper,
};
use super::client_shapes::ClientEventHandlerShape;
use super::expr::ScopeId;
use super::ir::{
    AttrIr, BindOp, EventOp, EventTarget, ExprId, IrNode, MixedAttrPart, NodeId, SpecialKind,
};
use verter_span::Span;

impl<'a> SupportedClientIr<'a> {
    /// Project a `bind:` op into the narrow [`ClientRuntimeOp::Bind`], carrying the
    /// classifier's accepted [`ClientBindShape`](super::client_shapes::ClientBindShape)
    /// fact.
    pub(super) fn project_bind_op(
        &self,
        target: NodeId,
        bind: &BindOp,
        _scope: ScopeId,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        // The accepted bind SHAPE the classifier recorded for this target node is the
        // SOLE acceptance authority — `bind:this` plus the DATA-DRIVEN DOM value/
        // property family (`value`/`checked`/`group`/media/dimension/contenteditable/
        // property). A bind op with NO recorded shape was NOT accepted (a
        // component/special-host bind, not yet supported and owned by 5f, or an
        // unsupported name) — fail closed defensively (never emit an unclassified bind).
        let shape = self
            .bind_shapes
            .iter()
            .find(|(n, name, _)| *n == target && name == &bind.target)
            .map(|(_, _, s)| s.clone())
            .ok_or_else(|| UnsupportedSvelteRuntimeSurface::Binding {
                target: bind.target.clone(),
                span: Span::new(0, 0),
            })?;
        // The `should_proxy` flag is HOST-driven: a bind on a SPECIAL host
        // (`<svelte:window|document|body|element>`) emits the proxied setter
        // `$.set(local, $$value, true)` (the window/document/body/dynamic-element setter
        // baseline), while an ordinary DOM-element bind stays `$.set(local, $$value)`.
        let should_proxy = bind_target_is_special_host(self.ir.node(target));
        let (getter, setter) = self.bind_getter_setter(bind.expr, &bind.target, should_proxy)?;
        Ok(ClientRuntimeOp::Bind {
            target: ClientNodeId(target.0),
            shape,
            getter,
            setter,
        })
    }

    /// Project an event op into the narrow [`ClientRuntimeOp::Event`], building the
    /// reusable [`EventEmit`] substrate (mode / target host / capture / passive /
    /// modifier-wrapper stack / rewritten handler) and carrying the classifier's
    /// accepted [`ClientEventHandlerShape`](super::client_shapes::ClientEventHandlerShape)
    /// fact.
    ///
    /// The regular-element surface feeds regular DOM-node hosts (`EventTarget::Node`); the
    /// GLOBAL-host specials (`<svelte:window|document|body>`) feed the `Window` / `Document`
    /// / `Body` variants (a direct `$.event(...)` against `$.window` / `$.document` /
    /// `$.document.body`) — the SAME emitter serves both by feeding the global-host target
    /// kind. A global host event has no DOM node id, so its accepted shape is resolved
    /// host-keyed (by event type + handler expr).
    pub(super) fn project_event_op(
        &self,
        target: EventTarget,
        event: &EventOp,
        _scope: ScopeId,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        // The accepted handler SHAPE the classifier recorded for THIS event. A regular DOM
        // node resolves by the full (node, event type, handler expr) key so an element with
        // multiple events resolves to its OWN fact, never a sibling event's. A GLOBAL host
        // event (`<svelte:window|document|body>`) carries the global as its
        // [`EventTarget`] — it has NO DOM node id — so its accepted shape is recorded
        // host-keyed (the handler expr id is globally unique) and looked up by (event type,
        // handler expr). An event op with NO recorded shape is a classifier/plan divergence
        // — fail closed defensively (never emit an unclassified, possibly-non-function
        // handler).
        let (emit_target, shape) = match target {
            EventTarget::Node(node_id) => {
                let shape = find_event_shape(
                    &self.event_shapes,
                    node_id,
                    &event.event_type,
                    event.handler,
                )
                .ok_or_else(|| {
                    UnsupportedSvelteRuntimeSurface::NonDelegatedEvent {
                        event_type: event.event_type.clone(),
                        span: Span::new(0, 0),
                    }
                })?;
                (EventEmitTarget::Node(ClientNodeId(node_id.0)), shape)
            }
            EventTarget::Window | EventTarget::Document | EventTarget::Body => {
                let shape =
                    find_host_event_shape(&self.event_shapes, &event.event_type, event.handler)
                        .ok_or_else(|| UnsupportedSvelteRuntimeSurface::NonDelegatedEvent {
                            event_type: event.event_type.clone(),
                            span: Span::new(0, 0),
                        })?;
                let host = match target {
                    EventTarget::Window => EventEmitTarget::Window,
                    EventTarget::Document => EventEmitTarget::Document,
                    // The remaining arm is `Body` (Node handled above).
                    _ => EventEmitTarget::Body,
                };
                (host, shape)
            }
        };
        let analyzed = self.ir.analysis.expressions.get(event.handler);
        let handler = self.rewrite(event.handler, analyzed.scope)?;
        let emit = EventEmit {
            // A GLOBAL host event is ALWAYS a direct `$.event(...)` registration (never
            // delegated) — the lowering records `delegated: false` for the host targets, so
            // this maps to `Direct`.
            mode: if event.delegated {
                EventMode::Delegated
            } else {
                EventMode::Direct
            },
            target: emit_target,
            event_type: event.event_type.clone(),
            capture: event.capture,
            passive: event.passive,
            wrappers: event_wrappers(&event.modifiers),
            handler,
        };
        Ok(ClientRuntimeOp::Event { emit, shape })
    }

    /// The getter + setter bodies for a `bind:` target. The getter is the bound
    /// expression rewritten as a value read. The setter is shaped by the target's
    /// STRUCTURAL lvalue kind (never a raw-text assignment):
    ///
    /// - a bare-IDENTIFIER signal target sets the signal directly
    ///   (`$.set(name, $$value)`);
    /// - a bare-IDENTIFIER non-signal target (a plain local) assigns it directly
    ///   (`name = $$value`);
    /// - a MEMBER target (`obj.x`, `a[i]`) writes through the SAME fallible
    ///   lvalue-aware rewrite as the getter, so a signal-wrapped-proxy member write
    ///   is `$.get(obj).x = $$value` (NOT a raw `obj.x = $$value` — the raw form
    ///   would read the unproxied object and miss the reactive write);
    /// - a FUNCTION-PAIR target (`{get, set}`) returns the TWO user-supplied
    ///   expressions, each rewritten INDEPENDENTLY as a value expression (so a signal
    ///   read/write inside an inline arrow lowers — `() => $.get(v)` /
    ///   `(x) => $.set(v, x, true)` — while a bare function identifier passes
    ///   through). The emitter passes them DIRECTLY to the helper (no thunk wrap).
    ///
    /// The lvalue kind is decided STRUCTURALLY from the parsed OXC node (never a
    /// `source.contains('.')` text scan); a non-lvalue target was already refused by
    /// the classifier.
    pub(super) fn bind_getter_setter(
        &self,
        expr: ExprId,
        _target: &str,
        should_proxy: bool,
    ) -> Result<(String, String), UnsupportedSvelteRuntimeSurface> {
        let analyzed = self.ir.analysis.expressions.get(expr);
        // `text` is the raw bound-target source — used ONLY as the human-readable diagnostic
        // label for a fail-closed refusal, NEVER as the identifier root (the root comes from
        // the typed `bind_target.root_ident` below, so a parenthesized identifier resolves
        // correctly). The shared bind-target fact (computed once at analysis time) is the SOLE
        // classification authority here — no per-call reparse.
        let text = analyzed.source.trim();
        let kind = analyzed.bind_target.kind;
        // A FUNCTION-PAIR returns the two rewritten sequence elements directly, so the
        // whole-expression rewrite (which would `(get, set)`-comma-join them) is NOT
        // used as the getter — each element is rewritten on its own.
        if let Some(super::expr::BindTargetKind::FunctionPair) = kind {
            // The plain-Svelte-JS function-pair slices (the default-closed lane: parsed as
            // `mjs`, exact two-element shape, NO TS-only construct) are carried on the fact.
            // Each element is rewritten through the PLAIN-JS rewrite lane (mjs parse, NO TS
            // strip) — so a valid-JS element the TSX parser would reinterpret as TS (the
            // ``tag<string>`x` `` trap) is not corrupted. An absent pair (the fact's
            // `function_pair` is `None`) fails closed.
            let Some((get_src, set_src)) = &analyzed.bind_target.function_pair else {
                return Err(UnsupportedSvelteRuntimeSurface::Binding {
                    target: text.to_string(),
                    span: Span::new(0, 0),
                });
            };
            let getter = self.rewrite_source_plain_js(get_src, analyzed.scope)?;
            let setter = self.rewrite_source_plain_js(set_src, analyzed.scope)?;
            return Ok((getter, setter));
        }
        let getter = self.rewrite(expr, analyzed.scope)?;
        let setter = match kind {
            // A bare identifier: a signal sets directly; a plain local assigns
            // directly. The signal vs plain decision reads the resolved binding kind. The
            // identifier ROOT comes from the typed fact (`root_ident`), NOT `source.trim()`
            // — so a parenthesized identifier (`bind:value={(s)}`) sets its root `s`
            // (`$.set(s, $$value)`), matching official. An Identifier kind always carries a
            // root; a missing root fails closed defensively.
            Some(super::expr::BindTargetKind::Identifier) => {
                let Some(root) = analyzed.bind_target.root_ident.as_deref() else {
                    return Err(UnsupportedSvelteRuntimeSurface::Binding {
                        target: text.to_string(),
                        span: Span::new(0, 0),
                    });
                };
                let is_signal = self
                    .ir
                    .analysis
                    .bindings
                    .resolve_kind(&self.ir.analysis.scopes, analyzed.scope, root)
                    .is_some_and(is_signal_kind);
                if is_signal {
                    // A SPECIAL-host bind (`should_proxy`) carries the 3rd `true` proxy-flag
                    // arg (`$.set(local, $$value, true)`) — the window/document/body/dynamic-
                    // element setter baseline; an ordinary DOM bind stays `$.set(local,
                    // $$value)`.
                    if should_proxy {
                        format!("$.set({root}, $$value, true)")
                    } else {
                        format!("$.set({root}, $$value)")
                    }
                } else {
                    format!("{root} = $$value")
                }
            }
            // A member target: the LHS is the bound member expression rewritten as a
            // value read (identical to the getter — `$.get(obj).x`), assigned to.
            // Routing through the rewriter is the fix for the signal-wrapped-proxy
            // member write.
            Some(super::expr::BindTargetKind::Member) => {
                let lvalue = self.rewrite(expr, analyzed.scope)?;
                format!("{lvalue} = $$value")
            }
            // A function-pair was handled above; a non-lvalue target was refused by the
            // classifier (`bind:value={f()}`). Defensive — fail closed rather than emit
            // `f() = $$value` / a comma-joined `(get, set) = $$value`.
            Some(super::expr::BindTargetKind::FunctionPair) | None => {
                return Err(UnsupportedSvelteRuntimeSurface::Binding {
                    target: text.to_string(),
                    span: Span::new(0, 0),
                });
            }
        };
        Ok((getter, setter))
    }

    /// Whether a `bind:group` input's SINGLE `value` expression is PROVABLY DEFINED — the
    /// official `evaluated.is_defined` gate that suppresses the outer `?? ''` group-value
    /// coercion. Reuses the SAME `mixed_chunk_nullish_wrap` definedness analysis the
    /// mixed-attribute parts run (no new analysis path): a `None` (provably-defined) result
    /// suppresses the coercion, `Bare` / `Parenthesized` keeps it. `false` when the `value`
    /// attr is not a single expression (a bare `value={n}` Dynamic or a one-interpolation
    /// `value="{n}"` Mixed are the two shapes `attr_value_for` lowers to `AttrValue::Single`).
    /// Read by [`SupportedClientIr::collect_group_dynamic_values`].
    pub(super) fn group_value_single_is_defined(&self, el: &super::ir::ElementIr) -> bool {
        let expr = el.attrs.iter().find_map(|attr| match attr {
            AttrIr::Dynamic { name, expr } if name == "value" => Some(*expr),
            AttrIr::Mixed { name, parts } if name == "value" => match parts.as_slice() {
                [MixedAttrPart::Expr(e)] => Some(*e),
                _ => None,
            },
            _ => None,
        });
        let Some(expr) = expr else {
            return false;
        };
        let analyzed = self.ir.analysis.expressions.get(expr);
        let has_call = self.expr_has_call(expr);
        matches!(
            super::reactive_fold::mixed_chunk_nullish_wrap(
                analyzed.source,
                analyzed.scope,
                &self.ir.analysis.bindings,
                &self.ir.analysis.scopes,
                self.ir.analysis.scripts.instance_source,
                has_call,
            ),
            super::reactive_fold::NullishCoalesce::None
        )
    }
}

/// Whether a `bind:` target node is a SPECIAL HOST (`<svelte:window|document|body|
/// element>`) — the host-driven `should_proxy` discriminant. A special-host bind emits the
/// proxied setter (`$.set(local, $$value, true)`); an ordinary element bind does not.
/// Structural over the typed IR node, never a name scan.
///
/// This is the SOLE authority for the emitted proxy flag — NOT the `should_proxy` column on
/// the bind-contract row (which is DOCUMENTARY, consumed only by the contract test). A row
/// carrying `should_proxy: false` (e.g. `focused`) still emits the proxied setter on a special
/// host because THIS predicate drives it. See the `BindContract::should_proxy` field doc.
fn bind_target_is_special_host(node: &IrNode) -> bool {
    matches!(
        node,
        IrNode::Special(s) if matches!(
            s.kind,
            SpecialKind::Window | SpecialKind::Document | SpecialKind::Body | SpecialKind::Element
        )
    )
}

/// Find the accepted handler shape recorded for a GLOBAL-host event (`<svelte:window|
/// document|body>`), keyed by (event type, handler expr) — the host-keyed lookup that does
/// NOT consult a DOM node id (a global host event carries the global as its event target,
/// not a node). The handler expr id is globally unique in the expression arena, so the
/// (event type, handler) pair is an unambiguous key. Returns `None` when no fact matches (a
/// classifier/plan divergence the caller fails closed on).
fn find_host_event_shape(
    facts: &[(NodeId, String, ExprId, ClientEventHandlerShape)],
    event_type: &str,
    handler: ExprId,
) -> Option<ClientEventHandlerShape> {
    facts
        .iter()
        .find(|(_, ty, h, _)| ty == event_type && *h == handler)
        .map(|(_, _, _, shape)| shape.clone())
}

/// Build the legacy modifier WRAPPER stack from an event's modifier set, in the FIXED
/// official application order ([`EventWrapper::ORDER`], inner→outer) INDEPENDENT of
/// source order — only the recognized wrapper modifiers (`stopPropagation` …`once`)
/// contribute; `capture` / `passive` / `nonpassive` are positional args, not wrappers,
/// and are skipped here. The result drives the emitter's inner-to-outer
/// `$.<modifier>(handler)` nesting.
pub(super) fn event_wrappers(modifiers: &[String]) -> Vec<EventWrapper> {
    EventWrapper::ORDER
        .into_iter()
        .filter(|wrapper| {
            modifiers
                .iter()
                .any(|m| EventWrapper::from_modifier(m) == Some(*wrapper))
        })
        .collect()
}

/// Find the accepted handler shape recorded for a SPECIFIC event — keyed by the target
/// node, the normalized event type, AND the handler expression id. The full key is what
/// keeps an element with multiple events (`<button onfocus={a} onclick={b}>`) from
/// collapsing onto the element's first recorded event: a node-only match would return
/// some event's shape for ANY event on the node, but each event must resolve to its OWN
/// fact. Returns `None` when no fact matches the exact key (a classifier/plan divergence
/// the caller fails closed on).
fn find_event_shape(
    facts: &[(NodeId, String, ExprId, ClientEventHandlerShape)],
    node: NodeId,
    event_type: &str,
    handler: ExprId,
) -> Option<ClientEventHandlerShape> {
    facts
        .iter()
        .find(|(n, ty, h, _)| *n == node && ty == event_type && *h == handler)
        .map(|(_, _, _, shape)| shape.clone())
}

#[cfg(test)]
mod tests {
    use super::find_event_shape;
    use super::ClientEventHandlerShape;
    use super::{ExprId, NodeId};

    #[test]
    fn event_shape_lookup_is_keyed_by_node_event_and_handler() {
        let node = NodeId(1);
        // Two events on the SAME node (`<button onfocus={a} onclick={b}>`), distinguished
        // ONLY by their event type + handler expr id — the shape value is identical, so
        // the discriminator is the KEY, not the value.
        let facts = vec![
            (
                node,
                "focus".to_string(),
                ExprId(10),
                ClientEventHandlerShape::Inline,
            ),
            (
                node,
                "click".to_string(),
                ExprId(20),
                ClientEventHandlerShape::Inline,
            ),
        ];
        // Each event resolves to ITS OWN fact under the full (node, type, handler) key.
        assert_eq!(
            find_event_shape(&facts, node, "focus", ExprId(10)),
            Some(ClientEventHandlerShape::Inline)
        );
        assert_eq!(
            find_event_shape(&facts, node, "click", ExprId(20)),
            Some(ClientEventHandlerShape::Inline)
        );
        // DISCRIMINATING: a coarse node-only match would return the element's FIRST fact
        // for EVERY query on the node — so these would wrongly resolve to `Some`. The
        // precise key returns `None` because no fact has that exact (type, handler)
        // combination, proving the lookup does not collapse a sibling event's fact.
        assert_eq!(find_event_shape(&facts, node, "click", ExprId(10)), None);
        assert_eq!(find_event_shape(&facts, node, "focus", ExprId(20)), None);
        assert_eq!(
            find_event_shape(&facts, node, "mouseover", ExprId(10)),
            None
        );
    }
}
