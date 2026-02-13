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
//! - `n{X}` â€” node references for elements that have dynamic content (effects/statements)
//! - `p{X}` â€” path variables for intermediate navigation (stepping stones)
//! - `x{X}` â€” text node references via `_txt()`
//!
//! ## Template HTML Building
//!
//! Each root element accumulates a static HTML string in `current_html`.
//! Trailing close tags are stripped via the `pending_close_tags` mechanism.
//!
//! ## Module Structure
//!
//! The implementation is split into sub-modules mirroring the VDOM backend:
//! - [`element`] â€” element open/close handling, block body building
//! - [`props`] â€” prop classification and effect/statement generation
//! - [`directives`] â€” structural directive processing (v-if, v-for, v-slot)
//! - [`component`] â€” component setup, resolution, and call building
//! - [`helpers`] â€” pure utility functions (text building, identifier checks, var mappings)

mod component;
mod directives;
mod element;
mod helpers;
mod props;
pub(crate) mod types;

use std::{cell::RefCell, collections::HashSet, rc::Rc, sync::LazyLock};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        binding_types::BindingType,
        plugin::SyntaxPluginContext,
        types::{
            CompiledRootTemplateEnd, CompiledRootTemplateStart, OxcCompiledElementStart, PropKind,
        },
    },
};

use super::shared::helper::escape_js_string;
use crate::syntax_kai::plugins::code_gen::types::{
    TemplateCodeGenError, TemplateCodeGenResult, VaporImportDependencies,
};

use types::{
    VaporCounters, VaporElementState, VaporPendingContent, VaporResolutions, VaporTextPart,
    VaporVIfChainState,
};

/// Events that can be delegated (handled via event delegation at document level).
static DELEGATABLE_EVENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
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
    ])
});

pub(crate) struct VaporTemplateGenerator<'alloc> {
    code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
    bindings: FxHashMap<&'alloc str, BindingType>,
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

    /// Node/text/path/for/slot counters for variable naming.
    counters: VaporCounters,

    /// Position of `<template>` open tag start â€” hoisted constants emitted here.
    template_start_pos: u32,

    /// Root node reference indices for the return statement.
    root_nodes: Vec<u32>,

    /// Delegated event names (unique, for `_delegateEvents(...)` call).
    delegated_events: Vec<String>,
    /// Hash set for O(1) delegated event dedup lookups.
    delegated_events_set: FxHashSet<String>,

    /// Nested content collected during tree walk, emitted at root close.
    pending: VaporPendingContent,

    /// Whether any element uses `ref` â€” triggers `_createTemplateRefSetter()` once.
    has_template_ref: bool,
    inside_template: bool,

    /// Resolved component and directive declarations.
    resolutions: VaporResolutions,

    /// Active v-if chain states. When a v-if element opens, a chain is started.
    /// When a v-else-if/v-else follows, it extends the chain. When a non-continuation
    /// sibling appears (or parent closes), the chain is flushed.
    pending_vif_chains: Vec<VaporVIfChainState>,
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
            counters: VaporCounters::new(),
            template_start_pos: 0,
            root_nodes: Vec::new(),
            delegated_events: Vec::new(),
            delegated_events_set: FxHashSet::default(),
            pending: VaporPendingContent::new(),
            has_template_ref: false,
            inside_template: false,
            resolutions: VaporResolutions::new(),
            pending_vif_chains: Vec::new(),
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

    // â”€â”€ Counter helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Allocate a new node reference index.
    pub(super) fn next_node_ref(&mut self) -> u32 {
        self.counters.next_node_ref()
    }

    /// Allocate a new text node reference index.
    pub(super) fn next_text_node_ref(&mut self) -> u32 {
        self.counters.next_text_node_ref()
    }

    /// Allocate a new path variable index.
    pub(super) fn next_path_ref(&mut self) -> u32 {
        self.counters.next_path_ref()
    }

    /// Build a prefixed expression string with both accessor prefixes and v-for/v-slot
    /// variable mappings applied, using OXC-parsed binding positions for precision.
    pub(super) fn prefix_expr(
        &self,
        val_text: &str,
        val_start: u32,
        bindings_result: Option<&crate::utils::oxc::BindingExtractionResult>,
    ) -> String {
        let var_mappings = self
            .stack
            .last()
            .map(|s| s.for_var_mappings.as_slice())
            .unwrap_or(&[]);
        super::shared::helper::build_prefixed_value_with_var_mappings(
            val_text,
            val_start,
            bindings_result,
            &self.bindings,
            self.is_production,
            var_mappings,
        )
    }

    // â”€â”€ HTML buffer helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Flush pending close tags into the HTML buffer.
    fn flush_pending_close_tags(&mut self) {
        for tag in self.pending_close_tags.drain(..) {
            self.current_html.push_str(&tag);
        }
    }

    /// Append content to the HTML buffer, flushing pending close tags first.
    pub(super) fn append_html(&mut self, content: &str) {
        self.flush_pending_close_tags();
        self.current_html.push_str(content);
    }

    /// Push a close tag to the pending list (will be flushed or stripped).
    pub(super) fn push_close_tag(&mut self, tag_name: &str) {
        self.pending_close_tags.push(format!("</{}>", tag_name));
    }

    /// Finalize the HTML buffer: discard pending close tags (they're trailing).
    pub(super) fn finalize_html(&mut self) -> String {
        self.pending_close_tags.clear();
        std::mem::take(&mut self.current_html)
    }

    /// Flush static text parts from a (popped) element state into the HTML buffer,
    /// then push the element's close tag. Used by both `complete_element_close` and
    /// `build_block_body` for native elements.
    pub(super) fn flush_element_text_to_html(&mut self, state: &mut VaporElementState) {
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
    }

    /// Finalize the current HTML buffer and register it as a hoisted template.
    /// Returns the template index (for `t{idx}()` references).
    pub(super) fn register_template(&mut self) -> u32 {
        let html = self.finalize_html();
        let template_idx = self.templates.len() as u32;
        self.templates.push(html);
        self.imports.add(VaporImportDependencies::TEMPLATE);
        template_idx
    }

    /// Drain pending navigation instructions and text node creations into a buffer.
    pub(super) fn drain_pending_instructions(&mut self, buf: &mut String) {
        self.pending.drain_instructions(buf);
    }

    /// Emit effects wrapped in `_renderEffect()`, or as direct statements for v-once.
    /// Appends the generated code to `buf` using the specified `indent`.
    pub(super) fn emit_render_effect(
        &mut self,
        rendered: &[String],
        is_once: bool,
        indent: &str,
        buf: &mut String,
    ) {
        if rendered.is_empty() {
            return;
        }
        if is_once {
            for effect in rendered {
                buf.push_str(&format!("{}{}\n", indent, effect));
            }
        } else {
            self.imports.add(VaporImportDependencies::RENDER_EFFECT);
            if rendered.len() == 1 {
                buf.push_str(&format!("{}_renderEffect(() => {})\n", indent, rendered[0]));
            } else {
                buf.push_str(&format!("{}_renderEffect(() => {{\n", indent));
                for effect in rendered {
                    buf.push_str(&format!("{}  {}\n", indent, effect));
                }
                buf.push_str(&format!("{}}})\n", indent));
            }
        }
    }

    /// Flush text_parts from the top-of-stack element into the HTML buffer.
    /// Only called for static parents (no dynamic children).
    pub(super) fn flush_text_parts_to_html(&mut self) {
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

    // â”€â”€ Build static HTML open tag â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Build the static HTML open tag string from tag name + static props.
    pub(super) fn build_static_open_tag(
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
                    // Skip `ref` â€” handled at runtime via _setTemplateRef.
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

    // â”€â”€ Navigation helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Ensure all ancestors on the stack from root to the current top have `var_name` set.
    /// Assigns path variables and generates navigation instructions for any that don't.
    pub(super) fn ensure_ancestor_var_names(&mut self) {
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
            let parent_var = if let Some(v) = self.stack[i - 1].var_name.as_ref() {
                v.clone()
            } else {
                continue;
            };
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
                // No previous nav ref â€” create synthetic _child for first child.
                self.imports.add(VaporImportDependencies::CHILD);
                self.imports.add(VaporImportDependencies::NEXT);
                let child0_path = self.next_path_ref();
                let child0_var = format!("p{}", child0_path);
                self.pending
                    .nav
                    .push(format!("  const {} = _child({})", child0_var, parent_var));
                format!(
                    "  const {} = _next({}, {})",
                    var_name, child0_var, child_index
                )
            };

            self.stack[i - 1].last_nav_child_var = Some(var_name.clone());
            self.stack[i].var_name = Some(var_name);
            self.stack[i].needs_node_ref = true;
            self.pending.nav.push(nav);
        }
    }

    /// Build a navigation instruction for a non-root element.
    /// Returns the navigation code line. Also updates parent's `last_nav_child_var`.
    pub(super) fn build_nav_instruction(
        &mut self,
        var_name: &str,
        child_index: u32,
    ) -> TemplateCodeGenResult<String> {
        let top = self
            .stack
            .last()
            .ok_or(TemplateCodeGenError::StackUnderflow(
                "build_nav_instruction: stack must not be empty",
            ))?;
        let parent_var = top
            .var_name
            .as_ref()
            .ok_or(TemplateCodeGenError::StackUnderflow(
                "build_nav_instruction: parent must have var_name",
            ))?
            .clone();
        let prev_nav = top.last_nav_child_var.clone();

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
            // No previous nav ref â€” create synthetic _child for first child.
            self.imports.add(VaporImportDependencies::CHILD);
            self.imports.add(VaporImportDependencies::NEXT);
            let child0_path = self.next_path_ref();
            let child0_var = format!("p{}", child0_path);
            self.pending
                .nav
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

        Ok(nav)
    }

    // â”€â”€ Event handling helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    pub(super) fn is_delegatable(event_name: &str) -> bool {
        DELEGATABLE_EVENTS.contains(event_name)
    }

    pub(super) fn has_non_delegatable_modifier(
        modifiers: &Option<Vec<crate::common::Span>>,
        ctx: &SyntaxPluginContext,
    ) -> bool {
        if let Some(mods) = modifiers {
            for m in mods {
                let name = &ctx.input[m.start as usize..m.end as usize];
                if super::shared::helper::classify_modifier(name)
                    == super::shared::helper::ModifierKind::ListenerOption
                {
                    return true;
                }
            }
        }
        false
    }

    // â”€â”€ Template-level handlers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    pub(crate) fn handle_template_start(
        &mut self,
        ev: &CompiledRootTemplateStart,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
        self.template_start_pos = ev.tag_open.start;
        self.inside_template = true;
        Ok(())
    }

    pub(crate) fn handle_template_closed(
        &mut self,
        ev: &CompiledRootTemplateEnd,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
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
        for decl in &self.resolutions.component_decls {
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
        for decl in &self.resolutions.directive_decls {
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

        Ok(())
    }
}
