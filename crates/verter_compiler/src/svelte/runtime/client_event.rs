//! The regular-element event-registration emission half of the Svelte client
//! emitter, extracted from `client.rs` to keep it under the file-size guard.
//!
//! These free functions render the official `svelte/internal/client` event shape
//! from the narrow [`EventEmit`] substrate: [`render_event_registration`] emits the
//! COMPLETE `$.event` / `$.delegated` call (host expression, modifier-wrapper nesting
//! inner→outer, and the trailing capture / passive positional args),
//! [`event_target_host`] resolves an [`EventEmitTarget`] to its emitted host
//! expression (a regular node from the node-var map, or the official `$.window` /
//! `$.document` / `$.document.body` globals), and [`emit_delegate_epilogue`] renders
//! the `$.delegate([...])` module epilogue from the first-seen-ordered delegated
//! event-type set. The emitter pushes each rendered fragment verbatim onto its
//! accumulator; isolating the rendering from the accumulator makes the COMPLETE
//! emitted call directly assertable for every target host.

use super::client_plan::{EventEmit, EventEmitTarget, EventMode};
use super::ir::NodeId;
use super::output::MappedCode;

/// Render the single DOM event-registration statement for an [`EventEmit`] — the
/// official `$.event` / `$.delegated` shape
/// `$.<helper>('<type>', <target>, <wrapped-handler>[, <capture>][, <passive>]);` (a
/// leading indent + trailing newline). The emitter pushes the result verbatim onto its
/// accumulator; isolating the rendering from the accumulator makes the COMPLETE emitted
/// call — host expression, wrapper nesting, and trailing positional args — directly
/// assertable for every target host, including the global hosts the regular-element
/// surface never constructs.
pub(super) fn render_event_registration(
    emit: &EventEmit,
    node_var: &rustc_hash::FxHashMap<NodeId, String>,
) -> MappedCode {
    render_event_call(emit, node_var).wrapped("\t", ";\n")
}

/// Render the effect-wrapped registration for a NON-DELEGATED event on a `use:`
/// action host — the official `$.effect(() => $.event(…));` statement (svelte@5.56.3
/// wraps each such event in its OWN init-domain effect so the listener re-registers
/// in action order; delegated events and action-less elements keep the bare form).
/// The inner call is the SAME [`render_event_call`] rendering the bare registration
/// uses — only the `$.effect(() => …)` wrapper differs.
pub(super) fn render_effect_wrapped_event(
    emit: &EventEmit,
    node_var: &rustc_hash::FxHashMap<NodeId, String>,
) -> MappedCode {
    render_event_call(emit, node_var).wrapped("\t$.effect(() => ", ");\n")
}

/// Render the bare `$.event` / `$.delegated` call expression (no indent, no
/// terminating `;`) — the single arg-assembly authority both the bare statement
/// ([`render_event_registration`]) and the `use:`-host effect wrap
/// ([`render_effect_wrapped_event`]) share.
fn render_event_call(
    emit: &EventEmit,
    node_var: &rustc_hash::FxHashMap<NodeId, String>,
) -> MappedCode {
    let target = event_target_host(emit.target, node_var);
    // Nest the handler in the modifier wrappers inner→outer: the FIRST wrapper in
    // the (fixed-order) stack is the INNERMOST (closest to the handler).
    let mut handler = emit.handler.clone();
    for wrapper in &emit.wrappers {
        handler = handler.wrapped(&format!("$.{}(", wrapper.helper()), ")");
    }
    let helper = match emit.mode {
        EventMode::Delegated => "$.delegated",
        EventMode::Direct => "$.event",
    };
    // The trailing capture / passive positional args (the official `b.call`
    // arg-trimming: a present `passive` forces the capture SLOT — `true` or the
    // `void 0` placeholder — plus the passive boolean; a capture-only registration
    // emits just the 4th `true`; otherwise no trailing args).
    let trailing = match emit.passive {
        Some(passive) => {
            let capture_slot = if emit.capture { "true" } else { "void 0" };
            format!(", {capture_slot}, {passive}")
        }
        None if emit.capture => ", true".to_string(),
        None => String::new(),
    };
    let mut call = MappedCode::unmapped(format!("{helper}('{}', {target}, ", emit.event_type));
    call.push_mapped(&handler);
    call.push_unmapped(&trailing);
    call.push_unmapped(")");
    call
}

/// Resolve an [`EventEmitTarget`] to its emitted host expression — the 2nd positional
/// `$.event` / `$.delegated` argument. A regular `Node` resolves from the node-var map
/// (falling back to `node`); the global hosts map to the official `$.window` /
/// `$.document` / `$.document.body` (the reusable special-element event substrate —
/// never produced by the regular-element surface, but exercised by the emitter so the
/// SAME path serves both).
fn event_target_host(
    target: EventEmitTarget,
    node_var: &rustc_hash::FxHashMap<NodeId, String>,
) -> String {
    match target {
        EventEmitTarget::Node(id) => node_var
            .get(&NodeId(id.0))
            .cloned()
            .unwrap_or_else(|| "node".to_string()),
        EventEmitTarget::Window => "$.window".to_string(),
        EventEmitTarget::Document => "$.document".to_string(),
        EventEmitTarget::Body => "$.document.body".to_string(),
        // A `<svelte:element>` legacy `on:` listener targets the element callback's
        // `$$element` param (the host is the dynamic element itself).
        EventEmitTarget::SvelteElement => "$$element".to_string(),
    }
}

/// Emit the `$.delegate([...])` module epilogue from the first-seen-ordered
/// delegated event-type set.
pub(super) fn emit_delegate_epilogue(out: &mut String, events: &[String]) {
    let list = events
        .iter()
        .map(|e| format!("'{e}'"))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("$.delegate([{list}]);\n"));
}

#[cfg(test)]
mod event_target_host_tests {
    use super::*;
    use crate::svelte::runtime::client_plan::ClientNodeId;
    use crate::svelte::runtime::client_plan_types::EventWrapper;

    #[test]
    fn event_target_host_resolves_node_and_global_hosts() {
        // A regular node resolves from the node-var map; the global hosts (the reusable
        // special-element event substrate) resolve to the official `$.window` /
        // `$.document` / `$.document.body` globals. This exercises EVERY `EventEmitTarget`
        // variant so the substrate is proven, not dead.
        let mut node_var = rustc_hash::FxHashMap::default();
        node_var.insert(NodeId(7), "button".to_string());
        assert_eq!(
            event_target_host(EventEmitTarget::Node(ClientNodeId(7)), &node_var),
            "button"
        );
        // An unmapped node falls back to the literal `node` (defensive).
        assert_eq!(
            event_target_host(EventEmitTarget::Node(ClientNodeId(99)), &node_var),
            "node"
        );
        assert_eq!(
            event_target_host(EventEmitTarget::Window, &node_var),
            "$.window"
        );
        assert_eq!(
            event_target_host(EventEmitTarget::Document, &node_var),
            "$.document"
        );
        assert_eq!(
            event_target_host(EventEmitTarget::Body, &node_var),
            "$.document.body"
        );
    }

    #[test]
    fn render_event_registration_emits_the_full_call_for_each_global_target() {
        // The reusable emitter renders the COMPLETE official `$.event` call for every
        // target host — including the special-element global hosts (window / document /
        // document.body) the regular-element surface never constructs — and the capture
        // / passive / modifier-wrapper argument variants. A global host resolves from the
        // target kind alone, so the node-var map is empty here. Exact-string assertions
        // discriminate a wrong host expression or a wrong positional-argument order.
        let node_var = rustc_hash::FxHashMap::<NodeId, String>::default();
        let emit =
            |target, event_type: &str, capture, passive, wrappers: Vec<EventWrapper>| EventEmit {
                mode: EventMode::Direct,
                // The global-host listeners here model the legacy `on:` directive form
                // (`<svelte:window on:resize>`); the render shape is origin-independent.
                origin: crate::svelte::runtime::ir::EventOrigin::LegacyDirective,
                target,
                event_type: event_type.to_string(),
                capture,
                passive,
                wrappers,
                handler: MappedCode::unmapped("h"),
            };
        // `window`, plain listener — no trailing positional args.
        assert_eq!(
            render_event_registration(
                &emit(EventEmitTarget::Window, "resize", false, None, vec![]),
                &node_var,
            )
            .as_str()
            .trim(),
            "$.event('resize', $.window, h);"
        );
        // `document.body`, capture-only — the 4th positional `true`.
        assert_eq!(
            render_event_registration(
                &emit(EventEmitTarget::Body, "click", true, None, vec![]),
                &node_var,
            )
            .as_str()
            .trim(),
            "$.event('click', $.document.body, h, true);"
        );
        // `document`, passive + a `preventDefault` WRAPPER — the wrapped handler plus the
        // `void 0` capture-slot placeholder and the 5th positional passive `true`.
        assert_eq!(
            render_event_registration(
                &emit(
                    EventEmitTarget::Document,
                    "scroll",
                    false,
                    Some(true),
                    vec![EventWrapper::PreventDefault],
                ),
                &node_var,
            )
            .as_str()
            .trim(),
            "$.event('scroll', $.document, $.preventDefault(h), void 0, true);"
        );
    }
}
