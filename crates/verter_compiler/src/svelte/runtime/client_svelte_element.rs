//! The `<svelte:element this={…}>` dynamic-element PROJECTION + EMISSION.
//!
//! A `<svelte:element>` is a COMMENT-ANCHORED renderable (the SAME `var fragment =
//! $.comment(); var node = $.first_child(fragment); … ; $.append($$anchor, fragment);`
//! topology a control-flow block uses) whose `<!>` anchor hosts a `$.element(node, () =>
//! <tag>, <is_svg>, ($$element, $$anchor) => { … })` call. The callback body is the
//! element's own setup — the `bind:this` ref captures, then the attribute build (the
//! lone-class `$.set_class($$element, 0, …)` fast path OR the single
//! `$.attribute_effect($$element, () => ({ … }))` fold with its hoisted event handlers —
//! see [`svelte_element_attr_route`]), then the remaining `$$element`-hosted binds and
//! the legacy `on:` registrations — followed by the child-content body region (emitted
//! through the shared
//! [`emit_region_callback_with_prelude`](ClientEmitter::emit_region_callback_with_prelude)).
//! The callback is OMITTED (a 3-argument `$.element(node, get_tag, is_svg)` call) when the
//! whole inner body is empty (no attributes, no binds, no rendered children).

use super::client::ClientEmitter;
use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_codegen_helpers::js_single_quoted;
use super::client_component_emit::CallbackPlacement;
use super::client_effect::{emit_text_effect, EffectBody, Memoizer};
use super::client_event::render_event_registration;
use super::client_plan::SupportedClientIr;
use super::client_plan_bind::event_wrappers;
use super::client_plan_types::{
    AttributeEffectItem, ClientElementBind, ClientNode, ClientRuntimeOp, ClientSvelteElement,
    EventEmit, EventEmitTarget, EventMode,
};
use super::client_shapes::ClientBindShape;
use super::ir::{AttrIr, BindOp, NodeId, SpecialElementIr};

impl<'a> SupportedClientIr<'a> {
    /// Project a `<svelte:element this={…}>` into its narrow [`ClientNode::SvelteElement`]:
    /// the get-tag thunk body, the routed attribute build (the lone-class `$.set_class`
    /// pieces via the SHARED class projection, OR the `$.attribute_effect` fold items —
    /// attributes + events in source order plus the directive-synthesized `class: ''` /
    /// `style: ''` entries), the `$$element`-hosted binds (each with its rewritten
    /// proxied setter), and the child-content body region. A bind whose `(name,
    /// svelte:element)` pair has no host-scoped routing (`bind:value` /
    /// `bind:devicePixelRatio` / …) fails closed at the shared bind classifier (the §1.8
    /// wrong-host/invalid-name negatives).
    pub(super) fn project_svelte_element(
        &self,
        s: &SpecialElementIr,
        node_id: NodeId,
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        // The get-tag thunk BODY: a DYNAMIC `this={…}` prepares its expression
        // through the sole authored-value entry (a RAW semantic role — official
        // visits the tag expression without `build_expression`); a STATIC
        // `this="div"` is the single-quoted literal. A `<svelte:element>` with
        // neither is a parse error upstream — fail closed defensively.
        let get_tag = if let Some(expr) = s.this_expr {
            self.prepare_template_value(
                super::client_legacy_value::AuthoredExpr(expr),
                super::client_legacy_value::AuthoredValueSurface::SvelteElementThis,
            )?
            .inline_expression()
        } else if let Some(tag) = &s.static_tag {
            js_single_quoted(tag)
        } else {
            return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "svelte:element without this",
                span: s.span,
            });
        };
        // The body region MUST exist for a renderable-region special (set at lowering).
        let body_region =
            s.body_region
                .ok_or(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "svelte:element without body region",
                    span: s.span,
                })?;

        let mut binds: Vec<ClientElementBind> = Vec::new();
        let mut events: Vec<String> = Vec::new();
        // The scope hash for THIS dynamic element — `Some` iff the selector-to-template
        // matcher marked the host node scoped (the SAME shared [`CssScopeFacts`] read
        // every other injection site consumes). The SCOPED fact feeds the route (the
        // official analyze-phase synthetic-class condition reads `node.metadata.scoped`)
        // and the fold's official 6th `$.attribute_effect` argument; the SetClass fast
        // path reads the hash again through the shared class projection.
        //
        // [`CssScopeFacts`]: super::css::types::CssScopeFacts
        let scope_hash = self
            .css_scope
            .as_ref()
            .and_then(|facts| facts.hash_for(node_id));
        // The official `SvelteElement` attribute ROUTE (see [`svelte_element_attr_route`]):
        // the lone-static-class `$.set_class($$element, 0, …)` fast path (WITH any
        // co-located `class:` directives merged into the directive-object argument, via the
        // SHARED regular-element class projection), the `$.attribute_effect` fold, or no
        // attribute emission. On the fast path the class attribute + the `class:`
        // directives are CONSUMED by the pieces (they do not fold).
        let route = svelte_element_attr_route(&s.attrs, scope_hash.is_some());
        let set_class = if matches!(route, SvelteElementAttrRoute::SetClass) {
            Some(self.project_set_class_pieces(node_id, &s.attrs)?)
        } else {
            None
        };
        // The NON-fold attribute families: `bind:` (each with its rewritten
        // proxied getter/setter), the LEGACY `on:` directives (direct `$.event`
        // registrations — NOT fold entries), and the fail-closed lifecycle /
        // `let:` directives. The FOLD-eligible families route through the ONE
        // shared attribute-effect item builder below.
        for attr in &s.attrs {
            match attr {
                AttrIr::Bind {
                    target,
                    expr: Some(expr),
                } => {
                    let bind_op = BindOp {
                        target: target.clone(),
                        expr: *expr,
                    };
                    let scope = self.ir.analysis.expressions.get(*expr).scope;
                    let ClientRuntimeOp::Bind {
                        shape,
                        getter,
                        setter,
                        ..
                    } = self.project_bind_op(node_id, &bind_op, scope)?
                    else {
                        // `project_bind_op` always returns a `Bind` op.
                        unreachable!("project_bind_op returns a Bind op");
                    };
                    binds.push(ClientElementBind {
                        shape,
                        getter,
                        setter,
                    });
                }
                AttrIr::Bind { expr: None, target } => {
                    // A shorthand `bind:x` with no expression is not an emittable bind.
                    return Err(UnsupportedSvelteRuntimeSurface::Binding {
                        target: target.clone(),
                        span: s.span,
                    });
                }
                AttrIr::Event {
                    event_type,
                    handler,
                    capture,
                    modifiers,
                    passive,
                    origin,
                    ..
                } => {
                    // A LEGACY `on:` directive on a `<svelte:element>` is a DIRECT
                    // `$.event('type', $$element, <wrapped-handler>[, capture][, passive])`
                    // registration (the official `SvelteElement` `OnDirective` → `after_update`
                    // path) — NOT an `$.attribute_effect` fold entry (that is the MODERN `on*`
                    // attribute form, an `AttrIr::Dynamic`). This carries the modifier wrappers /
                    // capture / passive that the fold form silently dropped. Rendered through the
                    // SHARED event substrate (`render_event_registration`) against the `$$element`
                    // host (a callback-local, so the node-var map is empty).
                    let handler_body = self
                        .prepare_template_value(
                            super::client_legacy_value::AuthoredExpr(*handler),
                            super::client_legacy_value::AuthoredValueSurface::EventHandler,
                        )?
                        .inline_expression();
                    let emit = EventEmit {
                        mode: EventMode::Direct,
                        origin: *origin,
                        target: EventEmitTarget::SvelteElement,
                        event_type: event_type.clone(),
                        capture: *capture,
                        passive: *passive,
                        wrappers: event_wrappers(modifiers),
                        handler: handler_body,
                    };
                    let rendered =
                        render_event_registration(&emit, &rustc_hash::FxHashMap::default());
                    events.push(rendered.trim().to_string());
                }
                // A lifecycle directive (`use:` / `transition:` / `animate:` /
                // `{@attach}`) or `let:` on a dynamic element is the DEFERRED
                // host-lifecycle surface (ledger D-39) — fail closed (the gate already
                // refuses it, this is defensive).
                AttrIr::Use { .. }
                | AttrIr::Transition { .. }
                | AttrIr::Animate { .. }
                | AttrIr::Attach { .. }
                | AttrIr::Let { .. } => {
                    return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                        construct: "directive",
                        span: s.span,
                    });
                }
                // The FOLD-eligible families (static/dynamic/mixed attributes,
                // spreads, `class:`/`style:` directives) route through the shared
                // item builder below.
                _ => {}
            }
        }
        // The FOLD items — the SAME shared typed item builder the regular-element
        // spread fold uses (one wrap/memoize substrate, no per-host drift), with
        // the `<svelte:element>` host knobs: the analyze-phase synthesized
        // `class: ''` / `style: ''` entries and the SetClass-consumed class skip.
        let (synth_class, synth_style) = match route {
            SvelteElementAttrRoute::Fold {
                synth_class,
                synth_style,
            } => (synth_class, synth_style),
            _ => (false, false),
        };
        let fold: Vec<AttributeEffectItem> = if matches!(route, SvelteElementAttrRoute::Fold { .. })
        {
            self.attribute_effect_items(
                &s.attrs,
                super::client_plan_spread_html::AttributeEffectFoldOptions {
                    synth_class,
                    synth_style,
                    skip_class: false,
                },
            )?
        } else if set_class.is_some() {
            // The lone-class fast path consumed the class family; nothing folds.
            Vec::new()
        } else {
            Vec::new()
        };

        Ok(ClientNode::SvelteElement(ClientSvelteElement {
            get_tag,
            // SVG / MathML host elements are not in the client element allowlist, so a
            // dynamic element is always HTML-hosted on the reachable surface.
            is_svg: false,
            set_class,
            fold,
            // The fold's official 6th `$.attribute_effect` argument — the scope-hash
            // literal for a SCOPED node (`build_attribute_effect`), `None` otherwise.
            css_hash: scope_hash.map(js_single_quoted),
            binds,
            events,
            body_region,
        }))
    }
}

/// The `<svelte:element>` attribute ROUTE — which emission form the element's typed
/// attribute inventory takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SvelteElementAttrRoute {
    /// The official LONE-class fast path: `$.set_class($$element, 0, <base>[, css_hash,
    /// prev, next])`, with any co-located `class:` directives merged into the
    /// directive-object argument (`build_set_class`).
    SetClass,
    /// The `$.attribute_effect($$element, () => ({ … }))` fold. `synth_class` /
    /// `synth_style` carry the analyze-phase SYNTHESIZED empty `class` / `style`
    /// attribute entries (`class: ''` / `style: ''`) the fold must append after the real
    /// attributes.
    Fold {
        /// Whether the fold appends the directive-synthesized `class: ''` entry.
        synth_class: bool,
        /// Whether the fold appends the directive-synthesized `style: ''` entry.
        synth_style: bool,
    },
    /// No plain attributes and no `class:` / `style:` directives — no attribute
    /// emission (binds / legacy `on:` events are routed separately).
    None,
}

/// The official `SvelteElement` attribute routing over the typed `AttrIr` inventory —
/// the analyze-phase empty-`class`/`style` synthesis rule composed with the transform's
/// lone-class check:
///
/// - Official synthesizes an empty `class=""` when the node is SCOPED or `class:`
///   directives are present, with no `class` attribute and no spread
///   (`!has_spread && !has_class && (node.metadata.scoped || has_class_directive)`),
///   and an empty `style=""` when `style:` directives are present with no `style`
///   attribute and no spread (`phases/2-analyze/index.js`), appending them AFTER the
///   real attributes. `scoped` is the caller-provided per-node fact (the shared
///   `CssScopeFacts` membership) — never inferred from the attribute inventory.
/// - `attributes.length === 1 && class && is_text_attribute` — counting the synthetics,
///   NOT `on:` / `bind:` / `class:` / `style:` directives — routes to the
///   [`SetClass`](SvelteElementAttrRoute::SetClass) fast path (so a lone static-text
///   `class`, with or without co-located `class:` directives, a PURE `class:`
///   directive set, and a SCOPED node with no plain attributes all take `$.set_class`;
///   a co-located `style:` directive synthesizes the `style` attribute and forces the
///   fold).
/// - Any other non-empty effective attribute set routes to the
///   [`Fold`](SvelteElementAttrRoute::Fold).
///
/// Structural over the typed `AttrIr` — no source scan, no `starts_with("class")`.
/// SHARED by the projection (`project_svelte_element`) and the plan topology
/// (`topology.rs`) so the recorded helper (`$.set_class` vs `$.attribute_effect`) never
/// drifts from the emission — both callers pass the SAME per-node scoped fact.
pub(super) fn svelte_element_attr_route(attrs: &[AttrIr], scoped: bool) -> SvelteElementAttrRoute {
    let mut plain_count = 0usize;
    let mut has_spread = false;
    let mut has_class = false;
    let mut has_style = false;
    let mut has_class_dirs = false;
    let mut has_style_dirs = false;
    // The FIRST plain attribute (the `attributes[0]` of the official check; the
    // synthetics always append after the real attributes, so a single real attribute is
    // always first).
    let mut first_plain: Option<&AttrIr> = None;
    for attr in attrs {
        match attr {
            AttrIr::Static { name, .. }
            | AttrIr::Dynamic { name, .. }
            | AttrIr::Mixed { name, .. } => {
                plain_count += 1;
                first_plain.get_or_insert(attr);
                // Case-insensitive NAME matches — the official analyze-phase synthesis
                // checks `attribute.name.toLowerCase() === 'class'` / `'style'`.
                has_class |= name.eq_ignore_ascii_case("class");
                has_style |= name.eq_ignore_ascii_case("style");
            }
            AttrIr::Spread { .. } => {
                plain_count += 1;
                first_plain.get_or_insert(attr);
                has_spread = true;
            }
            AttrIr::Class { .. } => has_class_dirs = true,
            AttrIr::Style { .. } => has_style_dirs = true,
            _ => {}
        }
    }
    // The official synthetic-class condition (`phases/2-analyze/index.js`):
    // `!has_spread && !has_class && (node.metadata.scoped || has_class_directive)` —
    // a SCOPED node synthesizes the empty class even with no `class:` directives
    // (the spread path appends the hash on its own, so a spread suppresses it).
    let synth_class = !has_spread && !has_class && (scoped || has_class_dirs);
    let synth_style = !has_spread && !has_style && has_style_dirs;
    let effective = plain_count + usize::from(synth_class) + usize::from(synth_style);
    if effective == 0 {
        return SvelteElementAttrRoute::None;
    }
    if effective == 1 {
        // A PURE `class:` directive set — the single effective attribute is the
        // synthesized empty class (always static text) ⇒ the fast path with base `''`.
        if plain_count == 0 && synth_class {
            return SvelteElementAttrRoute::SetClass;
        }
        // A single REAL static-TEXT `class` (a valueless `class` is the boolean `true`,
        // not text — it folds). The NAME matches case-insensitively — the official
        // `attributes[0].name.toLowerCase() === 'class'`.
        if let Some(AttrIr::Static {
            name,
            value: Some(_),
        }) = first_plain
        {
            if name.eq_ignore_ascii_case("class") {
                return SvelteElementAttrRoute::SetClass;
            }
        }
    }
    SvelteElementAttrRoute::Fold {
        synth_class,
        synth_style,
    }
}

impl<'a> ClientEmitter<'a> {
    /// Emit a projected `<svelte:element>` against its `<!>` anchor var: `$.element(node, () =>
    /// <tag>, <is_svg>, ($$element, $$anchor) => { <setup> <body> })`. The setup — the
    /// `bind:this` ref captures, then the lone-class `$.set_class` OR the
    /// `$.attribute_effect` fold with hoisted event handlers, then the remaining
    /// `$$element`-hosted binds and the legacy `on:` registrations (the official
    /// `SvelteElement` init/update/after_update order) — is built into a prelude buffer,
    /// then the body region is emitted through the shared region-callback emitter. The
    /// callback is OMITTED (a 3-argument call) when the whole inner body is empty (no
    /// class/fold, no binds, no rendered children).
    pub(super) fn emit_svelte_element(&mut self, out: &mut String, node: NodeId, anchor_var: &str) {
        let ClientNode::SvelteElement(el) = self.client_node(node) else {
            return;
        };
        let el: ClientSvelteElement = el.clone();

        // (a) The element setup prelude, in the official `SvelteElement` order:
        //
        //   1. the `bind:this` ref captures (official pushes them into the init body
        //      DURING the attribute loop, BEFORE the class/attribute build runs — a fold
        //      expression may reference the captured ref);
        //   2. the lone-class `$.set_class` (a REACTIVE one as its own accumulator
        //      `$.template_effect`) OR the single `$.attribute_effect` fold (with its
        //      event-handler hoists);
        //   3. the remaining (measurement / property) binds — the official
        //      `after_update` slot;
        //   4. the legacy `on:` `$.event(...)` registrations.
        //
        // Built into a buffer so the hoist-name allocation + the bind emission run
        // through `self` before the callback.
        let mut setup = String::new();
        for bind in &el.binds {
            // The bind runs against `$$element` (resolved by `emit_bind`'s host-expr
            // router for a `SpecialKind::Element` target).
            if matches!(bind.shape, ClientBindShape::This { .. }) {
                self.emit_bind(&mut setup, node, &bind.shape, &bind.getter, &bind.setter);
            }
        }
        if let Some(sc) = &el.set_class {
            // The official lone-class fast path (`is_html` false ⇒ the `0` flags arg),
            // assembled by the SHARED `$.set_class` emitter against the `$$element`
            // callback param. A REACTIVE call declares its `let <classes>;` accumulator
            // and joins its own `$.template_effect` (the official `build_set_class`
            // update path, with the `has_call` directive object memoized into a `$N`
            // deps slot); a non-reactive call is a one-shot init.
            if sc.reactive {
                let acc = sc.accumulator_stem.map(|stem| self.alloc_name(stem));
                if let Some(acc) = &acc {
                    setup.push_str(&format!("let {acc};"));
                }
                let mut memoizer = Memoizer::default();
                let body = self.assemble_set_class(
                    "$$element".to_string(),
                    false,
                    &sc.value,
                    sc.css_hash.as_deref(),
                    sc.directives.as_deref(),
                    sc.directives_has_call,
                    acc.as_deref(),
                    &mut Some(&mut memoizer),
                );
                let deps = memoizer.into_deps();
                emit_text_effect(&mut setup, &[EffectBody::Expr(body)], &deps);
            } else {
                let body = self.assemble_set_class(
                    "$$element".to_string(),
                    false,
                    &sc.value,
                    sc.css_hash.as_deref(),
                    sc.directives.as_deref(),
                    sc.directives_has_call,
                    None,
                    &mut None,
                );
                setup.push_str(&format!("{body};"));
            }
        }
        if !el.fold.is_empty() {
            // The fold renders through the SHARED per-effect item renderer (the same
            // memoizer + hoist substrate the regular-element spread fold uses): each
            // `has_call` value hoists into a `$N` arrow param + dependency, and the
            // event-handler stability hoists precede the effect. A SCOPED dynamic
            // element threads its scope-hash literal as the official 6th argument
            // (`build_attribute_effect`); `<svelte:element>` never takes the `<input>`
            // remove-defaults tail (the official `SvelteElement` visitor passes no
            // `should_remove_defaults`).
            let mut hoists = String::new();
            let (body, deps) = self.render_attribute_effect_items(&el.fold, &mut hoists);
            // The setup prelude is single-line (per-statement indentation stripped);
            // normalize the hoist lines the shared renderer indents for the
            // walk-position form.
            for line in hoists.lines() {
                setup.push_str(line.trim_start());
            }
            let call = super::client_spread_html_emit::attribute_effect_call(
                "$$element",
                &body,
                &deps,
                false,
                el.css_hash.as_deref(),
            );
            setup.push_str(&format!("{call};"));
        }
        for bind in &el.binds {
            // The measurement / property binds stay AFTER the class/attribute build (the
            // official `after_update` slot) — only `bind:this` moves before it.
            if !matches!(bind.shape, ClientBindShape::This { .. }) {
                self.emit_bind(&mut setup, node, &bind.shape, &bind.getter, &bind.setter);
            }
        }
        // The legacy `on:` `$.event(...)` registrations — after the attribute build +
        // binds, before the child body.
        for event in &el.events {
            setup.push_str(event);
        }

        // (b) Whether the callback is emitted: a non-empty setup OR a body region that renders
        // something. An empty inner body OMITS the callback (a 3-argument `$.element` call).
        let has_inner = !setup.is_empty() || !self.region_emits_nothing(el.body_region);
        let is_svg = if el.is_svg { "true" } else { "false" };
        if has_inner {
            out.push_str(&format!(
                "\t$.element({anchor_var}, () => {}, {is_svg}, ",
                el.get_tag
            ));
            // `emit_bind` indents its statements with a leading tab + trailing newline; the
            // setup is injected as the callback prelude, so strip the per-statement indentation
            // (the region-callback body is single-line in the normalized topology). The
            // structural comparator is whitespace-insensitive OUTSIDE literals, so the exact
            // indentation is cosmetic — the setup statements are emitted verbatim as the
            // callback prelude.
            self.emit_region_callback_with_prelude(
                out,
                el.body_region,
                &["$$element".to_string(), "$$anchor".to_string()],
                &[],
                &setup,
                CallbackPlacement::InlineArg,
            );
            out.push_str(");\n");
        } else {
            out.push_str(&format!(
                "\t$.element({anchor_var}, () => {}, {is_svg});\n",
                el.get_tag
            ));
        }
    }
}
