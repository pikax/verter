//! The EULER-TOUR after-update linearization / post-walk op emission for the
//! `ClientEmitter`, extracted from `client.rs` to keep the emitter core under
//! the file-size guard.
//!
//! Owns the region op-emission driver ([`ClientEmitter::emit_ops`]): the single
//! combined `$.template_effect`, the global-host init emission, and the
//! Euler-rank-sorted after-update directive-batch stream.

use super::client::{AccKind, ClientEmitter};
use super::client_effect::{emit_text_effect, EffectBody, Memoizer};
use super::client_event::render_event_registration;
use super::client_plan::ClientRuntimeOp;
use super::client_shapes::ClientBindShape;
use super::ir::{NodeId, TemplateScopeId};
use super::output::{MappedCode, SvelteRuntimeOutput};

impl<'a> ClientEmitter<'a> {
    /// Emit the POST-WALK reactive ops for a region: the single combined
    /// `$.template_effect` (the reactive text plus the reactive dynamic attr / class /
    /// style, in source order), then the binds + events. The NON-REACTIVE attribute
    /// inits (autofocus, non-reactive attr/property/class/style) and the reactive
    /// class/style `let <acc>;` accumulator declarations are emitted INLINE during the
    /// walk ([`Self::emit_node_inline_inits`]) at each element's `init` position —
    /// matching official — so this stage does NOT emit them. Every op is the NARROW
    /// [`ClientRuntimeOp`] vocabulary — no broad `RuntimeOp` is matched.
    pub(super) fn emit_ops(&mut self, out: &mut SvelteRuntimeOutput, scope_id: TemplateScopeId) {
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
                        update_bodies.push(EffectBody::Stmt(MappedCode::unmapped(body)));
                    }
                }
                ClientRuntimeOp::ReactiveAttr {
                    emit,
                    reactive: true,
                    target,
                } => {
                    update_bodies.push(EffectBody::Expr(MappedCode::unmapped(
                        self.emit_reactive_attr(NodeId(target.0), emit, &mut Some(&mut memoizer)),
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
                    update_bodies.push(EffectBody::Expr(MappedCode::unmapped(
                        self.emit_set_class(
                            node,
                            value,
                            css_hash.as_deref(),
                            directives.as_deref(),
                            *directives_has_call,
                            acc.as_deref(),
                            &mut Some(&mut memoizer),
                        ),
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
                    update_bodies.push(EffectBody::Expr(MappedCode::unmapped(
                        self.emit_set_style(
                            node,
                            value,
                            directives.as_deref(),
                            *directives_has_call,
                            acc.as_deref(),
                            &mut Some(&mut memoizer),
                        ),
                    )));
                }
                _ => {}
            }
        }
        let deps = memoizer.into_mapped_deps();
        emit_text_effect(out, &update_bodies, &deps);

        // The AFTER-UPDATE STREAM — the official `RegularElement.js`
        // after_update phase, ONE linearized stream per fragment holding: the
        // `$.transition` / `$.animation` lifecycle ops, the bare LEGACY `on:`
        // events (the `DirectiveBatch` slot — a legacy event on a non-`use:` host),
        // the bare non-`this` DOM binds (the `DirectiveBatch` bind slot — a
        // regular-element bind on a non-`use:` host), AND the regular-element
        // MODERN `on*` registrations (delegated `$.delegated` + direct `$.event`),
        // emitted LAST (the global-host init stream already ran before the DOM walk).
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
                    out.push_mapped(&render_event_registration(emit, &self.node_var));
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

    /// Emit the init-only operations owned by one authored global-host node.
    /// Events precede binds within that host, matching the official special
    /// visitor, while the caller orders distinct hosts with head/debug nodes.
    pub(super) fn emit_special_host_ops(&self, out: &mut SvelteRuntimeOutput, node: NodeId) {
        let target = super::client_plan_types::ClientNodeId(node.0);
        for op in self.plan.all_ops() {
            let ClientRuntimeOp::Event {
                target: owner,
                emit,
                ..
            } = op
            else {
                continue;
            };
            if *owner == target
                && !matches!(
                    emit.target,
                    super::client_plan_types::EventEmitTarget::Node(_)
                )
            {
                out.push_mapped(&render_event_registration(emit, &self.node_var));
            }
        }
        for op in self.plan.all_ops() {
            let ClientRuntimeOp::Bind {
                target: owner,
                shape,
                getter,
                setter,
            } = op
            else {
                continue;
            };
            if *owner == target
                && super::client_lifecycle::bind_emission_slot(
                    self.plan,
                    &self.action_hosts,
                    shape,
                    node,
                ) == super::client_lifecycle::BindEmissionSlot::SpecialHost
            {
                out.push_str(&self.render_bind_stmt(node, shape, getter, setter));
            }
        }
    }

    /// The AFTER-UPDATE stream ENTER rank for `node` — a regular-element MODERN
    /// event registration's stream position. An unranked node is a HARD error
    /// (the fail-loud [`require_after_update_rank`] invariant — never a silent
    /// tail position).
    ///
    /// [`require_after_update_rank`]: super::client_lifecycle::require_after_update_rank
    fn after_update_pre_rank(&self, node: NodeId) -> u32 {
        super::client_lifecycle::require_after_update_rank(&self.after_update_rank, node).pre
    }

    /// The AFTER-UPDATE stream EXIT rank for `node` — a directive-batch item's
    /// (`$.transition` / `$.animation` / bare legacy `$.event` / bare `$.bind_*`)
    /// stream position. An unranked node is a HARD error (fail-loud), never a
    /// silent `u32::MAX` tail sort.
    fn after_update_post_rank(&self, node: NodeId) -> u32 {
        super::client_lifecycle::require_after_update_rank(&self.after_update_rank, node).post
    }
}
