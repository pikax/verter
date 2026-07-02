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
use oxc_span::GetSpan;

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

/// The per-component-call projection state: the prop-derived `$N` counter (the official
/// per-`build_component` `Memoizer`, named `$0`, `$1`, … in order) and the hoisted
/// pre-statements (the deriveds + function-pair bind vars emitted before the call).
struct CallBuild {
    /// Function-pair component binds (`bind:x={get, set}`) — each carries its rewritten
    /// get/set expressions plus a component-function-scoped pair index. The emitter mints the
    /// `var bind_get` / `var bind_set` locals from the index (via the shared allocator) and
    /// emits them at the call's statement level (the official `state.init`), NOT inside the
    /// wrapping block.
    fn_pair_binds: Vec<ComponentFnPairBind>,
    /// Prop deriveds (`let $N = $.derived(…)`) — emitted INSIDE the wrapping block (the
    /// official `memoizer.deriveds()`).
    block_statements: Vec<String>,
    derived_counter: usize,
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
                    Some(expr) => self.rewrite_value_preserving_source(expr)?,
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

        let mut build = CallBuild {
            fn_pair_binds: Vec::new(),
            block_statements: Vec::new(),
            derived_counter: 0,
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
                    let arg = self.project_spread_arg(*expr)?;
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
                    let body = self.rewrite_value_preserving_source(*handler)?;
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
            block_statements: build.block_statements,
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
            AttrIr::Static { name, value } => Ok(ComponentMember::Init {
                key: name.clone(),
                value: match value {
                    Some(v) => js_single_quoted(&v.value),
                    None => "true".to_string(),
                },
            }),
            AttrIr::Dynamic { name, expr } => self.project_dynamic_prop(name, *expr, scope, build),
            AttrIr::Mixed { name, .. } => {
                // A mixed component prop (`label="a {b}"`) is a string-concatenation value.
                // Route through the shared mixed-value path.
                let (value, has_state) = self.mixed_attr_value(match attr {
                    AttrIr::Mixed { parts, .. } => parts,
                    _ => unreachable!(),
                })?;
                let rendered = self.render_attr_value(&value);
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
        let value = self.rewrite_value_preserving_source(expr)?;
        let has_state = super::reactive_analysis::prop_value_has_state(
            analyzed.source,
            scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
        );
        let has_call = self.expr_has_call(expr);
        // The official `should_wrap_in_derived`: the chunk expression is NOT a simple
        // `Identifier` / `MemberExpression` (a compound expression that may over-fire).
        let should_wrap = !expr_is_simple_ref(analyzed.source);
        let memoize = has_call || (should_wrap && has_state);
        let final_value = if memoize {
            let n = build.derived_counter;
            build.derived_counter += 1;
            build
                .block_statements
                .push(format!("let ${n} = $.derived(() => {value});"));
            format!("$.get(${n})")
        } else {
            value
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

    /// Render an [`AttrValue`](super::client_plan_types::AttrValue) to its emitted form
    /// for a non-memoized (init/getter) component prop — a const verbatim, a single
    /// expression bare, or a mixed template literal.
    fn render_attr_value(&self, value: &super::client_plan_types::AttrValue) -> String {
        use super::client_plan_types::{AttrValue, AttrValuePart};
        match value {
            AttrValue::Const(c) => c.clone(),
            AttrValue::Single { rewritten, .. } => rewritten.clone(),
            AttrValue::Mixed(parts) => {
                let mut s = String::from("`");
                for part in parts {
                    match part {
                        AttrValuePart::Literal(t) => s.push_str(t),
                        AttrValuePart::Expr { rewritten, .. } => {
                            s.push_str(&format!("${{{rewritten} ?? ''}}"));
                        }
                    }
                }
                s.push('`');
                s
            }
        }
    }

    /// Project the spread argument of a `{...rest}` attribute — `() => <rewritten>` (the
    /// official `b.thunk` of the spread expression; a stateful spread is always thunked).
    fn project_spread_arg(&self, expr: ExprId) -> Result<String, UnsupportedSvelteRuntimeSurface> {
        let rewritten = self.rewrite_value_preserving_source(expr)?;
        Ok(format!("() => {rewritten}"))
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
                    expr.source,
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
    pub(super) fn project_render(
        &self,
        callee: &RenderCallee,
        args: &[ExprId],
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        let arg_thunks = args
            .iter()
            .map(|&a| {
                Ok(format!(
                    "() => {}",
                    self.rewrite_value_preserving_source(a)?
                ))
            })
            .collect::<Result<Vec<_>, UnsupportedSvelteRuntimeSurface>>()?;
        match callee {
            // A static snippet-name call (`{@render pair(1, 2)}`) — a DIRECT call
            // `pair(node, () => 1, () => 2)`.
            RenderCallee::Snippet(binding) => {
                let name = self.ir.analysis.bindings.get(*binding).name.clone();
                Ok(ClientNode::Render(ClientRender {
                    dynamic: false,
                    callee: name,
                    maybe_call: false,
                    args: arg_thunks,
                }))
            }
            // A dynamic render (`{@render children?.()}` / `{@render (cond?a:b)()}`) —
            // `$.snippet(node, () => <fn>[ ?? $.noop], …args)`. The `?? $.noop` is added
            // only for the optional `?.()` (ChainExpression) form.
            RenderCallee::Dynamic(expr) => {
                let analyzed = self.ir.analysis.expressions.get(*expr);
                let (fn_body, _is_chain) =
                    self.render_dynamic_callee(analyzed.source, analyzed.scope)?;
                Ok(ClientNode::Render(ClientRender {
                    dynamic: true,
                    callee: fn_body,
                    maybe_call: false,
                    args: arg_thunks,
                }))
            }
        }
    }

    /// Rewrite a dynamic `{@render}` callee expression into the `$.snippet` snippet-fn
    /// thunk body: peel the trailing call, rewrite the callee, and append `?? $.noop` for
    /// the optional `?.()` (ChainExpression) form. Returns (fn-body, is_chain).
    fn render_dynamic_callee(
        &self,
        source: &str,
        scope: super::expr::ScopeId,
    ) -> Result<(String, bool), UnsupportedSvelteRuntimeSurface> {
        // Parse `(<source>);` to peel the trailing call expression and detect the optional
        // chain (`fn?.()`), mirroring the official `unwrap_optional` + ChainExpression rule.
        let alloc = Allocator::default();
        let wrapped = format!("({source});");
        let parsed = oxc_parser::Parser::new(&alloc, &wrapped, oxc_span::SourceType::tsx()).parse();
        if parsed.panicked || !parsed.errors.is_empty() {
            return Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "render",
                span: Span::new(0, 0),
            });
        }
        use oxc_ast::ast::{Expression, Statement};
        let mut is_chain = false;
        // The callee SOURCE slice (the part before the trailing `()`).
        let callee_src = parsed
            .program
            .body
            .first()
            .and_then(|stmt| match stmt {
                Statement::ExpressionStatement(s) => Some(&s.expression),
                _ => None,
            })
            .and_then(|expr| {
                let mut e = expr;
                while let Expression::ParenthesizedExpression(p) = e {
                    e = &p.expression;
                }
                // A `ChainExpression` wraps an optional call (`fn?.()`).
                if let Expression::ChainExpression(chain) = e {
                    is_chain = true;
                    if let oxc_ast::ast::ChainElement::CallExpression(call) = &chain.expression {
                        let s = call.callee.span();
                        return Some(slice_inner(source, s.start, s.end));
                    }
                }
                if let Expression::CallExpression(call) = e {
                    let s = call.callee.span();
                    return Some(slice_inner(source, s.start, s.end));
                }
                None
            });
        let callee_src = callee_src.ok_or(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
            construct: "render",
            span: Span::new(0, 0),
        })?;
        let rewritten = self.rewrite_source(callee_src, scope)?;
        let body = if is_chain {
            format!("{rewritten} ?? $.noop")
        } else {
            rewritten
        };
        Ok((body, is_chain))
    }

    /// Whether the component needs a component context (`$.push`/`$.pop`) — the official
    /// `needs_context` analysis over the instance script + every template expression.
    pub(super) fn needs_context(&self, alloc: &Allocator) -> bool {
        // A `{@render}` DYNAMIC callee (`children?.()` / `(cond ? a : b)()`) is a SNIPPET
        // invocation — it never needs a component context, even though its callee is a
        // call rooted at a prop. Exclude those callee expressions from the scan (the
        // official `needs_context` counts `getContext` / `$effect` / lifecycle, never a
        // snippet render); a render ARGUMENT still counts (it may call a context-using
        // import).
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
        let template_expr_sources: Vec<&str> = self
            .ir
            .analysis
            .expressions
            .all()
            .iter()
            .enumerate()
            .filter(|(idx, _)| !render_callee_exprs.contains(&ExprId(*idx as u32)))
            .map(|(_, e)| e.source)
            .collect();
        super::reactive_analysis::needs_context(
            alloc,
            self.ir.analysis.scripts.instance_source,
            &template_expr_sources,
        )
    }
}

/// Whether an expression's transparent-paren-unwrapped root is a simple `Identifier` or
/// `MemberExpression` (NOT a compound expression) — the official component-prop
/// `should_wrap_in_derived` negation (`n.expression.type !== 'Identifier' &&
/// !== 'MemberExpression'`).
fn expr_is_simple_ref(source: &str) -> bool {
    use oxc_ast::ast::{Expression, Statement};
    let alloc = Allocator::default();
    let wrapped = format!("({source});");
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, oxc_span::SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return false;
    }
    parsed
        .program
        .body
        .first()
        .and_then(|stmt| match stmt {
            Statement::ExpressionStatement(s) => Some(&s.expression),
            _ => None,
        })
        .map(|expr| {
            let mut e = expr;
            while let Expression::ParenthesizedExpression(p) = e {
                e = &p.expression;
            }
            matches!(
                e,
                Expression::Identifier(_)
                    | Expression::StaticMemberExpression(_)
                    | Expression::ComputedMemberExpression(_)
            )
        })
        .unwrap_or(false)
}

/// Slice the inner source `source[start - off .. end - off]` where the spans were
/// computed over the `(source)`-wrapped parse (offset by the leading `(`).
fn slice_inner(source: &str, start: u32, end: u32) -> &str {
    // The wrap is `(source);` — the leading `(` shifts every span by 1.
    let s = (start as usize).saturating_sub(1);
    let e = (end as usize).saturating_sub(1);
    source.get(s..e).unwrap_or(source)
}
