//! The client TOPOLOGY summary: the structure-unambiguous helper trace, the
//! delegated-event set, and the runtime import plan a component's runtime IR +
//! static-template plan determine.
//!
//! This records WHICH `svelte/internal/client` helper families a template's
//! structure needs (the structural-helper subset) — NOT the fine-grained DOM-walk
//! helpers or the script read-rewrite helpers, which the emitting backend
//! selects. It emits NO JS string.

use super::css::types::CssScopeFacts;
use super::helpers::{DelegatedEvents, HelperTrace, ImportPlan, SvelteHelper};
use super::html::{StaticTemplatePlan, TemplateFactory};
use super::ir::{
    AttrIr, BlockIr, IrNode, NodeId, RenderCallee, SpecialKind, SvelteMode, SvelteRuntimeIr, TagIr,
    TemplateScopeId,
};

/// The client topology SUMMARY a component plans: the helper trace (in
/// IR-traversal order), the delegated-event set, the import plan, and a reference
/// to the template skeleton it was planned against. Emits NO JS string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientTopologyPlan {
    /// The helper families the topology plans, in IR-traversal order.
    pub helpers: HelperTrace,
    /// The delegated event types, in first-seen order.
    pub delegated_events: DelegatedEvents,
    /// The runtime import plan.
    pub imports: ImportPlan,
    /// The serialized static-template skeleton this topology was planned against
    /// (the `templates` half of the static plan — shared with the server backend).
    pub templates: Vec<TemplateFactory>,
}

/// Plan the CLIENT topology summary for a component's runtime IR + its static
/// template plan.
///
/// Records the STRUCTURE-UNAMBIGUOUS helper topology a topology walk determines
/// from the IR — the template factories, the mount, the block helpers, and the
/// event-delegation helpers — via [`HelperTrace::call`] in IR-traversal order. It
/// deliberately does NOT record the fine-grained DOM-WALK helpers
/// (`first_child` / `child` / `sibling` / `reset` / `next` / `text`): the precise
/// walk strategy is the emitting backend's choice (the official compiler picks
/// `child` vs `first_child` + `next` per its own strategy), so the node-path PLAN
/// lives on [`StaticTemplatePlan::client_paths`] but the specific walk-helper
/// SELECTION is a backend concern. It likewise does NOT model the script
/// read-rewrite helpers (`$.get` / `$.set` / `$.template_effect` / …). The
/// recorded set is therefore the structural-helper SUBSET of the official
/// module's full helper set. Emits NO JS string.
///
/// `scope_facts` is the proven `<style>` plan's scope-injection facts (`None`
/// for a style-less component) — the SAME per-node scoped fact the emission
/// reads, fed to the shared `<svelte:element>` attribute routing so the
/// recorded helper (`$.set_class` vs `$.attribute_effect`) never drifts from
/// the emitted one.
#[must_use]
pub fn plan_client_topology(
    ir: &SvelteRuntimeIr,
    html: &StaticTemplatePlan,
    scope_facts: Option<&CssScopeFacts>,
) -> ClientTopologyPlan {
    let mut helpers = HelperTrace::new();
    let mut delegated = DelegatedEvents::new();

    // Template factories: one `$.from_html` per static-HTML region, `$.text` per
    // text-first region (a region whose root IS a single text node), `$.comment`
    // per comment anchor. The text-first `$.text` ROOT FACTORY is recorded here
    // (it is the region's mount root, structurally parallel to `from_html`); this
    // is distinct from the INTERIOR reactive `$.text` nodes a `from_html` region
    // creates mid-DOM-walk, which stay the emitting backend's concern (and are NOT
    // recorded by the planner). The owned-helper universe therefore counts ONLY
    // text-first ROOT factories for `Text`, never the interior reactive `$.text()`.
    // LATENT CONSTRAINT: the matrix's owned-helper `Text`-COUNT assertion
    // (`runtime_tests` assertion (2)) compares this text-first-root-only planned
    // count against the FULL golden `text` count (root factories PLUS any interior
    // reactive `$.text()`); the equality is sound today ONLY because every committed
    // fixture's interior-text count is 0, so its full golden `text` count equals its
    // text-first-root count. A future fixture emitting an interior reactive
    // `$.text()` would make the golden count exceed the planned count and trip that
    // assertion — it would need an `OwnedHelperCounts` topology-ledger row.
    for factory in &html.templates {
        match factory {
            TemplateFactory::FromHtml { .. } => helpers.call(SvelteHelper::FromHtml),
            TemplateFactory::TextNode { .. } => helpers.call(SvelteHelper::Text),
            TemplateFactory::CommentAnchor { reason } => {
                let _ = reason;
                helpers.call(SvelteHelper::Comment);
            }
            // A STANDALONE component / render root emits NO root-factory helper —
            // it is mounted against the parent block's anchor directly (no
            // `from_html` / `text` / `comment` clone). The component-mount /
            // snippet-render call itself is recorded by the IR node walk below.
            TemplateFactory::Standalone { .. } => {}
        }
    }

    // Block + event topology: walk the IR's nodes, recording block helpers and
    // delegated/non-delegated event helpers in traversal order. The module-level
    // `$.delegate([...])` set declaration is gated below on the COLLECTED delegated
    // set being non-empty (no separate "saw a delegated event" flag).
    walk_topology(ir, ir.root, &mut helpers, &mut delegated, scope_facts);

    // The mount: each template REGION that clones a fragment mounts it with
    // `$.append`, so the append count equals the number of MOUNTING factories — one
    // `$.append` per `from_html` / `text` / `comment` region. A STANDALONE region
    // has no cloned fragment to append (the component/snippet is mounted against the
    // parent anchor directly), so it contributes NO `$.append`.
    for factory in &html.templates {
        if !matches!(factory, TemplateFactory::Standalone { .. }) {
            helpers.call(SvelteHelper::Append);
        }
    }

    // If any delegated event was registered, the module declares the set.
    if !delegated.is_empty() {
        helpers.call(SvelteHelper::Delegate);
    }

    // The import plan derives its flag set from the component reactivity mode: a
    // legacy (non-runes) component carries the `svelte/internal/flags/legacy`
    // side-effect import.
    let legacy_mode = ir.component.mode == SvelteMode::Legacy;

    ClientTopologyPlan {
        helpers,
        delegated_events: delegated,
        imports: ImportPlan::client_for_mode(legacy_mode),
        templates: html.templates.clone(),
    }
}

/// Walk the IR template scopes recording block + event helper topology and
/// collecting delegated events.
fn walk_topology(
    ir: &SvelteRuntimeIr,
    scope: TemplateScopeId,
    helpers: &mut HelperTrace,
    delegated: &mut DelegatedEvents,
    scope_facts: Option<&CssScopeFacts>,
) {
    let roots: Vec<NodeId> = ir.template_scope(scope).roots.clone();
    for node in roots {
        walk_node_topology(ir, node, helpers, delegated, scope_facts);
    }
}

fn walk_node_topology(
    ir: &SvelteRuntimeIr,
    node: NodeId,
    helpers: &mut HelperTrace,
    delegated: &mut DelegatedEvents,
    scope_facts: Option<&CssScopeFacts>,
) {
    match ir.node(node) {
        IrNode::Element(el) => {
            // The host element's full typed attribute inventory feeds the official
            // host-attribute bind gate (the SAME gate the emitter applies), so a
            // refused host shape records NO bind helper.
            for attr in &el.attrs {
                record_attr_topology(attr, Some(&el.tag), &el.attrs, helpers, delegated);
            }
            for &child in &el.children {
                walk_node_topology(ir, child, helpers, delegated, scope_facts);
            }
        }
        // A component invocation records ONLY the `$.bind_this` helper for a
        // `bind:this={ref}` — its props / events / `bind:prop` are CLIENT-PROJECTED into
        // the `Child(<anchor>, …)` call (a component `on:foo` is the `$$events` forward,
        // NOT a DOM `$.event` listener; a component `bind:prop` is a getter/setter, NOT a
        // `$.bind_*`; a spread is `$.spread_props`, a prop-builder, not a structural
        // helper). The slot-content STRUCTURAL helpers come from the slot REGIONS (their
        // factories ride `html.templates`; their block / render helpers ride this walk).
        IrNode::Component(c) => {
            record_component_bind_this(&c.attrs, helpers);
            walk_component_slot_topology(ir, &c.slots, helpers, delegated, scope_facts);
        }
        IrNode::Special(s) => {
            // The component-FAMILY specials are component invocations (only `bind:this`
            // records a helper, slot regions walked) — exactly like `IrNode::Component`.
            if matches!(
                s.kind,
                SpecialKind::Component | SpecialKind::SelfRef | SpecialKind::Fragment
            ) {
                record_component_bind_this(&s.attrs, helpers);
                walk_component_slot_topology(ir, &s.slots, helpers, delegated, scope_facts);
                return;
            }
            if s.kind == SpecialKind::Head {
                helpers.call(SvelteHelper::Head);
            }
            // A GLOBAL / dynamic-element host (`<svelte:window|document|body|element>`)
            // resolves its `bind:` helper through the HOST-SCOPED bind contract — the host
            // TOKEN (`svelte:window` / …) is the bind classifier's host key, so a window-only
            // bind on the window host records `$.bind_window_size`, a body dimension bind
            // records `$.bind_element_size`, etc. (a wrong-host pair records nothing — it
            // fails closed). `<svelte:head|boundary>` have no host-scoped binds (host_token =
            // None). Events record the direct `$.event` helper independent of the host.
            let host_token = match s.kind {
                SpecialKind::Window => Some("svelte:window"),
                SpecialKind::Document => Some("svelte:document"),
                SpecialKind::Body => Some("svelte:body"),
                SpecialKind::Element => Some("svelte:element"),
                _ => None,
            };
            // A `<svelte:boundary>`'s `onerror` lowers to an `error` event attr, but it is a
            // PROPS member of the `$.boundary(node, { onerror }, …)` call — NOT a structural
            // `$.event` listener — so the boundary records NO attr helper (its body + snippet
            // regions ride the factory loop + the child walk; the `$.boundary` call itself is
            // not in the owned-helper universe).
            if s.kind != SpecialKind::Boundary {
                for attr in &s.attrs {
                    // A `<svelte:element>` SPREAD is a fold ENTRY of the single
                    // `$.attribute_effect` the fold-route check below records once
                    // for the whole element — the per-attribute spread arm (the
                    // regular-element rule, one call PER element) must not record
                    // a second one.
                    if s.kind == SpecialKind::Element && matches!(attr, AttrIr::Spread { .. }) {
                        continue;
                    }
                    record_attr_topology(attr, host_token, &s.attrs, helpers, delegated);
                }
            }
            // A `<svelte:element>` routed to the FOLD emits ONE `$.attribute_effect` for
            // the whole co-located fold (the official dynamic-element fold) — record it
            // once. The SHARED routing (`svelte_element_attr_route`) decides: the
            // lone-static-class route (with or without co-located `class:` directives)
            // and the SCOPED class-less synthetic-class route emit the dedicated
            // `$.set_class` (NOT folded, not in the owned-helper universe here); a
            // `bind:` records its own bind helper above; a LEGACY `on:`
            // (`AttrIr::Event`) records `$.event` above (NOT folded). Mirrors the
            // emitter's routing exactly — including the per-node SCOPED fact read from
            // the SAME shared scope facts — so the recorded helper never drifts from
            // the emission.
            if s.kind == SpecialKind::Element
                && matches!(
                    super::client_svelte_element::svelte_element_attr_route(
                        &s.attrs,
                        scope_facts.is_some_and(|facts| facts.hash_for(node).is_some()),
                    ),
                    super::client_svelte_element::SvelteElementAttrRoute::Fold { .. }
                )
            {
                helpers.call(SvelteHelper::AttributeEffect);
            }
            for &child in &s.children {
                walk_node_topology(ir, child, helpers, delegated, scope_facts);
            }
        }
        IrNode::Block(block) => match block {
            BlockIr::If { branches } => {
                helpers.call(SvelteHelper::If);
                for b in branches {
                    walk_topology(ir, b.body, helpers, delegated, scope_facts);
                }
            }
            BlockIr::Each {
                body, else_body, ..
            } => {
                helpers.call(SvelteHelper::Each);
                walk_topology(ir, *body, helpers, delegated, scope_facts);
                if let Some(eb) = else_body {
                    walk_topology(ir, *eb, helpers, delegated, scope_facts);
                }
            }
            BlockIr::Await {
                pending,
                then_body,
                catch_body,
                ..
            } => {
                helpers.call(SvelteHelper::Await);
                for ts in [pending, then_body, catch_body].into_iter().flatten() {
                    walk_topology(ir, *ts, helpers, delegated, scope_facts);
                }
            }
            BlockIr::Key { body, .. } => {
                helpers.call(SvelteHelper::Key);
                walk_topology(ir, *body, helpers, delegated, scope_facts);
            }
            BlockIr::Snippet { body, .. } => {
                walk_topology(ir, *body, helpers, delegated, scope_facts);
            }
        },
        IrNode::Tag(tag) => match tag {
            TagIr::Html { .. } => helpers.call(SvelteHelper::Html),
            TagIr::Render {
                callee: RenderCallee::Dynamic(_),
                ..
            } => helpers.call(SvelteHelper::Snippet),
            _ => {}
        },
        IrNode::Text { .. } | IrNode::Comment { .. } | IrNode::Interpolation { .. } => {}
    }
}

/// Record the ONLY structural helper a component invocation's attributes contribute: a
/// `$.bind_this` per `bind:this={ref}`. Every other component attribute (prop / event /
/// `bind:prop` / spread) is projected into the call body (no structural helper).
fn record_component_bind_this(attrs: &[AttrIr], helpers: &mut HelperTrace) {
    for attr in attrs {
        if let AttrIr::Bind { target, .. } = attr {
            if target == "this" {
                helpers.call(SvelteHelper::BindThis);
            }
        }
    }
}

/// Walk a component's slot regions (default + named) + its `{#snippet}`-def body regions
/// for their block / render / event helper topology. The slot FACTORIES
/// (`from_html` / `text` / `comment`) + their `$.append` mounts ride `html.templates`
/// (collected via `collect_component_slot_template_scopes`), so this records only the
/// in-region STRUCTURAL helpers, never a factory/append (which would double-count).
fn walk_component_slot_topology(
    ir: &SvelteRuntimeIr,
    slots: &super::ir::ComponentSlots,
    helpers: &mut HelperTrace,
    delegated: &mut DelegatedEvents,
    scope_facts: Option<&CssScopeFacts>,
) {
    for &snippet in &slots.snippet_defs {
        walk_node_topology(ir, snippet, helpers, delegated, scope_facts);
    }
    if let Some(default) = slots.default {
        walk_topology(ir, default, helpers, delegated, scope_facts);
    }
    for named in &slots.named {
        walk_topology(ir, named.region, helpers, delegated, scope_facts);
    }
}

/// Record the helper topology + delegated events for one attribute.
///
/// `host_tag` is the DOM host tag for an `IrNode::Element` attribute, or `None` for a
/// component / special-element host (whose hosts are not yet supported, owned by 5f, and
/// record no DOM bind helper). `host_attrs` is the host element's full typed attribute
/// inventory — fed to the official host-attribute bind gate (the SAME gate the
/// emitter applies), empty for a component/special host. The bind helper + its
/// prelude are resolved DATA-DRIVEN from the shared bind routing — never a per-name
/// match arm pile.
fn record_attr_topology(
    attr: &AttrIr,
    host_tag: Option<&str>,
    host_attrs: &[AttrIr],
    helpers: &mut HelperTrace,
    delegated: &mut DelegatedEvents,
) {
    match attr {
        AttrIr::Event {
            event_type,
            delegated: is_delegated,
            ..
        } => {
            if *is_delegated {
                helpers.call(SvelteHelper::Delegated);
                delegated.register(event_type);
            } else {
                helpers.call(SvelteHelper::Event);
            }
        }
        AttrIr::Bind { target, .. } => {
            if target == "this" {
                helpers.call(SvelteHelper::BindThis);
                return;
            }
            // A DOM value/property bind on an element host: resolve its routing and
            // record the bind helper DATA-DRIVEN. A component/special host
            // (`host_tag == None`) or an unsupported `(name, host)` records nothing
            // (the bind fails closed). The per-host PRELUDE cleanup
            // (`remove_input_defaults` / `remove_textarea_child`) is a DOM-walk-level
            // helper, NOT part of the structural-topology owned subset, so it is not
            // recorded here (the emitter places it during the walk).
            let Some(tag) = host_tag else {
                return;
            };
            let Some(routing) = crate::svelte::bind_contract::resolve_runtime_bind(target, tag)
            else {
                return;
            };
            // Apply the SAME official host-attribute gate the emitter
            // (`classify_dom_value_bind`) applies: a host shape the emitter refuses
            // (`<input bind:checked>` with no `type`, `<input type bind:group>`, …)
            // records NOTHING here, so the structural oracle and the emitter never
            // disagree. Single gate authority — reuses `host_attr_gate_passes` (no
            // duplicated gate logic).
            if !super::host_attr_gate::host_attr_gate_passes(target, tag, &routing, host_attrs) {
                return;
            }
            helpers.call(bind_helper_for(routing.helper));
        }
        AttrIr::Spread { .. } => helpers.call(SvelteHelper::AttributeEffect),
        _ => {}
    }
}

/// Map a runtime bind [`RuntimeHelper`](crate::svelte::bind_contract::RuntimeHelper)
/// to its structural [`SvelteHelper`] — the single mapping authority shared by the
/// topology recorder and the emitter's helper vocabulary.
fn bind_helper_for(helper: crate::svelte::bind_contract::RuntimeHelper) -> SvelteHelper {
    use crate::svelte::bind_contract::RuntimeHelper;
    match helper {
        RuntimeHelper::Value => SvelteHelper::BindValue,
        RuntimeHelper::SelectValue => SvelteHelper::BindSelectValue,
        RuntimeHelper::Checked => SvelteHelper::BindChecked,
        RuntimeHelper::Group => SvelteHelper::BindGroup,
        RuntimeHelper::CurrentTime => SvelteHelper::BindCurrentTime,
        RuntimeHelper::Paused => SvelteHelper::BindPaused,
        RuntimeHelper::Played => SvelteHelper::BindPlayed,
        RuntimeHelper::ElementSize => SvelteHelper::BindElementSize,
        RuntimeHelper::ContentEditable => SvelteHelper::BindContentEditable,
        RuntimeHelper::Property => SvelteHelper::BindProperty,
        RuntimeHelper::This => SvelteHelper::BindThis,
        RuntimeHelper::WindowSize => SvelteHelper::BindWindowSize,
        RuntimeHelper::WindowScroll => SvelteHelper::BindWindowScroll,
        RuntimeHelper::Online => SvelteHelper::BindOnline,
        RuntimeHelper::Focused => SvelteHelper::BindFocused,
        RuntimeHelper::ActiveElement => SvelteHelper::BindActiveElement,
    }
}
