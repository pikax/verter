//! Vapor template code generation.
//!
//! Vapor mode compiles `<template vapor>` to direct DOM manipulation code:
//! - Static HTML is hoisted into `_template()` constants
//! - Dynamic parts use `_renderEffect()` with setters (`_setText`, `_setClass`, etc.)
//! - Events use `_delegateEvents` / `_createInvoker` for delegation
//!
//! ## Architecture
//!
//! Like the VDOM backend, vapor uses the "store on open, emit on close" pattern:
//! - Element open: record metadata, build static HTML open tag
//! - Text: store in parent's `text_parts` (deferred)
//! - Interpolation: mark parent as dynamic, store expression
//! - Element close: finalize HTML, emit node creation + effects via `code_transform`
//!
//! ## Nested Element Navigation
//!
//! Non-root elements with dynamic content use `_child()` / `_next()` to navigate
//! from the root node to nested elements. Navigation instructions, effects, and
//! statements are collected into pending vectors during the tree walk, then emitted
//! when the root element closes.
//!
//! Variable naming:
//! - `n{X}` — node references for elements that have dynamic content (effects/statements)
//! - `p{X}` — path variables for intermediate navigation (stepping stones)
//! - `x{X}` — text node references via `_txt()`
//!
//! ## Template HTML Building
//!
//! Each root element accumulates a static HTML string in `current_html`.
//! Trailing close tags are stripped via the `pending_close_tags` mechanism.

mod types;

use std::{cell::RefCell, rc::Rc};

use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        binding_types::BindingType,
        plugin::SyntaxPluginContext,
        types::{
            Comment, CompiledRootTemplateEnd, CompiledRootTemplateStart, ElementKind, ElementScope,
            OxcCompiledElementClosed, OxcCompiledElementStart, OxcInterpolation, PropKind, Text,
        },
    },
};

use super::shared::helper::{apply_dynamic_arg_prefix, build_prefixed_value, escape_js_string};
use crate::syntax_kai::plugins::code_gen::types::VaporImportDependencies;

use types::{VaporElementState, VaporScopeKind, VaporSlotInfo, VaporTextPart, VaporVIfChainState};

/// Events that can be delegated (handled via event delegation at document level).
const DELEGATABLE_EVENTS: &[&str] = &[
    "auxclick",
    "click",
    "contextmenu",
    "dblclick",
    "focusin",
    "focusout",
    "input",
    "keydown",
    "keyup",
    "mousedown",
    "mouseenter",
    "mouseleave",
    "mousemove",
    "mouseout",
    "mouseover",
    "mouseup",
    "pointerdown",
    "pointerenter",
    "pointerleave",
    "pointermove",
    "pointerout",
    "pointerover",
    "pointerup",
    "submit",
    "touchend",
    "touchmove",
    "touchstart",
];

pub(crate) struct VaporTemplateGenerator<'alloc> {
    code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
    bindings: FxHashMap<&'alloc str, BindingType>,
    #[allow(dead_code)] // Used in future phases for production optimizations
    is_production: bool,
    imports: VaporImportDependencies,

    /// Element stack (depth-first).
    stack: Vec<VaporElementState>,

    /// Hoisted template HTML strings (one per root element).
    templates: Vec<String>,

    /// HTML buffer for the current root element's template.
    current_html: String,
    /// Pending close tags for the HTML buffer (stripped if trailing).
    pending_close_tags: Vec<String>,

    /// Node reference counter (`n0`, `n1`, ...).
    node_counter: u32,
    /// Text node reference counter (`x0`, `x1`, ...).
    text_node_counter: u32,
    /// Path variable counter (`p0`, `p1`, ...) for intermediate navigation.
    path_counter: u32,

    /// Position of `<template>` open tag start — hoisted constants emitted here.
    template_start_pos: u32,

    /// Root node reference indices for the return statement.
    root_nodes: Vec<u32>,

    /// Delegated event names (unique, for `_delegateEvents(...)` call).
    delegated_events: Vec<String>,

    /// Collected navigation instructions for the current root element.
    pending_nav: Vec<String>,
    /// Collected text node creations (`const x{N} = _txt(n{X})`).
    pending_text_creations: Vec<String>,
    /// Collected effects from nested dynamic descendants.
    pending_nested_effects: Vec<String>,
    /// Collected statements from nested dynamic descendants.
    pending_nested_statements: Vec<String>,

    /// Whether any element uses `ref` — triggers `_createTemplateRefSetter()` once.
    has_template_ref: bool,
    /// Resolved custom directive names for deduplication.
    resolved_directives: Vec<String>,
    /// Resolved directive declarations to emit at top of render function.
    resolved_directive_decls: Vec<String>,

    inside_template: bool,

    // ── Structural directive state ──────────────────────────────────────
    /// Resolved component names for `_resolveComponent` declarations (deduped).
    resolved_components: Vec<String>,
    /// Resolved component declarations to emit before render function.
    resolved_component_decls: Vec<String>,

    /// Active v-if chain states. When a v-if element opens, a chain is started.
    /// When a v-else-if/v-else follows, it extends the chain. When a non-continuation
    /// sibling appears (or parent closes), the chain is flushed.
    pending_vif_chains: Vec<VaporVIfChainState>,

    /// Current v-for nesting depth (for `_for_item0`, `_for_item1` naming).
    for_depth: u32,

    /// Counter for `_slotProps0`, `_slotProps1` naming.
    #[allow(dead_code)] // Used in future phases for scoped slots
    slot_props_counter: u32,
}

impl<'alloc> VaporTemplateGenerator<'alloc> {
    pub(crate) fn new(
        code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
        is_production: bool,
    ) -> Self {
        Self {
            code_transform,
            bindings: FxHashMap::default(),
            is_production,
            imports: VaporImportDependencies::default(),
            stack: Vec::new(),
            templates: Vec::new(),
            current_html: String::new(),
            pending_close_tags: Vec::new(),
            node_counter: 0,
            text_node_counter: 0,
            path_counter: 0,
            template_start_pos: 0,
            root_nodes: Vec::new(),
            delegated_events: Vec::new(),
            pending_nav: Vec::new(),
            pending_text_creations: Vec::new(),
            pending_nested_effects: Vec::new(),
            pending_nested_statements: Vec::new(),
            has_template_ref: false,
            resolved_directives: Vec::new(),
            resolved_directive_decls: Vec::new(),
            inside_template: false,
            resolved_components: Vec::new(),
            resolved_component_decls: Vec::new(),
            pending_vif_chains: Vec::new(),
            for_depth: 0,
            slot_props_counter: 0,
        }
    }

    pub(crate) fn set_bindings(&mut self, bindings: FxHashMap<&'alloc str, BindingType>) {
        self.bindings = bindings;
    }

    pub(crate) fn get_code(&self) -> String {
        self.code_transform.borrow().to_string()
    }

    pub(crate) fn generate_source_map(&self) -> String {
        self.code_transform
            .borrow()
            .generate_map_json(Default::default())
    }

    pub(crate) fn is_inside_template(&self) -> bool {
        self.inside_template
    }

    /// Allocate a new node reference index.
    fn next_node_ref(&mut self) -> u32 {
        let idx = self.node_counter;
        self.node_counter += 1;
        idx
    }

    /// Allocate a new text node reference index.
    fn next_text_node_ref(&mut self) -> u32 {
        let idx = self.text_node_counter;
        self.text_node_counter += 1;
        idx
    }

    /// Allocate a new path variable index.
    fn next_path_ref(&mut self) -> u32 {
        let idx = self.path_counter;
        self.path_counter += 1;
        idx
    }

    // ── HTML buffer helpers ─────────────────────────────────────────────

    /// Flush pending close tags into the HTML buffer.
    fn flush_pending_close_tags(&mut self) {
        for tag in self.pending_close_tags.drain(..) {
            self.current_html.push_str(&tag);
        }
    }

    /// Append content to the HTML buffer, flushing pending close tags first.
    fn append_html(&mut self, content: &str) {
        self.flush_pending_close_tags();
        self.current_html.push_str(content);
    }

    /// Push a close tag to the pending list (will be flushed or stripped).
    fn push_close_tag(&mut self, tag_name: &str) {
        self.pending_close_tags.push(format!("</{}>", tag_name));
    }

    /// Finalize the HTML buffer: discard pending close tags (they're trailing).
    fn finalize_html(&mut self) -> String {
        self.pending_close_tags.clear();
        std::mem::take(&mut self.current_html)
    }

    /// Flush text_parts from the top-of-stack element into the HTML buffer.
    /// Only called for static parents (no dynamic children).
    fn flush_text_parts_to_html(&mut self) {
        let parts: Vec<String> = if let Some(state) = self.stack.last_mut() {
            state
                .text_parts
                .drain(..)
                .filter_map(|p| match p {
                    VaporTextPart::Static(s) => Some(s),
                    VaporTextPart::Dynamic(_) => None,
                })
                .collect()
        } else {
            return;
        };
        for part in parts {
            self.append_html(&part);
        }
    }

    // ── Build static HTML open tag ──────────────────────────────────────

    /// Build the static HTML open tag string from tag name + static props.
    fn build_static_open_tag(
        &self,
        tag_name: &str,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> String {
        let mut html = format!("<{}", tag_name);

        for prop in &ev.event.props {
            match prop.kind {
                PropKind::Value | PropKind::ClassValue | PropKind::StyleValue => {
                    let attr_name = &ctx.input[prop.start as usize..prop.name_end as usize];
                    // Skip `ref` — handled at runtime via _setTemplateRef.
                    if attr_name == "ref" {
                        continue;
                    }
                    if let Some(ref val) = prop.value {
                        let attr_val = &ctx.input[val.start as usize..val.end as usize];
                        html.push_str(&format!(" {}=\"{}\"", attr_name, attr_val));
                    } else {
                        html.push_str(&format!(" {}", attr_name));
                    }
                }
                _ => {}
            }
        }

        html.push('>');
        html
    }

    // ── Navigation helpers ──────────────────────────────────────────────

    /// Ensure all ancestors on the stack from root to the current top have `var_name` set.
    /// Assigns path variables and generates navigation instructions for any that don't.
    fn ensure_ancestor_var_names(&mut self) {
        let stack_len = self.stack.len();

        // Find the first element without a var_name (scanning from top toward bottom).
        let first_without = {
            let mut idx = stack_len;
            for i in (0..stack_len).rev() {
                if self.stack[i].var_name.is_some() {
                    idx = i + 1;
                    break;
                }
                if i == 0 {
                    idx = 0;
                }
            }
            idx
        };

        for i in first_without..stack_len {
            if self.stack[i].var_name.is_some() {
                continue;
            }

            if self.stack[i].is_root {
                let var_name = format!("n{}", self.stack[i].node_ref);
                self.stack[i].var_name = Some(var_name);
                continue;
            }

            // Get parent info (clone to avoid borrow issues).
            let parent_var = self.stack[i - 1].var_name.as_ref().unwrap().clone();
            let prev_nav = self.stack[i - 1].last_nav_child_var.clone();
            let child_index = self.stack[i].child_index;

            let path_ref = self.next_path_ref();
            let var_name = format!("p{}", path_ref);

            let nav = if child_index == 0 {
                self.imports.add(VaporImportDependencies::CHILD);
                format!("  const {} = _child({})", var_name, parent_var)
            } else if let Some(prev_var) = prev_nav {
                self.imports.add(VaporImportDependencies::NEXT);
                format!(
                    "  const {} = _next({}, {})",
                    var_name, prev_var, child_index
                )
            } else {
                // No previous nav ref — create synthetic _child for first child.
                self.imports.add(VaporImportDependencies::CHILD);
                self.imports.add(VaporImportDependencies::NEXT);
                let child0_path = self.next_path_ref();
                let child0_var = format!("p{}", child0_path);
                self.pending_nav
                    .push(format!("  const {} = _child({})", child0_var, parent_var));
                format!(
                    "  const {} = _next({}, {})",
                    var_name, child0_var, child_index
                )
            };

            self.stack[i - 1].last_nav_child_var = Some(var_name.clone());
            self.stack[i].var_name = Some(var_name);
            self.stack[i].needs_node_ref = true;
            self.pending_nav.push(nav);
        }
    }

    /// Build a navigation instruction for a non-root element.
    /// Returns the navigation code line. Also updates parent's `last_nav_child_var`.
    fn build_nav_instruction(&mut self, var_name: &str, child_index: u32) -> String {
        let parent_var = self
            .stack
            .last()
            .unwrap()
            .var_name
            .as_ref()
            .unwrap()
            .clone();
        let prev_nav = self.stack.last().unwrap().last_nav_child_var.clone();

        let nav = if child_index == 0 {
            self.imports.add(VaporImportDependencies::CHILD);
            format!("  const {} = _child({})", var_name, parent_var)
        } else if let Some(prev_var) = prev_nav {
            self.imports.add(VaporImportDependencies::NEXT);
            format!(
                "  const {} = _next({}, {})",
                var_name, prev_var, child_index
            )
        } else {
            // No previous nav ref — create synthetic _child for first child.
            self.imports.add(VaporImportDependencies::CHILD);
            self.imports.add(VaporImportDependencies::NEXT);
            let child0_path = self.next_path_ref();
            let child0_var = format!("p{}", child0_path);
            self.pending_nav
                .push(format!("  const {} = _child({})", child0_var, parent_var));
            format!(
                "  const {} = _next({}, {})",
                var_name, child0_var, child_index
            )
        };

        // Update parent's last navigated child.
        if let Some(parent) = self.stack.last_mut() {
            parent.last_nav_child_var = Some(var_name.to_string());
        }

        nav
    }

    // ── Event handling helpers ───────────────────────────────────────────

    fn is_delegatable(event_name: &str) -> bool {
        DELEGATABLE_EVENTS.contains(&event_name)
    }

    fn has_non_delegatable_modifier(
        modifiers: &Option<Vec<crate::common::Span>>,
        ctx: &SyntaxPluginContext,
    ) -> bool {
        if let Some(mods) = modifiers {
            for m in mods {
                let name = &ctx.input[m.start as usize..m.end as usize];
                if matches!(name, "capture" | "once" | "passive") {
                    return true;
                }
            }
        }
        false
    }

    // ── Main event handlers ─────────────────────────────────────────────

    pub(crate) fn handle_template_start(
        &mut self,
        ev: &CompiledRootTemplateStart,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.template_start_pos = ev.tag_open.start;
        self.inside_template = true;
    }

    pub(crate) fn handle_template_closed(
        &mut self,
        ev: &CompiledRootTemplateEnd,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        // Flush any remaining v-if chains.
        self.flush_pending_vif_chain();

        self.inside_template = false;
        let code_transform = &mut self.code_transform.borrow_mut();

        // Build the return statement.
        let return_stmt = if self.root_nodes.len() == 1 {
            format!("  return n{}\n}}\n", self.root_nodes[0])
        } else if self.root_nodes.is_empty() {
            "  return null\n}\n".to_string()
        } else {
            let refs: Vec<String> = self.root_nodes.iter().map(|r| format!("n{}", r)).collect();
            format!("  return [{}]\n}}\n", refs.join(", "))
        };

        // Build delegateEvents call (before render function).
        let delegate_str = if !self.delegated_events.is_empty() {
            let events: Vec<String> = self
                .delegated_events
                .iter()
                .map(|e| format!("\"{}\"", e))
                .collect();
            self.imports.add(VaporImportDependencies::DELEGATE_EVENTS);
            format!("_delegateEvents({})\n", events.join(", "))
        } else {
            String::new()
        };

        // Build hoisted template constants.
        let mut hoist_str = String::new();
        for (i, html) in self.templates.iter().enumerate() {
            let escaped = escape_js_string(html);
            hoist_str.push_str(&format!("const t{} = _template(\"{}\")\n", i, escaped));
        }

        // Build component resolution declarations (hoisted before render function).
        let mut comp_decl_str = String::new();
        for decl in &self.resolved_component_decls {
            comp_decl_str.push_str(decl);
            comp_decl_str.push('\n');
        }

        // Build import line.
        let import_str = if !self.imports.is_empty() {
            format!(
                "import {{{}}} from 'vue';\n",
                self.imports.to_import_string()
            )
        } else {
            String::new()
        };

        // Emit: imports + hoisted templates + component decls + delegateEvents before render function.
        let preamble = format!(
            "{}{}{}{}",
            import_str, hoist_str, comp_decl_str, delegate_str
        );
        if !preamble.is_empty() {
            code_transform.prepend_left(self.template_start_pos, &preamble);
        }

        let open_tag_end = if let Some(ref content) = ev.content {
            content.start
        } else if let Some(ref tag_close) = ev.tag_close {
            tag_close.start
        } else {
            ev.end
        };

        // Build render-function-level declarations.
        let mut render_decls = String::new();
        if self.has_template_ref {
            render_decls.push_str("  const _setTemplateRef = _createTemplateRefSetter()\n");
        }
        for decl in &self.resolved_directive_decls {
            render_decls.push_str(decl);
            render_decls.push('\n');
        }

        code_transform.overwrite(
            self.template_start_pos,
            open_tag_end,
            &format!("\nexport function render(_ctx) {{\n{}", render_decls),
        );

        // Replace </template> with return + close.
        if let Some(tag_close) = &ev.tag_close {
            code_transform.overwrite(tag_close.start, tag_close.end, &return_stmt);
        } else {
            code_transform.append_right(ev.end, &return_stmt);
        }
    }

    pub(crate) fn handle_element_start(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let open_tag = &ev.event.event_open_tag;
        let open_tag_end = &ev.event.event_open_tag_end;

        let tag_name =
            ctx.input[(open_tag.start + 1) as usize..open_tag.name_end as usize].to_string();

        // Detect element kind.
        let kind = &open_tag.kind;
        let is_component = kind.is_component();
        let is_slot_outlet = *kind == ElementKind::SlotOutlet;
        let is_dynamic_component = *kind == ElementKind::DynamicComponent;
        let is_template_element = *kind == ElementKind::Template;

        // Detect structural directives from scopes.
        let is_vif_continuation = ev
            .scopes
            .iter()
            .any(|s| matches!(s, ElementScope::ElseIf(_) | ElementScope::Else(_)));
        let has_vif = ev.scopes.iter().any(|s| matches!(s, ElementScope::If(_)));
        let has_vfor = ev.scopes.iter().any(|s| matches!(s, ElementScope::For(_)));
        let is_structural = has_vif
            || is_vif_continuation
            || has_vfor
            || is_component
            || is_slot_outlet
            || is_dynamic_component;

        let is_root = self.stack.is_empty();

        // Flush any pending v-if chain if this element is NOT a continuation.
        if !is_vif_continuation && !is_root {
            self.flush_pending_vif_chain();
        }

        // If nested, handle parent state before this element.
        if !is_root {
            // Reset parent's text_child_started (element breaks text sequence).
            if let Some(parent) = self.stack.last_mut() {
                parent.text_child_started = false;
            }

            // Flush parent's pending text_parts to HTML (only for non-structural parents).
            if !is_structural {
                let parent_is_static = self
                    .stack
                    .last()
                    .map(|s| !s.has_dynamic_children)
                    .unwrap_or(false);
                if parent_is_static {
                    self.flush_text_parts_to_html();
                }
            }
        }

        let node_ref = self.next_node_ref();

        let mut state = VaporElementState::new(
            node_ref,
            tag_name.clone(),
            is_root,
            open_tag.is_void_element,
            open_tag_end.is_self_closing,
            open_tag.start,
            open_tag_end.end,
        );

        state.is_component = is_component;
        state.is_slot_outlet = is_slot_outlet;
        state.is_dynamic_component = is_dynamic_component;
        state.is_template_element = is_template_element;

        // Process structural scopes.
        self.process_scopes(ev, ctx, &mut state);

        // Detect built-in components and register component resolution.
        if is_component || is_dynamic_component {
            self.setup_component(&tag_name, &mut state, ev, ctx);
        }

        if is_root {
            // Root elements get var_name immediately.
            state.var_name = Some(format!("n{}", node_ref));
            // Start fresh HTML buffer.
            if !is_component && !is_slot_outlet && !is_dynamic_component {
                self.current_html.clear();
                self.pending_close_tags.clear();
            }
        } else if !is_vif_continuation {
            // Non-root, non-continuation: set child_index and increment parent's child_count.
            if let Some(parent) = self.stack.last_mut() {
                state.child_index = parent.child_count;
                // Structural children don't count as DOM children for navigation
                // (they're virtual nodes, not real DOM children).
                if !is_structural {
                    parent.child_count += 1;
                }
            }
        }

        // For structural elements, save the parent's HTML buffer and start fresh.
        // The structural element's HTML will be built into its own template.
        if is_structural && !is_component && !is_slot_outlet && !is_dynamic_component {
            // Save parent HTML state — will be restored when the structural element closes.
            // For now, just clear the buffer; structural elements get their own template.
            self.current_html.clear();
            self.pending_close_tags.clear();
        }

        // Build HTML for native elements (including those with structural directives).
        // Components, slot outlets, and dynamic components don't produce HTML.
        if !is_component && !is_slot_outlet && !is_dynamic_component && !is_template_element {
            let html_tag = self.build_static_open_tag(&tag_name, ev, ctx);
            self.append_html(&html_tag);
        }

        // Push state onto stack BEFORE processing props.
        self.stack.push(state);

        // Process props: classify static vs dynamic.
        if !is_slot_outlet {
            self.process_props(ev, ctx);
        }

        // Void/self-closing elements: complete immediately.
        if open_tag_end.is_self_closing || open_tag.is_void_element {
            self.complete_element_close(None);
        }
    }

    pub(crate) fn handle_element_closed(
        &mut self,
        ev: &OxcCompiledElementClosed,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.complete_element_close(ev.event.event_close_tag.as_ref());
    }

    /// Shared logic for completing an element close (both normal and void/self-closing).
    fn complete_element_close(
        &mut self,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) {
        // Flush any pending v-if chain from children before closing this element,
        // BUT only if this element is not itself a v-else-if/v-else continuation
        // (those need to extend the chain, not flush it).
        let is_continuation = self
            .stack
            .last()
            .and_then(|s| s.scope.as_ref())
            .map(|s| matches!(s, VaporScopeKind::ElseIf { .. } | VaporScopeKind::Else))
            .unwrap_or(false);
        if !is_continuation {
            self.flush_pending_vif_chain();
        }

        let mut state = self
            .stack
            .pop()
            .expect("Element close without matching open");

        let is_structural = state.scope.is_some();
        let is_component = state.is_component || state.is_dynamic_component;
        let is_slot_outlet = state.is_slot_outlet;

        // Handle structural elements (v-if, v-for).
        if is_structural {
            self.complete_structural_element_close(&mut state, close_tag);
            return;
        }

        // Handle component elements.
        if is_component || is_slot_outlet {
            self.complete_component_element_close(&mut state, close_tag);
            return;
        }

        // Handle HTML content and close tag for native elements.
        if state.has_dynamic_children {
            self.append_html(" ");
        } else {
            let texts: Vec<String> = state
                .text_parts
                .drain(..)
                .filter_map(|p| match p {
                    VaporTextPart::Static(s) => Some(s),
                    VaporTextPart::Dynamic(_) => None,
                })
                .collect();
            for text in texts {
                self.append_html(&text);
            }
        }

        if !state.is_void && !state.is_self_closing {
            self.push_close_tag(&state.tag_name);
        }

        if state.is_root {
            self.complete_root_element_close(&mut state, close_tag);
        } else {
            self.complete_non_root_element_close(&mut state, close_tag);
        }
    }

    /// Complete a structural element close (v-if, v-else-if, v-else, v-for).
    fn complete_structural_element_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) {
        let scope = state.scope.take().unwrap();
        let close_end = close_tag.map(|ct| ct.end).unwrap_or(state.open_tag_end);

        match scope {
            VaporScopeKind::If { condition } => {
                self.imports.add(VaporImportDependencies::CREATE_IF);

                // Build the block body for this branch.
                let body = self.build_block_body(state, close_tag, "    ");

                let node_ref = state.node_ref;
                let code = format!(
                    "  const n{} = _createIf(() => ({}), () => {{\n{}  }}",
                    node_ref, condition, body
                );

                // Start a new v-if chain.
                self.pending_vif_chains.push(VaporVIfChainState {
                    node_ref,
                    branch_index: 0,
                    code,
                    open_parens: 1,
                    chain_start: state.open_tag_start,
                    chain_end: close_end,
                    child_index: state.child_index,
                });

                // Remove source from code_transform.
                let code_transform = &mut self.code_transform.borrow_mut();
                code_transform.overwrite(state.open_tag_start, close_end, "");
            }

            VaporScopeKind::ElseIf { condition } => {
                self.imports.add(VaporImportDependencies::CREATE_IF);

                // Build the block body for this branch.
                let body = self.build_block_body(state, close_tag, "    ");

                // Extend the pending v-if chain.
                if let Some(chain) = self.pending_vif_chains.last_mut() {
                    chain.branch_index += 1;
                    // Close the previous _createIf and start a nested one.
                    chain.code.push_str(&format!(
                        ", () => _createIf(() => ({}), () => {{\n{}  }}",
                        condition, body
                    ));
                    chain.open_parens += 1;
                    chain.chain_end = close_end;
                }

                // Remove source from code_transform.
                let code_transform = &mut self.code_transform.borrow_mut();
                code_transform.overwrite(state.open_tag_start, close_end, "");
            }

            VaporScopeKind::Else => {
                // Build the block body for this branch.
                let body = self.build_block_body(state, close_tag, "    ");

                // Extend the pending v-if chain with the else branch.
                if let Some(chain) = self.pending_vif_chains.last_mut() {
                    chain.code.push_str(&format!(", () => {{\n{}  }}", body));
                    chain.chain_end = close_end;

                    // Close all open parens now (chain is complete).
                    for i in (0..chain.open_parens).rev() {
                        chain.code.push_str(&format!(", null, {})", i));
                    }
                    chain.open_parens = 0;

                    // Flush the chain immediately since it's complete.
                    let chain = self.pending_vif_chains.pop().unwrap();
                    let code = chain.code;
                    let code_with_newline = format!("{}\n", code);

                    let is_root = self.stack.is_empty();
                    if is_root {
                        self.root_nodes.push(chain.node_ref);
                        let code_transform = &mut self.code_transform.borrow_mut();
                        code_transform.overwrite(
                            chain.chain_start,
                            chain.chain_end,
                            &code_with_newline,
                        );
                    } else {
                        if let Some(parent) = self.stack.last_mut() {
                            parent.structural_children.push(code_with_newline.clone());
                        }
                        let code_transform = &mut self.code_transform.borrow_mut();
                        code_transform.overwrite(chain.chain_start, chain.chain_end, "");
                    }
                }

                // Remove source from code_transform (already handled above for chain).
                // The chain_start..chain_end overwrite covers this element too.
            }

            VaporScopeKind::For {
                iterable,
                callback_params,
                original_params: _,
                key_fn,
                depth: _,
            } => {
                self.imports.add(VaporImportDependencies::CREATE_FOR);

                // Build the block body.
                let body = self.build_block_body(state, close_tag, "    ");

                let params_str = callback_params.join(", ");
                let node_ref = state.node_ref;

                let mut code = format!(
                    "  const n{} = _createFor(() => ({}), ({}) => {{\n{}  }}",
                    node_ref, iterable, params_str, body
                );

                // Add key function if present.
                if let Some(ref kf) = key_fn {
                    code.push_str(&format!(", {}", kf));
                }

                code.push_str(")\n");

                // Decrement for_depth.
                self.for_depth = self.for_depth.saturating_sub(1);

                let is_root = self.stack.is_empty();
                if is_root {
                    self.root_nodes.push(node_ref);
                    let code_transform = &mut self.code_transform.borrow_mut();
                    code_transform.overwrite(state.open_tag_start, close_end, &code);
                } else {
                    if let Some(parent) = self.stack.last_mut() {
                        parent.structural_children.push(code);
                    }
                    let code_transform = &mut self.code_transform.borrow_mut();
                    code_transform.overwrite(state.open_tag_start, close_end, "");
                }
            }
        }
    }

    /// Complete a component element close: build component call with slots.
    fn complete_component_element_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) {
        let close_end = close_tag.map(|ct| ct.end).unwrap_or(state.open_tag_end);

        // Collect default slot from children if any structural children exist.
        if !state.structural_children.is_empty() {
            let mut slot_body = String::new();
            for child in state.structural_children.drain(..) {
                slot_body.push_str(&child);
            }
            // Check if there's already a default slot.
            let has_default = state.slot_children.iter().any(|s| s.name == "default");
            if !has_default && !slot_body.is_empty() {
                state.slot_children.push(VaporSlotInfo {
                    name: "default".to_string(),
                    is_dynamic: false,
                    dynamic_name_expr: None,
                    params: None,
                    body: slot_body,
                });
            }
        }

        // Build the component call.
        let comp_code = self.build_component_call(state, "  ");
        let node_ref = state.node_ref;
        let code = format!("  const n{} = {}\n", node_ref, comp_code);

        let is_root = self.stack.is_empty();
        if is_root {
            self.root_nodes.push(node_ref);
            let code_transform = &mut self.code_transform.borrow_mut();
            code_transform.overwrite(state.open_tag_start, close_end, &code);
        } else {
            if let Some(parent) = self.stack.last_mut() {
                parent.structural_children.push(code);
            }
            let code_transform = &mut self.code_transform.borrow_mut();
            code_transform.overwrite(state.open_tag_start, close_end, "");
        }
    }

    /// Complete a root element close: finalize HTML, emit template + navigation + effects.
    fn complete_root_element_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) {
        let html = self.finalize_html();
        let template_idx = self.templates.len() as u32;
        self.templates.push(html);
        self.imports.add(VaporImportDependencies::TEMPLATE);

        // Build creation code: template instantiation + navigation + text creations.
        let mut creation = format!("  const n{} = t{}()\n", state.node_ref, template_idx);

        // Root's own text node creation (if root has dynamic text directly).
        if state.has_dynamic_children && !state.text_parts.is_empty() {
            let text_ref = state.text_node_ref.unwrap();
            self.imports.add(VaporImportDependencies::TXT);
            creation.push_str(&format!(
                "  const x{} = _txt(n{})\n",
                text_ref, state.node_ref
            ));
        }

        // Append navigation instructions from nested elements.
        for nav in self.pending_nav.drain(..) {
            creation.push_str(&nav);
            creation.push('\n');
        }

        // Append text node creations from nested elements.
        for tc in self.pending_text_creations.drain(..) {
            creation.push_str(&tc);
            creation.push('\n');
        }

        // Build close code: effects + statements.
        let mut close_code = String::new();

        // Collect all effects: root's own + nested.
        let mut all_effects = std::mem::take(&mut state.effects);

        if state.has_dynamic_children && !state.text_parts.is_empty() {
            self.imports.add(VaporImportDependencies::SET_TEXT);
            let text_ref = state.text_node_ref.unwrap();
            let set_text = build_set_text_call(text_ref, &state.text_parts);
            all_effects.push(set_text);
        }

        all_effects.append(&mut self.pending_nested_effects);

        if !all_effects.is_empty() {
            self.imports.add(VaporImportDependencies::RENDER_EFFECT);
            if all_effects.len() == 1 {
                close_code.push_str(&format!("  _renderEffect(() => {})\n", all_effects[0]));
            } else {
                close_code.push_str("  _renderEffect(() => {\n");
                for effect in &all_effects {
                    close_code.push_str(&format!("    {}\n", effect));
                }
                close_code.push_str("  })\n");
            }
        }

        // Collect all statements: root's own + nested.
        for stmt in &state.statements {
            close_code.push_str(&format!("  {}\n", stmt));
        }
        for stmt in self.pending_nested_statements.drain(..) {
            close_code.push_str(&format!("  {}\n", stmt));
        }

        self.root_nodes.push(state.node_ref);

        // Now borrow code_transform and emit.
        let code_transform = &mut self.code_transform.borrow_mut();

        code_transform.overwrite(state.open_tag_start, state.open_tag_end, &creation);

        let close_start = if let Some(ct) = close_tag {
            ct.start
        } else {
            state.open_tag_end
        };

        if close_start > state.open_tag_end {
            code_transform.overwrite(state.open_tag_end, close_start, "");
        }

        if let Some(ct) = close_tag {
            code_transform.overwrite(ct.start, ct.end, &close_code);
        } else if !close_code.is_empty() {
            code_transform.append_right(state.open_tag_end, &close_code);
        }
    }

    /// Complete a non-root element close: determine if navigation is needed,
    /// and push effects/statements to pending vectors for the root to emit.
    fn complete_non_root_element_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) {
        let has_own_dynamic = !state.effects.is_empty()
            || !state.statements.is_empty()
            || (state.has_dynamic_children && !state.text_parts.is_empty());

        let needs_nav = has_own_dynamic || state.needs_node_ref;

        // If navigation is needed and not yet generated, generate it now.
        if needs_nav && state.var_name.is_none() {
            self.ensure_ancestor_var_names();

            let var_name = if has_own_dynamic {
                format!("n{}", state.node_ref)
            } else {
                let p = self.next_path_ref();
                format!("p{}", p)
            };

            let nav = self.build_nav_instruction(&var_name, state.child_index);
            self.pending_nav.push(nav);

            state.var_name = Some(var_name);
        }

        // Push dynamic content to pending vectors.
        if has_own_dynamic {
            let var_name = state.var_name.as_ref().unwrap().clone();

            // Text node creation + setText effect.
            if state.has_dynamic_children && !state.text_parts.is_empty() {
                let text_ref = state.text_node_ref.unwrap();
                self.imports.add(VaporImportDependencies::TXT);
                self.pending_text_creations
                    .push(format!("  const x{} = _txt({})", text_ref, var_name));

                self.imports.add(VaporImportDependencies::SET_TEXT);
                let set_text = build_set_text_call(text_ref, &state.text_parts);
                self.pending_nested_effects.push(set_text);
            }

            // Effects and statements.
            for effect in state.effects.drain(..) {
                self.pending_nested_effects.push(effect);
            }
            for stmt in state.statements.drain(..) {
                self.pending_nested_statements.push(stmt);
            }
        }

        // Remove source from code_transform.
        let code_transform = &mut self.code_transform.borrow_mut();
        code_transform.overwrite(state.open_tag_start, state.open_tag_end, "");
        if let Some(ct) = close_tag {
            code_transform.overwrite(ct.start, ct.end, "");
        }
    }

    pub(crate) fn handle_comment(&mut self, ev: &Comment, ctx: &SyntaxPluginContext<'alloc>) {
        let content = &ctx.input[ev.content.start as usize..ev.content.end as usize];
        self.append_html(&format!("<!--{}-->", content));

        let code_transform = &mut self.code_transform.borrow_mut();
        code_transform.overwrite(ev.start, ev.end, "");
    }

    pub(crate) fn handle_text(&mut self, ev: &Text, ctx: &SyntaxPluginContext<'alloc>) {
        let text = &ctx.input[ev.start as usize..ev.end as usize];

        if let Some(state) = self.stack.last_mut() {
            state
                .text_parts
                .push(VaporTextPart::Static(text.to_string()));

            // Track child_count: consecutive text/interpolation = one DOM child.
            if !state.text_child_started {
                state.child_count += 1;
                state.text_child_started = true;
            }
        }

        let code_transform = &mut self.code_transform.borrow_mut();
        code_transform.overwrite(ev.start, ev.end, "");
    }

    pub(crate) fn handle_interpolation(
        &mut self,
        ev: &OxcInterpolation<'alloc>,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let raw_content = &_ctx.input[ev.content.start as usize..ev.content.end as usize];
        let leading_ws = raw_content.len() - raw_content.trim_start().len();
        let trailing_ws = raw_content.len() - raw_content.trim_end().len();
        let trimmed_start = ev.content.start + leading_ws as u32;
        let trimmed_end = ev.content.end - trailing_ws as u32;
        let expr_text = &_ctx.input[trimmed_start as usize..trimmed_end as usize];
        let prefixed = build_prefixed_value(
            expr_text,
            trimmed_start,
            &ev.bindings,
            &self.bindings,
            false,
        );

        self.imports.add(VaporImportDependencies::TO_DISPLAY_STRING);
        let display_expr = format!("_toDisplayString({})", prefixed);

        let needs_text_ref = if let Some(state) = self.stack.last_mut() {
            state.has_dynamic_children = true;
            state.text_parts.push(VaporTextPart::Dynamic(display_expr));

            // Track child_count: consecutive text/interpolation = one DOM child.
            if !state.text_child_started {
                state.child_count += 1;
                state.text_child_started = true;
            }

            state.text_node_ref.is_none()
        } else {
            false
        };

        if needs_text_ref {
            let text_ref = self.next_text_node_ref();
            if let Some(state) = self.stack.last_mut() {
                state.text_node_ref = Some(text_ref);
            }
        }

        let code_transform = &mut self.code_transform.borrow_mut();
        code_transform.overwrite(ev.start, ev.end, "");
    }

    // ── Structural directive processing ────────────────────────────────

    /// Process structural directive scopes (v-if, v-else-if, v-else, v-for, v-slot).
    fn process_scopes(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
        state: &mut VaporElementState,
    ) {
        for scope in &ev.scopes {
            match scope {
                ElementScope::If(cond) => {
                    let condition = if let Some(ref val_span) = cond.event.value {
                        let raw = &ctx.input[val_span.start as usize..val_span.end as usize];
                        build_prefixed_value(
                            raw,
                            val_span.start,
                            &cond.bindings,
                            &self.bindings,
                            false,
                        )
                    } else {
                        "true".to_string()
                    };
                    state.scope = Some(VaporScopeKind::If { condition });
                }
                ElementScope::ElseIf(cond) => {
                    let condition = if let Some(ref val_span) = cond.event.value {
                        let raw = &ctx.input[val_span.start as usize..val_span.end as usize];
                        build_prefixed_value(
                            raw,
                            val_span.start,
                            &cond.bindings,
                            &self.bindings,
                            false,
                        )
                    } else {
                        "true".to_string()
                    };
                    state.scope = Some(VaporScopeKind::ElseIf { condition });
                }
                ElementScope::Else(_) => {
                    state.scope = Some(VaporScopeKind::Else);
                }
                ElementScope::For(vfor) => {
                    // The v-for value span contains the full expression: "item in items"
                    // We need to extract just the iterable (right side).
                    // `right_offset()` is already absolute (file-relative).
                    let val_span = vfor.event.value.as_ref();
                    let iterable = if let Some(val) = val_span {
                        let right_offset = vfor.parsed.right_offset();
                        let iterable_raw = &ctx.input[right_offset as usize..val.end as usize];
                        // Apply _ctx. prefix to external references manually
                        // (same approach as VDOM backend).
                        let mut result_str = iterable_raw.to_string();
                        let mut refs: Vec<_> = vfor.parsed.references.iter().collect();
                        refs.sort_by(|a, b| b.start.cmp(&a.start));
                        for r in refs {
                            // Only apply to references within the iterable range.
                            if r.start >= right_offset && r.end <= val.end {
                                let offset = (r.start - right_offset) as usize;
                                let name = &ctx.input[r.start as usize..r.end as usize];
                                let prefix = if let Some(bt) = self.bindings.get(name) {
                                    bt.accessor_prefix(false)
                                } else {
                                    "_ctx."
                                };
                                if !prefix.is_empty() {
                                    result_str.insert_str(offset, prefix);
                                }
                            }
                        }
                        result_str
                    } else {
                        "[]".to_string()
                    };

                    // Build callback parameter names.
                    let depth = self.for_depth;
                    let original_params: Vec<String> = vfor
                        .parsed
                        .locals
                        .iter()
                        .map(|span| ctx.input[span.start as usize..span.end as usize].to_string())
                        .collect();

                    let callback_params: Vec<String> = (0..original_params.len().max(1))
                        .map(|i| match i {
                            0 => format!("_for_item{}", depth),
                            1 => format!("_for_key{}", depth),
                            _ => format!("_for_index{}", depth),
                        })
                        .collect();

                    // Extract :key expression if present.
                    let key_fn = self.extract_key_fn(ev, ctx, &original_params);

                    state.scope = Some(VaporScopeKind::For {
                        iterable,
                        callback_params,
                        original_params,
                        key_fn,
                        depth,
                    });
                    self.for_depth += 1;
                }
                ElementScope::SlotElement(_) | ElementScope::SlotTemplate(_) => {
                    // Slots are handled during component close.
                }
                ElementScope::Once(_) => {
                    // v-once not yet handled in vapor.
                }
            }
        }
    }

    /// Extract `:key` expression from element props for v-for key function.
    fn extract_key_fn(
        &self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
        original_params: &[String],
    ) -> Option<String> {
        for oxc_prop in &ev.props {
            let prop = &oxc_prop.event;
            if prop.kind == PropKind::Bind {
                if let Some(ref arg) = prop.arg {
                    let attr_name = &ctx.input[arg.start as usize..arg.end as usize];
                    if attr_name == "key" {
                        if let Some(ref exp) = oxc_prop.exp {
                            let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                            // The key function uses original param names, not _for_item{N}.
                            let params_str = original_params.join(", ");
                            return Some(format!("({}) => ({})", params_str, expr_text));
                        }
                    }
                }
            }
        }
        None
    }

    /// Set up component resolution and detect built-in components.
    fn setup_component(
        &mut self,
        tag_name: &str,
        state: &mut VaporElementState,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let lower = tag_name.to_lowercase();

        match lower.as_str() {
            "teleport" => {
                self.imports.add(VaporImportDependencies::VAPOR_TELEPORT);
                self.imports.add(VaporImportDependencies::CREATE_COMPONENT);
                state.component_var = Some("_VaporTeleport".to_string());
            }
            "transition" => {
                self.imports.add(VaporImportDependencies::VAPOR_TRANSITION);
                self.imports.add(VaporImportDependencies::CREATE_COMPONENT);
                state.component_var = Some("_VaporTransition".to_string());
            }
            "transition-group" | "transitiongroup" => {
                self.imports
                    .add(VaporImportDependencies::VAPOR_TRANSITION_GROUP);
                self.imports.add(VaporImportDependencies::CREATE_COMPONENT);
                state.component_var = Some("_VaporTransitionGroup".to_string());
            }
            "keep-alive" | "keepalive" => {
                // KeepAlive uses _resolveComponent + _createComponentWithFallback.
                self.imports.add(VaporImportDependencies::RESOLVE_COMPONENT);
                self.imports
                    .add(VaporImportDependencies::CREATE_COMPONENT_WITH_FALLBACK);
                self.imports.add(VaporImportDependencies::WITH_VAPOR_CTX);
                let comp_var = format!("_component_{}", tag_name.replace('-', "_"));
                if !self.resolved_components.contains(&tag_name.to_string()) {
                    self.resolved_components.push(tag_name.to_string());
                    self.resolved_component_decls.push(format!(
                        "const {} = _resolveComponent(\"{}\")",
                        comp_var, tag_name
                    ));
                }
                state.component_var = Some(comp_var);
                state.needs_vapor_ctx = true;
            }
            "suspense" => {
                // Suspense uses _resolveComponent + _createComponentWithFallback.
                self.imports.add(VaporImportDependencies::RESOLVE_COMPONENT);
                self.imports
                    .add(VaporImportDependencies::CREATE_COMPONENT_WITH_FALLBACK);
                self.imports.add(VaporImportDependencies::WITH_VAPOR_CTX);
                let comp_var = format!("_component_{}", tag_name.replace('-', "_"));
                if !self.resolved_components.contains(&tag_name.to_string()) {
                    self.resolved_components.push(tag_name.to_string());
                    self.resolved_component_decls.push(format!(
                        "const {} = _resolveComponent(\"{}\")",
                        comp_var, tag_name
                    ));
                }
                state.component_var = Some(comp_var);
                state.needs_vapor_ctx = true;
            }
            _ if state.is_dynamic_component => {
                // <component :is="expr"> → _createDynamicComponent
                self.imports
                    .add(VaporImportDependencies::CREATE_DYNAMIC_COMPONENT);
                // Extract :is expression.
                for oxc_prop in &ev.props {
                    let prop = &oxc_prop.event;
                    if prop.kind == PropKind::Bind {
                        if let Some(ref arg) = prop.arg {
                            let attr_name = &ctx.input[arg.start as usize..arg.end as usize];
                            if attr_name == "is" {
                                if let Some(ref exp) = oxc_prop.exp {
                                    let expr_text =
                                        &ctx.input[exp.start as usize..exp.end as usize];
                                    let prefixed = build_prefixed_value(
                                        expr_text,
                                        exp.start,
                                        &exp.bindings,
                                        &self.bindings,
                                        false,
                                    );
                                    state.dynamic_is_expr = Some(prefixed);
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                // Regular user component.
                self.imports.add(VaporImportDependencies::RESOLVE_COMPONENT);
                self.imports
                    .add(VaporImportDependencies::CREATE_COMPONENT_WITH_FALLBACK);
                let comp_var = format!("_component_{}", tag_name.replace('-', "_"));
                if !self.resolved_components.contains(&tag_name.to_string()) {
                    self.resolved_components.push(tag_name.to_string());
                    self.resolved_component_decls.push(format!(
                        "const {} = _resolveComponent(\"{}\")",
                        comp_var, tag_name
                    ));
                }
                state.component_var = Some(comp_var);
            }
        }
    }

    /// Flush any pending v-if chain (emit the accumulated code).
    /// Called when a non-continuation sibling appears or when the parent closes.
    fn flush_pending_vif_chain(&mut self) {
        if let Some(chain) = self.pending_vif_chains.pop() {
            let mut code = chain.code;

            // Close all open _createIf parens that haven't been closed by v-else.
            // Each open paren corresponds to a _createIf( call.
            // For a simple v-if (no else), open_parens=1, branch_index=0 → just close with `)`
            // For v-if/v-else-if (no else), open_parens=2, branch_index=1 → close inner then outer
            if chain.open_parens > 0 {
                // Close from innermost to outermost.
                // The innermost _createIf has the highest branch index.
                for _ in 0..chain.open_parens {
                    code.push(')');
                }
            }

            code.push('\n');

            let is_root = self.stack.is_empty();
            if is_root {
                // Root-level v-if: emit as a root node.
                self.root_nodes.push(chain.node_ref);
                let code_transform = &mut self.code_transform.borrow_mut();
                code_transform.overwrite(chain.chain_start, chain.chain_end, &code);
            } else {
                // Nested v-if: emit as a structural child of the parent.
                if let Some(parent) = self.stack.last_mut() {
                    parent.structural_children.push(code);
                }
                let code_transform = &mut self.code_transform.borrow_mut();
                code_transform.overwrite(chain.chain_start, chain.chain_end, "");
            }
        }
    }

    /// Build a block body for a structural element (v-if branch, v-for iteration, slot).
    /// This generates the template instantiation, navigation, effects, and return statement
    /// as a string that can be used inside a block function.
    ///
    /// The `state.node_ref` is used for the outer structural directive result (e.g., `_createIf`).
    /// A new inner node_ref is allocated for the template instantiation inside the block.
    fn build_block_body(
        &mut self,
        state: &mut VaporElementState,
        _close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
        indent: &str,
    ) -> String {
        let mut body = String::new();

        // Allocate a new node_ref for the inner template node.
        // The outer `state.node_ref` is used for the structural directive result.
        let inner_ref = self.next_node_ref();

        if state.is_component || state.is_dynamic_component || state.is_slot_outlet {
            // Component/slot outlet: build the component call.
            let comp_code = self.build_component_call(state, indent);
            body.push_str(&format!("{}const n{} = {}\n", indent, inner_ref, comp_code));
        } else if state.is_template_element {
            // <template v-if/v-for>: children are the block body directly.
            // Structural children from nested v-if/v-for.
            for child in state.structural_children.drain(..) {
                body.push_str(&child);
                body.push('\n');
            }
            // For template wrappers, we don't create a template node.
            return body;
        } else {
            // Native element: finalize HTML and create template.
            // Handle HTML content and close tag.
            if state.has_dynamic_children {
                self.append_html(" ");
            } else {
                let texts: Vec<String> = state
                    .text_parts
                    .drain(..)
                    .filter_map(|p| match p {
                        VaporTextPart::Static(s) => Some(s),
                        VaporTextPart::Dynamic(_) => None,
                    })
                    .collect();
                for text in texts {
                    self.append_html(&text);
                }
            }

            if !state.is_void && !state.is_self_closing {
                self.push_close_tag(&state.tag_name);
            }

            let html = self.finalize_html();
            let template_idx = self.templates.len() as u32;
            self.templates.push(html);
            self.imports.add(VaporImportDependencies::TEMPLATE);

            body.push_str(&format!(
                "{}const n{} = t{}()\n",
                indent, inner_ref, template_idx
            ));

            // Text node creation for dynamic text.
            if state.has_dynamic_children && !state.text_parts.is_empty() {
                let text_ref = state.text_node_ref.unwrap();
                self.imports.add(VaporImportDependencies::TXT);
                body.push_str(&format!(
                    "{}const x{} = _txt(n{})\n",
                    indent, text_ref, inner_ref
                ));
            }

            // Navigation instructions from nested elements.
            for nav in self.pending_nav.drain(..) {
                body.push_str(&nav);
                body.push('\n');
            }

            // Text node creations from nested elements.
            for tc in self.pending_text_creations.drain(..) {
                body.push_str(&tc);
                body.push('\n');
            }

            // Structural children (nested v-if/v-for inside this element).
            for child in state.structural_children.drain(..) {
                body.push_str(&child);
                body.push('\n');
            }
        }

        // Effects — rewrite node_ref references from state.node_ref to inner_ref.
        let mut all_effects = std::mem::take(&mut state.effects);
        let old_ref = format!("n{}", state.node_ref);
        let new_ref = format!("n{}", inner_ref);
        for effect in &mut all_effects {
            *effect = effect.replace(&old_ref, &new_ref);
        }

        if state.has_dynamic_children && !state.text_parts.is_empty() {
            self.imports.add(VaporImportDependencies::SET_TEXT);
            let text_ref = state.text_node_ref.unwrap();
            let set_text = build_set_text_call(text_ref, &state.text_parts);
            all_effects.push(set_text);
        }
        all_effects.append(&mut self.pending_nested_effects);

        if !all_effects.is_empty() {
            self.imports.add(VaporImportDependencies::RENDER_EFFECT);
            if all_effects.len() == 1 {
                body.push_str(&format!(
                    "{}_renderEffect(() => {})\n",
                    indent, all_effects[0]
                ));
            } else {
                body.push_str(&format!("{}_renderEffect(() => {{\n", indent));
                for effect in &all_effects {
                    body.push_str(&format!("{}  {}\n", indent, effect));
                }
                body.push_str(&format!("{}}})\n", indent));
            }
        }

        // Statements — rewrite node_ref references.
        let mut all_stmts: Vec<String> = state
            .statements
            .drain(..)
            .map(|s| s.replace(&old_ref, &new_ref))
            .collect();
        for stmt in self.pending_nested_statements.drain(..) {
            all_stmts.push(stmt);
        }
        for stmt in &all_stmts {
            body.push_str(&format!("{}{}\n", indent, stmt));
        }

        // Return statement.
        body.push_str(&format!("{}return n{}\n", indent, inner_ref));

        body
    }

    /// Build a component call expression (without `const n{X} = ` prefix).
    fn build_component_call(&mut self, state: &mut VaporElementState, indent: &str) -> String {
        if state.is_slot_outlet {
            return self.build_slot_outlet_call(state);
        }

        if state.is_dynamic_component {
            return self.build_dynamic_component_call(state, indent);
        }

        let comp_var = state.component_var.as_ref().unwrap().clone();

        // Determine if this uses _createComponent or _createComponentWithFallback.
        let is_builtin_create = comp_var.starts_with("_Vapor");
        let create_fn = if is_builtin_create {
            "_createComponent"
        } else {
            "_createComponentWithFallback"
        };

        // Build props object.
        let props_str = self.build_component_props(state);

        // Build slots object.
        let slots_str = self.build_component_slots(state, indent);

        format!(
            "{}({}, {}, {}, true)",
            create_fn, comp_var, props_str, slots_str
        )
    }

    /// Build props object for a component call.
    fn build_component_props(&self, state: &VaporElementState) -> String {
        // Collect effects as reactive props: each effect like `_setProp(n{X}, "attr", expr)`
        // becomes `attr: () => (expr)` in the props object.
        // For components, effects are actually prop bindings.
        if state.effects.is_empty() {
            return "null".to_string();
        }

        let mut entries = Vec::new();
        for effect in &state.effects {
            // Parse effect strings to extract prop name and value.
            // Effects for components look like: `_setProp(n{X}, "attr", expr)`
            if let Some(prop_entry) = parse_effect_as_component_prop(effect) {
                entries.push(prop_entry);
            }
        }

        if entries.is_empty() {
            "null".to_string()
        } else {
            format!("{{ {} }}", entries.join(", "))
        }
    }

    /// Build slots object for a component call.
    fn build_component_slots(&mut self, state: &mut VaporElementState, indent: &str) -> String {
        if state.slot_children.is_empty() {
            return "null".to_string();
        }

        let slots = std::mem::take(&mut state.slot_children);
        let needs_vapor_ctx = state.needs_vapor_ctx;

        let mut static_slots = Vec::new();
        let mut dynamic_slots = Vec::new();

        for slot in slots {
            if slot.is_dynamic {
                dynamic_slots.push(slot);
            } else {
                static_slots.push(slot);
            }
        }

        let mut parts = Vec::new();

        for slot in &static_slots {
            let params = slot.params.as_deref().unwrap_or("");
            let wrapper_start = if needs_vapor_ctx && slot.name == "default" {
                format!("_withVaporCtx(({}) => {{\n", params)
            } else {
                format!("({}) => {{\n", params)
            };
            let wrapper_end = if needs_vapor_ctx && slot.name == "default" {
                format!("{}}})", indent)
            } else {
                format!("{}}}", indent)
            };
            parts.push(format!(
                "\"{}\": {}{}{}",
                slot.name, wrapper_start, slot.body, wrapper_end
            ));
        }

        if !dynamic_slots.is_empty() {
            let mut dyn_entries = Vec::new();
            for slot in &dynamic_slots {
                let name_expr = slot.dynamic_name_expr.as_deref().unwrap_or("\"default\"");
                let params = slot.params.as_deref().unwrap_or("");
                dyn_entries.push(format!(
                    "() => ({{\n{}  name: {},\n{}  fn: ({}) => {{\n{}{}\n{}  }}\n{}}})",
                    indent, name_expr, indent, params, indent, slot.body, indent, indent
                ));
            }
            parts.push(format!("$: [{}]", dyn_entries.join(", ")));
        }

        if parts.is_empty() {
            "null".to_string()
        } else {
            format!(
                "{{\n{}  {}\n{}}}",
                indent,
                parts.join(&format!(",\n{}  ", indent)),
                indent
            )
        }
    }

    /// Build a `_createSlot(...)` call for `<slot>` outlets.
    fn build_slot_outlet_call(&mut self, _state: &VaporElementState) -> String {
        self.imports.add(VaporImportDependencies::CREATE_SLOT);
        let slot_name = "\"default\""; // TODO: extract from name prop
        format!("_createSlot({}, null)", slot_name)
    }

    /// Build a `_createDynamicComponent(...)` call.
    fn build_dynamic_component_call(&self, state: &VaporElementState, _indent: &str) -> String {
        let is_expr = state.dynamic_is_expr.as_deref().unwrap_or("undefined");
        format!(
            "_createDynamicComponent(() => ({}), null, null, true)",
            is_expr
        )
    }

    // ── Props processing ────────────────────────────────────────────────

    fn process_props(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let node_ref = self.stack.last().unwrap().node_ref;

        for oxc_prop in &ev.props {
            let prop = &oxc_prop.event;
            match prop.kind {
                PropKind::ClassValue | PropKind::StyleValue => {
                    // Static props are already in the HTML.
                }

                PropKind::Value => {
                    // Static props are already in the HTML.
                    // Detect ref="..." — remove from HTML and emit _setTemplateRef.
                    let attr_name = &ctx.input[prop.start as usize..prop.name_end as usize];
                    if attr_name == "ref" {
                        if let Some(ref val) = prop.value {
                            let ref_name = &ctx.input[val.start as usize..val.end as usize];
                            self.has_template_ref = true;
                            self.imports
                                .add(VaporImportDependencies::CREATE_TEMPLATE_REF_SETTER);
                            let stmt = format!("_setTemplateRef(n{}, \"{}\")", node_ref, ref_name);
                            self.stack.last_mut().unwrap().statements.push(stmt);
                        }
                    }
                }

                PropKind::ClassBind => {
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = build_prefixed_value(
                            expr_text,
                            exp.start,
                            &exp.bindings,
                            &self.bindings,
                            false,
                        );
                        self.imports.add(VaporImportDependencies::SET_CLASS);
                        let effect = format!("_setClass(n{}, {})", node_ref, prefixed);
                        self.stack.last_mut().unwrap().effects.push(effect);
                    }
                }

                PropKind::StyleBind => {
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = build_prefixed_value(
                            expr_text,
                            exp.start,
                            &exp.bindings,
                            &self.bindings,
                            false,
                        );
                        self.imports.add(VaporImportDependencies::SET_STYLE);
                        let effect = format!("_setStyle(n{}, {})", node_ref, prefixed);
                        self.stack.last_mut().unwrap().effects.push(effect);
                    }
                }

                PropKind::Bind => {
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = build_prefixed_value(
                            expr_text,
                            exp.start,
                            &exp.bindings,
                            &self.bindings,
                            false,
                        );

                        if prop.has_dynamic_arg {
                            // :[attrName]="value" → _setDynamicProps(n{X}, [{ [expr]: value }])
                            let arg_span = prop.arg.unwrap();
                            let arg_raw =
                                &ctx.input[arg_span.start as usize..arg_span.end as usize];
                            let arg_prefixed = apply_dynamic_arg_prefix(
                                arg_raw,
                                arg_span.start,
                                &oxc_prop.arg.as_ref().and_then(|a| a.bindings.clone()),
                                &self.bindings,
                                false,
                            );
                            self.imports.add(VaporImportDependencies::SET_DYNAMIC_PROPS);
                            let effect = format!(
                                "_setDynamicProps(n{}, [{{ [{}]: {} }}])",
                                node_ref, arg_prefixed, prefixed
                            );
                            self.stack.last_mut().unwrap().effects.push(effect);
                        } else {
                            let attr_name = if let Some(ref arg) = prop.arg {
                                ctx.input[arg.start as usize..arg.end as usize].to_string()
                            } else {
                                String::new()
                            };

                            self.imports.add(VaporImportDependencies::SET_PROP);
                            let effect =
                                format!("_setProp(n{}, \"{}\", {})", node_ref, attr_name, prefixed);
                            self.stack.last_mut().unwrap().effects.push(effect);
                        }
                    }
                }

                PropKind::BindSpread => {
                    // v-bind="obj" → _setDynamicProps(n{X}, [expr])
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = build_prefixed_value(
                            expr_text,
                            exp.start,
                            &exp.bindings,
                            &self.bindings,
                            false,
                        );
                        self.imports.add(VaporImportDependencies::SET_DYNAMIC_PROPS);
                        let effect = format!("_setDynamicProps(n{}, [{}])", node_ref, prefixed);
                        self.stack.last_mut().unwrap().effects.push(effect);
                    }
                }

                PropKind::On => {
                    self.process_event(oxc_prop, ctx, node_ref);
                }

                PropKind::OnSpread => {
                    // v-on="obj" → _on with dynamic handling
                    // For now, treat as a dynamic event binding
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = build_prefixed_value(
                            expr_text,
                            exp.start,
                            &exp.bindings,
                            &self.bindings,
                            false,
                        );
                        self.imports.add(VaporImportDependencies::SET_DYNAMIC_PROPS);
                        let effect = format!("_setDynamicProps(n{}, [{}])", node_ref, prefixed);
                        self.stack.last_mut().unwrap().effects.push(effect);
                    }
                }

                PropKind::Html => {
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = build_prefixed_value(
                            expr_text,
                            exp.start,
                            &exp.bindings,
                            &self.bindings,
                            false,
                        );
                        self.imports.add(VaporImportDependencies::SET_HTML);
                        let effect = format!("_setHtml(n{}, {})", node_ref, prefixed);
                        self.stack.last_mut().unwrap().effects.push(effect);
                    }
                }

                PropKind::Text => {
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = build_prefixed_value(
                            expr_text,
                            exp.start,
                            &exp.bindings,
                            &self.bindings,
                            false,
                        );
                        self.imports.add(VaporImportDependencies::TO_DISPLAY_STRING);
                        let display_expr = format!("_toDisplayString({})", prefixed);

                        let state = self.stack.last_mut().unwrap();
                        state.has_dynamic_children = true;
                        state.text_parts.push(VaporTextPart::Dynamic(display_expr));
                        if state.text_node_ref.is_none() {
                            let text_ref = self.text_node_counter;
                            self.text_node_counter += 1;
                            state.text_node_ref = Some(text_ref);
                        }
                    }
                }

                PropKind::Show => {
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = build_prefixed_value(
                            expr_text,
                            exp.start,
                            &exp.bindings,
                            &self.bindings,
                            false,
                        );
                        self.imports.add(VaporImportDependencies::APPLY_V_SHOW);
                        let stmt = format!("_applyVShow(n{}, () => ({}))", node_ref, prefixed);
                        self.stack.last_mut().unwrap().statements.push(stmt);
                    }
                }

                PropKind::Model => {
                    self.process_model(oxc_prop, ev, ctx, node_ref);
                }

                PropKind::Directive => {
                    self.process_directive(oxc_prop, ctx, node_ref);
                }

                _ => {
                    // Structural directives (If/ElseIf/Else/For/Slot/Once)
                    // not yet handled in vapor mode.
                }
            }
        }
    }

    fn process_event(
        &mut self,
        oxc_prop: &crate::syntax_kai::types::OxcProp<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
        node_ref: u32,
    ) {
        let prop = &oxc_prop.event;

        let is_dynamic = prop.has_dynamic_arg;

        let event_name = if let Some(ref arg) = prop.arg {
            ctx.input[arg.start as usize..arg.end as usize].to_string()
        } else {
            return;
        };

        let handler_expr = if let Some(ref exp) = oxc_prop.exp {
            let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
            let prefixed =
                build_prefixed_value(expr_text, exp.start, &exp.bindings, &self.bindings, false);

            let trimmed = prefixed.trim();
            if is_simple_identifier(trimmed) {
                format!("e => {}(e)", trimmed)
            } else if trimmed.contains("$event") {
                format!("$event => ({})", trimmed)
            } else {
                format!("() => ({})", trimmed)
            }
        } else {
            return;
        };

        self.imports.add(VaporImportDependencies::CREATE_INVOKER);

        let modifier_names: Vec<String> = if let Some(ref mods) = prop.modifiers {
            mods.iter()
                .map(|m| ctx.input[m.start as usize..m.end as usize].to_string())
                .collect()
        } else {
            Vec::new()
        };

        // Classify modifiers into three categories.
        let mut runtime_mods: Vec<&String> = Vec::new();
        let mut key_mods: Vec<&String> = Vec::new();
        let mut listener_opts: Vec<&String> = Vec::new();

        for m in &modifier_names {
            match m.as_str() {
                "capture" | "once" | "passive" => listener_opts.push(m),
                "enter" | "tab" | "delete" | "esc" | "space" | "up" | "down" | "left" | "right" => {
                    key_mods.push(m)
                }
                _ => runtime_mods.push(m),
            }
        }

        // Build handler: runtime modifiers first, then key modifiers.
        let mut wrapped = handler_expr;
        if !runtime_mods.is_empty() {
            self.imports.add(VaporImportDependencies::WITH_MODIFIERS);
            let mods_str = runtime_mods
                .iter()
                .map(|m| format!("\"{}\"", m))
                .collect::<Vec<_>>()
                .join(", ");
            wrapped = format!("_withModifiers({}, [{}])", wrapped, mods_str);
        }
        if !key_mods.is_empty() {
            self.imports.add(VaporImportDependencies::WITH_KEYS);
            let keys_str = key_mods
                .iter()
                .map(|m| format!("\"{}\"", m))
                .collect::<Vec<_>>()
                .join(", ");
            wrapped = format!("_withKeys({}, [{}])", wrapped, keys_str);
        }
        let invoker_expr = format!("_createInvoker({})", wrapped);

        if is_dynamic {
            // Dynamic event: @[eventName]="handler"
            // → _on(n{X}, expr, handler, { effect: true }) inside _renderEffect
            let arg_span = prop.arg.unwrap();
            let arg_raw = &ctx.input[arg_span.start as usize..arg_span.end as usize];
            let arg_prefixed = apply_dynamic_arg_prefix(
                arg_raw,
                arg_span.start,
                &oxc_prop.arg.as_ref().and_then(|a| a.bindings.clone()),
                &self.bindings,
                false,
            );
            self.imports.add(VaporImportDependencies::ON);
            let effect = format!(
                "_on(n{}, {}, {}, {{\n      effect: true\n    }})",
                node_ref, arg_prefixed, invoker_expr
            );
            self.stack.last_mut().unwrap().effects.push(effect);
        } else {
            let non_delegatable = Self::has_non_delegatable_modifier(&prop.modifiers, ctx);

            if !non_delegatable && Self::is_delegatable(&event_name) {
                if !self.delegated_events.contains(&event_name) {
                    self.delegated_events.push(event_name.clone());
                }
                let stmt = format!("n{}.$evt{} = {}", node_ref, event_name, invoker_expr);
                self.stack.last_mut().unwrap().statements.push(stmt);
            } else {
                self.imports.add(VaporImportDependencies::ON);
                let opts = if !listener_opts.is_empty() {
                    let opts_str = listener_opts
                        .iter()
                        .map(|o| format!("{}: true", o))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(", {{ {} }}", opts_str)
                } else {
                    String::new()
                };
                let stmt = format!(
                    "_on(n{}, \"{}\", {}{})",
                    node_ref, event_name, invoker_expr, opts
                );
                self.stack.last_mut().unwrap().statements.push(stmt);
            }
        }
    }

    fn process_model(
        &mut self,
        oxc_prop: &crate::syntax_kai::types::OxcProp<'alloc>,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
        node_ref: u32,
    ) {
        let Some(ref exp) = oxc_prop.exp else {
            return;
        };

        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
        let prefixed =
            build_prefixed_value(expr_text, exp.start, &exp.bindings, &self.bindings, false);

        let tag_name = &self.stack.last().unwrap().tag_name.clone();

        // Determine which apply*Model helper to use.
        let helper = match tag_name.as_str() {
            "select" => {
                self.imports
                    .add(VaporImportDependencies::APPLY_SELECT_MODEL);
                "_applySelectModel"
            }
            "textarea" => {
                self.imports.add(VaporImportDependencies::APPLY_TEXT_MODEL);
                "_applyTextModel"
            }
            "input" => {
                let input_type = Self::find_static_attr_value("type", ev, ctx);
                match input_type.as_deref() {
                    Some("checkbox") => {
                        self.imports
                            .add(VaporImportDependencies::APPLY_CHECKBOX_MODEL);
                        "_applyCheckboxModel"
                    }
                    Some("radio") => {
                        self.imports.add(VaporImportDependencies::APPLY_RADIO_MODEL);
                        "_applyRadioModel"
                    }
                    _ => {
                        self.imports.add(VaporImportDependencies::APPLY_TEXT_MODEL);
                        "_applyTextModel"
                    }
                }
            }
            _ => {
                self.imports.add(VaporImportDependencies::APPLY_TEXT_MODEL);
                "_applyTextModel"
            }
        };

        // Build modifiers object if any.
        let prop = &oxc_prop.event;
        let mods_str = if let Some(ref mods) = prop.modifiers {
            let entries: Vec<String> = mods
                .iter()
                .map(|m| {
                    let name = &ctx.input[m.start as usize..m.end as usize];
                    format!("{}: true", name)
                })
                .collect();
            if entries.is_empty() {
                String::new()
            } else {
                format!(", {{ {} }}", entries.join(","))
            }
        } else {
            String::new()
        };

        let stmt = format!(
            "{}(n{}, () => ({}), _value => ({} = _value){})",
            helper, node_ref, prefixed, prefixed, mods_str
        );
        self.stack.last_mut().unwrap().statements.push(stmt);
    }

    fn process_directive(
        &mut self,
        oxc_prop: &crate::syntax_kai::types::OxcProp<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
        node_ref: u32,
    ) {
        let prop = &oxc_prop.event;

        // Extract directive name: "v-my-directive" → "my-directive"
        let dir_raw_name = &ctx.input[prop.start as usize..prop.name_end as usize];
        let dir_name = dir_raw_name.strip_prefix("v-").unwrap_or(dir_raw_name);
        let dir_var = format!("_directive_{}", dir_name.replace('-', "_"));

        // Register for _resolveDirective declaration (deduped).
        if !self.resolved_directives.contains(&dir_name.to_string()) {
            self.resolved_directives.push(dir_name.to_string());
            self.imports.add(VaporImportDependencies::RESOLVE_DIRECTIVE);
            self.resolved_directive_decls.push(format!(
                "  const {} = _resolveDirective(\"{}\")",
                dir_var, dir_name
            ));
        }
        self.imports
            .add(VaporImportDependencies::WITH_VAPOR_DIRECTIVES);

        // Build value expression.
        let value = if let Some(ref exp) = oxc_prop.exp {
            let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
            let prefixed =
                build_prefixed_value(expr_text, exp.start, &exp.bindings, &self.bindings, false);
            format!("() => {}", prefixed)
        } else {
            String::new()
        };

        // Build arg.
        let arg = prop
            .arg
            .map(|arg_span| {
                let raw = &ctx.input[arg_span.start as usize..arg_span.end as usize];
                if prop.has_dynamic_arg {
                    apply_dynamic_arg_prefix(
                        raw,
                        arg_span.start,
                        &oxc_prop.arg.as_ref().and_then(|a| a.bindings.clone()),
                        &self.bindings,
                        false,
                    )
                } else {
                    format!("\"{}\"", raw)
                }
            })
            .unwrap_or_default();

        // Build modifiers object.
        let mods_str = if let Some(ref mods) = prop.modifiers {
            let entries: Vec<String> = mods
                .iter()
                .map(|m| {
                    let name = &ctx.input[m.start as usize..m.end as usize];
                    format!("{}: true", name)
                })
                .collect();
            if entries.is_empty() {
                String::new()
            } else {
                format!(", {{ {} }}", entries.join(", "))
            }
        } else {
            String::new()
        };

        // Build directive entry: [directive, value, arg, mods]
        let mut entry = dir_var;
        if !value.is_empty() || !arg.is_empty() || !mods_str.is_empty() {
            entry = format!(
                "{}, {}",
                entry,
                if value.is_empty() { "void 0" } else { &value }
            );
        }
        if !arg.is_empty() || !mods_str.is_empty() {
            entry = format!(
                "{}, {}",
                entry,
                if arg.is_empty() { "void 0" } else { &arg }
            );
        }
        if !mods_str.is_empty() {
            // mods_str already starts with ", { ... }"
            entry = format!("{}{}", entry, mods_str);
        }

        let stmt = format!("_withVaporDirectives(n{}, [[{}]])", node_ref, entry);
        self.stack.last_mut().unwrap().statements.push(stmt);
    }

    /// Find the value of a static attribute on the element.
    fn find_static_attr_value(
        attr_name: &str,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> Option<String> {
        for prop in &ev.event.props {
            if prop.kind == PropKind::Value {
                let name = &ctx.input[prop.start as usize..prop.name_end as usize];
                if name == attr_name {
                    if let Some(ref val) = prop.value {
                        return Some(ctx.input[val.start as usize..val.end as usize].to_string());
                    }
                }
            }
        }
        None
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Build a `_setText(xN, ...)` call from text parts.
fn build_set_text_call(text_ref: u32, parts: &[VaporTextPart]) -> String {
    let args = parts
        .iter()
        .map(|p| match p {
            VaporTextPart::Static(s) => format!("\"{}\"", escape_js_string(s)),
            VaporTextPart::Dynamic(expr) => expr.clone(),
        })
        .collect::<Vec<_>>()
        .join(" + ");
    format!("_setText(x{}, {})", text_ref, args)
}

/// Check if a string is a simple JavaScript identifier (possibly with dot-access).
fn is_simple_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' && first != '$' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.')
}

/// Parse an effect string like `_setProp(n0, "attr", expr)` or `_setClass(n0, expr)`
/// into a component prop entry like `attr: () => (expr)`.
fn parse_effect_as_component_prop(effect: &str) -> Option<String> {
    // _setProp(n{X}, "attr", expr)
    if let Some(rest) = effect.strip_prefix("_setProp(") {
        let rest = rest.strip_suffix(')')?.to_string();
        // Split: n{X}, "attr", expr
        let first_comma = rest.find(", ")?;
        let after_first = &rest[first_comma + 2..];
        // Find the attr name in quotes.
        if let Some(stripped) = after_first.strip_prefix('"') {
            let end_quote = stripped.find('"')?;
            let attr_name = &stripped[..end_quote];
            let expr = stripped[end_quote + 3..].to_string(); // skip `", `
            return Some(format!("{}: () => ({})", attr_name, expr));
        }
    }
    // _setClass(n{X}, expr)
    if let Some(rest) = effect.strip_prefix("_setClass(") {
        let rest = rest.strip_suffix(')')?.to_string();
        let first_comma = rest.find(", ")?;
        let expr = &rest[first_comma + 2..];
        return Some(format!("class: () => ({})", expr));
    }
    // _setStyle(n{X}, expr)
    if let Some(rest) = effect.strip_prefix("_setStyle(") {
        let rest = rest.strip_suffix(')')?.to_string();
        let first_comma = rest.find(", ")?;
        let expr = &rest[first_comma + 2..];
        return Some(format!("style: () => ({})", expr));
    }
    None
}
