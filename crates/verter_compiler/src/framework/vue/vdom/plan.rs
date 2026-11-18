//! Planning that runs before or beside the walk: handler cache slots, static
//! array groups, static text-run caches and hoisted constants, numbered the
//! way the official compiler numbers them.

use rustc_hash::FxHashMap;

use crate::framework::vue::structure::{VueChildKind, VueChildRecord, VueCompileNodeId};

use crate::ast::types::{AstNodeKind, ChildrenMode, ElementNode, TagType};
use crate::code_transform::SegmentAnchor;
use crate::template::code_gen::types::{ChildKind, ChildRecord};
use crate::template::oxc::types::{OxcNodeData, OxcParsedElement};
use crate::types::NodeId;

use super::{directives, element, events, is_v_on, VdomState};

/// How the planner settled one inline event handler or native `v-model`
/// update handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HandlerPlan {
    /// Stored once in `_cache[N]`, so the handler keeps one identity across
    /// renders.
    Cached(usize),
    /// Reads a `v-for` alias or `v-slot` parameter, so it is created on
    /// every render and is a dynamic prop, as Vue's `hasScopeRef` decides. A
    /// component-level `_cache` slot would hand every row the first row's
    /// handler.
    ScopeBound,
}

/// What the planning passes reserved for one element before the walk
/// leaves it. Official numbering runs these reservations before or apart
/// from the bottom-up emission, so they are made ahead and read on leave.
#[derive(Debug, Default)]
pub(super) struct VdomNodePlan {
    /// The `_hoisted_N` index of the element's own constant props object
    /// or injected branch key, reserved in document pre-order.
    pub(super) hoist: Option<usize>,
    /// The plan of each cacheable inline handler and native `v-model`
    /// update handler, by prop index.
    pub(super) handler_plans: FxHashMap<usize, HandlerPlan>,
    /// `_cache[N]` holding the element's whole static children array.
    pub(super) array_group: Option<usize>,
    /// The parent caches this element inside its children array group.
    pub(super) array_grouped: bool,
}

impl VdomState<'_> {
    /// The compile identity of a parser element.
    pub(super) fn plan_key(&self, id: NodeId) -> VueCompileNodeId {
        let node = self
            .inputs
            .topology
            .node_for_syntax(id)
            .expect("a walked parser node is in the topology");
        self.inputs.structure.compile_id(node)
    }

    pub(super) fn plan_of(&self, id: NodeId) -> Option<&VdomNodePlan> {
        self.plans.get(self.plan_key(id))
    }

    fn plan_mut(&mut self, id: NodeId) -> &mut VdomNodePlan {
        let key = self.plan_key(id);
        if self.plans.get(key).is_none() {
            self.plans.insert(key, VdomNodePlan::default());
        }
        self.plans
            .get_mut(key)
            .expect("the plan was inserted above")
    }
}

impl<'c> VdomState<'c> {
    /// True when the single logical root element carries a `v-memo` directive.
    /// Used by leave_template to suppress the outer single-root `_openBlock()`
    /// wrapper (a v-memo factory owns its own openBlock inside the memo closure).
    pub(super) fn root_element_has_v_memo(&self, root_children: &[NodeId], source: &str) -> bool {
        for &cid in root_children {
            if let AstNodeKind::Element(el) = &self.ast.nodes[cid.0].kind {
                return el.props.iter().any(|p| {
                    p.is_directive && &source[p.start as usize..p.name_end as usize] == "v-memo"
                });
            }
        }
        false
    }

    /// True when the single logical root element carries a directive that
    /// needs `_withDirectives()` wrapping (v-show, native v-model, a custom
    /// directive) and has no `v-if`/`v-for` of its own. Official Vue emits
    /// `_withDirectives((_openBlock(), createXBlock(...)), [...])` — the
    /// `openBlock()` sequence nests INSIDE `_withDirectives`, built by
    /// `process_element_leave` (see `needs_block_wrapper`). Used by
    /// `leave_template` to suppress its own outer single-root `_openBlock()`
    /// wrapper, mirroring the `v-memo` bypass above.
    pub(super) fn root_element_has_directives_wrap(
        &self,
        root_children: &[NodeId],
        source: &str,
    ) -> bool {
        for &cid in root_children {
            if let AstNodeKind::Element(el) = &self.ast.nodes[cid.0].kind {
                if el.v_for.is_some() || el.v_condition.is_some() {
                    return false;
                }
                let is_component = el.tag_type.is_component();
                return el.props.iter().any(|p| {
                    if !p.is_directive {
                        return false;
                    }
                    let dname = &source[p.start as usize..p.name_end as usize];
                    if dname == "v-model" {
                        return !is_component;
                    }
                    if dname == "v-show" {
                        return true;
                    }
                    !matches!(
                        dname,
                        ":" | "v-bind"
                            | "@"
                            | "v-on"
                            | "v-if"
                            | "v-else-if"
                            | "v-else"
                            | "v-for"
                            | "v-slot"
                            | "v-once"
                            | "v-text"
                            | "v-html"
                            | "v-cloak"
                            | "v-memo"
                            | "v-pre"
                    ) && dname.starts_with("v-")
                });
            }
        }
        false
    }

    /// Walk `ids` bottom-up (children before self, matching `leave_element`'s
    /// own visitation order) and plan every cacheable inline event handler /
    /// native `v-model` update handler found, into the element's
    /// `VdomNodePlan::handler_plans`: a `_cache[N]` slot, or
    /// [`HandlerPlan::ScopeBound`] when the handler reads a name the
    /// element's template scope declares. Advances `self.cache_index` as it
    /// reserves — called once, before the main walk, so these reservations
    /// occupy the lowest indices.
    ///
    /// Elements nest without bound, so the walk runs from an explicit stack:
    /// an element waits on it while its children reserve.
    pub(super) fn reserve_handler_caches(&mut self, ids: &[NodeId], source: &str) {
        enum Visit {
            Enter(NodeId),
            Reserve(NodeId),
        }
        let ast = self.ast;
        let mut visits: Vec<Visit> = ids.iter().rev().map(|&id| Visit::Enter(id)).collect();
        while let Some(visit) = visits.pop() {
            let id = match visit {
                Visit::Enter(id) => {
                    let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
                        continue;
                    };
                    visits.push(Visit::Reserve(id));
                    if let Some(children) = el.content.as_ref().map(|c| c.children.as_slice()) {
                        visits.extend(children.iter().rev().map(|&child| Visit::Enter(child)));
                    }
                    continue;
                }
                Visit::Reserve(id) => id,
            };
            let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
                continue;
            };
            let oxc_el = match self.oxc_ast.data.get(id.0) {
                Some(OxcNodeData::Element(oxc_el)) => Some(oxc_el.as_ref()),
                _ => None,
            };
            let is_component = el.tag_type.is_component();
            // Handlers are written in the element's props scope: its own
            // `v-for` aliases and every enclosing alias and slot parameter.
            let scope = oxc_el.map_or_else(
                || self.oxc_ast.children_scope(id),
                |oxc_el| oxc_el.props_scope,
            );
            let mut reservations: FxHashMap<usize, HandlerPlan> = FxHashMap::default();
            for (prop_idx, prop) in el.props.iter().enumerate() {
                if !prop.is_directive {
                    continue;
                }
                let dname = &source[prop.start as usize..prop.name_end as usize];
                let is_on = is_v_on(dname);
                let is_native_model = dname == "v-model" && !is_component;
                if !((is_on && prop.arg_start.is_some()) || is_native_model) {
                    continue;
                }
                let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) else {
                    continue;
                };
                let oxc_exp =
                    crate::template::binding::prefixed_expr::find_prop_oxc_exp(oxc_el, prop_idx);
                if oxc_exp.is_some_and(|exp| self.oxc_ast.reads_scope(exp, scope)) {
                    reservations.insert(prop_idx, HandlerPlan::ScopeBound);
                    continue;
                }
                if is_native_model {
                    // Native `v-model` always produces a `$event => (… = $event)`
                    // update handler — cacheable unless it reads the scope.
                    let idx = self.cache_index;
                    self.cache_index += 1;
                    reservations.insert(prop_idx, HandlerPlan::Cached(idx));
                    continue;
                }
                let value = &source[vs as usize..ve as usize];
                // Modifier wrapping (`_withModifiers`/`_withKeys`) composes
                // around the handler span separately — no current case
                // combines it with handler caching, so skip reserving
                // when modifiers are present (mirrors the emission-site
                // guard in `build_props_object_into`).
                if prop.modifiers.is_empty()
                    && events::is_cacheable_inline_event_handler(value, oxc_exp)
                {
                    let idx = self.cache_index;
                    self.cache_index += 1;
                    reservations.insert(prop_idx, HandlerPlan::Cached(idx));
                }
            }
            if !reservations.is_empty() {
                self.plan_mut(id).handler_plans = reservations;
            }
        }
    }

    /// A direct element child that (per the leave-time `cache_idx` logic in
    /// `leave_element`) would be individually `_cache[N]`-eligible: fully
    /// static, not a block root, no structural directive, not a component
    /// (components use slot-level caching instead). A child found via
    /// `el.content.children` is never itself the single template root, so
    /// that check does not apply here.
    pub(super) fn child_is_group_cacheable(&self, id: NodeId) -> bool {
        let AstNodeKind::Element(el) = &self.ast.nodes[id.0].kind else {
            return false;
        };
        el.is_fully_static
            && el.v_condition.is_none()
            && el.v_for.is_none()
            && !el.tag_type.is_component()
    }

    /// Walk `ids` bottom-up (children before self) and, for every element
    /// whose direct children are ALL individually `_cache[N]`-eligible
    /// (`child_is_group_cacheable`), reserve ONE `_cache[N]` slot to hold
    /// the whole children array — official Vue's `cacheStatic`: when
    /// `toCache.length === children.length`, the node's children are
    /// grouped into one cached array and spread back
    /// (`[...(_cache[N] || (_cache[N] = [...]))]`) instead of caching each
    /// child separately. A mix of static and dynamic/directive children
    /// disqualifies the whole parent — each cacheable child then falls back
    /// to its own individual `cache_idx`, unchanged. The parent's OWN
    /// eligibility (block root, directives, …) does not gate this —
    /// `static-element.vue`'s root `<div id=… title=…>` IS the block root
    /// and still groups its one static `<p>` child.
    ///
    /// Elements nest without bound, so the walk runs from an explicit stack:
    /// an element waits on it while its children reserve.
    pub(super) fn reserve_array_group_caches(&mut self, ids: &[NodeId], is_root_level: bool) {
        enum Visit {
            Enter(NodeId, bool),
            Reserve(NodeId, bool),
        }
        let ast = self.ast;
        let mut visits: Vec<Visit> = ids
            .iter()
            .rev()
            .map(|&id| Visit::Enter(id, is_root_level))
            .collect();
        while let Some(visit) = visits.pop() {
            let (id, is_root_level, entering) = match visit {
                Visit::Enter(id, is_root_level) => (id, is_root_level, true),
                Visit::Reserve(id, is_root_level) => (id, is_root_level, false),
            };
            let AstNodeKind::Element(el) = &ast.nodes[id.0].kind else {
                continue;
            };
            // Slot outlets (`<slot>` fallback content), `<template>`
            // (`v-slot`/`v-if`/`v-for`), and components all route their
            // children through entirely separate leave-time functions
            // (`process_slot_outlet`, `process_template_slot`,
            // `leave_template_fragment`, `leave_component_with_slots`,
            // `leave_component_with_default_slot`) that never consult
            // `VdomNodePlan::array_group` — each already has its own
            // pre-existing slot-level cache-grouping mechanism. Reserving
            // (and thus advancing `self.cache_index`) for their children
            // here would both dead-reserve an unused slot and desync the
            // numbering those mechanisms compute independently at leave
            // time. Do not recurse into them at all.
            if el.tag_type.is_slot_outlet()
                || el.tag_type == TagType::Template
                || el.tag_type.is_component()
            {
                continue;
            }
            let children = el
                .content
                .as_ref()
                .map(|c| c.children.as_slice())
                .unwrap_or(&[]);
            if entering {
                visits.push(Visit::Reserve(id, is_root_level));
                visits.extend(
                    children
                        .iter()
                        .rev()
                        .map(|&child| Visit::Enter(child, false)),
                );
                continue;
            }

            if children.is_empty() {
                continue;
            }

            // If `id` itself would be swallowed into an ancestor's single
            // cache (fully static, not a block root, no structural
            // directive, not a component — mirrors the leave-time
            // `parent_is_cached` check in `leave_element`), its children
            // never reach individual OR grouped caching at all: the
            // ancestor's one cache already covers the whole subtree bare.
            let id_is_block_root = el.v_condition.is_some()
                || el.v_for.is_some()
                || (is_root_level && self.single_root);
            let id_swallowed = el.is_fully_static
                && !id_is_block_root
                && el.v_condition.is_none()
                && el.v_for.is_none()
                && !el.tag_type.is_component();
            if id_swallowed {
                continue;
            }

            // The resolved children: a comment the structure dropped (comments
            // disabled) or whitespace it removed is not a child here.
            let structure = self.inputs.structure;
            let topology = self.inputs.topology;
            let resolved = structure.children(self.plan_key(id));
            let syntax_of =
                |child: &VueChildRecord| topology.syntax(structure.template_id(child.node));
            let all_eligible = resolved.iter().all(|child| match child.kind {
                VueChildKind::Element => {
                    syntax_of(child).is_some_and(|cid| self.child_is_group_cacheable(cid))
                }
                VueChildKind::WhitespaceNewline | VueChildKind::WhitespaceSpace => true,
                VueChildKind::Text | VueChildKind::Comment | VueChildKind::Interpolation => false,
            });
            let has_element_child = resolved
                .iter()
                .any(|child| child.kind == VueChildKind::Element);
            if !all_eligible || !has_element_child {
                continue;
            }
            let grouped: Vec<NodeId> = resolved
                .iter()
                .filter(|child| child.kind == VueChildKind::Element)
                .filter_map(syntax_of)
                .collect();
            let idx = self.cache_index;
            self.cache_index += 1;
            self.plan_mut(id).array_group = Some(idx);
            for cid in grouped {
                self.plan_mut(cid).array_grouped = true;
            }
        }
    }

    /// Reserve a `_cache[N]` slot for each static-only text/interpolation
    /// run in `children` (array-mode children: `Mixed`/`MultiElement`/
    /// `SingleElement` — the same run definition `wrap_array_text_runs`
    /// itself walks), keyed by the run's own start offset. Official Vue
    /// caches a static text sibling exactly like a static element sibling:
    /// `_cache[N] || (_cache[N] = _createTextVNode("...", -1 /* CACHED */))`
    /// (verified against the real compiler — `v-model/checkbox`'s trailing
    /// " Agree" text beside a directive-wrapped `<input>`).
    ///
    /// Skipped entirely when `children_mode` isn't array mode, or when
    /// EVERY child is already covered by an `VdomNodePlan::array_group`
    /// entry on `own_id` (the whole list groups into one shared array cache
    /// instead — an individually-cached run inside that group would
    /// double-count `self.cache_index`; the group's own per-item text
    /// shape is a separate, not-yet-covered gap, left unchanged here).
    pub(super) fn reserve_static_text_run_caches(
        &mut self,
        own_id: NodeId,
        children: &[ChildRecord],
        children_mode: ChildrenMode,
    ) -> FxHashMap<u32, usize> {
        let mut reservations = FxHashMap::default();
        if !self.options.hoist_static
            || !matches!(
                children_mode,
                ChildrenMode::Mixed | ChildrenMode::MultiElement | ChildrenMode::SingleElement
            )
            || self
                .plan_of(own_id)
                .is_some_and(|plan| plan.array_group.is_some())
        {
            return reservations;
        }
        let mut i = 0;
        while i < children.len() {
            let kind = children[i].kind;
            if kind == ChildKind::Text || kind == ChildKind::Interpolation {
                let run_start = i;
                let mut has_dynamic = kind == ChildKind::Interpolation;
                i += 1;
                while i < children.len()
                    && matches!(children[i].kind, ChildKind::Text | ChildKind::Interpolation)
                {
                    if children[i].kind == ChildKind::Interpolation {
                        has_dynamic = true;
                    }
                    i += 1;
                }
                if !has_dynamic {
                    let idx = self.cache_index;
                    self.cache_index += 1;
                    reservations.insert(children[run_start].start, idx);
                }
            } else {
                i += 1;
            }
        }
        reservations
    }

    /// Reserve a document-pre-order `_hoisted_N` slot for `id`'s own props
    /// or injected branch key, BEFORE any of its children are visited. See
    /// [`VdomNodePlan::hoist`] for the full rationale and scope.
    pub(super) fn try_reserve_element_hoist(
        &mut self,
        id: NodeId,
        element: &ElementNode,
        oxc: Option<&OxcParsedElement<'c>>,
        source: &'c str,
    ) {
        // Whole-subtree-static elements route through `_createStaticVNode`
        // / `_cache[N]` instead — restrict to elements with at least one
        // dynamic descendant, where `has_cached_patchflag` is structurally
        // guaranteed false (see the field doc comment).
        if element.is_fully_static || element.tag_type.is_component() {
            return;
        }
        // A slot outlet (`<slot>`) never reaches `process_element_leave` —
        // `leave_element` routes it to `process_slot_outlet`, which builds
        // its own `_renderSlot(...)` props string and never consults
        // `pre_reserved_hoist`. Same for ANY `<template>` element — both
        // `v-slot` (`process_template_slot`) and `v-if`/`v-for`
        // (`leave_template_fragment`) route to their own separate
        // functions. Reserving for either here would push an orphaned,
        // unreferenced `_hoisted_N` (confirmed as a regression on
        // `slots.vue`'s `<slot name="header">` during development).
        if element.tag_type.is_slot_outlet() || element.tag_type == TagType::Template {
            return;
        }

        let has_props = !element.props.is_empty() || element.v_ref.is_some();
        if has_props {
            // Any directive makes the props object non-constant; bail to
            // the unchanged leave-time path, which resolves the dynamic
            // shape correctly.
            if element.props.iter().any(|p| p.is_directive) {
                return;
            }
            let mut buf = String::new();
            let mut anchors: Vec<SegmentAnchor> = Vec::new();
            let props_result = element::build_props_object_into(
                &mut buf,
                element,
                source,
                &self.resolver,
                oxc,
                None,
                self.options.force_js,
                &mut anchors,
                // No directive props reach here (early return above) — a
                // handler/`v-model` reservation is structurally impossible.
                None,
                !self.options.cache_handlers,
            );
            // Mirrors `can_hoist_props`'s guard set in
            // `element::process_element_leave`, minus
            // `injected_key.is_none()` (a `has_props` element never carries
            // one — injected keys only apply on the props-less branch) and
            // `!has_cached_patchflag` (structurally false here already).
            let can_hoist = props_result.dynamic_props.is_empty()
                && !props_result.has_vnode_key
                && !props_result.has_dynamic_ref
                && !props_result.uses_merge
                && !props_result.uses_normalize_class
                && !props_result.uses_normalize_style
                && !props_result.uses_normalize_props
                && !props_result.uses_guard_reactive_props
                && !props_result.uses_to_handlers
                && props_result.native_vmodel.is_none()
                && props_result.directive_entries.is_empty();
            if can_hoist {
                self.reserve_hoist(id, buf, anchors);
            }
        } else if element.v_condition.is_some()
            && element.v_for.is_none()
            && !directives::element_has_vnode_key(element, source)
        {
            if let Some(k) = directives::condition_branch_index(self.ast, id) {
                self.reserve_hoist(id, format!("{{ key: {k} }}"), Vec::new());
            }
        }
    }

    /// Push (or dedup-reuse) a hoisted constant now and remember its index
    /// for `id`, so `process_element_leave` can reference it directly
    /// instead of pushing again at leave time. `anchors` are `content`'s own
    /// embedded authored anchors (relative to `content`'s own start) — see
    /// `hoisted_constants`'s field doc.
    pub(super) fn reserve_hoist(
        &mut self,
        id: NodeId,
        content: String,
        anchors: Vec<SegmentAnchor>,
    ) {
        let idx = if let Some(existing) = self
            .hoisted_constants
            .iter()
            .position(|(c, _)| *c == content)
        {
            existing + 1
        } else {
            self.hoisted_constants.push((content, anchors));
            self.hoisted_constants.len()
        };
        self.plan_mut(id).hoist = Some(idx);
    }
}
