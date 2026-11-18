//! An open element or closure records rows until every reference is known.
use super::counters::VaporCounters;
use super::effect::{EffectId, TextPart};
use super::ops::{ClosedBlock, Nav, RunId, Stmt, TextCreation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MergedConstructKind {
    SlotOutlet,
    DynamicComponent,
    /// A component with a static type: a stable slot root.
    Component,
    Other,
}

/// One slot a component's slot templates declare, in source order.
#[derive(Debug, Clone)]
pub(super) enum NamedSlot {
    /// `"name": (props) => {...}`, a statically named slot.
    Static(RunId),
    /// A dynamically named slot's `{ name, fn }` getter.
    Dynamic(RunId),
    /// A looped slot template's `createForSlots`.
    Loop(RunId),
    /// A `v-if` slot and its `v-else-if`/`v-else` siblings: each arm's
    /// condition (none for `v-else`) and its `{ name, fn }` object.
    Conditional(Vec<(Option<RunId>, RunId)>),
}

#[derive(Debug)]
pub(super) enum PendingNavRequest<'c> {
    Merge {
        dom_child_index: u32,
        has_following: bool,
        nav_slot: Option<usize>,
        stmt_slot: usize,
    },
    OwnRef {
        own_ref: u32,
        dom_child_index: u32,
        nav_slot: usize,
    },
    /// One text run beside other nodes, reached as its own node: the parts
    /// its text effect writes are the run's own, taken when the run ends.
    TextRef {
        own_ref: u32,
        dom_child_index: u32,
        nav_slot: usize,
        stmt_slot: usize,
        parts: Vec<TextPart<'c>>,
    },
}

#[derive(Debug)]
pub(super) struct VaporScope<'c> {
    pub(super) html: String,
    pub(super) once: bool,
    pub(super) memo: Option<RunId>,
    pub(super) text_parts: Vec<TextPart<'c>>,
    pub(super) node_ref: Option<u32>,
    /// Whether output names `node_ref` as a node: effects, events, insertion
    /// points and child navigation do; a text-only use reads it as text.
    pub(super) node_named: bool,
    pub(super) text_node_ref: Option<u32>,
    pub(super) text_ref_generated: bool,
    pub(super) children_all_text_like: bool,
    pub(super) own_effects: Vec<EffectId>,
    pub(super) child_nav: Vec<Nav>,
    pub(super) child_text_creations: Vec<TextCreation>,
    pub(super) child_effects: Vec<EffectId>,
    pub(super) child_statements: Vec<Stmt>,
    pub(super) named_slots: Vec<NamedSlot>,
    /// Locals that keep each dynamic slot's function across name changes,
    /// declared before the component creation that reads them.
    pub(super) slot_caches: Vec<&'c str>,
    pub(super) dom_child_cursor: u32,
    pub(super) dom_in_text_run: bool,
    pub(super) text_run_html_start: Option<usize>,
    pub(super) text_run_has_dynamic: bool,
    /// Where the open text run's parts start in `text_parts`.
    pub(super) text_run_parts_start: usize,
    /// The navigation request of the open text run, once it is dynamic.
    pub(super) open_text_request: Option<usize>,
    pub(super) pending_nav_requests: Vec<PendingNavRequest<'c>>,
    pub(super) is_component_scope: bool,
    pub(super) is_transparent_wrapper: bool,
    /// A transparent wrapper's children, each closed as its own block root.
    pub(super) roots: Vec<ClosedBlock>,
    /// The wrapper body has one root, so its template is a root template
    /// when the wrapper is the template's single root.
    pub(super) root_template: bool,
    /// A run of text and interpolation that is one root of a transparent wrapper.
    pub(super) text_root: bool,
    pub(super) merged_construct_kinds: Vec<(MergedConstructKind, u32)>,
    /// The namespace of the element whose template this scope writes: 0
    /// for HTML, 1 for SVG, 2 for MathML.
    pub(super) namespace: u8,
}

impl Default for VaporScope<'_> {
    fn default() -> Self {
        Self {
            html: String::new(),
            once: false,
            memo: None,
            text_parts: Vec::new(),
            node_ref: None,
            node_named: false,
            text_node_ref: None,
            text_ref_generated: true,
            children_all_text_like: true,
            own_effects: Vec::new(),
            child_nav: Vec::new(),
            child_text_creations: Vec::new(),
            child_effects: Vec::new(),
            child_statements: Vec::new(),
            named_slots: Vec::new(),
            slot_caches: Vec::new(),
            dom_child_cursor: 0,
            dom_in_text_run: false,
            text_run_html_start: None,
            text_run_has_dynamic: false,
            text_run_parts_start: 0,
            open_text_request: None,
            pending_nav_requests: Vec::new(),
            is_component_scope: false,
            is_transparent_wrapper: false,
            roots: Vec::new(),
            root_template: false,
            text_root: false,
            merged_construct_kinds: Vec::new(),
            namespace: 0,
        }
    }
}

impl<'c> VaporScope<'c> {
    pub(super) fn reset(&mut self) {
        self.html.clear();
        self.once = false;
        self.memo = None;
        self.text_parts.clear();
        self.node_ref = None;
        self.node_named = false;
        self.text_node_ref = None;
        self.text_ref_generated = true;
        self.children_all_text_like = true;
        self.own_effects.clear();
        self.child_nav.clear();
        self.child_text_creations.clear();
        self.child_effects.clear();
        self.child_statements.clear();
        self.named_slots.clear();
        self.slot_caches.clear();
        self.dom_child_cursor = 0;
        self.dom_in_text_run = false;
        self.text_run_html_start = None;
        self.text_run_has_dynamic = false;
        self.text_run_parts_start = 0;
        self.open_text_request = None;
        self.pending_nav_requests.clear();
        self.is_component_scope = false;
        self.is_transparent_wrapper = false;
        self.roots.clear();
        self.root_template = false;
        self.text_root = false;
        self.merged_construct_kinds.clear();
        self.namespace = 0;
    }

    pub(super) fn observe_dom_text_run(&mut self) -> bool {
        if self.dom_in_text_run {
            return false;
        }
        self.end_text_run();
        self.dom_in_text_run = true;
        self.dom_child_cursor += 1;
        self.text_run_parts_start = self.text_parts.len();
        true
    }

    /// End the open text run. A navigated run takes its own parts into its
    /// request; beside other nodes, a run with no dynamic part keeps none, as
    /// its text is already in the static template.
    pub(super) fn end_text_run(&mut self) {
        let start = self.text_run_parts_start.min(self.text_parts.len());
        match self.open_text_request.take() {
            Some(request) => {
                let run = self.text_parts.split_off(start);
                if let Some(PendingNavRequest::TextRef { parts, .. }) =
                    self.pending_nav_requests.get_mut(request)
                {
                    *parts = run;
                }
            }
            None if !self.children_all_text_like => self.text_parts.truncate(start),
            None => {}
        }
        self.text_run_parts_start = self.text_parts.len();
    }

    pub(super) fn observe_dom_comment(&mut self) {
        self.observe_dom_element();
    }

    pub(super) fn observe_dom_element(&mut self) -> u32 {
        self.end_text_run();
        let index = self.dom_child_cursor;
        self.dom_in_text_run = false;
        self.text_run_html_start = None;
        self.text_run_has_dynamic = false;
        self.dom_child_cursor += 1;
        index
    }

    pub(super) fn ensure_node_ref(&mut self, counters: &mut VaporCounters) -> u32 {
        self.node_named = true;
        *self.node_ref.get_or_insert_with(|| counters.next_node())
    }

    pub(super) fn ensure_text_ref(&mut self, counters: &mut VaporCounters) -> u32 {
        if let Some(text) = self.text_node_ref {
            return text;
        }
        let text = *self.node_ref.get_or_insert_with(|| counters.next_node());
        self.text_node_ref = Some(text);
        text
    }

    /// A node reference for one navigated text run. The first one also
    /// marks the scope's text as navigated rather than read from its first
    /// child.
    pub(super) fn reserve_nav_text_ref(&mut self, counters: &mut VaporCounters) -> u32 {
        let text = counters.next_node();
        if self.text_node_ref.is_none() {
            self.text_node_ref = Some(text);
        }
        self.text_ref_generated = false;
        text
    }
}
