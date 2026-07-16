//! The element lifecycle-directive emission half of the Svelte client emitter
//! (`use:` actions, `transition:`/`in:`/`out:` transitions, keyed-each `animate:`
//! animations, element-position `{@attach}` attachments), extracted alongside
//! `client_event.rs` to keep `client.rs` under the file-size guard.
//!
//! These free functions render the official `svelte/internal/client` lifecycle
//! shapes from the narrow [`ElementLifecycleOp`] substrate:
//! [`render_lifecycle_op`] emits the COMPLETE helper statement per variant —
//!
//! - `$.action(el, ($$node) => callee?.($$node)[, () => arg])` (the closure gains
//!   `$$action_arg` when an argument is present — the EXACT official param names);
//! - `$.transition(FLAG, el, () => fn[, () => params])` (the precomputed
//!   `TRANSITION_IN|OUT|GLOBAL` integer; the getParams thunk present IFF params);
//! - `$.animation(el, () => fn, PARAMS)` (ALWAYS 3 args — the literal `null` when
//!   no params);
//! - `$.attach(el, () => payload)` (2 args) —
//!
//! and [`build_inline_render_index`] pre-indexes the INIT-DOMAIN per-node render
//! ops (inline `bind:this` + `Action` / `Attachment` lifecycle, in plan-op — i.e.
//! attribute source — order) so the walk drains each node's inline sequence in
//! O(1), preserving the official source-order interleave between a `bind:this`
//! and an adjacent action/attachment. `Transition` / `Animation` are NOT indexed
//! here: they emit LAST in the post-walk AFTER-UPDATE stream (after the
//! global-host listeners, source-ordered with the bare legacy `on:` events and
//! bare non-`this` binds at their element's EXIT rank, interleaved with the
//! regular-element modern events at each element's ENTER rank — see
//! [`after_update_ranks`]), the official `RegularElement` phase split.

use super::client_plan::{ClientModulePlan, ClientRuntimeOp, EventEmit};
use super::client_plan_types::{ElementLifecycleOp, EventEmitTarget};
use super::client_shapes::{BindGetSetForm, ClientBindShape};
use super::ir::{EventOrigin, IrNode, NodeId};

/// One INIT-DOMAIN per-node render op drained inline during the walk — a
/// `bind:this`, an init-domain lifecycle op (`Action` / `Attachment`), the
/// effect-wrapped LEGACY `on:` event of a `use:` action host, or the
/// effect-wrapped non-`this` DOM bind of a `use:` action host, in attribute
/// source order. Keeping all four in ONE ordered sequence preserves the
/// official interleave (`<div use:foo bind:this={el}>` emits `$.action` then
/// `$.bind_this`; the reverse source order reverses the emission; an
/// `on:click` before `use:foo` emits its `$.effect(() => $.event(…))` BEFORE
/// the `$.action`; a `bind:value` after `use:foo` emits its
/// `$.effect(() => $.bind_value(…))` AFTER it).
pub(super) enum InlineRenderOp {
    /// An inline `bind:this` (the render-side binding) — the bind's
    /// [`BindGetSetForm`] plus its rewritten getter/setter bodies.
    BindThis {
        /// The bind's identifier-thunk vs function-pair form.
        getset: BindGetSetForm,
        /// The rewritten getter body.
        getter: String,
        /// The rewritten setter body.
        setter: String,
    },
    /// An init-domain lifecycle op (`Action` / `Attachment`).
    Lifecycle(ElementLifecycleOp),
    /// A LEGACY `on:` event on a `use:` action host — official svelte@5.56.3
    /// wraps each such registration in its OWN `$.effect(() => $.event(…))` in
    /// the init domain (at the event's attribute source position), instead of
    /// the bare directive-batch `$.event(…)` statement action-less elements
    /// keep. The wrap trigger is the LEGACY `on:` ORIGIN, not delegation: a
    /// MODERN non-delegated event on the same host stays a bare post-walk
    /// `$.event(…)` (official `RegularElement.js` wraps only an `OnDirective`
    /// under `has_use`).
    EffectEvent(EventEmit),
    /// A non-`this` DOM bind on a `use:` action host — official wraps each in its
    /// OWN `$.effect(() => $.bind_*(...));` in the init domain at its attribute
    /// source position (svelte@5.56.3 RegularElement.js under has_use). `bind:this`
    /// is NEVER wrapped (it stays the unwrapped inline BindThis arm).
    EffectBind {
        /// The bind's accepted DOM shape (routing + get/set form + group key).
        shape: ClientBindShape,
        /// The rewritten getter body.
        getter: String,
        /// The rewritten setter body.
        setter: String,
    },
}

/// The nodes hosting a `use:` action ([`ElementLifecycleOp::Action`]) — the
/// trigger set for the official effect wrap of co-located LEGACY `on:` events.
/// Computed ONCE per plan and shared by [`build_inline_render_index`] (which
/// routes the wrapped events inline) and the post-walk event stages (which
/// consult the same [`event_emission_slot`]) so the sides can never disagree.
pub(super) fn action_host_nodes(plan: &ClientModulePlan<'_>) -> rustc_hash::FxHashSet<NodeId> {
    let mut hosts = rustc_hash::FxHashSet::default();
    for op in plan.all_ops() {
        if let ClientRuntimeOp::Lifecycle(ElementLifecycleOp::Action { target, .. }) = op {
            hosts.insert(NodeId(target.0));
        }
    }
    hosts
}

/// WHERE an event registration is emitted — the closed three-slot placement
/// vocabulary every event consumer classifies through (the SINGLE shared
/// predicate: the inline init-index build, the post-walk event stage, and the
/// directive-batch stage all key on this, so they can never disagree).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventEmissionSlot {
    /// A LEGACY `on:` event on a `use:` action host — wrapped
    /// `$.effect(() => $.event(…))` in the INIT domain at its attribute source
    /// position (official `RegularElement.js`: an `OnDirective` under `has_use`).
    InitEffectWrapped,
    /// A bare LEGACY `on:` event on a regular node WITHOUT a `use:` action — it
    /// joins the element's post-walk DIRECTIVE BATCH, source-ordered with
    /// `$.transition` / `$.animation` (official: `other_directives` →
    /// `element_state.after_update`).
    DirectiveBatch,
    /// Every other event — a MODERN `on*` attribute (delegated `$.delegated` or
    /// direct `$.event`, NEVER effect-wrapped) and every GLOBAL-host listener
    /// (`$.window` / `$.document` / `$.document.body`). A GLOBAL-host listener
    /// (non-`Node` target) emits in the post-walk phase BEFORE the after-update
    /// stream; a REGULAR-element modern event (`Node` target) joins the stream
    /// at its element's ENTER rank ([`AfterUpdateRank::pre`] — official pushes
    /// it onto the enclosing after_update at attribute-visit time, before the
    /// children merge) — still never wrapped, never in the element's own batch.
    PostWalk,
}

/// Classify an event registration's emission slot — the official svelte@5.56.3
/// placement rule keys on the LEGACY `on:` ORIGIN, not on delegation: only an
/// `OnDirective` joins `RegularElement.js`'s `other_directives` walk (wrapped
/// under `has_use`, else the after-update directive batch), while a MODERN
/// `on*` attribute — delegated or not — pushes its registration BEFORE the
/// batch and never wraps. Global-host events (`$.window` / `$.document` /
/// `$.document.body` / `$$element`) never wrap or batch: lifecycle directives
/// on those hosts are fail-closed upstream, and the official special-element
/// visitors push their listeners straight to the init body.
pub(super) fn event_emission_slot(
    action_hosts: &rustc_hash::FxHashSet<NodeId>,
    emit: &EventEmit,
) -> EventEmissionSlot {
    let EventEmitTarget::Node(id) = emit.target else {
        return EventEmissionSlot::PostWalk;
    };
    if emit.origin != EventOrigin::LegacyDirective {
        return EventEmissionSlot::PostWalk;
    }
    if action_hosts.contains(&NodeId(id.0)) {
        EventEmissionSlot::InitEffectWrapped
    } else {
        EventEmissionSlot::DirectiveBatch
    }
}

/// WHERE a `bind:` registration is emitted — the closed four-slot placement
/// vocabulary every bind consumer classifies through, PARALLEL to
/// [`EventEmissionSlot`] (the SINGLE shared predicate: the inline init-index
/// build, the post-walk phase, and the directive-batch stage all key on
/// [`bind_emission_slot`], so the sides can never disagree).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BindEmissionSlot {
    /// `bind:this` on a REGULAR element — inline UNWRAPPED init
    /// ([`InlineRenderOp::BindThis`]); never effect-wrapped, even on a `use:`
    /// action host (official `RegularElement.js` wraps only the non-`this`
    /// binds under `has_use`).
    InlineThis,
    /// A non-`this` bind on a REGULAR element that is a `use:` action host —
    /// inline `$.effect(() => $.bind_*(...))` init
    /// ([`InlineRenderOp::EffectBind`]) at its attribute source position.
    InitEffectWrapped,
    /// A non-`this` bind on a REGULAR element without `use:` — it joins the
    /// element's after-update DIRECTIVE BATCH, bare and source-ordered with
    /// `$.transition` / `$.animation` / the bare legacy `on:` events
    /// (official: the bind pushes onto `element_state.after_update`).
    DirectiveBatch,
    /// The bind targets a SPECIAL node (window/document/body/svelte:element) —
    /// keep the EXISTING special/global-host emission path unchanged. The
    /// classifier makes no regular-element decision for these.
    SpecialHost,
}

/// Classify a bind registration's emission slot — the official svelte@5.56.3
/// placement rule for `RegularElement.js`: `bind:this` stays an unwrapped
/// inline init op; any other bind wraps in its own init `$.effect` under
/// `has_use`, else joins the element's after-update directive batch. A bind on
/// any Special node keeps its existing path (global-host binds emit in the
/// post-walk phase; `<svelte:element>` binds emit inside the element callback).
pub(super) fn bind_emission_slot(
    plan: &ClientModulePlan<'_>,
    action_hosts: &rustc_hash::FxHashSet<NodeId>,
    shape: &ClientBindShape,
    target: NodeId,
) -> BindEmissionSlot {
    if matches!(plan.build.ir.node(target), IrNode::Special(_)) {
        return BindEmissionSlot::SpecialHost;
    }
    if matches!(shape, ClientBindShape::This { .. }) {
        return BindEmissionSlot::InlineThis;
    }
    if action_hosts.contains(&target) {
        BindEmissionSlot::InitEffectWrapped
    } else {
        BindEmissionSlot::DirectiveBatch
    }
}

/// A node's EULER-TOUR positions over the template tree — the AFTER-UPDATE
/// stream linearization authority. Official `svelte@5.56.3` builds ONE
/// after-update stream per fragment: a MODERN `on*` event registration is
/// pushed onto the ENCLOSING state's after_update IMMEDIATELY at its element's
/// attribute-visit time (BEFORE the children are merged), while the element's
/// own directive batch (`$.transition` / `$.animation` / bare legacy `on:`
/// events / bare non-`this` binds) merges AFTER its children's
/// (`RegularElement.js`: `context.state.after_update.push(
/// ...child_state.after_update, ...element_state.after_update)`). Sorting every
/// stream item by its element's ENTER position (modern events → [`Self::pre`])
/// or EXIT position (batch items → [`Self::post`]) on one shared Euler counter,
/// tie-broken by op source index, reproduces exactly that recursive interleave:
/// a parent's modern event precedes its child's batch, a child's batch precedes
/// its parent's, and sibling groups keep document order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AfterUpdateRank {
    /// The Euler-tour ENTER position — where the element's MODERN `on*` event
    /// registrations join the after-update stream.
    pub(super) pre: u32,
    /// The Euler-tour EXIT position (after every descendant's enter/exit) —
    /// where the element's own directive-batch items join the stream.
    pub(super) post: u32,
}

/// The [`AfterUpdateRank`] of every narrow template node. Ranks are assigned
/// over the NARROW node arena (the sole emission input), walking every template
/// scope's roots left-to-right; they are only ever COMPARED within one scope,
/// so the cross-scope numbering is irrelevant.
///
/// COMPLETENESS: the walk is exhaustive over every same-scope op-hosting node.
/// EVERY template scope's roots are ranked (region bodies — an `{#each}` body, a
/// `{#snippet}` body, a component slot region, a special's body region — are their
/// OWN scopes, so their roots enter through the per-scope loop), and the ONLY
/// narrow node kind carrying SAME-SCOPE children is `Element` (every other
/// container hosts its content in a separate template scope). The descent match is
/// EXHAUSTIVE by construction — a new `ClientNode` variant must decide its
/// child-ranking arm here or fail to compile, so the ranking can never silently
/// skip a same-scope subtree; an unranked after-update op target is a HARD error
/// at emit ([`require_after_update_rank`]), never a silent tail sort.
pub(super) fn after_update_ranks(
    plan: &ClientModulePlan<'_>,
) -> rustc_hash::FxHashMap<NodeId, AfterUpdateRank> {
    fn assign(
        plan: &ClientModulePlan<'_>,
        id: NodeId,
        next: &mut u32,
        out: &mut rustc_hash::FxHashMap<NodeId, AfterUpdateRank>,
    ) {
        use super::client_plan::ClientNode;
        let pre = *next;
        *next += 1;
        match plan.nodes.get(id.0 as usize) {
            // The ONLY same-scope child carrier: an element's children live in the
            // element's own scope and are ranked inside its enter/exit window.
            Some(ClientNode::Element { children, .. }) => {
                for child in children {
                    assign(plan, NodeId(child.0), next, out);
                }
            }
            // Leaf nodes — no children.
            Some(
                ClientNode::Text { .. }
                | ClientNode::Comment { .. }
                | ClientNode::StaticText { .. }
                | ClientNode::ReactiveText { .. }
                | ClientNode::RawHtml { .. }
                | ClientNode::OptionsMarker { .. }
                | ClientNode::SpecialHost { .. }
                | ClientNode::SnippetDecl { .. }
                | ClientNode::Declarations { .. }
                | ClientNode::Debug { .. },
            ) => {}
            // Region-hosting nodes — their content lives in SEPARATE template
            // scopes (block bodies, slot regions, special body regions), ranked by
            // the per-scope loop below; they carry NO same-scope children.
            Some(
                ClientNode::Block(_)
                | ClientNode::Component(_)
                | ClientNode::Slot(_)
                | ClientNode::Render(_)
                | ClientNode::SvelteElement(_)
                | ClientNode::Boundary(_)
                | ClientNode::Head(_),
            ) => {}
            // An out-of-arena id cannot host ops (defensive; the arena mirrors the
            // IR node-id space).
            None => {}
        }
        let post = *next;
        *next += 1;
        out.insert(id, AfterUpdateRank { pre, post });
    }
    let mut out = rustc_hash::FxHashMap::default();
    let mut next = 0u32;
    for scope in &plan.build.ir.template_scopes {
        for &root in &scope.roots {
            assign(plan, root, &mut next, &mut out);
        }
    }
    out
}

/// The FAIL-LOUD after-update rank lookup: an after-update op whose target node is
/// missing from the rank map is a planner/emitter DESYNC — a silent fallback rank
/// would tail-sort the op and misorder the official after-update stream. Panic
/// with the invariant name instead.
pub(super) fn require_after_update_rank(
    ranks: &rustc_hash::FxHashMap<NodeId, AfterUpdateRank>,
    node: NodeId,
) -> AfterUpdateRank {
    *ranks.get(&node).unwrap_or_else(|| {
        unreachable!(
            "after-update op target not ranked: node {} (every same-scope op-hosting \
             node must have an Euler-tour rank)",
            node.0
        )
    })
}

/// Render the single lifecycle helper statement for an [`ElementLifecycleOp`] —
/// the exact official call shape (a leading indent + trailing newline). The
/// emitter pushes the result verbatim onto its accumulator; isolating the
/// rendering makes the COMPLETE emitted call — helper family, FLAG integer,
/// closure params, thunk arity — directly assertable per variant.
pub(super) fn render_lifecycle_op(
    op: &ElementLifecycleOp,
    node_var: &rustc_hash::FxHashMap<NodeId, String>,
) -> String {
    let target = op.target();
    let el = node_var
        .get(&NodeId(target.0))
        .cloned()
        .unwrap_or_else(|| "node".to_string());
    match op {
        // No-arg: `$.action(el, ($$node) => callee?.($$node));`
        // With-arg: `$.action(el, ($$node, $$action_arg) => callee?.($$node,
        // $$action_arg), () => arg);` — the official `$$node` / `$$action_arg`
        // param names and the optional-chained callee call.
        ElementLifecycleOp::Action { callee, arg, .. } => match arg {
            Some(arg) => format!(
                "\t$.action({el}, ($$node, $$action_arg) => {callee}?.($$node, $$action_arg), () => {arg});\n"
            ),
            None => format!("\t$.action({el}, ($$node) => {callee}?.($$node));\n"),
        },
        // `$.transition(FLAG, el, () => fn[, () => params]);` — the 4th getParams
        // thunk present IFF params were given.
        ElementLifecycleOp::Transition {
            flags,
            get_fn,
            params,
            ..
        } => match params {
            Some(params) => {
                format!("\t$.transition({flags}, {el}, () => {get_fn}, () => {params});\n")
            }
            None => format!("\t$.transition({flags}, {el}, () => {get_fn});\n"),
        },
        // `$.animation(el, () => fn, PARAMS);` — ALWAYS 3 args; no params → the
        // official literal `null`.
        ElementLifecycleOp::Animation { get_fn, params, .. } => match params {
            Some(params) => {
                format!("\t$.animation({el}, () => {get_fn}, () => {params});\n")
            }
            None => format!("\t$.animation({el}, () => {get_fn}, null);\n"),
        },
        // `$.attach(el, () => payload);` — 2 args, getter thunk over the PREPARED
        // payload (a legacy-wrapped payload keeps the thunk over the sequence; the
        // raw path applies the shared `b.thunk` zero-arg unthunk).
        ElementLifecycleOp::Attachment { payload, .. } => {
            format!("\t$.attach({el}, {});\n", payload.thunk())
        }
    }
}

/// Pre-index every INIT-DOMAIN inline render op (`bind:this` + `Action` /
/// `Attachment` + the effect-wrapped LEGACY `on:` events AND effect-wrapped
/// non-`this` DOM binds of `use:` action hosts) by its target node id, in
/// plan-op (attribute source) order, so the walk drains each node's inline
/// sequence in O(1) — one ordered sequence per node, preserving the official
/// bind:this ↔ action/attach ↔ effect(event) ↔ effect(bind) source interleave.
/// Built ONCE in `ClientEmitter::new` from the SAME [`action_host_nodes`] set +
/// [`event_emission_slot`] / [`bind_emission_slot`] classifiers the post-walk
/// stages consult, so a wrapped event/bind is never double-emitted.
/// `Transition` / `Animation` ops are NOT init-domain and stay in the post-walk
/// directive-batch stage.
pub(super) fn build_inline_render_index(
    plan: &ClientModulePlan<'_>,
    action_hosts: &rustc_hash::FxHashSet<NodeId>,
) -> rustc_hash::FxHashMap<NodeId, Vec<InlineRenderOp>> {
    let mut index: rustc_hash::FxHashMap<NodeId, Vec<InlineRenderOp>> =
        rustc_hash::FxHashMap::default();
    for op in plan.all_ops() {
        match op {
            // A REGULAR-element `bind:this` (the `InlineThis` slot) — the ONLY
            // `bind:this` indexed inline. A GLOBAL-host `bind:this`
            // (`SpecialHost`) fails this guard and falls through unindexed: a
            // global host renders no element (no walk position), so an inline
            // entry could never drain; its registration emits post-walk in the
            // init body instead. The SAME `bind_emission_slot` classifier decides
            // every side, so a bind is never double- or zero-emitted.
            ClientRuntimeOp::Bind {
                target,
                shape: shape @ ClientBindShape::This { getset },
                getter,
                setter,
            } if bind_emission_slot(plan, action_hosts, shape, NodeId(target.0))
                == BindEmissionSlot::InlineThis =>
            {
                index
                    .entry(NodeId(target.0))
                    .or_default()
                    .push(InlineRenderOp::BindThis {
                        getset: *getset,
                        getter: getter.clone(),
                        setter: setter.clone(),
                    });
            }
            // A non-`this` DOM bind on a `use:` action host. Non-`use:` DomBinds
            // (DirectiveBatch) and special-host binds — `bind:this` included —
            // fail the slot guards and fall through to `_ => {}` (unindexed —
            // they emit post-walk).
            ClientRuntimeOp::Bind {
                target,
                shape,
                getter,
                setter,
            } if bind_emission_slot(plan, action_hosts, shape, NodeId(target.0))
                == BindEmissionSlot::InitEffectWrapped =>
            {
                index
                    .entry(NodeId(target.0))
                    .or_default()
                    .push(InlineRenderOp::EffectBind {
                        shape: shape.clone(),
                        getter: getter.clone(),
                        setter: setter.clone(),
                    });
            }
            ClientRuntimeOp::Lifecycle(lifecycle) if lifecycle.is_init_domain() => {
                index
                    .entry(NodeId(lifecycle.target().0))
                    .or_default()
                    .push(InlineRenderOp::Lifecycle(lifecycle.clone()));
            }
            ClientRuntimeOp::Event { emit, .. }
                if event_emission_slot(action_hosts, emit)
                    == EventEmissionSlot::InitEffectWrapped =>
            {
                let EventEmitTarget::Node(id) = emit.target else {
                    unreachable!("the InitEffectWrapped slot admits Node targets only");
                };
                index
                    .entry(NodeId(id.0))
                    .or_default()
                    .push(InlineRenderOp::EffectEvent(emit.clone()));
            }
            _ => {}
        }
    }
    index
}

#[cfg(test)]
mod render_lifecycle_op_tests {
    use super::*;
    use crate::svelte::runtime::client_plan::ClientNodeId;

    fn vars() -> rustc_hash::FxHashMap<NodeId, String> {
        let mut node_var = rustc_hash::FxHashMap::default();
        node_var.insert(NodeId(3), "div".to_string());
        node_var
    }

    #[test]
    fn action_renders_the_official_closure_shapes() {
        // No-arg: 2 args, `$$node` param, optional-chained callee.
        assert_eq!(
            render_lifecycle_op(
                &ElementLifecycleOp::Action {
                    target: ClientNodeId(3),
                    callee: "foo".to_string(),
                    arg: None,
                },
                &vars(),
            )
            .trim(),
            "$.action(div, ($$node) => foo?.($$node));"
        );
        // With-arg: the closure gains `$$action_arg`; the 3rd arg is the getter thunk.
        assert_eq!(
            render_lifecycle_op(
                &ElementLifecycleOp::Action {
                    target: ClientNodeId(3),
                    callee: "obj.foo".to_string(),
                    arg: Some("($.get(c))".to_string()),
                },
                &vars(),
            )
            .trim(),
            "$.action(div, ($$node, $$action_arg) => obj.foo?.($$node, $$action_arg), () => ($.get(c)));"
        );
    }

    #[test]
    fn transition_renders_the_flag_and_conditional_params_thunk() {
        // No params → EXACTLY 3 args (no trailing thunk).
        assert_eq!(
            render_lifecycle_op(
                &ElementLifecycleOp::Transition {
                    target: ClientNodeId(3),
                    flags: 7,
                    get_fn: "fade".to_string(),
                    params: None,
                },
                &vars(),
            )
            .trim(),
            "$.transition(7, div, () => fade);"
        );
        // Params → the 4th getParams thunk.
        assert_eq!(
            render_lifecycle_op(
                &ElementLifecycleOp::Transition {
                    target: ClientNodeId(3),
                    flags: 1,
                    get_fn: "fly".to_string(),
                    params: Some("({ duration: 200 })".to_string()),
                },
                &vars(),
            )
            .trim(),
            "$.transition(1, div, () => fly, () => ({ duration: 200 }));"
        );
    }

    #[test]
    fn animation_is_always_three_args_with_null_default() {
        // No params → the literal `null` 3rd arg (NEVER a 2-arg call, NEVER
        // `$.transition`).
        let rendered = render_lifecycle_op(
            &ElementLifecycleOp::Animation {
                target: ClientNodeId(3),
                get_fn: "flip".to_string(),
                params: None,
            },
            &vars(),
        );
        assert_eq!(rendered.trim(), "$.animation(div, () => flip, null);");
        assert!(!rendered.contains("$.transition"));
        // Params → the getParams thunk replaces `null`.
        assert_eq!(
            render_lifecycle_op(
                &ElementLifecycleOp::Animation {
                    target: ClientNodeId(3),
                    get_fn: "flip".to_string(),
                    params: Some("({ duration: 200 })".to_string()),
                },
                &vars(),
            )
            .trim(),
            "$.animation(div, () => flip, () => ({ duration: 200 }));"
        );
    }

    #[test]
    fn attachment_is_the_two_arg_getter_thunk() {
        assert_eq!(
            render_lifecycle_op(
                &ElementLifecycleOp::Attachment {
                    target: ClientNodeId(3),
                    payload: super::super::client_legacy_value::PreparedTemplateValue::test_raw(
                        super::super::client_legacy_value::AuthoredValueSurface::AttachPayload,
                        "(fn)",
                    ),
                },
                &vars(),
            )
            .trim(),
            "$.attach(div, () => (fn));"
        );
    }

    #[test]
    fn init_domain_split_matches_the_official_phase_order() {
        // Action/Attachment are the INIT-domain (inline-in-walk) half; Transition/
        // Animation the post-event half — the official `RegularElement` phase split.
        assert!(ElementLifecycleOp::Action {
            target: ClientNodeId(0),
            callee: "a".to_string(),
            arg: None,
        }
        .is_init_domain());
        assert!(ElementLifecycleOp::Attachment {
            target: ClientNodeId(0),
            payload: super::super::client_legacy_value::PreparedTemplateValue::test_raw(
                super::super::client_legacy_value::AuthoredValueSurface::AttachPayload,
                "p",
            ),
        }
        .is_init_domain());
        assert!(!ElementLifecycleOp::Transition {
            target: ClientNodeId(0),
            flags: 3,
            get_fn: "f".to_string(),
            params: None,
        }
        .is_init_domain());
        assert!(!ElementLifecycleOp::Animation {
            target: ClientNodeId(0),
            get_fn: "f".to_string(),
            params: None,
        }
        .is_init_domain());
    }
}
