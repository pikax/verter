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
use super::client_plan_types::{ClientNodeId, ClientRuntimeOp};
use super::expr::ScopeId;
use super::ir::{AttrIr, BindOp, EventOp, EventTarget, ExprId, MixedAttrPart, NodeId};
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
        let (getter, setter) = self.bind_getter_setter(bind.expr, &bind.target)?;
        Ok(ClientRuntimeOp::Bind {
            target: ClientNodeId(target.0),
            shape,
            getter,
            setter,
        })
    }

    /// Project an event op into the narrow [`ClientRuntimeOp::Event`], carrying the
    /// classifier's accepted [`ClientEventHandlerShape`] fact.
    pub(super) fn project_event_op(
        &self,
        target: EventTarget,
        event: &EventOp,
        _scope: ScopeId,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        let EventTarget::Node(node_id) = target else {
            return Err(UnsupportedSvelteRuntimeSurface::NonDelegatedEvent {
                event_type: event.event_type.clone(),
                span: Span::new(0, 0),
            });
        };
        // The accepted handler SHAPE the classifier recorded for this target node. An
        // event op with NO recorded shape is a classifier/plan divergence — fail
        // closed defensively (never emit an unclassified, possibly-non-function
        // handler).
        let shape = self
            .event_shapes
            .iter()
            .find(|(n, _)| *n == node_id)
            .map(|(_, s)| s.clone())
            .ok_or_else(|| UnsupportedSvelteRuntimeSurface::NonDelegatedEvent {
                event_type: event.event_type.clone(),
                span: Span::new(0, 0),
            })?;
        let analyzed = self.ir.analysis.expressions.get(event.handler);
        let handler = self.rewrite(event.handler, analyzed.scope)?;
        Ok(ClientRuntimeOp::Event {
            target: ClientNodeId(node_id.0),
            event_type: event.event_type.clone(),
            shape,
            handler,
        })
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
                    format!("$.set({root}, $$value)")
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
