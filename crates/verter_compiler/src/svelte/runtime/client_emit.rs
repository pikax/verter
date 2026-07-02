//! The EULER-TOUR after-update linearization / post-walk op emission for the
//! `ClientEmitter`, extracted from `client.rs` to keep the emitter core under
//! the file-size guard.
//!
//! Owns the region op-emission driver ([`ClientEmitter::emit_ops`]): the single
//! combined `$.template_effect`, the global-host special grouping, and the
//! Euler-rank-sorted after-update directive-batch stream.

use super::client::{AccKind, ClientEmitter};
use super::client_effect::{emit_text_effect, EffectBody, Memoizer};
use super::client_event::render_event_registration;
use super::client_plan::ClientRuntimeOp;
use super::client_shapes::ClientBindShape;
use super::ir::{IrNode, NodeId, TemplateScopeId};

impl<'a> ClientEmitter<'a> {
    /// Emit the POST-WALK reactive ops for a region: the single combined
    /// `$.template_effect` (the reactive text plus the reactive dynamic attr / class /
    /// style, in source order), then the binds + events. The NON-REACTIVE attribute
    /// inits (autofocus, non-reactive attr/property/class/style) and the reactive
    /// class/style `let <acc>;` accumulator declarations are emitted INLINE during the
    /// walk ([`Self::emit_node_inline_inits`]) at each element's `init` position —
    /// matching official — so this stage does NOT emit them. Every op is the NARROW
    /// [`ClientRuntimeOp`] vocabulary — no broad `RuntimeOp` is matched.
    pub(super) fn emit_ops(&mut self, out: &mut String, scope_id: TemplateScopeId) {
        // The single combined `$.template_effect` — ALL reactive updates in source
        // order: reactive text (one `$.set_text` per text-node run, the official
        // `flush_sequence` dedup), then the reactive dynamic attr / class / style
        // writes. A reactive-text chunk that `has_call` is MEMOIZED through the shared
        // deps-array form (`$0, $1, …`); a bare read stays inline. A reactive
        // class/style op reads the accumulator name allocated for its node during the
        // walk (`self.acc_name[node]`), so the `prev` arg + `<acc> =` assignment match
        // the inline-declared `let <acc>;`.
        let mut memoizer = Memoizer::default();
        let mut update_bodies: Vec<EffectBody> = Vec::new();
        let mut seen_text_vars = rustc_hash::FxHashSet::default();
        for op in self.plan.ops_in(scope_id) {
            match op {
                ClientRuntimeOp::ReactiveText { target, .. } => {
                    let node = NodeId(target.0);
                    let var = self
                        .interp_var
                        .get(&node)
                        .cloned()
                        .unwrap_or_else(|| "text".to_string());
                    if !seen_text_vars.insert(var) {
                        continue;
                    }
                    let body = self.emit_set_text(node, &mut memoizer);
                    update_bodies.push(EffectBody::Expr(body));
                }
                // A `bind:group` input with a REACTIVE dynamic/mixed `value={…}` folds its
                // guarded change-detection write into THIS combined effect, in source order
                // (the input precedes a sibling reactive text), BEFORE the post-walk
                // `$.bind_group`. It is a STATEMENT body (the `if (…) { … }` guard), so it
                // forces the block form. The value routes through the SHARED memoizer (a
                // `has_call` value → a `$N` deps slot, reused in the guard + write).
                ClientRuntimeOp::Bind {
                    shape:
                        ClientBindShape::DomBind {
                            group_key: Some(_), ..
                        },
                    target,
                    ..
                } => {
                    if let Some(body) =
                        self.emit_group_dynamic_value_effect(NodeId(target.0), &mut memoizer)
                    {
                        update_bodies.push(EffectBody::Stmt(body));
                    }
                }
                ClientRuntimeOp::ReactiveAttr {
                    emit,
                    reactive: true,
                    target,
                } => {
                    update_bodies.push(EffectBody::Expr(self.emit_reactive_attr(
                        NodeId(target.0),
                        emit,
                        &mut Some(&mut memoizer),
                    )));
                }
                ClientRuntimeOp::SetClass {
                    target,
                    value,
                    css_hash,
                    directives,
                    directives_has_call,
                    reactive: true,
                    ..
                } => {
                    let node = NodeId(target.0);
                    let acc = self.acc_name.get(&(node, AccKind::Class)).cloned();
                    update_bodies.push(EffectBody::Expr(self.emit_set_class(
                        node,
                        value,
                        css_hash.as_deref(),
                        directives.as_deref(),
                        *directives_has_call,
                        acc.as_deref(),
                        &mut Some(&mut memoizer),
                    )));
                }
                ClientRuntimeOp::SetStyle {
                    target,
                    value,
                    directives,
                    directives_has_call,
                    reactive: true,
                    ..
                } => {
                    let node = NodeId(target.0);
                    let acc = self.acc_name.get(&(node, AccKind::Style)).cloned();
                    update_bodies.push(EffectBody::Expr(self.emit_set_style(
                        node,
                        value,
                        directives.as_deref(),
                        *directives_has_call,
                        acc.as_deref(),
                        &mut Some(&mut memoizer),
                    )));
                }
                _ => {}
            }
        }
        let deps = memoizer.into_deps();
        emit_text_effect(out, &update_bodies, &deps);

        // (d) The POST-walk special-host binds + MODERN events — every op a NARROW
        // `ClientRuntimeOp` (already-rewritten getter / setter / handler bodies). A
        // regular-element `bind:this` is NOT emitted here: it is a render-side binding
        // emitted INLINE during the walk (see `emit_inline_render_ops`), BEFORE this grouped
        // text effect — matching the official op order. Only SPECIAL-node binds
        // (window/document/body), MODERN `on*` events (delegated + direct), and global-host
        // listeners are post-walk; a regular-element non-`this` DOM bind is NOT (it
        // effect-wraps inline on a `use:` host, else joins the directive batch in phase (e)
        // — the shared `bind_emission_slot` classifier), and a LEGACY `on:` event is NOT
        // either (it effect-wraps inline on a `use:` host, else joins the directive batch).
        //
        // A GLOBAL-host special (`<svelte:window|document|body>`) groups its EVENTS before its
        // BINDS (the official `visit_special_element` order — every `$.event(...)` then every
        // `$.bind_*`), so the source-order op list is reordered per host node
        // ([`Self::post_walk_ops_host_grouped`]). Regular-element ops keep SOURCE order.
        for op in &self.post_walk_ops_host_grouped(scope_id) {
            match op {
                ClientRuntimeOp::Bind {
                    target,
                    shape,
                    getter,
                    setter,
                } => {
                    // Only a `SpecialHost` bind still emits here — the SAME
                    // `bind_emission_slot` classifier decides every side, so a bind
                    // is never double- or zero-emitted: a regular-element `bind:this`
                    // (`InlineThis`) emitted INLINE during the walk (see
                    // `emit_inline_render_ops`), and a regular-element non-`this`
                    // bind either effect-wrapped inline (`InitEffectWrapped` — a
                    // `use:` host) or joins the after-update stream in phase (e)
                    // below (`DirectiveBatch`). The `SpecialHost` slot classifies
                    // every SPECIAL node (window/document/body/svelte:element), but
                    // at RUNTIME only GLOBAL-host (`<svelte:window|document|body>`)
                    // binds reach this arm: a `<svelte:element>` bind never enters
                    // the scope op stream — it lives on the element's `binds` and
                    // emits inside the `($$element, …)` callback
                    // (`client_svelte_element.rs`). A global host renders no element
                    // (no walk position), so its binds — `bind:this` included — are
                    // emitted HERE in the init body, host-grouped with its events.
                    let node = NodeId(target.0);
                    if super::client_lifecycle::bind_emission_slot(
                        self.plan,
                        &self.action_hosts,
                        shape,
                        node,
                    ) == super::client_lifecycle::BindEmissionSlot::SpecialHost
                    {
                        self.emit_bind(out, node, shape, getter, setter);
                    }
                }
                ClientRuntimeOp::Event { emit, .. } => {
                    // Only a GLOBAL-host `PostWalk` listener (`$.window` /
                    // `$.document` / `$.document.body`) emits here — official pushes
                    // global listeners into the init prelude BEFORE the after-update
                    // stream, regardless of source order. A REGULAR-element MODERN
                    // event (a `PostWalk` slot with a `Node` target) joins the
                    // after-update stream in phase (e) below at its element's ENTER
                    // rank; a LEGACY `on:` event either moved into the init domain
                    // as `$.effect(() => $.event(…))` (a `use:` action host —
                    // emitted inline during the walk, see `emit_inline_render_ops`)
                    // or joins the stream at its element's EXIT rank — the SAME
                    // `event_emission_slot` classifier decides every side, so an
                    // event is never double- or zero-emitted.
                    if super::client_lifecycle::event_emission_slot(&self.action_hosts, emit)
                        == super::client_lifecycle::EventEmissionSlot::PostWalk
                        && !matches!(
                            emit.target,
                            super::client_plan_types::EventEmitTarget::Node(_)
                        )
                    {
                        self.emit_event(out, emit);
                    }
                }
                // The reactive-text / reactive-attr / class / style ops were grouped
                // above; the non-reactive attr inits were emitted in (b). The
                // `$.attribute_effect` spread fold and the `$.html` raw-markup op are
                // INLINE init-domain ops (emitted during the walk at the element's init
                // position / the `{@html}` anchor descent), so they are not emitted
                // here. A LIFECYCLE op is either init-domain (action / attach — emitted
                // inline during the walk) or the directive-batch phase (e) below.
                ClientRuntimeOp::ReactiveText { .. }
                | ClientRuntimeOp::ReactiveAttr { .. }
                | ClientRuntimeOp::SetClass { .. }
                | ClientRuntimeOp::SetStyle { .. }
                | ClientRuntimeOp::AttributeEffect { .. }
                | ClientRuntimeOp::Html { .. }
                | ClientRuntimeOp::Lifecycle(_) => {}
            }
        }

        // (e) The AFTER-UPDATE STREAM — the official `RegularElement.js`
        // after_update phase, ONE linearized stream per fragment holding: the
        // `$.transition` / `$.animation` lifecycle ops, the bare LEGACY `on:`
        // events (the `DirectiveBatch` slot — a legacy event on a non-`use:` host),
        // the bare non-`this` DOM binds (the `DirectiveBatch` bind slot — a
        // regular-element bind on a non-`use:` host), AND the regular-element
        // MODERN `on*` registrations (delegated `$.delegated` + direct `$.event`),
        // emitted LAST (the official phase order: `$.template_effect` →
        // global-host listeners/binds → the after-update stream → `$.append`).
        // The init-domain lifecycle half (`$.action` / `$.attach`) and the
        // effect-wrapped legacy events + non-`this` binds of `use:` hosts were
        // emitted INLINE during the walk.
        //
        // Ordering is the official after_update construction, an EULER-TOUR
        // interleave: a MODERN event registration is pushed onto the ENCLOSING
        // after_update at its element's attribute-visit time (the element's ENTER
        // rank — BEFORE its children's items), while an element's own directive
        // batch merges AFTER its children's (`…child_state.after_update,
        // …element_state.after_update` — the element's EXIT rank). So a parent's
        // modern event precedes its child's batch, a child's batch precedes its
        // parent's batch, sibling groups keep document order, and WITHIN an
        // element the modern events come first (enter < exit) while batch items
        // keep attribute SOURCE order (a source-first `on:click` precedes a later
        // `transition:fade`, a source-first `transition:fade` precedes a later
        // `bind:value`, and vice versa). The stable sort on (Euler rank, op index)
        // reproduces exactly that linearization.
        let mut batch: Vec<(u32, usize, &ClientRuntimeOp)> = Vec::new();
        for (idx, op) in self.plan.ops_in(scope_id).iter().enumerate() {
            let rank = match op {
                ClientRuntimeOp::Lifecycle(lifecycle) if !lifecycle.is_init_domain() => {
                    self.after_update_post_rank(NodeId(lifecycle.target().0))
                }
                ClientRuntimeOp::Event { emit, .. } => {
                    match super::client_lifecycle::event_emission_slot(&self.action_hosts, emit) {
                        // A bare LEGACY `on:` event — the element's own batch (EXIT).
                        super::client_lifecycle::EventEmissionSlot::DirectiveBatch => {
                            let super::client_plan_types::EventEmitTarget::Node(id) = emit.target
                            else {
                                unreachable!("the DirectiveBatch slot admits Node targets only");
                            };
                            self.after_update_post_rank(NodeId(id.0))
                        }
                        // A REGULAR-element MODERN event — the stream at the
                        // element's ENTER rank. A global-host listener (non-`Node`
                        // target) emitted in phase (d) above — never streamed.
                        super::client_lifecycle::EventEmissionSlot::PostWalk => {
                            let super::client_plan_types::EventEmitTarget::Node(id) = emit.target
                            else {
                                continue;
                            };
                            self.after_update_pre_rank(NodeId(id.0))
                        }
                        // Emitted inline during the walk (`use:` host wrap).
                        super::client_lifecycle::EventEmissionSlot::InitEffectWrapped => continue,
                    }
                }
                ClientRuntimeOp::Bind { target, shape, .. }
                    if super::client_lifecycle::bind_emission_slot(
                        self.plan,
                        &self.action_hosts,
                        shape,
                        NodeId(target.0),
                    ) == super::client_lifecycle::BindEmissionSlot::DirectiveBatch =>
                {
                    self.after_update_post_rank(NodeId(target.0))
                }
                _ => continue,
            };
            batch.push((rank, idx, op));
        }
        batch.sort_by_key(|&(rank, idx, _)| (rank, idx));
        for (_, _, op) in batch {
            match op {
                ClientRuntimeOp::Lifecycle(lifecycle) => {
                    out.push_str(&super::client_lifecycle::render_lifecycle_op(
                        lifecycle,
                        &self.node_var,
                    ));
                }
                ClientRuntimeOp::Event { emit, .. } => {
                    out.push_str(&render_event_registration(emit, &self.node_var));
                }
                // A bare non-`this` bind registration — the IMMUTABLE render (the
                // batch holds `&ClientRuntimeOp` borrows of the plan, so the
                // `&mut self` emit wrapper cannot be used here).
                ClientRuntimeOp::Bind {
                    target,
                    shape,
                    getter,
                    setter,
                } => {
                    out.push_str(&self.render_bind_stmt(NodeId(target.0), shape, getter, setter));
                }
                // The batch collection above admits only the three arms.
                _ => unreachable!("the directive batch admits lifecycle + event + bind ops only"),
            }
        }
    }

    /// The post-walk op list for `scope_id`, with each GLOBAL-host special's ops reordered so
    /// its EVENTS precede its BINDS (the official `visit_special_element` grouping — every
    /// `$.event(...)` then every `$.bind_*`). A host special's ops are CONTIGUOUS in the
    /// source-order op list (one node's attributes), so each contiguous same-host run is
    /// stable-partitioned events-before-binds; non-host ops (regular elements) keep SOURCE
    /// order. Returns an OWNED snapshot so the emit calls (`&mut self`) borrow freely.
    fn post_walk_ops_host_grouped(&self, scope_id: TemplateScopeId) -> Vec<ClientRuntimeOp> {
        let ops = self.plan.ops_in(scope_id);
        let mut result: Vec<ClientRuntimeOp> = Vec::with_capacity(ops.len());
        let mut i = 0;
        while i < ops.len() {
            let group = self.op_host_group(&ops[i]);
            let Some(_) = group else {
                // A non-host op (regular element bind / event) keeps its source position.
                result.push(ops[i].clone());
                i += 1;
                continue;
            };
            // Gather the CONTIGUOUS run of ops sharing this host group, then emit its EVENTS
            // (source order) followed by its BINDS (source order).
            let mut j = i;
            while j < ops.len() && self.op_host_group(&ops[j]) == group {
                j += 1;
            }
            for op in &ops[i..j] {
                if matches!(op, ClientRuntimeOp::Event { .. }) {
                    result.push(op.clone());
                }
            }
            for op in &ops[i..j] {
                if !matches!(op, ClientRuntimeOp::Event { .. }) {
                    result.push(op.clone());
                }
            }
            i = j;
        }
        result
    }

    /// The GLOBAL-host special group (`Window` / `Document` / `Body`) an op belongs to — a
    /// host EVENT (`$.event` against `$.window` / `$.document` / `$.document.body`) or a host
    /// BIND (targeting a `<svelte:window|document|body>` node). `None` for a regular-element op
    /// (never reordered) or a non-bind/-event op. Drives the events-before-binds host grouping.
    fn op_host_group(&self, op: &ClientRuntimeOp) -> Option<super::ir::SpecialKind> {
        match op {
            ClientRuntimeOp::Event { emit, .. } => match emit.target {
                super::client_plan_types::EventEmitTarget::Window => {
                    Some(super::ir::SpecialKind::Window)
                }
                super::client_plan_types::EventEmitTarget::Document => {
                    Some(super::ir::SpecialKind::Document)
                }
                super::client_plan_types::EventEmitTarget::Body => {
                    Some(super::ir::SpecialKind::Body)
                }
                _ => None,
            },
            ClientRuntimeOp::Bind { target, .. } => {
                let IrNode::Special(s) = self.ir().node(NodeId(target.0)) else {
                    return None;
                };
                matches!(
                    s.kind,
                    super::ir::SpecialKind::Window
                        | super::ir::SpecialKind::Document
                        | super::ir::SpecialKind::Body
                )
                .then_some(s.kind)
            }
            _ => None,
        }
    }

    /// The AFTER-UPDATE stream ENTER rank for `node` — a regular-element MODERN
    /// event registration's stream position (`u32::MAX` for an unranked node, the
    /// defensive tail position).
    fn after_update_pre_rank(&self, node: NodeId) -> u32 {
        self.after_update_rank
            .get(&node)
            .map(|r| r.pre)
            .unwrap_or(u32::MAX)
    }

    /// The AFTER-UPDATE stream EXIT rank for `node` — a directive-batch item's
    /// (`$.transition` / `$.animation` / bare legacy `$.event` / bare `$.bind_*`)
    /// stream position (`u32::MAX` for an unranked node).
    fn after_update_post_rank(&self, node: NodeId) -> u32 {
        self.after_update_rank
            .get(&node)
            .map(|r| r.post)
            .unwrap_or(u32::MAX)
    }
}
