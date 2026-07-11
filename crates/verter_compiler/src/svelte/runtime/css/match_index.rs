//! The template-neighborhood half of the selector-to-template matcher: the
//! read-only [`TemplateIndex`] built in ONE downward walk over the runtime IR
//! (per-node `path`, per-fragment ordered node lists, snippet⇄site links, the
//! element inventory), the official existence tri-state
//! (`NODE_PROBABLY_EXISTS` / `NODE_DEFINITELY_EXISTS`, `higher_existence` =
//! max, the exhaustive downgrade on absent branches), and the
//! `css-prune.js` DOM-neighborhood helpers (`get_ancestor_elements`,
//! `get_descendant_elements`, `get_element_parent`,
//! `get_possible_element_siblings`, `get_possible_nested_siblings`,
//! `loop_child`).

use rustc_hash::{FxHashMap, FxHashSet};
use verter_span::Span;

use super::values::{expression_attr_shape, ExprAttrShape};
use super::{MatchResult, MatcherRefusal};
use crate::svelte::runtime::expr::{BindingRuntimeKind, RenderCalleeShape};
use crate::svelte::runtime::ir::{
    AttrIr, BindingId, BlockIr, ComponentSlots, EventOrigin, IrNode, MixedAttrPart, NodeId,
    RenderCallee, SpecialKind, SvelteRuntimeIr, TagIr, TemplateScopeId,
};

/// The official `FORWARD` / `BACKWARD` walk directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    Forward,
    Backward,
}

/// The official `NODE_PROBABLY_EXISTS` (0) / `NODE_DEFINITELY_EXISTS` (1)
/// tri-state existence values; `higher_existence` is the max.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Existence {
    Probably,
    Definitely,
}

/// The official `higher_existence(exist1, exist2)` — max, `undefined` loses.
fn higher_existence(exist1: Existence, exist2: Option<Existence>) -> Existence {
    match exist2 {
        None => exist1,
        Some(exist2) => exist1.max(exist2),
    }
}

/// An insertion-ordered `Map<node, NodeExistsValue>` (the official JS `Map`
/// preserves insertion order; `set` on an existing key keeps its position).
#[derive(Default)]
pub(super) struct NodeExistMap {
    entries: Vec<(NodeId, Existence)>,
}

impl NodeExistMap {
    fn set(&mut self, node: NodeId, exist: Existence) {
        if let Some(entry) = self.entries.iter_mut().find(|(n, _)| *n == node) {
            entry.1 = exist;
        } else {
            self.entries.push((node, exist));
        }
    }

    fn get(&self, node: NodeId) -> Option<Existence> {
        self.entries
            .iter()
            .find(|(n, _)| *n == node)
            .map(|(_, e)| *e)
    }

    /// The `(node, existence)` entries in insertion order — the sibling walk
    /// reads the existence alongside the node (a PROBABLY sibling relation
    /// caps the match certainty; the official walk iterated `keys()` and
    /// ignored the values).
    pub(super) fn entries(&self) -> impl Iterator<Item = (NodeId, Existence)> + '_ {
        self.entries.iter().copied()
    }

    /// The official `add_to_map(from, to)` — merge via `higher_existence`.
    fn merge_into(&self, to: &mut NodeExistMap) {
        for (node, exist) in &self.entries {
            let merged = higher_existence(*exist, to.get(*node));
            to.set(*node, merged);
        }
    }

    /// The official non-exhaustive downgrade: every value becomes PROBABLY.
    fn downgrade_all(&mut self) {
        for entry in &mut self.entries {
            entry.1 = Existence::Probably;
        }
    }
}

/// The official `has_definite_elements(result)`.
pub(super) fn has_definite_elements(map: &NodeExistMap) -> bool {
    map.entries
        .iter()
        .any(|(_, exist)| *exist == Existence::Definitely)
}

// ─────────────────────────────────────────────────────────────────────────────
// The template neighborhood index (built in ONE downward walk from
// `root_scope().roots` — ir.rs exposes no parent/sibling/path accessor).
// ─────────────────────────────────────────────────────────────────────────────

/// One entry of a node's ancestor `path` — the official `metadata.path`
/// interleaves Fragment nodes with their container nodes; the index mirrors
/// that shape with fragment ids and node ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathEntry {
    /// A fragment (a template-scope root list or a container's child list).
    Frag(usize),
    /// A container node (element / component / special / block / snippet).
    Node(NodeId),
}

/// How a `{@render}` / component-family renderer resolves to snippets before
/// the post-walk site linking (the official `snippet_renderers` map).
enum RendererPlan {
    /// Unambiguously one local `{#snippet}` (by name binding).
    KnownSnippet(BindingId),
    /// Resolved to an EXTERNAL snippet (prop / import / undeclared) — links
    /// no local snippet.
    External,
    /// Cannot be resolved — links EVERY local snippet (the official
    /// conservative fallback).
    Unresolved,
    /// A resolved component: the attr-resolved snippet name bindings plus the
    /// direct `{#snippet}` children.
    ComponentResolved(Vec<BindingId>, Vec<NodeId>),
}

/// The read-only template neighborhood index over one [`SvelteRuntimeIr`].
pub(super) struct TemplateIndex<'ir, 'src> {
    pub(super) ir: &'ir SvelteRuntimeIr<'src>,
    /// Ordered node list per fragment.
    frags: Vec<Vec<NodeId>>,
    /// Template scope → fragment id.
    frag_of_scope: FxHashMap<u32, usize>,
    /// Container node → its child-list fragment id.
    frag_of_children: FxHashMap<NodeId, usize>,
    /// Per-node ancestor path (root→parent), indexed by the dense node arena.
    paths: Vec<Vec<PathEntry>>,
    /// Every matchable element (`RegularElement` + `SvelteElement`) in
    /// document order — the official `analysis.elements` iterable.
    pub(super) elements: Vec<NodeId>,
    /// Every `{#snippet}` block in document order — the official
    /// `analysis.snippets`.
    all_snippets: Vec<NodeId>,
    /// Renderer → the snippets it may render (`metadata.snippets`).
    snippets_of_renderer: FxHashMap<NodeId, Vec<NodeId>>,
    /// Snippet → the renderers that may render it (`metadata.sites`).
    sites_of_snippet: FxHashMap<NodeId, Vec<NodeId>>,
    /// The style-body span — the unprovable anchor for constructs that carry
    /// no span of their own.
    pub(super) fallback_span: Span,
}

impl<'ir, 'src> TemplateIndex<'ir, 'src> {
    pub(super) fn build(ir: &'ir SvelteRuntimeIr<'src>, fallback_span: Span) -> MatchResult<Self> {
        let mut index = Self {
            ir,
            frags: Vec::new(),
            frag_of_scope: FxHashMap::default(),
            frag_of_children: FxHashMap::default(),
            paths: vec![Vec::new(); ir.nodes.len()],
            elements: Vec::new(),
            all_snippets: Vec::new(),
            snippets_of_renderer: FxHashMap::default(),
            sites_of_snippet: FxHashMap::default(),
            fallback_span,
        };
        let mut snippet_of_binding: FxHashMap<u32, NodeId> = FxHashMap::default();
        let mut renderer_plans: Vec<(NodeId, RendererPlan)> = Vec::new();
        let root_frag = index.mint_scope_frag(ir.root);
        let mut path = vec![PathEntry::Frag(root_frag)];
        let roots = ir.root_scope().roots.clone();
        index.walk_nodes(
            &roots,
            &mut path,
            &mut snippet_of_binding,
            &mut renderer_plans,
        )?;
        index.link_snippets(&snippet_of_binding, renderer_plans)?;
        Ok(index)
    }

    fn mint_scope_frag(&mut self, scope: TemplateScopeId) -> usize {
        let idx = self.frags.len();
        self.frags.push(self.ir.template_scope(scope).roots.clone());
        self.frag_of_scope.insert(scope.0, idx);
        idx
    }

    fn mint_children_frag(&mut self, node: NodeId, children: &[NodeId]) -> usize {
        let idx = self.frags.len();
        self.frags.push(children.to_vec());
        self.frag_of_children.insert(node, idx);
        idx
    }

    /// The ONE downward walk: record each node's path, mint fragments, and
    /// collect the element / snippet / renderer inventories.
    fn walk_nodes(
        &mut self,
        nodes: &[NodeId],
        path: &mut Vec<PathEntry>,
        snippet_of_binding: &mut FxHashMap<u32, NodeId>,
        renderer_plans: &mut Vec<(NodeId, RendererPlan)>,
    ) -> MatchResult<()> {
        for &id in nodes {
            self.paths[id.0 as usize] = path.clone();
            match self.ir.node(id) {
                IrNode::Text { .. } | IrNode::Comment { .. } | IrNode::Interpolation { .. } => {}
                IrNode::Element(el) => {
                    if el.tag == "slot" {
                        // The official `SlotElement` has block semantics
                        // (fragment, PROBABLY existence); the IR carries a
                        // plain intrinsic element — not provable.
                        return Err(MatcherRefusal::at(
                            el.span,
                            "a legacy `<slot>` element (official `SlotElement` block semantics are not represented in the runtime IR)",
                        ));
                    }
                    self.elements.push(id);
                    let frag = self.mint_children_frag(id, &el.children);
                    let children = el.children.clone();
                    self.walk_container(
                        id,
                        frag,
                        &children,
                        path,
                        snippet_of_binding,
                        renderer_plans,
                    )?;
                }
                IrNode::Component(component) => {
                    self.check_component_family(id, component.span, &component.slots)?;
                    renderer_plans.push((
                        id,
                        self.plan_component_renderer(&component.attrs, &component.slots),
                    ));
                    let frag = self.mint_children_frag(id, &component.children);
                    let children = component.children.clone();
                    self.walk_container(
                        id,
                        frag,
                        &children,
                        path,
                        snippet_of_binding,
                        renderer_plans,
                    )?;
                }
                IrNode::Special(special) => {
                    match special.kind {
                        SpecialKind::Component | SpecialKind::SelfRef => {
                            self.check_component_family(id, special.span, &special.slots)?;
                            renderer_plans.push((
                                id,
                                self.plan_component_renderer(&special.attrs, &special.slots),
                            ));
                        }
                        SpecialKind::Element => {
                            self.elements.push(id);
                        }
                        SpecialKind::Head => {
                            if special.head_title.is_some() {
                                // The official AST keeps the `<title>` as a
                                // RegularElement inside the head fragment; the
                                // IR decomposes it into title chunks — its
                                // neighborhood is not reconstructible.
                                return Err(MatcherRefusal::at(
                                    special.span,
                                    "a `<svelte:head>` `<title>` (decomposed out of the runtime IR fragment)",
                                ));
                            }
                        }
                        SpecialKind::Window
                        | SpecialKind::Document
                        | SpecialKind::Body
                        | SpecialKind::Boundary
                        | SpecialKind::Options
                        | SpecialKind::Fragment => {}
                    }
                    let frag = self.mint_children_frag(id, &special.children);
                    let children = special.children.clone();
                    self.walk_container(
                        id,
                        frag,
                        &children,
                        path,
                        snippet_of_binding,
                        renderer_plans,
                    )?;
                }
                IrNode::Block(block) => match block {
                    BlockIr::If { branches } => {
                        let bodies: Vec<TemplateScopeId> =
                            branches.iter().map(|branch| branch.body).collect();
                        for body in bodies {
                            self.walk_scope(id, body, path, snippet_of_binding, renderer_plans)?;
                        }
                    }
                    BlockIr::Each {
                        body, else_body, ..
                    } => {
                        let (body, else_body) = (*body, *else_body);
                        self.walk_scope(id, body, path, snippet_of_binding, renderer_plans)?;
                        if let Some(else_body) = else_body {
                            self.walk_scope(
                                id,
                                else_body,
                                path,
                                snippet_of_binding,
                                renderer_plans,
                            )?;
                        }
                    }
                    BlockIr::Await {
                        pending,
                        then_body,
                        catch_body,
                        ..
                    } => {
                        let scopes = [*pending, *then_body, *catch_body];
                        for scope in scopes.into_iter().flatten() {
                            self.walk_scope(id, scope, path, snippet_of_binding, renderer_plans)?;
                        }
                    }
                    BlockIr::Key { body, .. } => {
                        let body = *body;
                        self.walk_scope(id, body, path, snippet_of_binding, renderer_plans)?;
                    }
                    BlockIr::Snippet { name, body, .. } => {
                        self.all_snippets.push(id);
                        snippet_of_binding.insert(name.0, id);
                        let body = *body;
                        self.walk_scope(id, body, path, snippet_of_binding, renderer_plans)?;
                    }
                },
                IrNode::Tag(tag) => match tag {
                    TagIr::Render {
                        callee,
                        spread_arg_span,
                        ..
                    } => {
                        if let Some(span) = spread_arg_span {
                            // Official hard-errors on a `{@render}` spread
                            // argument — no official facts exist for it.
                            return Err(MatcherRefusal::at(
                                *span,
                                "a `{@render}` spread argument (an official compile error)",
                            ));
                        }
                        renderer_plans.push((id, self.plan_render_renderer(callee)?));
                    }
                    TagIr::Html { .. }
                    | TagIr::LegacyConst { .. }
                    | TagIr::Declaration { .. }
                    | TagIr::Debug { .. }
                    | TagIr::Attach { .. } => {}
                },
            }
        }
        Ok(())
    }

    /// Descend into a container node's child-list fragment (path grows by
    /// `[Node(container), Frag(children)]`).
    fn walk_container(
        &mut self,
        container: NodeId,
        frag: usize,
        children: &[NodeId],
        path: &mut Vec<PathEntry>,
        snippet_of_binding: &mut FxHashMap<u32, NodeId>,
        renderer_plans: &mut Vec<(NodeId, RendererPlan)>,
    ) -> MatchResult<()> {
        path.push(PathEntry::Node(container));
        path.push(PathEntry::Frag(frag));
        let result = self.walk_nodes(children, path, snippet_of_binding, renderer_plans);
        path.pop();
        path.pop();
        result
    }

    /// Descend into a block's body template scope (path grows by
    /// `[Node(block), Frag(scope roots)]`).
    fn walk_scope(
        &mut self,
        container: NodeId,
        scope: TemplateScopeId,
        path: &mut Vec<PathEntry>,
        snippet_of_binding: &mut FxHashMap<u32, NodeId>,
        renderer_plans: &mut Vec<(NodeId, RendererPlan)>,
    ) -> MatchResult<()> {
        let frag = self.mint_scope_frag(scope);
        let roots = self.frags[frag].clone();
        path.push(PathEntry::Node(container));
        path.push(PathEntry::Frag(frag));
        let result = self.walk_nodes(&roots, path, snippet_of_binding, renderer_plans);
        path.pop();
        path.pop();
        result
    }

    /// Fail closed on component-family shapes whose lowered child list cannot
    /// reproduce the official source fragment (order or boundary).
    fn check_component_family(
        &self,
        id: NodeId,
        span: Span,
        slots: &ComponentSlots,
    ) -> MatchResult<()> {
        if !slots.named.is_empty() {
            // Named-slot regions are appended after the default region — the
            // lowered child order no longer reproduces the official source
            // fragment order the sibling scans depend on.
            return Err(MatcherRefusal::at(
                span,
                "a named-slot filler on a component (lowered child order diverges from the source fragment)",
            ));
        }
        if slots.has_duplicate_slot || slots.has_default_slot_conflict || slots.has_unsupported_let
        {
            // Official compile-errors — no official matcher facts exist.
            return Err(MatcherRefusal::at(
                span,
                "a component slot composition the official compiler rejects",
            ));
        }
        let children = match self.ir.node(id) {
            IrNode::Component(component) => &component.children,
            IrNode::Special(special) => &special.children,
            _ => return Ok(()),
        };
        for child in children {
            let is_direct = self.ir.direct_slot_attr_child_hosts.contains(child);
            let is_snippet_def = slots.snippet_defs.contains(child);
            if !is_direct && !is_snippet_def {
                // A `<svelte:fragment slot>`'s hoisted children: the fragment
                // node (the official climb-out boundary) is erased from the
                // IR, so the hoisted nodes would see phantom siblings.
                return Err(MatcherRefusal::at(
                    span,
                    "`<svelte:fragment slot>` hoisted slot content (the official fragment boundary is erased)",
                ));
            }
        }
        Ok(())
    }

    /// The official `RenderTag` snippet resolution (`RenderTag.js` +
    /// `is_resolved_snippet`).
    fn plan_render_renderer(&self, callee: &RenderCallee) -> MatchResult<RendererPlan> {
        match callee {
            RenderCallee::Snippet { binding, .. } => Ok(RendererPlan::KnownSnippet(*binding)),
            RenderCallee::Dynamic(expr) => {
                let analyzed = self.ir.analysis.expressions.get(*expr);
                // The stored render-callee fact — classified ONCE by the same
                // parse that analyzed the expression (never a reparse here).
                match &analyzed.render_callee {
                    Ok(RenderCalleeShape::StaticName { name, .. }) => {
                        let binding = self.ir.analysis.scopes.resolve(
                            &self.ir.analysis.bindings,
                            analyzed.scope,
                            name,
                        );
                        Ok(self.classify_identifier_renderer(binding))
                    }
                    Ok(RenderCalleeShape::Dynamic { .. }) => Ok(RendererPlan::Unresolved),
                    // A spread argument is caught on the node fact; a torn
                    // expression means the lowering already failed — both
                    // defensive here.
                    Ok(RenderCalleeShape::SpreadArguments) | Err(()) => Err(MatcherRefusal::at(
                        self.fallback_span,
                        "an unresolvable `{@render}` callee expression",
                    )),
                }
            }
        }
    }

    /// The official identifier-callee classification: `!binding` /
    /// import / prop family ⇒ resolved-external; a `{#snippet}` name ⇒ that
    /// snippet; anything else ⇒ unresolved (link all).
    fn classify_identifier_renderer(&self, binding: Option<BindingId>) -> RendererPlan {
        match binding {
            None => RendererPlan::External,
            Some(binding) => match self.ir.analysis.bindings.get(binding).kind {
                BindingRuntimeKind::SnippetName => RendererPlan::KnownSnippet(binding),
                BindingRuntimeKind::Prop
                | BindingRuntimeKind::BindableProp
                | BindingRuntimeKind::ComponentImport
                | BindingRuntimeKind::ImportedValue => RendererPlan::External,
                _ => RendererPlan::Unresolved,
            },
        }
    }

    /// The official `visit_component` snippet resolution (`shared/component.js`):
    /// spreads / binds / non-identifier non-literal expression attributes make
    /// the component unresolved; identifier attributes fold
    /// `is_resolved_snippet`; a resolved component links its attr-resolved
    /// snippets plus its direct `{#snippet}` children.
    fn plan_component_renderer(&self, attrs: &[AttrIr], slots: &ComponentSlots) -> RendererPlan {
        let mut resolved = true;
        let mut known: Vec<BindingId> = Vec::new();
        for attr in attrs {
            let expr = match attr {
                AttrIr::Spread { .. } | AttrIr::Bind { .. } => {
                    resolved = false;
                    continue;
                }
                // The modern `on*` attribute is an official `Attribute` with
                // an expression value; the legacy `on:` directive is an
                // `OnDirective` (skipped).
                AttrIr::Event {
                    origin: EventOrigin::ModernAttribute,
                    handler,
                    ..
                } => Some(*handler),
                AttrIr::Event {
                    origin: EventOrigin::LegacyDirective,
                    ..
                } => None,
                AttrIr::Dynamic { expr, .. } => Some(*expr),
                // A quoted single-expression value (`foo="{bar}"`) is an
                // official expression attribute; any other mixed value is not.
                AttrIr::Mixed { parts, .. } => match parts.as_slice() {
                    [MixedAttrPart::Expr(expr)] => Some(*expr),
                    _ => None,
                },
                AttrIr::Static { .. }
                | AttrIr::Class { .. }
                | AttrIr::Style { .. }
                | AttrIr::Use { .. }
                | AttrIr::Transition { .. }
                | AttrIr::Animate { .. }
                | AttrIr::Attach { .. }
                | AttrIr::Let { .. } => None,
            };
            let Some(expr) = expr else { continue };
            let analyzed = self.ir.analysis.expressions.get(expr);
            match expression_attr_shape(&analyzed.matcher_expr) {
                ExprAttrShape::Identifier(name) => {
                    let binding = self.ir.analysis.scopes.resolve(
                        &self.ir.analysis.bindings,
                        analyzed.scope,
                        &name,
                    );
                    resolved &= is_resolved_snippet(self, binding);
                    if let Some(binding) = binding {
                        if self.ir.analysis.bindings.get(binding).kind
                            == BindingRuntimeKind::SnippetName
                        {
                            known.push(binding);
                        }
                    }
                }
                ExprAttrShape::Literal => {}
                ExprAttrShape::Other => resolved = false,
            }
        }
        if resolved {
            RendererPlan::ComponentResolved(known, slots.snippet_defs.clone())
        } else {
            RendererPlan::Unresolved
        }
    }

    /// The post-walk site linking — the official analysis tail:
    /// `snippet_renderers` unresolved ⇒ every local snippet; then every
    /// renderer registers as a site on each of its snippets.
    fn link_snippets(
        &mut self,
        snippet_of_binding: &FxHashMap<u32, NodeId>,
        renderer_plans: Vec<(NodeId, RendererPlan)>,
    ) -> MatchResult<()> {
        for (renderer, plan) in renderer_plans {
            let snippets: Vec<NodeId> = match plan {
                RendererPlan::KnownSnippet(binding) => {
                    let Some(&snippet) = snippet_of_binding.get(&binding.0) else {
                        return Err(MatcherRefusal::at(
                            self.fallback_span,
                            "a snippet-name binding without a `{#snippet}` declaration",
                        ));
                    };
                    vec![snippet]
                }
                RendererPlan::External => Vec::new(),
                RendererPlan::Unresolved => self.all_snippets.clone(),
                RendererPlan::ComponentResolved(bindings, defs) => {
                    let mut snippets: Vec<NodeId> = Vec::new();
                    for binding in bindings {
                        let Some(&snippet) = snippet_of_binding.get(&binding.0) else {
                            return Err(MatcherRefusal::at(
                                self.fallback_span,
                                "a snippet-name binding without a `{#snippet}` declaration",
                            ));
                        };
                        if !snippets.contains(&snippet) {
                            snippets.push(snippet);
                        }
                    }
                    for def in defs {
                        if !snippets.contains(&def) {
                            snippets.push(def);
                        }
                    }
                    snippets
                }
            };
            for &snippet in &snippets {
                let sites = self.sites_of_snippet.entry(snippet).or_default();
                if !sites.contains(&renderer) {
                    sites.push(renderer);
                }
            }
            self.snippets_of_renderer.insert(renderer, snippets);
        }
        Ok(())
    }

    // ── node-kind accessors ──────────────────────────────────────────────

    fn path(&self, node: NodeId) -> &[PathEntry] {
        &self.paths[node.0 as usize]
    }

    fn frag_nodes(&self, frag: usize) -> &[NodeId] {
        &self.frags[frag]
    }

    /// The child fragments a "visit everything" walk descends into (the
    /// zimmerframe `context.next()` over a template node).
    fn child_frags(&self, node: NodeId) -> Vec<usize> {
        match self.ir.node(node) {
            IrNode::Element(_) | IrNode::Component(_) | IrNode::Special(_) => self
                .frag_of_children
                .get(&node)
                .copied()
                .into_iter()
                .collect(),
            IrNode::Block(block) => {
                let scopes: Vec<TemplateScopeId> = match block {
                    BlockIr::If { branches } => branches.iter().map(|b| b.body).collect(),
                    BlockIr::Each {
                        body, else_body, ..
                    } => std::iter::once(*body).chain(*else_body).collect(),
                    BlockIr::Await {
                        pending,
                        then_body,
                        catch_body,
                        ..
                    } => [*pending, *then_body, *catch_body]
                        .into_iter()
                        .flatten()
                        .collect(),
                    BlockIr::Key { body, .. } | BlockIr::Snippet { body, .. } => vec![*body],
                };
                scopes
                    .into_iter()
                    .filter_map(|s| self.frag_of_scope.get(&s.0).copied())
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// `RegularElement` — the official type check.
    pub(super) fn is_regular_element(&self, node: NodeId) -> bool {
        matches!(self.ir.node(node), IrNode::Element(_))
    }

    /// `SvelteElement`.
    pub(super) fn is_svelte_element(&self, node: NodeId) -> bool {
        matches!(
            self.ir.node(node),
            IrNode::Special(sp) if sp.kind == SpecialKind::Element
        )
    }

    /// `RegularElement | SvelteElement` — the matchable element set.
    pub(super) fn is_matchable_element(&self, node: NodeId) -> bool {
        self.is_regular_element(node) || self.is_svelte_element(node)
    }

    /// `Component` — EXACTLY the official `Component` type
    /// (`SvelteComponent` / `SvelteSelf` deliberately excluded, as upstream).
    pub(super) fn is_component_node(&self, node: NodeId) -> bool {
        matches!(self.ir.node(node), IrNode::Component(_))
    }

    /// `Component | SvelteComponent | SvelteSelf` — the transparent climb-out
    /// family.
    fn is_component_family(&self, node: NodeId) -> bool {
        match self.ir.node(node) {
            IrNode::Component(_) => true,
            IrNode::Special(sp) => matches!(sp.kind, SpecialKind::Component | SpecialKind::SelfRef),
            _ => false,
        }
    }

    /// The official `is_block` — `IfBlock | EachBlock | AwaitBlock |
    /// KeyBlock | SlotElement` (`SnippetBlock` is NOT a block; the slot
    /// element never reaches the index).
    fn is_block_node(&self, node: NodeId) -> bool {
        matches!(
            self.ir.node(node),
            IrNode::Block(
                BlockIr::If { .. }
                    | BlockIr::Each { .. }
                    | BlockIr::Await { .. }
                    | BlockIr::Key { .. }
            )
        )
    }

    fn is_snippet_block(&self, node: NodeId) -> bool {
        matches!(self.ir.node(node), IrNode::Block(BlockIr::Snippet { .. }))
    }

    pub(super) fn is_render_tag(&self, node: NodeId) -> bool {
        matches!(self.ir.node(node), IrNode::Tag(TagIr::Render { .. }))
    }

    /// The element NAME (the official `element.name`): the intrinsic tag, or
    /// the literal `svelte:element` for a `SvelteElement` (never
    /// name-matched — the type-selector arm short-circuits on the kind).
    pub(super) fn element_name(&self, node: NodeId) -> &str {
        match self.ir.node(node) {
            IrNode::Element(el) => &el.tag,
            IrNode::Special(sp) if sp.kind == SpecialKind::Element => "svelte:element",
            _ => "",
        }
    }

    /// The element's attributes (empty for a non-element node).
    pub(super) fn attrs_of(&self, node: NodeId) -> &[AttrIr] {
        match self.ir.node(node) {
            IrNode::Element(el) => &el.attrs,
            IrNode::Special(sp) => &sp.attrs,
            IrNode::Component(c) => &c.attrs,
            _ => &[],
        }
    }

    /// The official `has_slot_attribute` scan — any `Attribute` named `slot`
    /// (static, dynamic, or mixed).
    fn has_slot_attribute(&self, node: NodeId) -> bool {
        self.attrs_of(node).iter().any(|attr| match attr {
            // The official compare is `attr.name.toLowerCase() === 'slot'`
            // (css-prune.js) — a FULL Unicode fold, not an ASCII fold.
            AttrIr::Static { name, .. }
            | AttrIr::Dynamic { name, .. }
            | AttrIr::Mixed { name, .. } => name.to_lowercase() == "slot",
            _ => false,
        })
    }

    fn snippets_of(&self, renderer: NodeId) -> &[NodeId] {
        self.snippets_of_renderer
            .get(&renderer)
            .map_or(&[], Vec::as_slice)
    }

    fn sites_of(&self, snippet: NodeId) -> &[NodeId] {
        self.sites_of_snippet
            .get(&snippet)
            .map_or(&[], Vec::as_slice)
    }

    fn each_body_frag(&self, node: NodeId) -> Option<usize> {
        match self.ir.node(node) {
            IrNode::Block(BlockIr::Each { body, .. }) => self.frag_of_scope.get(&body.0).copied(),
            _ => None,
        }
    }
}

/// The official `is_resolved_snippet(binding)` over the runtime binding
/// classification: no binding (undeclared/global), an import, a prop-family
/// binding, or a `{#snippet}` name binding.
fn is_resolved_snippet(index: &TemplateIndex<'_, '_>, binding: Option<BindingId>) -> bool {
    match binding {
        None => true,
        Some(binding) => matches!(
            index.ir.analysis.bindings.get(binding).kind,
            BindingRuntimeKind::Prop
                | BindingRuntimeKind::BindableProp
                | BindingRuntimeKind::ComponentImport
                | BindingRuntimeKind::ImportedValue
                | BindingRuntimeKind::SnippetName
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The DOM-neighborhood helpers (css-prune.js tail).
// ─────────────────────────────────────────────────────────────────────────────

/// The official `get_element_parent(node)` — the nearest element ancestor.
pub(super) fn get_element_parent(index: &TemplateIndex<'_, '_>, node: NodeId) -> Option<NodeId> {
    index.path(node).iter().rev().find_map(|entry| match entry {
        PathEntry::Node(parent) if index.is_matchable_element(*parent) => Some(*parent),
        _ => None,
    })
}

/// The official `get_ancestor_elements(node, adjacent_only, seen)` — walk the
/// path backward; a snippet parent recurses into its sites; `<option>` inside
/// `<select>` also yields the enclosing `<selectedcontent>`.
pub(super) fn get_ancestor_elements(
    index: &TemplateIndex<'_, '_>,
    node: NodeId,
    adjacent_only: bool,
    seen: &mut FxHashSet<NodeId>,
) -> Vec<NodeId> {
    let mut ancestors: Vec<NodeId> = Vec::new();
    let path = index.path(node);
    let mut i = path.len();
    while i > 0 {
        i -= 1;
        let PathEntry::Node(parent) = path[i] else {
            continue;
        };
        if index.is_snippet_block(parent) {
            if seen.insert(parent) {
                for &site in index.sites_of(parent) {
                    ancestors.extend(get_ancestor_elements(index, site, adjacent_only, seen));
                }
            }
            break;
        }
        if index.is_matchable_element(parent) {
            // `<option>` inside `<select>`: elements inside the option are
            // also descendants of `<selectedcontent>` (it clones the selected
            // option's content).
            if index.is_regular_element(parent) && index.element_name(parent) == "option" {
                let is_direct_child = ancestors.is_empty();
                let select_element = path[..i].iter().rev().find_map(|entry| match entry {
                    PathEntry::Node(n)
                        if index.is_regular_element(*n) && index.element_name(*n) == "select" =>
                    {
                        Some(*n)
                    }
                    _ => None,
                });
                if let Some(select) = select_element {
                    if !adjacent_only || is_direct_child {
                        let selectedcontent = find_first_selectedcontent(index, select);
                        if adjacent_only && is_direct_child {
                            if let Some(sc) = selectedcontent {
                                return vec![sc, parent];
                            }
                        } else if let Some(sc) = selectedcontent {
                            ancestors.push(sc);
                        }
                    }
                }
            }
            ancestors.push(parent);
            if adjacent_only {
                break;
            }
        }
    }
    ancestors
}

/// The official first-`<selectedcontent>` preorder search under a `<select>`
/// subtree (the `context.stop()` walk).
fn find_first_selectedcontent(index: &TemplateIndex<'_, '_>, node: NodeId) -> Option<NodeId> {
    if index.is_regular_element(node) && index.element_name(node) == "selectedcontent" {
        return Some(node);
    }
    for frag in index.child_frags(node) {
        for &child in index.frag_nodes(frag) {
            if let Some(found) = find_first_selectedcontent(index, child) {
                return Some(found);
            }
        }
    }
    None
}

/// The official `get_descendant_elements(node, adjacent_only, seen)` — walk
/// downward through every fragment; render tags descend their snippets'
/// bodies; a `<selectedcontent>` also yields the descendants of the enclosing
/// select's `<option>`s.
pub(super) fn get_descendant_elements(
    index: &TemplateIndex<'_, '_>,
    node: NodeId,
    adjacent_only: bool,
) -> Vec<NodeId> {
    let mut descendants: Vec<NodeId> = Vec::new();
    let mut seen: FxHashSet<NodeId> = FxHashSet::default();
    // `walk_children(node.type === 'RenderTag' ? node : node.fragment)`.
    if index.is_render_tag(node) {
        walk_descendant(index, node, adjacent_only, &mut seen, &mut descendants);
    } else {
        for frag in index.child_frags(node) {
            for &child in index.frag_nodes(frag) {
                walk_descendant(index, child, adjacent_only, &mut seen, &mut descendants);
            }
        }
    }
    if index.is_regular_element(node) && index.element_name(node) == "selectedcontent" {
        let select_element = index.path(node).iter().rev().find_map(|entry| match entry {
            PathEntry::Node(n)
                if index.is_regular_element(*n) && index.element_name(*n) == "select" =>
            {
                Some(*n)
            }
            _ => None,
        });
        if let Some(select) = select_element {
            walk_select_subtree(
                index,
                select,
                false,
                adjacent_only,
                &mut seen,
                &mut descendants,
            );
        }
    }
    descendants
}

/// The `walk_children` visitor of `get_descendant_elements`.
fn walk_descendant(
    index: &TemplateIndex<'_, '_>,
    node: NodeId,
    adjacent_only: bool,
    seen: &mut FxHashSet<NodeId>,
    out: &mut Vec<NodeId>,
) {
    if index.is_matchable_element(node) {
        out.push(node);
        if adjacent_only {
            return;
        }
        for frag in index.child_frags(node) {
            for &child in index.frag_nodes(frag) {
                walk_descendant(index, child, adjacent_only, seen, out);
            }
        }
    } else if index.is_render_tag(node) {
        for &snippet in index.snippets_of(node) {
            if !seen.insert(snippet) {
                continue;
            }
            // `walk_children(snippet.body)` — the body fragment's nodes.
            for frag in index.child_frags(snippet) {
                for &child in index.frag_nodes(frag) {
                    walk_descendant(index, child, adjacent_only, seen, out);
                }
            }
        }
    } else {
        for frag in index.child_frags(node) {
            for &child in index.frag_nodes(frag) {
                walk_descendant(index, child, adjacent_only, seen, out);
            }
        }
    }
}

/// The `<selectedcontent>` special walk over the enclosing `<select>` — an
/// `<option>` flips `inside_option`; inside an option every node's subtree is
/// collected via the regular descendant walk.
fn walk_select_subtree(
    index: &TemplateIndex<'_, '_>,
    node: NodeId,
    inside_option: bool,
    adjacent_only: bool,
    seen: &mut FxHashSet<NodeId>,
    out: &mut Vec<NodeId>,
) {
    if index.is_regular_element(node) && index.element_name(node) == "option" {
        for frag in index.child_frags(node) {
            for &child in index.frag_nodes(frag) {
                walk_select_subtree(index, child, true, adjacent_only, seen, out);
            }
        }
    } else if inside_option {
        walk_descendant(index, node, adjacent_only, seen, out);
    } else {
        for frag in index.child_frags(node) {
            for &child in index.frag_nodes(frag) {
                walk_select_subtree(index, child, false, adjacent_only, seen, out);
            }
        }
    }
}

/// The official `get_possible_element_siblings(node, direction,
/// adjacent_only, seen)` — the core sibling walk over the alternating
/// fragment/container path.
pub(super) fn get_possible_element_siblings(
    index: &TemplateIndex<'_, '_>,
    node: NodeId,
    direction: Direction,
    adjacent_only: bool,
    seen: &mut FxHashSet<NodeId>,
) -> NodeExistMap {
    let mut result = NodeExistMap::default();
    let path = index.path(node);
    let mut current = node;
    let mut i = path.len() as isize;

    loop {
        // `while (i--)` …
        i -= 1;
        if i < 0 {
            break;
        }
        // `const fragment = path[i--]`.
        let PathEntry::Frag(fragment) = path[i as usize] else {
            debug_assert!(false, "the path alternates fragment/container entries");
            break;
        };
        i -= 1;
        let nodes = index.frag_nodes(fragment);
        let Some(position) = nodes.iter().position(|&n| n == current) else {
            debug_assert!(false, "a node is always a member of its parent fragment");
            break;
        };
        let step: isize = match direction {
            Direction::Forward => 1,
            Direction::Backward => -1,
        };
        let mut j = position as isize + step;
        while j >= 0 && (j as usize) < nodes.len() {
            let sibling = nodes[j as usize];
            if index.is_regular_element(sibling) {
                // Slot-attribute-bearing elements render inside another
                // component — not siblings here.
                if !index.has_slot_attribute(sibling) {
                    result.set(sibling, Existence::Definitely);
                    if adjacent_only {
                        return result;
                    }
                }
            } else if index.is_block_node(sibling) || index.is_component_node(sibling) {
                if index.is_component_node(sibling) {
                    result.set(sibling, Existence::Probably);
                }
                let mut nested_seen: FxHashSet<NodeId> = FxHashSet::default();
                let possible_last_child = get_possible_nested_siblings(
                    index,
                    sibling,
                    direction,
                    adjacent_only,
                    &mut nested_seen,
                );
                possible_last_child.merge_into(&mut result);
                if adjacent_only
                    && !index.is_component_node(sibling)
                    && has_definite_elements(&possible_last_child)
                {
                    return result;
                }
            } else if index.is_svelte_element(sibling) {
                result.set(sibling, Existence::Probably);
            } else if index.is_render_tag(sibling) {
                result.set(sibling, Existence::Probably);
                for &snippet in index.snippets_of(sibling) {
                    let mut nested_seen: FxHashSet<NodeId> = FxHashSet::default();
                    get_possible_nested_siblings(
                        index,
                        snippet,
                        direction,
                        adjacent_only,
                        &mut nested_seen,
                    )
                    .merge_into(&mut result);
                }
            }
            j += step;
        }

        // `current = path[i]; if (!current) break;`
        if i < 0 {
            break;
        }
        let PathEntry::Node(container) = path[i as usize] else {
            debug_assert!(false, "the path alternates fragment/container entries");
            break;
        };
        current = container;

        if index.is_component_family(current) {
            // Transparent boundary — keep walking up.
            continue;
        }

        if index.is_snippet_block(current) {
            if !seen.insert(current) {
                break;
            }
            let sites = index.sites_of(current);
            for &site in sites {
                let siblings =
                    get_possible_element_siblings(index, site, direction, adjacent_only, seen);
                let definite = has_definite_elements(&siblings);
                siblings.merge_into(&mut result);
                if adjacent_only && sites.len() == 1 && definite {
                    return result;
                }
            }
        }

        if !index.is_block_node(current) {
            break;
        }

        // `{#each ...}<a /><b />{/each}` — wrap-around self-adjacency.
        if index.each_body_frag(current) == Some(fragment) {
            let mut nested_seen: FxHashSet<NodeId> = FxHashSet::default();
            get_possible_nested_siblings(
                index,
                current,
                direction,
                adjacent_only,
                &mut nested_seen,
            )
            .merge_into(&mut result);
        }
    }

    result
}

/// The official `get_possible_nested_siblings(node, direction, adjacent_only,
/// seen)` — the boundary fragments per container kind, with the exhaustive
/// downgrade on a null/absent branch. The IR flattens the official nested
/// `{:else if}` chain into ordered branches; pushing one fragment per branch
/// plus a `None` for a missing trailing `{:else}` folds to the identical
/// exhaustive/downgrade result as the official nested IfBlock recursion.
fn get_possible_nested_siblings(
    index: &TemplateIndex<'_, '_>,
    node: NodeId,
    direction: Direction,
    adjacent_only: bool,
    seen: &mut FxHashSet<NodeId>,
) -> NodeExistMap {
    let mut fragments: Vec<Option<usize>> = Vec::new();
    let mut exhaustive = true;

    match index.ir.node(node) {
        IrNode::Block(BlockIr::Each {
            body, else_body, ..
        }) => {
            fragments.push(index.frag_of_scope.get(&body.0).copied());
            fragments.push(else_body.and_then(|s| index.frag_of_scope.get(&s.0).copied()));
        }
        IrNode::Block(BlockIr::If { branches }) => {
            for branch in branches {
                fragments.push(index.frag_of_scope.get(&branch.body.0).copied());
            }
            let has_else = branches
                .last()
                .is_some_and(|branch| branch.condition.is_none());
            if !has_else {
                // The official missing `alternate` fragment.
                fragments.push(None);
            }
        }
        IrNode::Block(BlockIr::Await {
            pending,
            then_body,
            catch_body,
            ..
        }) => {
            for scope in [*pending, *then_body, *catch_body] {
                fragments.push(scope.and_then(|s| index.frag_of_scope.get(&s.0).copied()));
            }
        }
        IrNode::Block(BlockIr::Key { body, .. }) => {
            fragments.push(index.frag_of_scope.get(&body.0).copied());
        }
        IrNode::Block(BlockIr::Snippet { body, .. }) => {
            if !seen.insert(node) {
                return NodeExistMap::default();
            }
            exhaustive = false;
            fragments.push(index.frag_of_scope.get(&body.0).copied());
        }
        IrNode::Component(_) => {
            fragments.push(index.frag_of_children.get(&node).copied());
            for &snippet in index.snippets_of(node) {
                match index.ir.node(snippet) {
                    IrNode::Block(BlockIr::Snippet { body, .. }) => {
                        fragments.push(index.frag_of_scope.get(&body.0).copied());
                    }
                    _ => fragments.push(None),
                }
            }
        }
        // `SvelteComponent` / `SvelteSelf` and every other kind have no case
        // in the official switch — no fragments, an empty (vacuously
        // exhaustive) result.
        _ => {}
    }

    let mut result = NodeExistMap::default();
    for fragment in fragments {
        let Some(fragment) = fragment else {
            exhaustive = false;
            continue;
        };
        let map = loop_child(index, fragment, direction, adjacent_only, seen);
        exhaustive &= has_definite_elements(&map);
        map.merge_into(&mut result);
    }

    if !exhaustive {
        result.downgrade_all();
    }

    result
}

/// The official `loop_child(children, direction, adjacent_only, seen)` —
/// iterate a fragment's nodes from one end.
fn loop_child(
    index: &TemplateIndex<'_, '_>,
    fragment: usize,
    direction: Direction,
    adjacent_only: bool,
    seen: &mut FxHashSet<NodeId>,
) -> NodeExistMap {
    let mut result = NodeExistMap::default();
    let children = index.frag_nodes(fragment);
    let step: isize = match direction {
        Direction::Forward => 1,
        Direction::Backward => -1,
    };
    let mut i: isize = match direction {
        Direction::Forward => 0,
        Direction::Backward => children.len() as isize - 1,
    };
    while i >= 0 && (i as usize) < children.len() {
        let child = children[i as usize];
        if index.is_regular_element(child) {
            result.set(child, Existence::Definitely);
            if adjacent_only {
                break;
            }
        } else if index.is_svelte_element(child) {
            result.set(child, Existence::Probably);
        } else if index.is_render_tag(child) {
            for &snippet in index.snippets_of(child) {
                get_possible_nested_siblings(index, snippet, direction, adjacent_only, seen)
                    .merge_into(&mut result);
            }
        } else if index.is_block_node(child) {
            let child_result =
                get_possible_nested_siblings(index, child, direction, adjacent_only, seen);
            let definite = has_definite_elements(&child_result);
            child_result.merge_into(&mut result);
            if adjacent_only && definite {
                break;
            }
        }
        i += step;
    }
    result
}
