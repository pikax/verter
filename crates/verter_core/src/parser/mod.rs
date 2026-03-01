//! Tokenizer event dispatcher and SFC parser.
//!
//! [`Syntax`] consumes a stream of [`TokenizerEvent`]s and produces:
//! - **Root nodes** — [`RootNodeScript`], [`RootNodeStyle`], [`RootNodeTemplate`],
//!   [`RootNodeUnknown`] for top-level SFC blocks.
//! - **Template AST** — A [`TemplateAst`] built incrementally via
//!   [`TemplateAstBuilder`] for the `<template>` block's children.
//!
//! In **SFC mode** (default), the first nesting level (`stack depth == 1`)
//! represents root blocks. Their attributes are routed to special fields
//! (`prop_lang`, `prop_setup`, etc.) rather than the AST builder.
//!
//! In **template mode** (`template_mode = true`), there are no root blocks —
//! all elements go directly into the AST builder.

use crate::ast::builder::TemplateAstBuilder;
use crate::ast::types::{
    AstNodeKind, ElementNodeCondition, ElementNodeConditionKind, PropFlags, TagType, TemplateAst,
};
use crate::common::Span;
use crate::cursor::ScriptLanguage;
use crate::diagnostics::{CompilerErrorCode, Diagnostic, SyntaxPluginContext};
use crate::parser::types::{
    RootNodeKind, RootNodeScript, RootNodeStyle, RootNodeTemplate, RootNodeTemplateContent,
    RootNodeUnknown, StyleLang,
};
use crate::tokenizer::{Event as TokenizerEvent, QuoteType};
use crate::types::{NodeId, NodeProp, NodeTag};
use crate::utils::vue::{is_html_tag, is_mathml_tag, is_svg_tag, is_void_tag};
use smallvec::SmallVec;

#[cfg(test)]
mod mod_tests;

pub mod types;

/// Minimal bookkeeping for an open element on the nesting stack.
///
/// 16 bytes (`Copy`) — stores just enough to validate close tags and
/// create `NodeTag` spans. Pushed on `OpenTagName`, popped on
/// `CloseTag` / `SelfClosingTag` / EOF.
#[derive(Debug, Clone, Copy)]
struct StackElement {
    /// Byte offset of `<` in the open tag.
    tag_open_start: u32,
    /// Byte offset past `>` — initially set to `tag_open_start`, updated
    /// on `OpenTagEnd` / `SelfClosingTag`.
    tag_open_end: u32,
    /// Byte offset of the first character of the tag name (= `tag_open_start + 1`).
    name_start: u32,
    /// Byte offset past the last character of the tag name.
    name_end: u32,
}

impl StackElement {
    /// Extract the tag name bytes from the source buffer.
    fn name_bytes<'a>(&self, ctx: &SyntaxPluginContext<'a>) -> &'a [u8] {
        &ctx.bytes[self.name_start as usize..self.name_end as usize]
    }
}

/// Event-driven SFC and template parser.
///
/// Consumes [`TokenizerEvent`]s via [`handle()`](Syntax::handle) and
/// accumulates parsed root nodes and a template AST. Acts as a state
/// machine with three categories of state:
///
/// 1. **Element nesting** — `stack_elements` tracks open tags for close-tag
///    validation and SFC root detection.
/// 2. **Attribute accumulation** — `current_prop` + `element_props` build up
///    attributes/directives for the current element.
/// 3. **Root node storage** — Parsed `<script>`, `<style>`, `<template>`, and
///    custom blocks are stored in dedicated fields.
pub struct Syntax {
    // ---- configuration ----
    /// When `true`, all elements go directly to the AST builder (no SFC root
    /// detection). When `false` (default), depth-1 elements are treated as
    /// SFC root blocks (`<script>`, `<style>`, `<template>`, custom).
    template_mode: bool,

    // ---- SFC-level flags (accumulated across all root nodes) ----
    /// Any `<style scoped>` block was seen.
    has_style_scope: bool,
    /// Any `<style module>` block was seen.
    has_style_module: bool,
    /// `<template vapor>` was seen.
    is_vapor: bool,

    // ---- attribute accumulation for the current element ----
    /// The attribute/directive currently being assembled from tokenizer events.
    /// Initialized on `AttribName`/`DirName`, consumed on `AttribEnd`.
    current_prop: Option<NodeProp>,
    /// Collected attributes for the current SFC root tag. Only used when
    /// processing root-level elements in SFC mode; template content attributes
    /// are routed directly to the `ast_builder`.
    element_props: Vec<NodeProp>,

    /// Root-level `lang="..."` attribute value span (applies to all root kinds).
    prop_lang: Option<Span>,
    /// Root-level `src="..."` attribute value span (script only).
    prop_src: Option<Span>,
    /// Root-level `generic="..."` attribute value span (script only).
    prop_generic: Option<Span>,
    /// Root-level `setup` attribute was present (script only).
    prop_setup: bool,
    /// Root-level `scoped` attribute was present (style only).
    prop_scoped: bool,
    /// Root-level `module` attribute was present (style only).
    prop_module: bool,

    // ---- element nesting stack ----
    /// Stack of currently-open elements. Used for close-tag name validation,
    /// SFC root detection (`len() == 1`), and error spans for unclosed tags.
    stack_elements: Vec<StackElement>,

    // ---- parsed root nodes ----
    /// The `<script>` block (without `setup`), if any.
    script_node: Option<RootNodeScript>,
    /// The `<script setup>` block, if any.
    script_setup_node: Option<RootNodeScript>,
    /// All `<style>` blocks (multiple allowed).
    style_nodes: Vec<RootNodeStyle>,
    /// Custom/unknown blocks (e.g., `<i18n>`, `<docs>`).
    unknown_nodes: Vec<RootNodeUnknown>,

    // ---- template AST ----
    /// Active builder for template content. `Some` while inside a `<template>`
    /// root (SFC mode) or always in template mode. `None` before `<template>`
    /// is opened or after it's finalized.
    ast_builder: Option<TemplateAstBuilder>,
    /// Finalized template AST. Set when the template root closes or on EOF.
    template_ast: Option<TemplateAst>,

    // ---- diagnostics ----
    /// Accumulated parse diagnostics (errors / warnings). Stored here because
    /// `SyntaxPluginContext` is `&`-immutable during event dispatch.
    diagnostics: Vec<Diagnostic>,
}

impl Syntax {
    pub fn new(template_mode: bool) -> Self {
        // In template_mode, start with an active AST builder immediately —
        // there are no root nodes, everything is template content.
        let ast_builder = if template_mode {
            let synthetic_root = RootNodeTemplate {
                tag_open: NodeTag {
                    start: 0,
                    end: 0,
                    name_end: 0,
                },
                tag_close: None,
                lang: None,
                attributes: Vec::new(),
                content: Some(RootNodeTemplateContent {
                    start: 0,
                    end: 0,
                    children: SmallVec::new(),
                }),
            };
            Some(TemplateAstBuilder::new(synthetic_root))
        } else {
            None
        };

        Self {
            template_mode,
            script_node: None,
            script_setup_node: None,
            style_nodes: Vec::new(),
            unknown_nodes: Vec::new(),

            element_props: Vec::with_capacity(20),

            has_style_scope: false,
            has_style_module: false,

            is_vapor: false,

            current_prop: None,

            prop_lang: None,
            prop_src: None,
            prop_generic: None,
            prop_setup: false,
            prop_scoped: false,
            prop_module: false,

            stack_elements: Vec::with_capacity(16),

            ast_builder,
            template_ast: None,

            diagnostics: Vec::new(),
        }
    }

    /// Take all accumulated diagnostics.
    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    /// Whether any error-level diagnostics have been emitted.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == crate::diagnostics::DiagnosticSeverity::Error)
    }

    // ---- public accessors for parsed results ----

    pub fn script(&self) -> Option<&RootNodeScript> {
        self.script_node.as_ref()
    }

    pub fn script_setup(&self) -> Option<&RootNodeScript> {
        self.script_setup_node.as_ref()
    }

    pub fn style_nodes(&self) -> &[RootNodeStyle] {
        &self.style_nodes
    }

    pub fn unknown_nodes(&self) -> &[RootNodeUnknown] {
        &self.unknown_nodes
    }

    pub fn template_ast(&self) -> Option<&TemplateAst> {
        self.template_ast.as_ref()
    }

    pub fn take_template_ast(&mut self) -> Option<TemplateAst> {
        self.template_ast.take()
    }

    pub fn has_style_scope(&self) -> bool {
        self.has_style_scope
    }

    pub fn has_style_module(&self) -> bool {
        self.has_style_module
    }

    pub fn is_vapor(&self) -> bool {
        self.is_vapor
    }
}

// helpers

#[inline]
fn make_open_tag(se: &StackElement) -> NodeTag {
    NodeTag {
        start: se.tag_open_start,
        end: se.tag_open_end,
        name_end: se.name_end,
    }
}

#[inline]
fn make_close_tag(start: u32, end: u32, name_end: u32) -> NodeTag {
    NodeTag {
        start,
        end,
        name_end,
    }
}

#[inline]
fn resolve_root_kind(name: &[u8]) -> RootNodeKind {
    match name {
        b"template" => RootNodeKind::Template,
        b"script" => RootNodeKind::Script,
        b"style" => RootNodeKind::Style,
        _ => RootNodeKind::Unknown,
    }
}

/// Classify a tag name into a `TagType` from bytes.
///
/// - `slot` → SlotOutlet
/// - `template` → Template
/// - uppercase first byte or contains `-` → Component
/// - known HTML/SVG/MathML tag → Element
/// - unknown lowercase → Component
#[inline]
fn resolve_tag_type(name: &[u8]) -> TagType {
    if name == b"slot" {
        return TagType::SlotOutlet;
    }
    if name == b"template" {
        return TagType::Template;
    }
    // Uppercase first byte → Component (PascalCase)
    if let Some(&first) = name.first() {
        if first.is_ascii_uppercase() {
            return TagType::Component;
        }
    }
    // Contains dash → Component (kebab-case custom element)
    if name.contains(&b'-') {
        return TagType::Component;
    }
    // Known HTML/SVG/MathML tag → Element
    if is_html_tag(name) || is_svg_tag(name) || is_mathml_tag(name) {
        return TagType::Element;
    }
    // Unknown lowercase tag → Component
    TagType::Component
}

// event dispatch

impl<'alloc> Syntax {
    /// Dispatch a single tokenizer event, advancing the parser state.
    ///
    /// This is the main entry point — call once per event in order. Events
    /// drive tag lifecycle (open → attributes → close), leaf attachment,
    /// and end-of-stream finalization.
    pub fn handle(&mut self, event: &TokenizerEvent<'alloc>, ctx: &SyntaxPluginContext<'alloc>) {
        match event {
            // Tags
            TokenizerEvent::OpenTagName { start, end } => {
                self.handle_tag_open(*start, *end, ctx);
            }
            TokenizerEvent::OpenTagEnd { end } => {
                self.handle_open_tag_end(*end, ctx);
            }
            TokenizerEvent::SelfClosingTag { end } => {
                self.handle_self_closing(*end, ctx);
            }
            TokenizerEvent::CloseTag {
                start,
                end,
                name_end,
            } => {
                self.handle_close_tag(*start, *end, *name_end, ctx);
            }

            // Attributes
            TokenizerEvent::AttribName { start, end } => {
                self.handle_attribute_name(*start, *end);
            }
            TokenizerEvent::DirName { start, end } => {
                self.handle_directive_name(*start, *end);
            }
            TokenizerEvent::DirArg {
                is_dynamic,
                start,
                end,
            } => {
                self.handle_directive_arg(*start, *end, *is_dynamic);
            }
            TokenizerEvent::DirModifier { start, end } => {
                self.handle_directive_modifier(*start, *end);
            }
            TokenizerEvent::AttribData { start, end: _ } => {
                self.handle_attribute_value(*start);
            }
            TokenizerEvent::AttribEnd { quote, end } => {
                self.handle_attribute_end(*end, *quote, ctx);
            }

            // Leafs
            TokenizerEvent::Text { start, end } => {
                self.handle_text_leaf(*start, *end, false);
            }
            TokenizerEvent::TextEntity { start, end } => {
                self.handle_text_leaf(*start, *end, true);
            }
            TokenizerEvent::Comment {
                start,
                end,
                content_start,
                content_end,
            } => {
                self.handle_comment_leaf(*start, *end, *content_start, *content_end);
            }
            TokenizerEvent::Interpolation {
                start,
                end,
                delimiter_open_len,
                delimiter_close_len,
            } => {
                let inner_start = *start + *delimiter_open_len as u32;
                let inner_end = *end - *delimiter_close_len as u32;
                self.handle_interpolation_leaf(*start, *end, inner_start, inner_end);
            }

            // End-of-stream: emit diagnostics for unclosed elements, then finalize.
            TokenizerEvent::End => {
                self.handle_end(ctx);
            }

            _ => {}
        }
    }
}

// ---- tag lifecycle ----
//
// Open → attributes → OpenTagEnd/SelfClosingTag → (children) → CloseTag
//
// At each stage, `stack_elements` tracks nesting depth. In SFC mode,
// depth 1 means we're processing a root block and events are routed
// to root-node storage rather than the template AST builder.

impl Syntax {
    /// `OpenTagName` — push a new element onto the nesting stack.
    ///
    /// In SFC mode, depth-1 elements are SFC root blocks. In template
    /// mode (or inside a `<template>` root), the element is opened in
    /// the AST builder.
    fn handle_tag_open<'alloc>(
        &mut self,
        start: u32,
        name_end: u32,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let se = StackElement {
            tag_open_start: start,
            tag_open_end: start, // filled on OpenTagEnd / SelfClosingTag
            name_start: start + 1,
            name_end,
        };

        self.stack_elements.push(se);

        // If inside <template> (or template_mode), open an element node in the builder.
        // In SFC mode, skip the root element itself (stack len == 1) since it's handled
        // by root node detection. In template_mode, ALL elements go through the builder.
        if let Some(builder) = self.ast_builder.as_mut() {
            let is_sfc_root = !self.template_mode && self.stack_elements.len() == 1;
            if !is_sfc_root {
                let open_tag = make_open_tag(
                    self.stack_elements
                        .last()
                        .expect("invariant: stack non-empty after push"),
                );
                builder.open_element(open_tag);

                // Classify tag type from bytes
                let tag_name = &ctx.bytes[(start + 1) as usize..name_end as usize];
                let tag_type = resolve_tag_type(tag_name);
                builder.set_tag_type(tag_type);
            }
        }
    }

    /// `OpenTagEnd` — the `>` of an open tag was reached.
    ///
    /// Finalizes the open tag span and, for SFC roots, triggers root-node
    /// initialization (e.g., creating the `TemplateAstBuilder` for `<template>`).
    /// For template content elements, marks the content start in the builder.
    fn handle_open_tag_end<'alloc>(&mut self, end: u32, ctx: &SyntaxPluginContext<'alloc>) {
        if let Some(last) = self.stack_elements.last_mut() {
            last.tag_open_end = end;
        } else {
            // OpenTagEnd with empty stack — should not happen in well-formed input.
            self.diagnostics.push(
                Diagnostic::error("syntax", CompilerErrorCode::EofInTag)
                    .with_span(Span::new(end.saturating_sub(1), end)),
            );
            return;
        }

        let is_sfc_root = !self.template_mode && self.stack_elements.len() == 1;

        if is_sfc_root {
            // StackElement is Copy (4 × u32 = 16 bytes) — cheap copy avoids borrow conflict
            // with &mut self in handle_root_open.
            let root_event = *self
                .stack_elements
                .last()
                .expect("invariant: stack non-empty when is_sfc_root");
            let name = root_event.name_bytes(ctx);
            self.handle_root_open(&root_event, name, ctx);
        } else {
            // Check if this is a void HTML element (e.g., <img>, <br>, <input>).
            // Void elements cannot have children or closing tags — auto-close them
            // immediately, the same way self-closing tags are handled.
            let se = *self
                .stack_elements
                .last()
                .expect("invariant: stack non-empty in non-root branch");
            let tag_name = &ctx.bytes[se.name_start as usize..se.name_end as usize];
            let is_void = is_void_tag(tag_name);

            if let Some(builder) = self.ast_builder.as_mut() {
                builder.set_tag_open_end(end);
                if is_void {
                    builder.set_self_closing();
                    let closed_id = builder.close_element(None, end);
                    self.validate_v_condition_adjacency(closed_id, ctx);
                } else {
                    builder.mark_element_content_start(end);
                }
            }

            if is_void {
                self.stack_elements.pop();
            }
        }
    }

    /// `SelfClosingTag` — the `/>` of a self-closing tag was reached.
    ///
    /// Pops the element from the nesting stack and either stores it as an
    /// SFC root node or closes it in the AST builder. For root nodes, this
    /// means the block has no content (e.g., `<script src="..." />`).
    fn handle_self_closing<'alloc>(&mut self, end: u32, ctx: &SyntaxPluginContext<'alloc>) {
        if let Some(last) = self.stack_elements.last_mut() {
            last.tag_open_end = end;
        }

        let se = match self.stack_elements.pop() {
            Some(se) => se,
            None => {
                // Self-closing tag with empty stack — should not happen.
                self.diagnostics.push(
                    Diagnostic::error("syntax", CompilerErrorCode::EofInTag)
                        .with_span(Span::new(end.saturating_sub(2), end)),
                );
                return;
            }
        };

        let was_sfc_root = !self.template_mode && self.stack_elements.is_empty();
        let name = se.name_bytes(ctx);

        if was_sfc_root {
            match resolve_root_kind(name) {
                RootNodeKind::Template => {
                    if let Some(mut builder) = self.ast_builder.take() {
                        builder.ast.root.tag_close = None;
                        // Self-closing => no content
                        builder.ast.root.content = None;
                        self.template_ast = Some(builder.finish());
                    } else {
                        // Self-closing <template /> with no builder — create minimal
                        let root = RootNodeTemplate {
                            tag_open: make_open_tag(&se),
                            tag_close: None,
                            lang: self.prop_lang.take(),
                            attributes: self.take_props(),
                            content: None,
                        };
                        self.template_ast = Some(TemplateAst::new(root));
                    }
                    self.reset_prop_state();
                }
                RootNodeKind::Script => {
                    self.store_script_node(&se, None, None, ctx);
                }
                RootNodeKind::Style => {
                    self.store_style_node(&se, None, None, ctx);
                }
                RootNodeKind::Unknown => {
                    self.store_unknown_node(&se, None, None);
                }
            }
        } else if let Some(builder) = self.ast_builder.as_mut() {
            builder.set_tag_open_end(end);
            builder.set_self_closing();
            let closed_id = builder.close_element(None, end);
            self.validate_v_condition_adjacency(closed_id, ctx);
        }
    }

    /// `CloseTag` — a `</name>` close tag was encountered.
    ///
    /// Validates that the close tag name matches the top of `stack_elements`
    /// (case-insensitive). On match, pops the element and stores it as an
    /// SFC root node or closes it in the AST builder. On mismatch, emits
    /// `XInvalidEndTag` and rejects the close tag.
    fn handle_close_tag<'alloc>(
        &mut self,
        start: u32,
        end: u32,
        name_end: u32,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        // Extract the close-tag name from source bytes.
        // Close tag format: </name> — name starts at start+2, ends at name_end.
        let close_name = &ctx.bytes[(start + 2) as usize..name_end as usize];

        // Silently ignore closing tags for void HTML elements (e.g., </img>, </br>).
        // Void elements are auto-closed on their open tag end, so any explicit
        // closing tag is redundant. Real-world HTML tolerates this.
        if is_void_tag(close_name) {
            return;
        }

        // --- Strict validation: close-tag must match the top of the open stack ---
        let open = match self.stack_elements.last() {
            Some(se) => se,
            None => {
                // Orphan close tag — nothing on the stack to match.
                self.diagnostics.push(
                    Diagnostic::error("syntax", CompilerErrorCode::XInvalidEndTag)
                        .with_span(Span::new(start, end)),
                );
                return;
            }
        };

        let open_name = open.name_bytes(ctx);

        if !open_name.eq_ignore_ascii_case(close_name) {
            // Mismatch: close tag doesn't match the current open element.
            // Strict mode: emit diagnostic and reject the close tag entirely.
            self.diagnostics.push(
                Diagnostic::error("syntax", CompilerErrorCode::XInvalidEndTag)
                    .with_span(Span::new(start, end)),
            );
            return;
        }

        // Names match — safe to pop.
        debug_assert!(
            !self.stack_elements.is_empty(),
            "stack_elements should not be empty after name match"
        );
        let open = self
            .stack_elements
            .pop()
            .expect("invariant: stack non-empty after name match");

        let was_sfc_root = !self.template_mode && self.stack_elements.is_empty();
        let tag_close = make_close_tag(start, end, name_end);

        if was_sfc_root {
            let content = Some(Span::new(open.tag_open_end, start));

            match resolve_root_kind(open_name) {
                RootNodeKind::Template => {
                    if let Some(mut builder) = self.ast_builder.take() {
                        builder.ast.root.tag_close = Some(tag_close);
                        if let Some(c) = builder.ast.root.content.as_mut() {
                            c.end = start;
                        }
                        self.template_ast = Some(builder.finish());
                    }
                }
                RootNodeKind::Script => {
                    self.store_script_node(&open, Some(tag_close), content, ctx);
                }
                RootNodeKind::Style => {
                    self.store_style_node(&open, Some(tag_close), content, ctx);
                }
                RootNodeKind::Unknown => {
                    self.store_unknown_node(&open, Some(tag_close), content);
                }
            }
        } else if let Some(builder) = self.ast_builder.as_mut() {
            let closed_id = builder.close_element(Some(tag_close), start);
            self.validate_v_condition_adjacency(closed_id, ctx);
        }
    }

    /// `End` — end-of-stream reached.
    ///
    /// Force-closes all unclosed elements (innermost first), emitting
    /// `XMissingEndTag` for each. Then finalizes the template AST. In
    /// template mode, updates the synthetic root's content end to the
    /// actual source length.
    fn handle_end<'alloc>(&mut self, ctx: &SyntaxPluginContext<'alloc>) {
        // Emit XMissingEndTag for every unclosed element still on the stack.
        // Drain in reverse (innermost first) so the builder can close them properly.
        while let Some(se) = self.stack_elements.pop() {
            let is_sfc_root = !self.template_mode && self.stack_elements.is_empty();

            self.diagnostics.push(
                Diagnostic::error("syntax", CompilerErrorCode::XMissingEndTag)
                    .with_span(Span::new(se.tag_open_start, se.tag_open_end)),
            );

            // If this was a template element inside the builder, force-close it
            // so the node gets attached and isn't orphaned.
            if !is_sfc_root {
                if let Some(builder) = self.ast_builder.as_mut() {
                    // Use the open tag end as a synthetic content_end.
                    let closed_id = builder.close_element(None, se.tag_open_end);
                    self.validate_v_condition_adjacency(closed_id, ctx);
                }
            } else {
                // Unclosed SFC root — store what we can.
                let name = &ctx.bytes[se.name_start as usize..se.name_end as usize];
                let content = Some(Span::new(se.tag_open_end, ctx.bytes.len() as u32));
                match resolve_root_kind(name) {
                    RootNodeKind::Template => {
                        if let Some(mut builder) = self.ast_builder.take() {
                            builder.ast.root.tag_close = None;
                            if let Some(c) = builder.ast.root.content.as_mut() {
                                c.end = se.tag_open_end;
                            }
                            self.template_ast = Some(builder.finish());
                        }
                    }
                    RootNodeKind::Script => {
                        self.store_script_node(&se, None, content, ctx);
                    }
                    RootNodeKind::Style => {
                        self.store_style_node(&se, None, content, ctx);
                    }
                    RootNodeKind::Unknown => {
                        self.store_unknown_node(&se, None, content);
                    }
                }
            }
        }

        // Finalize the template AST if the builder is still active
        // (normal case: all elements were properly closed).
        if let Some(mut builder) = self.ast_builder.take() {
            // In template_mode, update the synthetic root content end to the actual input length.
            if self.template_mode {
                if let Some(c) = builder.ast.root.content.as_mut() {
                    c.end = ctx.bytes.len() as u32;
                }
            }
            self.template_ast = Some(builder.finish());
        }
    }
}

// root node storage

impl Syntax {
    /// Called when an SFC root element's open tag is complete.
    ///
    /// For `<template>`, creates a `TemplateAstBuilder` so subsequent
    /// children are parsed into the template AST. For other root kinds,
    /// defers storage until `CloseTag` / `SelfClosingTag` (since the
    /// content span isn't known yet).
    fn handle_root_open<'alloc>(
        &mut self,
        se: &StackElement,
        name: &[u8],
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        match resolve_root_kind(name) {
            RootNodeKind::Template => {
                let root = RootNodeTemplate {
                    tag_open: make_open_tag(se),
                    tag_close: None,
                    lang: self.prop_lang.take(),
                    attributes: self.take_props(),
                    content: Some(RootNodeTemplateContent {
                        start: se.tag_open_end,
                        end: se.tag_open_end,
                        children: SmallVec::new(),
                    }),
                };
                self.ast_builder = Some(TemplateAstBuilder::new(root));
                // Props consumed into the template root — safe to reset now.
                self.reset_prop_state();
            }
            RootNodeKind::Script | RootNodeKind::Style | RootNodeKind::Unknown => {
                // Do NOT reset here — root props (lang, setup, src, scoped, module)
                // are needed later when store_*_node is called on CloseTag/SelfClosingTag.
            }
        }
    }

    /// Store a parsed `<script>` (or `<script setup>`) root node.
    /// Emits `DuplicateScript` / `DuplicateScriptSetup` if one already exists.
    fn store_script_node<'alloc>(
        &mut self,
        se: &StackElement,
        tag_close: Option<NodeTag>,
        content: Option<Span>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let node = RootNodeScript {
            tag_open: make_open_tag(se),
            tag_close,
            is_setup: self.prop_setup,
            src: self.prop_src.take(),
            generic: self.prop_generic.take(),
            lang: self.prop_lang.take().map(|lang| {
                ScriptLanguage::from_bytes(&ctx.bytes[lang.start as usize..lang.end as usize])
            }),
            attributes: self.take_props(),
            content,
        };

        if self.prop_setup {
            if self.script_setup_node.is_some() {
                self.diagnostics.push(
                    Diagnostic::error("syntax", CompilerErrorCode::DuplicateScriptSetup)
                        .with_span(Span::new(se.tag_open_start, se.tag_open_end)),
                );
            }
            self.script_setup_node = Some(node);
        } else {
            if self.script_node.is_some() {
                self.diagnostics.push(
                    Diagnostic::error("syntax", CompilerErrorCode::DuplicateScript)
                        .with_span(Span::new(se.tag_open_start, se.tag_open_end)),
                );
            }
            self.script_node = Some(node);
        }

        self.reset_prop_state();
    }

    /// Store a parsed `<style>` root node. Multiple style blocks are allowed.
    fn store_style_node<'alloc>(
        &mut self,
        se: &StackElement,
        tag_close: Option<NodeTag>,
        content: Option<Span>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        if self.prop_scoped {
            self.has_style_scope = true;
        }
        if self.prop_module {
            self.has_style_module = true;
        }

        let node = RootNodeStyle {
            tag_open: make_open_tag(se),
            tag_close,
            lang: self.prop_lang.take().map(|lang| {
                StyleLang::from_bytes(&ctx.bytes[lang.start as usize..lang.end as usize])
            }),
            scoped: self.prop_scoped,
            module: self.prop_module,
            attributes: self.take_props(),
            content,
        };

        self.style_nodes.push(node);

        self.reset_prop_state();
    }

    fn store_unknown_node(
        &mut self,
        se: &StackElement,
        tag_close: Option<NodeTag>,
        content: Option<Span>,
    ) {
        let node = RootNodeUnknown {
            tag_open: make_open_tag(se),
            tag_close,
            attributes: self.take_props(),
            content,
        };

        self.unknown_nodes.push(node);

        self.reset_prop_state();
    }

    fn reset_prop_state(&mut self) {
        self.prop_lang = None;
        self.prop_src = None;
        self.prop_generic = None;
        self.prop_setup = false;
        self.prop_scoped = false;
        self.prop_module = false;
    }
}

// attribute handling

impl Syntax {
    fn handle_attribute_name(&mut self, start: u32, name_end: u32) {
        self.current_prop = Some(NodeProp {
            start,
            name_end,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            value_end: None,
            modifiers: SmallVec::new(),
        });
    }

    fn handle_directive_name(&mut self, start: u32, name_end: u32) {
        self.current_prop = Some(NodeProp {
            start,
            name_end,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            value_end: None,
            modifiers: SmallVec::new(),
        });
    }

    fn handle_directive_arg(&mut self, arg_start: u32, arg_end: u32, is_dynamic: bool) {
        if let Some(prop) = &mut self.current_prop {
            prop.arg_start = Some(arg_start);
            prop.arg_end = Some(arg_end);
            prop.is_dynamic = Some(is_dynamic);
        }
    }

    fn handle_directive_modifier(&mut self, modifier_start: u32, modifier_end: u32) {
        if let Some(prop) = &mut self.current_prop {
            prop.modifiers.push(Span::new(modifier_start, modifier_end));
        }
    }

    fn handle_attribute_value(&mut self, value_start: u32) {
        if let Some(prop) = &mut self.current_prop {
            prop.value_start = Some(value_start);
        }
    }

    /// `AttribEnd` — finalize the current attribute/directive.
    ///
    /// Computes `value_end` from the quote type, detects special root-level
    /// attributes (`lang`, `setup`, `src`, `scoped`, `module`, `vapor`),
    /// classifies directives into prop flags and cached directive fields
    /// (v-if/v-for/v-slot/v-once), and routes the prop to either the AST
    /// builder (template content) or `element_props` (SFC root).
    fn handle_attribute_end<'alloc>(
        &mut self,
        end: u32,
        quote: QuoteType,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let Some(mut prop) = self.current_prop.take() else {
            return;
        };

        // Compute value_end for all props (needed by codegen for expression spans).
        if let Some(vs) = prop.value_start {
            prop.value_end = Some(match quote {
                QuoteType::Single | QuoteType::Double => {
                    if end > vs {
                        end - 1
                    } else {
                        vs
                    }
                }
                QuoteType::Unquoted => end,
                QuoteType::NoValue => vs,
            });
        }

        // Detect special root-level attributes (SFC mode only).
        // Attributes arrive before OpenTagEnd, so ast_builder may not exist yet
        // for <template>. We check stack depth, not builder presence.
        let is_root_tag = !self.template_mode && self.stack_elements.len() == 1;
        if is_root_tag {
            let attr_name = &ctx.bytes[prop.start as usize..prop.name_end as usize];
            // Reuse value_end already computed above (same quote-type logic).
            let value_span = prop
                .value_start
                .zip(prop.value_end)
                .map(|(vs, ve)| Span::new(vs, ve));

            let root_name = self
                .stack_elements
                .last()
                .expect("invariant: stack non-empty when is_root_tag")
                .name_bytes(ctx);
            let root_kind = resolve_root_kind(root_name);

            match attr_name {
                // lang applies to all root node kinds
                b"lang" => self.prop_lang = value_span,
                // script-only
                b"setup" if root_kind == RootNodeKind::Script => self.prop_setup = true,
                b"src" if root_kind == RootNodeKind::Script => self.prop_src = value_span,
                b"generic" if root_kind == RootNodeKind::Script => self.prop_generic = value_span,
                // style-only
                b"scoped" if root_kind == RootNodeKind::Style => self.prop_scoped = true,
                b"module" if root_kind == RootNodeKind::Style => self.prop_module = true,
                // template-only
                b"vapor" if root_kind == RootNodeKind::Template => self.is_vapor = true,
                _ => {}
            }
        }

        // Route the prop to either the builder or the root element_props.
        // Only push to element_props when we're at root depth (SFC root tag).
        // This prevents nested element attributes from leaking into root node attributes.
        if let Some(builder) = self.ast_builder.as_mut() {
            // Classify built-in directives from bytes.
            // First occurrence wins; duplicates emit a warning diagnostic.
            // Cached directives are moved into the cache field and NOT added to props.
            let mut prop = Some(prop);
            // SAFETY: `prop` is `Some` here and `.take()` is called at most once per match arm
            if prop.as_ref().expect("invariant: prop is Some").is_directive {
                let p = prop.as_ref().expect("invariant: prop is Some");
                let dir_name = &ctx.bytes[p.start as usize..p.name_end as usize];
                let prop_start = p.start;
                let prop_name_end = p.name_end;
                // Helper: emit duplicate directive warning if setter returns true.
                let mut warn_if_dup = |is_dup: bool| {
                    if is_dup {
                        self.diagnostics.push(
                            Diagnostic::warning("syntax", CompilerErrorCode::XDuplicateDirective)
                                .with_span(Span::new(prop_start, prop_name_end)),
                        );
                    }
                };

                match dir_name {
                    b"v-if" | b"v-else-if" | b"v-else" => {
                        let kind = match dir_name {
                            b"v-if" => ElementNodeConditionKind::If,
                            b"v-else-if" => ElementNodeConditionKind::ElseIf,
                            _ => ElementNodeConditionKind::Else,
                        };
                        let cond = ElementNodeCondition {
                            kind,
                            prop: prop
                                .take()
                                .expect("invariant: prop not yet taken in v-if branch"),
                        };
                        warn_if_dup(builder.set_v_condition(cond));
                    }
                    b"v-for" => {
                        warn_if_dup(
                            builder.set_v_for(
                                prop.take()
                                    .expect("invariant: prop not yet taken in v-for branch"),
                            ),
                        );
                    }
                    b"v-slot" | b"#" => {
                        warn_if_dup(
                            builder.set_v_slot(
                                prop.take()
                                    .expect("invariant: prop not yet taken in v-slot branch"),
                            ),
                        );
                    }
                    b"v-once" => {
                        warn_if_dup(
                            builder.set_v_once(
                                prop.take()
                                    .expect("invariant: prop not yet taken in v-once branch"),
                            ),
                        );
                    }
                    // v-bind / : shorthand — classify arg for key/class/style, or spread
                    b"v-bind" | b":" => {
                        let p = prop
                            .as_ref()
                            .expect("invariant: prop not yet taken in v-bind branch");
                        if let (Some(arg_s), Some(arg_e)) = (p.arg_start, p.arg_end) {
                            let arg = &ctx.bytes[arg_s as usize..arg_e as usize];
                            match arg {
                                b"key" => builder.add_prop_flag(PropFlags::HasDynamicKey),
                                b"class" => builder.add_prop_flag(PropFlags::HasDynamicClass),
                                b"style" => builder.add_prop_flag(PropFlags::HasDynamicStyle),
                                _ => builder.add_prop_flag(PropFlags::HasDynamicBinding),
                            }
                        } else {
                            // v-bind with no arg = spread
                            builder.add_prop_flag(PropFlags::HasBindSpread);
                        }
                    }
                    // v-on / @ shorthand — event listener or spread
                    b"v-on" | b"@" => {
                        let p = prop
                            .as_ref()
                            .expect("invariant: prop not yet taken in v-on branch");
                        if p.arg_start.is_some() {
                            builder.add_prop_flag(PropFlags::HasEventListener);
                        } else {
                            // v-on with no arg = spread
                            builder.add_prop_flag(PropFlags::HasOnSpread);
                        }
                    }
                    // v-model
                    b"v-model" => {
                        builder.add_prop_flag(PropFlags::HasModel);
                    }
                    // v-show
                    b"v-show" => {
                        builder.add_prop_flag(PropFlags::HasShow);
                    }
                    // v-html
                    b"v-html" => {
                        builder.add_prop_flag(PropFlags::HasVHtml);
                    }
                    // v-text
                    b"v-text" => {
                        builder.add_prop_flag(PropFlags::HasVText);
                    }
                    // Built-in directives that don't set any prop flag
                    b"v-pre" | b"v-cloak" | b"v-memo" => {}
                    // Any other v-* directive is a custom directive
                    _ => {
                        builder.add_prop_flag(PropFlags::HasCustomDirective);
                    }
                }
            } else {
                // Non-directive attribute — check for ref, class, style
                let p = prop
                    .as_ref()
                    .expect("invariant: prop is Some in non-directive branch");
                let attr_name = &ctx.bytes[p.start as usize..p.name_end as usize];
                match attr_name {
                    b"ref" => {
                        builder.add_prop_flag(PropFlags::HasRef);
                        builder.set_v_ref(
                            prop.take()
                                .expect("invariant: prop not yet taken in ref branch"),
                        );
                    }
                    b"class" => builder.add_prop_flag(PropFlags::HasStaticClass),
                    b"style" => builder.add_prop_flag(PropFlags::HasStaticStyle),
                    _ => {}
                }
            }
            if let Some(prop) = prop {
                builder.push_prop_to_current(prop);
            }
        } else if is_root_tag {
            self.element_props.push(prop);
        }
        // else: non-root attribute outside of template builder — drop it.
        // This can happen for attributes on nested elements inside non-template
        // root nodes (e.g. <custom><x a="1"></x></custom>). These are part of
        // raw content and should not be attached to the root node's attributes.
    }
}

// leaf handling

impl Syntax {
    fn handle_text_leaf(&mut self, start: u32, end: u32, is_entity: bool) {
        if let Some(b) = self.ast_builder.as_mut() {
            b.add_text(start, end, is_entity);
        }
    }

    fn handle_comment_leaf(&mut self, start: u32, end: u32, content_start: u32, content_end: u32) {
        if let Some(b) = self.ast_builder.as_mut() {
            b.add_comment(start, end, content_start, content_end);
        }
    }

    fn handle_interpolation_leaf(
        &mut self,
        start: u32,
        end: u32,
        inner_start: u32,
        inner_end: u32,
    ) {
        if let Some(b) = self.ast_builder.as_mut() {
            b.add_interpolation(start, end, inner_start, inner_end);
        }
    }
}

// v-else / v-else-if adjacency validation

impl Syntax {
    /// Validate that a v-else or v-else-if element has an adjacent v-if/v-else-if sibling.
    /// Called after close_element attaches the node to its parent.
    /// Walks prev_sibling, skipping comments and whitespace-only text nodes.
    fn validate_v_condition_adjacency<'alloc>(
        &mut self,
        id: NodeId,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let builder = match self.ast_builder.as_ref() {
            Some(b) => b,
            None => return,
        };
        let ast = &builder.ast;
        let node = &ast.nodes[id.0];
        let AstNodeKind::Element(el) = &node.kind else {
            return;
        };
        let cond = match &el.v_condition {
            Some(c) => c,
            None => return,
        };
        // Only v-else-if and v-else need adjacency validation
        if matches!(cond.kind, ElementNodeConditionKind::If) {
            return;
        }

        let tag_span = Span::new(el.tag_open.start, el.tag_open.name_end);

        // Walk backwards through siblings
        let mut prev = ast.prev_sibling(id);
        while let Some(prev_id) = prev {
            let prev_node = &ast.nodes[prev_id.0];
            match &prev_node.kind {
                AstNodeKind::Comment(_) => {
                    // Skip comments between branches
                    prev = ast.prev_sibling(prev_id);
                }
                AstNodeKind::Text(t) => {
                    // Skip whitespace-only text nodes
                    let text_bytes = &ctx.bytes[t.start as usize..t.end as usize];
                    if text_bytes.iter().all(|b| b.is_ascii_whitespace()) {
                        prev = ast.prev_sibling(prev_id);
                    } else {
                        // Non-whitespace text before v-else — invalid
                        self.diagnostics.push(
                            Diagnostic::error("syntax", CompilerErrorCode::XVElseNoAdjacentIf)
                                .with_span(tag_span),
                        );
                        return;
                    }
                }
                AstNodeKind::Element(prev_el) => {
                    if let Some(prev_cond) = &prev_el.v_condition {
                        if matches!(
                            prev_cond.kind,
                            ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf
                        ) {
                            // Valid: previous sibling has v-if or v-else-if
                            return;
                        }
                    }
                    // Previous element has no v-condition or has v-else — invalid
                    self.diagnostics.push(
                        Diagnostic::error("syntax", CompilerErrorCode::XVElseNoAdjacentIf)
                            .with_span(tag_span),
                    );
                    return;
                }
                AstNodeKind::Interpolation(_) => {
                    // Interpolation before v-else — invalid
                    self.diagnostics.push(
                        Diagnostic::error("syntax", CompilerErrorCode::XVElseNoAdjacentIf)
                            .with_span(tag_span),
                    );
                    return;
                }
            }
        }

        // No previous sibling at all → error
        self.diagnostics.push(
            Diagnostic::error("syntax", CompilerErrorCode::XVElseNoAdjacentIf).with_span(tag_span),
        );
    }
}

// utilities

impl Syntax {
    /// Drain element_props into a new Vec, preserving the original's capacity
    /// so subsequent root nodes still benefit from the pre-allocation.
    ///
    /// We intentionally use `drain(..).collect()` instead of `std::mem::take`
    /// because `take` would reset the Vec capacity to 0, forcing a re-allocation
    /// on the next root node. With `drain`, the original Vec keeps its capacity
    /// (pre-allocated with 20) and is ready for the next root's attributes.
    ///
    /// TODO(new_impl): consider switching to `SmallVec<[NodeProp; 8]>` for
    /// element_props — most SFC root tags have few attributes (<script setup>,
    /// <style scoped lang="scss">) and a SmallVec would avoid the heap
    /// allocation entirely for typical cases.
    #[inline]
    fn take_props(&mut self) -> Vec<NodeProp> {
        self.element_props.drain(..).collect()
    }
}
