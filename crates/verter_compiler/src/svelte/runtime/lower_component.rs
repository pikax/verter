//! The component-family SLOT-DECOMPOSITION lowering — the official `Component.js`
//! child grouping, extracted from `mod.rs` to keep the lowering module under the
//! file-size guard.
//!
//! A `<Foo>` component / `<svelte:component>` / `<svelte:self>` / `<svelte:fragment>`
//! decomposes its children into SLOT regions: a `{#snippet}` def becomes a hoisted
//! local const + shorthand prop, a `slot="x"`-bearing child becomes a NAMED slot
//! region, and everything else becomes the DEFAULT slot region. Each slot region is its
//! OWN lexical scope (a child of the component's scope) so its `let:` slot-prop bindings
//! (lowered as `Derived` so a read emits `$.get`) shadow correctly.

use super::expr::{parse_let_alias_identifier, BindingInfo, BindingRuntimeKind, ScopeId};
use super::ir::{ComponentSlots, LetBinding, NamedSlot, NodeId, TemplateScopeId};
use super::{lower_children_in_scope, lower_node, span_text, LoweringCtx};
use crate::svelte::parser::{
    SvelteAttributeKind, SvelteAttributeValue, SvelteBlockKind, SvelteDirectiveKind, SvelteElement,
    SvelteElementKind, SvelteNode, SvelteSpecialKind,
};

/// Decompose a component-family node's children into SLOT regions. Returns the FULL
/// child node-id list (the structural mirror) + the slots.
pub(super) fn lower_component_slots(
    ctx: &mut LoweringCtx,
    el: &SvelteElement,
    scope: ScopeId,
) -> (Vec<NodeId>, ComponentSlots) {
    let source = ctx.source;
    // The component's own `let:` directives apply to the DEFAULT slot (the official
    // `slot_scope_applies_to_itself = false` path): the shorthand `let:item` and the
    // simple-identifier alias `let:item={alias}` decompose here; an unsupported form sets
    // `has_unsupported_let` so the projection fails closed (never a silent drop).
    let (default_lets, mut has_unsupported_let) = let_directive_bindings(el, source);

    let mut all_children = Vec::new();
    let mut snippet_defs = Vec::new();
    let mut default_nodes: Vec<SvelteNode> = Vec::new();
    // Named-slot groups in first-seen order: (name, content nodes, the slot's own `let:`).
    let mut named_groups: Vec<(String, Vec<SvelteNode>, Vec<LetBinding>)> = Vec::new();

    for child in &el.children {
        // (1) A `{#snippet}` DEF declared directly inside the component — hoist it (lower
        // in the component's `scope` so its name binds for sibling `{@render}`), and pass
        // it as a shorthand prop.
        if let SvelteNode::Block(block) = child {
            if matches!(block.kind, SvelteBlockKind::Snippet { .. }) {
                if let Some(id) = lower_node(ctx, child, scope) {
                    all_children.push(id);
                    snippet_defs.push(id);
                }
                continue;
            }
        }
        // (2) A `slot="x"`-bearing child (a `<svelte:fragment slot="x">` or any element)
        // is a NAMED slot; group it (carrying its OWN `let:` bindings). A
        // `<svelte:fragment slot>` is TRANSPARENT — its CHILDREN are the slot content (the
        // fragment renders nothing itself); a regular `slot=`-bearing element IS the slot
        // content.
        if let SvelteNode::Element(child_el) = child {
            if let Some(slot_name) = static_slot_name(child_el, source) {
                let (child_lets, child_unsupported) = let_directive_bindings(child_el, source);
                has_unsupported_let |= child_unsupported;
                let content: Vec<SvelteNode> = if matches!(
                    child_el.kind,
                    SvelteElementKind::Special(SvelteSpecialKind::Fragment)
                ) {
                    child_el.children.clone()
                } else {
                    vec![child.clone()]
                };
                match named_groups.iter_mut().find(|(n, _, _)| *n == slot_name) {
                    Some((_, nodes, _)) => nodes.extend(content),
                    None => named_groups.push((slot_name, content, child_lets)),
                }
                continue;
            }
        }
        // (3) Everything else is DEFAULT-slot content.
        default_nodes.push(child.clone());
    }

    // The DEFAULT slot region (only when it has non-whitespace content — an
    // all-whitespace default with no `let:` produces no `children` prop).
    let default = if default_slot_has_content(source, &default_nodes) {
        let region = lower_slot_region(ctx, &default_nodes, scope, &default_lets);
        // The default region's roots are ALSO part of the structural child list.
        all_children.extend(ctx.template_scopes[region.0 as usize].roots.iter().copied());
        Some(region)
    } else {
        None
    };

    // The NAMED slot regions, in first-seen order.
    let mut named = Vec::with_capacity(named_groups.len());
    for (name, nodes, lets) in named_groups {
        let region = lower_slot_region(ctx, &nodes, scope, &lets);
        all_children.extend(ctx.template_scopes[region.0 as usize].roots.iter().copied());
        // Carry the slot's `let:` bindings on the plan (the plan-time fact) so the emitter
        // consumes them directly instead of rescanning the IR / binding table.
        named.push(NamedSlot { name, region, lets });
    }

    (
        all_children,
        ComponentSlots {
            default,
            default_lets,
            named,
            snippet_defs,
            has_unsupported_let,
        },
    )
}

/// Lower a slot's content nodes into a fresh template-scope region under a NEW lexical
/// slot scope (a child of `parent_scope`), declaring its `let:` slot props as `Derived`
/// bindings FIRST (so a read inside the slot emits `$.get(item)`).
fn lower_slot_region(
    ctx: &mut LoweringCtx,
    children: &[SvelteNode],
    parent_scope: ScopeId,
    lets: &[LetBinding],
) -> TemplateScopeId {
    let slot_scope = ctx.scopes.push_scope(Some(parent_scope));
    for binding in lets {
        let id = ctx.bindings.push(BindingInfo {
            name: binding.name.clone(),
            scope: slot_scope,
            kind: BindingRuntimeKind::Derived,
            state: None,
        });
        ctx.scopes.declare(slot_scope, &binding.name, id);
    }
    lower_children_in_scope(ctx, children, slot_scope)
}

/// Decompose an element's `let:` slot-prop directives into [`LetBinding`]s, read directly
/// from the PARSED directive inventory (used for BOTH a component's own default-slot lets
/// and a named-slot child's lets). Each directive is one of:
///
/// - the SHORTHAND `let:item` — the slot prop binds a same-named local (`key == name`);
/// - the simple-identifier ALIAS `let:item={alias}` — renames the slot prop `item` to the
///   local `alias` (`key = item`, `name = alias`);
/// - an UNSUPPORTED form — a destructuring / non-identifier alias (`let:item={{a, b}}`) or a
///   quoted-text / mixed value.
///
/// Returns the decomposed bindings PLUS whether any directive used an unsupported form. The
/// let decomposition is infallible here, so an unsupported form sets the flag (consumed by
/// the fallible component projection, which fails CLOSED) rather than being silently dropped.
fn let_directive_bindings(el: &SvelteElement, source: &str) -> (Vec<LetBinding>, bool) {
    let mut out = Vec::new();
    let mut unsupported = false;
    for a in &el.attributes {
        let SvelteAttributeKind::Directive(d) = &a.kind else {
            continue;
        };
        if d.kind != SvelteDirectiveKind::Let {
            continue;
        }
        match &d.value {
            // Shorthand `let:item` — the slot prop binds a same-named local.
            None => out.push(LetBinding {
                name: d.local.clone(),
                key: d.local.clone(),
            }),
            // `let:item={alias}` — the `{expr}` value is a binding pattern; ONLY a bare
            // identifier renames the slot prop `item` to the local `alias`. Parsed via the
            // shared pattern parser checking the NODE KIND (no text scan), so a destructuring
            // pattern (`{ a }` / `[a]`, even single-name), a multi-name list, or an
            // unparseable value yields the unsupported flag rather than a wrong binding.
            Some(SvelteAttributeValue::Expression(span)) => {
                match parse_let_alias_identifier(span_text(source, *span)) {
                    Some(name) => out.push(LetBinding {
                        name,
                        key: d.local.clone(),
                    }),
                    None => unsupported = true,
                }
            }
            // A quoted-text / mixed value is not a valid `let:` slot-prop form.
            Some(_) => unsupported = true,
        }
    }
    (out, unsupported)
}

/// The STATIC `slot="x"` name on a parsed element, or `None` (the official
/// `determine_slot`: a plain `slot` attribute with a text value).
fn static_slot_name(el: &SvelteElement, source: &str) -> Option<String> {
    el.attributes.iter().find_map(|a| match &a.kind {
        SvelteAttributeKind::Plain {
            name,
            value: Some(SvelteAttributeValue::Text(span)),
            ..
        } if name == "slot" => Some(span_text(source, *span).to_string()),
        _ => None,
    })
}

/// Whether a default-slot node run carries any RENDERABLE content (a non-whitespace
/// text, an element, an interpolation, a block, or a render/html tag) — an
/// all-whitespace / comment-only run produces no `children` prop (the official
/// `block.body.length === 0` skip).
fn default_slot_has_content(source: &str, nodes: &[SvelteNode]) -> bool {
    nodes.iter().any(|n| match n {
        // Significant only when the text run is not pure ASCII whitespace.
        SvelteNode::Text(span) => !span_text(source, *span)
            .chars()
            .all(|c| c.is_ascii_whitespace()),
        SvelteNode::Comment(_) => false,
        // An element / interpolation / block / tag is always renderable content.
        _ => true,
    })
}
