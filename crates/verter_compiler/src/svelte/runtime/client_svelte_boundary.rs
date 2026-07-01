//! The `<svelte:boundary>` error-boundary PROJECTION + EMISSION.
//!
//! A `<svelte:boundary>` is a COMMENT-ANCHORED renderable (the same comment-anchor frame a
//! control-flow block / `<svelte:element>` uses) whose `<!>` anchor hosts a `$.boundary(node,
//! { onerror, failed, pending }, ($$anchor) => { <body> })` call. The `failed` / `pending`
//! `{#snippet}` defs hoist to `const`s in a wrapping `{ … }` block above the call (when
//! present) and are passed by NAME (object shorthand) in the props object alongside the
//! optional `onerror` handler; the body is the callback's region (emitted through the shared
//! [`emit_region_callback`](ClientEmitter::emit_region_callback)).

use super::client::ClientEmitter;
use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_component_emit::CallbackPlacement;
use super::client_plan::SupportedClientIr;
use super::client_plan_types::{BoundaryAttrProp, ClientBoundary, ClientNode};
use super::ir::{AttrIr, BlockIr, EventOrigin, IrNode, NodeId, SpecialElementIr};

impl<'a> SupportedClientIr<'a> {
    /// Project a `<svelte:boundary>` into its narrow [`ClientNode::Boundary`]: the `onerror` /
    /// `failed` / `pending` ATTRIBUTE props (in source order, each getter-or-init per
    /// state-bearing-ness), the `failed` / `pending` snippet def node ids (hoisted + passed by
    /// name), and the body region.
    pub(super) fn project_svelte_boundary(
        &self,
        s: &SpecialElementIr,
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        let body_region =
            s.body_region
                .ok_or(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "svelte:boundary without body region",
                    span: s.span,
                })?;
        // The boundary ATTRIBUTE props — official's single attribute loop (`SvelteBoundary.js`).
        // Each accepted attribute (`onerror` / `failed` / `pending`) is rewritten to its emitted
        // form and recorded with its `has_state`, so the emitter picks the getter accessor vs the
        // plain init. Order is SOURCE order (the `s.attrs` iteration order), matching official.
        //
        // - `onerror={…}` lowers to an `AttrIr::Event` (event type `error`, `ModernAttribute`
        //   origin — the `OtherSpecial` host event path). The origin guard is the invariant
        //   `classify_svelte_boundary` already enforces (a legacy `on:error` is refused before
        //   projection), so matching it here keeps projection from ever picking up a legacy
        //   directive.
        // - `failed={…}` / `pending={…}` (and the shorthand `{failed}` / `{pending}`) lower to an
        //   `AttrIr::Dynamic`.
        //
        // `has_state` is the SAME sync-only, snippet-name-aware predicate the analogous
        // `Component.js` prop getter-vs-init decision uses (`prop_value_has_state`): a prop /
        // signal / snippet reference is state-bearing (⇒ getter), an inline `onerror` arrow whose
        // only reads are inside its own body is a constant (⇒ plain init).
        let mut attr_props = Vec::new();
        for attr in &s.attrs {
            match attr {
                AttrIr::Event {
                    event_type,
                    handler,
                    origin: EventOrigin::ModernAttribute,
                    ..
                } if event_type == "error" => {
                    attr_props.push(self.boundary_attr_prop("onerror", *handler)?);
                }
                AttrIr::Dynamic { name, expr } if name == "failed" || name == "pending" => {
                    attr_props.push(self.boundary_attr_prop(name, *expr)?);
                }
                _ => {}
            }
        }
        Ok(ClientNode::Boundary(ClientBoundary {
            attr_props,
            snippets: s.slots.snippet_defs.clone(),
            body_region,
        }))
    }

    /// Build one boundary [`BoundaryAttrProp`] — the rewritten value expression plus its
    /// `has_state` (the getter-vs-init discriminator, official's `metadata.expression.has_state`).
    fn boundary_attr_prop(
        &self,
        name: &str,
        expr: super::ir::ExprId,
    ) -> Result<BoundaryAttrProp, UnsupportedSvelteRuntimeSurface> {
        let analyzed = self.ir.analysis.expressions.get(expr);
        let has_state = super::reactive_analysis::prop_value_has_state(
            analyzed.source,
            analyzed.scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
        );
        Ok(BoundaryAttrProp {
            name: name.to_string(),
            expr: self.rewrite(expr, analyzed.scope)?,
            has_state,
        })
    }
}

impl<'a> ClientEmitter<'a> {
    /// Emit a projected `<svelte:boundary>` against its `<!>` anchor var: `$.boundary(node, {
    /// onerror, failed, pending }, ($$anchor) => { <body> })`. The `failed` / `pending` snippet
    /// `const`s hoist into a wrapping `{ … }` block above the call (when present), referenced
    /// by name in the props.
    pub(super) fn emit_svelte_boundary(
        &mut self,
        out: &mut String,
        node: NodeId,
        anchor_var: &str,
    ) {
        let ClientNode::Boundary(b) = self.client_node(node) else {
            return;
        };
        let b: ClientBoundary = b.clone();

        // A wrapping `{ … }` block hoists the `failed` / `pending` snippet `const`s above the
        // boundary call (the official `block([...hoisted, boundary])`). With no snippet, the
        // boundary call is emitted directly (no block).
        let needs_block = !b.snippets.is_empty();
        out.push('\t');
        if needs_block {
            out.push('{');
        }
        for &snippet in &b.snippets {
            self.emit_snippet_decl(out, snippet);
        }
        // The props object: each ATTRIBUTE prop in source order — a getter accessor `get name() {
        // return <expr>; }` when state-bearing, else a plain `name: <expr>` init (official's
        // `SvelteBoundary.js` `has_state ? b.get : b.init`) — THEN each snippet's NAME (object
        // shorthand), also in source order. Official processes ALL attributes before the fragment
        // snippets, so the attribute props always precede the snippet shorthands (and a same-named
        // `failed` attribute + `{#snippet failed}` child yield BOTH keys — a duplicate-key object,
        // official parity, no dedupe).
        let mut props: Vec<String> = Vec::new();
        for p in &b.attr_props {
            if p.has_state {
                props.push(format!("get {}() {{ return {}; }}", p.name, p.expr));
            } else {
                props.push(format!("{}: {}", p.name, p.expr));
            }
        }
        for &snippet in &b.snippets {
            props.push(self.boundary_snippet_name(snippet));
        }
        out.push_str(&format!(
            "$.boundary({anchor_var}, {{{}}}, ",
            props.join(", ")
        ));
        // The body callback `($$anchor) => { <body> }` through the shared region-callback
        // emitter.
        self.emit_region_callback(
            out,
            b.body_region,
            &["$$anchor".to_string()],
            &[],
            CallbackPlacement::InlineArg,
        );
        out.push_str(");");
        if needs_block {
            out.push('}');
        }
        out.push('\n');
    }

    /// The declared name of a `{#snippet}` def node (the boundary's `failed` / `pending`) — the
    /// props-shorthand key + the hoisted `const`'s name.
    fn boundary_snippet_name(&self, node: NodeId) -> String {
        if let IrNode::Block(BlockIr::Snippet { name, .. }) = self.ir().node(node) {
            self.ir().analysis.bindings.get(*name).name.clone()
        } else {
            String::new()
        }
    }
}
