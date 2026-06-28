//! The client TOPOLOGY summary: the structure-unambiguous helper trace, the
//! delegated-event set, and the runtime import plan a component's runtime IR +
//! static-template plan determine.
//!
//! This records WHICH `svelte/internal/client` helper families a template's
//! structure needs (the structural-helper subset) — NOT the fine-grained DOM-walk
//! helpers or the script read-rewrite helpers, which the emitting backend
//! selects. It emits NO JS string.

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
#[must_use]
pub fn plan_client_topology(ir: &SvelteRuntimeIr, html: &StaticTemplatePlan) -> ClientTopologyPlan {
    let mut helpers = HelperTrace::new();
    let mut delegated = DelegatedEvents::new();

    // Template factories: one `$.from_html` per static-HTML region, `$.text` per
    // text-first region (a region whose root IS a single text node), `$.comment`
    // per comment anchor. The text-first `$.text` ROOT FACTORY is recorded here
    // (it is the region's mount root, structurally parallel to `from_html`); this
    // is distinct from the INTERIOR reactive `$.text` nodes a `from_html` region
    // creates mid-DOM-walk, which stay the emitting backend's concern (and are NOT
    // recorded by the planner). The owned-helper universe therefore counts the
    // text-first root factory but the matrix only asserts the `Text` count for
    // regions the planner actually plans as text-first.
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
    walk_topology(ir, ir.root, &mut helpers, &mut delegated);

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
) {
    let roots: Vec<NodeId> = ir.template_scope(scope).roots.clone();
    for node in roots {
        walk_node_topology(ir, node, helpers, delegated);
    }
}

fn walk_node_topology(
    ir: &SvelteRuntimeIr,
    node: NodeId,
    helpers: &mut HelperTrace,
    delegated: &mut DelegatedEvents,
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
                walk_node_topology(ir, child, helpers, delegated);
            }
        }
        IrNode::Component(c) => {
            // A component-host bind is NOT a DOM-element bind (component hosts are not yet
            // supported, owned by 5f); pass no DOM host tag so no DOM bind helper records.
            for attr in &c.attrs {
                record_attr_topology(attr, None, &[], helpers, delegated);
            }
            for &child in &c.children {
                walk_node_topology(ir, child, helpers, delegated);
            }
        }
        IrNode::Special(s) => {
            if s.kind == SpecialKind::Head {
                helpers.call(SvelteHelper::Head);
            }
            // A special-element (`<svelte:*>`) bind has no DOM host (special-element hosts
            // are not yet supported, owned by 5f).
            for attr in &s.attrs {
                record_attr_topology(attr, None, &[], helpers, delegated);
            }
            for &child in &s.children {
                walk_node_topology(ir, child, helpers, delegated);
            }
        }
        IrNode::Block(block) => match block {
            BlockIr::If { branches } => {
                helpers.call(SvelteHelper::If);
                for b in branches {
                    walk_topology(ir, b.body, helpers, delegated);
                }
            }
            BlockIr::Each {
                body, else_body, ..
            } => {
                helpers.call(SvelteHelper::Each);
                walk_topology(ir, *body, helpers, delegated);
                if let Some(eb) = else_body {
                    walk_topology(ir, *eb, helpers, delegated);
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
                    walk_topology(ir, *ts, helpers, delegated);
                }
            }
            BlockIr::Key { body, .. } => {
                helpers.call(SvelteHelper::Key);
                walk_topology(ir, *body, helpers, delegated);
            }
            BlockIr::Snippet { body, .. } => {
                walk_topology(ir, *body, helpers, delegated);
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
    }
}
