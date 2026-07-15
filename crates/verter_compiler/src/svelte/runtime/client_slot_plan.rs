//! The `<slot>` outlet PROJECTION + EMISSION — the official `SlotElement`
//! client transform (`$.slot(node, $$props, name, props, fallback)`).
//!
//! The projection half ([`SupportedClientIr::project_slot`]) builds the typed
//! [`ClientSlot`] from an [`IrNode::Slot`](super::ir::IrNode): every prop value
//! is PREPARED through the sole
//! [`prepare_template_value`](SupportedClientIr::prepare_template_value)
//! entry (the official `build_expression` deep-read/untrack sequence in a
//! definitely-legacy component), a `has_call` value memoizes through the
//! shared MODE-AWARE [`DerivedMemoizer`] (`$.derived` in runes mode,
//! `$.derived_safe_equal` in every non-runes mode), and the official spread
//! topology is preserved (ONE leading ordinary-props object, then every spread
//! thunk in source order — a slot spread NEVER memoizes and NEVER wraps,
//! exactly the official `SlotElement.js` plain `b.thunk`). The emission half
//! ([`ClientEmitter::emit_slot`]) renders the call against the walked `<!>`
//! anchor var and recurses into the fallback region through the shared region
//! emitter.

use super::client::ClientEmitter;
use super::client_codegen_helpers::{js_single_quoted, object_key};
use super::client_component_plan::DerivedMemoizer;
use super::client_plan::SupportedClientIr;
use super::client_plan_types::{ClientNode, ClientSlot, SlotProp};
use super::ir::{AttrIr, ExprId, NodeId, SlotElementIr, SvelteMode};
use super::unsupported::UnsupportedSvelteRuntimeSurface;

impl<'a> SupportedClientIr<'a> {
    /// Project a `<slot>` outlet node into its [`ClientNode::Slot`].
    ///
    /// The official `SlotElement.js` attribute loop, replayed over the typed
    /// attr inventory in SOURCE order: the `name` attribute was consumed at
    /// lowering (and validated by the classifier), a `slot` attribute is
    /// DROPPED (official skips it), a spread pushes its thunk, and every other
    /// plain attribute becomes a props-object member — a getter when
    /// state-bearing, a plain init otherwise, with a `has_call` value memoized
    /// through the shared per-slot memoizer (`$.get($N)`).
    pub(super) fn project_slot(
        &self,
        slot: &SlotElementIr,
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        let mut memoizer = DerivedMemoizer::new(self.ir.component.mode == SvelteMode::Runes);
        let mut props: Vec<SlotProp> = Vec::new();
        let mut spreads: Vec<String> = Vec::new();
        for attr in &slot.props {
            match attr {
                // The consumed `name` + the dropped `slot` attribute (any shape).
                AttrIr::Static { name, .. }
                | AttrIr::Dynamic { name, .. }
                | AttrIr::Mixed { name, .. }
                    if name == "name" || name == "slot" => {}
                AttrIr::Static { name, value } => props.push(SlotProp::Init {
                    key: name.clone(),
                    value: match value {
                        Some(v) => js_single_quoted(v.value.as_str()),
                        None => "true".to_string(),
                    },
                }),
                AttrIr::Dynamic { name, expr } => {
                    let member = self.project_slot_dynamic_prop(name, *expr, &mut memoizer)?;
                    props.push(member);
                }
                AttrIr::Mixed { parts, name } => {
                    let (value, has_state) = self.mixed_attr_value(
                        parts,
                        super::client_legacy_value::AuthoredValueSurface::SlotProp,
                    )?;
                    let rendered = self.render_memoized_attr_value(&value, &mut memoizer);
                    props.push(if has_state {
                        SlotProp::Getter {
                            key: name.clone(),
                            body: rendered,
                        }
                    } else {
                        SlotProp::Init {
                            key: name.clone(),
                            value: rendered,
                        }
                    });
                }
                AttrIr::Spread { expr } => {
                    // A slot spread is the official plain `b.thunk` — NEVER
                    // memoized (unlike a component spread) and NEVER
                    // legacy-wrapped (`SlotElement.js` pushes the visited spread
                    // without a memoize callback or `build_expression`) — the
                    // RAW policy row; the prepared thunk applies the shared
                    // zero-arg unthunk.
                    let prepared = self.prepare_template_value(
                        super::client_legacy_value::AuthoredExpr(*expr),
                        super::client_legacy_value::AuthoredValueSurface::SlotSpreadOperand,
                    )?;
                    spreads.push(prepared.thunk());
                }
                // `let:` was refused by the classifier — defensive skip keeps the
                // projection total over the accepted inventory.
                AttrIr::Let { .. } => {}
                // Every directive family was refused by the classifier (the
                // official `slot_element_invalid_attribute`) — defensive refusal,
                // never a silent drop.
                _ => {
                    return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                        construct: "slot attribute",
                        span: slot.span,
                    });
                }
            }
        }
        // The official `null`-vs-callback decision keys on the RAW fragment node
        // list (`node.fragment.nodes.length === 0`): a child-less slot is `null`;
        // a whitespace-only fallback keeps its (empty-emitting) callback.
        let fallback =
            (!self.ir.template_scope(slot.fallback).roots.is_empty()).then_some(slot.fallback);
        Ok(ClientNode::Slot(ClientSlot {
            span: slot.span,
            name: slot.name.clone(),
            props,
            spreads,
            memo_hoists: memoizer.into_statements(),
            fallback,
        }))
    }

    /// Project one DYNAMIC slot prop (`foo={expr}` / shorthand `{foo}`) — the
    /// official slot memoize rule (`metadata.has_call || has_await` ⇒
    /// `$.get(memoizer.add(value))`) over the SHARED legacy-wrapped value, then
    /// the `has_state ? getter : init` member choice.
    fn project_slot_dynamic_prop(
        &self,
        name: &str,
        expr: ExprId,
        memoizer: &mut DerivedMemoizer,
    ) -> Result<SlotProp, UnsupportedSvelteRuntimeSurface> {
        let analyzed = self.ir.analysis.expressions.get(expr);
        let has_state = super::reactive_analysis::prop_value_has_state(
            &analyzed.references,
            analyzed.source,
            analyzed.scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
        )
        .map_err(|()| {
            UnsupportedSvelteRuntimeSurface::expression_fact_recovery("binding-impurity")
        })?;
        // The sole authored-value preparation (wrap first), then the slot
        // memoize rule over the prepared value.
        let prepared = self.prepare_template_value(
            super::client_legacy_value::AuthoredExpr(expr),
            super::client_legacy_value::AuthoredValueSurface::SlotProp,
        )?;
        let final_value = if prepared.has_call() {
            memoizer.memoize(prepared.memo_input())
        } else {
            prepared.inline_expression()
        };
        Ok(if has_state {
            SlotProp::Getter {
                key: name.to_string(),
                body: final_value,
            }
        } else {
            SlotProp::Init {
                key: name.to_string(),
                value: final_value,
            }
        })
    }
}

impl<'a> ClientEmitter<'a> {
    /// Emit a projected `<slot>` outlet against its walked `<!>` anchor var:
    /// `$.slot(<anchor>, $$props, '<name>', <props>, <fallback>);`, with the
    /// memoized-value hoists wrapping the statement in a `{ … }` block (the
    /// official `b.block(statements)` when the per-slot memoizer produced
    /// deriveds), and the non-empty fallback emitted as its own
    /// `($$anchor) => { … }` region callback.
    pub(super) fn emit_slot(
        &mut self,
        out: &mut super::output::SvelteRuntimeOutput,
        node_id: NodeId,
        anchor_var: &str,
    ) {
        let ClientNode::Slot(slot) = self.client_node(node_id) else {
            return;
        };
        let slot: ClientSlot = slot.clone();
        out.push('\t');
        let needs_block = !slot.memo_hoists.is_empty();
        if needs_block {
            out.push('{');
            for stmt in &slot.memo_hoists {
                out.push_str(stmt);
            }
        }
        // The props payload: the ONE ordinary object, wrapped in
        // `$.spread_props(object, thunk, …)` when any spread is present.
        let object = render_slot_props_object(&slot.props);
        let props = if slot.spreads.is_empty() {
            object
        } else {
            format!("$.spread_props({object}, {})", slot.spreads.join(", "))
        };
        let name = js_single_quoted(&slot.name);
        match slot.fallback {
            None => {
                out.push_str(&format!(
                    "$.slot({anchor_var}, $$props, {name}, {props}, null);"
                ));
            }
            Some(region) => {
                out.push_str(&format!(
                    "$.slot({anchor_var}, $$props, {name}, {props}, ($$anchor) => {{"
                ));
                self.emit_region(out, region, "$$anchor");
                out.push_str("});");
            }
        }
        if needs_block {
            out.push('}');
        }
        out.push('\n');
    }
}

/// Render the ordinary-props object — `{}` when empty, `{ k: v, get k() { … } }`
/// otherwise (each key routed through [`object_key`], so a hyphenated prop
/// quotes).
fn render_slot_props_object(props: &[SlotProp]) -> String {
    if props.is_empty() {
        return "{}".to_string();
    }
    let members = props
        .iter()
        .map(|prop| match prop {
            SlotProp::Init { key, value } => format!("{}: {value}", object_key(key)),
            SlotProp::Getter { key, body } => {
                format!("get {}() {{ return {body}; }}", object_key(key))
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {members} }}")
}
