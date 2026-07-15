//! The `<slot>`-SURFACE classification half of the default-deny classifier:
//! the unified `slot=`-ATTRIBUTE placement choke-point
//! ([`validate_slot_placement`]) and the `<slot>`-ELEMENT official-rule
//! classifier ([`classify_slot_element`]). Extracted from `client_surface.rs`
//! (the file-size guard boundary); the per-node walk dispatches here.

use super::ir::{AttrIr, IrNode, NodeId, SlotElementIr, SpecialKind};
use super::unsupported::UnsupportedSvelteRuntimeSurface;

/// The three lowering-recorded `slot=` placement-fact sets the unified slot choke-point
/// keys on, borrowed from the IR for the classification walk (see
/// [`SvelteRuntimeIr::static_slot_filler_hosts`] /
/// [`SvelteRuntimeIr::direct_slot_attr_child_hosts`] /
/// [`SvelteRuntimeIr::direct_snippet_slot_attr_child_hosts`] for the membership
/// contracts).
#[derive(Clone, Copy)]
pub(super) struct SlotPlacementFacts<'a> {
    /// The DIRECT static-slot-declaring component children (any node kind).
    pub(super) static_slot_filler_hosts: &'a rustc_hash::FxHashSet<NodeId>,
    /// Every source-level DIRECT child of a component-family node.
    pub(super) direct_slot_attr_child_hosts: &'a rustc_hash::FxHashSet<NodeId>,
    /// Every lowered SOURCE-LEVEL direct child of a `{#snippet}` block body.
    pub(super) direct_snippet_slot_attr_child_hosts: &'a rustc_hash::FxHashSet<NodeId>,
}

/// The UNIFIED `slot`-attribute choke-point — the SOLE `slot=` validation authority,
/// applied to EVERY template node at [`classify_node`] entry, BEFORE any per-kind
/// accept / fold / prop projection. Covering every node kind by construction means no
/// attr-bearing host — regular element, component, or `<svelte:*>` special — can
/// quietly route a `slot` attribute past the official disposition.
///
/// The official `svelte@5.56.3` disposition (`validate_slot_attribute`, driven here
/// from the typed IR node kinds plus the three lowering-recorded placement-fact sets —
/// never name/text sniffing):
///
/// - **Filler (Class A)** — a STATIC `slot="x"` on a DIRECT slot-declaring component
///   child (the node id is in [`SvelteRuntimeIr::static_slot_filler_hosts`]) is
///   accepted on a FILLER host kind: a regular element, a component, a
///   `<svelte:component>` / `<svelte:self>`, or a `<svelte:element>`. The filler
///   routes into the parent's `$$slots.NAME` region; a component-family filler ALSO
///   keeps `slot` as an ordinary prop on its own call, and a `<svelte:element>`
///   filler folds it into `$.attribute_effect` — both are official output shapes.
///   Lowered slot-region-root membership is NOT the placement fact: a transparent
///   `<svelte:fragment slot>`'s hoisted children are region roots but never fillers.
/// - **Plain prop (Class B)** — a `slot` (static OR dynamic/mixed) on a
///   COMPONENT-FAMILY host (a component / `<svelte:component>` / `<svelte:self>`)
///   with NO direct-placement owner at all — neither a component parent
///   ([`SvelteRuntimeIr::direct_slot_attr_child_hosts`]) nor a `{#snippet}` body
///   ([`SvelteRuntimeIr::direct_snippet_slot_attr_child_hosts`]) — is an ordinary
///   prop: official validates a component host with `is_component = true` and accepts
///   it at every owner-less placement, top level included.
/// - **Snippet static** — a SINGLE static TEXT-VALUED `slot="x"` on a DIRECT
///   `{#snippet}`-body child
///   (the node id is in [`SvelteRuntimeIr::direct_snippet_slot_attr_child_hosts`])
///   is accepted on a filler-capable host kind: official validates a snippet child as
///   component-owned placement, so the `slot` stays an ordinary attr/prop on the host
///   itself — a snippet child is NOT a filler, never routes into `$$slots`, and never
///   enters the duplicate/default-slot checks. An element bakes it into the skeleton,
///   a component-family host keeps the plain prop, a `<svelte:element>` folds it into
///   `$.attribute_effect`. The text value is part of the acceptance (official
///   `is_text_attribute`): a VALUELESS/boolean `slot` on a direct snippet child
///   REJECTS (Class C).
/// - **Reject (Class C)** — everything else fails closed with the typed slot refusal:
///   a dynamic/mixed `slot` on a DIRECT component child, on a DIRECT snippet child, or
///   on any element-family host (official `slot_attribute_invalid` — "must be a
///   static value"), a VALUELESS/boolean `slot` on a DIRECT snippet child (the same
///   official `slot_attribute_invalid` — not a text-valued attribute), a static
///   `slot` on an element outside direct-filler /
///   direct-snippet placement (official `slot_attribute_invalid_placement`), and a
///   `slot` on a non-filler special (`<svelte:head>` / `<svelte:boundary>` /
///   `<svelte:fragment>` / the global hosts / `<svelte:options>` — each an official
///   per-host attribute reject, kind-gated even at snippet placement).
///
/// A node kind with no attribute surface (text / comment / interpolation / block /
/// tag) validates trivially.
pub(super) fn validate_slot_placement(
    node: &IrNode,
    node_id: NodeId,
    slot_placement: SlotPlacementFacts<'_>,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    let SlotPlacementFacts {
        static_slot_filler_hosts,
        direct_slot_attr_child_hosts,
        direct_snippet_slot_attr_child_hosts,
    } = slot_placement;
    let (attrs, span) = match node {
        IrNode::Element(el) => (&el.attrs, el.span),
        IrNode::Component(c) => (&c.attrs, c.span),
        IrNode::Special(s) => (&s.attrs, s.span),
        // A `<slot>` element: official's slot-ATTRIBUTE placement validation
        // never runs for a `SlotElement` host (the analyze `SlotElement`
        // visitor owns its whole attribute disposition — a `slot="x"` on a
        // `<slot>` is accepted and DROPPED at emission; as a direct component
        // child it still routes the slot node into the parent's named region).
        IrNode::Slot(_) => return Ok(()),
        // No attribute surface — nothing can carry a `slot=`.
        IrNode::Text { .. }
        | IrNode::Comment { .. }
        | IrNode::Interpolation { .. }
        | IrNode::Block(_)
        | IrNode::Tag(_) => return Ok(()),
    };
    // A PLAIN-PROP host receives a non-direct `slot` as an ordinary prop (official
    // validates these with `is_component = true`).
    let plain_component_slot_prop_host = matches!(node, IrNode::Component(_))
        || matches!(node, IrNode::Special(s) if matches!(s.kind, SpecialKind::Component | SpecialKind::SelfRef));
    // A FILLER host can be routed into the parent's `$$slots` as a DIRECT static-slot
    // child (the plain-prop hosts plus the element family).
    let slot_filler_host = plain_component_slot_prop_host
        || matches!(node, IrNode::Element(_))
        || matches!(node, IrNode::Special(s) if s.kind == SpecialKind::Element);
    let direct_component_child = direct_slot_attr_child_hosts.contains(&node_id);
    let direct_snippet_child = direct_snippet_slot_attr_child_hosts.contains(&node_id);
    // The PLAIN-PROP acceptance requires a host with NO direct-placement owner at all
    // — neither a component parent nor a `{#snippet}` body (a direct snippet child
    // carrying a dynamic `slot` must REJECT, never leak through the plain-prop path).
    let plain_prop =
        plain_component_slot_prop_host && !direct_component_child && !direct_snippet_child;
    for attr in attrs {
        if let AttrIr::Dynamic { name, .. } | AttrIr::Mixed { name, .. } = attr {
            // A dynamic/mixed `slot` is accepted ONLY as an owner-less plain prop; on
            // a direct component child, a direct snippet child, or any element-family
            // host it is the official `slot_attribute_invalid` compile error.
            if name == "slot" && !plain_prop {
                return Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                    name: name.clone(),
                    span,
                });
            }
        }
        if let AttrIr::Static { name, value } = attr {
            if name == "slot" {
                let component_filler =
                    slot_filler_host && static_slot_filler_hosts.contains(&node_id);
                // A static TEXT-VALUED `slot` on a DIRECT snippet child is
                // component-owned placement on a filler-capable host kind — accepted
                // as a plain attr/prop on the host itself, NEVER routed into
                // `$$slots`. The text value is REQUIRED (official `is_text_attribute`):
                // a valueless/boolean `slot` (`<span slot>` / `<Inner slot/>`) is the
                // official `slot_attribute_invalid` reject — snippet membership
                // already disables the plain-prop path, so it falls through to the
                // typed refusal below. The value gate is SNIPPET-ONLY: the owner-less
                // Class B plain prop (top-level `<Inner slot/>` → `{slot: true}`) is
                // a genuine official accept and stays untouched, and the Class A
                // filler set is value-gated at lowering (`static_slot_name` only
                // records text-valued slots).
                let snippet_static = direct_snippet_child && slot_filler_host && value.is_some();
                if !(component_filler || snippet_static || plain_prop) {
                    return Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                        name: name.clone(),
                        span,
                    });
                }
            }
        }
    }
    Ok(())
}

/// Classify a `<slot>` element — the official analyze `SlotElement` rules
/// replayed over the typed attr inventory in SOURCE order (the first violation
/// errors, mirroring the official walk): a non-static `name` is
/// `slot_element_invalid_name`, `name="default"` is
/// `slot_element_invalid_name_default`, and any directive other than `let:` /
/// a spread is `slot_element_invalid_attribute`. Duplicate attributes are the
/// parse-level `attribute_duplicate` (upstream). A `<slot let:x>` (the
/// producer-side provider binding) is official-ACCEPTED syntax whose PINNED
/// official output is INVALID: svelte@5.56.3 emits a component-instance-scope
/// `$.derived_safe_equal(() => $$slotProps.x)` reading an UNBOUND `$$slotProps`
/// (bound only inside a component slot-content callback) — a guaranteed runtime
/// `ReferenceError`. Verter INTENTIONALLY refuses it on the attribute inventory
/// (an upstream-bug divergence) rather than emitting the unbound read.
pub(super) fn classify_slot_element(
    slot: &SlotElementIr,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    use super::official_rule::{CoreOfficialValidationRule, OfficialRejection};
    for attr in &slot.props {
        match attr {
            AttrIr::Static { name, value } if name == "name" => match value {
                Some(v) if v.value.as_str() == "default" => {
                    return Err(UnsupportedSvelteRuntimeSurface::OfficialReject {
                        rejection: OfficialRejection::of(
                            CoreOfficialValidationRule::SlotElementInvalidNameDefault,
                        ),
                        span: slot.span,
                    });
                }
                Some(_) => {}
                // A valueless `name` is not a text attribute — the same
                // official `slot_element_invalid_name`.
                None => {
                    return Err(UnsupportedSvelteRuntimeSurface::OfficialReject {
                        rejection: OfficialRejection::of(
                            CoreOfficialValidationRule::SlotElementInvalidName,
                        ),
                        span: slot.span,
                    });
                }
            },
            AttrIr::Dynamic { name, .. } | AttrIr::Mixed { name, .. } if name == "name" => {
                return Err(UnsupportedSvelteRuntimeSurface::OfficialReject {
                    rejection: OfficialRejection::of(
                        CoreOfficialValidationRule::SlotElementInvalidName,
                    ),
                    span: slot.span,
                });
            }
            // Plain attributes + spreads are the slot-prop surface.
            AttrIr::Static { .. }
            | AttrIr::Dynamic { .. }
            | AttrIr::Mixed { .. }
            | AttrIr::Spread { .. } => {}
            // `let:` is official-ACCEPTED; the fail-closed gate below owns it.
            AttrIr::Let { .. } => {}
            // Every other directive family (`class:` / `style:` / `bind:` /
            // `on:` / `use:` / `transition:` / `animate:` / `{@attach}`) —
            // the official `slot_element_invalid_attribute`. (An `on*`
            // ATTRIBUTE never lowers to `AttrIr::Event` on a slot host, so
            // an Event here is exactly the legacy `on:` directive form.)
            AttrIr::Class { .. }
            | AttrIr::Style { .. }
            | AttrIr::Bind { .. }
            | AttrIr::Event { .. }
            | AttrIr::Use { .. }
            | AttrIr::Transition { .. }
            | AttrIr::Animate { .. }
            | AttrIr::Attach { .. } => {
                return Err(UnsupportedSvelteRuntimeSurface::OfficialReject {
                    rejection: OfficialRejection::of(
                        CoreOfficialValidationRule::SlotElementInvalidAttribute,
                    ),
                    span: slot.span,
                });
            }
        }
    }
    // A `<slot let:x>` (the producer-side provider binding feeding
    // `slot_props`) is official-ACCEPTED, but the pinned official compiler
    // ITSELF emits an UNBOUND instance-level `$$slotProps` read for it — a
    // runtime `ReferenceError` (the degenerate-residue class this backend
    // refuses; the same disposition as the bare `$host()`). Refuse through the
    // DEDICATED `SlotLetUnbound` surface at the authored DIRECTIVE span,
    // failing closed on the ATTRIBUTE inventory (so a malformed alias that
    // decomposed to no binding still refuses).
    if let Some(span) = slot.props.iter().find_map(|a| match a {
        AttrIr::Let { span, .. } => Some(*span),
        _ => None,
    }) {
        return Err(UnsupportedSvelteRuntimeSurface::SlotLetUnbound { span });
    }
    Ok(())
}
