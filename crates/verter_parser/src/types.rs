//! Shared low-level types for the AST-based parser.
//!
//! These types are used across both the `ast` and `syntax` modules — they
//! represent source-position spans (`NodeTag`), arena indices (`NodeId`),
//! and parsed attribute/directive metadata (`NodeProp`).

use crate::common::Span;
use smallvec::SmallVec;

/// Source-position span for an HTML tag (open or close).
///
/// Covers the full tag delimiter including angle brackets.
/// For `<div class="x">`:  start = 0 (`<`), name_end = 4 (`div`), end = 17 (`>`).
/// For `</div>`:           start = 0 (`<`), name_end = 5 (`div`), end = 6 (`>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeTag {
    /// Byte offset of the opening delimiter (`<` for open tags, `<` for close tags).
    pub start: u32,
    /// Byte offset past the closing delimiter (`>` or `/>` end).
    pub end: u32,
    /// Byte offset past the tag name (before attributes/whitespace).
    pub name_end: u32,
}

/// Index into a `TemplateAst.nodes` arena.
///
/// Lightweight handle — Copy, 8 bytes on 64-bit. Use `.0` to index into the
/// arena `Vec<AstNode>` directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub usize);

/// Parsed attribute or directive on an element.
///
/// Represents both plain attributes (`class="foo"`) and directives
/// (`v-if="show"`, `@click.stop="handler"`, `:key="id"`). All positions
/// are byte offsets into the original source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeProp {
    /// Start position of the attribute/directive name
    pub start: u32,
    /// End position of the name
    pub name_end: u32,
    /// Whether this is a directive (vs a regular attribute)
    pub is_directive: bool,
    /// Directive argument start position (if any)
    pub arg_start: Option<u32>,
    /// Directive argument end position (if any)
    pub arg_end: Option<u32>,
    /// Start position of the value (after the opening quote)
    pub value_start: Option<u32>,
    /// End position of the value (before the closing quote)
    pub value_end: Option<u32>,
    /// Directive modifiers (e.g., `.prevent`, `.stop`). Inline for up to 2
    /// modifiers (covers the vast majority of real-world usage) to avoid
    /// a heap allocation per attribute.
    pub modifiers: SmallVec<[Span; 2]>,
    /// Whether the directive argument is dynamic (e.g., :[arg])
    pub is_dynamic: Option<bool>,
}

impl NodeProp {
    /// Returns `true` if this attribute is valid on an SFC root node.
    ///
    /// Root nodes (`<script>`, `<style>`, `<template>`) only accept plain
    /// attributes — directives like `v-if` on a root node are invalid.
    pub fn is_valid_root(&self) -> bool {
        !self.is_directive
    }
}

// ── Binding classification ─────────────────────────────────────────────────

/// Classification of a binding for correct accessor prefix/suffix in template codegen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingType {
    SetupConst,
    SetupLet,
    SetupRef,
    SetupReactiveConst,
    SetupMaybeRef,
    LiteralConst,
    Props,
    PropsAliased,
    /// A real local introduced by destructuring defineProps/withDefaults.
    /// Runtime codegen still treats it as a prop; IDE TSX resolves it bare so
    /// the preserved source binding carries template hovers and liveness.
    PropsDestructured,
    SetupImport,
    Data,
    Options,
}

impl BindingType {
    #[inline]
    pub fn reactivity_level(&self) -> ReactivityLevel {
        match self {
            BindingType::SetupConst | BindingType::SetupImport | BindingType::LiteralConst => {
                ReactivityLevel::Static
            }
            _ => ReactivityLevel::Dynamic,
        }
    }

    #[inline]
    pub fn is_setup(&self) -> bool {
        matches!(
            self,
            BindingType::SetupConst
                | BindingType::SetupLet
                | BindingType::SetupRef
                | BindingType::SetupReactiveConst
                | BindingType::SetupMaybeRef
                | BindingType::SetupImport
                | BindingType::LiteralConst
        )
    }

    #[inline]
    pub fn is_props(&self) -> bool {
        matches!(
            self,
            BindingType::Props | BindingType::PropsAliased | BindingType::PropsDestructured
        )
    }

    #[inline]
    pub fn needs_value_access(&self) -> bool {
        matches!(self, BindingType::SetupRef | BindingType::SetupMaybeRef)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactivityLevel {
    Static,
    Dynamic,
}
