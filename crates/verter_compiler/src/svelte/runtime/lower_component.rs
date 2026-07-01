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

use super::attr_lowering::AttrHost;
use super::expr::{parse_let_alias_identifier, BindingInfo, BindingRuntimeKind, ScopeId};
use super::ir::{
    AttrIr, ComponentSlots, ExprId, HeadTitleIr, LetBinding, NamedSlot, NodeId, SpecialKind,
    TemplateScopeId, TitleChunkIr,
};
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

/// Split the dynamic-tag / component `this` selector out of a special element's
/// attribute list: REMOVE the attribute named `this` from `attrs` and return its
/// reactive expression (`<svelte:element this={tag}>` → `Some(tag)`). A STATIC
/// `this="div"` (a literal tag) is still removed from `attrs` (it is not a DOM
/// attribute) but carries no [`ExprId`], so `this_expr` is `None`. Any non-`this`
/// attribute / directive stays in `attrs`.
pub(super) fn extract_this_expr(
    attrs: Vec<AttrIr>,
) -> (Option<ExprId>, Option<String>, Vec<AttrIr>) {
    let mut this_expr = None;
    let mut static_tag = None;
    let mut kept = Vec::with_capacity(attrs.len());
    for attr in attrs {
        let is_this = match &attr {
            AttrIr::Static { name, .. }
            | AttrIr::Dynamic { name, .. }
            | AttrIr::Mixed { name, .. } => name == "this",
            _ => false,
        };
        if is_this {
            // Capture the reactive expression of a DYNAMIC `this={…}`, or the STATIC
            // `this="div"` tag literal (the runtime tag a `() => 'div'` thunk emits). A MIXED
            // `this="a{x}"` carries no single fact — the concatenated-tag form is out of the
            // §1.6 core (rare; a follow-up).
            match &attr {
                AttrIr::Dynamic { expr, .. } => this_expr = Some(*expr),
                AttrIr::Static { value: Some(v), .. } => static_tag = Some(v.value.clone()),
                _ => {}
            }
            // Drop the `this` attribute from the generic list either way.
            continue;
        }
        kept.push(attr);
    }
    (this_expr, static_tag, kept)
}

/// Map a parser element kind to the attribute host kind that decides how an `on*`
/// event lowers (the official `metadata.delegated` parent-kind rule). A regular
/// intrinsic element delegates; a component (incl. `<svelte:component>` /
/// `<svelte:self>`) forwards events as props; a `<svelte:element this={…}>` runs
/// them through `$.attribute_effect`; a window/body/document binds a direct global
/// listener; any other `<svelte:*>` falls through to the element-event path. An
/// unrecognised special (`Unknown`) records a diagnostic later, so the host here is
/// irrelevant (the node is dropped) — classify it as `OtherSpecial`.
pub(super) fn attr_host_for(kind: &SvelteElementKind) -> AttrHost {
    match kind {
        SvelteElementKind::Intrinsic | SvelteElementKind::NestedStyle => AttrHost::Element,
        SvelteElementKind::Component => AttrHost::Component,
        SvelteElementKind::Special(special) => match special {
            SvelteSpecialKind::Element => AttrHost::DynamicElement,
            SvelteSpecialKind::Window | SvelteSpecialKind::Document | SvelteSpecialKind::Body => {
                AttrHost::GlobalSpecial
            }
            // `<svelte:component this={C}>` / `<svelte:self>` are component hosts —
            // an `on*` forwards as a prop, exactly like a `<Foo onclick>`.
            SvelteSpecialKind::Component | SvelteSpecialKind::SelfRef => AttrHost::Component,
            SvelteSpecialKind::Head
            | SvelteSpecialKind::Options
            | SvelteSpecialKind::Boundary
            | SvelteSpecialKind::Fragment
            | SvelteSpecialKind::Unknown => AttrHost::OtherSpecial,
        },
    }
}

/// Map a parser special-element kind to the IR special kind. An unrecognised
/// `<svelte:*>` (the parser's `Unknown`) yields `None` so the caller records a
/// diagnostic rather than coercing the element to a fragment.
pub(super) fn lower_special_kind(kind: SvelteSpecialKind) -> Option<SpecialKind> {
    Some(match kind {
        SvelteSpecialKind::Head => SpecialKind::Head,
        SvelteSpecialKind::Window => SpecialKind::Window,
        SvelteSpecialKind::Document => SpecialKind::Document,
        SvelteSpecialKind::Body => SpecialKind::Body,
        SvelteSpecialKind::Element => SpecialKind::Element,
        SvelteSpecialKind::Boundary => SpecialKind::Boundary,
        SvelteSpecialKind::Options => SpecialKind::Options,
        SvelteSpecialKind::Component => SpecialKind::Component,
        SvelteSpecialKind::SelfRef => SpecialKind::SelfRef,
        SvelteSpecialKind::Fragment => SpecialKind::Fragment,
        SvelteSpecialKind::Unknown => return None,
    })
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

/// Lower a RENDERABLE-region special (`<svelte:element>` / `<svelte:boundary>`) child set into
/// its own body TEMPLATE SCOPE — the callback region the special emits its children into.
/// Returns the FULL child node-id list (the structural mirror), the slots (a boundary's
/// `{#snippet failed/pending}` children hoisted into `snippet_defs`, recognized by name — the
/// official boundary snippet rule; `<svelte:element>` has none), and the body region id.
pub(super) fn lower_renderable_special_region(
    ctx: &mut LoweringCtx,
    el: &SvelteElement,
    scope: ScopeId,
    is_boundary: bool,
) -> (Vec<NodeId>, ComponentSlots, Option<TemplateScopeId>) {
    let ts = ctx.push_template_scope(scope);
    let mut all_children = Vec::new();
    let mut body_roots = Vec::new();
    let mut snippet_defs = Vec::new();
    for child in &el.children {
        let is_boundary_snippet = is_boundary
            && matches!(
                child,
                SvelteNode::Block(b)
                    if matches!(
                        &b.kind,
                        SvelteBlockKind::Snippet { name_text, .. }
                            if name_text == "failed" || name_text == "pending"
                    )
            );
        if let Some(id) = lower_node(ctx, child, scope) {
            all_children.push(id);
            if is_boundary_snippet {
                snippet_defs.push(id);
            } else {
                body_roots.push(id);
            }
        }
    }
    ctx.template_scopes[ts.0 as usize].roots = body_roots;
    let slots = ComponentSlots {
        snippet_defs,
        ..ComponentSlots::default()
    };
    (all_children, slots, Some(ts))
}

/// Lower a `<svelte:head>`'s children into its body TEMPLATE SCOPE, SEPARATING the `<title>`
/// child (the official `SvelteHead` fragment + `TitleElement` split). The `<title>` renders no
/// DOM node — it drives `$.document.title` — so its fragment is decomposed into
/// [`TitleChunkIr`]s (returned as [`HeadTitleIr`]) and is NOT a body-region root (nor a walked
/// node, so it produces no reactive-text op). Every other child (`<meta>` / `<link>` / a text
/// run / …) is the body region — the `$.from_html` + `$.append` content INSIDE the `$.head`
/// callback. Returns the FULL body child node-id list (the structural mirror), the body region
/// id, and the decomposed title (when present).
pub(super) fn lower_head_region(
    ctx: &mut LoweringCtx,
    el: &SvelteElement,
    scope: ScopeId,
) -> (Vec<NodeId>, Option<TemplateScopeId>, Option<HeadTitleIr>) {
    let ts = ctx.push_template_scope(scope);
    let mut all_children = Vec::new();
    let mut body_roots = Vec::new();
    let mut head_title = None;
    for child in &el.children {
        // The `<title>` child is the special one: decompose its fragment into title chunks and
        // keep it OUT of the body-region DOM. A later `<title>` (a degenerate multi-title head)
        // overwrites — svelte's runtime is likewise last-write-wins on `document.title`.
        if let SvelteNode::Element(child_el) = child {
            if is_head_title_element(child_el) {
                head_title = Some(lower_title_chunks(ctx, child_el, scope));
                continue;
            }
        }
        if let Some(id) = lower_node(ctx, child, scope) {
            all_children.push(id);
            body_roots.push(id);
        }
    }
    ctx.template_scopes[ts.0 as usize].roots = body_roots;
    (all_children, Some(ts), head_title)
}

/// Whether a parsed child element is a `<svelte:head>`'s special `<title>` — an INTRINSIC
/// element named `title` (a component `<Title>` is not the head-title, and a `<svelte:*>`
/// special never is). Structural over the parsed element kind + tag, never a source scan.
fn is_head_title_element(el: &SvelteElement) -> bool {
    matches!(el.kind, SvelteElementKind::Intrinsic) && el.name == "title"
}

/// Decompose a `<title>`'s fragment into its ordered [`TitleChunkIr`]s (the official
/// `TitleElement` reads `node.fragment.nodes`): a text run is a literal chunk, an `{expr}`
/// interpolation an expression chunk (interned into the shared expression arena in the head's
/// scope, so the projector rewrites + evaluates it through the same rail every interpolation
/// uses). A `<title>` legally contains only text + interpolations (svelte's `build_template_chunk`
/// asserts `Array<Text | ExpressionTag>`); any other child is ignored.
fn lower_title_chunks(ctx: &mut LoweringCtx, el: &SvelteElement, scope: ScopeId) -> HeadTitleIr {
    let mut chunks = Vec::new();
    // The official `TitleElement` analyze visitor errors `title_invalid_content` on the FIRST
    // title child that is neither `Text` nor `ExpressionTag` (a nested element / comment /
    // block). Record its span so the surface classifier fails the head closed rather than
    // SILENTLY dropping the unsupported child.
    let mut invalid_content = None;
    for child in &el.children {
        match child {
            SvelteNode::Text(span) => {
                chunks.push(TitleChunkIr::Text(span_text(ctx.source, *span).to_string()));
            }
            SvelteNode::Interpolation(span) => {
                chunks.push(TitleChunkIr::Expr(ctx.push_expr(*span, scope)));
            }
            // A nested element / comment / block / standalone tag is not valid `<title>`
            // content — record the first offender's span (never silently skipped).
            other => {
                if invalid_content.is_none() {
                    invalid_content = Some(match other {
                        SvelteNode::Comment(span) => *span,
                        SvelteNode::Element(child_el) => child_el.open_span,
                        SvelteNode::Block(block) => block.span,
                        SvelteNode::Tag(tag) => tag.span,
                        // Text / Interpolation handled above.
                        SvelteNode::Text(span) | SvelteNode::Interpolation(span) => *span,
                    });
                }
            }
        }
    }
    HeadTitleIr {
        chunks,
        invalid_content,
    }
}
