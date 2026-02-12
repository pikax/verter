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
            Comment, CompiledRootTemplateEnd, CompiledRootTemplateStart, OxcCompiledElementClosed,
            OxcCompiledElementStart, OxcInterpolation, PropKind, Text,
        },
    },
};

use super::shared::helper::{build_prefixed_value, escape_js_string};
use crate::syntax_kai::plugins::code_gen::types::VaporImportDependencies;

use types::{VaporElementState, VaporTextPart};

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

    inside_template: bool,
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
            inside_template: false,
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
        // Single root: `_template("...", true)`. Multi-root: no second arg.
        let is_single_root = self.root_nodes.len() == 1;
        let mut hoist_str = String::new();
        for (i, html) in self.templates.iter().enumerate() {
            let escaped = escape_js_string(html);
            let true_arg = if is_single_root { ", true" } else { "" };
            hoist_str.push_str(&format!(
                "const t{} = _template(\"{}\"{})\n",
                i, escaped, true_arg
            ));
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

        // Emit: imports + hoisted templates + delegateEvents before render function.
        let preamble = format!("{}{}{}", import_str, hoist_str, delegate_str);
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

        code_transform.overwrite(
            self.template_start_pos,
            open_tag_end,
            "\nexport function render(_ctx) {\n",
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

        let is_root = self.stack.is_empty();

        // If nested, handle parent state before this element.
        if !is_root {
            // Reset parent's text_child_started (element breaks text sequence).
            if let Some(parent) = self.stack.last_mut() {
                parent.text_child_started = false;
            }

            // Flush parent's pending text_parts to HTML.
            let parent_is_static = self
                .stack
                .last()
                .map(|s| !s.has_dynamic_children)
                .unwrap_or(false);
            if parent_is_static {
                self.flush_text_parts_to_html();
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

        if is_root {
            // Root elements get var_name immediately.
            state.var_name = Some(format!("n{}", node_ref));
            // Start fresh HTML buffer.
            self.current_html.clear();
            self.pending_close_tags.clear();
        } else {
            // Non-root: set child_index and increment parent's child_count.
            if let Some(parent) = self.stack.last_mut() {
                state.child_index = parent.child_count;
                parent.child_count += 1;
            }
        }

        let html_tag = self.build_static_open_tag(&tag_name, ev, ctx);
        self.append_html(&html_tag);

        // Push state onto stack BEFORE processing props.
        self.stack.push(state);

        // Process props: classify static vs dynamic.
        self.process_props(ev, ctx);

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
        let mut state = self
            .stack
            .pop()
            .expect("Element close without matching open");

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

        if state.is_root {
            self.complete_root_element_close(&mut state, close_tag);
        } else {
            self.complete_non_root_element_close(&mut state, close_tag);
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
                PropKind::Value | PropKind::ClassValue | PropKind::StyleValue => {
                    // Static props are already in the HTML.
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

                PropKind::On => {
                    self.process_event(oxc_prop, ctx, node_ref);
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

                _ => {
                    // Other directives not handled yet.
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

        let behavioral_mods: Vec<&String> = modifier_names
            .iter()
            .filter(|m| !matches!(m.as_str(), "capture" | "once" | "passive"))
            .collect();
        let listener_opts: Vec<&String> = modifier_names
            .iter()
            .filter(|m| matches!(m.as_str(), "capture" | "once" | "passive"))
            .collect();

        let invoker_expr = if !behavioral_mods.is_empty() {
            self.imports.add(VaporImportDependencies::WITH_MODIFIERS);
            let mods_str = behavioral_mods
                .iter()
                .map(|m| format!("\"{}\"", m))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "_createInvoker(_withModifiers({}, [{}]))",
                handler_expr, mods_str
            )
        } else {
            format!("_createInvoker({})", handler_expr)
        };

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
