//! The narrow-plan PROJECTION of a component invocation (`<Foo …/>` /
//! `<svelte:component>` / `<svelte:self>`) and the `{@render}` tag.
//!
//! Every prop value, event handler, bind getter/setter, spread thunk, and `{@render}`
//! argument is REWRITTEN here through the shared FALLIBLE rewriter (driven by each
//! expression's own recorded scope), so a refusal fails closed BEFORE the plan exists.
//! The slot-content regions are carried by their [`TemplateScopeId`]; the emitter emits
//! their callbacks through the shared region-callback emitter. The member ORDER mirrors
//! the official `Component.js` `build_component`: regular props (incl. `onX={}` callback
//! props) → bind get/set pairs → `$$events` → snippet-def shorthand props → `children` →
//! `$$slots`.

use oxc_allocator::Allocator;

use super::client_codegen_helpers::js_single_quoted;
use super::client_plan::SupportedClientIr;
use super::client_plan_types::{
    ClientComponent, ClientNode, ClientRender, ComponentBindThis, ComponentCallee,
    ComponentFnPairBind, ComponentMember, ComponentProps, ComponentSpreadPart, SlotEntry,
};
use super::ir::{
    AttrIr, ComponentIrNode, ComponentSlots, ExprId, IrNode, NodeId, RenderCallee,
    SpecialElementIr, SpecialKind,
};
use super::unsupported::UnsupportedSvelteRuntimeSurface;
use verter_span::Span;

/// The official per-call `Memoizer` (svelte@5.56.3 client `shared/utils.js`): mints the
/// ordered `let $N = $.derived(() => <expr>);` hoist statements and hands back the
/// `$.get($N)` read for each memoized value. The `$N` numbering restarts per memoizer
/// instance — one per component call (`CallBuild`), one per `<slot>` outlet, and one
/// per `{@render}` tag, exactly the official per-`build_component` / per-`SlotElement`
/// / per-`RenderTag` instances. This is the SINGLE memoize engine for every template
/// value hoist, so the numbering, the derived statement shape, the concise-arrow body
/// wrap, AND the MODE-AWARE helper choice can never diverge between the component-prop,
/// slot-prop, and render-argument surfaces. The helper is the official
/// `Memoizer.deriveds(runes)` rule: `$.derived` in runes mode, `$.derived_safe_equal`
/// in EVERY non-runes mode (definitely-legacy AND maybe-runes alike — the separate
/// legacy VALUE wrap is owned by the sole
/// [`prepare_template_value`](SupportedClientIr::prepare_template_value) entry).
pub(super) struct DerivedMemoizer {
    runes: bool,
    counter: usize,
    statements: Vec<String>,
}

impl DerivedMemoizer {
    /// A memoizer for one call surface, mode-aware: `runes` picks the derived
    /// helper (`$.derived` vs `$.derived_safe_equal`).
    pub(super) fn new(runes: bool) -> Self {
        Self {
            runes,
            counter: 0,
            statements: Vec::new(),
        }
    }

    /// Memoize one rewritten value expression: push its `let $N = <helper>(() => …);`
    /// hoist (the value embedded as a CONCISE ARROW BODY through the shared
    /// [`concise_arrow_expr_body`] wrap, so an object-literal / sequence / legacy-wrap
    /// sequence value stays a valid expression body) and return the `$.get($N)` read.
    ///
    /// [`concise_arrow_expr_body`]: super::client_codegen_helpers::concise_arrow_expr_body
    pub(super) fn memoize(&mut self, value: &str) -> String {
        let n = self.counter;
        self.counter += 1;
        let helper = if self.runes {
            "$.derived"
        } else {
            "$.derived_safe_equal"
        };
        let body = super::client_codegen_helpers::concise_arrow_expr_body(value);
        self.statements
            .push(format!("let ${n} = {helper}(() => {body});"));
        format!("$.get(${n})")
    }

    /// The ordered `let $N = <helper>(…);` hoist statements (the official
    /// `memoizer.deriveds(runes)`), consumed into the wrapping block.
    pub(super) fn into_statements(self) -> Vec<String> {
        self.statements
    }
}

/// The per-component-call projection state: the prop-derived memoizer (the official
/// per-`build_component` `Memoizer`, named `$0`, `$1`, … in order) and the hoisted
/// pre-statements (the deriveds + function-pair bind vars emitted before the call).
struct CallBuild {
    /// Function-pair component binds (`bind:x={get, set}`) — each carries its rewritten
    /// get/set expressions plus a component-function-scoped pair index. The emitter mints the
    /// `var bind_get` / `var bind_set` locals from the index (via the shared allocator) and
    /// emits them at the call's statement level (the official `state.init`), NOT inside the
    /// wrapping block.
    fn_pair_binds: Vec<ComponentFnPairBind>,
    /// The prop-derived memoizer — its `let $N = $.derived(…)` statements are emitted
    /// INSIDE the wrapping block (the official `memoizer.deriveds()`).
    memoizer: DerivedMemoizer,
}

impl<'a> SupportedClientIr<'a> {
    /// Project a static `<Foo …/>` component node into its [`ClientNode::Component`].
    pub(super) fn project_component(
        &self,
        c: &ComponentIrNode,
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        // The static component callee MUST resolve to an admitted `.svelte`-component import
        // (a non-reactive `ComponentImport` binding). A capitalized tag naming no such
        // import is an unsupported component SOURCE — a dynamically-assigned / re-exported /
        // globally-injected component whose callee semantics this vertical does not model —
        // so it fails CLOSED here rather than emitting a call on an unbound global. The
        // `<svelte:self>` recursion (its own compile-name) and the dynamic
        // `<svelte:component this={…}>` selector reach `project_component_call` on separate
        // paths that do NOT route through this static-import gate.
        if !self.static_component_callee_is_import(&c.name, c.scope) {
            return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "component",
                span: c.span,
            });
        }
        let callee = ComponentCallee::Static {
            name: c.name.clone(),
        };
        self.project_component_call(callee, &c.attrs, &c.slots, c.scope, c.span)
    }

    /// Whether a static component tag name RESOLVES to an admitted `.svelte`-component
    /// import (a [`BindingRuntimeKind::ComponentImport`](super::expr::BindingRuntimeKind)
    /// binding). The WHOLE name must be a BARE identifier: a DOTTED name (`Ns.Comp`) is a
    /// namespace/member-component source — an advanced form this vertical does not model — so
    /// it fails CLOSED on the dot even when the head segment IS an admitted import. A default
    /// `.svelte` import is a component FUNCTION, not a namespace object, so `.Comp` is a
    /// likely-undefined member access; emitting `Ns.Comp(…)` on it would be wrong. The
    /// imported local is the SOLE supported static component source in this vertical, so a
    /// callee that resolves to anything else — a `$props()` prop, a plain local, an unbound
    /// global, or any dotted member — fails closed, making the emitted callee a PROVEN
    /// resolved import rather than coincidental bare tag text.
    fn static_component_callee_is_import(&self, name: &str, scope: super::expr::ScopeId) -> bool {
        // A dotted member name (`Ns.Comp`) is never a bare admitted import — fail closed.
        if name.contains('.') {
            return false;
        }
        matches!(
            self.ir
                .analysis
                .bindings
                .resolve_kind(&self.ir.analysis.scopes, scope, name),
            Some(super::expr::BindingRuntimeKind::ComponentImport)
        )
    }

    /// Project a component-INVOCATION special (`<svelte:component>` / `<svelte:self>`) into
    /// its [`ClientNode::Component`]. A `<svelte:fragment>` named slot is ABSORBED into its
    /// parent component's `$$slots` at lowering (never a node here); a STANDALONE
    /// `<svelte:fragment>` is the transparent-wrapper surface, refused upstream.
    pub(super) fn project_special_component(
        &self,
        s: &SpecialElementIr,
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        let callee = match s.kind {
            // `<svelte:self>` recurses through the component's own compile-name.
            SpecialKind::SelfRef => ComponentCallee::Static {
                name: self.ir.component.name.clone(),
            },
            // `<svelte:component this={expr}>` is a DYNAMIC component — the `this`
            // expression drives `$.component(node, () => <this>, …)`.
            SpecialKind::Component => {
                let this_expr = match s.this_expr {
                    // A RAW semantic role — routed through the policy entry point.
                    Some(expr) => self
                        .prepare_template_value(
                            super::client_legacy_value::AuthoredExpr(expr),
                            super::client_legacy_value::AuthoredValueSurface::ComponentSelector,
                        )?
                        .inline_expression(),
                    // A `<svelte:component>` with no `this` is a parse error upstream;
                    // defensively project the `undefined` selector.
                    None => "undefined".to_string(),
                };
                ComponentCallee::Dynamic { this_expr }
            }
            _ => {
                return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "special",
                    span: s.span,
                })
            }
        };
        self.project_component_call(callee, &s.attrs, &s.slots, s.scope, s.span)
    }

    /// The shared component-call projection — used by both the static-component and the
    /// component-family-special paths.
    fn project_component_call(
        &self,
        callee: ComponentCallee,
        attrs: &[AttrIr],
        slots: &ComponentSlots,
        scope: super::expr::ScopeId,
        span: Span,
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        // A `let:` slot-prop directive (on the component OR a named-slot child) using an
        // UNSUPPORTED form — a destructuring / non-identifier alias (`let:item={{a, b}}`) or a
        // quoted-text value — fails CLOSED here. The let decomposition is infallible at
        // lowering, so it carries `has_unsupported_let` and this fallible projection gate
        // enforces it (never a silent drop).
        if slots.has_unsupported_let {
            return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "let-directive",
                span,
            });
        }
        // Two children carrying the SAME static `slot` name is the official
        // `slot_attribute_duplicate` compile error — fail closed, never emit the merged
        // region official refuses.
        if slots.has_duplicate_slot {
            return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "duplicate slot name",
                span,
            });
        }
        // An explicit `slot="default"` child alongside ANY non-exempt sibling fragment
        // node is the official `slot_default_duplicate` compile error — the per-node
        // walk exempts EXACTLY a whitespace-only text run or a regular element /
        // `<svelte:fragment>` carrying a `slot` attribute; a comment, a `{#snippet}`
        // def, an interpolation, a block, and a component-family node (including the
        // `slot="default"`-bearing child ITSELF) all conflict — fail closed.
        if slots.has_default_slot_conflict {
            return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "default slot conflict",
                span,
            });
        }

        let mut build = CallBuild {
            fn_pair_binds: Vec::new(),
            memoizer: DerivedMemoizer::new(self.ir.component.mode == super::ir::SvelteMode::Runes),
        };

        // The props are built as the official `props_and_spreads`: object groups + spread
        // thunks in SOURCE ORDER. A pure-object component flushes ONE group.
        let mut spread_parts: Vec<ComponentSpreadPart> = Vec::new();
        let mut current_group: Vec<ComponentMember> = Vec::new();
        let mut has_spread = false;
        let mut delayed_binds: Vec<ComponentMember> = Vec::new();
        let mut events: Vec<(String, String)> = Vec::new();
        let mut bind_this: Option<ComponentBindThis> = None;

        for attr in attrs {
            match attr {
                // A regular prop attribute — a static literal, a reactive value, or a
                // callback prop (`onfoo={…}`). `let:` is handled via the slots.
                AttrIr::Static { .. } | AttrIr::Dynamic { .. } | AttrIr::Mixed { .. } => {
                    let member = self.project_prop_member(attr, scope, &mut build)?;
                    current_group.push(member);
                }
                // A spread switches the props payload to `$.spread_props(…)`: flush the
                // current object group, then push the spread thunk.
                AttrIr::Spread { expr } => {
                    has_spread = true;
                    if !current_group.is_empty() {
                        spread_parts.push(ComponentSpreadPart::Group(std::mem::take(
                            &mut current_group,
                        )));
                    }
                    let arg = self.project_spread_arg(*expr, &mut build)?;
                    spread_parts.push(ComponentSpreadPart::Spread { arg });
                }
                AttrIr::Bind { target, expr } => {
                    // The component bind's lvalue ROOT must be a SHARED-policy writable target
                    // (a `$state` signal / proxy / plain local). A `$props()` prop, an import,
                    // or an unbound / free root has no correct component setter in this
                    // vertical, so it fails CLOSED here — consuming the shared writable-root
                    // classification instead of synthesizing a setter from a non-writable raw
                    // source. A function-pair `{get, set}` target is exempt (the user owns the
                    // get/set).
                    if !self.component_bind_root_is_writable(target, *expr, scope) {
                        return Err(UnsupportedSvelteRuntimeSurface::Binding {
                            target: target.clone(),
                            span,
                        });
                    }
                    if target == "this" {
                        bind_this = Some(self.project_bind_this(*expr, scope)?);
                    } else {
                        delayed_binds
                            .push(self.project_bind_prop(target, *expr, scope, &mut build)?);
                    }
                }
                // A legacy `on:` directive forwards as `$$events`.
                AttrIr::Event {
                    event_type,
                    handler,
                    ..
                } => {
                    // A RAW semantic role — routed through the policy entry point.
                    let body = self
                        .prepare_template_value(
                            super::client_legacy_value::AuthoredExpr(*handler),
                            super::client_legacy_value::AuthoredValueSurface::EventHandler,
                        )?
                        .inline_expression();
                    events.push((event_type.clone(), body));
                }
                // A `let:` slot-prop directive is CONSUMED by the slot decomposition at
                // lowering (the component's own lets become `default_lets`); an UNSUPPORTED
                // let form already failed closed via the `has_unsupported_let` gate above, so
                // a supported `let:` is a no-op here.
                AttrIr::Let { .. } => {}
                // A `class:` / `style:` / `use:` / `transition:` / `animate:` directive on
                // a COMPONENT is invalid Svelte the official compiler rejects
                // (`component_invalid_directive` — a component is not a DOM element host),
                // and a component `{@attach}` — which official ACCEPTS as the computed-key
                // `[$.attachment()]` prop — is the DEFERRED component-attachment forwarding
                // (ledger D-38). Both fail CLOSED — there is NO upstream classifier gate for
                // these (the classifier delegates component-attr validation to this
                // projection).
                AttrIr::Class { .. }
                | AttrIr::Style { .. }
                | AttrIr::Use { .. }
                | AttrIr::Transition { .. }
                | AttrIr::Animate { .. }
                | AttrIr::Attach { .. } => {
                    return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                        construct: "directive",
                        span,
                    });
                }
            }
        }

        // Append (in official order) the delayed binds, the `$$events`, the snippet-def
        // shorthand props, and the `children` / `$$slots` slot members onto the LAST
        // object group.
        let mut trailing: Vec<ComponentMember> = Vec::new();
        trailing.append(&mut delayed_binds);
        if !events.is_empty() {
            // The keys are emitted STRUCTURALLY (not a pre-joined string), so each routes
            // through `object_key` at emit time (a hyphenated `on:foo-bar` quotes to
            // `'foo-bar'`).
            trailing.push(ComponentMember::Events { entries: events });
        }
        self.append_slot_members(slots, &mut trailing);

        // Assemble the props payload.
        current_group.extend(trailing);
        let props = if has_spread {
            if !current_group.is_empty() {
                spread_parts.push(ComponentSpreadPart::Group(current_group));
            }
            ComponentProps::Spread(spread_parts)
        } else {
            ComponentProps::Object(current_group)
        };

        Ok(ClientNode::Component(ClientComponent {
            callee,
            span,
            fn_pair_binds: build.fn_pair_binds,
            block_statements: build.memoizer.into_statements(),
            props,
            snippet_defs: slots.snippet_defs.clone(),
            bind_this,
        }))
    }

    /// Project a regular prop attribute into its [`ComponentMember`] — the official
    /// `build_attribute_value` + getter/init decision: a STATIC literal → `key: 'v'`; a
    /// dynamic value → a getter (reactive) or init (non-reactive), with a COMPOUND
    /// reactive value memoized into a `$N` derived (the official `should_wrap_in_derived`).
    fn project_prop_member(
        &self,
        attr: &AttrIr,
        scope: super::expr::ScopeId,
        build: &mut CallBuild,
    ) -> Result<ComponentMember, UnsupportedSvelteRuntimeSurface> {
        match attr {
            // A STATIC prop value carries the producer-DECODED semantic text
            // (the attribute-IR boundary owns the decode), matching official's
            // parse-time decoded `Text.data`: a retained `slot="foo&amp;bar"`
            // prop emits `slot: 'foo&bar'`, identical to its `$$slots` key, and
            // every other static prop (`title="foo&amp;bar"`) reads the same
            // decoded value (via `as_str`, never a second decode). The Mixed arm
            // needs no decode here — its literal parts were decoded at lowering;
            // a Dynamic value is a JS expression (no decode applies).
            AttrIr::Static { name, value } => Ok(ComponentMember::Init {
                key: name.clone(),
                value: match value {
                    Some(v) => js_single_quoted(v.value.as_str()),
                    None => "true".to_string(),
                },
            }),
            AttrIr::Dynamic { name, expr } => self.project_dynamic_prop(name, *expr, scope, build),
            AttrIr::Mixed { name, .. } => {
                // A mixed component prop (`label="a {b}"`) is a string-concatenation value.
                // Route through the shared mixed-value path; each expression chunk
                // legacy-wraps and memoizes through the SHARED per-call memoizer
                // (the official `build_attribute_value` memoize callback — a
                // `has_call` chunk reads `$.get($N)`).
                let (value, has_state) = self.mixed_attr_value(
                    match attr {
                        AttrIr::Mixed { parts, .. } => parts,
                        _ => unreachable!(),
                    },
                    super::client_legacy_value::AuthoredValueSurface::ComponentProp,
                )?;
                let rendered = self.render_memoized_attr_value(&value, &mut build.memoizer);
                if has_state {
                    Ok(ComponentMember::Getter {
                        key: name.clone(),
                        body: rendered,
                    })
                } else {
                    Ok(ComponentMember::Init {
                        key: name.clone(),
                        value: rendered,
                    })
                }
            }
            _ => unreachable!("project_prop_member is only called for static/dynamic/mixed"),
        }
    }

    /// Project a `Dynamic` prop value (`name={expr}` / shorthand `{name}`).
    fn project_dynamic_prop(
        &self,
        name: &str,
        expr: ExprId,
        scope: super::expr::ScopeId,
        build: &mut CallBuild,
    ) -> Result<ComponentMember, UnsupportedSvelteRuntimeSurface> {
        let analyzed = self.ir.analysis.expressions.get(expr);
        let has_state = super::reactive_analysis::prop_value_has_state(
            &analyzed.references,
            analyzed.source,
            scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
        )
        .map_err(|()| {
            UnsupportedSvelteRuntimeSurface::expression_fact_recovery("binding-impurity")
        })?;
        // The sole authored-value preparation (the official `build_expression`
        // runs BEFORE the memoize decision — a memoized value memoizes the
        // WRAPPED sequence; a non-memoized wrapped value embeds parenthesized).
        let prepared = self.prepare_template_value(
            super::client_legacy_value::AuthoredExpr(expr),
            super::client_legacy_value::AuthoredValueSurface::ComponentProp,
        )?;
        // The official `should_wrap_in_derived`: the chunk expression is NOT a simple
        // `Identifier` / `MemberExpression` (a compound expression that may over-fire)
        // — read from the POPULATED unwrapped-root-kind fact of the canonical parse
        // (no reparse).
        let should_wrap = !matches!(
            analyzed.unwrapped_root_kind,
            super::expr::UnwrappedRootKind::Identifier
                | super::expr::UnwrappedRootKind::MemberExpression
        );
        let memoize = prepared.has_call() || (should_wrap && has_state);
        let final_value = if memoize {
            build.memoizer.memoize(prepared.memo_input())
        } else {
            prepared.inline_expression()
        };
        if has_state {
            Ok(ComponentMember::Getter {
                key: name.to_string(),
                body: final_value,
            })
        } else {
            Ok(ComponentMember::Init {
                key: name.to_string(),
                value: final_value,
            })
        }
    }

    /// Render a structured [`AttrValue`](super::client_plan_types::AttrValue) for a
    /// component / `<slot>` prop through the SHARED per-call memoizer — the official
    /// `build_attribute_value` / `build_template_chunk` with the memoize callback:
    /// a const verbatim; a single expression memoized when `has_call`; a mixed
    /// template literal whose each expression chunk memoizes when `has_call` (the
    /// chunk reads `$.get($N)`) and `?? ''`-coerces per its recorded
    /// [`NullishCoalesce`] rule. The legacy value wrap was PREPARED on the carrier
    /// at planning time (wrap first, then the memoize decision) — this renderer only
    /// serializes it: a memoized value memoizes the wrapped sequence; a non-memoized
    /// wrapped value embeds parenthesized.
    ///
    /// [`NullishCoalesce`]: super::reactive_fold::NullishCoalesce
    pub(super) fn render_memoized_attr_value(
        &self,
        value: &super::client_plan_types::AttrValue,
        memoizer: &mut DerivedMemoizer,
    ) -> String {
        use super::client_plan_types::{AttrValue, AttrValuePart, PlannedTemplateValue};
        match value {
            AttrValue::Const(c) => c.clone(),
            AttrValue::Single(PlannedTemplateValue::Authored(p)) => {
                if p.has_call() {
                    memoizer.memoize(p.memo_input())
                } else {
                    p.inline_expression()
                }
            }
            AttrValue::Single(PlannedTemplateValue::Synthesized(s)) => {
                if s.has_call() {
                    memoizer.memoize(s.raw_text())
                } else {
                    s.raw_text().to_string()
                }
            }
            AttrValue::Mixed(parts) => {
                let mut s = String::from("`");
                for part in parts {
                    match part {
                        AttrValuePart::Literal(t) => s.push_str(t),
                        AttrValuePart::Expr { value, .. } => {
                            let read = if value.has_call() {
                                memoizer.memoize(value.memo_input())
                            } else {
                                value.inline_expression()
                            };
                            s.push_str(&format!("${{{read} ?? ''}}"));
                        }
                    }
                }
                s.push('`');
                s
            }
        }
    }

    /// Project the spread argument of a `{...rest}` attribute — the official
    /// component-spread rule: a `has_call` spread MEMOIZES through the shared
    /// per-call memoizer (`() => $.get($N)`; mode-aware helper, NEVER
    /// legacy-wrapped — official visits a `SpreadAttribute` without
    /// `build_expression`), and the thunk routes through the shared
    /// [`js_thunk`](super::client_codegen_helpers::js_thunk) so a bare zero-arg
    /// accessor read unthunks (`{...rest}` over a legacy prop → `rest`).
    fn project_spread_arg(
        &self,
        expr: ExprId,
        build: &mut CallBuild,
    ) -> Result<String, UnsupportedSvelteRuntimeSurface> {
        let prepared = self.prepare_template_value(
            super::client_legacy_value::AuthoredExpr(expr),
            super::client_legacy_value::AuthoredValueSurface::ComponentSpreadOperand,
        )?;
        if prepared.has_call() {
            let read = build.memoizer.memoize(prepared.memo_input());
            Ok(format!("() => {read}"))
        } else {
            Ok(prepared.thunk())
        }
    }

    /// Whether a component bind's LVALUE root is a SUPPORTED writable target under the
    /// SHARED writable-root policy (a `$state` signal / `$.state($.proxy)` / plain local).
    /// The root is the typed bind-target fact's `root_ident` for an explicit `bind:x={root}`,
    /// or the bind NAME for a shorthand `bind:x` (which binds a same-named local). A
    /// function-pair `{get, set}` target has NO lvalue root (the user owns the get/set), so it
    /// is validated as supported. A bind to a `$props()` prop, an import, an unbound/free
    /// target, or a non-lvalue expression fails closed — the component setter is then NEVER
    /// synthesized from a non-writable root.
    fn component_bind_root_is_writable(
        &self,
        target: &str,
        expr: Option<ExprId>,
        scope: super::expr::ScopeId,
    ) -> bool {
        let bindings = &self.ir.analysis.bindings;
        let scopes = &self.ir.analysis.scopes;
        if let Some(e) = expr {
            let fact = &self.ir.analysis.expressions.get(e).bind_target;
            // A function-pair `{get, set}` target has no lvalue root to validate.
            if matches!(fact.kind, Some(super::expr::BindTargetKind::FunctionPair)) {
                return true;
            }
            return match &fact.root_ident {
                Some(root) => super::client_shapes::bind_root_is_writable_target(
                    bindings, scopes, scope, root,
                ),
                // A non-lvalue, non-pair target (a call / literal / member bottoming at a
                // non-identifier) has no writable root.
                None => false,
            };
        }
        // A shorthand `bind:x` binds a same-named local.
        super::client_shapes::bind_root_is_writable_target(bindings, scopes, scope, target)
    }

    /// Project a `bind:this={ref}` into its (setter, getter) bodies wrapping the call in
    /// `$.bind_this(<call>, <setter>, <getter>)`.
    fn project_bind_this(
        &self,
        expr: Option<ExprId>,
        scope: super::expr::ScopeId,
    ) -> Result<ComponentBindThis, UnsupportedSvelteRuntimeSurface> {
        // A function-pair `bind:this={get, set}` — the official `build_bind_this` reads
        // `[getter, setter] = expression.expressions` and emits `$.bind_this(call, set,
        // get)`. The two elements are already arrow functions, so they are rewritten + used
        // verbatim (no `() =>` / `($$value) =>` synthesis).
        if let Some(e) = expr {
            let analyzed = self.ir.analysis.expressions.get(e);
            if let Some((get_src, set_src)) = &analyzed.bind_target.function_pair {
                return Ok(ComponentBindThis {
                    getter: self.rewrite_source_plain_js(get_src, scope)?,
                    setter: self.rewrite_source_plain_js(set_src, scope)?,
                });
            }
        }
        // A simple `bind:this={ref}` — `$.bind_this(call, ($$value) => ref = $$value, () =>
        // ref)` (`build_bind_this` synthesises the get/set arrows from the lvalue).
        let bound = match expr {
            Some(e) => self.ir.analysis.expressions.get(e).source.to_string(),
            None => "this".to_string(),
        };
        let getter = format!("() => {}", self.rewrite_source(&bound, scope)?);
        let setter = format!(
            "($$value) => {}",
            self.rewrite_source(&format!("{bound} = $$value"), scope)?
        );
        Ok(ComponentBindThis { setter, getter })
    }

    /// Project a component `bind:prop` (or function-pair bind) into its `GetSet` member.
    /// A simple `bind:value={v}` → `get value() { return $.get(v); } set value($$value) {
    /// $.set(v, $$value, true); }`; a function-pair `bind:value={get, set}` hoists `var
    /// bind_get`/`var bind_set` and the member calls them.
    fn project_bind_prop(
        &self,
        target: &str,
        expr: Option<ExprId>,
        scope: super::expr::ScopeId,
        build: &mut CallBuild,
    ) -> Result<ComponentMember, UnsupportedSvelteRuntimeSurface> {
        // A function-pair bind (`bind:value={get, set}`) — the analyzed bind target fact
        // carries the two element source slices.
        if let Some(e) = expr {
            let analyzed = self.ir.analysis.expressions.get(e);
            if let Some((get_src, set_src)) = &analyzed.bind_target.function_pair {
                let get_expr = self.rewrite_source_plain_js(get_src, scope)?;
                let set_expr = self.rewrite_source_plain_js(set_src, scope)?;
                // Record the pair with a component-function-scoped INDEX (assigned in source
                // order across every component call). The emitter mints the `var bind_get` /
                // `var bind_set` locals from this index through the shared scope-aware
                // allocator, so MULTIPLE function-pair binds never alias the same `var` AND the
                // names never collide with a user binding — the official per-component-function
                // `state.scope.generate('bind_get')` uniquing. The member links back to the
                // minted names through the same index.
                let index = self.fn_pair_bind_seq.get();
                self.fn_pair_bind_seq.set(index + 1);
                build.fn_pair_binds.push(ComponentFnPairBind {
                    index,
                    get_expr,
                    set_expr,
                });
                return Ok(ComponentMember::FnPairGetSet {
                    key: target.to_string(),
                    index,
                });
            }
        }
        // A simple `bind:value` / `bind:value={v}` — the bound source is the expr's source
        // (or the shorthand target name).
        let bound = match expr {
            Some(e) => self.ir.analysis.expressions.get(e).source.to_string(),
            None => target.to_string(),
        };
        let get_body = self.rewrite_source(&bound, scope)?;
        let set_body = self.rewrite_source(&format!("{bound} = $$value"), scope)?;
        Ok(ComponentMember::GetSet {
            key: target.to_string(),
            get_body,
            set_body,
        })
    }

    /// Append the snippet-def shorthand props + the `children` / `$$slots` slot members
    /// onto `out`, in official order (snippet props → children → `$$slots`).
    fn append_slot_members(&self, slots: &ComponentSlots, out: &mut Vec<ComponentMember>) {
        // (a) The `{#snippet}`-as-child shorthand props.
        let mut slot_entries: Vec<SlotEntry> = Vec::new();
        for &snippet_node in &slots.snippet_defs {
            if let Some(name) = self.snippet_name(snippet_node) {
                out.push(ComponentMember::SnippetProp { name: name.clone() });
                slot_entries.push(SlotEntry::TrueMarker { name });
            }
        }

        // (b) The default slot → `children` (no `let:`) or `$$slots.default` callback
        // (with `let:`) + the `children: $.invalid_default_snippet` sentinel.
        if let Some(default) = slots.default {
            if slots.default_lets.is_empty() {
                out.push(ComponentMember::DefaultChildren { region: default });
                slot_entries.push(SlotEntry::TrueMarker {
                    name: "default".to_string(),
                });
            } else {
                // The default-slot callback carries the COMPONENT's own `let:` slot props,
                // PLANNED here so the emitter never rescans the IR for them.
                slot_entries.push(SlotEntry::Callback {
                    name: "default".to_string(),
                    region: default,
                    lets: slots.default_lets.clone(),
                });
                out.push(ComponentMember::InvalidDefaultSnippet);
            }
        }

        // (c) The named slots — each carrying its OWN `let:` slot props (the plan-time fact),
        // so the emitter consumes them directly instead of rescanning.
        for named in &slots.named {
            slot_entries.push(SlotEntry::Callback {
                name: named.name.clone(),
                region: named.region,
                lets: named.lets.clone(),
            });
        }

        // The default-slot `children`/`InvalidDefaultSnippet` member must precede the
        // `$$slots` object (official order). Re-order: snippet props + children are
        // already pushed; append the `$$slots` object LAST.
        if !slot_entries.is_empty() {
            out.push(ComponentMember::Slots {
                entries: slot_entries,
            });
        }
    }

    /// Partition the TOP-LEVEL `{#snippet}` defs (the root-fragment direct children) into
    /// the MODULE-hoistable set (capture only their params) and the INSTANCE-scope set
    /// (capture component state / props), in source order — the official `can_hoist`
    /// placement (`module_level_snippets` vs `instance_level_snippets`). A component-nested
    /// snippet is NOT top-level (it rides its component's wrapping block).
    pub(super) fn collect_top_level_snippets(&self) -> (Vec<NodeId>, Vec<NodeId>) {
        let mut module = Vec::new();
        let mut instance = Vec::new();
        for &root in &self.ir.root_scope().roots {
            if let IrNode::Block(super::ir::BlockIr::Snippet { body, .. }) = self.ir.node(root) {
                if self.snippet_can_hoist(*body) {
                    module.push(root);
                } else {
                    instance.push(root);
                }
            }
        }
        (module, instance)
    }

    /// Whether a `{#snippet}` body CAN hoist to module scope — it reads NO outer reactive
    /// binding (a signal / prop / each / await item); a read of the snippet's OWN params
    /// (or a global) is fine. (A capture of an outer PLAIN local is not yet handled here; the
    /// official `can_hoist` is the full closure analysis.)
    fn snippet_can_hoist(&self, body: super::ir::TemplateScopeId) -> bool {
        let body_scope = self.ir.template_scope(body).scope;
        for expr in self.ir.analysis.expressions.all() {
            if self.scope_within(expr.scope, body_scope)
                && super::reactive_analysis::expr_references_signal(
                    &expr.references,
                    expr.scope,
                    &self.ir.analysis.bindings,
                    &self.ir.analysis.scopes,
                )
            {
                return false;
            }
        }
        true
    }

    /// Whether `scope` is `ancestor` or a descendant of it (walking the scope parent chain).
    fn scope_within(&self, scope: super::expr::ScopeId, ancestor: super::expr::ScopeId) -> bool {
        let mut cur = Some(scope);
        while let Some(s) = cur {
            if s == ancestor {
                return true;
            }
            cur = self.ir.analysis.scopes.parent(s);
        }
        false
    }

    /// The snippet NAME of a `{#snippet name}` def node (its binding name).
    fn snippet_name(&self, node: NodeId) -> Option<String> {
        if let IrNode::Block(super::ir::BlockIr::Snippet { name, .. }) = self.ir.node(node) {
            return Some(self.ir.analysis.bindings.get(*name).name.clone());
        }
        None
    }

    /// Project a `{@render}` tag into its [`ClientNode::Render`].
    ///
    /// Each argument is thunked (`() => <expr>`); a `has_call`-bearing argument (a call,
    /// a spread — the official `SpreadElement` analysis counts a spread as a call) is
    /// MEMOIZED through the SHARED [`DerivedMemoizer`] (the official per-`RenderTag`
    /// `Memoizer` with `memoize_if_state = false`: `has_state` alone never memoizes a
    /// render argument): the hoisted `let $N = $.derived(() => …);` statements ride the
    /// node's `memo_hoists` and the thunk reads `$.get($N)`.
    pub(super) fn project_render(
        &self,
        callee: &RenderCallee,
        args: &[ExprId],
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        let mut memoizer =
            DerivedMemoizer::new(self.ir.component.mode == super::ir::SvelteMode::Runes);
        let arg_thunks = args
            .iter()
            .map(|&a| {
                // The official per-arg `build_expression` + memoize: the legacy
                // wrap applies to the ARG's own metadata, the memoized read is
                // `$.get($N)`, and the thunk routes through the shared
                // `b.thunk` zero-arg unthunk.
                let prepared = self.prepare_template_value(
                    super::client_legacy_value::AuthoredExpr(a),
                    super::client_legacy_value::AuthoredValueSurface::RenderArg,
                )?;
                Ok(if prepared.has_call() {
                    let read = memoizer.memoize(prepared.memo_input());
                    format!("() => {read}")
                } else {
                    prepared.thunk()
                })
            })
            .collect::<Result<Vec<_>, UnsupportedSvelteRuntimeSurface>>()?;
        let memo_hoists = memoizer.into_statements();
        match callee {
            // A static snippet-name call (`{@render pair(1, 2)}`) — a DIRECT call
            // `pair(node, () => 1, () => 2)`; the optional `{@render pair?.(1)}` form
            // emits the direct OPTIONAL call `pair?.(node, () => 1)` (the official
            // `b.maybe_call`).
            RenderCallee::Snippet { binding, optional } => {
                let name = self.ir.analysis.bindings.get(*binding).name.clone();
                Ok(ClientNode::Render(ClientRender {
                    dynamic: false,
                    callee: name,
                    maybe_call: *optional,
                    memo_hoists,
                    args: arg_thunks,
                }))
            }
            // A dynamic render (`{@render children?.()}` / `{@render (cond?a:b)()}`) —
            // `$.snippet(node, () => <fn>[ ?? $.noop], …args)`. The `?? $.noop` is added
            // only for the optional `?.()` (ChainExpression) form.
            RenderCallee::Dynamic(expr) => {
                let fn_body = self.render_dynamic_callee(*expr)?;
                Ok(ClientNode::Render(ClientRender {
                    dynamic: true,
                    callee: fn_body,
                    maybe_call: false,
                    memo_hoists,
                    args: arg_thunks,
                }))
            }
        }
    }

    /// Lower a dynamic `{@render}` callee expression into the `$.snippet` snippet-fn
    /// thunk body — consuming the CANONICAL-analysis dynamic-callee facts (the
    /// populated span/shape/reference facts on `AnalyzedExpr.render_dynamic_callee`):
    /// slice the callee by the populated span, rewrite it, and append `?? $.noop` for
    /// the optional `?.()` (ChainExpression) form. No reparse, no re-collected
    /// references, no fail-open reference fallback — a missing fact (a non-call
    /// dynamic expression) fails closed with the render refusal.
    fn render_dynamic_callee(
        &self,
        expr: ExprId,
    ) -> Result<String, UnsupportedSvelteRuntimeSurface> {
        let analyzed = self.ir.analysis.expressions.get(expr);
        let Some(facts) = analyzed.render_dynamic_callee.as_ref() else {
            return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "render",
                span: Span::new(0, 0),
            });
        };
        // The callee SOURCE slice (the part before the trailing `()`), sliced by
        // the canonical parse's populated INNER-TEXT-relative span. A boundary
        // failure (impossible for a span produced by the same parse) fails
        // CLOSED with the render refusal — never a whole-source fallback.
        let callee_src = analyzed
            .source
            .get(facts.span.0 as usize..facts.span.1 as usize)
            .ok_or(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "render",
                span: Span::new(0, 0),
            })?;
        let rewritten = self.rewrite_source(callee_src, analyzed.scope)?;
        // The official `build_expression(callee, …)` legacy wrap over the PEELED
        // callee slice — routed through the SOLE preparation entry (the
        // `RenderCalleeSlice` arm of the closed `AuthoredValueInput` vocabulary)
        // under the `RenderCallee` surface policy: the trigger/reference facts
        // ride the populated callee facts (its references are a subset of the
        // whole render expression's, collected over the callee subtree so the
        // outer snippet CALL never mis-triggers the wrap). The chain fallback
        // (`?? $.noop`) applies OUTSIDE the wrapped sequence, exactly as official
        // composes `b.logical('??', snippet_function, $.noop)` after the wrap.
        let body = self
            .prepare_template_value(
                super::client_legacy_value::AuthoredValueInput::RenderCalleeSlice {
                    source: callee_src,
                    scope: analyzed.scope,
                    facts,
                    rewritten: &rewritten,
                },
                super::client_legacy_value::AuthoredValueSurface::RenderCallee,
            )?
            .inline_expression();
        Ok(if facts.is_chain {
            format!("{body} ?? $.noop")
        } else {
            body
        })
    }

    /// Whether the component needs a component context (`$.push`/`$.pop`) — the official
    /// `needs_context` analysis over the instance script + every template expression.
    pub(super) fn needs_context(&self, alloc: &Allocator) -> bool {
        // A `{@render}` DYNAMIC callee (`children?.()` / `$host().snip()`) is a SNIPPET
        // invocation: the OUTER call is never an "unsafe call" trigger (official
        // excludes the render call itself — a prop-rooted `children?.()` stays
        // frame-free), but the CALLEE expression inside it scans NORMALLY — a
        // member/call/`new`-rooted callee (`$host().snip`, `imported.snip`,
        // `(new Date())`) opens the frame exactly as it would in a handler. Those
        // expressions route through the peel-aware render-callee scan; a render
        // ARGUMENT is its own analyzed expression and rides the normal template
        // scan (it may call a context-using import).
        let render_callee_exprs: rustc_hash::FxHashSet<ExprId> = self
            .ir
            .nodes
            .iter()
            .filter_map(|n| match n {
                IrNode::Tag(super::ir::TagIr::Render {
                    callee: RenderCallee::Dynamic(expr),
                    ..
                }) => Some(*expr),
                _ => None,
            })
            .collect();
        let mut template_expr_sources: Vec<&str> = Vec::new();
        let mut render_callee_sources: Vec<&str> = Vec::new();
        for (idx, e) in self.ir.analysis.expressions.all().iter().enumerate() {
            if render_callee_exprs.contains(&ExprId(idx as u32)) {
                render_callee_sources.push(e.source);
            } else {
                template_expr_sources.push(e.source);
            }
        }
        super::needs_context::needs_context(
            alloc,
            self.ir.analysis.scripts.instance_source,
            self.ir.analysis.scripts.module_source,
            &template_expr_sources,
            &render_callee_sources,
            &self.ir.analysis.script_imports,
        )
    }
}
