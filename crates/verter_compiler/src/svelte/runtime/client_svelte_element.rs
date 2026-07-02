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
use super::client_codegen_helpers::{js_single_quoted, object_key};
use super::client_component_emit::CallbackPlacement;
use super::client_effect::{emit_text_effect, EffectBody, Memoizer};
use super::client_event::render_event_registration;
use super::client_plan::SupportedClientIr;
use super::client_plan_bind::event_wrappers;
use super::client_plan_types::{
    ClientElementBind, ClientNode, ClientRuntimeOp, ClientSvelteElement, ElementFoldItem,
    EventEmit, EventEmitTarget, EventMode,
};
use super::client_shapes::ClientBindShape;
use super::ir::{AttrIr, BindOp, NodeId, SpecialElementIr, StyleDirectiveValue};

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
        // The get-tag thunk BODY: a DYNAMIC `this={…}` rewrites its expression
        // (source-preserving), a STATIC `this="div"` is the single-quoted literal. A
        // `<svelte:element>` with neither is a parse error upstream — fail closed defensively.
        let get_tag = if let Some(expr) = s.this_expr {
            self.rewrite_value_preserving_source(expr)?
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

        let mut fold: Vec<ElementFoldItem> = Vec::new();
        let mut binds: Vec<ClientElementBind> = Vec::new();
        let mut events: Vec<String> = Vec::new();
        // The merged `class:` / `style:` directive entries, appended LAST (after the plain
        // attributes / events, in directive source order) — the official `Element.js`
        // attribute-effect fold ordering.
        let mut class_dirs: Vec<String> = Vec::new();
        let mut style_dirs: Vec<String> = Vec::new();
        // The official `SvelteElement` attribute ROUTE (see [`svelte_element_attr_route`]):
        // the lone-static-class `$.set_class($$element, 0, …)` fast path (WITH any
        // co-located `class:` directives merged into the directive-object argument, via the
        // SHARED regular-element class projection), the `$.attribute_effect` fold, or no
        // attribute emission. On the fast path the class attribute + the `class:`
        // directives are CONSUMED by the pieces (they do not fold).
        let route = svelte_element_attr_route(&s.attrs);
        let set_class = if matches!(route, SvelteElementAttrRoute::SetClass) {
            Some(self.project_set_class_pieces(&s.attrs)?)
        } else {
            None
        };
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
                    let scope = self.ir.analysis.expressions.get(*handler).scope;
                    let handler_body = self.rewrite(*handler, scope)?;
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
                AttrIr::Static { name, value } => {
                    // On the `set_class` fast path the static-text `class` is the pieces'
                    // BASE value, so it is NOT folded here — skip it (the NAME matches
                    // case-insensitively, the official `toLowerCase()` rule). Every other
                    // static attribute (and a non-lone / non-text `class`) folds normally.
                    if set_class.is_some() && name.eq_ignore_ascii_case("class") {
                        continue;
                    }
                    // A valueless boolean attribute folds as `name: true`; a present value as
                    // the entity-decoded single-quoted literal.
                    let v = match value {
                        None => "true".to_string(),
                        Some(v) => {
                            js_single_quoted(&super::entity_decode::decode_attr_entities(&v.value))
                        }
                    };
                    fold.push(ElementFoldItem::Entry(format!("{}: {v}", object_key(name))));
                }
                AttrIr::Dynamic { name, expr } => {
                    let v = self.rewrite_value_preserving_source(*expr)?;
                    // The official `is_event_attribute` rule (`name.startsWith('on')` on an
                    // expression attribute): a `<svelte:element>`-hosted `on*` handler folds
                    // into `$.attribute_effect` as a HOISTED stable local (`var event_handler =
                    // <handler>; … on<event>: event_handler`) — the handler-stability hoist the
                    // re-running effect arrow requires. A non-`on*` attribute is a plain fold
                    // entry. (The `on*`-name check mirrors svelte's compiler rule — it is the
                    // attribute-effect fold's own handler classification, NOT a type-resolver
                    // heuristic.)
                    if name.starts_with("on") {
                        fold.push(ElementFoldItem::Event {
                            prop: name.clone(),
                            handler: v,
                        });
                    } else {
                        fold.push(ElementFoldItem::Entry(format!("{}: {v}", object_key(name))));
                    }
                }
                AttrIr::Mixed { name, parts } => {
                    let (value, _) = self.mixed_attr_value(parts)?;
                    let v = self.fold_attr_value_text(&value);
                    fold.push(ElementFoldItem::Entry(format!("{}: {v}", object_key(name))));
                }
                AttrIr::Spread { expr } => {
                    fold.push(ElementFoldItem::Entry(format!(
                        "...{}",
                        self.rewrite_value_preserving_source(*expr)?
                    )));
                }
                AttrIr::Class { name, condition } => {
                    // On the `set_class` fast path every `class:` directive is merged
                    // into the pieces' directive-object argument — skip the fold entry.
                    if set_class.is_some() {
                        continue;
                    }
                    let key = object_key(name);
                    let entry = match condition {
                        Some(e) => super::client_codegen_helpers::object_property(
                            &key,
                            &self.rewrite_value_preserving_source(*e)?,
                        ),
                        None => key,
                    };
                    class_dirs.push(entry);
                }
                AttrIr::Style {
                    property, value, ..
                } => {
                    let key = object_key(property);
                    let entry = match value {
                        StyleDirectiveValue::Expr(e) => {
                            super::client_codegen_helpers::object_property(
                                &key,
                                &self.rewrite_value_preserving_source(*e)?,
                            )
                        }
                        StyleDirectiveValue::Text(text) => {
                            super::client_codegen_helpers::object_property(
                                &key,
                                &js_single_quoted(text),
                            )
                        }
                        StyleDirectiveValue::Mixed(parts) => {
                            let (mixed, _) = self.mixed_attr_value(parts)?;
                            super::client_codegen_helpers::object_property(
                                &key,
                                &self.fold_attr_value_text(&mixed),
                            )
                        }
                    };
                    style_dirs.push(entry);
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
            }
        }
        // The analyze-phase SYNTHESIZED empty `class` / `style` attributes (official
        // `phases/2-analyze/index.js`): a `class:`/`style:` directive with no matching
        // plain attribute (and no spread) contributes a `class: ''` / `style: ''` fold
        // entry, appended AFTER the real attributes (official pushes the synthetics onto
        // the attribute list) and BEFORE the `[$.CLASS]` / `[$.STYLE]` directive entries.
        if let SvelteElementAttrRoute::Fold {
            synth_class,
            synth_style,
        } = route
        {
            if synth_class {
                fold.push(ElementFoldItem::Entry("class: ''".to_string()));
            }
            if synth_style {
                fold.push(ElementFoldItem::Entry("style: ''".to_string()));
            }
        }
        if !class_dirs.is_empty() {
            fold.push(ElementFoldItem::Entry(format!(
                "[$.CLASS]: {{ {} }}",
                class_dirs.join(", ")
            )));
        }
        if !style_dirs.is_empty() {
            fold.push(ElementFoldItem::Entry(format!(
                "[$.STYLE]: {{ {} }}",
                style_dirs.join(", ")
            )));
        }

        Ok(ClientNode::SvelteElement(ClientSvelteElement {
            get_tag,
            // SVG / MathML host elements are not in the client element allowlist, so a
            // dynamic element is always HTML-hosted on the reachable surface.
            is_svg: false,
            set_class,
            fold,
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
/// - Official synthesizes an empty `class=""` when `class:` directives are present with
///   no `class` attribute and no spread, and an empty `style=""` when `style:`
///   directives are present with no `style` attribute and no spread
///   (`phases/2-analyze/index.js`), appending them AFTER the real attributes.
/// - `attributes.length === 1 && class && is_text_attribute` — counting the synthetics,
///   NOT `on:` / `bind:` / `class:` / `style:` directives — routes to the
///   [`SetClass`](SvelteElementAttrRoute::SetClass) fast path (so a lone static-text
///   `class`, with or without co-located `class:` directives, and a PURE `class:`
///   directive set both take `$.set_class`; a co-located `style:` directive synthesizes
///   the `style` attribute and forces the fold).
/// - Any other non-empty effective attribute set routes to the
///   [`Fold`](SvelteElementAttrRoute::Fold).
///
/// Structural over the typed `AttrIr` — no source scan, no `starts_with("class")`.
/// SHARED by the projection (`project_svelte_element`) and the plan topology
/// (`topology.rs`) so the recorded helper (`$.set_class` vs `$.attribute_effect`) never
/// drifts from the emission.
pub(super) fn svelte_element_attr_route(attrs: &[AttrIr]) -> SvelteElementAttrRoute {
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
    let synth_class = !has_spread && !has_class && has_class_dirs;
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
            let mut entries: Vec<String> = Vec::with_capacity(el.fold.len());
            for item in &el.fold {
                match item {
                    ElementFoldItem::Entry(entry) => entries.push(entry.clone()),
                    ElementFoldItem::Event { prop, handler } => {
                        // The official attribute-effect handler-stability hoist: a stable `var
                        // event_handler = <handler>;` referenced by name in the fold.
                        let name = self.alloc_name("event_handler");
                        setup.push_str(&format!("var {name} = {handler};"));
                        entries.push(format!("{prop}: {name}"));
                    }
                }
            }
            setup.push_str(&format!(
                "$.attribute_effect($$element, () => ({{ {} }}));",
                entries.join(", ")
            ));
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
