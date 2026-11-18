//! What the server emitter holds while it walks one template.

use std::ops::{Deref, DerefMut};

use crate::ast::types::ElementNode;
use crate::code_transform::{EditRecorder, MappedGeneratedText};
use crate::framework::vue::semantic::VueBindingView;
use crate::framework::vue::structure::VueCompileNodeId;
use crate::framework::vue::target::{
    VueHelperSet, VueTargetInputs, VueTargetOptions, VueTargetRefusal,
};
use crate::template::binding::prefixed_expr::{
    build_prefixed_iterable, expression_ts_removal_spans, prefixed_expr_plan, resolve_expr,
};
use crate::template::code_gen::shared::helpers::{
    is_builtin_component, BuiltinComponentFlags, SsrHelper, SsrHelperFlags, VdomHelper,
    VdomHelperFlags,
};
use crate::template::oxc::types::{OxcParsedElement, OxcParsedExpression, OxcParsedVFor};
use crate::template::topology::{VueTemplateNodeId, VueTemplateNodeKind};
use crate::types::NodeProp;

/// What each entered element means for its `leave_element`.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ElemCtx {
    /// Element inside the parent's push literal: emit `</tag>` only.
    InParentPush,
    /// Element that opened its own push: emit `</tag>` and close the push.
    OwnPush,
    /// Fully handled on enter (component, void element, `v-html`/`v-text`).
    Complete,
    /// Component with slot children: leave closes the slots object.
    ComponentWithSlots,
    /// `<component :is>` with slot children: leave closes the slots,
    /// `_createVNode` and `_ssrRenderVNode`.
    DynamicComponentWithSlots,
    /// `<template v-slot>` inside a component: one named slot.
    SlotTemplate,
    /// `<slot>` outlet with fallback children.
    SlotOutletFallback,
    /// `<Suspense>` with slot children: leave closes `_ssrRenderSuspense`.
    SuspenseSlots,
    /// `<template v-slot>` inside `<Suspense>`: plain arrow functions.
    SuspenseSlotTemplate,
    /// Transparent built-in (`Transition`, `KeepAlive`): children render
    /// directly and the closing tag is removed.
    TransparentBuiltin,
    /// `<TransitionGroup tag="X">` renders as `<X>`; holds the tag name.
    TransitionGroupTag(String),
    /// `<Teleport>` body: leave closes `_ssrRenderTeleport`.
    TeleportBody,
}

/// The recorded edits of one render function and the helpers it imports.
pub(super) struct SsrOut<'c> {
    pub(super) edits: EditRecorder<'c>,
    vdom: VdomHelperFlags,
    ssr: SsrHelperFlags,
    builtin: BuiltinComponentFlags,
    /// Runtime helpers named by the client emission of slot content.
    named_runtime: Vec<&'static str>,
}

impl<'c> SsrOut<'c> {
    pub(super) fn new(edits: EditRecorder<'c>) -> Self {
        Self {
            edits,
            vdom: VdomHelperFlags(0),
            ssr: SsrHelperFlags(0),
            builtin: BuiltinComponentFlags(0),
            named_runtime: Vec::new(),
        }
    }

    pub(super) fn add_vdom_import(&mut self, helper: VdomHelper) {
        self.vdom = self.vdom.add(helper);
    }

    pub(super) fn add_ssr_import(&mut self, helper: SsrHelper) {
        self.ssr = self.ssr.add(helper);
    }

    /// Helpers another emitter reports by import name join the same flag
    /// sets, so the import order is the helper tables' order.
    pub(super) fn add_runtime_names(&mut self, names: &[&'static str]) {
        for &name in names {
            if let Some(helper) = VdomHelper::from_name(name) {
                self.add_vdom_import(helper);
                continue;
            }
            match is_builtin_component(name.trim_start_matches('_')) {
                Some((flag, import)) if import == name => self.add_builtin_component(flag),
                _ => {
                    if !self.named_runtime.contains(&name) {
                        self.named_runtime.push(name);
                    }
                }
            }
        }
    }

    pub(super) fn add_builtin_component(&mut self, flag: u8) {
        self.builtin = self.builtin.add(flag);
    }

    /// The imported helpers, runtime ones first and built-in components
    /// after them, in the order the helper tables define.
    pub(super) fn helpers(&self) -> VueHelperSet {
        let mut runtime = self.vdom.to_imports();
        runtime.extend(self.builtin.to_imports());
        for &name in &self.named_runtime {
            if !runtime.contains(&name) {
                runtime.push(name);
            }
        }
        VueHelperSet {
            runtime,
            server: self.ssr.to_imports(),
        }
    }
}

impl<'c> Deref for SsrOut<'c> {
    type Target = EditRecorder<'c>;

    fn deref(&self) -> &EditRecorder<'c> {
        &self.edits
    }
}

impl<'c> DerefMut for SsrOut<'c> {
    fn deref_mut(&mut self) -> &mut EditRecorder<'c> {
        &mut self.edits
    }
}

/// One element as the emitter sees it on enter and leave.
pub(super) struct Visit<'c> {
    pub(super) tid: VueTemplateNodeId,
    pub(super) el: &'c ElementNode,
    pub(super) oxc: Option<&'c OxcParsedElement<'c>>,
    pub(super) tag_name: &'c str,
    pub(super) is_root: bool,
}

/// The slots of a component that has conditional or repeated slot
/// templates: static slots in an object, the rest in an array, as
/// `_createSlots({ …, _: 2 }, [ … ])`.
#[derive(Debug, Default)]
pub(super) struct SlotArray {
    /// The object is closed and the array is open.
    pub(super) open: bool,
    /// Key of the next conditional slot entry.
    pub(super) next_key: u32,
    /// The implicit default slot was opened as an array entry.
    pub(super) default_in_array: bool,
}

/// The server emitter's walk state.
pub(super) struct SsrEmitter<'c, M: verter_audit::observe::ObserveExec> {
    pub(super) _capture: std::marker::PhantomData<M>,
    /// Generated slot holes paired with the fragment that fills each one.
    pub(super) slot_fills: Vec<(&'c str, u32, crate::emission::EmissionFragment<'c>)>,
    pub(super) inputs: &'c VueTargetInputs<'c>,
    pub(super) source: &'c str,
    pub(super) options: &'c VueTargetOptions,
    pub(super) bindings: VueBindingView<'c>,
    /// Reusable buffer for building output strings.
    pub(super) buf: String,
    /// Element nesting depth (0 = root). Root elements merge `_attrs`.
    pub(super) depth: u32,
    /// Inside an open `` _push(`…` `` template literal.
    pub(super) in_push: bool,
    pub(super) elem_ctx: Vec<ElemCtx>,
    /// `_resolveComponent()` declarations hoisted to the function preamble.
    pub(super) component_resolves: Vec<String>,
    /// `_resolveDirective()` declarations hoisted to the function preamble.
    pub(super) directive_resolves: Vec<String>,
    /// Several effective roots: fragment markers, no `_attrs` on roots.
    pub(super) is_multi_root: bool,
    /// The template needs `<!--[-->…<!--]-->` (several element roots, or
    /// root-level comments, which hydration needs to see).
    pub(super) needs_fragment: bool,
    /// Depth of component slot content; children there are not roots.
    pub(super) in_component_slots: u32,
    /// `data-v-<hash>` when the component has scoped styles.
    pub(super) scope_id: Option<String>,
    /// An implicit `default: _withCtx(…)` is open in the current component.
    pub(super) default_slot_open: bool,
    pub(super) saved_default_slot_open: Vec<bool>,
    /// Closing arguments of each open `<Teleport>`.
    pub(super) teleport_closing_args: Vec<String>,
    /// An implicit `default: () => {` is open in a `<Suspense>`.
    pub(super) suspense_implicit_default_open: bool,
    /// Resolved `v-model` of the enclosing `<select>`, for its options.
    pub(super) select_v_model_expr: Option<String>,
    /// Range of an implicit default slot opened before named slots; it is
    /// relocated after them when the component closes.
    pub(super) default_slot_move_start: Option<u32>,
    pub(super) default_slot_move_end: Option<u32>,
    /// The text that closed a default slot that moves after the named ones;
    /// it travels with the slot.
    pub(super) default_slot_close: Option<&'c str>,
    /// Control-flow closers at the exclusive edge of a moving default slot.
    pub(super) default_slot_edge_closing: String,
    pub(super) saved_default_slot_move: Vec<(Option<u32>, Option<u32>, Option<&'c str>)>,
    /// Start of the current component's children (its open tag's end).
    pub(super) comp_children_start: Option<u32>,
    /// `v-for` nesting; inside a loop a component's slots are dynamic.
    pub(super) v_for_depth: std::cell::Cell<u32>,
    /// Scoped-slot nesting; inside one, child component slots are dynamic.
    pub(super) v_slot_scope_depth: u32,
    pub(super) scoped_slot_entered: Vec<bool>,
    /// A root `v-model` on a native element needs `let _temp0`.
    pub(super) temp_var_needed: bool,
    /// Removals of comments when comments are off, resolved at
    /// `leave_template`: a removal inside a range the function header
    /// claims is absorbed by that claim, the rest become plain removals.
    pub(super) pending_disabled_comment_removals: Vec<(u32, u32)>,
    /// The first refusal raised while walking; the render function is
    /// refused when it is set.
    pub(super) refusal: Option<VueTargetRefusal>,
    /// One entry per component with slot children, innermost last.
    pub(super) slot_arrays: Vec<Option<SlotArray>>,
    /// One entry per open slot template: the text closing it as an array
    /// entry, or `None` for an object entry.
    pub(super) slot_entry_tails: Vec<Option<String>>,
}

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    pub(super) fn new(inputs: &'c VueTargetInputs<'c>, bindings: VueBindingView<'c>) -> Self {
        let scope = inputs.scope;
        Self {
            inputs,
            _capture: std::marker::PhantomData,
            slot_fills: Vec::new(),
            source: inputs.source.text,
            options: inputs.options,
            bindings,
            buf: String::with_capacity(256),
            depth: 0,
            in_push: false,
            elem_ctx: Vec::with_capacity(16),
            component_resolves: Vec::new(),
            directive_resolves: Vec::new(),
            is_multi_root: false,
            needs_fragment: false,
            in_component_slots: 0,
            scope_id: scope.has_scoped.then(|| scope.scope.attribute()),
            default_slot_open: false,
            saved_default_slot_open: Vec::new(),
            teleport_closing_args: Vec::new(),
            suspense_implicit_default_open: false,
            select_v_model_expr: None,
            default_slot_move_start: None,
            default_slot_move_end: None,
            default_slot_close: None,
            default_slot_edge_closing: String::new(),
            saved_default_slot_move: Vec::new(),
            comp_children_start: None,
            v_for_depth: std::cell::Cell::new(0),
            v_slot_scope_depth: 0,
            scoped_slot_entered: Vec::new(),
            temp_var_needed: false,
            pending_disabled_comment_removals: Vec::new(),
            refusal: None,
            slot_arrays: Vec::new(),
            slot_entry_tails: Vec::new(),
        }
    }

    // ── lookups ─────────────────────────────────────────────────

    pub(super) fn tid(&self, node: VueCompileNodeId) -> VueTemplateNodeId {
        self.inputs.structure.template_id(node)
    }

    /// The element payload of an element-like node.
    pub(super) fn element(&self, tid: VueTemplateNodeId) -> &'c ElementNode {
        self.inputs
            .topology
            .element(tid)
            .expect("an element-like node has an element payload")
    }

    pub(super) fn oxc_element(&self, tid: VueTemplateNodeId) -> Option<&'c OxcParsedElement<'c>> {
        self.inputs.semantic.element_exprs(tid)
    }

    pub(super) fn oxc_interpolation(
        &self,
        tid: VueTemplateNodeId,
    ) -> Option<&'c OxcParsedExpression<'c>> {
        self.inputs.semantic.interpolation(tid)
    }

    pub(super) fn kind(&self, tid: VueTemplateNodeId) -> VueTemplateNodeKind {
        self.inputs.topology.kind(tid)
    }

    /// Authored text of a node.
    pub(super) fn text(&self, tid: VueTemplateNodeId) -> &'c str {
        let span = self.inputs.topology.span(tid);
        &self.source[span.start as usize..span.end as usize]
    }

    pub(super) fn tag_name<'s>(&self, el: &ElementNode, source: &'s str) -> &'s str {
        &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize]
    }

    /// End of the element: its close tag, or its self-closing open tag.
    pub(super) fn el_end(&self, el: &ElementNode) -> u32 {
        el.tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(el.tag_open.end)
    }

    pub(super) fn visit(&self, node: VueCompileNodeId) -> Visit<'c> {
        let tid = self.tid(node);
        let el = self.element(tid);
        Visit {
            tid,
            el,
            oxc: self.oxc_element(tid),
            tag_name: self.tag_name(el, self.source),
            is_root: self.is_root(),
        }
    }

    /// Server output reads CSS `v-bind()` values from `_cssVars`.
    pub(super) fn has_css_vars(&self) -> bool {
        !self.inputs.scope.css_vars.is_empty()
    }

    // ── expressions ─────────────────────────────────────────────

    /// A template expression with its bindings' access prefixes and
    /// suffixes applied. Type-only syntax is skipped by the shared rewriter
    /// before the expression is embedded in generated server code.
    pub(super) fn resolve_expr(
        &self,
        expr: &str,
        offset: u32,
        oxc_expr: Option<&OxcParsedExpression<'c>>,
    ) -> String {
        resolve_expr(
            expr,
            offset,
            oxc_expr,
            &self.bindings,
            self.options.force_js,
        )
    }

    /// Like [`resolve_expr`](Self::resolve_expr), with the plan of which
    /// generated runs are authored source when a map is captured.
    pub(super) fn resolve_expr_segments(
        &self,
        expr: &str,
        offset: u32,
        oxc_expr: &OxcParsedExpression<'c>,
    ) -> MappedGeneratedText {
        prefixed_expr_plan(
            expr,
            offset,
            oxc_expr,
            &self.bindings,
            &expression_ts_removal_spans(Some(oxc_expr), self.options.force_js),
            self.options.map_mode,
        )
    }

    /// A `v-for` source with its bindings' prefixes, read from the parsed
    /// loop.
    pub(super) fn resolve_for_iterable(
        &self,
        iterable: &str,
        v_for: &NodeProp,
        vfor: &OxcParsedVFor<'c>,
    ) -> String {
        build_prefixed_iterable(
            iterable,
            self.source,
            v_for,
            vfor,
            &self.bindings,
            self.options.force_js,
        )
    }

    // ── scope identity ──────────────────────────────────────────

    /// The component's scope attribute and any inherited slot scope for open tags.
    pub(super) fn scope_id_suffix(&self) -> String {
        let mut suffix = match &self.scope_id {
            Some(id) => format!(" {}", id),
            None => String::new(),
        };
        if self.in_slot_function() {
            suffix.push_str("${_scopeId}");
        }
        suffix
    }

    /// `, "data-v-<hash>"` as the extra `_ssrRenderComponent` argument.
    pub(super) fn scope_id_arg(&self) -> String {
        match &self.scope_id {
            Some(id) => format!(", \"{}\"", id),
            None => String::new(),
        }
    }

    // ── roots ───────────────────────────────────────────────────

    /// The current element is a direct child of `<template>` and receives
    /// `_attrs`.
    pub(super) fn is_root(&self) -> bool {
        self.depth == 0 && !self.is_multi_root && self.in_component_slots == 0
    }

    /// `_cssVars` injects on every root-level node (each multi-root sibling
    /// and each root `v-if` branch), unlike `_attrs`.
    pub(super) fn is_css_vars_root(&self) -> bool {
        self.depth == 0 && self.in_component_slots == 0
    }
}
