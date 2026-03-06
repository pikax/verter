//! Root-level SFC block types.
//!
//! Each Vue SFC has up to four kinds of root blocks:
//! - `<script>` / `<script setup>` → [`RootNodeScript`]
//! - `<style>` (multiple allowed) → [`RootNodeStyle`]
//! - `<template>` → [`RootNodeTemplate`] (with children in [`RootNodeTemplateContent`])
//! - Custom blocks (e.g., `<i18n>`, `<docs>`) → [`RootNodeUnknown`]
//!
//! These types store the tag positions, special attributes (lang, scoped,
//! setup, etc.), and the raw content span between open and close tags.

use smallvec::SmallVec;

use crate::{
    ast::types::TemplateAst,
    common::Span,
    cursor::ScriptLanguage,
    diagnostics::Diagnostic,
    types::{NodeId, NodeProp, NodeTag},
};

/// Discriminant for SFC root block kind, resolved from the tag name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootNodeKind {
    Script,
    Template,
    Style,
    /// Any tag that isn't `script`, `template`, or `style` (e.g., `<i18n>`).
    Unknown,
}

/// Style preprocessor language, parsed from `<style lang="...">`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleLang {
    Css = 0,
    Scss,
    Sass,
    Less,
    Stylus,
    Unknown,
}

/// A parsed `<script>` or `<script setup>` SFC root block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootNodeScript {
    /// Tag span for `<script ...>`.
    pub tag_open: NodeTag,
    /// Tag span for `</script>`, or `None` if self-closing / unclosed.
    pub tag_close: Option<NodeTag>,

    /// Whether the `setup` attribute was present.
    pub is_setup: bool,
    /// Parsed `lang` attribute (e.g., `ts`, `tsx`).
    pub lang: Option<ScriptLanguage>,
    /// `src="..."` attribute value span (external script source).
    pub src: Option<Span>,
    /// `generic="..."` attribute value span (generic type parameters for `<script setup>`).
    pub generic: Option<Span>,
    /// `attrs="..."` or `attributes="..."` attribute value span (typed `$attrs`).
    pub attrs: Option<Span>,

    /// All attributes on the tag (including already-parsed special ones).
    pub attributes: Vec<NodeProp>,

    /// Raw content span between open and close tags. `None` if self-closing.
    pub content: Option<Span>,
}

/// A parsed `<style>` SFC root block.
///
/// Multiple `<style>` blocks are allowed in a single SFC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootNodeStyle {
    pub tag_open: NodeTag,
    pub tag_close: Option<NodeTag>,

    /// Parsed `lang` attribute (e.g., `scss`, `less`).
    pub lang: Option<StyleLang>,
    /// Whether the `scoped` attribute was present.
    pub scoped: bool,
    /// Whether the `module` attribute was present.
    pub module: bool,

    pub attributes: Vec<NodeProp>,

    /// Raw content span between open and close tags. `None` if self-closing.
    pub content: Option<Span>,
}

/// A parsed custom/unknown SFC root block (e.g., `<i18n>`, `<docs>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootNodeUnknown {
    pub tag_open: NodeTag,
    pub tag_close: Option<NodeTag>,

    pub attributes: Vec<NodeProp>,

    pub content: Option<Span>,
}

/// A parsed `<template>` SFC root block.
///
/// Unlike other root nodes, the template's children are parsed into a full
/// AST (via [`super::super::ast::builder::TemplateAstBuilder`]) and stored
/// in [`RootNodeTemplateContent::children`] as [`NodeId`] references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootNodeTemplate {
    pub tag_open: NodeTag,
    pub tag_close: Option<NodeTag>,

    /// Raw `lang` attribute value span (e.g., `pug`). Not parsed into an
    /// enum since template languages are open-ended.
    pub lang: Option<Span>,

    pub attributes: Vec<NodeProp>,

    /// Template content region with parsed child node IDs. `None` if the
    /// template is self-closing or the content region was never opened.
    pub content: Option<RootNodeTemplateContent>,
}

/// The content region of a `<template>` root block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootNodeTemplateContent {
    /// Byte offset immediately after `<template ...>` closing `>`.
    pub start: u32,
    /// Byte offset of `</template>` opening `<` (or EOF).
    pub end: u32,
    /// Root-level children of the template (arena node IDs).
    pub children: SmallVec<[NodeId; 4]>,
}

/// Finalized SFC parse result, suitable for caching and borrowing during codegen.
///
/// Produced by [`super::Syntax::into_parsed_sfc()`] after tokenization completes.
/// All fields are owned; consumers borrow via accessor methods. Stored behind
/// `Arc` in the host for zero-copy reuse across compile profiles.
#[derive(Debug, Clone)]
pub struct ParsedSfc {
    pub template_ast: Option<TemplateAst>,
    pub script_node: Option<RootNodeScript>,
    pub script_setup_node: Option<RootNodeScript>,
    pub style_nodes: Vec<RootNodeStyle>,
    pub unknown_nodes: Vec<RootNodeUnknown>,
    pub has_style_scope: bool,
    pub has_style_module: bool,
    pub is_vapor: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub has_errors: bool,
}

impl ParsedSfc {
    pub fn script(&self) -> Option<&RootNodeScript> {
        self.script_node.as_ref().filter(|s| !s.is_setup)
    }

    pub fn script_setup(&self) -> Option<&RootNodeScript> {
        self.script_setup_node.as_ref().filter(|s| s.is_setup)
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

    pub fn has_errors(&self) -> bool {
        self.has_errors
    }

    pub fn has_style_scope(&self) -> bool {
        self.has_style_scope
    }

    pub fn is_vapor(&self) -> bool {
        self.is_vapor
    }

    /// Clone diagnostics out for ownership transfer to compile results.
    /// This is the only clone needed — all other accesses are borrows.
    pub fn clone_diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics.clone()
    }
}

impl StyleLang {
    /// Parse a style language from the raw `lang` attribute bytes.
    pub fn from_bytes(lang: &[u8]) -> Self {
        match lang {
            b"css" => StyleLang::Css,
            b"scss" => StyleLang::Scss,
            b"sass" => StyleLang::Sass,
            b"less" => StyleLang::Less,
            b"stylus" | b"styl" => StyleLang::Stylus,
            _ => StyleLang::Unknown,
        }
    }
}
